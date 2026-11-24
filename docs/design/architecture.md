# 主要コンポーネント（詳細）

[DESIGN.md](../../DESIGN.md) の「主要コンポーネント」の一覧。

| コンポーネント | 責務 |
| --- | --- |
| `src/main.rs` / `src/lib.rs` | 同期の`main`で最初にVelopackの起動時処理を行ってからtokioのランタイムを作る。引数なし／`--stdio`で本体を起動し、`--setup`・内部の`--proxy-daemon`・Setup版の`--update-apply`・`--uninstall-setup`・`--help`を同じネイティブ実行ファイルへ接続する |
| `src/locale.rs` / `src/locales/<タグ>.json` | 人が読む文言の言語判定と9言語の辞書（[ローカライズ](conventions.md)）。辞書の整合テストも持つ |
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
| `.github/workflows/distribution.yml` / `xtask/src/ci.rs` / `xtask/src/r2.rs` / `xtask/src/velopack.rs` | 配布CI（[配布と依存更新](release-process.md)）。利用者のMCP実行から独立する |
| `tests/` 配下のE2E | [検証の構成](testing.md)を参照 |
