# 検証の構成と設計判断

[DESIGN.md](../../DESIGN.md) の検証に関する詳細。失敗条件とE2Eの方針は [docs/verification.md](../verification.md)、コマンドは [AGENTS.md](../../AGENTS.md) を参照する。

## テスト部品

| テスト | 検証対象 |
| --- | --- |
| `tests/api_e2e.rs` / `tests/native_e2e.rs` / `tests/tls_e2e.rs` / `tests/support/` / `tests/native-fixture.rs` | Rustの偽HTTP・HTTPS APIと別プロセスのMCPを使い、通信内容・回数・失敗状態を検証する。失敗注入は`test-fixture`ビルドに限定する |
| `tests/proxy_e2e.rs`と`src/proxy.rs` / `src/proxy_egress.rs` / `src/proxy_ipc.rs`内のテスト | Rustの偽SSH・echo、認証付きIPCとCONNECT出口の境界を検証する |
| `tests/setup_e2e.rs` / `tests/setup_vps_e2e.rs` | Windows・macOSの設定ウィザード、個別保存先、鍵の保護、VPS CLIの検証と拒否条件。実VPSへの導入は別途確認が必要 |
| `tests/setup_velopack_e2e.rs` / `tests/cli-stub.rs` | Setup（Velopack）の隔離E2E（[Setup](setup-velopack.md)） |

## 設計判断

- **配布と検証**：ネイティブ実行ファイルを同梱し、利用者側のNode.js・Rust・`node_modules`を不要にする。Rustソース変更時は対象OSの配布物を再ビルドする。E2EとRustテストは実際のXへの副作用を避けつつMCP・HTTP・OS別の保存とプロキシ境界を検証するが、実サービス上のCookie有効性や投稿成功を証明するものではない。
