# Installation

## Windows: Installation mit Setup (empfohlen)

1. Laden Sie [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe) herunter und führen Sie die Datei aus. Die Installation erfolgt pro Benutzer, Administratorrechte sind nicht erforderlich. Das Setup trägt die Codesignatur des Autors (`Open Source Developer Yuichiro Shinozaki`).
2. Plumivox wird nach `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` installiert. Sind die CLIs `claude` und `codex` im PATH vorhanden, wird der MCP-Server `plumivox` (Argument `--stdio`) automatisch bei Claude Code (pro Benutzer) und Codex registriert. Wurde keine CLI gefunden, erfolgt die Registrierung, sobald Sie die CLI später installieren und sich danach erneut anmelden oder Plumivox über das Startmenü starten.
3. Folgen Sie den Hinweisen auf dem Bildschirm direkt nach der Installation und führen Sie im Terminal `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` aus, um die [Ersteinrichtung](../../../README.de.md#ersteinrichtung) vorzunehmen. In Claude Code und Codex steht Plumivox ab der nächsten neuen Sitzung zur Verfügung.

Updates erfolgen automatisch. Während der MCP-Server läuft, wird eine neue Version von R2 (`https://plumivox-updates.kagayoi.com/velopack/`) abgerufen und bei der Anmeldung am PC nur dann angewendet, wenn Plumivox (MCP und gemeinsames Relay) nicht läuft. Läuft es, wird das Update auf die nächste Anmeldung verschoben; es wird nichts zwangsweise beendet. Vor dem Anwenden wird der Signaturgeber geprüft; stimmt er nicht überein, wird das Update nicht angewendet. Um automatische Updates abzuschalten, setzen Sie `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` auf `{"auto_update": false}`. Ein Protokoll der Abläufe liegt im selben Ordner unter `app-update.log`.

Zum Deinstallieren wählen Sie Plumivox in der Liste unter „Einstellungen“ → „Apps“. Dabei werden nur die auf diese Installation verweisenden `plumivox`-Registrierungen in Claude Code und Codex sowie die Update-Registrierung für die Anmeldung entfernt; der Datenordner mit Einstellungen und Sitzungen (`%LOCALAPPDATA%\Kagayoi\Plumivox`) bleibt erhalten. Wenn Sie zuerst nur die Registrierungen entfernen möchten, prüfen Sie den geplanten Ablauf mit `plumivox.exe --uninstall-setup` und führen Sie ihn mit `--uninstall-setup --apply` aus.

Beim Umstieg vom Plugin (siehe unten): Das Setup ersetzt eine gleichnamige `plumivox`-Registrierung, die etwa auf `dist` des alten Plugins verweist, durch eine Registrierung, die auf das Installationsziel verweist. Das vom Plugin selbst bereitgestellte MCP (`plumivox@plumivox-local`) ist eine eigene Registrierung; damit Plumivox nicht über zwei Wege startet, deaktivieren Sie es oder entfernen Sie das Plugin. In Codex fügen Sie dasselbe `enabled = false` wie in [Abschnitt zu Toolport](#verwendung-über-toolport) hinzu (oder entfernen das Plugin); in Claude Code deaktivieren oder deinstallieren Sie das Plugin über `/plugin`. Codex bevorzugt eine gleichnamige Registrierung gegenüber dem MCP des Plugins und sollte daher, solange die Setup-Registrierung besteht, nur eines starten, Claude Code kann jedoch beide starten. Das Setup ändert die Einstellungen von Codex und Claude Code nie; ist das Plugin noch aktiviert, warnt es nur (in der Anleitung beim Start von Plumivox, in `app-update.log` und als `legacyPlugins` in `app-update\setup-registration.json`). Auch Toolport-Registrierungen werden nicht automatisch umgeschrieben; wenn Sie Toolport nutzen, prüfen Sie es selbst wie in [Abschnitt zu Toolport](#verwendung-über-toolport) beschrieben.

## Installation als Plugin (macOS, bisherige Methode)

Für macOS (Apple Silicon) wird diese Methode verwendet, bis das Setup dafür ausgeliefert wird. Unter Windows kann sie weiterhin genutzt werden.

Die Installation erfolgt aus dem neuesten Commit von `main` im GitHub-Repository. Ein lokaler Entwicklungsklon des Benutzers wird nicht verwendet, und es wird weder auf ein Tag noch auf einen Commit-SHA festgelegt.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` ist der Bezeichner des Marketplace. Die Quelle ist die oben genannte GitHub-URL. Da `dist/plumivox.exe` für Windows x64 und `dist/plumivox` (ARM64) für Apple-Silicon-Macs mitgeliefert werden, müssen Benutzer weder Abhängigkeiten installieren noch die MCP-Konfiguration manuell ergänzen.

Die Entwicklungs-CI legt die unter Windows und macOS verifizierten Build-Artefakte in Cloudflare R2 ab und übergibt sie bei der Vorbereitung der Auslieferung. Die [Anleitung zum Ablegen und Abrufen der Artefakte](../../distribution.md) richtet sich an Entwickler. Plugin-Benutzer benötigen keine R2-Konfiguration und keine Zugangsdaten; Installation und Updates erfolgen weiterhin über `main` auf GitHub.

Zum Aktualisieren rufen Sie den neuesten Stand von `main` ab, installieren ihn und starten Codex neu, damit er geladen wird.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Prüfen Sie die unter `installed` angezeigte Versionsnummer. Ein bereits installiertes Plugin wird nach dem Update nicht erneut mit `add` hinzugefügt; starten Sie stattdessen Codex neu, um den neuen MCP-Server und die neuen Skills zu laden.

Wie Sie einen GitHub folgenden Marketplace registrieren und aktualisieren, beschreibt die [Codex-Plugin-Dokumentation](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

## Verwendung über Toolport

Sie können auch nur den MCP-Server in Toolport registrieren. Da Plumivox die Toolport-Registrierung nicht automatisch ändert, fügen Sie je nach Installationsart den folgenden stdio-Server hinzu oder passen ihn an. Geben Sie weder einen Entwicklungsklon noch einen Installations-Cache mit Versionsnummer an.

- Name: `plumivox`
- Befehl: Für die Setup-Version unter Windows `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (`%LOCALAPPDATA%` zum tatsächlichen Pfad erweitert angeben; der Pfad `current` bleibt gleich, auch wenn sein Inhalt bei Updates ausgetauscht wird). Für die Plugin-Version ermitteln Sie mit `codex plugin marketplace list` das abgerufene Stammverzeichnis von `plumivox-local`; unter Windows `<abgerufenes Stammverzeichnis>/dist/plumivox.exe`, unter macOS `<abgerufenes Stammverzeichnis>/dist/plumivox`
- Argumente: `--stdio`
- Arbeitsverzeichnis: das Home-Verzeichnis des Benutzers
- Anfrage-Timeout: 300000 Millisekunden (einschließlich der Zeit für Login und Relay-Vorbereitung)

Aktivieren Sie den Server in Toolport für das Profil, das Codex verwendet, und prüfen Sie mit `toolport_search_tools` und `server="plumivox"` die 11 Tools. Suchen Sie `list_accounts` und rufen Sie es auf; deaktivieren Sie die direkte Verbindung erst, wenn die Einstellungen geladen werden. Bei der Setup-Version entfernen Sie die direkten Registrierungen mit `codex mcp remove plumivox` und `claude mcp remove plumivox --scope user` (nachdem die Registrierung einmal erfolgt ist, wird sie nicht automatisch erneut angelegt, bis Sie per Setup neu installieren). Bei der Plugin-Version ergänzen Sie die Codex-Benutzerkonfiguration `config.toml` wie folgt, lassen das Plugin selbst aktiviert und behalten so den Einrichtungs-Skill.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Nach einem Neustart von Codex ist die direkte Verbindung deaktiviert. Danach suchen Sie Tools über Toolport und übergeben den zurückgegebenen tatsächlichen Toolnamen an `toolport_call_tool`. API-Schlüssel, Cookies und SSH-Schlüssel werden aus der bestehenden Plumivox-Konfiguration gelesen und nicht in die Toolport-Argumente übernommen.

Die Plugin-Version aktualisieren Sie mit dem oben genannten `codex plugin marketplace upgrade plumivox-local`. Da ein von Toolport ausgeführter Server bereits geladenen Code verwendet, deaktivieren und reaktivieren Sie nach dem Update `plumivox` in Toolport, damit die neue ausführbare Datei geladen wird. Hat sich das abgerufene Stammverzeichnis geändert, passen Sie auch den registrierten Befehl an. Beim Start werden weder Git-Abrufe noch Installationen von Abhängigkeiten durchgeführt.
