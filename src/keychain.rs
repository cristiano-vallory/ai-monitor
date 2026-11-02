//! Opt-in, endpoint-specific Bearer storage. Never stores human passwords/cookies.
const SERVICE: &str = "io.github.cristianovallory.ai-monitor.bearer";
fn account(endpoint: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(endpoint).map_err(|_| "URL inválida para o Keychain.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("URL inválida para o Keychain.".into());
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.to_string())
}
pub fn load(endpoint: &str) -> Result<Option<String>, String> {
    match security_framework::passwords::get_generic_password(SERVICE, &account(endpoint)?) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "Entrada do Keychain inválida.".into()),
        Err(e) if e.code() == -25300 => Ok(None),
        Err(_) => Err(
            "O Keychain não permitiu ler o token. Autorize o app ou informe-o manualmente.".into(),
        ),
    }
}
pub fn save(endpoint: &str, token: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err("Informe um token para guardar no Keychain.".into());
    }
    security_framework::passwords::set_generic_password(
        SERVICE,
        &account(endpoint)?,
        token.as_bytes(),
    )
    .map_err(|_| "O Keychain não permitiu guardar o token.".into())
}
pub fn forget(endpoint: &str) -> Result<(), String> {
    match security_framework::passwords::delete_generic_password(SERVICE, &account(endpoint)?) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == -25300 => Ok(()),
        Err(_) => Err("O Keychain não permitiu remover o token.".into()),
    }
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
}
