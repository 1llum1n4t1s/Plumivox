# Requisitos

- Windows x64 ou Mac com Apple Silicon (macOS ARM64). O núcleo do MCP é um executável nativo escrito em Rust; não é necessário instalar Node.js nem Rust.
- A configuração inicial no Windows e no macOS usa o executável incluído. O PowerShell 7 (`pwsh`) não é necessário.
- Uma chave de API do TwitterAPI.io. O uso da API é cobrado por consumo pelo próprio serviço.
- Para publicar: os cookies de login v2 da conta de destino e um proxy. Além de um proxy externo já existente, no Windows e no macOS é possível compartilhar entre vários chats a saída por conexão residencial e o relay SSH gerenciados pelo Plumivox. É necessária uma preparação inicial do servidor de relay público.
