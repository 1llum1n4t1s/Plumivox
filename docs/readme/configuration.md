# Toolport configuration

You can register only the MCP server with Toolport. Plumivox does not modify Toolport registrations, so add the following stdio server to match your installation method. Do not use a development clone or a versioned install cache.

- Name: `plumivox`
- Command: Setup version: `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (expanded; `current` stays the same across updates). Plugin version: `<fetched root>/dist/plumivox.exe` (Windows) or `<fetched root>/dist/plumivox` (macOS), where the root is shown by `codex plugin marketplace list` for `plumivox-local`
- Arguments: `--stdio`
- Working directory: your home directory
- Request timeout: 300000 milliseconds (includes login and relay preparation)

Enable it in the Toolport profile Codex uses, confirm the 11 tools with `toolport_search_tools` using `server="plumivox"`, call `list_accounts`, then disable the direct connection.

- **Setup version:** a new install skips the direct registration when Toolport already serves Plumivox (below). If you added Toolport later, remove the direct registrations with `codex mcp remove plumivox` and `claude mcp remove plumivox --scope user`; Setup never removes them.
- **Plugin version:** add this to Codex's `config.toml`, keeping the plugin enabled for its setup skill, and restart Codex:

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Then search for tools in Toolport and pass the returned name to `toolport_call_tool`. Credentials stay in Plumivox's settings, not in Toolport arguments.

## Skipping the direct registration (Setup version, Windows)

Per client (Claude Code `~/.claude.json`, Codex `~/.codex/config.toml`), Setup skips the direct registration only when the Toolport gateway is registered and exists, and the Toolport registry has an enabled server in that gateway's profiles whose `command` file name (case-insensitive, with any `.exe` removed) is `plumivox` and which launches with `--stdio`. This check runs on Windows, macOS and Linux with the same rule; if it cannot be determined, or on any other OS, it registers directly. If the gateway or server executable later disappears, Setup registers directly at the next start or sign-in; it does not re-add clients you removed or servers you disabled in Toolport. Results are logged in `app-update.log`.

Set `mcp_registration` in `app-update\app-update.json` in the data folder:

| Value | Behavior |
|---|---|
| `auto` (default) | Skip when Toolport serves Plumivox. |
| `direct` | Always register directly. |
| `off` | Never add registrations automatically; uninstall still removes them. |

Update the plugin version with `codex plugin marketplace upgrade plumivox-local` ([commands](commands.md)), then disable and re-enable `plumivox` in Toolport to load the new executable.
