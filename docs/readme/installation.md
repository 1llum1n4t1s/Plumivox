# Installation

## Windows: Install with Setup (recommended)

1. Download and run [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). It installs per user and does not require administrator rights. Setup is code-signed by the author (`Open Source Developer Yuichiro Shinozaki`).
2. It installs to `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`, and if the `claude` and `codex` CLIs are on PATH, it automatically registers the MCP server `plumivox` (argument `--stdio`) with Claude Code (per user) and Codex. If the CLIs were not found, install them later; registration happens at your next logon or when you launch Plumivox from the Start menu.
3. Follow the on-screen guidance shown right after installation and run `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` in a terminal to complete the [Initial setup](initial-setup.md). Claude Code and Codex can use it from new sessions.

Updates are automatic. While the MCP server is running, it downloads new versions from R2 (`https://plumivox-updates.kagayoi.com/velopack/`) and applies them at computer logon, only when Plumivox (MCP and shared relay) is not running. If it is running, the update is deferred to the next logon; nothing is force-terminated. The signer is verified before applying, and the update is not applied if it does not match. To disable automatic updates, set `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` to `{"auto_update": false}`. Activity is logged to `app-update.log` in the same folder.

To uninstall, select Plumivox from the list under Settings → Apps. This removes only the Claude Code and Codex `plumivox` registrations that point to this installation and the logon update registration; the settings and session data folder (`%LOCALAPPDATA%\Kagayoi\Plumivox`) is kept. To remove only the registrations beforehand, check the plan with `plumivox.exe --uninstall-setup` and run it with `--uninstall-setup --apply`.

Migrating from the plugin (below): Setup replaces a same-named `plumivox` registration that points to the old plugin's `dist` or similar with one that points to the install location. The MCP server provided by the plugin itself (`plumivox@plumivox-local`) is a separate registration, so to avoid starting Plumivox through two routes, turn it off or remove the plugin. In Codex, add the same `enabled = false` as in [Using via Toolport](#using-via-toolport) (or remove the plugin); in Claude Code, disable or uninstall the plugin from `/plugin`. Codex prefers a same-named registration over the plugin's MCP, so it should start only one while the Setup registration exists, but Claude Code may start both. Setup never edits the Codex or Claude Code settings; if the plugin is still enabled it only warns (in the guide shown when you launch Plumivox, in `app-update.log`, and as `legacyPlugins` in `app-update\setup-registration.json`). Toolport registrations are not rewritten automatically either; if you use Toolport, review it yourself as described in [Using via Toolport](#using-via-toolport).

## Install as a plugin (macOS / legacy method)

Use this method on macOS (Apple Silicon) until Setup distribution starts. It also continues to work on Windows.

Install from the latest commit on `main` of the GitHub repository. It does not reference your development clone and is not pinned to a tag or commit SHA.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` is the marketplace identifier. The source is the GitHub URL above. Because it bundles `dist/plumivox.exe` for Windows x64 and `dist/plumivox` (ARM64) for Apple Silicon Macs, you do not need to install dependencies or add MCP settings manually.

Development CI stores artifacts verified on Windows and macOS in Cloudflare R2 and hands them over during release preparation. The [artifact storage and retrieval procedure](../distribution.md) is for developers. Plugin users do not need R2 settings or credentials, and installation and updates continue to come from GitHub `main`.

To update, fetch and install the latest state of `main`, then restart Codex to load it.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Check the version number shown under `installed` in the list. Do not run `add` again for an installed plugin after updating; restart Codex to load the new MCP server and skills.

For how to register and update from GitHub, see the [Codex plugin documentation](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

## Using via Toolport

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

Update the plugin version with `codex plugin marketplace upgrade plumivox-local` as above. Because a server Toolport is running uses already-loaded code, disable and re-enable `plumivox` in Toolport after updating to load the new executable. If the fetched root changes, update the registered command as well. No Git fetch or dependency installation happens at startup.
