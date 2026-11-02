# Administração do ai-memory — execução por fases

Escopo autorizado em 02/11/2026: implementar até a v0.5 em PRs, com testes,
code review e bug review a cada PR; corrigir os achados antes de integrar.
Contrato de referência: ai-memory v3.3.1, commit
`1c506228b701f4496d3d9af6f00180749e0563d1`.

## PR / v0.3 — Saúde e inspeção

- Cliente HTTP limitado, sem redirects/proxy, erros sem conteúdo sensível.
- Saúde: identidade/permissões, providers, filas, índices, armazenamento,
  métricas e alertas na barra de menus, independentes da janela.
- Sessões paginadas e timeline pesquisável; tokens/cache/modelo e consolidação.
- Wiki: páginas, busca, briefing, histórico/diff e relações.
- Recursos ausentes ou negados aparecem como indisponíveis, nunca zero.

## PR / v0.4 — Revisão e manutenção

- Propostas: listagem, evidências, diff, aprovar/rejeitar.
- Handoffs: histórico, cancelar por ID, expirar por idade com confirmação.
- Lint, curadoria, retenção e embeddings com prévia quando a API suporta;
  reconsolidação por ID de sessão e feedback de memória.
- Backup privado, exportação OKF, checkpoints e restauração de página.
- Escritas em trabalhador separado, sem retry automático; confirmação vinculada
  à conexão/escopo/objeto e cancelada ao trocar a conexão.

## PR / v0.5 — Administração avançada

- Projetos/workspaces: renomear, mover, mesclar, excluir com prévia de impacto;
  mover sessões e administrar o perfil compartilhado.
- Usuários, grants, senhas e chaves; login humano/CSRF e Keychain opcional.
- Inbox: leitura não consumidora, envio/cancelamento; workstreams e eventos.
- Operação local explicitamente configurada: Docker ou LaunchAgent, logs,
  iniciar/parar/reiniciar, atualização e recuperação offline orientada.
- Documentar limites de servidor remoto e ações que dependem de configuração.

## Critério por PR

1. Formatação, testes de comportamento e Clippy sem warnings.
2. Code review: autenticação, escopo, concorrência, segredos e manutenção.
3. Bug review: estados vazios/parciais, negação de acesso, troca de conexão,
   paginação, duplicação, falhas de rede e ações destrutivas.
4. Corrigir achados e registrar evidências em `docs/reviews/`; repetir checks
   afetados. Revisões locais devem ser identificadas como tal, sem alegar
   revisão independente ou homologação real que não ocorreu.
5. CI aprovado no HEAD do PR; merge verificado; DMG Universal e smoke de
   instalação. Release sem credenciais/configurações/dados da instalação.

Assinatura Apple permanece dependente de certificado Developer ID e credenciais
de notarização. As releases continuam ad hoc enquanto isso não estiver disponível.
Testes administrativos usam fixtures sintéticas; não executar manutenção,
exclusão, atualização ou reinício na memória real para validar funcionalidades.

## Estado

- v0.3: concluída. PR #2 integrado; release v0.3.0 publicada, download público verificado.
- v0.4: concluída. PR #3 integrado; release v0.4.0 publicada, download público verificado.
- v0.5: implementação concluída com testes, revisões locais e instalador validados. Evidências em `docs/reviews/v0.5.0.md`; integração e distribuição rastreadas no PR e na release v0.5.0.

## Port Linux — fases

Decisões fechadas: no Linux o app mostra apenas ícone na bandeja (sem texto
na barra). Alvos: Omarchy 4 (Arch + Hyprland; barra Quickshell com system
tray) e Ubuntu ≥ 22.04 (extensão AppIndicator, ativa por padrão).
Arquiteturas x86_64 e aarch64. Cada fase é testada com o artefato do CI do
PR na VM Omarchy aarch64 do usuário; não há VM Ubuntu — Ubuntu é coberto
somente pelo CI. Toda diferença de plataforma fica atrás de
`cfg`/constantes em `src/platform.rs`; o comportamento macOS é preservado.
Segredos no Linux usam Secret Service (crate `secret-service`, runtime
async-io e criptografia em Rust puro, reutilizando o zbus 5 já resolvido);
não existe fallback em texto puro.

- Fase 1 — **concluída (PR #7)**: compila e executa como janela normal no
  Linux; CI Linux (fmt, testes, clippy, integração Secret Service real,
  build, smoke sob Xvfb, artefato por PR).
- Fase 2 — **concluída (PR #8)**: tray via StatusNotifierItem (ksni), ícone
  apenas. No Wayland uma janela não pode ser ocultada (`set_visible` do
  winit é no-op): fechar destrói a janela e reabrir a recria, com a coleta
  contínua. Verificado headless sob Xvfb e sway headless (abrir/fechar em
  ciclos, segunda instância, quit). O teste do usuário na VM Omarchy
  acontece ao fim das 4 fases.
- Fase 3 — **concluída (PR #9)**: backend systemd `--user` para
  `ai-memory.service` (a unit que `ai-memory setup` instala no Linux), em
  substituição ao LaunchAgent.
- Fase 4 — **concluída (PR #10, release v0.6.0)**: `.deb` (Ubuntu) +
  `PKGBUILD` (Omarchy/Arch) + pipeline de release. Validação final na VM
  Omarchy pendente (decisão do usuário: testar ao fim das 4 fases).
- v0.6.1 — **em andamento (este PR)**: migração de identificadores para
  `cristiano_vallory`/`cristiano-vallory` e correções acumuladas do port.
