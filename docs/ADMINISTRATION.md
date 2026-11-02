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

A v0.3 é de inspeção: propostas, manutenção e administração entram nas próximas
fases descritas em [IMPLEMENTATION.md](IMPLEMENTATION.md). Falhas de rede, rotas
desabilitadas e permissões são mostradas; não são contornadas pelo aplicativo.
