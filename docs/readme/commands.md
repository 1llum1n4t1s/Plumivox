# Plugin installation and update commands

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
