//! O nível de confirmação de uma escrita. Módulo-folha: a manutenção, o
//! catálogo avançado e o serviço local o usam sem depender uns dos outros
//! por ele.

/// O que uma escrita pede antes de executar (Review.dc.html). Leitura não
/// pede nada: os `confirmation()` devolvem `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirmation {
    /// Um clique no botão final basta.
    Click,
    /// O botão final só habilita com este texto digitado, exato.
    Type(String),
}
