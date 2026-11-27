# Installation

## Windows: Install with Setup (recommended)

1. Download and run [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). It installs per user and does not require administrator rights. Setup is code-signed by the author (`Open Source Developer Yuichiro Shinozaki`).
2. It installs to `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`, and if the `claude` and `codex` CLIs are on PATH, it automatically registers the MCP server `plumivox` (argument `--stdio`) with Claude Code (per user) and Codex. If the CLIs were not found, install them later; registration happens at your next logon or when you launch Plumivox from the Start menu.
3. Follow the on-screen guidance shown right after installation and run `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` in a terminal to complete the [Initial setup](initial-setup.md). Claude Code and Codex can use it from new sessions.

Updates are applied automatically at logon when Plumivox is not running, and uninstalling keeps your settings folder. See [Updates and uninstall](updates-and-uninstall.md).

Other command-line options of `plumivox.exe`: `--setup` (initial setup), `--uninstall-setup` (remove registrations), and `--rejection-log on|off|status|show|clear` (time-limited local log of redacted rejection text, off by default; see [Local rejection log](when-posting-or-authentication-fails.md#local-rejection-log)). The log is kept after uninstalling and is removed by `--rejection-log clear`.

Migrating from the plugin: Setup does not disable the old plugin or change Toolport registrations (it skips the direct registration when Toolport already serves Plumivox, and warns when Toolport serves a different executable; if you use Toolport, change its `plumivox` command to `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` and do not copy the executable elsewhere; see [Toolport configuration](configuration.md)). To avoid starting Plumivox through two routes, follow [Migrating from the plugin to Setup](workflow.md).

## Install as a plugin (macOS / legacy method)

Use this method on macOS (Apple Silicon) until Setup distribution starts; it also continues to work on Windows. See [Plugin installation and update commands](commands.md).

## Using via Toolport

You can register only the MCP server with Toolport and disable the direct connection. See [Toolport configuration](configuration.md).
