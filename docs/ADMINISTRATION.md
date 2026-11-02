# Central ai-memory

A Central fica no painel aberto pela barra de menus. O contrato inicial foi
validado contra o código e a documentação do
[ai-memory v3.3.1](https://github.com/akitaonrails/ai-memory/tree/v3.3.1).
O servidor deve expor a API de leitura (`--enable-api` ou `--enable-web`).
Nenhum servidor ai-memory, modelo, banco ou Docker é incluído no instalador.

## Saúde

A coleta de saúde roda em segundo plano a cada 30 segundos após terminar a
anterior, mesmo com a janela fechada. Usa `/identity`, `/admin/status`,
`/admin/metrics` e `/api/v1/docs/features/status`. Um `!` ao lado do indicador
na barra sinaliza falhas de consolidação/provider, fila de escrita ocupada em
80% ou mais, menos de 512 MiB de disco livre ou falha da consulta de saúde.
A dica do indicador informa o motivo. Leituras com mais de 90 segundos são
marcadas como atrasadas. Os limites são fixos nesta versão.

A tela mostra contagens, fila, providers, tokens reportados/estimados por
atividade, armazenamento e índices. São observações passivas do servidor:
abrir a tela não dispara testes pagos dos providers. Os contadores podem
reiniciar quando o processo ai-memory reinicia. `Pausar tela` congela as
medições do painel; a barra continua atualizando. Consultas manuais da Central
continuam disponíveis.

As rotas administrativas podem exigir root/Admin. Uma chave pessoal `aim_`
continua com nível User, mesmo se pertencer a um administrador. Sem permissão,
a tela informa a recusa e mantém as funções de leitura autorizadas. Campos
não fornecidos aparecem como `—`, sem presumir zero ou saúde comprovada.

## Sessões e handoffs

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

## Conhecimento

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
externa, exportação automática ou persistência de token. Conteúdo da memória
pode ser privado: considere isso ao compartilhar a tela. O projeto público e
os testes usam somente dados sintéticos.

Falhas de rede, rotas desabilitadas e permissões são mostradas; não são contornadas pelo aplicativo. A execução por fases está em [IMPLEMENTATION.md](IMPLEMENTATION.md).


## Administrar memória (v0.4)

As operações abaixo ficam em **Administrar memória**. Selecione o projeto exato,
prepare a ação, examine a prévia/evidências e digite a confirmação solicitada.
Os campos ficam bloqueados enquanto há uma prévia aguardando decisão; **Cancelar
e editar** permite alterá-los. Trocar aba/projeto cancela a prévia. Uma confirmação
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
