# Toolport configuration

You can register only the MCP server with Toolport. Plumivox does not modify Toolport registrations automatically, so add or change the following stdio server to match your installation method. Do not specify a development clone or a versioned install cache.

- Name: `plumivox`
- Command: For the Windows Setup version, `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (expand `%LOCALAPPDATA%` to the actual path; the `current` path does not change even when updates replace its contents). For the plugin version, check the fetched root of `plumivox-local` with `codex plugin marketplace list`, and use `<fetched root>/dist/plumivox.exe` on Windows or `<fetched root>/dist/plumivox` on macOS
- Arguments: `--stdio`
- Working directory: your home directory
- Request timeout: 300000 milliseconds (including time for login and relay preparation)

Enable it in the Toolport profile that Codex uses, and confirm the 11 tools with `toolport_search_tools` using `server="plumivox"`. Search for and call `list_accounts`, and disable the direct connection once the settings load. For the Setup version, remove the direct registrations with `codex mcp remove plumivox` and `claude mcp remove plumivox --scope user` (once registration has completed, it is not re-registered automatically until you reinstall with Setup). For the plugin version, add the following to Codex's user settings `config.toml`, leaving the plugin itself enabled so the setup skill remains available.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Restart Codex to disable the direct connection. From then on, search for tools in Toolport and pass the returned actual tool name to `toolport_call_tool`. The API key, cookies, and SSH keys are read from Plumivox's existing settings and are not moved into Toolport arguments.

Update the plugin version with `codex plugin marketplace upgrade plumivox-local` as described in [Plugin installation and update commands](commands.md). Because a server Toolport is running uses already-loaded code, disable and re-enable `plumivox` in Toolport after updating to load the new executable. If the fetched root changes, update the registered command as well. No Git fetch or dependency installation happens at startup.
