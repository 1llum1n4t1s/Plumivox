# Installation

## Windows: Install with Setup (recommended)

1. Download and run [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). It installs per user and does not require administrator rights. Setup is code-signed by the author (`Open Source Developer Yuichiro Shinozaki`).
2. It installs to `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`, and if the `claude` and `codex` CLIs are on PATH, it automatically registers the MCP server `plumivox` (argument `--stdio`) with Claude Code (per user) and Codex. If the CLIs were not found, install them later; registration happens at your next logon or when you launch Plumivox from the Start menu.
3. Follow the on-screen guidance shown right after installation and run `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` in a terminal to complete the [Initial setup](initial-setup.md). Claude Code and Codex can use it from new sessions.

Updates are automatic. While the MCP server is running, it downloads new versions from R2 (`https://plumivox-updates.kagayoi.com/velopack/`) and applies them at computer logon, only when Plumivox (MCP and shared relay) is not running. If it is running, the update is deferred to the next logon; nothing is force-terminated. The signer is verified before applying, and the update is not applied if it does not match. To disable automatic updates, set `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` to `{"auto_update": false}`. Activity is logged to `app-update.log` in the same folder.

To uninstall, select Plumivox from the list under Settings → Apps. This removes only the Claude Code and Codex `plumivox` registrations that point to this installation and the logon update registration; the settings and session data folder (`%LOCALAPPDATA%\Kagayoi\Plumivox`) is kept. To remove only the registrations beforehand, check the plan with `plumivox.exe --uninstall-setup` and run it with `--uninstall-setup --apply`.

Migrating from the plugin: Setup does not disable the old plugin or change Toolport registrations (on a new install it skips the direct registration when Toolport already serves Plumivox; see [Toolport configuration](configuration.md)). To avoid starting Plumivox through two routes, follow [Migrating from the plugin to Setup](workflow.md).

## Install as a plugin (macOS / legacy method)

Use this method on macOS (Apple Silicon) until Setup distribution starts; it also continues to work on Windows. See [Plugin installation and update commands](commands.md).

## Using via Toolport

You can register only the MCP server with Toolport and disable the direct connection. See [Toolport configuration](configuration.md).
