# Updates and uninstall (Windows Setup)

How a Setup installation from [Installation](installation.md) is updated and removed.

## Automatic updates

Updates are automatic. While the MCP server is running, it downloads new versions from R2 (`https://plumivox-updates.kagayoi.com/velopack/`) and applies them at computer logon, only when Plumivox (MCP and shared relay) is not running. If it is running, the update is deferred to the next logon; nothing is force-terminated. The signer is verified before applying, and the update is not applied if it does not match. To disable automatic updates, set `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` to `{"auto_update": false}`. Activity is logged to `app-update.log` in the same folder.

## Uninstall

To uninstall, select Plumivox from the list under Settings → Apps. This removes only the Claude Code and Codex `plumivox` registrations that point to this installation and the logon update registration; the settings and session data folder (`%LOCALAPPDATA%\Kagayoi\Plumivox`) is kept. The `plumivox` server in Toolport remains, so remove it in Toolport yourself. To remove only the registrations beforehand, check the plan with `plumivox.exe --uninstall-setup` and run it with `--uninstall-setup --apply`.
