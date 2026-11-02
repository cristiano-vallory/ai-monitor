//! Human sessions stay in memory and never silently fall back to Bearer.
use crate::{
    client::{Connection, Error, check_status, read_response},
    model::Config,
    monitor::Shared,
};
use reqwest::{Method, blocking::RequestBuilder, header::HeaderMap};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::Ordering,
    mpsc::{self, Receiver},
};

#[derive(Clone)]
pub struct Session {
    session: String,
    csrf: String,
}
impl Session {
    #[cfg(test)]
    pub(crate) fn for_test(session: &str, csrf: &str) -> Self {
        Self {
            session: session.into(),
            csrf: csrf.into(),
        }
    }
    pub fn headers(&self, request: RequestBuilder, method: &Method) -> RequestBuilder {
        let request = request.header(
            "Cookie",
            format!(
                "ai_memory_session={}; ai_memory_csrf={}",
                self.session, self.csrf
            ),
        );
        if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
            request
        } else {
            request.header("x-csrf-token", &self.csrf)
        }
    }
    fn from_headers(headers: &HeaderMap) -> Result<Self, Error> {
        let mut session = None;
        let mut csrf = None;
        for header in headers.get_all("set-cookie") {
            let text = header.to_str().map_err(|_| Error::Format)?;
            let Some((name, value)) = text.split(';').next().and_then(|p| p.split_once('=')) else {
                continue;
            };
            let name = name.trim();
            if !["ai_memory_session", "ai_memory_csrf"].contains(&name) {
                continue;
            }
            if value.is_empty()
                || value.len() > 4096
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_graphic() && !b";\"\\,".contains(&b))
            {
                return Err(Error::Format);
            }
            if name == "ai_memory_session" {
                session = Some(value.into());
            } else {
                csrf = Some(value.into());
            }
        }
        Ok(Self {
            session: session.ok_or(Error::Format)?,
            csrf: csrf.ok_or(Error::Format)?,
        })
    }
}
fn secure(config: &Config) -> Result<(), Error> {
    let mut check = config.clone();
    check.token = "transport-validation".into();
    check.validate().map_err(Error::Invalid)
}
fn login(config: &Config, username: &str, password: &str) -> Result<(Config, Value), Error> {
    secure(config)?;
    if username.trim().is_empty() || password.is_empty() {
        return Err(Error::Invalid("Informe usuário e senha.".into()));
    }
    let mut anonymous = config.clone();
    anonymous.token.clear();
    anonymous.human = None;
    let c = Connection::new(&anonymous)?;
    let response = c
        .request_builder(Method::POST, &["auth", "login"], &[])?
        .json(&json!({"username":username,"password":password}))
        .send()
        .map_err(|_| Error::Unavailable)?;
    check_status(&response)?;
    let session = Session::from_headers(response.headers())?;
    read_response(response)?;
    let mut connected = config.clone();
    connected.human = Some(session);
    // Confirm capability state using only the new human session.
    let me = Connection::new(&connected)?.get::<Value>(&["auth", "me"], &[])?;
    Ok((connected, me))
}
fn password(
    config: &Config,
    current: &str,
    new: &str,
    confirmation: &str,
) -> Result<Config, Error> {
    secure(config)?;
    if config.human.is_none() {
        return Err(Error::Authentication);
    }
    if current.is_empty() || new.is_empty() || new != confirmation {
        return Err(Error::Invalid(
            "Informe a senha atual e confirme a nova senha.".into(),
        ));
    }
    let response=Connection::new(config)?.request_builder(Method::POST,&["auth","password"],&[])?.json(&json!({"current_password":current,"new_password":new,"new_password_confirmation":confirmation})).send().map_err(|_|Error::Unavailable)?;
    check_status(&response)?;
    let session = Session::from_headers(response.headers())?;
    read_response(response)?;
    let mut config = config.clone();
    config.human = Some(session);
    Ok(config)
}
pub enum Command {
    Login {
        username: String,
        password: String,
    },
    Password {
        current: String,
        new: String,
        confirmation: String,
    },
    Logout,
    Me,
}
struct Reply {
    generation: u64,
    config: Option<Config>,
    result: Result<Value, Error>,
}
pub struct Panel {
    pending: Option<Receiver<Reply>>,
    pub busy: bool,
    username: String,
    password: String,
    current: String,
    new: String,
    confirmation: String,
    info: Option<Value>,
    error: Option<String>,
}
impl Panel {
    pub fn new() -> Self {
        Self {
            pending: None,
            busy: false,
            username: String::new(),
            password: String::new(),
            current: String::new(),
            new: String::new(),
            confirmation: String::new(),
            info: None,
            error: None,
        }
    }
    fn submit(&mut self, shared: &Arc<Shared>, command: Command) {
        if self.busy {
            return;
        }
        let config = shared.config.read().unwrap();
        let generation = shared.generation.load(Ordering::Acquire);
        let config = config.clone();
        self.busy = true;
        self.error = None;
        self.info = None;
        let shared = shared.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let mut changed = None;
            let result = (|| {
                if shared.generation.load(Ordering::Acquire) != generation {
                    return Err(Error::Cancelled);
                }
                match command {
                    Command::Login { username, password } => {
                        let (next, v) = login(&config, &username, &password)?;
                        changed = Some(next);
                        Ok(v)
                    }
                    Command::Password {
                        current,
                        new,
                        confirmation,
                    } => {
                        changed = Some(password(&config, &current, &new, &confirmation)?);
                        Ok(json!({"message":"Senha atualizada. A sessão foi renovada."}))
                    }
                    Command::Logout => {
                        let mut next = config.clone();
                        next.human = None;
                        changed = Some(next);
                        let result =
                            Connection::new(&config)?.post(&["auth", "logout"], &[], &json!({}));
                        result.map(|_| json!({"message":"Sessão humana encerrada; conexão voltou ao Bearer configurado."}))
                    }
                    Command::Me => Connection::new(&config)?.get(&["auth", "me"], &[]),
                }
            })();
            let _ = tx.send(Reply {
                generation,
                config: changed,
                result,
            });
        });
    }
    pub fn poll(&mut self, shared: &Shared) -> Option<Config> {
        let reply = match self.pending.as_ref()?.try_recv() {
            Ok(reply) => reply,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                self.busy = false;
                self.error = Some("Operação de autenticação interrompida; consulte a identidade antes de repetir.".into());
                return None;
            }
        };
        self.pending = None;
        self.busy = false;
        if reply.generation != shared.generation.load(Ordering::Acquire) {
            return None;
        }
        match reply.result {
            Ok(v) => self.info = Some(v),
            Err(e) => self.error = Some(e.to_string()),
        }
        reply.config
    }
    pub fn clear(&mut self) {
        self.username.clear();
        self.password.clear();
        self.current.clear();
        self.new.clear();
        self.confirmation.clear();
        self.info = None;
        self.error = None;
    }
    pub fn show(&mut self, ui: &mut eframe::egui::Ui, shared: &Arc<Shared>, enabled: bool) {
        use eframe::egui;
        ui.heading("Autenticação humana");
        let human = shared.config.read().unwrap().human.is_some();
        ui.label(if human{"Conexão ativa: sessão humana. Cookies e CSRF somente em memória."}else{"Conexão ativa: Bearer configurado ou acesso anônimo. Login usa credenciais humanas separadas."});
        ui.small("Não há fallback de sessão humana para Bearer. MCP e workstreams exigem conexão Bearer. Senhas humanas não são guardadas no Keychain.");
        ui.add_enabled_ui(enabled && !self.busy, |ui| {
            if !human {
                ui.label("Usuário");
                ui.add(egui::TextEdit::singleline(&mut self.username).char_limit(256));
                ui.label("Senha");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                        .char_limit(4096),
                );
                if ui.button("Entrar com sessão humana").clicked() {
                    let password = std::mem::take(&mut self.password);
                    self.submit(
                        shared,
                        Command::Login {
                            username: self.username.clone(),
                            password,
                        },
                    );
                }
            } else {
                ui.horizontal(|ui| {
                    if ui.button("Consultar identidade e capacidades").clicked() {
                        self.submit(shared, Command::Me);
                    }
                    if ui
                        .button("Sair da sessão humana e voltar ao Bearer")
                        .clicked()
                    {
                        self.submit(shared, Command::Logout);
                    }
                });
                egui::CollapsingHeader::new("Alterar minha senha").show(ui, |ui| {
                    for (label, value) in [
                        ("Senha atual", &mut self.current),
                        ("Nova senha", &mut self.new),
                        ("Confirmar nova senha", &mut self.confirmation),
                    ] {
                        ui.label(label);
                        ui.add(
                            egui::TextEdit::singleline(value)
                                .password(true)
                                .char_limit(4096),
                        );
                    }
                    if ui.button("Alterar senha e renovar sessão").clicked() {
                        let current = std::mem::take(&mut self.current);
                        let new = std::mem::take(&mut self.new);
                        let confirmation = std::mem::take(&mut self.confirmation);
                        self.submit(
                            shared,
                            Command::Password {
                                current,
                                new,
                                confirmation,
                            },
                        );
                    }
                });
            }
        });
        if self.busy {
            ui.spinner();
        }
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(244, 190, 103), e);
        }
        if let Some(v) = &self.info {
            crate::console::show_values(ui, v, 0);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Mock, Response};
    #[test]
    fn human_session_does_not_fall_back_to_bearer_and_uses_csrf() {
        let mock = Mock::new(|r| {
            if r.path == "/auth/login" {
                assert!(!r.headers.to_lowercase().contains("authorization:"));
                Response {
                    status: 200,
                    body: b"{}".to_vec(),
                    headers: vec![
                        (
                            "Set-Cookie".into(),
                            "ai_memory_session=synthetic-session; HttpOnly; Path=/".into(),
                        ),
                        (
                            "Set-Cookie".into(),
                            "ai_memory_csrf=synthetic-csrf; Path=/".into(),
                        ),
                    ],
                }
            } else {
                Response::json(200, json!({"role":"user"}))
            }
        });
        let mut original = mock.config();
        original.token = "synthetic-bearer".into();
        let (config, me) = login(&original, "fixture", "synthetic-password").unwrap();
        assert_eq!(me["role"], "user");
        assert_eq!(mock.requests().len(), 2);
        Connection::new(&config)
            .unwrap()
            .post(&["admin", "lint"], &[], &json!({"dry_run":true}))
            .unwrap();
        let requests = mock.requests();
        let r = requests.last().unwrap();
        assert!(
            r.headers
                .to_lowercase()
                .contains("x-csrf-token: synthetic-csrf")
        );
        assert!(!r.headers.to_lowercase().contains("authorization:"));
        let persisted = serde_json::to_string(&config).unwrap();
        assert!(!persisted.contains("synthetic"));
        assert!(
            Connection::new(&config)
                .unwrap()
                .get::<Value>(&["workstream", "recent"], &[])
                .is_err()
        );
    }
    #[test]
    fn passwords_require_https_outside_loopback() {
        let c = Config {
            endpoint: "http://example.test".into(),
            ..Config::default()
        };
        assert!(login(&c, "fixture", "password").is_err());
    }
    #[test]
    fn password_rotation_replaces_both_cookies_and_logout_accepts_204() {
        let mock = Mock::new(|r| {
            if r.path == "/auth/password" {
                assert!(r.headers.contains("synthetic-old-csrf"));
                assert_eq!(
                    r.body["new_password_confirmation"],
                    "synthetic-new-password"
                );
                Response {
                    status: 200,
                    body: b"{}".to_vec(),
                    headers: vec![
                        (
                            "Set-Cookie".into(),
                            "ai_memory_session=synthetic-new-session; Path=/".into(),
                        ),
                        (
                            "Set-Cookie".into(),
                            "ai_memory_csrf=synthetic-new-csrf; Path=/".into(),
                        ),
                    ],
                }
            } else {
                Response {
                    status: 204,
                    body: vec![],
                    headers: vec![],
                }
            }
        });
        let config = Config {
            human: Some(Session {
                session: "synthetic-old-session".into(),
                csrf: "synthetic-old-csrf".into(),
            }),
            ..mock.config()
        };
        let next = password(
            &config,
            "synthetic-old-password",
            "synthetic-new-password",
            "synthetic-new-password",
        )
        .unwrap();
        let result = Connection::new(&next)
            .unwrap()
            .post(&["auth", "logout"], &[], &json!({}))
            .unwrap();
        assert!(result.is_null());
        let requests = mock.requests();
        let last = requests.last().unwrap();
        assert!(last.headers.contains("synthetic-new-session"));
        assert!(last.headers.contains("synthetic-new-csrf"));
        assert!(!last.headers.contains("synthetic-old"));
    }
    #[test]
    fn rejected_human_session_never_retries_with_retained_bearer() {
        let mock = Mock::new(|_| Response::json(401, json!({"error":"private"})));
        let config = Config {
            human: Some(Session {
                session: "synthetic-session".into(),
                csrf: "synthetic-csrf".into(),
            }),
            token: "synthetic-bearer".into(),
            ..mock.config()
        };
        assert_eq!(
            Connection::new(&config)
                .unwrap()
                .get::<Value>(&["admin", "users"], &[]),
            Err(Error::Authentication)
        );
        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].headers.contains("synthetic-bearer"));
    }
    #[test]
    fn malformed_or_missing_cookies_fail_closed() {
        let mut headers = HeaderMap::new();
        headers.append(
            "set-cookie",
            "ai_memory_session=synthetic; Path=/".parse().unwrap(),
        );
        assert!(Session::from_headers(&headers).is_err());
        headers.append(
            "set-cookie",
            "ai_memory_csrf=bad value; Path=/".parse().unwrap(),
        );
        assert!(Session::from_headers(&headers).is_err());
    }
    #[test]
    fn cookie_names_with_surrounding_space_are_accepted() {
        let mut headers = HeaderMap::new();
        headers.append(
            "set-cookie",
            "ai_memory_session =synthetic-session; Path=/"
                .parse()
                .unwrap(),
        );
        headers.append(
            "set-cookie",
            "ai_memory_csrf =synthetic-csrf; Path=/".parse().unwrap(),
        );
        let session = Session::from_headers(&headers)
            .expect("cookie names padded with spaces must still be recognized");
        assert_eq!(session.session, "synthetic-session");
        assert_eq!(session.csrf, "synthetic-csrf");
    }
}
