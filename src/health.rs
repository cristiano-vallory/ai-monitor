use crate::{
    client::{Connection, Error},
    model::Config,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

pub const INTERVAL: u64 = 30;
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Identity {
    pub version: String,
    pub level: String,
    pub distinguishes_operators: bool,
}
#[derive(Clone)]
pub struct Metric {
    pub series: String,
    pub value: f64,
}
#[derive(Clone, Default)]
pub struct Snapshot {
    pub at: Option<DateTime<Utc>>,
    pub identity: Option<Identity>,
    pub status: Option<Value>,
    pub features: Option<Value>,
    pub metrics: Vec<Metric>,
    pub error: Option<Error>,
    pub auxiliary_errors: Vec<String>,
    pub pending: Option<usize>,
    pub pending_scope: Option<crate::client::Scope>,
}
impl Snapshot {
    pub fn fresh(&self, now: DateTime<Utc>) -> bool {
        self.at
            .is_some_and(|at| (now - at).num_seconds() <= (INTERVAL * 3) as i64)
    }
    pub fn alerts(&self) -> Vec<String> {
        let mut alerts = Vec::new();
        if let Some(v) = &self.status {
            if v.pointer("/consolidation/failed")
                .and_then(Value::as_u64)
                .is_some_and(|n| n > 0)
            {
                alerts.push("Consolidações com falha".into());
            }
            if let Some(queue) = v["write_queue"].as_array()
                && let (Some(queued), Some(capacity)) = (
                    queue.first().and_then(Value::as_u64),
                    queue.get(1).and_then(Value::as_u64),
                )
                && capacity > 0
                && queued as f64 / capacity as f64 >= 0.8
            {
                alerts.push("Fila de escrita acima de 80%".into());
            }
            if v.pointer("/storage/data_dir_free_bytes")
                .and_then(Value::as_u64)
                .is_some_and(|n| n < 512 * 1024 * 1024)
            {
                alerts.push("Menos de 512 MiB livres no disco do servidor".into());
            }
            for role in ["llm", "embedding"] {
                if let Some(status) = v["providers"][role]["status"].as_str()
                    && matches!(status, "error" | "unhealthy" | "failed")
                {
                    alerts.push(format!("Provider {role} com falha"));
                }
            }
        }
        if matches!(
            self.error,
            Some(Error::Unavailable | Error::Http(_) | Error::Format)
        ) {
            alerts.push("Saúde do servidor indisponível".into());
        }
        alerts
    }
    pub fn menu_summary(&self, now: DateTime<Utc>) -> String {
        if self.at.is_none() {
            return "Saúde: aguardando".into();
        }
        if !self.fresh(now) {
            return "Saúde: leitura atrasada".into();
        }
        if let Some(error) = &self.error {
            return format!("Saúde: {error}");
        }
        let alerts = self.alerts();
        if alerts.is_empty() {
            "Saúde: sem alertas nas verificações disponíveis".into()
        } else {
            format!("Saúde: {}", alerts.join(" · "))
        }
    }
}
pub fn collect(config: &Config, cancelled: impl Fn() -> bool) -> Snapshot {
    let mut snapshot = Snapshot::default();
    let Ok(client) = Connection::new(config) else {
        snapshot.error = Some(Error::Invalid("Configuração inválida.".into()));
        snapshot.at = Some(Utc::now());
        return snapshot;
    };
    if cancelled() {
        return snapshot;
    }
    if config.human.is_some() {
        match client.get::<Value>(&["auth", "me"], &[]) {
            Ok(v) if v["role"].is_string() => {
                snapshot.identity = Some(Identity {
                    level: format!("human:{}", v["role"].as_str().unwrap()),
                    ..Identity::default()
                })
            }
            Ok(_) => snapshot
                .auxiliary_errors
                .push("Identidade humana: formato incompatível".into()),
            Err(e) => snapshot
                .auxiliary_errors
                .push(format!("Identidade humana: {e}")),
        }
    } else {
        match client.get(&["identity"], &[]) {
            Ok(v) => snapshot.identity = Some(v),
            Err(e) => snapshot.auxiliary_errors.push(format!("Identidade: {e}")),
        }
    }
    if cancelled() {
        return Snapshot::default();
    }
    match client.get::<Value>(&["admin", "status"], &[]) {
        Ok(v) if v["version"].is_string() && v["counts"].is_object() => snapshot.status = Some(v),
        Ok(_) => snapshot.error = Some(Error::Format),
        Err(e) => snapshot.error = Some(e),
    }
    if cancelled() {
        return Snapshot::default();
    }
    match client.get(&["api", "v1", "docs", "features", "status"], &[]) {
        Ok(v) => snapshot.features = Some(v),
        Err(e) => snapshot.auxiliary_errors.push(format!("Recursos: {e}")),
    }
    if cancelled() {
        return Snapshot::default();
    }
    if snapshot.status.is_some() {
        match client.text(&["admin", "metrics"]) {
            Ok(t) => snapshot.metrics = parse_metrics(&t),
            Err(e) => snapshot.auxiliary_errors.push(format!("Métricas: {e}")),
        }
    }
    if cancelled() {
        return Snapshot::default();
    }
    if let Some(scope) = &config.proposal_watch {
        snapshot.pending_scope = Some(scope.clone());
        match client.get::<Value>(
            &["admin", "pending-writes"],
            &[
                ("workspace", scope.workspace.clone()),
                ("project", scope.project.clone()),
                ("status", "pending".into()),
                ("limit", "200".into()),
            ],
        ) {
            Ok(Value::Array(rows)) => snapshot.pending = Some(rows.len()),
            Ok(_) => snapshot
                .auxiliary_errors
                .push("Propostas: formato incompatível".into()),
            Err(e) => snapshot.auxiliary_errors.push(format!("Propostas: {e}")),
        }
    }
    if cancelled() {
        return Snapshot::default();
    }
    snapshot.at = Some(Utc::now());
    snapshot
}

pub fn parse_metrics(text: &str) -> Vec<Metric> {
    text.lines()
        .filter_map(|line| {
            if line.starts_with('#') || line.len() > 1024 {
                return None;
            }
            let (series, value) = line.split_once(' ')?;
            let value = value.trim().parse::<f64>().ok()?;
            if !value.is_finite()
                || !series.starts_with("ai_memory_")
                || !series
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_{}=\",.+-".contains(c))
            {
                return None;
            }
            Some(Metric {
                series: series.into(),
                value,
            })
        })
        .take(256)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Mock, Response};
    use serde_json::json;
    #[test]
    fn pending_watch_is_one_explicit_scope_and_is_not_zero_on_denial() {
        let mock = Mock::new(|r| {
            if r.path.starts_with("/admin/pending-writes") {
                Response::json(403, json!({}))
            } else if r.path == "/admin/status" {
                Response::json(200, json!({"version":"3.3.1","counts":{}}))
            } else {
                Response::json(200, json!({}))
            }
        });
        let mut config = mock.config();
        assert!(collect(&config, || false).pending_scope.is_none());
        config.proposal_watch = Some(crate::client::Scope {
            workspace: "synthetic".into(),
            project: "demo".into(),
        });
        let result = collect(&config, || false);
        assert!(result.pending.is_none());
        assert_eq!(result.pending_scope, config.proposal_watch);
        let requests = mock.requests();
        let calls: Vec<_> = requests
            .iter()
            .filter(|r| r.path.starts_with("/admin/pending-writes"))
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "GET");
        assert!(
            calls[0]
                .path
                .contains("workspace=synthetic&project=demo&status=pending&limit=200")
        );
    }
    #[test]
    fn permission_denial_is_not_a_healthy_zero_and_never_triggers_writes() {
        let mock = Mock::new(|r| {
            if r.path.starts_with("/identity") {
                Response::json(200, json!({"version":"3.3.1","level":"user"}))
            } else {
                Response::json(403, json!({"error":"private"}))
            }
        });
        let s = collect(&mock.config(), || false);
        assert_eq!(s.error, Some(Error::Forbidden));
        assert!(s.status.is_none());
        assert!(s.metrics.is_empty());
        assert!(!s.menu_summary(Utc::now()).contains("sem alertas"));
        assert!(
            !mock
                .requests()
                .iter()
                .any(|r| r.path.starts_with("/handoff"))
        );
    }
    #[test]
    fn partial_or_unknown_fields_do_not_become_zero_measurements() {
        let mut s = Snapshot {
            at: Some(Utc::now()),
            status: Some(json!({"storage":{},"consolidation":{},"write_queue":[]})),
            ..Default::default()
        };
        assert!(s.alerts().is_empty());
        s.status = Some(
            json!({"storage":{"data_dir_free_bytes":0},"consolidation":{"failed":2},"write_queue":[80,100]}),
        );
        assert_eq!(s.alerts().len(), 3);
        s.at = Some(Utc::now() - chrono::Duration::seconds(91));
        assert!(!s.fresh(Utc::now()));
        assert!(s.menu_summary(Utc::now()).contains("atrasada"));
    }
    #[test]
    fn generation_change_stops_followup_requests() {
        let mock = Mock::new(|_| Response::json(200, json!({"version":"3.3.1","level":"root"})));
        let s = collect(&mock.config(), || !mock.requests().is_empty());
        assert!(s.at.is_none());
        assert_eq!(mock.requests().len(), 1);
    }
    #[test]
    fn prometheus_parser_ignores_nonfinite_and_foreign_series() {
        let m = parse_metrics(
            "# HELP ignored\nai_memory_mcp_tool_calls_total{kind=\"read\"} 42\nai_memory_x NaN\nother 5\nai_memory_x +Inf\n",
        );
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].value, 42.0);
    }
}
