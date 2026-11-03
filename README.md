# AI Monitor

Monitor nativo em **Rust para macOS**, com CPU e memória do **ai-memory** e do **Docker diretamente na barra de menus**. A coleta continua com a janela fechada; o painel reúne processos, threads, sessões, handoffs abertos, contêineres e imagens.

Projeto independente, sem afiliação oficial com ai-memory, Docker ou Apple. Interface em português brasileiro. Licença [MIT](LICENSE).

**[Baixar o instalador .dmg](https://github.com/cristiano-vallory/ai-monitor/releases/latest)** · **[Instalação e dependências](INSTALL.md)** · **[Segurança e privacidade](SECURITY.md)**

Linux (Ubuntu ≥ 22.04 e Omarchy 4): consulte a [seção Linux do guia de instalação](INSTALL.md#linux-ubuntu--2204-e-omarchy-4).

## Instalar sem compilar Rust

A versão **0.6.2** passa o intervalo padrão de sessões/handoffs para 60 s, reduzindo a carga constante no servidor ai-memory. Desde a 0.6.0 há suporte a Linux (Ubuntu ≥ 22.04 e Omarchy 4): bandeja de sistema, painel sob demanda, backend systemd de usuário e pacotes `.deb`/`PKGBUILD`. Veja as [notas da atualização](docs/releases/v0.6.2.md) e o [guia das páginas ai-memory](docs/ADMINISTRATION.md).

1. Baixe **AI-Monitor-0.6.2-macos-universal.dmg** na [release v0.6.2](https://github.com/cristiano-vallory/ai-monitor/releases/tag/v0.6.2).
2. Abra o `.dmg` e arraste **AI Monitor.app** para **Applications / Aplicativos**.
3. Ejete o disco, abra o app em Aplicativos e procure **AI / Docker** na barra de menus.
4. Escolha **Abrir painel** para configurar o servidor e o contêiner.

O pacote Universal contém código nativo para **Apple Silicon e Intel**. Não exige Rust, Cargo, Xcode ou ferramentas de desenvolvimento para instalar ou executar. O mínimo declarado é macOS 12.0. A interface foi validada em Apple Silicon; o executável Intel foi verificado via Rosetta, sem validação em hardware Intel ou macOS antigos.

Esta release possui **assinatura ad hoc, sem Developer ID nem notarização Apple**. Se o macOS bloquear por desenvolvedor não verificado, após tentar abrir use **Ajustes do Sistema → Privacidade e Segurança → Abrir Mesmo Assim**, confirmando a origem. Consulte o [guia de instalação](INSTALL.md) e a [orientação da Apple](https://support.apple.com/en-us/102445). A release inclui checksum SHA-256; não é necessário desativar o Gatekeeper globalmente.

## Compilar e desenvolver (opcional)

Com Rust estável, Xcode Command Line Tools e Git instalados:

```sh
git clone https://github.com/cristiano-vallory/ai-monitor.git
cd ai-monitor
bash scripts/build-app.sh
open 'dist/AI Monitor.app'
```

O app inicia na barra de menus. Clique no indicador e escolha **Abrir painel**. O [guia completo](INSTALL.md) explica como instalar as ferramentas, configurar as fontes, copiar o app para Aplicativos, atualizar e desinstalar.

O script procura Cargo em `~/.cargo/bin/cargo`; também aceita `CARGO_BIN`. Sem argumentos, gera a arquitetura do Mac que compila. `bash scripts/build-app.sh --universal` gera as duas arquiteturas; `bash scripts/build-dmg.sh` também cria o instalador e seu checksum. Veja [como gerar uma release](INSTALL.md#gerar-o-instalador-para-distribuição).

Docker Desktop e o servidor ai-memory são fontes externas, instaladas separadamente. Não são necessários para compilar nem executar os testes automatizados. Sem uma fonte, os dados correspondentes aparecem indisponíveis.

Para desenvolvimento e diagnóstico:

```sh
cargo run --locked
cargo run --locked -- --show-dashboard
cargo run --locked -- --diagnose
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

`--diagnose` consulta fontes reais e imprime recursos e contagens, sem resumos de handoffs nem tokens. Nomes de processos, contêineres, imagens, portas e projetos em mensagens de erro podem fazer parte da saída; revise antes de compartilhar. Capturas de tela do painel também podem revelar conteúdo privado.

`--trace-menu` emite até 120 leituras do indicador AppKit, visibilidade da janela e quantidade de amostras históricas no terminal, para verificar a atualização em segundo plano. Pode ser combinado com `--show-dashboard`; não registra conteúdo de sessões nem credenciais.

## Barra de menus do macOS

Ao iniciar, o app fica na barra de menus, sem abrir o painel nem ocupar o Dock. Exemplo ilustrativo, sem dados de uma instalação real: `AI 35% 512M | Docker 62% 1.5G`. São CPU e memória do ai-memory, seguidos da **soma dos contêineres ativos, incluindo ai-memory quando executado nesse Docker**. `M` e `G` representam MiB e GiB. O menu e a dica ao passar o mouse mostram os valores completos e a idade da última coleta.

O indicador AI prioriza o contêiner configurado. Quando ele não está disponível, o coletor pode usar processos nativos identificados como ai-memory. Os números dos processos Docker Desktop no macOS ficam separados no painel.

- **Saúde / Revisão / Administração:** atalhos diretos para essas páginas. O menu mostra handoffs abertos e propostas pendentes do projeto escolhido em Ajustes (`P200+` sinaliza o limite da lista).
- **Abrir painel:** mostra a janela e restaura uma janela minimizada.
- **Ocultar painel:** recolhe a janela e mantém a coleta ativa. O botão vermelho de fechar tem o mesmo comportamento.
- **Sair do AI Monitor:** encerra o monitor e remove o indicador da barra.

A barra é atualizada por um timer AppKit no loop principal, independente da renderização do painel, inclusive durante a abertura do menu. O botão de pausa do painel (`Pausar atualização da tela`) congela somente o painel; a barra continua acompanhando as fontes. O histórico de dez minutos é coletado em segundo plano e aparece ao reabrir a janela. Amostras atrasadas por mais de três intervalos (mínimo de dez segundos) são substituídas por `—`, sem aparentar uma medição atual. Falhas do Docker também aparecem como indisponíveis, sem virar zero.

Uma atividade nativa evita a suspensão da coleta por App Nap sem impedir o repouso normal do Mac. Durante o repouso não há coleta. A inicialização automática ao entrar no macOS não é configurada pelo app; ele pode ser adicionado manualmente aos Itens de Início após a instalação.

## Painéis

- **Visão geral:** faixa de saúde com link para **Saúde**; indicadores de CPU e memória do ai-memory (com média, pico e minigráfico da janela), contêineres ativos e o consumo dos demais, e continuidade (handoffs abertos, sessões abertas informadas pela API e, com um projeto vigiado, propostas pendentes); gráfico de recursos com ai-memory e outros contêineres empilhados, em CPU ou memória, nas janelas de 1, 5 ou 10 minutos (histórico mantido em memória), com os valores de cada amostra ao passar o mouse; contêineres em execução (o clique abre o contêiner no **Docker**) e os quatro handoffs mais recentes. Sem dado, os números aparecem como `—`, nunca como zero.
- **Sessões e handoffs:** lista de handoffs abertos agrupada por projeto, com o detalhe ao lado (ou no lugar da lista, em janelas estreitas): resumo, próximos passos, cópia do ID e do resumo. Mensagens entre sessões e relatos de subagente aparecem limpos, com uma faixa que identifica a origem e mostra o texto bruto. sessões abertas, com o total informado pela API no contador e as últimas 100 por projeto na tabela (agente, projeto, eventos, início e estado); projetos com contagens e última atividade. Busca e filtros por projeto e agente; somente leitura.
- **Docker:** contêineres com CPU, memória/limite, tarefas e portas do host, ordenáveis por coluna, e um inspetor ao lado (ou abaixo, em janelas estreitas) com estado em português, imagem, portas publicadas, limite de memória, I/O acumulado, ID e os processos do contêiner monitorado; imagens agrupadas por ID e associação aos contêineres; processos do Docker Desktop e ai-memory no Mac. A legenda mostra o contexto Docker em uso.
- **Saúde:** estado geral, alertas, captura, consolidação, fila de escrita, providers e tokens por atividade, armazenamento, índices, métricas e dados brutos do servidor. **Atualizar agora** pede uma coleta fora do ciclo de 30 s.
- **Explorar:** sessões detalhadas com eventos pesquisáveis e tokens; conhecimento com wiki, busca, histórico/diff e grafos de relações.
- **Revisão:** propostas e evidências; handoffs (histórico, cancelamento e expiração), com prévia e confirmação. A lateral mostra, em âmbar, as propostas pendentes do projeto escolhido em Ajustes.
- **Manutenção e backup:** auditoria/curadoria/retenção/embeddings, reconsolidação, feedback, backup/OKF, checkpoints e restauração, com prévia e confirmação. Revisão e Manutenção e backup usam o mesmo painel: com uma operação, prévia ou resultado na tela de uma, a outra avisa e oferece voltar ou descartar.
- **Administração:** projetos/workspaces, perfil, usuários/grants/chaves, inbox/workstreams e serviço local Docker/LaunchAgent.
- **Ajustes:** servidor, Bearer/Keychain opcional, login humano, projeto monitorado na barra, contêiner, Docker CLI e intervalos.

## Conexão e coleta

Padrões: `http://127.0.0.1:49374`, contêiner `ai-memory`, recursos a cada 2 s, API a cada 60 s e inventário de imagens a cada 15 s. O intervalo da API é contado após cada varredura para evitar sobrecarregar o servidor. A execução de uma consulta pode alongar o intervalo; a interface informa a idade de cada coleta. O app reconecta automaticamente. No alto do painel, o chip de atualização mostra `Conectando…` antes da primeira coleta, `Ao vivo · atualizado há N s`, `Atrasado` (ou `API atrasada` / `Recursos atrasados`, quando a última coleta passa de três intervalos) ou `Pausado`. O botão de pausa ao lado, com o tooltip `Pausar atualização da tela`, congela a visualização; os coletores continuam trabalhando.

Abra **Ajustes**, ajuste os campos e clique em **Salvar e conectar**.

| Campo | Padrão | Limites ou comportamento |
| --- | --- | --- |
| Servidor ai-memory | `http://127.0.0.1:49374` | Endereço local genérico; altere conforme sua instalação |
| Token Bearer | Vazio | Em memória por padrão; Keychain mediante opção explícita |
| Contêiner ai-memory | `ai-memory` | Nome exato no contexto Docker monitorado |
| Executável Docker | Detecção de caminhos usuais | Aceita caminho absoluto personalizado |
| Recursos | 2 segundos | Entre 1 e 60 segundos |
| Sessões/handoffs | 60 segundos | Entre 5 e 300 segundos; espera após cada varredura |
| Aparência | Sistema | Sistema, Claro ou Escuro; aplica e salva na hora, sem reconectar |

Configurações são salvas em `~/Library/Application Support/AI Monitor/config.json`, fora do projeto. **O token não entra no JSON.** Pode ser guardado opcionalmente no Keychain, separado por endereço de servidor. URLs com usuário, senha, query ou fragmento são recusadas. Tokens em servidores fora de loopback exigem HTTPS. Redirecionamentos HTTP são recusados. O app não depende do PATH do Finder para encontrar as instalações usuais do Docker Desktop.

| Variável opcional | Finalidade |
| --- | --- |
| `AI_MEMORY_SERVER_URL` | Sobrescrever a URL ao iniciar pelo terminal; sem credenciais embutidas |
| `AI_MEMORY_AUTH_TOKEN` | Fornecer o Bearer da sessão; não é salvo, registrado ou repassado aos comandos Docker |

Os nomes das variáveis são públicos; **seus valores reais não devem ser versionados**. O app não carrega `.env`. Prefira o campo protegido da interface para informar o token. Aplicativos iniciados pelo Finder não herdam automaticamente variáveis do terminal. Veja [autenticação sem salvar segredos](INSTALL.md#autenticação-sem-salvar-segredos).

A coleta automática usa apenas leituras: `docker ps`, `stats --no-stream`, `image ls`, `container inspect --format '{{.Id}} {{.Image}}'`, `top`, `context show` e rotas GET da API/saúde. Não solicita o ambiente dos contêineres, não aceita handoffs nem chama LLM para monitorar. As consultas manuais de **Explorar** incluem busca (que pode usar embeddings/reranking do servidor). **Revisão** e **Manutenção e backup** oferecem escritas HTTP/MCP somente após preparar e confirmar uma operação específica; embeddings e reconsolidação podem consumir providers. Veja o [guia administrativo](docs/ADMINISTRATION.md).

Cada comando Docker da coleta tem timeout de 6 s e saída limitada a 16 MiB. Cada requisição HTTP tem timeout de 8 s, conexão de até 2 s e resposta limitada a 8 MiB, sem proxy. Após descobrir projetos e identidade, o coletor distribui projetos entre até quatro trabalhadores, com orçamento de 30 s; requisições em andamento podem terminar depois desse orçamento. Dados parciais e erros são sinalizados; falhas não viram medições zero. Trocar a conexão descarta respostas da configuração anterior.

## Interpretação das métricas

- **CPU dos processos/contêineres:** 100% corresponde a um núcleo; valores acima de 100% são válidos. A CPU geral do Mac é normalizada pelo total de núcleos.
- **Memória dos contêineres:** valor do Docker CLI, que desconta cache inativo no Linux. O total inclui ai-memory e os demais contêineres ativos. O limite do contêiner não é memória consumida.
- **Tarefas do Docker:** `PIDs` contabiliza processos e threads de kernel. O detalhe do ai-memory usa `docker top … nlwp` para informar threads por processo.
- **Docker Desktop no Mac:** CPU, RSS e threads dos processos locais são separados. RSS pode compartilhar páginas; não deve ser somado às métricas dos contêineres. Esses números não pretendem representar a memória exclusiva de toda a VM.
- **Imagens:** associação feita por ID imutável de `container inspect`, nunca apenas pela tag. Tags do mesmo ID são agrupadas. Contêineres parados também mantêm associação. O tamanho é lógico, não espaço exclusivo em disco; camadas compartilhadas impedem somar os tamanhos como uso físico.
- **Sessões abertas:** `ended_at == null`, sem pressupor que há um agente trabalhando. O cartão soma as contagens `open_session_count` informadas pela API por projeto. A tabela mostra as 100 sessões mais recentes por projeto, preservando seus vínculos; sessões antigas abertas podem entrar na contagem sem aparecer na tabela. O diagnóstico também informa o número de sessões abertas únicas entre as carregadas.
- **Handoffs:** apenas os abertos e visíveis à identidade autenticada. A lista da API é limitada a 200 por projeto, sem paginação. Ao atingir o limite, o app consulta a contagem total pelo briefing e informa que a lista está limitada. Contagens incompletas por falha de consulta recebem `+`. Nenhum handoff é consumido.
- **Histórico:** começa ao abrir o app, não é persistido. Falhas produzem lacunas, não zeros.

Sessão aberta e handoff aberto são conceitos distintos. O handoff representa uma transferência pendente; nenhum dos dois, isoladamente, comprova atividade atual de um agente.

O Docker CLI usa o contexto ativo e as variáveis Docker herdadas ao iniciar o app. Um contexto remoto mostra contêineres remotos; os processos do Mac continuam sendo locais. A URL da API ai-memory é configurada separadamente. I/O de rede e disco dos contêineres é acumulado, não taxa por segundo.

## Arquitetura

O núcleo e a interface são escritos em Rust. `egui/eframe` desenha o painel e `objc2` integra o menu nativo. Um pequeno script Swift gera o ícone durante o empacotamento; não é necessário em tempo de execução.

```text
Docker CLI ──┐
             ├─ coletor de recursos ──┐
macOS ───────┘                       ├─ estado compartilhado ── painel egui
ai-memory API ── coletor de sessões ──┘                       └─ barra AppKit
```

| Arquivo | Responsabilidade |
| --- | --- |
| `src/main.rs` | Inicialização e opções de diagnóstico |
| `src/app.rs` | Painéis, filtros, gráficos e configuração |
| `src/menu_bar.rs` | Indicador AppKit, ações e atualização com a janela oculta |
| `src/monitor.rs` | Coletores, processos do macOS e histórico |
| `src/docker.rs` | Comandos Docker, timeouts e conversão de métricas |
| `src/api.rs` | Consultas HTTP, limites e autenticação |
| `src/model.rs` | Modelos, preferências e agregações |
| `scripts/` e `packaging/` | Ícone, bundle `.app`, instalador `.dmg`, verificação da instalação e metadados |

## Dependências

| Dependência | Uso |
| --- | --- |
| `eframe` / `egui_extras` 0.33 | Interface, tabelas e acessibilidade |
| `objc2` 0.6; AppKit/Foundation 0.3 | Integração nativa com macOS |
| `sysinfo` 0.37 | CPU, memória e processos do host |
| `reqwest` 0.12 com `rustls` | Cliente HTTP/HTTPS |
| `serde` / `serde_json` 1 | Serialização e leitura de JSON |
| `chrono` 0.4 | Datas e horários |
| `security-framework` 3 | Keychain nativo opcional |
| `sha2` 0.10 | Conferência da configuração Compose antes de atualizar |

O Cargo baixa as dependências; versões resolvidas e checksums estão no `Cargo.lock`, que é versionado. O aplicativo não depende de Node.js, Electron, Python, banco de dados ou chave de provedor de IA. Os requisitos de sistema e ferramentas estão no [INSTALL.md](INSTALL.md). Os [avisos das dependências](packaging/THIRD_PARTY_NOTICES.txt) acompanham o bundle.

## Testes e contribuição

Os testes usam dados sintéticos e servidores HTTP locais temporários; não exigem token, ai-memory nem Docker em execução. Cobrem limites de consultas, contagens, erros, percentuais multicore, associação de imagens, timeouts e proteção de credenciais. O workflow [Checks](.github/workflows/ci.yml) executa formatação, testes, Clippy, geração do DMG Universal e verificação de instalação (macOS), além dos mesmos checks, build, testes de Secret Service e smoke gráfico sob Xvfb (Linux x86_64 e aarch64) em pushes e pull requests, sem segredos de aplicação.

Para contribuir, rode os comandos de validação da seção de desenvolvimento e envie um pull request com exemplos sintéticos. Não anexe configurações, logs, diagnósticos ou capturas com informações privadas.

## Segurança e limites da versão

- Sem telemetria própria, analytics, sincronização externa ou persistência do histórico. O app acessa o servidor e o contexto Docker configurados.
- Token excluído da serialização, erros HTTP sem o corpo da resposta, URLs validadas e redirecionamentos recusados.
- `.env`, configurações locais, chaves, diagnósticos e artefatos de build excluídos do Git pelo `.gitignore`. Releases recebem somente o instalador revisado e seu checksum; o bundle é montado a partir de uma lista explícita de arquivos, com caminhos de compilação remapeados.
- Sem atualizador automático do monitor, notificações push ou notarização Apple. O serviço ai-memory pode ser atualizado manualmente após prévia; o app não altera instalações remotas.
- Interface validada em macOS Apple Silicon; executável Intel verificado via Rosetta. O bundle declara macOS 12.0 como mínimo; hardware Intel e versões antigas ainda precisam de validação. O Docker Desktop tem requisitos próprios.

Consulte [SECURITY.md](SECURITY.md) para proteção de dados e relato privado de vulnerabilidades. O projeto usa a licença [MIT](LICENSE); as dependências mantêm suas respectivas licenças.

Integração validada com ai-memory 3.3.0. Referências: [API oficial do ai-memory](https://github.com/akitaonrails/ai-memory/blob/main/docs/frontend-api.md), [Docker stats](https://docs.docker.com/reference/cli/docker/container/stats/), [eframe](https://docs.rs/eframe/0.33.3/eframe/).
