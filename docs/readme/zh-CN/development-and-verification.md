# 开发与验证

开发、构建和测试同样无需 Node.js。使用 Rust/Cargo 的 `cargo xtask build`、`cargo xtask check`、`cargo xtask test-e2e`。指定了旧 Node 启动器的注册，请改为上述原生启动方式。

Windows 的 `cargo xtask build` 会在使用 Sigilune（Certum SimplySign 的云签名）签名并验证签名后更新 `dist/plumivox.exe`。分发 CI 也会先用 Sigilune 为 Windows 产物签名再保存。签名所需的开发终端设置和 CI 的 Secrets 请参阅[分发产物](../../distribution.md#windows配布物のコード署名)。Windows 的 Setup 用 `cargo xtask velopack-pack` 打包，并用 `cargo xtask test-setup-velopack-e2e` 在不触及真实安装的情况下进行隔离验证。分发仅由推送 `main` 提交上的轻量标签 `v<版本>` 时启动的 `distribution.yml` 中的 Setup 分发 job 进行（[Setup（Velopack）的分发](../../distribution.md#setupvelopackの配信)）。

请参阅[面向开发者的构建与验证步骤](../../verification.md)、[工作规范](../../../AGENTS.md)和[系统设计](../../../DESIGN.md)。
