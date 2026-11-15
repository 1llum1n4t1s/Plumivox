# Plumivox の作業規約

## リリース

- Plumivoxは今後のリリースでもXへのリリース告知を行わない。`vava`の告知工程は非該当とし、告知用の画像生成・ログイン・投稿も行わない。
- Windowsの配布はSetup（Velopack）を既定にする（本人が2026-11-15に決定）。ユーザー単位・管理者不要・署名付きの`PlumivoxApp-win-Setup.exe`で導入し、更新は公開R2（`https://plumivox-updates.kagayoi.com/velopack/`）から取得して既定でログオン時に適用する。設計と境界は [Setup（Velopack）](#setupvelopack) を参照する。GitHubの最新`main`を参照する既存プラグイン経路（`mcp.json`と同梱`dist/`）は、R2配信を確かめてから縮小するまで併存させ、今回の変更で削除しない。macOSは`osx`チャネルを配信するまでプラグイン経路を使う。
- Windows・macOS本体とLinux x86_64・aarch64のVPSインストーラーのCI成果物は`distribution.yml`から非公開R2（`plumivox-artifacts`）へ保存する。`distribution.yml`は`release/<版>`の push（同じブランチでの手動再実行を含む）で起動し、Setupの配信は同じワークフローの`velopack` jobが`windows` jobの成功後に公開R2（`plumivox-updates`）の`velopack/`へ書く。`main`の手動起動はmacOSとLinuxの非公開成果物だけを作る。`release/<版>`の push は公開操作なので、`vava`（版の更新・公開の依頼）でだけ行う。CIの手順はRustで実行し、PowerShellスクリプトを使わない。プラグインの起動時ダウンロードへは切り替えない。設定・取得・保持は [docs/distribution.md](docs/distribution.md) を参照する。R2の実操作・Setupの公開・CI起動は依頼に含まれる場合だけ行う。

## 文書と構造

- 利用者向けの導入・操作・取得範囲は [README.md](README.md)（英語の正本。日本語版は [README.ja.md](README.ja.md)）、現在の構造と設計判断は [DESIGN.md](DESIGN.md) を参照する。
- 初期設定は [docs/setup.md](docs/setup.md)、設定と保存形式は [docs/configuration.md](docs/configuration.md)、管理プロキシの設定・運用は [docs/managed-proxy.md](docs/managed-proxy.md)、失敗条件とE2Eの検証方針は [docs/verification.md](docs/verification.md) を参照し、変更した挙動に対応する説明を更新する。
- 本体実装は`src/*.rs`、ツール定義は`src/tool-definitions.json`、開発用コマンドは`xtask/`、MCP経由のRust検証は`tests/`。Setup（Velopack）の起動時処理・自動更新・登録は`src/app_update.rs`と`src/app_update/`、Claude Code・Codexへの登録は`src/connect.rs`、梱包・公開・隔離E2Eは`xtask/src/velopack.rs`。配布物はWindows x64のSetup（`PlumivoxApp-win-Setup.exe`、R2から配信）と、プラグイン経路用のWindows x64の`dist/plumivox.exe`・macOS ARM64の`dist/plumivox`。開発・検証にもNode.jsやnpmパッケージを要求しない。`xtask/`と`tests/`が起動する子プロセスは、Windowsでコンソール窓を出さない（`CREATE_NO_WINDOW`。xtaskは`quiet()`経由）。新しい子プロセス起動にも同じ扱いを適用する。テストは`AllocConsole`で見えるコンソールを作らない。専用コンソールが必要なら`CREATE_NO_WINDOW`で起動した子に持たせる（窓を持たない専用コンソールになる。`tests/setup_windows.rs`の`terminal()`が`terminal_fixture`をそう起動し、fixtureは入力の前に窓が無い（または不可視）こと・コンソールに接続しているのが自分だけであることを確かめて、満たさなければ失敗する。窓がフォーカスを奪うと本人のキー入力がテストへ流れ込み、本人の画面も塞ぐため）。専用コンソールを使う子へ渡す製品の起動には`CREATE_NO_WINDOW`を付けず、そのコンソールを継承させる。入力は窓に依存しない`WriteConsoleInputW`で与える。専用コンソールのケースだけを確かめるときは`PLUMIVOX_SETUP_CASES`（カンマ区切りのケース名）を使い、全件検証では未設定にする。

## 必須コマンドと検証

開発には`rust-toolchain.toml`で固定したRust/Cargo 1.100.0を使う。利用者のMCP実行にはNode.js・Rustを要求しない。WindowsのDPAPI・ACL・ネイティブセットアップの検証にはWindows、VPSのRustインストーラーのOS境界検証にはLinux、Bash互換経路の構文・CLI確認にはBash（WindowsではGit for Windows）が必要。macOSのKeychain・Unix socket・ネイティブセットアップは実macOSで確認し、Windowsでの成功を代用しない。

```powershell
cargo test --locked --lib
cargo xtask build
cargo xtask check
cargo xtask test-e2e
```

- 実装・依存関係・ビルド設定を変更した場合は上記で検証し、Rustソースと再生成した対象OSの実行ファイルを一緒に更新する（プラグイン経路の`dist/`が残る間）。Setupの起動時処理・登録・更新・梱包を変更した場合は、Windowsで`cargo xtask test-setup-velopack-e2e`も実行する。macOSの正式配布物はARM64 Rustターゲットを導入して`cargo xtask build`で作り、arm64単独であることを確認する。`cargo xtask check`はCargoの形式・型検査、同梱Bashの構文、JSON設定を確認する。
- `cargo xtask test-e2e`はネイティブ本体を対象とするAPI・認証、管理プロキシ・共有daemon、PCセットアップ、VPSセットアップCLIの検証を行う。OS条件を理由に未実行の系統は明示し、全件成功へ数えない。全件検証ではAPI系のケース絞込み用`PLUMIVOX_E2E_CASE`を未設定にし、個別再実行の成功を全件成功として扱わない。ローカルの偽API・偽SSHと別プロセスのMCP・daemonを使い、実際のXへ投稿せず、実際のSSHホストへ接続しない。プロキシのRustテストも確認し、旧JavaScript実装だけの成功をネイティブ版の検証に代用しない。VPS CLIの確認だけで実Linuxへの導入成功を扱わない。失敗条件を先に確認し、挙動の検証は対応する既存E2Eへ追加する。
- 出力先を指定する場合は `PLUMIVOX_NATIVE_E2E_OUTPUT_DIR`、`PLUMIVOX_TLS_E2E_OUTPUT_DIR`、`PLUMIVOX_PROXY_NATIVE_E2E_OUTPUT_DIR`、`PLUMIVOX_E2E_OUTPUT_DIR`、`PLUMIVOX_SETUP_E2E_OUTPUT_DIR`（PCセットアップ共通。全OSで使う）、`PLUMIVOX_NATIVE_SETUP_E2E_OUTPUT_DIR`（macOSで追加して使う）、`PLUMIVOX_SETUP_VPS_E2E_OUTPUT_DIR`、`PLUMIVOX_SETUP_VELOPACK_E2E_OUTPUT_DIR`（Setup E2E。梱包物・導入先・隔離した環境を置く）をそれぞれ別の、リポジトリとCodex・Claude Codeのユーザー設定ディレクトリの外のフォルダーにする。未指定時は各系統がユーザーの一時ディレクトリ配下の `Codex/Plumivox/<UUID>` を使う。各出力先の `summary.json` の失敗数と各結果、API系の`http-trace.json`、共有中継の`ssh-trace.json`、VPS CLIの`mutation-trace.json`を読み、偽設定・画像・暗号化セッション・SSH鍵の清掃は呼び出し側で行う。
- 文書だけの変更では、リンク・コマンド・設定名・記載した挙動を対応する実装へ照合する。
- ローカライズ（[設計](DESIGN.md#ローカライズ)）: 人が読む文言（CLIの使い方・ヘルプ・エラー、`--setup`の質問と表示、Setup版の案内、`--uninstall-setup`の表示）は`src/locale.rs`の`tr`・`tr_with`で`src/locales/`の9言語の辞書から引く。人が読む文言を足す・変えるときは9言語の辞書を同時に更新し、`cargo test --locked --lib`の整合テスト（キー集合・プレースホルダー・参照キー）を通す。キーは文字列リテラルで渡す。AIが読む文言（`src/tool-definitions.json`のツール説明・入力スキーマ、MCP応答のエラー文）は英語のままにし、辞書を通さない。stdio MCPの経路から`tr`を呼ばない。
- READMEは英語の`README.md`が正本。`README.md`を変えたら、`README.ja.md`・`README.zh-CN.md`・`README.zh-TW.md`・`README.ko.md`・`README.es.md`・`README.fr.md`・`README.de.md`・`README.pt-BR.md`も同じ変更で直す（先頭の9言語の切替行、見出しとコードブロックの構成を揃える）。`docs/`からREADMEの見出しへのリンクは日本語版`README.ja.md`のアンカーを使う。
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
- Setup版は導入先の`current\plumivox.exe --stdio`をClaude Code（`claude mcp add --scope user`）・Codex（`codex mcp add`）へ登録して起動する。プラグイン経路は`mcp.json`のプラグイン内相対コマンド`./dist/plumivox --stdio`から起動し、Windowsは`.exe`を解決する。どちらも利用者側の依存インストールや起動時のコード取得を必要としない構成を維持する（Setup版の更新の取得は stdio MCP の背景スレッドだけが行い、起動を待たせない）。共有IPCのネイティブv3名前空間は旧JavaScript版から分離し、移行前の中継停止と再起動手順を保持する。プラグイン登録・設定を変更した場合は`plugin.json`、`mcp.json`、`.agents/plugins/marketplace.json`、`Cargo.toml`とREADMEの導入手順の整合を確認する。

## Setup（Velopack）

packIdは`PlumivoxApp`（`Plumivox`は既存の`%LOCALAPPDATA%\Plumivox`と衝突する）、packTitleは`Plumivox`、mainExeは`plumivox.exe`、Velopack CLIと crate はともに1.2.161（`velopack = "=1.2.161"`）。フィードは`https://plumivox-updates.kagayoi.com/velopack/`の`releases.win.json`（将来`releases.osx.json`）。Plumivoxは**多プロセス型**（セッションごとの stdio MCP と、MCP が`current_exe()`から起動する共有中継 daemon）で、Velopack の適用は導入先の自分以外の全プロセスを強制終了するため、日中に適用しない。この設計（`src/app_update.rs`・`src/app_update/`）を維持する。挙動と理由は [DESIGN.md](DESIGN.md#setupvelopack) を参照する。

- `main()`は tokio のランタイムを作る前の同期処理で、全サブコマンドの最初に`VelopackApp`を自動適用**オフ**で動かす。install・updated・uninstall のフックは必ず終了コード0で終え、インストール・更新・アンインストールを止めない。フック内の CLI 呼び出しは1回3秒（`src/app_update/process.rs`の Job 付き時間制限）、フック以外（`--update-apply`・引数なしの起動・stdio の背景スレッド）は30秒。
- 登録は全OSで`setup-registration.json`（データフォルダー`%LOCALAPPDATA%\Kagayoi\Plumivox`の子`app-update\`）に記録する。フックで終わらなかった登録はログオン時の`--update-apply`・引数なしの起動・stdio 起動でやり直す。MCP の登録が消えると Plumivox が起動されないため、この再試行を外さない。
- Claude Code・Codex の`plumivox`登録（`--stdio`付き）は Plumivox のもの: 登録前に`mcp get plumivox`で確かめ、この実行ファイルを指す登録は触らず、別の場所（旧プラグインの`dist`など）を指す同名の登録だけを remove → add で置き換える。ログオン時の適用の Run 値`<packId> Update`（`"<SystemRoot>\System32\conhost.exe" --headless "<exe>" --update-apply`）は、別の導入を指す値を上書き・削除しない。アンインストールと`--uninstall-setup --apply`は自分を指す登録だけを外し、データフォルダーは残す。Toolport の登録は自動で書き換えない（README の案内だけ）。Claude Code・Codex の旧プラグイン（`plumivox@<マーケットプレイス>`）が有効なままかは、`src/app_update/legacy.rs`が両 CLI の設定ファイルを読み取りだけで検知し、警告する（診断ログ・起動時の案内・状態ファイルの`legacyPlugins`）。設定は自動で書き換えない（Codex は同名のユーザー登録がプラグインの MCP より優先される。ソースで確認済み、実機は未確認）。
- 確認・取得は stdio MCP の背景スレッドだけが行い、適用しない（フック・`--proxy-daemon`・`--setup`では行わない）。標準出力へ何も書かず、診断は`app-update\app-update.log`へ。24時間の間引きと`app-update.lock`の排他で、複数の MCP が同時に起動しても取得は1回。管理外のビルド（開発・プラグインの`dist`）は何もしない。
- `--update-apply`（ログオン時）は保留が無ければすぐ終わり、導入先から他のプロセス（MCP・daemon）が1つでも動いていれば`busy`で適用しない（強制終了しない）。適用前に nupkg 内の全 PE の Authenticode 署名者 CN `Open Source Developer Yuichiro Shinozaki`（主実行ファイル`lib/app/plumivox.exe`と`Squirrel.exe`がちょうど1つずつ）を確かめ、再起動せずに適用する。
- `update-feed-test`機能（本番ビルド・署名梱包には入れない）だけが`PLUMIVOX_UPDATE_FEED_DIR`と`PLUMIVOX_UPDATE_ALLOW_UNSIGNED=1`を有効にする。`cargo xtask velopack-pack --sign`はテスト用の packId・版と併用できず、この機能付きのビルドを拒否する。
- セッション・設定・SSH鍵の保存先の禁止先に、Velopack の導入先（`agent_dirs::velopack_install_roots()`）を含める。プラグインルートの判定は変えない。

検証は本物の導入に触れずに行う: テスト用 packId`PlumivoxVelopackTest`（`PlumivoxApp`を使わない）、`--installto`は外部の作業フォルダー配下、子プロセスの`LOCALAPPDATA`・`APPDATA`・`USERPROFILE`・`HOME`・`CODEX_HOME`・`CLAUDE_CONFIG_DIR`・`TEMP`/`TMP`・`PLUMIVOX_SECRETS_FILE`をその配下へ、`PATH`はスタブ（`tests/cli-stub.rs`の`plumivox-cli-stub`）と System32 だけにする。`cargo xtask test-setup-velopack-e2e`がこれを行い、テスト用の Run 値・Uninstall の項目・導入先を本物のアンインストーラーで外して、本番の値と本物の`plumivox`登録の不変を確かめる。Run キーの単体テストは非公開の app hive（`RegLoadAppKeyW`）で行い、本物の Run キーを使わない。本物の Setup・`~/.claude`・`~/.codex`・Toolport の登録・HKCU Run には触れない。

梱包は`cargo xtask velopack-pack`（`vpk pack`。`--sign`は Sigilune の署名テンプレートで署名し、Setup と nupkg 内の全 PE の署名者・発行者・タイムスタンプを検証する）、公開は`cargo xtask velopack-publish`（`PLUMIVOX_UPDATES_R2_*`。版付きの成果物は新規作成のみ、固定名は置き換え、`releases.<channel>.json`は最後）。CI は`.github/workflows/distribution.yml`の`velopack` job（`release/<版>`の push で起動し、`windows` job の成功後に動く。`release/*`上だけ、Environment `r2-velopack`）で、署名用 Secrets を渡す step では cargo を動かさず、先の step でビルドした xtask を直接実行する。Sigilune は同じ TOTP 時間枠の並行ログインを Certum が拒否するため、`windows`と`velopack`の署名は`needs`で直列にする（`workflow_run`は既定ブランチの ref で動き、`release/*`限定の Environment に拒否されるので使わない）。公開済みの版を再 push すると版付きの照合で失敗して何も変わらない。公開・CI 起動（`release/<版>`の push）は明示の依頼があるときだけ行う。
