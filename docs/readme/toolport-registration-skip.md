# Skipping the direct registration (Setup version)

Per client (Claude Code `~/.claude.json`, Codex `~/.codex/config.toml`), Setup skips the direct registration only when the Toolport gateway is registered and exists, and the Toolport registry has an enabled server in that gateway's profiles whose `command` file name (case-insensitive, without `.exe`) is `plumivox` and which launches with `--stdio`. The check uses the same rule on Windows, macOS and Linux (Setup itself is currently Windows only); when it cannot be determined, Setup registers directly. If the gateway or server executable later disappears, Setup registers directly at the next start or sign-in; it does not re-add clients you removed or servers you disabled in Toolport. Results are logged in `app-update.log`.

If that Toolport server launches a different executable than this Setup's own (an old plugin `dist`, a copy, and so on), Setup still skips the direct registration but records `toolport-foreign` and warns in the launch guide (with the Toolport command) and in `app-update.log` (without it). Point the command to `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (expanded). Setup rechecks at every start, so the warning disappears once the command matches.

Set `mcp_registration` in `app-update\app-update.json` in the data folder:

| Value | Behavior |
|---|---|
| `auto` (default) | Skip when Toolport serves Plumivox. |
| `direct` | Always register directly. |
| `off` | Never add registrations automatically; uninstall still removes them. |
