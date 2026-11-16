# Plumivox の設計

## 目的と境界

PlumivoxはTwitterAPI.ioを経由してXへの投稿・認証更新・公開投稿の読取りを提供するローカルMCPサーバーである。WindowsではSetup（Velopack）で導入した実行ファイルを、macOSと従来の利用者はCodexプラグインの同梱実行ファイルを、いずれもstdioで起動し、ブラウザ操作は製品の実行経路に持たない。managedモードではloopback限定のCONNECT出口を起動し、既存の公開プロキシへSSH逆転送を張る。操作一覧と取得範囲は [README.md](README.md)、設定・保存形式は [docs/configuration.md](docs/configuration.md)、開発時の作業手順は [AGENTS.md](AGENTS.md) を参照する。

## 主要コンポーネント

| コンポーネント | 責務 |
| --- | --- |
| `src/main.rs` / `src/lib.rs` | 同期の`main`で最初にVelopackの起動時処理を行ってからtokioのランタイムを作る。引数なし／`--stdio`で本体を起動し、`--setup`・内部の`--proxy-daemon`・Setup版の`--update-apply`・`--uninstall-setup`・`--help`を同じネイティブ実行ファイルへ接続する |
| `src/locale.rs` / `src/locales/<タグ>.json` | 人が読む文言の言語判定と9言語の辞書（[ローカライズ](#ローカライズ)）。辞書の整合テストも持つ |
| `src/app_update.rs` / `src/app_update/` / `src/connect.rs` | Setup（Velopack）導入版のフック、背景の更新確認・取得、ログオン時の適用（他プロセスの検出・署名者検証）、Run値、登録状態、Claude Code・Codexへの`plumivox`登録。管理外のビルドでは何もしない |
| `src/server.rs` / `src/tool-definitions.json` | stdioのMCP JSON-RPC、11ツールの入力スキーマと検証、MCP注釈、アカウント選択、認証処理、定型エラー応答 |
| `src/config.rs` | 呼出し時の設定再読込み、アカウント・資格情報の検証、Cookieの選択、秘密を除いた準備状態の公開 |
| `src/setup.rs` / `src/setup_windows.rs` / `setup/vps/` | Windows・macOSのネイティブ設定ウィザード、Windowsの保存先・ACL・排他制御、VPSの認証付き公開プロキシの初回構築。通常の投稿処理から独立する |
| `vps-installer/` / `setup/vps/plumivox-vps-setup-x86_64` / `setup/vps/plumivox-vps-setup-aarch64` | Linux x86_64・aarch64向けの静的Rustインストーラー。他CPUには`setup/vps/install.sh`のBash互換経路を残す |
| `skills/setup-plumivox/SKILL.md` | 同梱資材を使う初期設定の案内。通常の投稿前条件を増やさない |
| `src/session.rs` | ネイティブDPAPI／Keychain内の鍵によるAES-GCM、保存先検証・権限、セッション照合、OS別の認証ロック、暗号化保存 |
| `src/proxy_egress.rs` | loopback限定のCONNECT出口、X宛先制限、DNS全候補検証、接続数・容量・時間制限 |
| `src/proxy_ipc.rs` | 相互HMAC認証IPC。Windowsの名前付きパイプとmacOSのUnix socketを利用する |
| `src/proxy.rs` | 設定と鍵の検証、russhによるSSHホスト鍵照合・逆転送・再接続、共有daemon・複数チャット・idle終了・取消・停止の制御 |
| `src/client.rs` / `src/api_rejection.rs` / `src/client_emoji.txt` | HTTP通信、応答検証・正規化、秘密を含まない拒否理由の固定分類、画像検証・アップロード、投稿、フィード再構成、検索条件と継続cursor、Unicode 17絵文字の検証 |
| `Cargo.toml` / `Cargo.lock` / `rust-toolchain.toml` / `.cargo/config.toml` / `xtask/` | 本体と開発ツールの依存・toolchain固定、Windows x64とmacOS ARM64のビルド、ライセンス生成、検査・E2E実行。Node.jsを使わない |
| `dist/plumivox.exe` / `dist/plumivox` | 正式なネイティブ実行ファイル |
| `.github/workflows/distribution.yml` / `xtask/src/ci.rs` / `xtask/src/r2.rs` / `xtask/src/velopack.rs` | `release/<版>`のpushで起動する配布CI（`unix`・`windows`・`velopack`の3 job）。RustによるWindows・macOS本体とLinux x86_64・aarch64のVPSインストーラーの検証とビルド、非公開R2への成果物保存・検証付き取得、`windows`の成功後に`velopack`が行うSetupの梱包・署名・署名者検証・公開R2（`plumivox-updates`の`velopack/`）への配信、隔離E2E（`tests/setup_velopack_e2e.rs`・`tests/cli-stub.rs`）。利用者のMCP実行から独立する |
| `tests/api_e2e.rs` / `tests/native_e2e.rs` / `tests/tls_e2e.rs` / `tests/support/` / `tests/native-fixture.rs` | Rustの偽HTTP・HTTPS APIと別プロセスのMCPを使い、通信内容・回数・失敗状態を検証する。失敗注入は`test-fixture`ビルドに限定する |
| `tests/proxy_e2e.rs`と`src/proxy.rs` / `src/proxy_egress.rs` / `src/proxy_ipc.rs`内のテスト | Rustの偽SSH・echo、認証付きIPCとCONNECT出口の境界を検証する |
| `tests/setup_e2e.rs` / `tests/setup_vps_e2e.rs` | Windows・macOSの設定ウィザード、個別保存先、鍵の保護、VPS CLIの検証と拒否条件。実VPSへの導入は別途確認が必要 |

`mcp.json`は作業ディレクトリを`${PLUGIN_ROOT}`として`./dist/plumivox --stdio`を起動する。Windowsは`.exe`を解決し、macOSはApple Silicon向けARM64実行ファイルを使う。本体と開発ツール・E2EはRustで、Node.jsのインストールや起動時のコード取得を必要としない。旧Nodeランチャーは廃止し、旧登録はネイティブ実行ファイルへ変更する。マーケットプレイスは`.agents/plugins/marketplace.json`、プラグイン情報は`plugin.json`に置く。

開発用の成果物は、`release/<版>`のpush（同じブランチでの手動再実行を含む）で起動する配布CIが各OSの実行環境で検証してから非公開R2へ保存する。`main`の手動起動はmacOSとLinuxの成果物だけを作る。`plumivox/ci/{commit}/{run}/{attempt}/{target}/`に実行ファイル・第三者ライセンスを保存し、サイズとSHA256を含むmanifestを最後に保存する。既存オブジェクトの上書きを拒否し、取得ではidentity・ファイル名・サイズ・SHA256を照合する。この非公開R2は配布準備の受け渡しであり、GitHub mainの同梱`dist`を使うプラグインの導入・更新経路は、Setupの配信を確かめて縮小するまで維持する。Setupの配信は別の公開バケットで行う（[Setup（Velopack）](#setupvelopack)）。CIは`dist`を自動コミットせず、latestの差替えや過去成果物の自動削除も行わない。操作・保持方針は [docs/distribution.md](docs/distribution.md) を参照する。

## 初期設定と公開中継の構築

設定ストアは環境変数、内部の明示パス、現在の利用者の既存 `~/dev/Secret/secrets.json`、利用者別の標準保存先の順にファイルを選ぶ。Windows・macOSのセットアップも同じ順序を使う。既存利用者の設定を維持しつつ、特定利用者の絶対パスに依存しないためである。MCPは起動時に選んだファイルを各呼出しで読み直し、消失・不正時に別ファイルへ切り替えない。保存先と設定項目は [設定仕様](docs/configuration.md) を参照する。

`--setup`は専用Ed25519鍵の準備、設定ウィザード、通信を伴わないローカル確認を担当する。Windows・macOSともRust本体で対話入力を処理する。鍵と設定の保存先・本人所有を確認してOS別の利用者限定アクセス権を適用し、適用後にも検証する。所有者は変更せず、所有者の再設定に必要な権限を要求しない。保存先ごとにWindowsのMutexまたはmacOSのファイルロックで排他し、既存鍵・同名アカウント・共有中継設定は上書きせず、他の設定項目を保持し、保存直前に元の内容との一致を確認する。macOSでは本人所有・リンクでない保存先を検証し、ディレクトリ700・ファイル600を適用する。ホスト鍵指紋はVPS提供元のコンソールで照合した値を入力する。初期設定の完了やローカル確認は、SSH接続・Xログインの成功を意味しない。

VPSインストーラーはDebian 12以降／Ubuntu 22.04以降の専用VPSを対象とし、既存SSH・有効なUFW・競合を事前確認してからSquidと専用SSHユーザーを構築する。再実行は管理対象のハッシュ・権限・導入条件が一致する場合だけ受け入れ、設定変更を自動適用しない。公開IPv4口はBasic認証付きのX宛先・443番CONNECTに限定し、SSHユーザーは指定loopbackポートへの逆転送だけを許可する。Squidは逆トンネルを親プロキシとして使い、PC出口が不在でもVPSから直接接続しない。systemdは資格情報の受渡しと資源制限を担当する。事業者側のファイアウォール設定は利用者が行い、通常のMCP処理はVPSの構築・設定変更を行わない。導入操作と確認手順は [セットアップ手順](docs/setup.md) を参照する。

Rust版VPSインストーラーはx86_64・aarch64の静的実行ファイルを優先し、他CPUでは同じCLI引数のBash互換経路を利用できる。新規導入ではSSH設定のACL・拡張属性・SELinux・全属性の確認に必要な`CAP_SYS_ADMIN`などを安全に扱えなければ変更前に拒否する。既存の同一導入の読取り確認はこの条件とは分ける。`--check`は書込み試験をせず、合格後も`RENAME_EXCHANGE`非対応などによって導入を安全に停止する場合がある。

Bash互換経路の新規導入にも同じ属性・capabilityの安全条件と同一ファイルシステム内の交換が必要で、`mv --exchange --no-copy -T`と`ln -T`対応GNU coreutils、`getfacl`・`getfattr`・`sync`・`readlink`・`lsattr`を要求する。古いcoreutilsや必要なコマンドがない環境では変更前に停止し、管理者の環境確認・準備前に自動導入・更新しない。Bash版も`--check`で書込みprobeを行わない。

新規導入は初期user namespace内の`CAP_SYS_ADMIN`、SSH設定と親ディレクトリのmount ID一致、保持不能なinode flagsがないことを要求し、別mountや確認不能な状態を拒否する。新規保存先の既存名との競合は上書き・再利用で回避しない。清掃失敗時はstage・バックアップの保持先を案内し、管理者が今回分と交換状態を確認して整理する。

SSH設定の交換中は同時管理操作を避ける。Rust版・Bash版とも交換した旧inodeをroot限定のバックアップディレクトリに保持し、旧ファイル記述子を利用する処理の終了、新旧内容、SSH管理接続の動作を本人が確認してから整理する。自動復元・自動清掃を行わず、安全条件をBash版への切替えで迂回しない。

配布CIはWindows・macOS本体に加え、Linux x86_64・aarch64で静的VPSインストーラーをビルドして対象CPUで起動を確認する。処理は`xtask/src/ci.rs`のRustコマンドへ集約し、PowerShellスクリプトは使わない。VPS成果物も既存R2経路で受け渡し、GitHub mainの同梱資材から導入する。ローカルのCLI・fixture検証と、実システムへの導入・実R2通信・CI起動の結果は区別する。

## Setup（Velopack）

本人の決定（2026-11-15）により、Windowsの既定の配布をSetup（Velopack）へ移した。共通仕様は`D:\Dev\APP-DISTRIBUTION.md`の多プロセス型、参考実装はRemnifold。packIdは`PlumivoxApp`、導入先は既定で`%LOCALAPPDATA%\PlumivoxApp`（`current\plumivox.exe`）、ユーザー単位で管理者権限を要求しない。

- **起動時処理**：`main`は全サブコマンドの最初に`VelopackApp`を自動適用オフで実行する。Velopackのフック引数（`--veloapp-install`など）はここで処理して終了コード0で終える。Velopackは環境変数を書き換えるため、tokioのスレッドを作る前の同期処理で行う。
- **多プロセス型の適用**：Velopackの適用は導入先の自分以外の全プロセスを強制終了する。Plumivoxはセッションごとのstdio MCPと、MCPが`current_exe()`から起動する共有中継daemonが導入先から動くため、日中には適用しない。確認・取得はstdio MCPの背景スレッドだけが行い（24時間の間引き、失敗も24時間は再試行しない、`app-update.lock`で複数MCPの重複取得を防ぐ）、適用はログオン時のRun値から起動する`--update-apply`だけが行う。導入先から他のプロセスが1つでも動いていれば`busy`として次のログオンへ回し、強制終了しない。
- **信頼**：取得はR2からのHTTPSとVelopackのフィードのハッシュ照合で守り、適用前にnupkg内の全PE（.exe・.dll）のAuthenticode署名が有効で、署名者のCNが本人の証明書（`Open Source Developer Yuichiro Shinozaki`）であることを確かめる。主実行ファイル`lib/app/plumivox.exe`と更新器`Squirrel.exe`はちょうど1つずつ、曖昧な名前を含むパッケージは拒否する。失効確認はしない（ログオン時のオフラインでも適用できるようにするため）。配信元へ別のビルドを置かれた場合も、署名者が違えば適用しない。
- **登録**：インストールフックと、フック以外の起動（`--update-apply`・引数なしの起動・stdio起動）で、Claude Code（`claude mcp add --scope user plumivox -- <exe> --stdio`）とCodex（`codex mcp add plumivox -- <exe> --stdio`）へ公式CLIで登録する。設定ファイルを直接書き換えない。`mcp get`でこの実行ファイルを指す登録は触らず、旧プラグインの`dist`など別の場所を指す同名の登録だけを置き換える。フックはCLI 1回を3秒で子孫ごと打ち切り（Velopackはフックを15～30秒で打ち切る）、終わらなかったCLIは`setup-registration.json`に未完了と残して30秒の上限でやり直す。更新の後は、パスが同じならMCPの登録を触らずRun値だけを張り直す（使用中のセッションの登録を外さないため）。CLIはPATH上の`.exe`だけを探す。
- **状態の置き場**：`auto_update`（既定ON）、確認・失敗時刻、ロック、診断ログ、登録状態は、データフォルダー（セッション保存の既定の親）の子`app-update\`に置く。インストールフックが`--setup`より先にデータフォルダーを作っても、セッション保存と同じ本人限定の権限（Windowsは保護付きDACL、macOSは0700）で作る。導入先の`current\`は更新で入れ替わるため使わない。
- **保存先の保護**：セッション・設定・SSH鍵の保存先の禁止先に、Velopackの導入先（実行中の導入の`get_root_dir()`と、既定の`%LOCALAPPDATA%\PlumivoxApp`・`/Applications/Plumivox.app`・`~/Applications/Plumivox.app`）を加えた。プラグインルートの判定は変えていない。
- **解除**：アンインストールフックと`--uninstall-setup --apply`は、この実行ファイルを指すMCP登録とRun値だけを外し、データフォルダー（設定・セッション・`app-update.json`）を残す。macOSにはVelopackのフックとアンインストーラーが無いため、引数なしの起動・stdio起動で登録し、解除は`--uninstall-setup --apply`で行う（`osx`チャネルは未配信）。
- **案内**：管理下のビルドを端末から引数なしで起動した場合とSetup直後の初回起動は、stdioを待たずに導入済み・登録済みであることと、未設定なら`--setup`の実行を案内する。初回起動と、コンソールに付いているのが自分だけ（スタートメニューからの起動など、終了とともに窓が閉じる）のときは、Enterを待ってから閉じる。秘密値を扱う`--setup`は自動で始めない。silentのSetupは導入後にアプリを起動しない（Velopack 1.2.161で確認）。MCPクライアントはstdinをパイプで渡すため従来どおりstdioで動き、管理外のビルドの素の起動もstdioのまま。
- **配信**：`distribution.yml`の`velopack` job（`release/<版>`のpushで起動し、`windows` jobの成功後に動く・`release/*`だけ・Environment `r2-velopack`。SigiluneのTOTP時間枠が`windows`の署名と重ならないよう`needs`で直列にする）が、cargoのビルドを済ませた後に、署名用Secretsを渡すstepでは事前にビルドしたxtaskだけを実行して梱包・署名・署名者検証を行い、公開R2の`velopack/`へ置く。版付きの成果物は新規作成のみ（既存なら内容を照合し、違えば失敗）、固定名のSetup・`releases.win.json`などは置き換え、`releases.win.json`を最後に置く。
- **範囲外**：Linuxは対象外（VPSインストーラーはそのまま）。プラグインの縮小（`mcp.json`と`dist/`の削除）とToolport登録の自動書き換えは行わない。

## ローカライズ

本人の決定（2026-11-15）により、共通仕様`D:\Dev\APP-DISTRIBUTION.md`の「8. ローカライズ」に揃えた。手本はRemnifoldの`native/src/locale.rs`。

- **人が読む文言は9言語**：ja・en・zh-CN・zh-TW・ko・es・fr・de・pt-BR。対象はCLIの使い方（不明な引数）と`--help`、`--setup`の質問・表示・エラー（`--check`で設定読込みから返ったエラーもコードに応じて訳す）、Setup版の案内（welcome）、`--uninstall-setup`の`description`・`reason`・`next`の値。JSONのキー・`status`・エラーコードは訳さない。
- **言語の決め方**：`PLUMIVOX_LANGUAGE` → `LC_ALL` → `LC_MESSAGES` → `LANG`（空・`C`・`POSIX`は飛ばす）→ OSの表示言語（Windowsは`GetUserDefaultUILanguage`、macOSは`AppleLanguages`の先頭）→ 英語。タグは`zh-Hans`・`zh-CN`・`zh-SG`をzh-CN、`zh-Hant`・`zh-TW`・`zh-HK`・`zh-MO`をzh-TW、`pt-*`をpt-BR、それ以外を主言語で照合する。判定は1プロセスで1回だけ、最初に文言を引いたときに行う。
- **辞書**：`src/locales/<タグ>.json`を`include_str!`で埋め込み、コードは安定した識別子のキー（`setup.prompt.x_user`など）を文字列リテラルで`locale::tr`・`tr_with`に渡す。プレースホルダーは`{name}`。辞書に無いキーは英語、英語にも無ければキーそのものを返す。
- **AIが読む文言は英語**：MCPのツールの説明・入力スキーマの説明（`src/tool-definitions.json`）と、MCP応答のエラー文（`client.rs`・`config.rs`・`server.rs`・`session.rs`・`proxy.rs`）は英語に統一し、辞書を通さない。stdio MCPの経路では言語判定（macOSでは子プロセスを起動する）も行わない。ツール名・引数・応答の形・エラーコード・段階情報（`stage`・`postAttempted`など）は変えていない。投稿本文など利用者のデータは訳さない。
- **整合テスト**（`src/locale.rs`のテスト、`cargo test --lib`で実行）：全言語の辞書のキー集合とプレースホルダーの一致、空の訳がないこと、`src/`のコードが参照するキーがすべて辞書にあり辞書のキーがすべて使われていること、タグ照合とWindowsのLANGID変換、`tool-definitions.json`がASCIIだけであること。実行ファイルでの切替は`tests/setup_e2e.rs`の`cli-text-follows-language-settings`で確かめる。ウィザードのE2Eは`PLUMIVOX_LANGUAGE=ja`に固定して日本語の質問文で照合する。
- **範囲外**：VPSインストーラー（`vps-installer/`・`setup/vps/install.sh`）の表示と、開発用の`xtask`の表示は日本語のまま。`docs/`・AGENTS.md・DESIGN.md・CHANGELOG.mdは訳さない。READMEは英語の`README.md`を正本に、`README.<タグ>.md`の8言語版を置く。

## データフロー

1. MCP要求をスキーマで検証し、設定ファイルの `twitterapi_io` から対象を解決する。明示した名前、`PLUMIVOX_DEFAULT_ACCOUNT`、埋込みprofile、`twitterapi_io.default_account`、唯一の登録先の順に選ぶ。保存された既定が不正なら省略呼出しを通信前に拒否し、複数登録・既定なしの場合も先頭へ自動選択しない。認証・中継準備へは選択済みの設定名を渡し、途中の既定変更で送信先を切り替えない。
2. 公開投稿の読取り4ツールは`AccountStore::get_read_account`でAPIキーとユーザー名だけを取得し、セッションの検査・読込み・復号を行わない。読取りをCookieの状態やDPAPI・Keychainに依存させないためである。一方、`list_accounts` と `login_account` のCookie再利用判定は準備状態を返すためセッションを検査する。投稿にはプロキシとCookieも要求する。Cookieが必要な処理ではPlumivox保存セッションを最優先し、保存先が無い場合だけ設定Cookie、旧vava／blblのDPAPI保存を参照する。不正な保存セッションから旧Cookieへ戻らない。
3. 読取りはAPI応答を必要な投稿フィールドへ正規化する。フィードはフォロー一覧で保護アカウントと判明した対象を除外し、最大200アカウントを20件ずつOR検索する。選択した検索対象の保護状態が不明ならcoverageへ未知数を残す。ページ内で投稿IDを重複除去し、日時降順へ並べる。
   `search_tweets` は語句と期間を検証し、全体検索では直接、フォロー先検索では同じフィード取得処理へ条件を追加して検索する。語句を引用したフレーズとし、タイムゾーン付き日時をUTCのUnix秒へ変換して、フォロー先条件と空白で結合する。
4. 投稿は加重文字数と全画像を通信前に検証する。本文をNFC正規化し、twitter-textで抽出したURLを23文字、単独文字をv3の重みで数える。Rust版パーサーと新しい複合絵文字の分割が異なるため、Unicode 17の複合絵文字はemoji-regex由来の同梱固定データで認識して重み2へ補正する。画像を順にアップロードし、得たメディアIDで投稿APIを1回呼ぶ。成功には数値形式の投稿IDを要求する。
5. 認証では `login_account` は利用可能な形式のCookieを再利用し、`refresh_session` は常にログインAPIを呼ぶ。再利用判定でも設定を読み直すが、準備状態の確認とセッション復号は選択済みのアカウントだけを対象にする。更新はプロセス内とプロセス間で排他し、資格情報を再読込みしてログイン前後の変更を検出する。WindowsはDPAPI、macOSはKeychainに保存した鍵のAES-GCMでCookieを暗号化し、一意のpendingファイルを書く。WindowsのMutex所有スレッドまたはmacOSのファイルロックを保持した処理で旧本体のバックアップと本体の置換を行う。ロックの所有を確認し、旧処理からの保存を防ぐ。
6. MCP成功応答はJSONテキストと `structuredContent`、業務エラーは定型コード・メッセージと `isError` で返す。保存前の取消・所有喪失・不正Cookieと、保存本体の置換指示後の結果不明、保存処理自体の失敗を区別する。入力スキーマ拒否はMCPプロトコル側のエラーとなる。

## 不変条件と採用した設計判断

- **既定アカウント**：`default_account`は設定名を指すメタデータであり、アカウント一覧には含めない。`list_accounts`の`isDefault`は優先設定も含めた有効な省略先を示し、既定が不正でも一覧取得は許可する。セットアップは初回登録を既定に保存し、有効な既定は保持する。既定だけの変更では既存のアカウント・中継設定を維持し、認証入力や通信を要求しない。操作と詳細な優先順位は [設定](docs/configuration.md) を参照する。
- **秘密の境界**：資格情報と復号Cookieは内部処理で使用する。公開状態は準備状況であり、Xでの認証成功を保証しない。WindowsはDPAPIのCurrentUserとユーザー限定ACL、macOSはログイン利用者のKeychainと本人限定のファイル権限を使う。認証処理はOSのネイティブAPIを使い、PowerShellに依存しない。
- **保存の結び付け**：保存先はアカウント名と小文字ユーザー名のハッシュで分け、schema version 2、ユーザー名、APIキーとプロキシのbindingを照合する。保存先はリンク先も検査し、リポジトリとCodex・Claude Codeなどのユーザー設定ディレクトリ（`~/.codex`・`~/.claude`、`CODEX_HOME`・`CLAUDE_CONFIG_DIR`の移動先）を拒否する。旧保存形式は読取り互換であり、更新先にはしない。
- **投稿の成否**：投稿作成・ログインの自動再送やローカル投稿履歴による重複抑止はない。通常のAPI要求は45秒、画像アップロードは1画像あたり120秒を上限とする。画像アップロードだけは、HTTP 502・503・504では同画像を250ms、プロキシ・上流障害・原因不明のJSON拒否（rejectionReasonsがproxy／upstream_*／unknownのみ）と通信切断（network）では1秒待機後に最大1回再試行し（認証・残高・メディア不正・重複・レート制限・タイムアウトは再試行せず）、待機と全要求を1画像の`image_timeout`内に収める。成功済み画像を再送せず、全画像の成功後に投稿作成を1回だけ呼ぶ。既知の投稿エラーには固定値のstage、postAttempted、画像要求総数uploadAttemptsを残し、画像の通信失敗には秘密値を含まないfailureKind（timeout／network／cancelled）を付ける。投稿専用のMCP境界で準備失敗の既定値を付け、クライアント側の実際の段階情報を優先する。未知の内部エラーでは未送信と断定しない。成否不明は `POST_UNCERTAIN` として返し、成功・未投稿を断定しない。明示的な再呼出しでは同文も毎回送信し、同文の受理・拒否はX側へ任せる。認証更新は投稿とは独立し、失敗した投稿を再開しない。
- **通信の境界**：API接続先は `https://api.twitterapi.io` と検証用HTTPループバックに限定し、リダイレクトを拒否する。応答はストリームで読み、10MiBを超えたら受信を中止する。タイムアウトとMCPの中止シグナルをHTTPへ渡し、画像アップロード後も中止状態を確認して投稿開始を防ぐ。提供元の生エラーは外へ返さない。
- **フィードの限界**：公式ホームの再現ではない。検索バッチは順に取得して要求数を制限するが、保護投稿や全フォロー先の網羅、ページ間の全体時系列順は保証しない。フォロー一覧の続きと検索の続きは別cursorで、同時指定を拒否する。提供元または生成cursorが受理上限を超えた場合は、取得済み投稿・提供元の続き有無を維持し、継続不能の理由をcoverageへ返す。opaqueなcursorは切り詰めない。
- **継続情報の境界**：フィードのcursorはversion 2で検索対象とcoverageをAPIキーのHMACで署名し、アカウントとユーザー名を照合して初回の検索対象を固定する。旧署名なし形式は拒否する。検索用cursorは条件と用途（投稿の続き／フォロー一覧の続き）を同じ方式で署名し、フィード用cursorとの混同や別条件への流用を拒否する。フォロー先検索では署名付きフィードcursorを内包する。APIキー変更で既存cursorは無効になる。署名は改変検知のためであり、内容の暗号化ではない。
- **配布と検証**：ネイティブ実行ファイルを同梱し、利用者側のNode.js・Rust・`node_modules`を不要にする。Rustソース変更時は対象OSの配布物を再ビルドする。E2EとRustテストは実際のXへの副作用を避けつつMCP・HTTP・OS別の保存とプロキシ境界を検証するが、実サービス上のCookie有効性や投稿成功を証明するものではない。
- **依存更新の自動化**：`.github/dependabot.yml` はCargo依存とGitHub Actionsを週次で更新し、minor／patchをまとめ、majorは個別PRにする。`dependency-validation.yml` はDependabot PRまたは手動起動でWindows上で`cargo xtask`の`build-unsigned`・`check`・`test-e2e`（ビルド・構文確認・全E2E）を別々のjobで並列に実行する（同じrefの古い実行は新しい実行で打ち切る）。Cargoの更新PRはWindows・macOS双方の正式配布物の再生成が必要なため自動マージせず、配布物を含めて手動で統合する。

## 管理するプロキシの境界

`proxy_mode` が `managed` のアカウントだけ、本文・画像の事前検証後または認証ロック取得後に中継を準備し、準備後に設定を再確認する。明示的な `start_proxy` でも起動できる。公開proxy URLは変更せず、Cookie bindingを維持する。設定の取得は資格情報ストア経由に限定し、API引数・MCP応答にSSH鍵や接続先を含めない。読取り・設定一覧・保存Cookieの再利用は中継を起動せず、external呼出しは既存中継に干渉しない。

投稿・ログインのAPI要求は従来どおりTwitterAPI.ioへ送り、同サービスが公開プロキシへ接続する。公開口から中継サーバーのloopbackポート、SSH逆転送、PCのloopback限定CONNECT出口を通り、住宅回線からXへ接続する。出口はX/Twitter/Twimgのドメインと443番だけを許可し、DNS全候補に特殊アドレスがあれば拒否する。検証済み公開IPへ直接接続してDNSの再解決を避け、接続数・容量・時間を制限する。

Windowsは設定別の名前付きパイプ、macOSは利用者専用ディレクトリ内のUnix socketへ接続し、未起動なら同じネイティブ実行ファイルを独立daemonとして起動する。Windowsでは起動時に親MCPの標準ハンドルの継承を抑止し、daemonが終了済みMCPの通信パイプを保持しないようにする。macOSではディレクトリ・socketの所有UIDと権限、接続相手のUIDを照合する。ネイティブIPCはv3の独立した名前空間を使い、旧JavaScript daemonと接続しない。SSHと固定ポートの所有を一つに集約して複数チャットで共有する。IPC秘密はSSH鍵ファイルから導出し、設定識別子・クライアントとチャネルのnonce・要求番号・操作・応答内容全体をHMACで結び付けて相互認証する。同じ鍵を使う別設定への転送や過去応答の再利用を拒否する。

SSHの再接続は通信経路だけを復旧し、投稿・ログインは再送しない。停止では最後に利用した設定の共有daemonのsocket、timer、逆転送、ローカル出口を閉じる。生きている自己SSH接続は転送解除応答と切断完了を有限時間だけ待ち、次の起動との競合を抑える。応答がない場合や既に通信が切れている場合は、VPS側のポート解放を保証しない。PCが消失した接続はVPS専用ユーザーの生存確認（15秒間隔・応答なし3回）で検知する。MCP終了ではそのクライアントだけを離脱し、他チャットの中継を維持する。全クライアントが離脱して5分後にdaemonを終了する。ポート競合では別設定の所有者を終了させず固定コードで拒否する。

別のMCPプロセスで停止と並行して進んでいる起動要求は、まだ共有daemonへ接続していない場合、停止後に完了することがある。

公開口は引き続き中継サーバーが担当し、無料Cloudflare Tunnel/WorkersをHTTP CONNECT公開プロキシの代替として実装していない。外部経路の初回準備と管理設定は [中継手順](docs/managed-proxy.md) を参照する。
