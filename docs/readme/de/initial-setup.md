# Ersteinrichtung

Bei der ersten Verwendung folgen Sie der [Einrichtungsanleitung](../../setup.md). Mitgeliefert werden native Einrichtungsassistenten für Windows und macOS sowie ein statischer Rust-VPS-Installer für Linux x86_64 und aarch64. Für andere CPUs bleibt ein Bash-kompatibler Pfad erhalten. Wenn Sie Codex bitten „Richte Plumivox ein“, kann Sie auch der mitgelieferte Skill `setup-plumivox` anleiten.

Die Konfigurationsdatei liegt außerhalb des Repositorys. Neue Benutzer verwenden unter Windows `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` und unter macOS `~/Library/Application Support/Kagayoi/Plumivox/settings.json`. Benutzer mit einer vorhandenen `~/dev/Secret/secrets.json` lesen deren `twitterapi_io`. Auch `release_posting` und `blog_posting` von vava/blbl bleiben erhalten. Für einen anderen Speicherort geben Sie `PLUMIVOX_SECRETS_FILE` an. Geheime Werte müssen weder im Gespräch noch in MCP-Argumenten übergeben werden.

Kann die ausgewählte Konfigurationsdatei nicht gelesen werden, wird nicht auf eine andere Konfiguration gewechselt, sondern ein Fehler ausgegeben. Wenn Sie den Speicherort ändern, starten Sie Codex neu. Die Ersteinrichtung überschreibt keine vorhandenen Schlüssel, gleichnamigen Konten oder Einstellungen des gemeinsamen Relays. Wie Sie vorhandene Einstellungen ändern, steht in der [Einrichtungsanleitung](../../setup.md).

[Erste Einrichtung](../../setup.md) · [Konten und Cookies konfigurieren](../../configuration.md) · [Proxy mit Plumivox verwalten](../../managed-proxy.md)
