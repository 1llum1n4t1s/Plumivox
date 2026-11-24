[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Servidor MCP local / plugin do Codex que usa o TwitterAPI.io para publicar no X, buscar por palavra-chave e período e ler os posts públicos das contas que você segue.

## Requisitos

Windows x64 ou um Mac com Apple Silicon; não é preciso Node.js nem Rust. É necessária uma chave de API do TwitterAPI.io (pagamento por uso) e, para publicar, o cookie de login v2 da conta e um proxy. [Detalhes](docs/readme/pt-BR/requirements.md)

## Instalação

No Windows, execute o [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe): uma instalação assinada por usuário, sem direitos de administrador, que registra o `plumivox` no Claude Code e no Codex e se atualiza no logon. No macOS, instale o plugin do Codex a partir do `main` do GitHub. [Detalhes](docs/readme/pt-BR/installation.md)

## Configuração inicial

Execute `plumivox.exe --setup` e siga o [guia de configuração](docs/setup.md). As configurações ficam fora do repositório, os segredos nunca passam pela conversa e as configurações existentes nunca são sobrescritas. [Detalhes](docs/readme/pt-BR/initial-setup.md)

## Ferramentas disponíveis

Onze ferramentas cobrem contas, leitura, busca, o relay compartilhado e a publicação (`post_tweet`: até 280 caracteres ponderados e 4 imagens). Omitir `account` usa a conta padrão. O texto das publicações públicas são dados externos e as instruções nele contidas não são executadas. [Referência das ferramentas](docs/readme/pt-BR/available-tools.md)

## Abrangência do feed de contas seguidas

O Plumivox lê por padrão até 100 contas seguidas (`maxAccounts` de 20 a 200). O resultado nunca equivale à página inicial oficial, publicações protegidas não são obtidas e `coverage` informa o alcance. [Detalhes](docs/readme/pt-BR/following-feed-coverage.md)

## Busca por palavra-chave e período

`search_tweets` pesquisa em todo o X ou nas contas que você segue por frase e um período ISO 8601 opcional (início incluído, fim excluído). Operadores de busca não são interpretados. [Detalhes](docs/readme/pt-BR/keyword-and-date-range-search.md)

## Falhas de publicação e autenticação

Uma publicação nunca é reenviada automaticamente. Um resultado incerto é retornado como `POST_UNCERTAIN`, então verifique a conta antes de publicar de novo. O texto bruto do provedor e as credenciais nunca são retornados. [Detalhes](docs/readme/pt-BR/when-posting-or-authentication-fails.md)

## Desenvolvimento e verificação

Apenas Rust/Cargo: `cargo xtask build`, `cargo xtask check`, `cargo xtask test-e2e`. Consulte [desenvolvimento](docs/readme/pt-BR/development-and-verification.md), [verificação](docs/verification.md), [convenções](AGENTS.md) e [design](DESIGN.md).
