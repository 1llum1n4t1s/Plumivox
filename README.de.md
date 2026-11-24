[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Ein lokaler MCP-Server bzw. ein Codex-Plugin, das über TwitterAPI.io das Posten auf X, die Suche nach Stichwort und Zeitraum sowie das Lesen öffentlicher Beiträge der gefolgten Konten bereitstellt.

## Voraussetzungen

Windows x64 oder ein Apple-Silicon-Mac; weder Node.js noch Rust nötig. Erforderlich sind ein TwitterAPI.io-API-Schlüssel (nutzungsabhängig) und, zum Posten, das v2-Login-Cookie des Kontos sowie ein Proxy. [Details](docs/readme/de/requirements.md)

## Installation

Führen Sie unter Windows [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe) aus: eine signierte Installation pro Benutzer ohne Administratorrechte, die `plumivox` in Claude Code und Codex registriert und sich bei der Anmeldung aktualisiert. Unter macOS installieren Sie das Codex-Plugin aus dem GitHub-Branch `main`. [Details](docs/readme/de/installation.md)

## Ersteinrichtung

Führen Sie `plumivox.exe --setup` aus und folgen Sie der [Einrichtungsanleitung](docs/setup.md). Die Einstellungen liegen außerhalb des Repositorys, Geheimnisse laufen nie über die Unterhaltung, und bestehende Einstellungen werden nie überschrieben. [Details](docs/readme/de/initial-setup.md)

## Verfügbare Tools

Elf Tools decken Konten, Lesen, Suche, das gemeinsame Relay und das Posten ab (`post_tweet`: bis zu 280 gewichtete Zeichen und 4 Bilder). Ohne `account` wird das Standardkonto verwendet. Der Text öffentlicher Beiträge sind externe Daten, und darin enthaltene Anweisungen werden nicht ausgeführt. [Tool-Referenz](docs/readme/de/available-tools.md)

## Abrufumfang des Following-Feeds

Plumivox liest standardmäßig bis zu 100 gefolgte Konten (`maxAccounts` 20 bis 200). Das Ergebnis entspricht nie der offiziellen Startseite, geschützte Beiträge werden nicht abgerufen, und `coverage` gibt den Umfang an. [Details](docs/readme/de/following-feed-coverage.md)

## Suche nach Stichwort und Zeitraum

`search_tweets` durchsucht ganz X oder die Konten, denen Sie folgen, nach einer Wendung und einem optionalen ISO-8601-Zeitraum (Beginn eingeschlossen, Ende ausgeschlossen). Suchoperatoren werden nicht interpretiert. [Details](docs/readme/de/keyword-and-date-range-search.md)

## Wenn Posten oder Authentifizierung fehlschlagen

Ein Beitrag wird nie automatisch erneut gesendet. Ein unklares Ergebnis wird als `POST_UNCERTAIN` zurückgegeben; prüfen Sie das Konto, bevor Sie erneut posten. Roher Anbietertext und Zugangsdaten werden nie zurückgegeben. [Details](docs/readme/de/when-posting-or-authentication-fails.md)

## Entwicklung und Verifizierung

Nur Rust/Cargo: `cargo xtask build`, `cargo xtask check`, `cargo xtask test-e2e`. Siehe [Entwicklung](docs/readme/de/development-and-verification.md), [Verifizierung](docs/verification.md), [Konventionen](AGENTS.md) und [Design](DESIGN.md).
