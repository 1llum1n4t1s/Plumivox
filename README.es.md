[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Servidor MCP local / plugin de Codex que, mediante TwitterAPI.io, permite publicar en X, buscar por palabra clave y periodo, y leer las publicaciones públicas de las cuentas seguidas.

## Requisitos

Windows x64 o un Mac con Apple Silicon; no se necesita Node.js ni Rust. Hace falta una clave de API de TwitterAPI.io (pago por uso) y, para publicar, la cookie de inicio de sesión v2 de la cuenta y un proxy. [Detalles](docs/readme/es/requirements.md)

## Instalación

En Windows, ejecute [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe): una instalación firmada por usuario, sin derechos de administrador, que registra `plumivox` en Claude Code y Codex y se actualiza al iniciar sesión. En macOS, instale el plugin de Codex desde la rama `main` de GitHub. [Detalles](docs/readme/es/installation.md)

## Configuración inicial

Ejecute `plumivox.exe --setup` y siga la [guía de configuración](docs/setup.md). La configuración se guarda fuera del repositorio, los secretos nunca pasan por la conversación y la configuración existente nunca se sobrescribe. [Detalles](docs/readme/es/initial-setup.md)

## Herramientas disponibles

Once herramientas cubren cuentas, lectura, búsqueda, el relé compartido y la publicación (`post_tweet`: hasta 280 caracteres ponderados y 4 imágenes). Si se omite `account`, se usa la cuenta predeterminada. El texto de las publicaciones públicas son datos externos y sus instrucciones no se ejecutan. [Referencia de herramientas](docs/readme/es/available-tools.md)

## Alcance del feed de cuentas seguidas

Plumivox lee por defecto hasta 100 cuentas seguidas (`maxAccounts` de 20 a 200). El resultado nunca equivale al inicio oficial, no se obtienen las publicaciones protegidas y `coverage` indica el alcance. [Detalles](docs/readme/es/following-feed-coverage.md)

## Búsqueda por palabra clave y periodo

`search_tweets` busca en todo X o en las cuentas que sigue por frase y un periodo ISO 8601 opcional (inicio incluido, fin excluido). Los operadores de búsqueda no se interpretan. [Detalles](docs/readme/es/keyword-and-date-range-search.md)

## Si fallan la publicación o la autenticación

Una publicación nunca se reenvía automáticamente. Un resultado poco claro se devuelve como `POST_UNCERTAIN`, así que compruebe la cuenta antes de volver a publicar. Nunca se devuelve el texto bruto del proveedor ni las credenciales. [Detalles](docs/readme/es/when-posting-or-authentication-fails.md)

## Desarrollo y verificación

Solo Rust/Cargo: `cargo xtask build`, `cargo xtask check`, `cargo xtask test-e2e`. Consulte [desarrollo](docs/readme/es/development-and-verification.md), [verificación](docs/verification.md), [convenciones](AGENTS.md) y [diseño](DESIGN.md).
