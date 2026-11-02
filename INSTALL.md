# Instalação e dependências

Este guia instala o AI Monitor pelo código-fonte no próprio Mac. O aplicativo abre na barra de menus; a janela é opcional. Não há instalador `.dmg`, Homebrew formula ou binário notarizado nesta versão.

## 1. Requisitos

| Requisito | Quando é necessário |
| --- | --- |
| macOS com sessão gráfica | Executar o aplicativo; validado em Apple Silicon |
| Rust estável e Cargo | Compilar e rodar testes |
| Xcode Command Line Tools | Clang, SDK macOS, Swift e Git para compilação/empacotamento |
| `iconutil` e `codesign` | Ferramentas do macOS utilizadas pelo script de build |
| Docker Desktop e Docker CLI | Coletar contêineres, imagens, CPU, memória e tarefas do Docker |
| Servidor ai-memory acessível | Consultar projetos, sessões e handoffs; integração validada com 3.3.0 |
| Internet na primeira compilação | Baixar o código e as crates do Cargo |

O ambiente de validação inicial foi macOS 27.0.1, Apple Silicon e Rust 1.99.1. Isso registra o ambiente testado, não um requisito mínimo de Rust. Use Rust estável atualizado. O bundle declara macOS 12.0, mas versões antigas e Macs Intel ainda não foram validados. O [Docker Desktop possui sua própria política de versões do macOS](https://docs.docker.com/desktop/setup/install/mac-install/).

Docker e ai-memory não são necessários para compilar ou testar. O app também abre sem eles, mostrando as fontes indisponíveis. Não é necessário instalar Node.js, Electron, Python, OpenSSL, um banco de dados ou uma chave de provedor de IA.

## 2. Instalar as ferramentas de compilação

### Xcode Command Line Tools

No Terminal:

```sh
xcode-select --install
```

Conclua a instalação na janela do macOS. Se as ferramentas já estiverem instaladas, confira:

```sh
xcode-select -p
xcrun --find clang
xcrun --find swift
git --version
```

Uma instalação completa do Xcode também fornece essas ferramentas. Downloads oficiais: [Apple Developer](https://developer.apple.com/xcode/resources/).

### Rust e Cargo

Instale pelo [rustup oficial](https://rust-lang.org/tools/install/):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Aceite a instalação padrão. Abra um novo terminal ou carregue o ambiente:

```sh
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

Se Rust já estiver instalado, atualize a ferramenta estável:

```sh
rustup update stable
```

Para executar as verificações de desenvolvimento:

```sh
rustup component add rustfmt clippy
```

## 3. Preparar as fontes de dados

### Docker

Instale e inicie o [Docker Desktop para Mac](https://docs.docker.com/desktop/setup/install/mac-install/), escolhendo a versão da arquitetura do seu computador. Aguarde o engine iniciar e confirme:

```sh
docker version
docker context show
docker ps
```

O monitor acompanha o contexto ativo do Docker CLI. Um contexto remoto representa os contêineres daquele servidor, enquanto os processos do Mac continuam locais. Não é necessário expor o daemon Docker em uma porta TCP nem fornecer credenciais Docker ao AI Monitor.

O app procura o executável em `/usr/local/bin/docker`, `/opt/homebrew/bin/docker` e `/Applications/Docker.app/Contents/Resources/bin/docker`. Se a instalação usar outro local, execute `command -v docker` no terminal e informe o caminho absoluto em **Conexão → Executável Docker**. Não use `~` nesse campo: informe o caminho expandido.

### ai-memory

Instale ou utilize seu servidor seguindo o [projeto ai-memory](https://github.com/akitaonrails/ai-memory). Este repositório não inclui servidor, dados, token nem configuração Docker Compose de uma instalação particular.

O endereço inicial do monitor é `http://127.0.0.1:49374` e o nome de contêiner é `ai-memory`. Esses valores são padrões genéricos, não detectam todas as instalações. Ajuste o endereço e o nome conforme seu ambiente.

Se o ai-memory usar autenticação, obtenha um token autorizado na sua instalação e informe-o na interface do monitor. A identidade autenticada limita quais projetos e handoffs podem ser vistos. O monitor não cria tokens, usuários ou permissões no servidor.

## 4. Baixar, compilar e abrir

```sh
git clone https://github.com/cristianovallory/ai-monitor.git
cd ai-monitor
bash scripts/build-app.sh
open 'dist/AI Monitor.app'
```

A primeira compilação baixa e compila as crates, podendo demorar alguns minutos. O script usa `cargo build --release --locked`, gera o ícone, monta o bundle e aplica/verifica uma assinatura ad hoc. O aplicativo fica em `dist/AI Monitor.app`.

O script usa Cargo de `~/.cargo/bin/cargo` ou do `PATH`; `CARGO_BIN` permite informar outro executável. Execute o build sem `CARGO_TARGET_DIR` ou `CARGO_BUILD_TARGET` personalizados, pois o empacotamento espera `target/release/ai-monitor`.

**Ao abrir, procure o texto AI/Docker na barra de menus.** Não aparece uma janela nem um ícone no Dock. Clique no indicador e escolha **Abrir painel**. Em **Conexão**, ajuste os campos e clique em **Salvar e conectar**. O botão vermelho da janela apenas oculta o painel; para encerrar, use **Sair do AI Monitor** no menu.

Para abrir explicitamente com a janela:

```sh
open 'dist/AI Monitor.app' --args --show-dashboard
```

Se o app já estiver aberto, use **Abrir painel** no menu; os argumentos de uma segunda abertura podem não ser aplicados à instância existente.

## 5. Instalar em Aplicativos

Encerre uma instância anterior pelo menu antes de substituir o aplicativo. Para instalar somente no seu usuário, sem `sudo`:

```sh
mkdir -p "$HOME/Applications"
ditto 'dist/AI Monitor.app' "$HOME/Applications/AI Monitor.app"
open "$HOME/Applications/AI Monitor.app"
```

Também é possível arrastar o bundle para Aplicativos no Finder. Depois de compilar e copiar, não é necessário manter Rust, Swift ou o repositório aberto para executar o app. Docker/ai-memory continuam necessários para os respectivos dados.

A assinatura ad hoc não equivale a Developer ID nem notarização Apple. Para esta versão, compile localmente. Se o macOS apresentar um bloqueio, siga as opções oficiais de **Privacidade e Segurança** apenas após confirmar a origem; não desative o Gatekeeper globalmente.

Para iniciar no login, adicione o app instalado em **Ajustes do Sistema → Geral → Itens de Início**; a nomenclatura pode variar conforme o macOS. Essa configuração é opcional e não é alterada pelo script.

## Autenticação sem salvar segredos

A opção mais simples é inserir o token em **Conexão → Token Bearer** e clicar em **Salvar e conectar**. Ele fica somente na memória do processo e deve ser informado novamente após encerrar o aplicativo. Não é salvo no JSON de preferências nem no Keychain.

Para uso pelo terminal, `AI_MEMORY_SERVER_URL` e `AI_MEMORY_AUTH_TOKEN` são opcionais. Exemplo para o **zsh padrão do macOS**, solicitando o token sem escrevê-lo no histórico nem exibi-lo:

```zsh
(
  export AI_MEMORY_SERVER_URL='http://127.0.0.1:49374'
  read -r -s 'AI_MEMORY_AUTH_TOKEN?Token ai-memory: '
  printf '\n'
  export AI_MEMORY_AUTH_TOKEN
  exec './dist/AI Monitor.app/Contents/MacOS/ai-monitor'
)
```

O subshell evita manter o token no ambiente do terminal após o app encerrar. Use a URL da sua instalação; servidores remotos com token exigem HTTPS. Execute o binário diretamente para receber as variáveis do terminal, após encerrar uma instância existente pelo menu.

O token fornecido por ambiente permanece acessível ao próprio processo; essa opção não é um cofre de segredos. Não coloque valores reais em comandos literais, `.env` do projeto, scripts versionados, issues ou capturas de tela. O app não lê arquivos `.env` e remove `AI_MEMORY_AUTH_TOKEN` do ambiente dos comandos Docker que inicia.

As preferências sem token ficam em `~/Library/Application Support/AI Monitor/config.json`. Esse arquivo pode revelar seu endpoint ou caminhos locais; mantenha-o fora do Git. Configurações inválidas carregadas na inicialização fazem o app voltar aos padrões.

## Atualizar

Encerre o app pelo menu. Dentro de uma cópia do repositório sem alterações locais pendentes:

```sh
git pull --ff-only
bash scripts/build-app.sh
ditto 'dist/AI Monitor.app' "$HOME/Applications/AI Monitor.app"
open "$HOME/Applications/AI Monitor.app"
```

As preferências locais são preservadas. O app não possui atualizador automático. Se você instalou em outro diretório, copie para aquele destino.

## Solução de problemas

| Sintoma | O que conferir |
| --- | --- |
| `cargo: command not found` | Abra um novo terminal ou carregue `source "$HOME/.cargo/env"` |
| Clang, SDK ou Swift ausente | Instalação do Xcode Command Line Tools e resultado de `xcode-select -p` |
| Não aparece janela | Comportamento esperado: use **Abrir painel** na barra de menus |
| Não vejo o indicador | Espaço disponível na barra, notch ou app que oculta itens; confirme se o monitor está aberto |
| Docker indisponível | Docker Desktop iniciado, `docker version`, contexto ativo e caminho do CLI no painel |
| AI indisponível, Docker funciona | Nome exato do contêiner ou existência de processo ai-memory nativo |
| Sessões/handoffs indisponíveis | URL da API, porta acessível, versão do servidor e permissões do token |
| Acesso recusado | Token ausente, inválido ou sem permissão para a identidade desejada |
| Token some após reiniciar | Comportamento esperado: não é persistido |
| Funciona pelo terminal, mas não pelo Finder | Variáveis de ambiente não herdadas; configure pela interface e use caminho absoluto do Docker |
| CPU acima de 100% | Valor por núcleo; consumo multicore pode ultrapassar 100% |
| Docker inclui o consumo do ai-memory | O total inclui todos os contêineres ativos, inclusive o ai-memory |
| Threads detalhadas indisponíveis | O daemon pode não suportar `docker top … nlwp`; tarefas/PIDs são outra métrica |
| Contagem maior que a lista | Limites por projeto: 100 sessões e 200 handoffs; consulte a interpretação no README |
| Dados ficam em `—` | Fonte indisponível ou amostra vencida; confira a idade da coleta e os erros em Conexão |

Para diagnóstico local, na pasta do repositório:

```sh
cargo run --locked -- --diagnose
```

Essa saída pode revelar nomes de recursos, portas e projetos em erros. Revise e remova dados privados antes de anexá-la a uma issue. Não envie seu arquivo de configuração nem um `docker inspect` completo.

## Desinstalar

1. Use **Sair do AI Monitor** na barra de menus.
2. Remova-o dos Itens de Início, caso tenha adicionado manualmente.
3. Mova `AI Monitor.app` para a Lixeira.
4. Opcionalmente, remova `~/Library/Application Support/AI Monitor` para apagar preferências.
5. A cópia do código, `target/` e `dist/` também podem ser removidas se não precisar mais compilar.

A desinstalação do monitor não remove Docker, ai-memory, contêineres, imagens, sessões ou handoffs.
