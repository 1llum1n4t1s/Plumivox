# ローカライズ

[DESIGN.md](../../DESIGN.md#ローカライズ) の詳細。

本人の決定（2026-11-15）により、共通仕様`D:\Dev\APP-DISTRIBUTION.md`の「8. ローカライズ」に揃えた。手本はRemnifoldの`native/src/locale.rs`。

- **人が読む文言は9言語**：ja・en・zh-CN・zh-TW・ko・es・fr・de・pt-BR。対象はCLIの使い方（不明な引数）と`--help`、`--setup`の質問・表示・エラー（`--check`で設定読込みから返ったエラーもコードに応じて訳す）、Setup版の案内（welcome）、`--uninstall-setup`の`description`・`reason`・`next`の値。JSONのキー・`status`・エラーコードは訳さない。
- **言語の決め方**：`PLUMIVOX_LANGUAGE` → `LC_ALL` → `LC_MESSAGES` → `LANG`（空・`C`・`POSIX`は飛ばす）→ OSの表示言語（Windowsは`GetUserDefaultUILanguage`、macOSは`AppleLanguages`の先頭）→ 英語。タグは`zh-Hans`・`zh-CN`・`zh-SG`をzh-CN、`zh-Hant`・`zh-TW`・`zh-HK`・`zh-MO`をzh-TW、`pt-*`をpt-BR、それ以外を主言語で照合する。判定は1プロセスで1回だけ、最初に文言を引いたときに行う。
- **辞書**：`src/locales/<タグ>.json`を`include_str!`で埋め込み、コードは安定した識別子のキー（`setup.prompt.x_user`など）を文字列リテラルで`locale::tr`・`tr_with`に渡す。プレースホルダーは`{name}`。辞書に無いキーは英語、英語にも無ければキーそのものを返す。
- **AIが読む文言は英語**：MCPのツールの説明・入力スキーマの説明（`src/tool-definitions.json`）と、MCP応答のエラー文（`client.rs`・`config.rs`・`server.rs`・`session.rs`・`proxy.rs`）は英語に統一し、辞書を通さない。stdio MCPの経路では言語判定（macOSでは子プロセスを起動する）も行わない。ツール名・引数・応答の形・エラーコード・段階情報（`stage`・`postAttempted`など）は変えていない。投稿本文など利用者のデータは訳さない。
- **整合テスト**（`src/locale.rs`のテスト、`cargo test --lib`で実行）：全言語の辞書のキー集合とプレースホルダーの一致、空の訳がないこと、`src/`のコードが参照するキーがすべて辞書にあり辞書のキーがすべて使われていること、タグ照合とWindowsのLANGID変換、`tool-definitions.json`がASCIIだけであること。実行ファイルでの切替は`tests/setup_e2e.rs`の`cli-text-follows-language-settings`で確かめる。ウィザードのE2Eは`PLUMIVOX_LANGUAGE=ja`に固定して日本語の質問文で照合する。
- **範囲外**：VPSインストーラー（`vps-installer/`・`setup/vps/install.sh`）の表示と、開発用の`xtask`の表示は日本語のまま。`docs/`（`docs/readme/`を除く）・AGENTS.md・DESIGN.md・CHANGELOG.mdは訳さない。READMEは英語の`README.md`を正本に、`README.<タグ>.md`の8言語版を置き、詳細は`docs/readme/`（英語）と`docs/readme/<タグ>/`（8言語）に分ける。
