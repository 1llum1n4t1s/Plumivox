# ローカライズと README の規約

人が読む文言のローカライズと、README 多言語版の同期に関する規約。

- ローカライズ（[設計](../../DESIGN.md#ローカライズ)）: 人が読む文言（CLIの使い方・ヘルプ・エラー、`--setup`の質問と表示、Setup版の案内、`--uninstall-setup`の表示）は`src/locale.rs`の`tr`・`tr_with`で`src/locales/`の9言語の辞書から引く。人が読む文言を足す・変えるときは9言語の辞書を同時に更新し、`cargo test --locked --lib`の整合テスト（キー集合・プレースホルダー・参照キー）を通す。キーは文字列リテラルで渡す。AIが読む文言（`src/tool-definitions.json`のツール説明・入力スキーマ、MCP応答のエラー文）は英語のままにし、辞書を通さない。stdio MCPの経路から`tr`を呼ばない。
- READMEは英語の`README.md`が正本。`README.md`を変えたら、`README.ja.md`・`README.zh-CN.md`・`README.zh-TW.md`・`README.ko.md`・`README.es.md`・`README.fr.md`・`README.de.md`・`README.pt-BR.md`も同じ変更で直す（先頭の9言語の切替行、見出しとコードブロックの構成を揃える）。`docs/`からREADMEの見出しへのリンクは日本語版`README.ja.md`のアンカーを使う。READMEの詳細は`docs/readme/`（英語の正本）と`docs/readme/<lang>/`（`ja`・`zh-CN`・`zh-TW`・`ko`・`es`・`fr`・`de`・`pt-BR`）に分割してあり、変更したら全言語の詳細ファイルと各`README.<lang>.md`の要約を同時に直す。
