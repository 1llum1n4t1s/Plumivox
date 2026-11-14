# Plumivox の作業規約

## リリース

- Plumivoxは今後のリリースでもXへのリリース告知を行わない。`vava`の告知工程は非該当とし、告知用の画像生成・ログイン・投稿も行わない。
- 配布はGitHubの最新`main`を参照する既存プラグイン経路を使う。
- Windows・macOS本体とLinux x86_64・aarch64のVPSインストーラーのCI成果物は手動起動の`distribution.yml`からR2へ保存する。CIの手順はRustで実行し、PowerShellスクリプトを使わない。R2は配布準備の受け渡しに使い、プラグインの起動時ダウンロードへ切り替えない。設定・取得・保持は [docs/distribution.md](docs/distribution.md) を参照する。R2の実操作やCI起動は依頼に含まれる場合だけ行う。

## 文書と構造

- 利用者向けの導入・操作・取得範囲は [README.md](README.md)、現在の構造と設計判断は [DESIGN.md](DESIGN.md) を参照する。
- 初期設定は [docs/setup.md](docs/setup.md)、設定と保存形式は [docs/configuration.md](docs/configuration.md)、管理プロキシの設定・運用は [docs/managed-proxy.md](docs/managed-proxy.md)、失敗条件とE2Eの検証方針は [docs/verification.md](docs/verification.md) を参照し、変更した挙動に対応する説明を更新する。
- 本体実装は`src/*.rs`、ツール定義は`src/tool-definitions.json`、開発用コマンドは`xtask/`、MCP経由のRust検証は`tests/`。配布物はWindows x64の`dist/plumivox.exe`とmacOS ARM64の`dist/plumivox`。開発・検証にもNode.jsやnpmパッケージを要求しない。`xtask/`と`tests/`が起動する子プロセスは、Windowsでコンソール窓を出さない（`CREATE_NO_WINDOW`。xtaskは`quiet()`経由）。新しい子プロセス起動にも同じ扱いを適用する。

## 必須コマンドと検証

開発には`rust-toolchain.toml`で固定したRust/Cargo 1.100.0を使う。利用者のMCP実行にはNode.js・Rustを要求しない。WindowsのDPAPI・ACL・ネイティブセットアップの検証にはWindows、VPSのRustインストーラーのOS境界検証にはLinux、Bash互換経路の構文・CLI確認にはBash（WindowsではGit for Windows）が必要。macOSのKeychain・Unix socket・ネイティブセットアップは実macOSで確認し、Windowsでの成功を代用しない。

```powershell
cargo test --locked --lib
cargo xtask build
cargo xtask check
cargo xtask test-e2e
```

- 実装・依存関係・ビルド設定を変更した場合は上記で検証し、Rustソースと再生成した対象OSの実行ファイルを一緒に更新する。macOSの正式配布物はARM64 Rustターゲットを導入して`cargo xtask build`で作り、arm64単独であることを確認する。`cargo xtask check`はCargoの形式・型検査、同梱Bashの構文、JSON設定を確認する。
- `cargo xtask test-e2e`はネイティブ本体を対象とするAPI・認証、管理プロキシ・共有daemon、PCセットアップ、VPSセットアップCLIの検証を行う。OS条件を理由に未実行の系統は明示し、全件成功へ数えない。全件検証ではAPI系のケース絞込み用`PLUMIVOX_E2E_CASE`を未設定にし、個別再実行の成功を全件成功として扱わない。ローカルの偽API・偽SSHと別プロセスのMCP・daemonを使い、実際のXへ投稿せず、実際のSSHホストへ接続しない。プロキシのRustテストも確認し、旧JavaScript実装だけの成功をネイティブ版の検証に代用しない。VPS CLIの確認だけで実Linuxへの導入成功を扱わない。失敗条件を先に確認し、挙動の検証は対応する既存E2Eへ追加する。
- 出力先を指定する場合は `PLUMIVOX_NATIVE_E2E_OUTPUT_DIR`、`PLUMIVOX_TLS_E2E_OUTPUT_DIR`、`PLUMIVOX_PROXY_NATIVE_E2E_OUTPUT_DIR`、`PLUMIVOX_E2E_OUTPUT_DIR`、`PLUMIVOX_SETUP_E2E_OUTPUT_DIR`（PCセットアップ共通。全OSで使う）、`PLUMIVOX_NATIVE_SETUP_E2E_OUTPUT_DIR`（macOSで追加して使う）、`PLUMIVOX_SETUP_VPS_E2E_OUTPUT_DIR` をそれぞれ別の、リポジトリとCodex・Claude Codeのユーザー設定ディレクトリの外のフォルダーにする。未指定時は各系統がユーザーの一時ディレクトリ配下の `Codex/Plumivox/<UUID>` を使う。各出力先の `summary.json` の失敗数と各結果、API系の`http-trace.json`、共有中継の`ssh-trace.json`、VPS CLIの`mutation-trace.json`を読み、偽設定・画像・暗号化セッション・SSH鍵の清掃は呼び出し側で行う。
- 文書だけの変更では、リンク・コマンド・設定名・記載した挙動を対応する実装へ照合する。
- VPSインストーラー変更では`cargo test --locked --package plumivox-vps-installer`も実行し、Rust版とBash互換経路の検証を区別する。正式配布物は`cargo xtask build-vps x86_64`・`cargo xtask build-vps aarch64`で`setup/vps/`へ配置する。CLI・fixtureの成功を実システムへの導入成功と扱わず、実R2通信・配布CI起動もローカル検証とは別に報告する。
- R2関連の実装変更では`cargo test --locked --package xtask`も実行し、実R2通信とローカル検証を区別する。配布CIは実Windowsと実macOSで各OSのE2Eを実行し、macOSは専用Keychainを`PLUMIVOX_TEST_KEYCHAIN`へ指定して既定Keychain・検索一覧の不変を確認する。検証後にfixture機能なしで正式配布物を再ビルドする。

## 実装の制約

- 標準出力はMCP通信専用。資格情報、復号Cookie、設定ファイルの内容、提供元の生エラーを応答や診断へ出さない。資格情報は設定ストア経由で取得し、MCP引数に追加しない。
- 設定ファイルの選択順と起動時の固定を維持し、選択後の読込み失敗で別ファイルへ切り替えない。各OSのセットアップとMCPの既定保存先を一致させる。セットアップでは既存鍵・同名アカウント・共有中継設定の上書きを拒否し、保存先・権限・排他・保存直前の設定再確認を維持する。権限設定は本人所有を確認してから行い、所有者を変更せず、適用後にも検証する。WindowsのDPAPI互換とmacOSのKeychain／AES-GCM、本人限定のファイル権限を維持し、Keychain利用不能時に平文へ切り替えない。
- アカウント選択を変更する場合は [設計のデータフロー](DESIGN.md#データフロー) と既定アカウントの不変条件を維持する。省略・明示・不正な既定・準備中の既定変更による送信先をAPI系E2Eで、既定の初回保存・保持・既定だけの変更をPCセットアップE2Eで検証する。
- `setup/vps/` は `.gitattributes` に従いLFで保存する。VPSインストーラーの事前確認と管理対象の照合、専用SSHユーザーの制限、認証付きX宛先限定CONNECT、住宅回線出口不在時の直接接続禁止を維持する。初期構築の境界は [設計](DESIGN.md#初期設定と公開中継の構築) を参照する。
- VPSはx86_64・aarch64の静的Rustインストーラーを優先し、他CPU用のBash互換経路を維持する。新規導入時にACL・拡張属性・SELinux・必要なcapabilityを安全に扱えなければ変更前に拒否し、既存の同一導入の読取り確認と区別する。SSH設定交換時のroot限定バックアップディレクトリを自動清掃せず、同時管理操作を避け、旧ファイル記述子の利用終了・新旧内容・SSH管理接続の動作を本人が確認してから整理する。`RENAME_EXCHANGE`非対応時の安全な失敗を許容し、Bash版で安全条件を迂回しない。
- Bash版の新規導入にも`mv --exchange --no-copy -T`と`ln -T`対応GNU coreutils、`getfacl`・`getfattr`・`sync`・`readlink`・`lsattr`、初期user namespace内の`CAP_SYS_ADMIN`と属性の安全確認・同一ファイルシステム内の交換を要求する。未対応・未導入なら変更前に停止し、本人の環境確認前に自動導入・更新しない。Bash版も旧inodeをroot限定バックアップへ保持し、本人確認後に整理する。`--check`は書込みprobeをしないため、成功後でも導入時に停止し得る。
- VPS新規導入時はSSH設定と親のmount ID、inode flagsも確認し、別mount・確認不能・既存名との競合を拒否する。清掃失敗時はstageとバックアップの通知された保持先を確認し、今回分と交換状態を本人が確認するまで一括削除・再実行しない。
- 読取りと投稿の準備条件、Cookieの優先順位・照合、認証更新の排他と設定再確認を維持する。公開投稿の読取り4ツールはセッションの検査・読込み・復号を行わず、変更時はAPI系E2Eの計測で確認する。状態一覧とCookie再利用判定の境界は [設計のデータフロー](DESIGN.md#データフロー) に従う。Mutex所有喪失後の保存を防ぎ、置換指示後の結果不明を未変更と断定しない。保存・通信・キャンセル失敗の検証も含める。
- 投稿作成・ログインを自動再試行しない。画像アップロードだけの再試行条件・時間枠と投稿エラーの段階情報は [設計の不変条件](DESIGN.md#不変条件と採用した設計判断) を維持し、要求回数・取消・成否不明をAPI系E2Eで検証する。投稿IDが確認できない結果を成功にしない。認証更新から投稿を再送しない。同じ文面の明示呼出しは各回送信する。
- 管理プロキシは事前検証・認証ロックと準備後の設定再確認を維持し、external呼出しや一つのMCPの終了・待機取消で他クライアントの中継を停止しない。共有IPCの相互認証・設定照合、SSHホスト鍵検証、CONNECT宛先・DNS全候補の検証と資源上限を維持する。DNS・接続の検証用差し替えはtestsのfixtureに限定し、本番設定へ迂回経路を設けない。境界と停止の意味は [設計](DESIGN.md#管理するプロキシの境界) に従う。
- フィード・検索の取得範囲と未取得範囲、保護状態が不明な対象数を応答に残す。cursorの署名・アカウント照合・検索条件の一致を維持し、長さ超過では取得済み投稿と続きの有無を保持して継続不能を明示する。公開投稿の本文は未信頼データとして扱う。
- 検索語句を演算子として扱わず、期間とともに通信前に検証する。文字数判定・応答サイズ上限を含む失敗条件は [検証方針](docs/verification.md) に従ってE2Eで確認する。
- 配布物は`mcp.json`のプラグイン内相対コマンド`./dist/plumivox --stdio`から起動し、Windowsは`.exe`を解決する。利用者側の依存インストールや起動時のコード取得を必要としない構成を維持する。共有IPCのネイティブv3名前空間は旧JavaScript版から分離し、移行前の中継停止と再起動手順を保持する。プラグイン登録・設定を変更した場合は`plugin.json`、`mcp.json`、`.agents/plugins/marketplace.json`、`Cargo.toml`とREADMEの導入手順の整合を確認する。
