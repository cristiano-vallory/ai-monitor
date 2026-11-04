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

Na linha de ferramentas, escolha o workspace/projeto exato e, no segmentado,
**Sessões detalhadas**; depois use **Carregar sessões**. A paginação (**Anterior**
e **Próxima**, com o intervalo “N–M”) consulta 50 registros por vez no servidor,
incluindo opcionalmente sessões abertas (**Incluir abertas**).
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

As operações abaixo ficam em **Revisão** (abas **Propostas** e **Handoffs**) e em
**Manutenção e backup** (abas **Manutenção** e **Backup**: auditoria, curadoria,
retenção, embeddings, reconsolidação, feedback, backup e checkpoints). Na linha
de ferramentas, escolha o projeto exato e a aba. Leituras (listas, **Ver
detalhes**, **Carregar histórico**, **Listar últimos 100 checkpoints**) rodam
direto. Uma escrita abre a prévia numa folha de confirmação, descrita em
[Confirmação em níveis](#confirmação-em-níveis). Enquanto a folha está aberta, o
projeto, as abas e os campos ficam sob o véu; **Cancelar** (ou Esc) a fecha e
libera os campos, que mantêm o que foi preenchido. Um resultado, um erro ou o
aviso de prévia vencida ficam na página até **Limpar**, trocar de aba ou de
projeto. Depois de aplicar uma escrita, o cartão **Resultado** traz o aviso
“Resposta recebida. Confira os detalhes abaixo.”. As duas páginas usam o mesmo
painel: com uma operação em andamento, uma prévia ou um resultado na tela de
uma delas, a outra não descarta nada sozinha; avisa e oferece **Ir para** a
página da operação ou **Descartar e abrir** a atual. Uma confirmação expira em
cinco minutos e não pode ser reaproveitada após mudar a conexão. Salvar outra
conexão fica indisponível durante uma operação administrativa.

**Gerenciar em Revisão →**, no detalhe de um handoff em **Sessões e handoffs**,
abre Revisão · Handoffs com o projeto e o handoff selecionados. Se o painel
estiver preso a uma operação, prévia ou resultado ainda na tela, nada muda e a
página Revisão mostra o que o prende.

| Área | Operações e limites |
| --- | --- |
| Propostas | A lista carrega sozinha ao escolher o projeto ou o estado (pendentes/aprovadas/rejeitadas/em conflito/com falha) e ao abrir a aba, até 200. Ao selecionar uma proposta (clique na linha, ou ID digitado e confirmado com Enter, Tab ou clique fora, inclusive um ID fora da lista), o detalhe é buscado sozinho (GET, só leitura) e mostra “Proposta <estado> · criada por <agente> há N”, o título, a justificativa e as evidências. O campo de ID e **Ver detalhes** ficam no pé da coluna da lista; **Ver detalhes** mostra a resposta inteira no cartão **Resultado**. Na tela estreita, o campo de ID fica no pé da lista e Enter abre o detalhe. **Atualizar** relê a lista e o detalhe selecionado; depois de aprovar ou rejeitar com sucesso, os dois são relidos sozinhos (com falha, nada é relido). **Rejeitar…** e **Aprovar…** ficam na linha do título do detalhe, e o motivo da rejeição, no pé do detalhe. A folha traz o alvo, o efeito, se é reversível (só em Aprovar) e a comparação entre a página atual e a proposta; com uma resposta fora do formato esperado, mostra os valores recebidos. O servidor verifica conflito da página ao aprovar. |
| Handoffs | A lista mostra os abertos da última coleta do monitor, sem consulta nova; **Carregar histórico** consulta até 200, sem consumir. **Cancelar handoff…** vale para um ID visível na lista de abertos; **Expirar antigos…** usa a idade mínima escolhida. Zero dias abrange todos os abertos próprios/compartilhados no projeto. |
| Auditoria | Prévia com `dry_run=true` e `no_llm=true`; aplicação grava o relatório, sem LLM. |
| Curadoria | Prévia somente leitura; aplicação cria proposta de relatório para revisão, sem autoaprovação. |
| Retenção | Prévia da política do servidor; aplicação pode remover/compactar páginas e eventos. Faça backup antes. |
| Embeddings | Prévia conta páginas sem chamar provider; aplicação no projeto selecionado, com opção de regenerar existentes. Pode gerar cobrança. |
| Reconsolidação | ID explícito de sessão, prévia de admissão, execução com LLM. A API não recebe workspace/projeto: o servidor decide o destino a partir da sessão. A seleção do painel é apenas contexto. A folha confirma com um clique (**Reconsolidar**) e repete esse limite no subtítulo e no aviso: confira-o antes de confirmar. |
| Feedback | Útil/não ajudou/desatualizada/incorreta, caminho exato e motivo de até 500 caracteres. Aplica à versão atual; não há trava de versão oferecida por essa API. |
| Checkpoints | Lista os últimos 100 da wiki inteira; criar checkpoint global com descrição; a folha mostra “Toda a wiki” como alvo. A prévia da criação é um plano local, sem execução no servidor. |
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

### Confirmação em níveis

A folha de confirmação vale para toda escrita em **Revisão**, **Manutenção e
backup** e **Administração**. O nível depende da operação:

| Nível | Operações | O que a folha pede |
| --- | --- | --- |
| Leitura | Listas, detalhes, inventários, consultas, logs e roteiro de recuperação offline | Nada: não há prévia nem folha. |
| Clique | A maior parte das escritas, inclusive backup e checkpoint globais e reconsolidação | O botão final, com o verbo da ação (**Aprovar**, **Restaurar**, **Iniciar**…). |
| Digitação | Só as destrutivas, sem desfazer | O botão final, em vermelho, só habilita com o nome do alvo digitado. |

O que digitar:

| Onde | Operações | Digite |
| --- | --- | --- |
| Revisão · Handoffs | **Cancelar handoff…**, **Expirar antigos…** | o projeto, como `workspace / projeto` |
| Manutenção | **Aplicar retenção** | o projeto, como `workspace / projeto` |
| Administração · Projetos | Mover, mesclar, excluir | o nome do projeto |
| Administração · Workspaces | Mesclar, excluir | o nome do workspace |
| Administração · Sessões | Mover uma sessão | o ID da sessão |
| Administração · Perfil | Reconstruir | `GLOBAL` |
| Administração · Usuários | Desabilitar login, redefinir senha | o nome do usuário |
| Administração · Permissões | Revogar | o nome do usuário |
| Administração · Permissões | Alterar acesso ao projeto | o nome do projeto |
| Administração · Chaves | Rotacionar, revogar | o ID da credencial |
| Administração · Mensagens | Cancelar por ID | o ID da mensagem |
| Serviço local | Parar, atualizar | o nome do contêiner (Docker) ou `ai-memory` (LaunchAgent, systemd) |

O texto digitado é comparado por igualdade exata: maiúsculas, espaços (inclusive
no fim) e a barra de `workspace / projeto` contam, e não há prefixo. A folha
mostra **Digite … para confirmar** com o texto pedido.

A folha desce do topo da área de conteúdo, sobre um véu; a lateral e a barra
superior seguem clicáveis. Mostra a pergunta como título, o projeto (ou
**Servidor · administração**, ou **Serviço deste Mac**), os fatos **Operação** e
**Alvo** (mais **Efeito** e **Reversível** em Aprovar; só **Efeito** em Rejeitar;
**Motivo** em Rejeitar e Feedback), a prévia (**Prévia da alteração** em Revisão e Manutenção
e backup; **Prévia de impacto** na Administração, sempre com credenciais
ocultas), a faixa **Atenção.** com o aviso da operação e, com digitação, o
campo. No rodapé, **Prévia válida por m:ss**, **Cancelar** e o botão com o verbo
da ação.

A prévia vale 5 minutos, com contagem regressiva. Ao chegar a zero, a folha
fecha e a operação precisa ser preparada de novo; em Revisão e Manutenção e
backup, o aviso fica na página até **Limpar**. **Cancelar** (ou Esc) descarta a
prévia e o texto digitado, sem enviar nada.

### Backup e exportação privada

No segmentado, escolha **Backup global** ou **Exportar projeto (OKF)**, informe
um **arquivo novo com caminho absoluto** e use **Revisar download privado**. A folha mostra
o alvo (“Toda a instalação” no backup global; o projeto no OKF) e confirma com
um clique (**Salvar backup** ou **Exportar**).
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

O catálogo fica na página **Administração**. Na linha de ferramentas, o
segmentado alterna entre **Servidor · administração** e **Serviço deste Mac** (no
Linux, **Serviço deste computador**). No servidor, escolha o **Grupo** (Projetos,
Workspaces, Sessões, Perfil, Usuários, Permissões, Chaves, Mensagens ou
Workstreams) e depois a **Operação**; os campos indicam exatamente os parâmetros
aceitos, e os opcionais trazem a dica “Opcional”. Nas operações com workspace, o
seletor **Preencher a partir de um projeto conhecido…** preenche nomes
conhecidos, e campos editáveis permitem administrar instalações ainda sem projetos.
Consultas usam **Consultar**. Escritas usam **Prévia…**, que abre a folha de
confirmação com a prévia de impacto; o nível (clique ou digitação do alvo) está em
[Confirmação em níveis](#confirmação-em-níveis). A conexão e outros grupos de
escrita ficam bloqueados enquanto a operação está em andamento.

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

Em **Ajustes → Conexão**, salve o endereço e informe o Bearer (**Token**); em
**Ajustes → Conta**, entre com sessão humana. Cookies humanos e
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

O menu da barra (no Linux, da bandeja) tem os atalhos **Saúde do ai-memory** e
**Revisar propostas…**; a linha **N handoffs abertos** abre **Sessões e handoffs**,
e a de propostas, **Revisão**. Com alertas de saúde, o menu mostra uma faixa âmbar
e o ícone, um ponto âmbar (desligável em Ajustes). Selecione um projeto em
**Ajustes → Barra de menus** (no Linux, **Ajustes → Bandeja**), no seletor
**Propostas** (legenda "Um projeto, até 200 pendentes. Contagem aparece no menu."),
para acompanhar pendentes a cada ciclo de saúde, com a janela fechada. `200+`
significa que a listagem atingiu o limite; `—` significa indisponível/atrasado,
nunca zero presumido.
A contagem é do projeto e da identidade conectada, não um total global. Na
lateral do painel, o mesmo número aparece em âmbar ao lado de **Revisão** quando
há pendentes; sem leitura válida ou com zero, não aparece.

### Operação local

**Serviço deste Mac** (no Linux, **Serviço deste computador**) oferece status,
logs, iniciar, parar, reiniciar e atualizar. Status, logs e recuperação offline
são leituras; iniciar e reiniciar confirmam com um clique; parar e atualizar
pedem que se digite o nome do serviço. Só funciona com conexão loopback.
Configuração local é informada explicitamente na tela e fica na memória; o app
não infere qual serviço deve ser interrompido.

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
