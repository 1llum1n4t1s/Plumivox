# 配布CIと依存更新

[DESIGN.md](../../DESIGN.md) の配布・依存更新に関する詳細。操作・保持方針は [docs/distribution.md](../distribution.md) を参照する。

## 配布CI

`.github/workflows/distribution.yml`・`xtask/src/ci.rs`・`xtask/src/r2.rs`・`xtask/src/velopack.rs`が担う。`v<版>`タグのpushで起動し、`guard`・`unix`・`windows`・`velopack`のjobで、RustによるWindows・macOS本体とLinux x86_64・aarch64のVPSインストーラーの検証とビルド、非公開R2への成果物保存・検証付き取得、`windows`の成功後に`velopack`が行うSetupの梱包・署名・署名者検証・公開R2（`plumivox-updates`の`velopack/`）への配信、隔離E2E（`tests/setup_velopack_e2e.rs`・`tests/cli-stub.rs`）を行う。処理は`xtask/src/ci.rs`のRustコマンドへ集約し、PowerShellスクリプトは使わない。利用者のMCP実行から独立する。

開発用の成果物は、`v<版>`タグのpush（同じタグでの手動再実行を含む）で起動する配布CIが各OSの実行環境で検証してから非公開R2へ保存する。`main`の手動起動はmacOSとLinuxの成果物だけを作る。`plumivox/ci/{commit}/{run}/{attempt}/{target}/`に実行ファイル・第三者ライセンスを保存し、サイズとSHA256を含むmanifestを最後に保存する。既存オブジェクトの上書きを拒否し、取得ではidentity・ファイル名・サイズ・SHA256を照合する。この非公開R2は配布準備の受け渡しであり、GitHub mainの同梱`dist`を使うプラグインの導入・更新経路は、Setupの配信を確かめて縮小するまで維持する。Setupの配信は別の公開バケットで行う（[Setup（Velopack）](setup-velopack.md)）。CIは`dist`を自動コミットせず、latestの差替えや過去成果物の自動削除も行わない。

VPSインストーラーはLinux x86_64・aarch64で静的ビルドして対象CPUで起動を確認し、既存R2経路で受け渡し、GitHub mainの同梱資材から導入する。ローカルのCLI・fixture検証と、実システムへの導入・実R2通信・CI起動の結果は区別する。

## 依存更新の自動化

`.github/dependabot.yml` はCargo依存とGitHub Actionsを週次で更新し、minor／patchをまとめ、majorは個別PRにする。`dependency-validation.yml` はDependabot PRまたは手動起動でWindows上で`cargo xtask`の`build-unsigned`・`check`・`test-e2e`（ビルド・構文確認・全E2E）を別々のjobで並列に実行する（同じrefの古い実行は新しい実行で打ち切る）。Cargoの更新PRはWindows・macOS双方の正式配布物の再生成が必要なため自動マージせず、配布物を含めて手動で統合する。
