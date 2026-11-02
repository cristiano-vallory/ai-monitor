# Segurança e privacidade

## Dados e credenciais

O AI Monitor é um cliente de monitoramento e administração. A coleta automática consulta o servidor ai-memory, comandos de leitura Docker e métricas locais. Escritas administrativas são explícitas, com prévia e confirmação no app. Não possui telemetria própria, analytics nem upload de diagnósticos. Busca, embeddings e reconsolidação podem consumir os providers configurados no servidor quando solicitados pelo usuário.

- O token Bearer é mantido em memória e ignorado na serialização e desserialização da configuração. Não é salvo no JSON nem no Keychain.
- O campo de token da interface é protegido visualmente. Tokens fornecidos por `AI_MEMORY_AUTH_TOKEN` não são repassados aos subprocessos Docker.
- URLs de servidor com usuário, senha, query ou fragmento são recusadas. Tokens fora de loopback exigem HTTPS.
- O cliente HTTP não segue redirecionamentos nem usa proxies de ambiente. Respostas de erro não são reproduzidas integralmente, evitando expor detalhes internos do servidor.
- `docker container inspect` consulta somente IDs de contêiner e imagem. O app não solicita o ambiente interno dos contêineres.
- Backups e exportações só são gravados após confirmação, em arquivo novo com permissão `0600`; não são publicados. Backup global inclui banco, wiki/histórico e `config.toml`, que pode conter segredos. O download tem limite de 2 GiB e 5 minutos.
- Ações administrativas não têm retry automático. Perda de resposta pode deixar resultado incerto: consulte o estado antes de repetir. Confirmações expiram em 5 minutos, são descartadas ao trocar conexão/escopo e os campos ficam bloqueados durante a execução.
- O histórico permanece em memória. Sessões, resumos de handoffs e próximos passos são exibidos no painel, sem exportação automática.
- Os binários de distribuição usam remapeamento de caminhos de compilação. O instalador é montado em uma pasta temporária limpa, com uma lista explícita de arquivos, sem copiar preferências ou o diretório do projeto.

Esses cuidados não tornam o processo um cofre: tokens e dados podem permanecer na memória enquanto o aplicativo está em execução. Use um Mac e um servidor de confiança. O Docker CLI usa as permissões e o contexto do usuário; o app se limita a comandos de leitura, mas não transforma a credencial do daemon em uma credencial tecnicamente restrita a leitura.

## O que não deve ser publicado

O repositório público deve conter código, documentação, testes com dados sintéticos e arquivos de build. Não adicione:

- Tokens, senhas, chaves privadas, certificados privados ou valores reais de variáveis sensíveis.
- `.env`, configurações Cargo locais com credenciais, preferências do aplicativo ou configuração pessoal do Docker.
- Bancos, backups, resumos reais de handoffs, observações, sessões ou nomes privados de projetos.
- Logs, saídas de diagnóstico ou capturas do painel sem revisão e anonimização.
- Builds locais sem revisão própria de distribuição e assinatura.

O `.gitignore` exclui classes comuns desses arquivos, incluindo `target/`, `dist/`, `.env*`, chaves e diretórios de dados. Isso é uma proteção auxiliar: não bloqueia `git add -f`, arquivos já rastreados ou segredos escritos dentro do código.

O arquivo `~/Library/Application Support/AI Monitor/config.json` não tem token, mas pode identificar endpoints e caminhos locais. Mantenha-o privado. `--diagnose` não imprime tokens nem resumos de handoffs, mas pode revelar nomes de processos, contêineres, imagens, portas e projetos em mensagens de erro. Capturas de tela podem revelar mais informações que o diagnóstico textual.

Os valores `test-token`, `very-secret` e `synthetic-*` presentes nos testes são fixtures artificiais, sem acesso a qualquer serviço.

## Verificações antes de contribuir

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
git diff --cached --check
git diff --cached
```

Uma varredura adicional pode ser feita com o [Gitleaks](https://github.com/gitleaks/gitleaks), instalado separadamente:

```sh
gitleaks git --redact .
```

Revise também arquivos novos e alterações ainda não commitadas: uma varredura do histórico não cobre conteúdo fora dele. Não envie relatórios com dados sensíveis para serviços externos. Nenhum scanner substitui a revisão dos arquivos publicados.

O workflow de CI usa permissão `contents: read`, não recebe tokens de aplicação e desabilita a persistência da credencial do checkout. Testes utilizam dados sintéticos e servidores HTTP locais temporários.

## Relatar uma vulnerabilidade

Use o [relato privado de vulnerabilidade do GitHub](https://github.com/cristianovallory/ai-monitor/security/advisories/new). Inclua versão, impacto e passos de reprodução com dados sintéticos. Não coloque credenciais nem dados de terceiros na descrição.

Não publique uma vulnerabilidade com dados sensíveis em uma issue aberta. Se uma credencial real tiver sido exposta, revogue-a ou rotacione-a no serviço de origem; remover somente o arquivo da versão atual não a remove do histórico.

As correções são desenvolvidas na branch `main`. Não há compromisso de suporte a versões antigas nem prazo garantido de resposta nesta fase inicial.

## Downloads das releases

Baixe o `.dmg` somente das [releases deste repositório](https://github.com/cristianovallory/ai-monitor/releases). O checksum SHA-256 acompanha o instalador; ele detecta alterações/corrupção, mas não substitui autenticação de desenvolvedor nem notarização.

Os pacotes das releases atuais são universais (Apple Silicon e Intel), com assinatura ad hoc e sem notarização Apple. O certificado pessoal Apple Development não é utilizado na distribuição. A assinatura verifica a integridade do bundle; não atesta uma identidade Developer ID. A eventual autorização de abertura pelo Gatekeeper deve ser feita para este app específico, conforme o [guia de instalação](INSTALL.md), sem desativação global das proteções do macOS.
