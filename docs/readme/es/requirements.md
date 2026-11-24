# Requisitos

- Windows x64 o un Mac con Apple Silicon (macOS ARM64). El núcleo MCP es un ejecutable nativo escrito en Rust, por lo que no es necesario instalar Node.js ni Rust.
- La configuración inicial en Windows y macOS usa el ejecutable incluido. No se necesita PowerShell 7 (`pwsh`).
- Una clave de API de TwitterAPI.io. El uso de la API se factura por consumo en ese servicio.
- Para publicar, las cookies de inicio de sesión v2 de la cuenta de destino y un proxy. Además de un proxy externo existente, en Windows y macOS la salida por línea residencial y el relé SSH integrados en Plumivox pueden compartirse entre varios chats. Es necesario preparar una vez el servidor de relé público.
