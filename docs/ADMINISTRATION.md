# Páginas ai-memory

As páginas do grupo ai-memory (Saúde, Explorar, Revisão, Manutenção e backup e
Administração) ficam no painel aberto pela barra de menus ou pela bandeja. O contrato inicial foi
validado contra o código e a documentação do
[ai-memory v3.3.1](https://github.com/akitaonrails/ai-memory/tree/v3.3.1).
O servidor deve expor a API de leitura (`--enable-api` ou `--enable-web`).
Nenhum servidor ai-memory, modelo, banco ou Docker é incluído no instalador.

## Saúde

A coleta de saúde roda em segundo plano a cada 30 segundos após terminar a
anterior, mesmo com a janela fechada. **Atualizar agora**, na página **Saúde**,
pede uma coleta fora desse ciclo: o coletor acorda na hora e coleta uma vez.
Cliques feitos antes dessa coleta começar viram um só, e o botão mostra
**Atualizando…** até ela chegar à tela; com a tela pausada, fica desabilitado.
Uma reconexão nesse meio-tempo descarta a coleta da conexão anterior. Usa `/identity`, `/admin/status`,
`/admin/metrics` e `/api/v1/docs/features/status`. Um `!` ao lado do indicador
na barra sinaliza falhas de consolidação/provider, fila de escrita ocupada em
80% ou mais, menos de 512 MiB de disco livre ou falha da consulta de saúde.
A dica do indicador informa o motivo. Leituras com mais de 90 segundos são
marcadas como atrasadas. Os limites são fixos nesta versão.

A página mostra o estado geral (saudável, com alertas, indisponível, leitura
atrasada ou aguardando) e cartões de captura, consolidação, fila de escrita,
providers com tokens reportados/estimados por atividade, armazenamento e
índices; métricas acumuladas e os dados brutos do servidor ficam recolhidos. São observações passivas do servidor:
abrir a tela não dispara testes pagos dos providers. Os contadores podem
reiniciar quando o processo ai-memory reinicia. O botão de pausa do painel
(`Pausar atualização da tela`) congela as medições; a barra continua
atualizando. Consultas manuais em **Explorar** continuam disponíveis.

As rotas administrativas podem exigir root/Admin. Uma chave pessoal `aim_`
continua com nível User, mesmo se pertencer a um administrador. Sem permissão,
a tela informa a recusa e mantém as funções de leitura autorizadas. Campos
não fornecidos aparecem como `—`, sem presumir zero ou saúde comprovada.

## Explorar: sessões detalhadas

Selecione o workspace/projeto exato e carregue sessões. A paginação consulta
50 registros por vez no servidor, incluindo opcionalmente sessões abertas.
Uma sessão aberta não prova que seu agente ainda está executando. A mesma
sessão pode aparecer em mais de um projeto: a timeline mostra somente eventos
do escopo selecionado. Pesquisa textual e tipo do evento são filtrados pelo
servidor antes da paginação; o corpo é solicitado com limite de 4.000 caracteres.

Tokens de entrada/saída/cache e modelo são exibidos quando reportados pelo
cliente. Totais da sessão não constituem fatura nem rateio entre projetos ou
modelos. A consolidação inclui estado e tentativas quando disponíveis.

O histórico de handoffs permite estados aberto, aceito e expirado. Usa somente
`/api/v1/.../handoffs`: **não chama o GET `/handoff` que pode consumir o baton**.
A API limita a lista a 200 itens, sem paginação; isso não representa todo o
histórico se houver mais registros. Resumos marcados `redacted` não são exibidos.

## Explorar: conhecimento

- Páginas e referências: lista com paginação local de 100 itens; abrir conteúdo
  não executa Markdown, HTML, scripts nem comandos nele encontrados. A prévia
  do corpo tem limite de 100 KiB e informa o corte. Referências entre projetos
  preservam suas próprias coordenadas de workspace/projeto.
- Busca: até 100 resultados. A busca do projeto pode acionar embeddings/reranker
  configurados no servidor; a global usa o índice textual. Refine a consulta
  para encontrar conteúdo fora do limite.
- Resumo: contagens, atividade recente, regras e páginas atualizadas.
- Histórico: 50 versões por página, seleção de duas versões e diff fornecido
  pelo servidor. O painel identifica diffs truncados ou grandes demais.
- Relações: grafo do projeto, limitado pelo ai-memory a 120 páginas; passe o
  mouse sobre um ponto ou abra a página na lista. Relações entre projetos
  mostram apenas arestas permitidas nas duas pontas pela API.

## Conexão, privacidade e limites

Cada consulta usa uma cópia da conexão e um identificador de geração. Ao salvar
outra conexão, resultados da anterior são descartados. Há somente um trabalhador
para consultas manuais, fila limitada, timeout de 12 segundos e resposta máxima
de 8 MiB. Redirects e proxies automáticos são desabilitados. Erros mostram a
classe do problema, sem reproduzir corpos do servidor ou credenciais.

Dados de sessões, páginas e relações ficam na memória do app. Não há telemetria
externa, exportação automática ou persistência de token fora da opção explícita de Keychain. Conteúdo da memória
pode ser privado: considere isso ao compartilhar a tela. O projeto público e
os testes usam somente dados sintéticos.

Falhas de rede, rotas desabilitadas e permissões são mostradas; não são contornadas pelo aplicativo. A execução por fases está em [IMPLEMENTATION.md](IMPLEMENTATION.md).


## Revisão e Manutenção e backup (v0.4)

As operações abaixo ficam em **Revisão** (propostas e handoffs) e em
**Manutenção e backup** (auditoria, curadoria, retenção, embeddings,
reconsolidação, feedback, backup e checkpoints). Selecione o projeto exato,
prepare a ação, examine a prévia/evidências e digite a confirmação solicitada.
Os campos ficam bloqueados enquanto há uma prévia aguardando decisão; **Cancelar
e editar** permite alterá-los. Trocar aba/projeto cancela a prévia. As duas
páginas usam o mesmo painel: com uma operação em andamento, uma prévia ou um
resultado na tela de uma delas, a outra não descarta nada sozinha; avisa e
oferece **Ir para** a página da operação ou **Descartar e abrir** a atual. Uma confirmação
expira em cinco minutos e não pode ser reaproveitada após mudar a conexão.
Salvar outra conexão fica indisponível durante uma operação administrativa.

| Área | Operações e limites |
| --- | --- |
| Propostas | Filtrar pendentes/aprovadas/rejeitadas/conflitos/falhas, até 200; detalhes por ID; evidências, corpo e comparação antes de aprovar/rejeitar. O servidor verifica conflito da página ao aprovar. |
| Handoffs | Histórico até 200, sem consumir; cancelamento de ID visível na lista de abertos; expiração por idade. Zero dias abrange todos os abertos próprios/compartilhados no projeto. |
| Auditoria | Prévia com `dry_run=true` e `no_llm=true`; aplicação grava o relatório, sem LLM. |
| Curadoria | Prévia somente leitura; aplicação cria proposta de relatório para revisão, sem autoaprovação. |
| Retenção | Prévia da política do servidor; aplicação pode remover/compactar páginas e eventos. Faça backup antes. |
| Embeddings | Prévia conta páginas sem chamar provider; aplicação no projeto selecionado, com opção de regenerar existentes. Pode gerar cobrança. |
| Reconsolidação | ID explícito de sessão, prévia de admissão, execução com LLM. A API não recebe workspace/projeto: o servidor decide o destino a partir da sessão. A seleção do painel é apenas contexto. Confirme `SESSAO <id>` após conferir esse limite. |
| Feedback | Útil/não ajudou/desatualizada/incorreta, caminho exato e motivo de até 500 caracteres. Aplica à versão atual; não há trava de versão oferecida por essa API. |
| Checkpoints | Lista os últimos 100 da wiki inteira; criar checkpoint global com descrição. A prévia da criação é um plano local, sem execução no servidor. |
| Restauração | Por ID de versão, compara a página atual com a versão escolhida; exige página atual legível. Por checkpoint Git, exige OID completo listado nos últimos 100; a API não fornece prévia do corpo dessa revisão. |

Prévia não é transação ou reserva do estado. Outros agentes podem alterar dados
antes da execução. A expiração de handoffs não tem dry-run nativo: a tela mostra
uma amostra de até 200 abertos e a idade escolhida; o servidor reavalia o conjunto
ao executar. Reconsolidação em dry-run não gera o texto final do LLM. Restauração
não oferece condição por versão atual na API: evite edições concorrentes.

Propostas dependem da política `require_approval` do servidor. O app não altera
essa configuração nem usa `memory_auto_improve` como se tivesse dry-run: essa
ferramenta rejeita os antigos campos de prévia. A revisão de propostas existentes
é oferecida independentemente de como elas foram criadas.

### Backup e exportação privada

Informe um **arquivo novo com caminho absoluto** e prepare o download. Confirme
`GLOBAL` para backup de toda a instalação, ou o projeto para exportar OKF.
O backup inclui SQLite consistente, wiki com histórico e `config.toml` da área
de dados; esse arquivo pode conter segredos. Não inclui todos os recursos externos
(modelos, raw, logs e credenciais externas). OKF contém conhecimento do projeto,
não substitui backup do banco. Checkpoint Git também não substitui esse backup.

O app cria o arquivo com permissão `0600`, recusa sobrescrita e remove downloads
incompletos quando ocorre falha. Limites: 2 GiB e cinco minutos; conteúdo é
transferido em blocos, sem carregar o arquivo inteiro na memória. A assinatura
gzip é conferida, mas isso **não é um teste de restauração**. A prévia do download
é local; não produz o backup até a confirmação. Nunca publique esses arquivos.

Restaurar um backup completo, resetar e reindexar continuam sendo operações
offline do ai-memory, com o serviço parado. A v0.4 restaura páginas individuais.

### Transporte e falhas administrativas

Escritas HTTP e chamadas MCP usam um trabalhador dedicado, fila limitada e
nenhuma repetição automática. Operações administrativas têm timeout de 120 s;
downloads, 300 s. Uma demora pode continuar sendo processada no servidor depois
do timeout ou de encerrar o app. **Consulte o estado antes de repetir.**

O cliente MCP suporta respostas JSON e SSE, inicialização de sessão quando
necessária, descarte ao trocar a conexão e encerramento do transporte stateful.
Cancelar uma prévia não cancela uma escrita já enviada. Resultados e formulários
permanecem em memória; só o download solicitado grava conteúdo em disco.

Relatórios são apresentados como campos e listas navegáveis. Textos acima de
100 KiB e estruturas muito profundas são sinalizados como parciais; consulte a
fonte completa antes de aprovar conteúdo que exceda esses limites.


## Administração (v0.5)

O catálogo fica na página **Administração**. Escolha a operação; os campos
indicam exatamente os parâmetros aceitos. O seletor de projeto preenche nomes
conhecidos, e campos editáveis permitem administrar instalações ainda sem projetos.
Consultas usam **Consultar**. Escritas exigem **Preparar prévia**, revisão dos
parâmetros e confirmação digitada do alvo. A conexão e outros grupos de escrita
ficam bloqueados enquanto a operação está em andamento.

| Área | Operações | Prévia e limites |
| --- | --- | --- |
| Projetos | Inventário, renomear, mover, mesclar, excluir | Mesclar/excluir têm dry-run do servidor. Renomear/mover consultam inventário e mostram plano, sem simulação transacional. Move mantém `force=false`, `on_conflict=block`; mesclar em destino existente exige `allow_lossy_merge=true` explícito. |
| Workspaces | Renomear, mesclar, excluir com conteúdo | Prévia somente GET, inclusive para excluir: servidores antigos ignoram dry_run e poderiam excluir workspace vazio. Mescla pode aplicar alguns projetos antes de falhar e pode perder histórico ao unir projetos homônimos. |
| Sessões | Mover uma sessão por ID | Dry-run com `confirm=false`; destino já existente, sem criar projeto nem forçar sessão ativa. Escolha mover páginas ou regenerá-las posteriormente. |
| Perfil | Estado, entradas, revisão, instruções e reconstrução | GET apply apenas devolve instruções; não modifica arquivos de agentes. Rebuild segue configuração global/workspace/usuário do servidor e pode chamar provider. |
| Usuários | Listar, criar, editar nome/e-mail/role, habilitar/desabilitar, redefinir senha | Role humano user/root; vazio em nome/e-mail mantém valor. Desabilitar login não revoga chaves de máquina. Senha temporária é retornada uma vez. |
| Permissões | Grants por usuário/projeto; conceder/revogar read/write; open/restricted | Consulte os grants antes de alterar. A API controla autorização e protege suas invariantes; o app não contorna recusas. |
| Chaves | Listar, criar, rotacionar e revogar | IDs explícitos; token novo oculto até revelar. Guarde-o antes de limpar/sair; não é persistido automaticamente no Keychain. |
| Mensagens | Inbox/outbox, enviar e cancelar por ID | GET não consome; até 200 itens, sem paginação oferecida por essa rota. Envio exige origem e destino; measured exige evidência. Cancelar sem ID é recusado para impedir cancelamento em massa. Enfileirar não inicia agente. |
| Workstreams | Recentes paginados, estado de run, eventos pesquisáveis | Bearer obrigatório, fingerprints reais de repositório/checkout; 50 por página com deslocamento numérico. Eventos até 100 por busca. Não envia heartbeat, accept, finish ou pop. |

As prévias de inventário **não garantem todos os efeitos de uma operação**.
Confira backup e a documentação do servidor para cascatas, dados de outros
projetos, perda de histórico e ações parcialmente aplicadas. Nada é excluído
para produzir essas prévias. A API reavalia autorização e estado ao executar.

### Autenticação e credenciais

Em **Ajustes**, salve o endereço e use Bearer ou login humano. Cookies humanos e
CSRF ficam em memória; falha/expiração não faz fallback silencioso para Bearer.
Para MCP/workstreams, saia explicitamente da sessão humana. Senha temporária
pode exigir troca antes de usar outras APIs. O Keychain é opcional e guarda
somente o Bearer, separado por URL/base path; não exporta segredos no JSON.
Alterar o endereço limpa os campos de autenticação antes de reconectar.

Resultados administrativos avançados ficam ocultos nos campos de segredo até
**Revelar credenciais neste resultado**. São removidos após cinco minutos,
ao trocar operação/conexão ou ao clicar **Limpar resultado privado**. Isso não
é garantia de zerar todas as cópias na RAM. Nenhum resultado é salvo em arquivo.

### Menu e acompanhamento

O menu tem os atalhos **Saúde**, **Revisão** e **Administração**; mostra handoffs abertos.
Selecione um projeto em **Ajustes → Propostas na barra de menus** para acompanhar
pendentes a cada ciclo de saúde, com a janela fechada. `P200+` significa que a
listagem atingiu o limite; `—` significa indisponível/atrasado, nunca zero presumido.
A contagem é do projeto e da identidade conectada, não um total global. Na
lateral do painel, o mesmo número aparece em âmbar ao lado de **Revisão** quando
há pendentes; sem leitura válida ou com zero, não aparece.

### Operação local

**Serviço deste Mac** oferece status, logs, iniciar, parar, reiniciar e atualizar.
Só funciona com conexão loopback. Configuração local é informada explicitamente
na tela e fica na memória; o app não infere qual serviço deve ser interrompido.

Docker exige contexto com socket Unix. A prévia fixa ID do contêiner, imagem e
labels; execução revalida o alvo. Atualização exige backup privado, Compose com
um único arquivo registrado e serviço com image explícita. Arquivo e configuração
resolvida (incluindo interpolação) são conferidos por SHA-256 antes e após pull;
o conteúdo resolvido não é mostrado nem persistido. Executa pull e up -d somente
no serviço, com no-deps/no-build/pull never; não executa prune, down ou remoção
de volumes. Stacks com overrides, build local ou dependências de ambiente do
terminal exigem operação pelo administrador. Configuração/serviço alterados
externamente durante o comando ainda podem causar corrida; não edite o stack
concomitantemente. Veja [Compose up](https://docs.docker.com/reference/cli/docker/compose/up/)
e [contextos](https://docs.docker.com/engine/manage-resources/contexts/).

LaunchAgent aceita o plist oficial, valida label, programa e comando serve,
e atua no domínio gui do usuário, sem sudo. Atualização exige serviço descarregado,
nenhum processo ai-memory ativo, backup privado e SHA-256 do novo binário. Usa
staging ao lado do executável e substituição atômica; conserva cópia privada do
binário anterior. O serviço permanece parado até **Iniciar**. Arquivos de manutenção
com symlink são recusados. Um digest confere bytes, não a autenticidade do fornecedor.

systemd (usuário, Linux) aceita a unit oficial `ai-memory.service`, valida
LoadState, FragmentPath, daemon-reload pendente e o ExecStart efetivo
(binário ai-memory absoluto + subcomando `serve`). Atua somente no gerenciador
do usuário, sem sudo; units de sistema estão fora de escopo. Atualização exige
serviço inativo/failed, nenhum processo ai-memory ativo, backup privado e
SHA-256 do novo binário. Usa o mesmo staging atômico do LaunchAgent e conserva
o binário anterior.

Logs são privados: Docker lê até 300 linhas de stdout/stderr (streams concatenados);
LaunchAgent lê até 150 de cada arquivo indicado no plist; systemd lê até 300
linhas do journal do usuário (`journalctl --user -u ai-memory.service`). Texto limitado a 100 KiB
por resultado/stream, sem exportação automática. Erros de comandos não reproduzem
stderr. Comandos têm prazo de 6–300 s, não repetem automaticamente e podem deixar
estado incerto após timeout: consulte status e saúde antes de repetir.

**Recuperação offline** apresenta roteiro para parar, conferir backup/config/pepper,
usar restore/reindex e validar após iniciar. Não há endpoint HTTP para restaurar
banco completo. O app não executa restore, reset, remoção de DB ou reindex por você.
Reindex exige banco vazio; não recupera sessões, handoffs, usuários, auditoria ou
outros dados que existem somente no banco. Preserve o original. Veja o
[restore upstream](https://github.com/akitaonrails/ai-memory/blob/v3.3.1/crates/ai-memory-cli/src/commands/restore.rs)
e o [reindex upstream](https://github.com/akitaonrails/ai-memory/blob/v3.3.1/crates/ai-memory-cli/src/commands/reindex.rs).

Importação e sincronização wiki com arquivos externos permanecem fora destas
três fases e requerem um projeto próprio de recuperação/conflitos.
