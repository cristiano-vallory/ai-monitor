//! Opt-in, endpoint-specific Bearer storage. Never stores human passwords/cookies.
use crate::platform::SECRET_STORE;

const SERVICE: &str = "io.github.cristiano_vallory.ai-monitor.bearer";
const LEGACY_SERVICE: &str = "io.github.cristianovallory.ai-monitor.bearer";

/// Operações de armazenamento por serviço/conta; permite migrar entradas
/// gravadas com o identificador antigo e testar a decisão sem chaveiro real.
pub trait StoreOps {
    fn read(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String>;
    fn write(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), String>;
    fn delete(&self, service: &str, account: &str) -> Result<(), String>;
}

fn load_with<S: StoreOps>(store: &S, account: &str) -> Result<Option<String>, String> {
    if let Some(bytes) = store.read(SERVICE, account)? {
        return String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| format!("Entrada do {SECRET_STORE} inválida."));
    }
    let Some(bytes) = store.read(LEGACY_SERVICE, account)? else {
        return Ok(None);
    };
    // Migração na primeira leitura: a entrada antiga só é apagada depois de
    // gravar no serviço novo; se a gravação falhar, o token ainda é devolvido.
    if store.write(SERVICE, account, &bytes).is_ok() {
        let _ = store.delete(LEGACY_SERVICE, account);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| format!("Entrada do {SECRET_STORE} inválida."))
}

fn save_with<S: StoreOps>(store: &S, account: &str, token: &str) -> Result<(), String> {
    store.write(SERVICE, account, token.as_bytes())?;
    let _ = store.delete(LEGACY_SERVICE, account);
    Ok(())
}

fn forget_with<S: StoreOps>(store: &S, account: &str) -> Result<(), String> {
    store.delete(SERVICE, account)?;
    store.delete(LEGACY_SERVICE, account)
}
fn account(endpoint: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(endpoint)
        .map_err(|_| format!("URL inválida para o {SECRET_STORE}."))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!("URL inválida para o {SECRET_STORE}."));
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.to_string())
}
#[cfg(target_os = "macos")]
struct Keychain;
#[cfg(target_os = "macos")]
impl StoreOps for Keychain {
    fn read(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
        match security_framework::passwords::get_generic_password(service, account) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.code() == -25300 => Ok(None),
            Err(_) => Err(format!(
                "O {SECRET_STORE} não permitiu ler o token. Autorize o app ou informe-o manualmente."
            )),
        }
    }
    fn write(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), String> {
        security_framework::passwords::set_generic_password(service, account, bytes)
            .map_err(|_| format!("O {SECRET_STORE} não permitiu guardar o token."))
    }
    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        match security_framework::passwords::delete_generic_password(service, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(_) => Err(format!("O {SECRET_STORE} não permitiu remover o token.")),
        }
    }
}
#[cfg(target_os = "macos")]
pub fn load(endpoint: &str) -> Result<Option<String>, String> {
    load_with(&Keychain, &account(endpoint)?)
}
#[cfg(target_os = "macos")]
pub fn save(endpoint: &str, token: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err(format!("Informe um token para guardar no {SECRET_STORE}."));
    }
    save_with(&Keychain, &account(endpoint)?, token)
}
#[cfg(target_os = "macos")]
pub fn forget(endpoint: &str) -> Result<(), String> {
    forget_with(&Keychain, &account(endpoint)?)
}

#[cfg(target_os = "linux")]
mod store {
    use super::{SECRET_STORE, StoreOps};
    use secret_service::{EncryptionType, blocking::SecretService};
    use std::collections::HashMap;

    const UNAVAILABLE: &str =
        "Chaveiro do sistema (Secret Service) indisponível. Informe o token manualmente.";

    fn connect() -> Result<SecretService<'static>, String> {
        SecretService::connect(EncryptionType::Dh).map_err(|_| UNAVAILABLE.to_owned())
    }
    /// Itens destravados para uma `service` específica (nova ou legada).
    fn items_for<'a>(
        service: &'a SecretService<'a>,
        account: &'a str,
        key: &str,
        error: impl Fn() -> String,
    ) -> Result<Vec<secret_service::blocking::Item<'a>>, String> {
        let found = service
            .search_items(HashMap::from([("service", key), ("account", account)]))
            .map_err(|_| error())?;
        if found.locked.is_empty() {
            return Ok(found.unlocked);
        }
        service
            .unlock_all(&found.locked.iter().collect::<Vec<_>>())
            .map_err(|_| error())?;
        let found = service
            .search_items(HashMap::from([("service", key), ("account", account)]))
            .map_err(|_| error())?;
        if !found.locked.is_empty() {
            return Err(error());
        }
        Ok(found.unlocked)
    }

    pub struct Secrets<'a>(pub &'a SecretService<'a>);
    impl StoreOps for Secrets<'_> {
        fn read(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
            let error = || {
                format!(
                    "O {SECRET_STORE} não permitiu ler o token. Autorize o app ou informe-o manualmente."
                )
            };
            let items = items_for(self.0, account, service, error)?;
            let Some(item) = items.into_iter().next() else {
                return Ok(None);
            };
            item.get_secret().map(Some).map_err(|_| error())
        }
        fn write(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), String> {
            let error = || format!("O {SECRET_STORE} não permitiu guardar o token.");
            let collection = self.0.get_default_collection().map_err(|_| error())?;
            if collection.is_locked().map_err(|_| error())? {
                collection.unlock().map_err(|_| error())?;
            }
            collection
                .create_item(
                    &format!("AI Monitor · Bearer {account}"),
                    HashMap::from([("service", service), ("account", account)]),
                    bytes,
                    true,
                    "text/plain",
                )
                .map(|_| ())
                .map_err(|_| error())?;
            // Sem duplicatas fora da coleção padrão que um search possa
            // devolver antes da entrada nova.
            // Falhar aqui não desfaz a gravação; o aviso no terminal deixa a
            // duplicata restante diagnosticável.
            let dedup = || -> Result<(), secret_service::Error> {
                for coll in self.0.get_all_collections()? {
                    if coll.collection_path == collection.collection_path {
                        continue;
                    }
                    let items = coll.search_items(HashMap::from([
                        ("service", service),
                        ("account", account),
                    ]))?;
                    if items.is_empty() {
                        continue;
                    }
                    // Item::delete exige item destravado.
                    if coll.is_locked()? {
                        coll.unlock()?;
                    }
                    for item in items {
                        item.delete()?;
                    }
                }
                Ok(())
            };
            if dedup().is_err() {
                eprintln!(
                    "AI Monitor: não foi possível remover uma cópia antiga do token em outra coleção do chaveiro."
                );
            }
            Ok(())
        }
        fn delete(&self, service: &str, account: &str) -> Result<(), String> {
            let error = || format!("O {SECRET_STORE} não permitiu remover o token.");
            for item in items_for(self.0, account, service, error)? {
                item.delete().map_err(|_| error())?;
            }
            Ok(())
        }
    }
    pub fn run<T>(op: impl FnOnce(&Secrets) -> Result<T, String>) -> Result<T, String> {
        let service = connect()?;
        op(&Secrets(&service))
    }
}
#[cfg(target_os = "linux")]
fn timed_with<T>(
    timeout: std::time::Duration,
    op: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(op());
    });
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "O chaveiro do sistema não respondeu em {} s. Informe o token manualmente.",
            timeout.as_secs()
        )),
        // A operação terminou sem resposta (pânico no worker): não é um timeout.
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(
            "Falha interna ao acessar o chaveiro do sistema. Informe o token manualmente.".into(),
        ),
    }
}
#[cfg(target_os = "linux")]
fn timed<T>(op: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String>
where
    T: Send + 'static,
{
    timed_with(std::time::Duration::from_secs(120), op)
}
#[cfg(target_os = "linux")]
pub fn load(endpoint: &str) -> Result<Option<String>, String> {
    let account = account(endpoint)?;
    timed(move || store::run(|s| load_with(s, &account)))
}
#[cfg(target_os = "linux")]
pub fn save(endpoint: &str, token: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err(format!("Informe um token para guardar no {SECRET_STORE}."));
    }
    let account = account(endpoint)?;
    let token = token.to_owned();
    timed(move || store::run(|s| save_with(s, &account, &token)))
}
#[cfg(target_os = "linux")]
pub fn forget(endpoint: &str) -> Result<(), String> {
    let account = account(endpoint)?;
    timed(move || store::run(|s| forget_with(s, &account)))
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "linux")]
    fn timed_with_enforces_the_deadline() {
        let start = std::time::Instant::now();
        let result: Result<(), String> = timed_with(std::time::Duration::from_millis(50), || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            Ok(())
        });
        let err = result.unwrap_err();
        assert!(
            err.starts_with("O chaveiro do sistema não respondeu em"),
            "{err}"
        );
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "deadline not enforced"
        );
        assert_eq!(
            timed_with(std::time::Duration::from_secs(5), || Ok(42)).unwrap(),
            42
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn timed_with_reports_a_worker_panic_as_internal_failure_not_timeout() {
        let start = std::time::Instant::now();
        let err = timed_with::<()>(std::time::Duration::from_secs(5), || {
            panic!("synthetic worker panic")
        })
        .unwrap_err();
        assert!(err.starts_with("Falha interna"), "{err}");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "a panicking worker must not wait for the full timeout"
        );
    }

    use super::*;
    #[test]
    fn accounts_are_server_and_base_path_specific() {
        assert_eq!(
            account("https://example.test/base/").unwrap(),
            account("https://example.test/base").unwrap()
        );
        assert_ne!(
            account("https://example.test/one").unwrap(),
            account("https://example.test/two").unwrap()
        );
        assert!(account("https://user:secret@example.test").is_err());
    }

    struct Mem {
        data: std::sync::Mutex<std::collections::HashMap<(String, String), Vec<u8>>>,
        fail_write: bool,
    }
    impl Mem {
        fn new() -> Self {
            Self {
                data: std::sync::Mutex::new(std::collections::HashMap::new()),
                fail_write: false,
            }
        }
        fn seed(&self, service: &str, account: &str, value: &[u8]) {
            self.data
                .lock()
                .unwrap()
                .insert((service.into(), account.into()), value.to_vec());
        }
        fn get(&self, service: &str, account: &str) -> Option<Vec<u8>> {
            self.data
                .lock()
                .unwrap()
                .get(&(service.into(), account.into()))
                .cloned()
        }
    }
    impl StoreOps for Mem {
        fn read(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.get(service, account))
        }
        fn write(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), String> {
            if self.fail_write {
                return Err("no".into());
            }
            self.seed(service, account, bytes);
            Ok(())
        }
        fn delete(&self, service: &str, account: &str) -> Result<(), String> {
            self.data
                .lock()
                .unwrap()
                .remove(&(service.into(), account.into()));
            Ok(())
        }
    }

    #[test]
    fn migration_covers_new_legacy_and_failure_cases() {
        let a = "acct";
        // acerto direto no serviço novo
        let mem = Mem::new();
        mem.seed(SERVICE, a, b"new-token");
        assert_eq!(load_with(&mem, a).unwrap().as_deref(), Some("new-token"));
        // entrada legada migra: gravada no novo, legada removida, token devolvido
        let mem = Mem::new();
        mem.seed(LEGACY_SERVICE, a, b"old-token");
        assert_eq!(load_with(&mem, a).unwrap().as_deref(), Some("old-token"));
        assert_eq!(mem.get(SERVICE, a).as_deref(), Some(b"old-token".as_ref()));
        assert_eq!(mem.get(LEGACY_SERVICE, a), None);
        // falha na gravação: token devolvido, legada preservada
        let mem = Mem {
            fail_write: true,
            ..Mem::new()
        };
        mem.seed(LEGACY_SERVICE, a, b"old-token");
        assert_eq!(load_with(&mem, a).unwrap().as_deref(), Some("old-token"));
        assert_eq!(
            mem.get(LEGACY_SERVICE, a).as_deref(),
            Some(b"old-token".as_ref())
        );
        // sem entrada em nenhum serviço
        assert_eq!(load_with(&Mem::new(), a).unwrap(), None);
        // forget remove as duas
        let mem = Mem::new();
        mem.seed(SERVICE, a, b"new");
        mem.seed(LEGACY_SERVICE, a, b"old");
        forget_with(&mem, a).unwrap();
        assert_eq!(mem.get(SERVICE, a), None);
        assert_eq!(mem.get(LEGACY_SERVICE, a), None);
        // save grava no novo e remove a legada
        let mem = Mem::new();
        mem.seed(LEGACY_SERVICE, a, b"old");
        save_with(&mem, a, "fresh").unwrap();
        assert_eq!(mem.get(SERVICE, a).as_deref(), Some(b"fresh".as_ref()));
        assert_eq!(mem.get(LEGACY_SERVICE, a), None);
    }

    #[test]
    #[ignore = "exige Secret Service real (gnome-keyring) na sessão D-Bus"]
    #[cfg(target_os = "linux")]
    fn secret_service_roundtrip_save_replace_load_forget() {
        let endpoint = "https://roundtrip.invalid/ai-monitor-test";
        let token = "synthetic-roundtrip-token";
        // semeia entrada legada e confirma migração na primeira leitura
        let service =
            secret_service::blocking::SecretService::connect(secret_service::EncryptionType::Dh)
                .unwrap();
        let collection = service.get_default_collection().unwrap();
        collection
            .create_item(
                "AI Monitor · Bearer legacy",
                std::collections::HashMap::from([
                    ("service", LEGACY_SERVICE),
                    ("account", account(endpoint).unwrap().as_str()),
                ]),
                b"legacy-token",
                true,
                "text/plain",
            )
            .unwrap();
        assert_eq!(load(endpoint).unwrap().as_deref(), Some("legacy-token"));
        let found = service
            .search_items(std::collections::HashMap::from([
                ("service", LEGACY_SERVICE),
                ("account", account(endpoint).unwrap().as_str()),
            ]))
            .unwrap();
        assert_eq!(
            found.unlocked.len() + found.locked.len(),
            0,
            "legacy must be deleted after migration"
        );
        forget(endpoint).unwrap();
        assert_eq!(load(endpoint).unwrap(), None);
        save(endpoint, token).unwrap();
        assert_eq!(load(endpoint).unwrap().as_deref(), Some(token));
        save(endpoint, token).unwrap();
        let service =
            secret_service::blocking::SecretService::connect(secret_service::EncryptionType::Dh)
                .unwrap();
        let found = service
            .search_items(std::collections::HashMap::from([
                ("service", SERVICE),
                ("account", account(endpoint).unwrap().as_str()),
            ]))
            .unwrap();
        assert_eq!(
            found.unlocked.len() + found.locked.len(),
            1,
            "save with replace must keep exactly one item"
        );
        forget(endpoint).unwrap();
        assert_eq!(load(endpoint).unwrap(), None);
        forget(endpoint).unwrap();
        assert!(save(endpoint, "   ").is_err());
    }

    #[test]
    #[ignore = "exige sessão gráfica: o desbloqueio do chaveiro precisa do prompt interativo do gnome-keyring"]
    #[cfg(target_os = "linux")]
    fn secret_service_locked_collection_is_unlocked_not_missed() {
        let endpoint = "https://locked.invalid/ai-monitor-test";
        let token = "synthetic-locked-token";
        save(endpoint, token).unwrap();
        let service =
            secret_service::blocking::SecretService::connect(secret_service::EncryptionType::Dh)
                .unwrap();
        let collection = service.get_default_collection().unwrap();
        collection.lock().unwrap();
        assert!(collection.is_locked().unwrap());
        assert_eq!(
            load(endpoint).unwrap().as_deref(),
            Some(token),
            "a match in a locked collection must be unlocked and returned, never a silent miss"
        );
        collection.lock().unwrap();
        forget(endpoint).unwrap();
        assert_eq!(load(endpoint).unwrap(), None);
    }

    #[test]
    #[ignore = "exige ausência de Secret Service; rode com DBUS_SESSION_BUS_ADDRESS inválido"]
    #[cfg(target_os = "linux")]
    fn secret_service_unavailable_errors_instead_of_silent_misses() {
        let endpoint = "https://unavailable.invalid/ai-monitor-test";
        for result in [load(endpoint), save(endpoint, "token").map(|_| None), {
            forget(endpoint).map(|_| None)
        }] {
            let Err(msg) = result else {
                panic!("unavailable Secret Service must error, not succeed");
            };
            assert!(msg.contains("indisponível"), "got: {msg}");
        }
    }
}
