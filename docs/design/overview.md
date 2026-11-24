# 目的と境界（詳細）

[DESIGN.md](../../DESIGN.md) の「目的と境界」の詳細。

PlumivoxはTwitterAPI.ioを経由してXへの投稿・認証更新・公開投稿の読取りを提供するローカルMCPサーバーである。WindowsではSetup（Velopack）で導入した実行ファイルを、macOSと従来の利用者はCodexプラグインの同梱実行ファイルを、いずれもstdioで起動し、ブラウザ操作は製品の実行経路に持たない。managedモードではloopback限定のCONNECT出口を起動し、既存の公開プロキシへSSH逆転送を張る。操作一覧と取得範囲は [README.md](../../README.md)、設定・保存形式は [docs/configuration.md](../configuration.md)、開発時の作業手順は [AGENTS.md](../../AGENTS.md) を参照する。

## プラグイン経路

`mcp.json`は作業ディレクトリを`${PLUGIN_ROOT}`として`./dist/plumivox --stdio`を起動する。Windowsは`.exe`を解決し、macOSはApple Silicon向けARM64実行ファイルを使う。本体と開発ツール・E2EはRustで、Node.jsのインストールや起動時のコード取得を必要としない。旧Nodeランチャーは廃止し、旧登録はネイティブ実行ファイルへ変更する。マーケットプレイスは`.agents/plugins/marketplace.json`、プラグイン情報は`plugin.json`に置く。
