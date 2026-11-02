//! Bounded, same-server transport. Error messages never include server bodies,
//! request URLs, bearer values or provider diagnostics.
use crate::{api::endpoint, model::Config};
use reqwest::{Method, blocking::Client};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{fmt, io::Read, time::Duration};

pub const RESPONSE_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid(String),
    Authentication,
    Forbidden,
    Missing,
    Conflict,
    RateLimited,
    Unavailable,
    TooLarge,
    Format,
    Http(u16),
    Cancelled,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Authentication => f.write_str("Autenticação necessária ou expirada."),
            Self::Forbidden => f.write_str("A identidade conectada não tem permissão para este recurso."),
            Self::Missing => f.write_str("Recurso ausente, desabilitado ou incompatível com esta versão."),
            Self::Conflict => f.write_str("O objeto mudou ou a operação conflita com o estado atual. Recarregue antes de continuar."),
            Self::RateLimited => f.write_str("Limite de requisições atingido. Aguarde antes de tentar novamente."),
            Self::Unavailable => f.write_str("Servidor indisponível ou tempo limite atingido."),
            Self::TooLarge => f.write_str("Resposta excedeu o limite de 8 MiB. Restrinja a consulta."),
            Self::Format => f.write_str("Formato de resposta incompatível."),
            Self::Http(code) => write!(f, "Servidor respondeu HTTP {code}."),
            Self::Cancelled => f.write_str("Consulta descartada após mudança de conexão."),
        }
    }
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub workspace: String,
    pub project: String,
}
impl Scope {
    pub fn label(&self) -> String {
        format!("{} / {}", self.workspace, self.project)
    }
    pub fn validate(&self) -> Result<(), Error> {
        for name in [&self.workspace, &self.project] {
            if name.trim().is_empty()
                || name == "."
                || name == ".."
                || name.chars().any(char::is_control)
            {
                return Err(Error::Invalid(
                    "Selecione um workspace e um projeto válidos.".into(),
                ));
            }
        }
        Ok(())
    }
    pub fn path(&self, tail: &[&str]) -> Result<Vec<String>, Error> {
        self.validate()?;
        let mut path = vec![
            "api".into(),
            "v1".into(),
            "workspaces".into(),
            self.workspace.clone(),
            "projects".into(),
            self.project.clone(),
        ];
        path.extend(tail.iter().map(|s| (*s).to_owned()));
        Ok(path)
    }
}

pub struct Connection {
    client: Client,
    config: Config,
}
impl Connection {
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn new(config: &Config) -> Result<Self, Error> {
        let mut config = config.clone();
        config.normalize();
        config.validate().map_err(Error::Invalid)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .retry(reqwest::retry::never())
            .user_agent(concat!("ai-monitor/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self { client, config })
    }
    pub fn get<T: DeserializeOwned>(
        &self,
        path: &[&str],
        query: &[(&str, String)],
    ) -> Result<T, Error> {
        let bytes = self.request(Method::GET, path, query, None)?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Format)
    }
    pub fn get_scoped<T: DeserializeOwned>(
        &self,
        scope: &Scope,
        tail: &[&str],
        query: &[(&str, String)],
    ) -> Result<T, Error> {
        let path = scope.path(tail)?;
        self.get(&path.iter().map(String::as_str).collect::<Vec<_>>(), query)
    }
    pub fn text(&self, path: &[&str]) -> Result<String, Error> {
        String::from_utf8(self.request(Method::GET, path, &[], None)?).map_err(|_| Error::Format)
    }
    fn request(
        &self,
        method: Method,
        path: &[&str],
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Vec<u8>, Error> {
        let mut request = self.request_builder(method, path, query)?;
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().map_err(|_| Error::Unavailable)?;
        read_response(response)
    }
    pub(crate) fn request_builder(
        &self,
        method: Method,
        path: &[&str],
        query: &[(&str, String)],
    ) -> Result<reqwest::blocking::RequestBuilder, Error> {
        if path
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == ".." || p.chars().any(char::is_control))
        {
            return Err(Error::Invalid("Caminho inválido.".into()));
        }
        let mut url = endpoint(&self.config.endpoint, path).map_err(Error::Invalid)?;
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        let mut request = self
            .client
            .request(method.clone(), url)
            .header("Cache-Control", "no-store");
        if let Some(session) = &self.config.human {
            if matches!(path.first(), Some(&"mcp" | &"workstream" | &"identity")) {
                return Err(Error::Invalid(
                    "Este recurso exige conexão Bearer; encerre a sessão humana para usá-lo."
                        .into(),
                ));
            }
            request = session.headers(request, &method);
        } else if path.first() != Some(&"auth") && !self.config.token.is_empty() {
            request = request.bearer_auth(&self.config.token);
        }
        Ok(request)
    }
    pub fn post(
        &self,
        path: &[&str],
        query: &[(&str, String)],
        body: &Value,
    ) -> Result<Value, Error> {
        self.json(Method::POST, path, query, body)
    }
    pub fn json(
        &self,
        method: Method,
        path: &[&str],
        query: &[(&str, String)],
        body: &Value,
    ) -> Result<Value, Error> {
        let response = self
            .request_builder(method, path, query)?
            .timeout(Duration::from_secs(120))
            .json(body)
            .send()
            .map_err(|_| Error::Unavailable)?;
        let no_content = response.status() == reqwest::StatusCode::NO_CONTENT;
        let bytes = read_response(response)?;
        if no_content {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Format)
    }
}

pub(crate) fn check_status(response: &reqwest::blocking::Response) -> Result<(), Error> {
    if response.status().is_success() {
        return Ok(());
    }
    Err(match response.status().as_u16() {
        401 => Error::Authentication,
        403 => Error::Forbidden,
        404 => Error::Missing,
        409 | 412 => Error::Conflict,
        429 => Error::RateLimited,
        n => Error::Http(n),
    })
}
pub(crate) fn read_response(response: reqwest::blocking::Response) -> Result<Vec<u8>, Error> {
    check_status(&response)?;
    if response
        .content_length()
        .is_some_and(|n| n > RESPONSE_LIMIT)
    {
        return Err(Error::TooLarge);
    }
    let mut bytes = Vec::new();
    response
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Unavailable)?;
    if bytes.len() as u64 > RESPONSE_LIMIT {
        return Err(Error::TooLarge);
    }
    Ok(bytes)
}

/// Page path is split intentionally, unlike workspace/project names (one segment).
pub fn page_segments(path: &str) -> Result<Vec<&str>, Error> {
    let parts: Vec<_> = path.split('/').collect();
    if path.len() > 4096
        || parts.iter().any(|s| {
            s.is_empty()
                || *s == "."
                || *s == ".."
                || s.contains('\\')
                || s.chars().any(char::is_control)
        })
    {
        return Err(Error::Invalid(
            "Informe um caminho de página relativo, sem segmentos vazios, . ou ...".into(),
        ));
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Mock, Response};
    use serde_json::json;
    #[test]
    fn scopes_and_pages_cannot_escape_their_paths() {
        let mock = Mock::new(|_| Response::json(200, json!({})));
        let c = Connection::new(&mock.config()).unwrap();
        let scope = Scope {
            workspace: "a/b".into(),
            project: "?# x".into(),
        };
        c.get_scoped::<Value>(&scope, &["pages", "ok.md"], &[])
            .unwrap();
        assert!(
            mock.requests()[0]
                .path
                .starts_with("/api/v1/workspaces/a%2Fb/projects/%3F%23%20x/pages/ok.md")
        );
        for p in ["../x", "/etc/passwd", "a//b", "a/./b", "a\\b", ""] {
            assert!(page_segments(p).is_err(), "{p}");
        }
        assert_eq!(page_segments("notes/a b.md").unwrap(), ["notes", "a b.md"]);
    }
    #[test]
    fn errors_never_echo_private_bodies_and_redirects_are_not_followed() {
        let mock = Mock::new(|_| Response {
            status: 302,
            body: "secret-token".into(),
            headers: vec![("Location".into(), "https://example.com/secret-token".into())],
        });
        let error = Connection::new(&mock.config())
            .unwrap()
            .get::<Value>(&["identity"], &[])
            .unwrap_err();
        assert_eq!(error, Error::Http(302));
        assert!(!error.to_string().contains("secret-token"));
        assert_eq!(mock.requests().len(), 1);
        for (code, expected) in [
            (401, Error::Authentication),
            (403, Error::Forbidden),
            (404, Error::Missing),
            (409, Error::Conflict),
            (429, Error::RateLimited),
        ] {
            let mock = Mock::new(move |_| Response::json(code, json!({"error":"secret-token"})));
            assert_eq!(
                Connection::new(&mock.config())
                    .unwrap()
                    .get::<Value>(&["identity"], &[]),
                Err(expected)
            );
        }
    }
    #[test]
    fn oversized_and_malformed_responses_fail_without_dumping_them() {
        let mock = Mock::new(|_| Response {
            status: 200,
            body: "x".into(),
            headers: vec![("Content-Length".into(), (RESPONSE_LIMIT + 1).to_string())],
        });
        assert_eq!(
            Connection::new(&mock.config())
                .unwrap()
                .get::<Value>(&["identity"], &[]),
            Err(Error::TooLarge)
        );
        let mock = Mock::new(|_| Response {
            status: 200,
            body: "private invalid JSON".into(),
            headers: vec![],
        });
        assert_eq!(
            Connection::new(&mock.config())
                .unwrap()
                .get::<Value>(&["identity"], &[]),
            Err(Error::Format)
        );
    }
    #[test]
    fn credentials_require_secure_remote_transport() {
        let config = Config {
            endpoint: "http://example.com".into(),
            token: "test-token".into(),
            ..Default::default()
        };
        assert!(Connection::new(&config).is_err());
    }
}
