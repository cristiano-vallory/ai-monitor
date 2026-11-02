//! Opt-in, endpoint-specific Bearer storage. Never stores human passwords/cookies.
use crate::platform::SECRET_STORE;

const SERVICE: &str = "io.github.cristianovallory.ai-monitor.bearer";
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
pub fn load(endpoint: &str) -> Result<Option<String>, String> {
    match security_framework::passwords::get_generic_password(SERVICE, &account(endpoint)?) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| format!("Entrada do {SECRET_STORE} inválida.")),
        Err(e) if e.code() == -25300 => Ok(None),
        Err(_) => Err(format!(
            "O {SECRET_STORE} não permitiu ler o token. Autorize o app ou informe-o manualmente."
        )),
    }
}
#[cfg(target_os = "macos")]
pub fn save(endpoint: &str, token: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err(format!("Informe um token para guardar no {SECRET_STORE}."));
    }
    security_framework::passwords::set_generic_password(
        SERVICE,
        &account(endpoint)?,
        token.as_bytes(),
    )
    .map_err(|_| format!("O {SECRET_STORE} não permitiu guardar o token."))
}
#[cfg(target_os = "macos")]
pub fn forget(endpoint: &str) -> Result<(), String> {
    match security_framework::passwords::delete_generic_password(SERVICE, &account(endpoint)?) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == -25300 => Ok(()),
        Err(_) => Err(format!("O {SECRET_STORE} não permitiu remover o token.")),
    }
}

#[cfg(target_os = "linux")]
mod store {
    use super::{SECRET_STORE, SERVICE, account};
    use secret_service::{EncryptionType, blocking::SecretService};
    use std::collections::HashMap;

    const UNAVAILABLE: &str =
        "Chaveiro do sistema (Secret Service) indisponível. Informe o token manualmente.";

    fn connect() -> Result<SecretService<'static>, String> {
        SecretService::connect(EncryptionType::Dh).map_err(|_| UNAVAILABLE.to_owned())
    }
    fn attributes(account: &str) -> HashMap<&str, &str> {
        HashMap::from([("service", SERVICE), ("account", account)])
    }
    fn unlocked_items<'a>(
        service: &'a SecretService<'a>,
        account: &'a str,
        error: impl Fn() -> String,
    ) -> Result<Vec<secret_service::blocking::Item<'a>>, String> {
        let mut found = service
            .search_items(attributes(account))
            .map_err(|_| error())?;
        if !found.locked.is_empty() {
            let locked: Vec<_> = found.locked.iter().collect();
            service.unlock_all(&locked).map_err(|_| error())?;
            // Items that were locked only move to `unlocked` in a fresh
            // search; a match that stayed locked is an error, never a miss.
            found = service
                .search_items(attributes(account))
                .map_err(|_| error())?;
            if !found.locked.is_empty() {
                return Err(error());
            }
        }
        Ok(found.unlocked)
    }

    pub fn load(endpoint: &str) -> Result<Option<String>, String> {
        let account = account(endpoint)?;
        let error = || {
            format!(
                "O {SECRET_STORE} não permitiu ler o token. Autorize o app ou informe-o manualmente."
            )
        };
        let service = connect()?;
        let items = unlocked_items(&service, &account, error)?;
        let Some(item) = items.into_iter().next() else {
            return Ok(None);
        };
        let secret = item.get_secret().map_err(|_| error())?;
        String::from_utf8(secret)
            .map(Some)
            .map_err(|_| format!("Entrada do {SECRET_STORE} inválida."))
    }
    pub fn save(endpoint: &str, token: &str) -> Result<(), String> {
        if token.trim().is_empty() {
            return Err(format!("Informe um token para guardar no {SECRET_STORE}."));
        }
        let account = account(endpoint)?;
        let error = || format!("O {SECRET_STORE} não permitiu guardar o token.");
        let service = connect()?;
        let collection = service.get_default_collection().map_err(|_| error())?;
        if collection.is_locked().map_err(|_| error())? {
            collection.unlock().map_err(|_| error())?;
        }
        collection
            .create_item(
                &format!("AI Monitor · Bearer {account}"),
                attributes(&account),
                token.as_bytes(),
                true,
                "text/plain",
            )
            .map(|_| ())
            .map_err(|_| error())
    }
    pub fn forget(endpoint: &str) -> Result<(), String> {
        let account = account(endpoint)?;
        let error = || format!("O {SECRET_STORE} não permitiu remover o token.");
        let service = connect()?;
        for item in unlocked_items(&service, &account, error)? {
            item.delete().map_err(|_| error())?;
        }
        Ok(())
    }
}
#[cfg(target_os = "linux")]
pub fn load(endpoint: &str) -> Result<Option<String>, String> {
    store::load(endpoint)
}
#[cfg(target_os = "linux")]
pub fn save(endpoint: &str, token: &str) -> Result<(), String> {
    store::save(endpoint, token)
}
#[cfg(target_os = "linux")]
pub fn forget(endpoint: &str) -> Result<(), String> {
    store::forget(endpoint)
}

#[cfg(test)]
mod tests {
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

    #[test]
    #[ignore = "exige Secret Service real (gnome-keyring) na sessão D-Bus"]
    #[cfg(target_os = "linux")]
    fn secret_service_roundtrip_save_replace_load_forget() {
        let endpoint = "https://roundtrip.invalid/ai-monitor-test";
        let token = "synthetic-roundtrip-token";
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
