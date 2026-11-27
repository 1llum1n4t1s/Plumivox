# Toolport configuration

You can register only the MCP server with Toolport. Plumivox does not modify Toolport registrations, so add the following stdio server to match your installation method. Do not use a development clone or a versioned install cache.

- Name: `plumivox`
- Command: Setup version: `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (expanded; `current` stays the same across updates). Plugin version: `<fetched root>/dist/plumivox.exe` (Windows) or `<fetched root>/dist/plumivox` (macOS), where the root is shown by `codex plugin marketplace list` for `plumivox-local`
- Arguments: `--stdio`
- Working directory: your home directory
- Request timeout: 300000 milliseconds (includes login and relay preparation)

Enable it in the Toolport profile Codex uses, confirm the 11 tools with `toolport_search_tools` using `server="plumivox"`, call `list_accounts`, then disable the direct connection.

- **Setup version:** If you added Toolport later, remove the direct registrations with `codex mcp remove plumivox` and `claude mcp remove plumivox --scope user`; Setup never removes them. When moving from the plugin to Setup, change the Toolport command to `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (expanded); do not copy the executable to another location, because a copy is never updated.
- **Plugin version:** add this to Codex's `config.toml`, keeping the plugin enabled for its setup skill, and restart Codex:

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Then search for tools in Toolport and pass the returned name to `toolport_call_tool`. Credentials stay in Plumivox's settings, not in Toolport arguments.

## Skipping the direct registration (Setup version)

Setup skips direct registration when Toolport already serves Plumivox, warns if Toolport launches a different executable, and can be controlled with `mcp_registration`. See [Skipping the direct registration](toolport-registration-skip.md).

To update the plugin version, first disable `plumivox` in Toolport so the executable is not locked and can be replaced, then run `codex plugin marketplace upgrade plumivox-local` ([commands](commands.md)), and finally re-enable `plumivox` to load the new executable.
