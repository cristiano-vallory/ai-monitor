//! Read-only requests for the interactive inspector. No lifecycle claim routes.
use crate::{
    client::{Connection, Error, Scope, page_segments},
    model::Config,
    monitor::Shared,
};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    Sessions {
        scope: Scope,
        offset: usize,
        include_open: bool,
    },
    Observations {
        scope: Scope,
        session: String,
        offset: usize,
        text: String,
        kind: String,
    },
    Pages(Scope),
    Search {
        scope: Option<Scope>,
        text: String,
    },
    Page {
        scope: Scope,
        path: String,
    },
    History {
        scope: Scope,
        path: String,
        offset: usize,
        from: String,
        to: String,
    },
    Briefing(Scope),
    Graph(Scope),
    CrossGraph,
    Handoffs {
        scope: Scope,
        state: String,
    },
}
impl Query {
    pub fn execute(&self, client: &Connection) -> Result<Value, Error> {
        let mut result: Value = match self {
            Self::Sessions {
                scope,
                offset,
                include_open,
            } => client.get_scoped(
                scope,
                &["sessions"],
                &[
                    ("limit", "50".into()),
                    ("offset", offset.to_string()),
                    ("include_open", include_open.to_string()),
                ],
            ),
            Self::Observations {
                scope,
                session,
                offset,
                text,
                kind,
            } => client.get_scoped(
                scope,
                &["sessions", session, "observations"],
                &[
                    ("limit", "50".into()),
                    ("offset", offset.to_string()),
                    ("order", "asc".into()),
                    ("q", text.clone()),
                    ("kinds", kind.clone()),
                    ("body_max_chars", "4000".into()),
                ],
            ),
            Self::Pages(scope) => client.get_scoped(scope, &["pages"], &[]),
            Self::Search { scope, text } => {
                if text.trim().is_empty() || text.len() > 4096 {
                    return Err(Error::Invalid("Informe uma pesquisa com até 4 KiB.".into()));
                }
                let mut q = vec![("q", text.clone()), ("limit", "100".into())];
                if let Some(s) = scope {
                    s.validate()?;
                    q.push(("workspace", s.workspace.clone()));
                    q.push(("project", s.project.clone()));
                }
                client.get(&["api", "v1", "search"], &q)
            }
            Self::Page { scope, path } | Self::History { scope, path, .. } => {
                let parts = page_segments(path)?;
                let mut tail = vec![if matches!(self, Self::History { .. }) {
                    "history"
                } else {
                    "pages"
                }];
                tail.extend(parts);
                let mut q = Vec::new();
                if let Self::History {
                    offset, from, to, ..
                } = self
                {
                    q.extend([("limit", "50".into()), ("offset", offset.to_string())]);
                    if !from.is_empty() {
                        q.push(("from", from.clone()));
                    }
                    if !to.is_empty() {
                        q.push(("to", to.clone()));
                    }
                }
                client.get_scoped(scope, &tail, &q)
            }
            Self::Briefing(scope) => {
                client.get_scoped(scope, &["briefing"], &[("limit", "20".into())])
            }
            Self::Graph(scope) => client.get_scoped(scope, &["graph"], &[]),
            Self::CrossGraph => client.get(&["api", "v1", "graph"], &[]),
            Self::Handoffs { scope, state } => {
                let mut q = vec![("limit", "200".into())];
                if !state.is_empty() {
                    q.push(("state", state.clone()));
                }
                client.get_scoped(scope, &["handoffs"], &q)
            }
        }?;
        self.validate_response(&result)?;
        if matches!(self, Self::Handoffs { .. })
            && let Some(rows) = result["handoffs"].as_array_mut()
        {
            for row in rows {
                if row["redacted"] == true
                    && let Some(fields) = row.as_object_mut()
                {
                    for field in ["summary", "open_questions", "next_steps"] {
                        fields.remove(field);
                    }
                }
            }
        }
        Ok(result)
    }
    fn validate_response(&self, v: &Value) -> Result<(), Error> {
        let rows = |v: &Value, max: usize, fields: &[&str]| {
            v.as_array().is_some_and(|rows| {
                rows.len() <= max
                    && rows
                        .iter()
                        .all(|r| r.is_object() && fields.iter().all(|field| r[*field].is_string()))
            })
        };
        let valid = match self {
            Self::Sessions { .. } => rows(
                &v["sessions"],
                50,
                &["session_id", "agent_kind", "started_at"],
            ),
            Self::Observations { .. } => {
                rows(&v["observations"], 50, &["id", "kind", "created_at"])
                    && v["total"].is_u64()
                    && v["session"].is_object()
            }
            Self::Pages(_) => rows(v, 100_000, &["path", "title"]),
            Self::Search { .. } => rows(v, 100, &["path", "title", "workspace", "project"]),
            Self::Page { scope, path } => {
                v["body_markdown"].is_string()
                    && v["id"].is_string()
                    && v["workspace"] == scope.workspace
                    && v["project"] == scope.project
                    && v["path"] == *path
                    && rows(&v["links"], 100_000, &["path", "workspace", "project"])
                    && rows(&v["backlinks"], 100_000, &["path", "workspace", "project"])
            }
            Self::History { scope, path, .. } => {
                rows(&v["versions"], 50, &["id", "created_at"])
                    && v["total"].is_u64()
                    && v["diff"].is_object()
                    && v["workspace"] == scope.workspace
                    && v["project"] == scope.project
                    && v["path"] == *path
            }
            Self::Briefing(_) => v["counts"].is_object() && v["recent_pages"].is_array(),
            Self::Graph(_) => {
                rows(&v["nodes"], 120, &["path", "title"])
                    && rows(&v["edges"], 14_400, &["from_path", "to_path"])
            }
            Self::CrossGraph => rows(
                &v["edges"],
                100_000,
                &[
                    "from_path",
                    "to_path",
                    "from_workspace",
                    "from_project",
                    "to_workspace",
                    "to_project",
                ],
            ),
            Self::Handoffs { .. } => rows(&v["handoffs"], 200, &["id", "state"]),
        };
        if valid { Ok(()) } else { Err(Error::Format) }
    }
}

struct Job {
    ticket: u64,
    generation: u64,
    config: Config,
    query: Query,
}
pub struct Reply {
    pub ticket: u64,
    pub generation: u64,
    pub query: Query,
    pub result: Result<Value, Error>,
}
pub struct Worker {
    sender: SyncSender<Job>,
    pub replies: Receiver<Reply>,
    stop: Arc<AtomicBool>,
}
impl Worker {
    pub fn new(shared: Arc<Shared>) -> Self {
        let (sender, jobs) = mpsc::sync_channel::<Job>(1);
        let (responses, replies) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            loop {
                if stopped.load(Ordering::Relaxed) || shared.stop.load(Ordering::Relaxed) {
                    break;
                }
                let job = match jobs.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(j) => j,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let cancelled = || {
                    stopped.load(Ordering::Relaxed)
                        || shared.stop.load(Ordering::Relaxed)
                        || shared.generation.load(Ordering::Acquire) != job.generation
                };
                let result = if cancelled() {
                    Err(Error::Cancelled)
                } else {
                    Connection::new(&job.config).and_then(|c| job.query.execute(&c))
                };
                let result = if cancelled() {
                    Err(Error::Cancelled)
                } else {
                    result
                };
                // Only one active UI request; bounded channel and no collector data retained.
                if responses
                    .send(Reply {
                        ticket: job.ticket,
                        generation: job.generation,
                        query: job.query,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            sender,
            replies,
            stop,
        }
    }
    pub fn submit(&self, ticket: u64, shared: &Shared, query: Query) -> Result<(), Error> {
        let config = shared.config.read().unwrap();
        self.sender
            .try_send(Job {
                ticket,
                generation: shared.generation.load(Ordering::Acquire),
                config: config.clone(),
                query,
            })
            .map_err(|_| Error::Invalid("Aguarde a consulta em andamento.".into()))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Mock, Response};
    use serde_json::json;
    fn scope() -> Scope {
        Scope {
            workspace: "team".into(),
            project: "app".into(),
        }
    }
    #[test]
    fn pagination_and_filters_are_sent_to_server_not_applied_after_a_page() {
        let mock =
            Mock::new(|_| Response::json(200, json!({"observations":[],"total":120,"session":{}})));
        Query::Observations {
            scope: scope(),
            session: "session-1".into(),
            offset: 50,
            text: "error & foo".into(),
            kind: "post-tool-use".into(),
        }
        .execute(&Connection::new(&mock.config()).unwrap())
        .unwrap();
        let path = &mock.requests()[0].path;
        assert!(path.contains("offset=50"));
        assert!(path.contains("q=error+%26+foo"));
        assert!(path.contains("kinds=post-tool-use"));
    }
    #[test]
    fn history_preserves_exact_page_and_requested_versions() {
        let mock = Mock::new(|_| {
            Response::json(
                200,
                json!({"versions":[],"total":0,"diff":{},"workspace":"team","project":"app","path":"notes/a b.md"}),
            )
        });
        Query::History {
            scope: scope(),
            path: "notes/a b.md".into(),
            offset: 100,
            from: "v1".into(),
            to: "v2".into(),
        }
        .execute(&Connection::new(&mock.config()).unwrap())
        .unwrap();
        let path = &mock.requests()[0].path;
        assert!(path.contains("history/notes/a%20b.md"));
        assert!(path.contains("from=v1&to=v2"));
        assert!(
            Query::Page {
                scope: scope(),
                path: "../../identity".into()
            }
            .execute(&Connection::new(&mock.config()).unwrap())
            .is_err()
        );
        assert_eq!(mock.requests().len(), 1);
    }
    #[test]
    fn malformed_success_is_an_error_not_an_empty_list() {
        let mock = Mock::new(|_| Response::json(200, json!({})));
        assert_eq!(
            Query::Sessions {
                scope: scope(),
                offset: 0,
                include_open: true
            }
            .execute(&Connection::new(&mock.config()).unwrap()),
            Err(Error::Format)
        );
    }
    #[test]
    fn changing_connection_discards_in_flight_result() {
        let (started, pending) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let mock = Mock::new(move |_| {
            started.send(()).unwrap();
            wait.recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            Response::json(200, json!({"sessions":[]}))
        });
        let shared = Shared::unstarted(mock.config());
        let worker = Worker::new(shared.clone());
        worker
            .submit(
                1,
                &shared,
                Query::Sessions {
                    scope: scope(),
                    offset: 0,
                    include_open: true,
                },
            )
            .unwrap();
        pending
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        shared.apply(mock.config());
        release.send(()).unwrap();
        let reply = worker
            .replies
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(reply.result, Err(Error::Cancelled));
        assert_eq!(reply.generation, 0);
        assert_eq!(mock.requests().len(), 1);
    }
    #[test]
    fn handoff_history_never_calls_consuming_endpoint() {
        let mock = Mock::new(|_| Response::json(200, json!({"handoffs":[]})));
        Query::Handoffs {
            scope: scope(),
            state: "accepted".into(),
        }
        .execute(&Connection::new(&mock.config()).unwrap())
        .unwrap();
        let path = &mock.requests()[0].path;
        assert!(path.contains("/api/v1/workspaces/team/projects/app/handoffs"));
        assert!(path.contains("state=accepted"));
    }
    #[test]
    fn redacted_handoff_text_cannot_reach_the_admin_report() {
        let mock = Mock::new(|_| {
            Response::json(
                200,
                json!({"handoffs":[{"id":"h1","state":"open","redacted":true,"summary":"hidden","next_steps":["hidden"],"open_questions":["hidden"]}]}),
            )
        });
        let result = Query::Handoffs {
            scope: scope(),
            state: "open".into(),
        }
        .execute(&Connection::new(&mock.config()).unwrap())
        .unwrap();
        assert!(!result.to_string().contains("hidden"));
        assert_eq!(result["handoffs"][0]["id"], "h1");
    }
}
