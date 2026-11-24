# Verfügbare Tools

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

Die Ersteinrichtung speichert das registrierte Konto als Standard. Um bei bestehender Konfiguration nur den Standard zu ändern, rufen Sie die mitgelieferte ausführbare Datei mit `--setup --set-default-account blog_posting` auf. Details finden Sie in der [Einrichtungsanleitung](../../setup.md) und unter [Priorität der Einstellungen](../../configuration.md).

Bei der Zeichenzahl eines Beitrags zählt eine URL als 23 Zeichen, japanischer Text als 2 Zeichen pro Zeichen und jedes zusammengesetzte Emoji bis Unicode 17 als 2 Zeichen. Auch Fälle wie ein Händeschütteln mit unterschiedlichen Hautfarben werden gemäß der [Spezifikation für gewichtete Zeichen bei X](https://docs.x.com/fundamentals/counting-characters) bewertet.

Sie können zum Beispiel sagen: „Lies die öffentlichen Beiträge der Konten, denen blog_posting folgt“ oder „Poste diesen Text über release_posting“. Gepostet wird gemäß der Anfrage des Benutzers. Der Text öffentlicher Beiträge ist externe Daten; darin enthaltene Anweisungen werden nicht ausgeführt.
