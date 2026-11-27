# Setup の MCP 登録と Toolport 判定

Setup（Velopack）が Claude Code・Codex へ`plumivox`を登録する処理（`src/app_update/`・`src/toolport.rs`）の規約。概要は [setup-velopack.md](setup-velopack.md) を参照する。

## 登録

- `plumivox`登録（`--stdio`付き）は Plumivox のもの。公式CLI（`claude mcp add --scope user`・`codex mcp add`）で登録し、設定ファイルを直接書き換えない。登録前に`mcp get plumivox`で確かめ、この実行ファイルを指す登録は触らず、別の場所（旧プラグインの`dist`など）を指す同名の登録だけを remove → add で置き換える。
- ログオン時の Run 値`<packId> Update`（`"<SystemRoot>\System32\conhost.exe" --headless "<exe>" --update-apply`）は、別の導入を指す値を上書き・削除しない。
- アンインストールと`--uninstall-setup --apply`は自分を指す登録だけを外し、データフォルダーは残す。`off`でも解除は行う。
- 旧プラグイン（`plumivox@<マーケットプレイス>`）が有効なままかは`src/app_update/legacy.rs`が両 CLI の設定を読み取りだけで検知して警告する（診断ログ・起動時の案内・状態ファイルの`legacyPlugins`）。設定は自動で書き換えない。

## Toolport 判定

- Toolport の設定は書き換えない。クライアントごとに、ゲートウェイがあり（実行ファイルが実在し）、registry に`command`のファイル名（大文字小文字を区別せず、`.exe`があれば除く）が`plumivox`で`--stdio`を付けて起動する有効なサーバーがあり、ゲートウェイのプロファイルで有効なら、`src/toolport.rs`が読み取りだけで「提供中」と判定し、直接登録を省いて状態の`skipped`に理由を残す。判定できなければ直接登録する。判定は Windows・macOS・Linux で行い（OS に依存せず、上のファイル名の規則で一致させる）、その他の OS は判定せず直接登録する。quarantine は判定しない。
- `app-update.json`の`mcp_registration`: `auto`（既定）・`direct`・`off`。不正値は`auto`扱い。判定結果の種類・`Unknown`の理由・不正値は登録処理の前に1回`app-update.log`へ残す（秘密値は入れない）。
- 既存の直接登録は外さない。一度直接登録したクライアントは外されても足し直さず、戻すのは省いたクライアントと前回未完了のクライアントだけ。省いた後にゲートウェイが消えた（サーバーが一致しても実行ファイルが無い場合を含む）ら、次の`ensure_registered`（引数なしの起動・stdio MCP・ログオン時の`--update-apply`）で直接登録し、Toolport側で無効化・削除した場合は足さない。
- 状態の作り直し時に提供中なら CLI を呼ばないため、移動前の exe を指す直接登録は残る。
- Codexで直接登録を省くと旧プラグインのMCPをユーザー登録で抑える効果が無くなり、二重起動し得る（`legacy.rs`の警告は出る）。
- `src/toolport.rs`は Edictora`crates/edictora/src/install/toolport.rs`が正本のバイト同一の複製（Remnifold`native/src/toolport.rs`も同じ）。ここでは編集せず、正本で変えて3つへ配り直す。
