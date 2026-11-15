[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Ein lokaler MCP-Server bzw. ein Codex-Plugin, das über TwitterAPI.io das Posten auf X, die Suche nach Stichwort und Zeitraum sowie das Lesen öffentlicher Beiträge der gefolgten Konten bereitstellt.

## Voraussetzungen

- Windows x64 oder ein Apple-Silicon-Mac (macOS ARM64). Der MCP-Server selbst ist eine native, in Rust geschriebene ausführbare Datei; Node.js oder Rust müssen nicht installiert werden.
- Die Ersteinrichtung unter Windows und macOS verwendet die mitgelieferte ausführbare Datei. PowerShell 7 (`pwsh`) wird nicht benötigt.
- Ein API-Schlüssel für TwitterAPI.io. Die API-Nutzung wird von diesem Dienst nutzungsabhängig abgerechnet.
- Zum Posten werden die v2-Login-Cookies des Zielkontos und ein Proxy benötigt. Neben einem vorhandenen externen Proxy können unter Windows und macOS ein Plumivox-interner Ausgang über einen Privatanschluss und ein SSH-Relay von mehreren Chats gemeinsam genutzt werden. Der öffentliche Relay-Server muss einmalig vorbereitet werden.

## Installation

### Windows: Installation mit Setup (empfohlen)

1. Laden Sie [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe) herunter und führen Sie die Datei aus. Die Installation erfolgt pro Benutzer, Administratorrechte sind nicht erforderlich. Das Setup trägt die Codesignatur des Autors (`Open Source Developer Yuichiro Shinozaki`).
2. Plumivox wird nach `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` installiert. Sind die CLIs `claude` und `codex` im PATH vorhanden, wird der MCP-Server `plumivox` (Argument `--stdio`) automatisch bei Claude Code (pro Benutzer) und Codex registriert. Wurde keine CLI gefunden, erfolgt die Registrierung, sobald Sie die CLI später installieren und sich danach erneut anmelden oder Plumivox über das Startmenü starten.
3. Folgen Sie den Hinweisen auf dem Bildschirm direkt nach der Installation und führen Sie im Terminal `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` aus, um die [Ersteinrichtung](#ersteinrichtung) vorzunehmen. In Claude Code und Codex steht Plumivox ab der nächsten neuen Sitzung zur Verfügung.

Updates erfolgen automatisch. Während der MCP-Server läuft, wird eine neue Version von R2 (`https://plumivox-updates.kagayoi.com/velopack/`) abgerufen und bei der Anmeldung am PC nur dann angewendet, wenn Plumivox (MCP und gemeinsames Relay) nicht läuft. Läuft es, wird das Update auf die nächste Anmeldung verschoben; es wird nichts zwangsweise beendet. Vor dem Anwenden wird der Signaturgeber geprüft; stimmt er nicht überein, wird das Update nicht angewendet. Um automatische Updates abzuschalten, setzen Sie `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` auf `{"auto_update": false}`. Ein Protokoll der Abläufe liegt im selben Ordner unter `app-update.log`.

Zum Deinstallieren wählen Sie Plumivox in der Liste unter „Einstellungen“ → „Apps“. Dabei werden nur die auf diese Installation verweisenden `plumivox`-Registrierungen in Claude Code und Codex sowie die Update-Registrierung für die Anmeldung entfernt; der Datenordner mit Einstellungen und Sitzungen (`%LOCALAPPDATA%\Kagayoi\Plumivox`) bleibt erhalten. Wenn Sie zuerst nur die Registrierungen entfernen möchten, prüfen Sie den geplanten Ablauf mit `plumivox.exe --uninstall-setup` und führen Sie ihn mit `--uninstall-setup --apply` aus.

Beim Umstieg vom Plugin (siehe unten): Das Setup ersetzt eine gleichnamige `plumivox`-Registrierung, die etwa auf `dist` des alten Plugins verweist, durch eine Registrierung, die auf das Installationsziel verweist. Das vom Plugin selbst bereitgestellte MCP (`plumivox@plumivox-local`) ist eine eigene Registrierung; damit Plumivox nicht über zwei Wege startet, deaktivieren Sie es oder entfernen Sie das Plugin. In Codex fügen Sie dasselbe `enabled = false` wie in [Abschnitt zu Toolport](#verwendung-über-toolport) hinzu (oder entfernen das Plugin); in Claude Code deaktivieren oder deinstallieren Sie das Plugin über `/plugin`. Codex bevorzugt eine gleichnamige Registrierung gegenüber dem MCP des Plugins und sollte daher, solange die Setup-Registrierung besteht, nur eines starten, Claude Code kann jedoch beide starten. Das Setup ändert die Einstellungen von Codex und Claude Code nie; ist das Plugin noch aktiviert, warnt es nur (in der Anleitung beim Start von Plumivox, in `app-update.log` und als `legacyPlugins` in `app-update\setup-registration.json`). Auch Toolport-Registrierungen werden nicht automatisch umgeschrieben; wenn Sie Toolport nutzen, prüfen Sie es selbst wie in [Abschnitt zu Toolport](#verwendung-über-toolport) beschrieben.

### Installation als Plugin (macOS, bisherige Methode)

Für macOS (Apple Silicon) wird diese Methode verwendet, bis das Setup dafür ausgeliefert wird. Unter Windows kann sie weiterhin genutzt werden.

Die Installation erfolgt aus dem neuesten Commit von `main` im GitHub-Repository. Ein lokaler Entwicklungsklon des Benutzers wird nicht verwendet, und es wird weder auf ein Tag noch auf einen Commit-SHA festgelegt.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` ist der Bezeichner des Marketplace. Die Quelle ist die oben genannte GitHub-URL. Da `dist/plumivox.exe` für Windows x64 und `dist/plumivox` (ARM64) für Apple-Silicon-Macs mitgeliefert werden, müssen Benutzer weder Abhängigkeiten installieren noch die MCP-Konfiguration manuell ergänzen.

Die Entwicklungs-CI legt die unter Windows und macOS verifizierten Build-Artefakte in Cloudflare R2 ab und übergibt sie bei der Vorbereitung der Auslieferung. Die [Anleitung zum Ablegen und Abrufen der Artefakte](docs/distribution.md) richtet sich an Entwickler. Plugin-Benutzer benötigen keine R2-Konfiguration und keine Zugangsdaten; Installation und Updates erfolgen weiterhin über `main` auf GitHub.

Zum Aktualisieren rufen Sie den neuesten Stand von `main` ab, installieren ihn und starten Codex neu, damit er geladen wird.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Prüfen Sie die unter `installed` angezeigte Versionsnummer. Ein bereits installiertes Plugin wird nach dem Update nicht erneut mit `add` hinzugefügt; starten Sie stattdessen Codex neu, um den neuen MCP-Server und die neuen Skills zu laden.

Wie Sie einen GitHub folgenden Marketplace registrieren und aktualisieren, beschreibt die [Codex-Plugin-Dokumentation](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

### Verwendung über Toolport

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

## Ersteinrichtung

Bei der ersten Verwendung folgen Sie der [Einrichtungsanleitung](docs/setup.md). Mitgeliefert werden native Einrichtungsassistenten für Windows und macOS sowie ein statischer Rust-VPS-Installer für Linux x86_64 und aarch64. Für andere CPUs bleibt ein Bash-kompatibler Pfad erhalten. Wenn Sie Codex bitten „Richte Plumivox ein“, kann Sie auch der mitgelieferte Skill `setup-plumivox` anleiten.

Die Konfigurationsdatei liegt außerhalb des Repositorys. Neue Benutzer verwenden unter Windows `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` und unter macOS `~/Library/Application Support/Kagayoi/Plumivox/settings.json`. Benutzer mit einer vorhandenen `~/dev/Secret/secrets.json` lesen deren `twitterapi_io`. Auch `release_posting` und `blog_posting` von vava/blbl bleiben erhalten. Für einen anderen Speicherort geben Sie `PLUMIVOX_SECRETS_FILE` an. Geheime Werte müssen weder im Gespräch noch in MCP-Argumenten übergeben werden.

Kann die ausgewählte Konfigurationsdatei nicht gelesen werden, wird nicht auf eine andere Konfiguration gewechselt, sondern ein Fehler ausgegeben. Wenn Sie den Speicherort ändern, starten Sie Codex neu. Die Ersteinrichtung überschreibt keine vorhandenen Schlüssel, gleichnamigen Konten oder Einstellungen des gemeinsamen Relays. Wie Sie vorhandene Einstellungen ändern, steht in der [Einrichtungsanleitung](docs/setup.md).

[Erste Einrichtung](docs/setup.md) · [Konten und Cookies konfigurieren](docs/configuration.md) · [Proxy mit Plumivox verwalten](docs/managed-proxy.md)

## Verfügbare Tools

| Tool | Beschreibung |
| --- | --- |
| `list_accounts` | Kontonamen und Bereitschaft für Lesen und Posten. Keine externe Kommunikation |
| `login_account` | Verwendet gespeicherte Cookies wieder; fehlen sie, wird mit den konfigurierten Zugangsdaten angemeldet und verschlüsselt gespeichert |
| `refresh_session` | Meldet sich explizit erneut an und aktualisiert die Sitzung zum Posten |
| `get_my_posts` | Beiträge des eigenen Profils. Einbeziehung von Antworten und Seitenumbruch einstellbar |
| `get_tweets` | Ruft öffentliche Beiträge anhand ihrer ID ab. Höchstens 20 |
| `get_following_feed` | Ruft öffentliche Beiträge der gefolgten Konten ab und sortiert sie absteigend nach Zeitpunkt |
| `search_tweets` | Sucht mit Stichwort und Zeitraum öffentliche Beiträge auf ganz X oder bei den gefolgten Konten |
| `get_proxy_status` | Status des gemeinsamen Relays, das dieser MCP-Server nutzt. Startet das Relay nicht |
| `start_proxy` | Startet den konfigurierten Ausgang über den Privatanschluss und das SSH-Relay. Postet nicht und meldet sich nicht an |
| `stop_proxy` | Stoppt das verwendete gemeinsame Relay. Wirkt sich auch auf andere Chats mit demselben Relay aus. Wird bei der nächsten Post- oder Authentifizierungsanfrage neu gestartet |
| `post_tweet` | Beitrag mit höchstens 280 gewichteten Zeichen. Bis zu 4 PNG/JPEG-Bilder, jeweils höchstens 5 MiB. Bilder per absolutem Pfad angeben |

Wird `account` weggelassen, verwenden Suche, Lesen, Posten und Authentifizierung das Standardkonto. Über `isDefault` von `list_accounts` sehen Sie, welches Konto bei Weglassen verwendet wird; für ein anderes Konto geben Sie in `account` explizit einen Konfigurationsnamen wie `release_posting` oder `blog_posting` an. Sind mehrere Konten registriert und keines ist Standard, wird eine Angabe verlangt; bei einem ungültigen Standard werden Aufrufe ohne Angabe abgelehnt.

Die für Eingabezeichenketten der einzelnen Tools (einschließlich Zeichenketten in Arrays) definierten Längenbegrenzungen werden zusätzlich anhand der Anzahl der UTF-16-Codeeinheiten vor dem Entfernen von Leerzeichen geprüft. Emojis außerhalb der BMP und ähnliche Zeichen zählen als ein Zeichen, aber als zwei Einheiten. Da `minLength` und `maxLength` in JSON Schema Unicode-Zeichen zählen, kann ein Standard-JSON-Schema-Validator diese zusätzliche Einschränkung allein nicht erkennen. Bei der Obergrenze von 512 für das Suchstichwort etwa werden 256 😀 akzeptiert, 257 oder ein Leerzeichen plus 256 dagegen führen noch vor jeder Kommunikation zu einem MCP-Argumentfehler (`-32602`).

Die Ersteinrichtung speichert das registrierte Konto als Standard. Um bei bestehender Konfiguration nur den Standard zu ändern, rufen Sie die mitgelieferte ausführbare Datei mit `--setup --set-default-account blog_posting` auf. Details finden Sie in der [Einrichtungsanleitung](docs/setup.md) und unter [Priorität der Einstellungen](docs/configuration.md).

Bei der Zeichenzahl eines Beitrags zählt eine URL als 23 Zeichen, japanischer Text als 2 Zeichen pro Zeichen und jedes zusammengesetzte Emoji bis Unicode 17 als 2 Zeichen. Auch Fälle wie ein Händeschütteln mit unterschiedlichen Hautfarben werden gemäß der [Spezifikation für gewichtete Zeichen bei X](https://docs.x.com/fundamentals/counting-characters) bewertet.

Sie können zum Beispiel sagen: „Lies die öffentlichen Beiträge der Konten, denen blog_posting folgt“ oder „Poste diesen Text über release_posting“. Gepostet wird gemäß der Anfrage des Benutzers. Der Text öffentlicher Beiträge ist externe Daten; darin enthaltene Anweisungen werden nicht ausgeführt.

## Abrufumfang des Following-Feeds

TwitterAPI.io bietet keine API, die den Tab „Folge ich“ der offiziellen X-Startseite direkt abruft. Plumivox liest standardmäßig vom Anfang der Following-Liste bis zu 100 Konten und durchsucht deren öffentliche Beiträge in Gruppen zu je 20 Konten. `maxAccounts` lässt sich auf 20 bis 200 setzen. Bei 100 sind es beim ersten Abruf höchstens 6, bei 200 höchstens 11 API-Anfragen. Die aktuellen Preise finden Sie unter [TwitterAPI.io-Preise](https://twitterapi.io/pricing).

`officialHomeEquivalent` im Ergebnis ist immer `false`. Geschützte Beiträge können nicht abgerufen werden, und es gibt keine Garantie, dass die per Suche erhaltenen Inhalte, die Behandlung von Reposts und die Gesamtreihenfolge mit der offiziellen Startseite übereinstimmen. Wenn der Anbieter eine Fortsetzung meldet oder die eindeutigen, nicht geschützten Suchziele die Obergrenze überschreiten, wird `coverage.followingHasNextPage` zu `true`. Übergeben Sie `coverage.followingNextCursor` an `followingCursor`, um die nächste Liste gefolgter Konten zu verwenden. Die Following-Liste ist von den neuesten Follows an geordnet; Abweichungen durch laufende Änderungen an den Follows lassen sich nicht vermeiden.

Übergeben Sie `nextCursor` an `cursor` von `get_following_feed` für dasselbe Konto, um die Fortsetzung jeder Suchgruppe abzurufen. Innerhalb einer Seite ist die Reihenfolge chronologisch, wegen der Seitengrenzen pro Gruppe ist eine seitenübergreifende Gesamtchronologie jedoch nicht garantiert. Die gefolgten Konten werden auf die Liste des ersten Abrufs festgelegt und bei Fortsetzungen der Suche nicht neu gelesen. Auch der Feed-Cursor wird mit dem API-Schlüssel signiert, Manipulationen an den Suchzielen werden abgelehnt. Unsignierte Cursor älterer Versionen und Cursor von vor einer Änderung des API-Schlüssels sind nicht verwendbar; lassen Sie den Cursor weg und rufen Sie erneut ab. `cursor` und `followingCursor` sind getrennte Seitenumbrüche und können nicht gleichzeitig angegeben werden.

In der [offiziellen Antwortspezifikation der Following-Liste](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings) ist kein Feld für den Schutzstatus erkennbar. Innerhalb der Antwort derselben Following-Liste werden gültige Benutzernamen ohne Beachtung der Groß-/Kleinschreibung zusammengefasst und in der Reihenfolge geschützt, öffentlich, unbekannt bewertet. Nach Ausschluss der als geschützt erkannten Ziele werden Ziele in der Reihenfolge ihres ersten Auftretens bis zur Obergrenze ausgewählt; die Anzahl der ausgewählten Ziele, deren Status nicht bestimmt werden kann, wird in `coverage.protectionStatusUnknownAccounts` zurückgegeben. `protectedAccountsExcluded` ist die Anzahl eindeutiger Konten, die in dieser Antwort als geschützt erkannt wurden. Doppelte und ungültige Benutzernamen werden nicht gezählt, geschützte Konten außerhalb der Abrufobergrenze dagegen schon, daher kann der Wert `maxAccounts` übersteigen. Auch bei 0 sind nicht unbedingt alle Ziele öffentliche Konten.

Fortsetzungen signierter Cursor, die von der alten Implementierung ausgegeben wurden, übernehmen neben Suchzielen und Anzahl auch die Bewertung des noch nicht durchsuchten Bereichs und der Fortsetzbarkeit durch die alte Implementierung; daher können Ziele außerhalb der Obergrenze übersprungen werden. Um zuverlässig mit der neuen Zusammenfassung des Schutzstatus, Zählung und Fortsetzungsbewertung erneut abzurufen, lassen Sie den Cursor weg.

Auch wenn Beiträge oder die Following-Liste eine Fortsetzung haben, kann nicht fortgesetzt werden, wenn der Cursor des Anbieters oder der kombinierte Cursor die Längenbegrenzung überschreitet. Die bereits abgerufenen Beiträge und `hasNextPage` bleiben erhalten, `nextCursor` ist `null`, `coverage.tweetContinuationAvailable` ist `false` und `tweetContinuationUnavailableReason` ist `cursor_too_long`. Für die Following-Liste prüfen Sie `followingNextCursor` sowie `followingContinuationAvailable` und `followingContinuationUnavailableReason`. Cursor werden nicht gekürzt, und bereits abgerufene Inhalte werden nicht als Fehler verworfen.

Gibt der Anbieter in einer einzigen Antwort mehr eindeutige, nicht geschützte Ziele als `maxAccounts` zurück, sind die Ziele außerhalb der Obergrenze nicht durchsucht. `followingHasNextPage` ist dann `true`, `followingContinuationUnavailableReason` ist `account_limit`, und eine Fortsetzung der Following-Liste wird nicht angeboten (`followingContinuationAvailable: false`, `followingNextCursor: null`). So wird verhindert, dass der nächste Cursor des Anbieters nicht durchsuchte Ziele überspringt. Ist `maxAccounts` kleiner als 200, können Sie die Obergrenze bis 200 erhöhen und ohne Cursor erneut abrufen. Liefert der Anbieter selbst bei einer Obergrenze von 200 zu viele Ziele, lassen sich die nicht durchsuchten Ziele dieser Antwort auf diesem Weg nicht abrufen. Wird die Obergrenze nur durch doppelte Zeilen mit gleichem Namen oder durch geschützte bzw. ungültige Namen überschritten, greift diese Einschränkung nicht. Fortsetzungen der Beiträge bereits ausgewählter Ziele lassen sich wie gewohnt abrufen, und der Zustand mit nicht durchsuchten Zielen wird ebenfalls übernommen.

## Suche nach Stichwort und Zeitraum

Sie können zum Beispiel sagen: „Suche auf ganz X nach Beiträgen zur Spracherkennung vom 1. bis 7. Oktober“ oder „Suche mit denselben Bedingungen bei den Konten, denen blog_posting folgt“. Die Suche wird auf API-Seite eingegrenzt und liefert Text, Autor, Zeitpunkt und URL der Beiträge. Eine semantische Suche findet nicht statt.

`scope` von `search_tweets` ist `all` (Standard, öffentliche Suche auf ganz X) oder `following` (die gefolgten Konten des angegebenen Kontos). `keyword` ist erforderlich; der Ausdruck ohne führende und abschließende Leerzeichen wird als Phrase gesucht. Auch enthaltene `OR` oder `from:` werden nicht als Suchoperatoren behandelt. Ausdrücke mit Anführungszeichen, Backslashes oder Steuerzeichen werden abgelehnt.

`startTime` und `endTime` sind optionale Zeitpunkte im ISO-8601-Format mit Zeitzone und Sekundengenauigkeit. Gesucht wird in einem Zeitraum, der den Beginn einschließt und das Ende ausschließt. Ein reines Datum wird nicht angegeben; Codex wandelt Datum und Zeitzone der Anfrage in Zeitpunkte um. Für den 1. bis 7. Oktober in japanischer Zeit lauten die Bedingungen beispielsweise:

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

Die Suche ruft zuerst die neuesten Beiträge ab. Übergeben Sie `nextCursor` an `cursor` des nächsten Aufrufs, um die Fortsetzung mit denselben Bedingungen abzurufen. Wenn Sie Konto, Ziel, Stichwort, Zeitraum oder die Obergrenze der gefolgten Konten ändern, lassen Sie den Cursor weg und beginnen eine neue Suche. Auch nach einer Änderung des API-Schlüssels sind vorhandene Such-Cursor nicht verwendbar.

`following` kann `maxAccounts` (20 bis 200, Standard 100) und `followingCursor` verwenden. Wenn Sie `coverage.followingNextCursor` übergeben, wird die nächste Liste gefolgter Konten mit denselben Bedingungen durchsucht. Gleichzeitig mit `cursor` der Beitragssuche kann dies nicht angegeben werden. Bei `all` werden diese beiden Argumente nicht angegeben. Cursor des vorhandenen `get_following_feed` und der Suche sind nicht untereinander austauschbar. Auch Such-Cursor älterer Versionen für gefolgte Konten, die einen alten unsignierten Feed-Cursor enthalten, sind nicht verwendbar; lassen Sie den Cursor weg und suchen Sie erneut.

Die Suche auf ganz X benötigt eine API-Anfrage pro Seite, die Suche bei gefolgten Konten wie der vorhandene Feed höchstens 6 (100 Personen) bzw. 11 (200 Personen) API-Anfragen. Die Vollständigkeit geschützter Beiträge oder aller Beiträge im Suchzeitraum ist nicht garantiert; der Abrufumfang wird in `coverage` zurückgegeben. Die Zeitraumoperatoren werden gemäß der [Beschreibung von TwitterAPI.io](https://twitterapi.io/blog/search-a-tweet) in UTC umgerechnet.

## Wenn Posten oder Authentifizierung fehlschlagen

Bei Erfolg werden die Beitrags-ID und die URL auf X zurückgegeben. Kommunikationsfehler beim Erstellen eines Beitrags oder eine fehlende Beitrags-ID werden als unbestätigt gemeldet und nicht automatisch erneut gesendet. Wenn Sie ausdrücklich darum bitten, denselben Text noch einmal zu posten, wird bei jedem Aufruf einmal gesendet. Es gibt keinen Mechanismus, der anhand des Verlaufs oder eines Hashs des Textes stoppt.

Das Standardzeitfenster für die Kommunikation beträgt 45 Sekunden für normale API-Anfragen und 120 Sekunden pro Bild für Bild-Uploads. Nur Bild-Uploads werden bei vorübergehenden Fehlern für dasselbe Bild höchstens einmal wiederholt. Betroffen sind HTTP 502, 503 und 504 (250 ms Wartezeit) sowie Störungen des Proxys oder der Gegenstelle, JSON-Ablehnungen unbekannter Ursache und Verbindungsabbrüche (1 Sekunde Wartezeit); Authentifizierung, Guthaben, ungültige Bilder, Duplikate, Ratenbegrenzung und Timeouts werden nicht wiederholt. Wartezeit und Wiederholung bleiben innerhalb des Zeitfensters des jeweiligen Bildes; frühere Bilder werden nicht erneut gesendet, und auch Beitragserstellung und Login werden nicht automatisch wiederholt. Schlägt ein Bild fehl, wird der Beitrag nicht gesendet; ist nach dem Erstellen des Beitrags der Erfolg unklar, wird `POST_UNCERTAIN` zurückgegeben.

Bekannte Fehler bei der Beitragsverarbeitung enthalten `stage` (`input_validation`, `pre_send`, `image_upload`, `post_creation`), `postAttempted`, das angibt, ob eine Anfrage zur Beitragserstellung versucht wurde, und `uploadAttempts`, die Gesamtzahl der ausgeführten Bild-Upload-Anfragen. Fehlende Konten oder Cookies und fehlgeschlagene Relay-Vorbereitung gehören zu `pre_send`. Bei unbekannten internen Fehlern und abgelehnten MCP-Eingabeformaten werden diese Felder nicht zurückgegeben. `postAttempted: true` bedeutet nicht, dass der Beitrag erfolgreich war. Rohe Fehler des Anbieters und Zugangsdaten werden nicht zurückgegeben.

`API_UNAVAILABLE` bei der Bildkommunikation enthält `failureKind` und unterscheidet `timeout` (Zeitüberschreitung), `network` (Kommunikationsstörung) und `cancelled` (Abbruch). Auch in diesen Fällen werden Bilder und Beiträge nicht automatisch erneut gesendet.

Ablehnungsfehler der API (`API_REJECTED`, `API_HTTP_ERROR`) enthalten `httpStatus`, das Array fester Kategorien `rejectionReasons` und die Herkunft der Kategorisierung `rejectionReasonSource`. Diese Diagnoseinformationen bleiben auch erhalten, wenn ein 5xx bei der Beitragserstellung zu `POST_UNCERTAIN` wird. Die Herkunft ist `provider_message` (aus der JSON-Meldung des Anbieters abgeleitete Kandidaten) oder `http_status` (Kategorisierung anhand des HTTP-Status). Die Kandidaten bedeuten weder eine sichere Ursache noch eine Erlaubnis zum erneuten Senden; nicht zuordenbare Meldungen werden als `unknown` eingestuft. Roher Antworttext, beliebige Fehlercodes und Request-IDs werden weder zurückgegeben noch gespeichert.

Die Kandidaten für `provider_message` sind `api_key`, `authentication`, `account_locked`, `account_suspended`, `rate_limit`, `credits`, `proxy`, `media`, `duplicate`, `upstream_timeout`, `upstream_unavailable` und `unknown`. Für `http_status` kommen zusätzlich `invalid_request` (400), `unauthorized` (401), `forbidden` (403) und `endpoint_not_found` (404) hinzu. Lauten etwa beim Bild-Upload `rejectionReasons: ["proxy", "upstream_timeout"]` und `postAttempted: false`, deutet das auf ein Proxy- bzw. Upstream-Timeout hin, und es ist erkennbar, dass vor der Beitragserstellung abgebrochen wurde. Der Text von Nicht-2xx-Antworten wird nicht gelesen; die bestehenden Kommunikationsgrenzen und Wiederholungsbedingungen bleiben erhalten.

Überschreitet eine API-Antwort 10 MiB, wird der Empfang abgebrochen und ein Fehler zurückgegeben. Überschreitet die Antwort auf einen Beitrag die Grenze, wird ebenfalls nicht erneut gesendet, da der Beitrag möglicherweise bereits veröffentlicht wurde.

Ein fehlgeschlagener Abgleich gespeicherter Cookies stoppt nur das Posten. Lesen und Suchen öffentlicher Beiträge verwenden nur den API-Schlüssel und den Benutzernamen und funktionieren, ohne gespeicherte Cookies zu laden. Zum Abrufen und Aktualisieren authentifizierter Cookies siehe die [Konfigurationsanleitung](docs/configuration.md).

`login_account` verwendet gespeicherte Cookies wieder, sofern sie in einem nutzbaren Format vorliegen, und meldet nicht, ihre Gültigkeit auf X geprüft zu haben. Wird ein Ablauf oder eine Abweichung der Einstellungen festgestellt, rufen Sie `refresh_session` auf. Neue Cookies werden im Speicherort von Plumivox verschlüsselt gespeichert, unter Windows mit DPAPI, unter macOS mit einem Schlüssel aus dem Keychain; fehlgeschlagene Beiträge werden nicht automatisch erneut gesendet.

Bei einem Abbruch vor dem Ersetzen des Gespeicherten wird `AUTH_CANCELLED` zurückgegeben, wenn die exklusive Sperre für die Authentifizierungsaktualisierung nicht aufrechterhalten werden kann `AUTH_LOCK_LOST`, und für nicht speicherbare Cookies `LOGIN_FAILED`. Lässt sich das Ergebnis nach der Anweisung zum Ersetzen des Gespeicherten nicht bestätigen, lautet die Rückgabe `AUTH_SAVE_UNCERTAIN`, und die Aktualisierung ist möglicherweise bereits erfolgt. `SESSION_STORAGE_UNAVAILABLE` bezeichnet Fehler des Speicherorts oder der Speicherverarbeitung, etwa DPAPI unter Windows oder des Keychain unter macOS.

## Entwicklung und Verifizierung

Auch für Entwicklung, Build und Tests wird kein Node.js benötigt. Verwendet werden `cargo xtask build`, `cargo xtask check` und `cargo xtask test-e2e` mit Rust/Cargo. Registrierungen, die den alten Node-Launcher angeben, stellen Sie bitte auf den oben beschriebenen nativen Start um.

`cargo xtask build` unter Windows aktualisiert `dist/plumivox.exe` nach dem Signieren und der Signaturprüfung mit Sigilune (Cloud-Signatur über Certum SimplySign). Auch die Auslieferungs-CI signiert die Windows-Artefakte mit Sigilune, bevor sie sie ablegt. Die für das Signieren nötige Konfiguration des Entwicklungsrechners und die CI-Secrets finden Sie unter [Auslieferungsartefakte](docs/distribution.md#windows配布物のコード署名). Das Windows-Setup wird mit `cargo xtask velopack-pack` gepackt und mit `cargo xtask test-setup-velopack-e2e` isoliert verifiziert, ohne eine echte Installation zu berühren. Die Auslieferung erfolgt ausschließlich über den Setup-Auslieferungs-Job von `distribution.yml`, der bei einem Push auf einen `release/<Version>`-Branch startet ([Auslieferung des Setups (Velopack)](docs/distribution.md#setupvelopackの配信)).

Siehe [Build- und Verifizierungsanleitung für Entwickler](docs/verification.md), [Arbeitsregeln](AGENTS.md) und [Systemdesign](DESIGN.md).
