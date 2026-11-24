# 開発・検証

開発・ビルド・テストにもNode.jsは不要です。Rust/Cargoの`cargo xtask build`、`cargo xtask check`、`cargo xtask test-e2e`を使います。旧Nodeランチャーを指定した登録は、上記のネイティブ起動へ変更してください。

Windowsの`cargo xtask build`はSigilune（Certum SimplySignのクラウド署名）での署名・署名検証後に`dist/plumivox.exe`を更新します。配布CIもWindows成果物をSigiluneで署名してから保存します。署名に必要な開発端末の設定とCIのSecretsは[配布成果物](../../distribution.md#windows配布物のコード署名)を参照してください。WindowsのSetupは`cargo xtask velopack-pack`で梱包し、`cargo xtask test-setup-velopack-e2e`で本物の導入に触れずに隔離検証します。配信は`main`のコミットに付けた軽量タグ`v<版>`のpushで起動する`distribution.yml`のSetup配信jobだけが行います（[Setup（Velopack）の配信](../../distribution.md#setupvelopackの配信)）。

[開発者向けのビルド・検証手順](../../verification.md)、[作業規約](../../../AGENTS.md)、[システム設計](../../../DESIGN.md)を参照してください。
