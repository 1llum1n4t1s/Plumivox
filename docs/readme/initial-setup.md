# Initial setup

If this is your first time, proceed to the [setup guide](../setup.md). It bundles native setup wizards for Windows and macOS and a static Rust VPS installer for Linux x86_64 and aarch64. A Bash-compatible path remains for other CPUs. You can also ask Codex to "set up Plumivox" and the bundled `setup-plumivox` skill will guide you.

The settings file is kept outside the repository. New users use `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` on Windows and `~/Library/Application Support/Kagayoi/Plumivox/settings.json` on macOS. Users who already have `~/dev/Secret/secrets.json` read `twitterapi_io` from that file. vava/blbl's `release_posting` and `blog_posting` are also kept. To use a different location, set `PLUMIVOX_SECRETS_FILE`. You never need to pass secret values in the conversation or as MCP arguments.

If the selected settings file cannot be read, it fails with an error instead of switching to another settings file. If you change the storage location, restart Codex. The initial setup does not overwrite existing keys, same-named accounts, or shared relay settings. For how to change existing settings, see the [setup guide](../setup.md).

[First-time setup](../setup.md) · [Configuring accounts and cookies](../configuration.md) · [Managing the proxy with Plumivox](../managed-proxy.md)
