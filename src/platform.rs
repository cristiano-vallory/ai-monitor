//! Textos visíveis que dependem do sistema operacional.

#[cfg(target_os = "macos")]
pub const HOST: &str = "Mac";
#[cfg(not(target_os = "macos"))]
pub const HOST: &str = "computador";

#[cfg(target_os = "macos")]
pub const OS: &str = "macOS";
#[cfg(not(target_os = "macos"))]
pub const OS: &str = "Linux";

#[cfg(target_os = "macos")]
pub const SECRET_STORE: &str = "Keychain";
#[cfg(not(target_os = "macos"))]
pub const SECRET_STORE: &str = "chaveiro do sistema";

#[cfg(target_os = "macos")]
pub const DOCKER_CONTEXT_LABEL: &str = "Contexto Docker local (ex.: desktop-linux)";
#[cfg(not(target_os = "macos"))]
pub const DOCKER_CONTEXT_LABEL: &str = "Contexto Docker local (ex.: default)";

#[cfg(target_os = "macos")]
pub const DOCKER_COMMAND_ERROR: &str =
    "Comando Docker falhou. Verifique o Docker Desktop e o contexto ativo.";
#[cfg(not(target_os = "macos"))]
pub const DOCKER_COMMAND_ERROR: &str = "Comando Docker falhou. Verifique o serviço Docker, o acesso ao socket (grupo docker) e o contexto ativo.";

#[cfg(target_os = "macos")]
pub const PROCESSES_CAPTION: &str = "Processos do Docker Desktop e clientes ai-memory no macOS. RSS pode compartilhar páginas; não some ao consumo dos contêineres.";
#[cfg(not(target_os = "macos"))]
pub const PROCESSES_CAPTION: &str = "Processos do Docker Engine (dockerd, containerd e shims) e do ai-memory nativo deste computador. Processos dentro de contêineres ficam de fora: aparecem na aba de contêineres. RSS pode compartilhar páginas; não some ao consumo dos contêineres.";

#[cfg(target_os = "macos")]
pub const LOCAL_SERVICE_HINT: &str = "Configure explicitamente o serviço. Docker usa contexto com socket Unix e fixa o ID revisado. LaunchAgent aceita o plist oficial; atualização nativa exige serviço parado, binário novo e SHA-256. Campos não são gravados em disco.";
#[cfg(not(target_os = "macos"))]
pub const LOCAL_SERVICE_HINT: &str = "Configure explicitamente o serviço. Docker usa contexto com socket Unix e fixa o ID revisado. Campos não são gravados em disco.";
