# リリースと配布の規約

Plumivox のリリース・Windows 配布（Setup）・R2 への成果物保存に関する規約。

- Plumivoxは今後のリリースでもXへのリリース告知を行わない。`vava`の告知工程は非該当とし、告知用の画像生成・ログイン・投稿も行わない。
- Windowsの配布はSetup（Velopack）を既定にする（本人が2026-11-15に決定）。ユーザー単位・管理者不要・署名付きの`PlumivoxApp-win-Setup.exe`で導入し、更新は公開R2（`https://plumivox-updates.kagayoi.com/velopack/`）から取得して既定でログオン時に適用する。設計と境界は [Setup（Velopack）](setup-velopack.md) を参照する。GitHubの最新`main`を参照する既存プラグイン経路（`mcp.json`と同梱`dist/`）は、R2配信を確かめてから縮小するまで併存させ、今回の変更で削除しない。macOSは`osx`チャネルを配信するまでプラグイン経路を使う。
- Windows・macOS本体とLinux x86_64・aarch64のVPSインストーラーのCI成果物は`distribution.yml`から非公開R2（`plumivox-artifacts`）へ保存する。`distribution.yml`は`main`のコミットに付けた軽量タグ`v<版>`の push（同じタグでの手動再実行を含む）で起動し、Setupの配信は同じワークフローの`velopack` jobが`windows` jobの成功後に公開R2（`plumivox-updates`）の`velopack/`へ書く。`main`の手動起動はmacOSとLinuxの非公開成果物だけを作る。`v<版>`タグの push は公開操作なので、`vava`（版の更新・公開の依頼）でだけ行う。CIの手順はRustで実行し、PowerShellスクリプトを使わない。プラグインの起動時ダウンロードへは切り替えない。設定・取得・保持は [distribution.md](../distribution.md) を参照する。R2の実操作・Setupの公開・CI起動は依頼に含まれる場合だけ行う。
- 配布CIは実Windowsと実macOSで各OSのE2Eを実行し、macOSは専用Keychainを`PLUMIVOX_TEST_KEYCHAIN`へ指定して既定Keychain・検索一覧の不変を確認する。検証後にfixture機能なしで正式配布物を再ビルドする。
- 正式配布物のVPSインストーラーは`cargo xtask build-vps x86_64`・`cargo xtask build-vps aarch64`で`setup/vps/`へ配置する。実R2通信・配布CI起動はローカル検証とは別に報告する。
