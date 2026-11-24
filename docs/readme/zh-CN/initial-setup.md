# 初始设置

首次使用时，请前往[设置步骤](../../setup.md)。随附 Windows、macOS 的原生设置向导，以及面向 Linux x86_64、aarch64 的静态 Rust VPS 安装程序。其他 CPU 保留了 Bash 兼容路径。也可以对 Codex 说“帮我设置 Plumivox”，由随附的 `setup-plumivox` 技能进行引导。

设置文件放在仓库之外。新用户在 Windows 上使用 `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`，在 macOS 上使用 `~/Library/Application Support/Kagayoi/Plumivox/settings.json`。已有 `~/dev/Secret/secrets.json` 的用户，会读取该文件中的 `twitterapi_io`。vava／blbl 的 `release_posting` 和 `blog_posting` 也会保留。如需使用其他位置，请指定 `PLUMIVOX_SECRETS_FILE`。无需把机密值传入对话或 MCP 参数。

如果无法读取所选的设置文件，不会切换到其他设置，而是报错。更改保存位置后请重启 Codex。首次设置不会覆盖现有的密钥、同名账号和共享中继设置。更改现有设置的方法请参阅[设置步骤](../../setup.md)。

[首次设置](../../setup.md) · [账号与 Cookie 的设置](../../configuration.md) · [用 Plumivox 管理代理](../../managed-proxy.md)
