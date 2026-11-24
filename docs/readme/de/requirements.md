# Voraussetzungen

- Windows x64 oder ein Apple-Silicon-Mac (macOS ARM64). Der MCP-Server selbst ist eine native, in Rust geschriebene ausführbare Datei; Node.js oder Rust müssen nicht installiert werden.
- Die Ersteinrichtung unter Windows und macOS verwendet die mitgelieferte ausführbare Datei. PowerShell 7 (`pwsh`) wird nicht benötigt.
- Ein API-Schlüssel für TwitterAPI.io. Die API-Nutzung wird von diesem Dienst nutzungsabhängig abgerechnet.
- Zum Posten werden die v2-Login-Cookies des Zielkontos und ein Proxy benötigt. Neben einem vorhandenen externen Proxy können unter Windows und macOS ein Plumivox-interner Ausgang über einen Privatanschluss und ein SSH-Relay von mehreren Chats gemeinsam genutzt werden. Der öffentliche Relay-Server muss einmalig vorbereitet werden.
