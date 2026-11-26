# Toolport configuration

You can register only the MCP server with Toolport. Plumivox does not modify Toolport registrations automatically, so add or change the following stdio server to match your installation method. Do not specify a development clone or a versioned install cache.

- Name: `plumivox`
- Command: For the Windows Setup version, `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (expand `%LOCALAPPDATA%` to the actual path; the `current` path does not change even when updates replace its contents). For the plugin version, check the fetched root of `plumivox-local` with `codex plugin marketplace list`, and use `<fetched root>/dist/plumivox.exe` on Windows or `<fetched root>/dist/plumivox` on macOS
- Arguments: `--stdio`
- Working directory: your home directory
- Request timeout: 300000 milliseconds (including time for login and relay preparation)

Enable it in the Toolport profile that Codex uses, and confirm the 11 tools with `toolport_search_tools` using `server="plumivox"`. Search for and call `list_accounts`, and disable the direct connection once the settings load. For the Setup version, a new install skips the direct registration automatically when Toolport already serves Plumivox (see "Skipping the direct registration" below). If you add Toolport after Setup has registered Plumivox directly, remove the direct registrations yourself with `codex mcp remove plumivox` and `claude mcp remove plumivox --scope user`; Setup never removes an existing direct registration (once registration has completed, it is not re-registered automatically until you reinstall with Setup; the only thing Setup restores later is a client it skipped). For the plugin version, add the following to Codex's user settings `config.toml`, leaving the plugin itself enabled so the setup skill remains available.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Restart Codex to disable the direct connection. From then on, search for tools in Toolport and pass the returned actual tool name to `toolport_call_tool`. The API key, cookies, and SSH keys are read from Plumivox's existing settings and are not moved into Toolport arguments.

## Skipping the direct registration (Setup version, Windows)

When Setup registers Plumivox, it checks per client (Claude Code: `mcpServers` in `~/.claude.json`, or under `CLAUDE_CONFIG_DIR`; Codex: `~/.codex/config.toml`, or under `CODEX_HOME`) whether Toolport serves it, and skips the direct registration only if all of the following hold:

- The client has the Toolport gateway registered, and the gateway executable exists.
- The Toolport registry (`%APPDATA%\Toolport\registry.json`, or the path given by `TOOLPORT_REGISTRY` / `TOOLPORT_DATA_DIR`) has an enabled stdio server that launches `plumivox.exe` with `--stdio`, and it is enabled in the profiles that gateway uses. The server's executable may be a different copy from Setup's.

If this cannot be determined (unreadable files, an unsupported registry), Setup registers directly as before. On a non-Windows OS it does not check at all and always registers directly. If the Toolport server is enabled but its executable no longer exists, that counts as not served and Setup registers directly. Setup never removes an existing direct registration, and a client it registered directly is not re-registered automatically if you remove the registration later; Setup only goes back for clients it skipped, or whose earlier registration did not finish. After skipping, if the gateway disappears from the client (or its executable is gone), the next time Plumivox starts (MCP server or launch from the Start menu or a terminal) or you sign in, Setup registers directly. If you disable or delete the server in Toolport, Setup does not add one. Toolport's tool quarantine is not checked. When Setup rebuilds its state (first run, a changed executable location, or a missing state file) and Toolport serves Plumivox, it does not call the CLI, so an existing direct registration that points to the previous executable is left in place and not repointed; remove it if needed, for example with `claude mcp remove plumivox --scope user`. Each run records the result per client (served, not connected, not served, or undetermined with the reason) in `app-update.log`, without any secret values.

Control this with `mcp_registration` in `app-update.json` in the data folder's `app-update\` folder:

| Value | Behavior |
|---|---|
| `auto` (default) | Skip the direct registration when Toolport serves Plumivox. |
| `direct` | Always register directly (also registers clients that were skipped). |
| `off` | Never add or repair registrations automatically. Uninstalling still removes the direct registration. |

Any other value is treated as `auto`; this is noted once in `app-update.log` when a registration run starts.

Update the plugin version with `codex plugin marketplace upgrade plumivox-local` as described in [Plugin installation and update commands](commands.md). Because a server Toolport is running uses already-loaded code, disable and re-enable `plumivox` in Toolport after updating to load the new executable. If the fetched root changes, update the registered command as well. No Git fetch or dependency installation happens at startup.
