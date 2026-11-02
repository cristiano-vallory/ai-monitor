use crate::model::{ApiSnapshot, Config, Handoff, Project, Session};
use chrono::{DateTime, Utc};
use reqwest::{Url, blocking::Client};
use serde::de::DeserializeOwned;
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub fn endpoint(base: &str, segments: &[&str]) -> Result<Url, String> {
    let mut url = Url::parse(base).map_err(|_| "URL inválida.")?;
    url.path_segments_mut()
        .map_err(|_| "URL sem suporte a caminhos.")?
        .pop_if_empty()
        .extend(segments);
    Ok(url)
}

fn get<T: DeserializeOwned>(client: &Client, config: &Config, url: Url) -> Result<T, String> {
    let mut request = client.get(url).header("Cache-Control", "no-cache");
    if !config.token.is_empty() {
        request = request.bearer_auth(&config.token);
    }
    let response = request
        .send()
        .map_err(|_| "Servidor indisponível ou tempo limite de 8 s.".to_owned())?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 | 403 => "Acesso recusado. Confira o token e suas permissões.".into(),
            404 => "Rota não encontrada. Confira a URL e a versão do ai-memory.".into(),
            code => format!("Servidor respondeu HTTP {code}."),
        });
    }
    let mut bytes = Vec::new();
    response
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Falha ao ler a resposta.".to_owned())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Resposta excedeu 8 MiB.".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Formato da API incompatível.".into())
}

#[derive(serde::Deserialize)]
struct SessionList {
    sessions: Vec<Session>,
}
#[derive(serde::Deserialize)]
struct HandoffList {
    handoffs: Vec<Handoff>,
}

pub fn collect(config: &Config, stop: &Arc<AtomicBool>) -> ApiSnapshot {
    let mut result = ApiSnapshot::default();
    let client = match Client::builder()
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            result.error = Some("Falha ao iniciar cliente HTTP.".into());
            return result;
        }
    };
    let projects = endpoint(&config.endpoint, &["api", "v1", "projects"])
        .and_then(|url| get::<Vec<Project>>(&client, config, url));
    result.projects = match projects {
        Ok(p) => p,
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    };
    result.version = endpoint(&config.endpoint, &["identity"])
        .and_then(|url| get::<serde_json::Value>(&client, config, url))
        .ok()
        .and_then(|v| v["version"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let deadline = Instant::now() + Duration::from_secs(30);
    let chunks = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|worker| {
                let projects = &result.projects;
                let client = &client;
                scope.spawn(move || {
                    let mut sessions = Vec::new();
                    let mut handoffs = Vec::new();
                    let mut errors = Vec::new();
                    let mut handoff_total=Some(0usize);
                    let mut warnings=Vec::new();
                    for project in projects.iter().skip(worker).step_by(4) {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        if Instant::now() > deadline {
                            errors.push("Coleta parcial: limite de 30 s atingido.".into());
                            handoff_total=None;
                            break;
                        }
                        let name = format!("{} / {}", project.workspace_name, project.project_name);
                        let base = [
                            "api",
                            "v1",
                            "workspaces",
                            &project.workspace_name,
                            "projects",
                            &project.project_name,
                        ];
                        let mut path = base.to_vec();
                        path.push("handoffs");
                        let hs = endpoint(&config.endpoint, &path).and_then(|mut u| {
                            u.query_pairs_mut()
                                .append_pair("state", "open")
                                .append_pair("limit", "200");
                            get::<HandoffList>(client, config, u)
                        });
                        match hs {
                            Ok(list) => {
                                let count=if list.handoffs.len()==200 {
                                    warnings.push(format!("{name}: lista limitada aos 200 handoffs mais recentes; total consultado separadamente."));
                                    let mut count_path=base.to_vec();count_path.push("briefing");
                                    match endpoint(&config.endpoint,&count_path).and_then(|mut u| {
                                        u.query_pairs_mut().append_pair("limit","1");
                                        get::<serde_json::Value>(client,config,u)
                                    }).and_then(|v|v["pending_handoff_count"].as_u64().map(|n|n as usize).ok_or("Total de handoffs ausente na API.".into())) {
                                        Ok(n)=>Some(n),
                                        Err(e)=>{errors.push(format!("{name}: {e}"));None}
                                    }
                                } else {Some(list.handoffs.iter().filter(|h|h.state=="open").count())};
                                handoff_total=handoff_total.zip(count).map(|(a,b)|a+b);
                                handoffs.extend(
                                    list.handoffs.into_iter().filter(|h| h.state == "open").map(
                                        |mut h| {
                                            h.scope = name.clone();
                                            h
                                        },
                                    ),
                                );
                            }
                            Err(e) => {errors.push(format!("{name}: {e}"));handoff_total=None;},
                        }
                        if project.session_count == Some(0) {
                            continue;
                        }
                        path.pop();
                        path.push("sessions");
                        if Instant::now()>deadline {
                            errors.push(format!("{name}: sessões não coletadas nesta rodada."));
                            continue;
                        }
                        let ss=endpoint(&config.endpoint,&path).and_then(|mut u| {
                            u.query_pairs_mut().append_pair("include_open","true").append_pair("limit","100").append_pair("offset","0");
                            get::<SessionList>(client,config,u)
                        });
                        match ss {
                            Ok(list)=>sessions.extend(list.sessions.into_iter().map(|mut s|{s.scope=name.clone();s})),
                            Err(e)=>errors.push(format!("{name}: {e}")),
                        }
                    }
                    (sessions, handoffs, errors, handoff_total, warnings)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join()).collect::<Vec<_>>()
    });
    result.handoff_total = Some(0);
    for chunk in chunks {
        match chunk {
            Ok((s, h, e, count, warnings)) => {
                result.handoff_total = result.handoff_total.zip(count).map(|(a, b)| a + b);
                result.warnings.extend(warnings);
                result.sessions.extend(s);
                result.handoffs.extend(h);
                result.errors.extend(e);
            }
            Err(_) => {
                result.errors.push("Um coletor foi interrompido.".into());
                result.handoff_total = None;
            }
        }
    }
    // Compare instants, not RFC3339 strings with varying fractional precision.
    result
        .sessions
        .sort_by_key(|s| std::cmp::Reverse(DateTime::parse_from_rfc3339(&s.started_at).ok()));
    result
        .sessions
        .dedup_by(|a, b| a.scope == b.scope && a.session_id == b.session_id);
    result
        .handoffs
        .sort_by_key(|h| std::cmp::Reverse(DateTime::parse_from_rfc3339(&h.at).ok()));
    result.at = Some(Utc::now());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{io::Write, net::TcpListener, sync::Mutex, thread};

    struct Mock {
        url: String,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
        requests: Arc<Mutex<Vec<String>>>,
    }
    impl Mock {
        fn new(handler: impl Fn(&str) -> (u16, serde_json::Value) + Send + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = requests.clone();
            let thread = thread::spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut req = Vec::new();
                    let mut byte = [0u8; 1];
                    while !req.ends_with(b"\r\n\r\n") && req.len() < 8192 {
                        if stream.read(&mut byte).unwrap_or(0) == 0 {
                            break;
                        }
                        req.push(byte[0]);
                    }
                    let req = String::from_utf8(req).unwrap();
                    captured.lock().unwrap().push(req.clone());
                    let path = req.split_whitespace().nth(1).unwrap_or("");
                    let (status, body) = handler(path);
                    let body = body.to_string();
                    write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                }
            });
            Self {
                url,
                stop,
                thread: Some(thread),
                requests,
            }
        }
    }
    impl Drop for Mock {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    #[test]
    fn bounds_session_history_and_only_reads_open_handoffs() {
        let server = Mock::new(|path| {
            if path == "/identity" {
                return (200, json!({"version":"3.3.0"}));
            }
            if path == "/api/v1/projects" {
                return (
                    200,
                    json!([
                        {"workspace_name":"default","project_name":"a","session_count":101},
                        {"workspace_name":"default","project_name":"b","session_count":1}
                    ]),
                );
            }
            if path.contains("/handoffs?") {
                return (
                    200,
                    json!({"handoffs":[{"id":path,"agent":"codex","at":"2026-01-01T00:00:00Z","state":"open"}]}),
                );
            }
            let session = |id: String, ended: bool| json!({"session_id":id,"agent_kind":"codex","started_at":"2026-01-01T00:00:00Z","ended_at":if ended{Some("2026-01-01T01:00:00Z")}else{None},"observation_count":3});
            if path.contains("/a/sessions") && path.contains("offset=0") {
                return (
                    200,
                    json!({"sessions":(0..100).map(|i|session(format!("closed-{i}"),true)).collect::<Vec<_>>()}),
                );
            }
            (
                200,
                json!({"sessions":[session("shared-open".into(),false)]}),
            )
        });
        let config = Config {
            endpoint: server.url.clone(),
            token: "test-token".into(),
            ..Default::default()
        };
        let result = collect(&config, &Arc::new(AtomicBool::new(false)));
        assert!(result.error.is_none());
        assert!(result.errors.is_empty());
        assert_eq!(result.sessions.len(), 101);
        assert_eq!(result.handoff_total, Some(2));
        assert_eq!(result.open_sessions(), 1);
        assert_eq!(result.handoffs.len(), 2);
        let requests = server.requests.lock().unwrap();
        assert!(requests.iter().all(|r| {
            r.starts_with("GET ")
                && r.to_lowercase()
                    .contains("authorization: bearer test-token")
        }));
        assert!(!requests.iter().any(|r| r.contains("offset=100")));
        assert!(
            requests
                .iter()
                .filter(|r| r.contains("/handoffs?"))
                .all(|r| r.contains("state=open"))
        );
    }

    #[test]
    fn full_handoff_page_uses_the_server_total_without_fetching_more_history() {
        let server = Mock::new(|path| {
            if path == "/api/v1/projects" {
                return (
                    200,
                    json!([{"workspace_name":"default","project_name":"a","session_count":0,"open_session_count":0}]),
                );
            }
            if path == "/identity" {
                return (200, json!({"version":"3.3.0"}));
            }
            if path.contains("/briefing?") {
                return (200, json!({"pending_handoff_count":350}));
            }
            (
                200,
                json!({"handoffs":(0..200).map(|n|json!({"id":n.to_string(),"agent":"codex","at":"2026-01-01T00:00:00Z","state":"open"})).collect::<Vec<_>>()}),
            )
        });
        let result = collect(
            &Config {
                endpoint: server.url.clone(),
                ..Default::default()
            },
            &Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(result.handoffs.len(), 200);
        assert_eq!(result.handoff_total, Some(350));
        assert_eq!(result.open_session_links(), Some(0));
        assert!(result.errors.is_empty());
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn authentication_failures_do_not_look_like_a_successful_empty_snapshot() {
        let server = Mock::new(|_| (401, json!({"error":"private internal error"})));
        let result = collect(
            &Config {
                endpoint: server.url.clone(),
                ..Default::default()
            },
            &Arc::new(AtomicBool::new(false)),
        );
        assert!(result.at.is_none());
        assert!(result.error.as_ref().unwrap().contains("Acesso recusado"));
        assert!(!result.error.unwrap().contains("private"));
    }
    #[test]
    fn scope_names_cannot_change_route_or_query() {
        let u = endpoint(
            "http://127.0.0.1:49374/base/",
            &["api", "v1", "projects", "a/b ?#=", "sessions"],
        )
        .unwrap();
        assert_eq!(u.path(), "/base/api/v1/projects/a%2Fb%20%3F%23=/sessions");
        assert!(u.query().is_none());
    }
    #[test]
    fn bearer_is_not_saved_and_remote_http_is_rejected() {
        let c = Config {
            token: "very-secret".into(),
            ..Default::default()
        };
        assert!(!serde_json::to_string(&c).unwrap().contains("very-secret"));
        assert!(serde_json::to_value(&c).unwrap().get("token").is_none());
        let loaded: Config =
            serde_json::from_value(json!({"token":"synthetic-disk-token"})).unwrap();
        assert!(loaded.token.is_empty());
        assert!(
            Config {
                endpoint: "http://example.com".into(),
                ..c
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn credentials_cannot_be_embedded_in_server_urls() {
        for endpoint in [
            "https://user:synthetic-password@example.com",
            "https://user@example.com",
            "https://example.com?token=synthetic-token",
            "https://example.com#synthetic-token",
            "file:///tmp/ai-memory",
        ] {
            assert!(
                Config {
                    endpoint: endpoint.into(),
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        for endpoint in [
            "https://example.com",
            "http://127.0.0.1:49374",
            "http://[::1]:49374",
        ] {
            assert!(
                Config {
                    endpoint: endpoint.into(),
                    token: "synthetic-token".into(),
                    ..Default::default()
                }
                .validate()
                .is_ok()
            );
        }
    }
}
