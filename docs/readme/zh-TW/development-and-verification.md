# 開發與驗證

開發、建置與測試也不需要 Node.js。使用 Rust/Cargo 的 `cargo xtask build`、`cargo xtask check`、`cargo xtask test-e2e`。指定舊 Node 啟動器的註冊，請變更為上述的原生啟動方式。

Windows 的 `cargo xtask build` 會在以 Sigilune（Certum SimplySign 的雲端簽章）完成簽署與簽章驗證後，更新 `dist/plumivox.exe`。發布 CI 也會先以 Sigilune 簽署 Windows 成品再儲存。簽署所需的開發機設定與 CI 的 Secrets 請參閱[發布成品](../../distribution.md#windows配布物のコード署名)。Windows 的 Setup 以 `cargo xtask velopack-pack` 封裝，並以 `cargo xtask test-setup-velopack-e2e` 在不接觸實際安裝的情況下進行隔離驗證。發布只由推送 `main` 提交上的輕量標籤 `v<版本>` 時啟動的 `distribution.yml` 中的 Setup 發布 job 進行（[Setup（Velopack）的發布](../../distribution.md#setupvelopackの配信)）。

請參閱[開發者用的建置與驗證步驟](../../verification.md)、[作業規範](../../../AGENTS.md)與[系統設計](../../../DESIGN.md)。
