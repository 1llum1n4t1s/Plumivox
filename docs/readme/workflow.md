# Migrating from the plugin to Setup

Setup replaces a same-named `plumivox` registration that points to the old plugin's `dist` or similar with one that points to the install location. The MCP server provided by the plugin itself (`plumivox@plumivox-local`) is a separate registration, so to avoid starting Plumivox through two routes, turn it off or remove the plugin.

In Codex, add the same `enabled = false` as in [Toolport configuration](configuration.md) (or remove the plugin); in Claude Code, disable or uninstall the plugin from `/plugin`.

Codex prefers a same-named registration over the plugin's MCP, so it should start only one while the Setup registration exists, but Claude Code may start both.

Setup never edits the Codex or Claude Code settings; if the plugin is still enabled it only warns (in the guide shown when you launch Plumivox, in `app-update.log`, and as `legacyPlugins` in `app-update\setup-registration.json`).

Toolport registrations are not rewritten either (Setup only reads them to decide whether to skip the direct registration on a new install). Note for Codex: the direct registration is what suppresses the old plugin's MCP (a same-named user registration wins). If Setup skips it because Toolport serves Plumivox and the old plugin is still enabled, that suppression is gone and Plumivox can start twice (through Toolport and through the plugin); the warning above still appears, so disable the plugin's MCP or remove the plugin. If you added Toolport after Setup had registered Plumivox directly, remove the direct registrations yourself as described in [Toolport configuration](configuration.md).
