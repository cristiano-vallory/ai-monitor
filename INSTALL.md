# Instalação e dependências

Você pode instalar pelo `.dmg` pronto, sem compilar Rust. O aplicativo abre na barra de menus; a janela é opcional. A compilação pelo código-fonte continua disponível para desenvolvedores.

## Instalar pelo DMG — sem Rust ou Xcode

1. Acesse a [release v0.5.0](https://github.com/cristianovallory/ai-monitor/releases/tag/v0.5.0) e baixe **AI-Monitor-0.5.0-macos-universal.dmg**. Os arquivos automáticos **Source code** são o código-fonte, não o instalador.
2. Abra o `.dmg` e arraste **AI Monitor.app** para o atalho **Applications / Aplicativos**.
3. Se estiver atualizando, encerre a versão anterior em **Sair do AI Monitor** antes de substituir o app.
4. Ejete o disco AI Monitor e abra o aplicativo em **Aplicativos**.
5. Procure **AI / Docker** na barra de menus. Clique e escolha **Abrir painel**.
6. Em **Conexão**, ajuste servidor, token opcional e nome do contêiner; use **Salvar e conectar**.

O instalador é **Universal**, com executáveis nativos para Apple Silicon e Intel. O mínimo declarado é macOS 12.0; a interface foi validada em Apple Silicon e o executável Intel foi verificado via Rosetta. Hardware Intel e versões antigas do macOS ainda não foram validados.

Para usar o app pronto, **não instale Rust, Cargo, Xcode, Swift ou Git**. Docker Desktop e ai-memory são necessários apenas para os respectivos dados. O pacote não inclui servidor, dados de usuários nem tokens.

### Primeira abertura e Gatekeeper

Esta release usa **assinatura ad hoc**, sem certificado Developer ID e sem notarização Apple. O macOS pode solicitar uma autorização adicional. Após tentar abrir o app, se a mensagem indicar desenvolvedor não verificado ou ausência de notarização:

1. Confirme que o download veio da release oficial acima.
2. Abra **Ajustes do Sistema → Privacidade e Segurança**.
3. Use **Abrir Mesmo Assim** para AI Monitor e confirme a abertura.

Essa é uma exceção para o aplicativo específico, conforme a [orientação da Apple](https://support.apple.com/en-us/102445). Não desative o Gatekeeper globalmente. Um Mac gerenciado pode impedir essa exceção. Se a mensagem indicar arquivo danificado, verifique o checksum e baixe novamente; não ignore um alerta de malware.

### Verificar a integridade do download (opcional)

Baixe também **AI-Monitor-0.5.0-macos-universal.dmg.sha256** da mesma release. Com os dois arquivos em Downloads:

```sh
cd "$HOME/Downloads"
shasum -a 256 -c AI-Monitor-0.5.0-macos-universal.dmg.sha256
```

O resultado deve terminar em `OK`. O checksum detecta alterações ou corrupção do arquivo; não substitui assinatura Developer ID ou notarização.

As próximas seções explicam dependências, configuração das fontes e a **compilação opcional**. Quem instalou pelo DMG pode ir diretamente a [Preparar as fontes de dados](#3-preparar-as-fontes-de-dados).

## 1. Requisitos

| Requisito | Quando é necessário |
| --- | --- |
| macOS com sessão gráfica | Executar o aplicativo; validado em Apple Silicon |
| Rust estável e Cargo | Somente para compilar e rodar testes; dispensados no DMG |
| Xcode Command Line Tools | Somente para compilação/empacotamento: Clang, SDK macOS, Swift e Git |
| `iconutil` e `codesign` | Ferramentas do macOS utilizadas pelo script de build |
| Docker Desktop e Docker CLI | Coletar contêineres, imagens, CPU, memória e tarefas do Docker |
| Servidor ai-memory acessível | Consultar projetos, sessões e handoffs; integração validada com 3.3.0 |
| Internet na primeira compilação | Baixar o código e as crates do Cargo |

O ambiente de validação inicial foi macOS 27.0.1, Apple Silicon e Rust 1.99.1; o executável Intel também foi verificado via Rosetta. Isso registra o ambiente testado, não um requisito mínimo de Rust. Para compilar, use Rust estável atualizado. O bundle declara macOS 12.0, mas versões antigas e hardware Intel ainda não foram validados. O [Docker Desktop possui sua própria política de versões do macOS](https://docs.docker.com/desktop/setup/install/mac-install/).

Docker e ai-memory não são necessários para compilar ou testar. O app também abre sem eles, mostrando as fontes indisponíveis. Não é necessário instalar Node.js, Electron, Python, OpenSSL, um banco de dados ou uma chave de provedor de IA.

## 2. Instalar as ferramentas de compilação (somente desenvolvedores)

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

Se o ai-memory usar autenticação, obtenha um token autorizado na sua instalação e informe-o na interface do monitor. A identidade autenticada limita quais projetos e handoffs podem ser vistos. Com permissão administrativa, a tela Administração avançada permite gerir usuários, grants e chaves; as operações exigem prévia e confirmação.

## 4. Baixar, compilar e abrir

```sh
git clone https://github.com/cristianovallory/ai-monitor.git
cd ai-monitor
bash scripts/build-app.sh
open 'dist/AI Monitor.app'
```

A primeira compilação baixa e compila as crates, podendo demorar alguns minutos. O script usa `cargo build --release --locked`, gera o ícone, monta o bundle e aplica/verifica uma assinatura ad hoc. O aplicativo fica em `dist/AI Monitor.app`.

O script usa Cargo de `~/.cargo/bin/cargo` ou do `PATH`; `CARGO_BIN` permite informar outro executável. Ele fixa o diretório de saída em `target/`, o alvo da arquitetura solicitada e o deployment target macOS 12.0. Os flags de release remapeiam caminhos do projeto, Cargo e Rustup para evitar incluir diretórios pessoais nas mensagens de erro compiladas.

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

A assinatura ad hoc não equivale a Developer ID nem notarização Apple. Para downloads da release, consulte [Primeira abertura e Gatekeeper](#primeira-abertura-e-gatekeeper).

Para iniciar no login, adicione o app instalado em **Ajustes do Sistema → Geral → Itens de Início**; a nomenclatura pode variar conforme o macOS. Essa configuração é opcional e não é alterada pelo script.

## Autenticação sem salvar segredos

A opção mais simples é inserir o token em **Conexão → Token Bearer** e clicar em **Salvar e conectar**. Ele fica somente na memória do processo e deve ser informado novamente após encerrar o aplicativo. Não é salvo no JSON de preferências. A opção **Guardar o Bearer no Keychain** permite persistir somente esse token, por endereço. Desmarque a opção e salve no mesmo servidor para removê-lo. Variável de ambiente explícita, mesmo vazia, tem prioridade sobre o Keychain.

Para uso pelo terminal, `AI_MEMORY_SERVER_URL` e `AI_MEMORY_AUTH_TOKEN` são opcionais. Exemplo para o **zsh padrão do macOS**, solicitando o token sem escrevê-lo no histórico nem exibi-lo:

```zsh
(
  export AI_MEMORY_SERVER_URL='http://127.0.0.1:49374'
  read -r -s 'AI_MEMORY_AUTH_TOKEN?Token ai-memory: '
  printf '\n'
  export AI_MEMORY_AUTH_TOKEN
  exec '/Applications/AI Monitor.app/Contents/MacOS/ai-monitor'
)
```

O subshell evita manter o token no ambiente do terminal após o app encerrar. Use a URL da sua instalação; servidores remotos com token exigem HTTPS. Ajuste o caminho se instalou em `~/Applications` ou está usando `dist/`. Execute o binário diretamente para receber as variáveis do terminal, após encerrar uma instância existente pelo menu.

O token fornecido por ambiente permanece acessível ao próprio processo; essa opção não é um cofre de segredos. Não coloque valores reais em comandos literais, `.env` do projeto, scripts versionados, issues ou capturas de tela. O app não lê arquivos `.env` e remove `AI_MEMORY_AUTH_TOKEN` do ambiente dos comandos Docker que inicia.

As preferências sem token ficam em `~/Library/Application Support/AI Monitor/config.json`. Esse arquivo pode revelar seu endpoint ou caminhos locais; mantenha-o fora do Git. Configurações inválidas carregadas na inicialização são substituídas por valores válidos ou pelos padrões, preservando as variáveis de ambiente válidas. O arquivo de preferências é salvo com acesso restrito ao usuário (`0600`).

## Atualizar

**Instalado pelo DMG:** baixe a nova release, encerre o app pelo menu e arraste a nova versão para Aplicativos, confirmando a substituição. Ejete o disco e abra o app novamente. As preferências são preservadas; informe novamente o token se não optou pelo Keychain. Sessões humanas precisam de novo login.

**Compilado pelo código-fonte:** dentro de uma cópia do repositório sem alterações locais pendentes, com o app encerrado:

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
| Token some após reiniciar | Esperado sem Keychain; confira a opção, endereço e permissão de acesso ao Keychain |
| Funciona pelo terminal, mas não pelo Finder | Variáveis de ambiente não herdadas; configure pela interface e use caminho absoluto do Docker |
| CPU acima de 100% | Valor por núcleo; consumo multicore pode ultrapassar 100% |
| Docker inclui o consumo do ai-memory | O total inclui todos os contêineres ativos, inclusive o ai-memory |
| Threads detalhadas indisponíveis | O daemon pode não suportar `docker top … nlwp`; tarefas/PIDs são outra métrica |
| Contagem maior que a lista | Limites por projeto: 100 sessões e 200 handoffs; consulte a interpretação no README |
| Dados ficam em `—` | Fonte indisponível ou amostra vencida; confira a idade da coleta e os erros em Conexão |

Para diagnóstico local do app instalado, sem Rust:

```sh
'/Applications/AI Monitor.app/Contents/MacOS/ai-monitor' --diagnose
```

Essa saída pode revelar nomes de recursos, portas e projetos em erros. Revise e remova dados privados antes de anexá-la a uma issue. Não envie seu arquivo de configuração nem um `docker inspect` completo. A opção `--version` informa somente versão e arquitetura, sem consultar as fontes.

## Gerar o instalador para distribuição

Somente mantenedores precisam executar estas etapas. Com as ferramentas de compilação instaladas:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
bash scripts/build-dmg.sh
bash scripts/verify-dmg.sh
```

Saídas em `dist/`:

- `AI Monitor.app`: bundle Universal, assinado ad hoc.
- `AI-Monitor-0.5.0-macos-universal.dmg`: imagem comprimida, somente leitura.
- `AI-Monitor-0.5.0-macos-universal.dmg.sha256`: checksum para download.

O pacote é montado em uma pasta temporária limpa, a partir do bundle, do atalho `/Applications` e do `LEIA-ME.txt`. Configurações, tokens, dados, repositório e diagnósticos não entram na imagem. O script verifica a assinatura e as arquiteturas; o verificador monta a imagem, copia o app para um destino temporário e executa `--version` antes e depois de ejetar. Essa verificação não abre o painel nem acessa Docker/ai-memory.

O bundle inclui a licença do projeto e os avisos das dependências. Ao alterar `Cargo.lock`, um mantenedor deve regenerar os avisos com `python3 scripts/collect-licenses.py` e revisar o resultado. Esse utilitário opcional usa Python 3 e internet para consultar licenças nos commits públicos de origem; Python não é necessário para instalar, executar ou compilar o app com os avisos já versionados. O build recusa avisos cujo hash não corresponda ao `Cargo.lock`.

Antes de publicar, confira a correspondência entre versão de `Cargo.toml`, `Info.plist` e tag, execute os testes e revise os artefatos. Não envie certificados privados nem use um certificado Apple Development como substituto de Developer ID. Uma futura distribuição notarizada exige configuração própria de assinatura e notarização; não há credenciais Apple no repositório.

## Desinstalar

1. Use **Sair do AI Monitor** na barra de menus.
2. Remova-o dos Itens de Início, caso tenha adicionado manualmente.
3. Mova `AI Monitor.app` para a Lixeira.
4. Opcionalmente, remova `~/Library/Application Support/AI Monitor` para apagar preferências.
5. A cópia do código, `target/` e `dist/` também podem ser removidas se não precisar mais compilar.

A desinstalação do monitor não remove Docker, ai-memory, contêineres, imagens, sessões ou handoffs.


## Recursos administrativos (v0.4)

Não há dependências novas para instalar o app. Habilite a API de leitura no
ai-memory (`--enable-api` ou `--enable-web`) para selecionar projetos e consultar
prévias. A administração exige autoridade apropriada; uma chave pessoal `aim_`
é User e não se torna root por pertencer a um administrador. Reconsolidação e
embeddings dependem dos providers configurados no servidor e podem gerar cobrança.

Abra **Administrar memória**, selecione o projeto, prepare uma operação, leia a
prévia e confirme o escopo. Backup e checkpoint globais usam a confirmação
`GLOBAL`; reconsolidação usa `SESSAO <id>`. Nunca use dados reais para testar
exclusão/manutenção. O [guia administrativo](docs/ADMINISTRATION.md) detalha
permissões, limites, backups privados e resultados incertos após falhas de rede.


## Administração avançada e serviço local (v0.5)

Não há ferramentas adicionais para login humano/Keychain. Em **Conexão**, salve
primeiro o endereço e entre com usuário/senha em **Autenticação humana**. Senha,
cookies e CSRF ficam apenas em memória. O botão de sair retorna explicitamente
ao Bearer previamente configurado. Senhas temporárias podem exigir troca na
mesma tela antes de acessar a administração.

Para operação local, **Administração avançada → Serviço deste Mac** exige URL
loopback e configuração explícita. Docker precisa do CLI e, para atualizar, do
plugin Compose v2 com suporte a `config --format json`, arquivo Compose único,
serviço existente e backup privado. Contextos remotos e stacks com overrides
são recusados. Variáveis arbitrárias do terminal não são herdadas nesses comandos;
mantenha as dependências de configuração no Compose e seus arquivos locais.

LaunchAgent exige o plist oficial `com.github.akitaonrails.ai-memory`, executando
`ai-memory serve` diretamente. Plist/binários de manutenção devem ser arquivos
regulares, sem symlink. Para atualizar: salve backup, pare o serviço, obtenha o
novo binário de fonte confiável e informe seu SHA-256; o app mantém o anterior
para rollback. Inicie novamente e confira a saúde. O hash confirma correspondência
dos bytes, não autenticidade da origem. O monitor não baixa binários automaticamente.

Consulte o [guia administrativo](docs/ADMINISTRATION.md) para semântica das
prévias, permissões, limites, logs privados e recuperação offline orientada.
