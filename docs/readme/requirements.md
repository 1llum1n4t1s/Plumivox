# Requirements

- Windows x64 or an Apple Silicon Mac (macOS ARM64). The MCP server itself is a native executable written in Rust, so you do not need to install Node.js or Rust.
- Initial setup on Windows and macOS uses the bundled executable. PowerShell 7 (`pwsh`) is not required.
- A TwitterAPI.io API key. Using the API incurs that service's pay-as-you-go charges.
- For posting, the target account's v2 login cookie and a proxy. In addition to an existing external proxy, on Windows and macOS you can share Plumivox's own residential egress and SSH relay across multiple chats. The public relay server needs a one-time initial preparation.
