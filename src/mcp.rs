//! A fresh MCP session per operation; no automatic retry of tool calls.
use crate::client::{Connection, Error, RESPONSE_LIMIT, check_status, read_response};
use reqwest::{Method, blocking::Response};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read},
    time::Duration,
};

fn decode(message: Value, id: u64) -> Result<Value, Error> {
    if message["jsonrpc"] != "2.0" || message["id"] != id {
        return Err(Error::Format);
    }
    if !message["error"].is_null() {
        return Err(Error::Invalid("O servidor recusou a chamada MCP. Verifique compatibilidade, permissões e estado do objeto.".into()));
    }
    if !message["result"].is_object() {
        return Err(Error::Format);
    }
    Ok(message["result"].clone())
}
fn rpc_body(response: Response, id: u64) -> Result<Value, Error> {
    check_status(&response)?;
    let is_sse = response
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.starts_with("text/event-stream"));
    if !is_sse {
        return decode(
            serde_json::from_slice(&read_response(response)?).map_err(|_| Error::Format)?,
            id,
        );
    }
    // Stateful servers can keep the stream open: finish at our response, not EOF.
    parse_sse(BufReader::new(response.take(RESPONSE_LIMIT + 1)), id)
}
fn parse_sse(mut reader: impl BufRead, id: u64) -> Result<Value, Error> {
    let mut total = 0;
    let mut data = String::new();
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|_| Error::Unavailable)?;
        total += n;
        if total > RESPONSE_LIMIT as usize {
            return Err(Error::TooLarge);
        }
        if n == 0 || line.trim_end().is_empty() {
            if let Ok(message) = serde_json::from_str::<Value>(&data)
                && message["id"] == id
            {
                return decode(message, id);
            }
            data.clear();
            if n == 0 {
                return Err(Error::Format);
            }
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
    }
}
impl Connection {
    pub fn tool(
        &self,
        name: &str,
        arguments: &Value,
        cancelled: impl Fn() -> bool,
    ) -> Result<Value, Error> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let init = self.request_builder(Method::POST, &["mcp"], &[])?
            .header("Accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"ai-monitor","version":env!("CARGO_PKG_VERSION")}}}))
            .send().map_err(|_|Error::Unavailable)?;
        check_status(&init)?;
        let session = init.headers().get("mcp-session-id").cloned();
        let initialized = rpc_body(init, 1)?;
        let protocol = initialized["protocolVersion"]
            .as_str()
            .ok_or(Error::Format)?;
        let call = |body: Value, id: Option<u64>| -> Result<Value, Error> {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let mut request = self
                .request_builder(Method::POST, &["mcp"], &[])?
                .timeout(Duration::from_secs(120))
                .header("Accept", "application/json, text/event-stream")
                .header("MCP-Protocol-Version", protocol)
                .json(&body);
            if let Some(s) = &session {
                request = request.header("Mcp-Session-Id", s);
            }
            let response = request.send().map_err(|_| Error::Unavailable)?;
            if let Some(id) = id {
                rpc_body(response, id)
            } else {
                check_status(&response)?;
                Ok(Value::Null)
            }
        };
        if session.is_some() {
            call(
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                None,
            )?;
        }
        let result = call(
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
            Some(2),
        );
        // Best-effort release of stateful transport resources, never repeat the tool.
        if let Some(session) = session
            && let Ok(request) = self.request_builder(Method::DELETE, &["mcp"], &[])
        {
            let _ = request
                .timeout(Duration::from_secs(2))
                .header("Mcp-Session-Id", session)
                .header("MCP-Protocol-Version", protocol)
                .send();
        }
        let result = result?;
        if result["isError"] == true {
            return Err(Error::Invalid("A ferramenta MCP não concluiu a operação. Consulte o estado atual antes de repetir.".into()));
        }
        if let Some(structured) = result.get("structuredContent") {
            return Ok(structured.clone());
        }
        let content = result["content"].as_array().ok_or(Error::Format)?;
        let text = content
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return Err(Error::Format);
        }
        Ok(serde_json::from_str(&text).unwrap_or(json!({"message":text})))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sse_ignores_notifications_and_finishes_at_matching_response() {
        let stream = b"data: {\"jsonrpc\":\"2.0\",\"method\":\"keepalive\"}\n\ndata: {\"jsonrpc\":\"2.0\",\n data:\"ignored\"}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(
            parse_sse(std::io::Cursor::new(stream), 2).unwrap(),
            json!({"ok":true})
        );
        assert_eq!(
            decode(json!({"jsonrpc":"2.0","id":99,"result":{}}), 2),
            Err(Error::Format)
        );
    }
}
#[cfg(test)]
mod transport_tests {
    use super::*;
    use crate::test_support::{Mock, Response};
    #[test]
    fn stateful_handshake_attaches_session_and_calls_tool_once() {
        let mock = Mock::new(|r| {
            if r.method == "DELETE" {
                return Response::json(200, json!({}));
            }
            match r.body["method"].as_str().unwrap(){
                "initialize"=>Response{status:200,body:json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05"}}).to_string().into_bytes(),headers:vec![("Mcp-Session-Id".into(),"synthetic-session".into())]},
                "notifications/initialized"=>Response::json(202,json!({})),
                "tools/call"=>Response{status:200,body:b"data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"{\\\"cancelled\\\":true}\"}]}}\n\n".to_vec(),headers:vec![("Content-Type".into(),"text/event-stream".into())]},
                _=>panic!("unexpected method"),
            }
        });
        let v = Connection::new(&mock.config())
            .unwrap()
            .tool(
                "memory_handoff_cancel",
                &json!({"handoff_id":"exact","workspace":"a","project":"b"}),
                || false,
            )
            .unwrap();
        assert_eq!(v, json!({"cancelled":true}));
        let requests = mock.requests();
        assert_eq!(requests.len(), 4);
        assert!(!requests[1].body.as_object().unwrap().contains_key("id"));
        assert!(
            requests[2]
                .headers
                .to_lowercase()
                .contains("mcp-session-id: synthetic-session")
        );
        assert_eq!(
            requests[2].body["params"]["arguments"]["handoff_id"],
            "exact"
        );
    }
    #[test]
    fn cancellation_after_initialize_prevents_tool_call() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let mock = Mock::new(move |_| {
            flag.store(true, Ordering::SeqCst);
            Response::json(
                200,
                json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05"}}),
            )
        });
        assert_eq!(
            Connection::new(&mock.config())
                .unwrap()
                .tool("memory_feedback", &json!({}), || cancelled
                    .load(Ordering::SeqCst)),
            Err(Error::Cancelled)
        );
        assert_eq!(mock.requests().len(), 1);
    }
}
