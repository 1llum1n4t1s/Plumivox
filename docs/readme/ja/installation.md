# インストール

## Windows: Setupで導入する（推奨）

1. [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)をダウンロードして実行します。利用者単位の導入で、管理者権限は不要です。Setupは本人のコード署名（`Open Source Developer Yuichiro Shinozaki`）付きです。
2. `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`に導入され、PATH上に`claude`・`codex`のCLIがあれば、Claude Code（利用者単位）とCodexへMCPサーバー`plumivox`（引数`--stdio`）を自動で登録します。CLIが見つからなかった場合は、後でCLIを入れてから次のログオンかスタートメニューのPlumivoxを起動すると登録します。
3. 導入直後の画面の案内に従い、ターミナルで`"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup`を実行して[初期設定](../../../README.ja.md#初期設定)を行います。Claude Code・Codexは新しいセッションから使えます。

更新は自動です。MCPの起動中に新しい版をR2（`https://plumivox-updates.kagayoi.com/velopack/`）から取得し、パソコンへのログオン時に、Plumivox（MCP・共有中継）が動いていないときだけ適用します。動いていれば次のログオンへ回し、強制終了しません。適用前に署名者を確かめ、違えば適用しません。自動更新を止めるには`%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json`を`{"auto_update": false}`にします。動作の記録は同じフォルダーの`app-update.log`にあります。

アンインストールは「設定」→「アプリ」の一覧からPlumivoxを選びます。自分を指すClaude Code・Codexの`plumivox`登録とログオン時の更新の登録だけを外し、設定・セッションのデータフォルダー（`%LOCALAPPDATA%\Kagayoi\Plumivox`）は残します。登録だけを先に外す場合は`plumivox.exe --uninstall-setup`で予定を確認し、`--uninstall-setup --apply`で実行します。

`plumivox.exe`のそのほかのコマンドラインの指定: `--setup`（初期設定）、`--uninstall-setup`（登録の解除）、`--rejection-log on|off|status|show|clear`（伏字処理した拒否文面の期限付きローカル記録。既定は無効。[拒否文面のローカル記録](when-posting-or-authentication-fails.md#拒否文面のローカル記録)を参照）。記録はアンインストールしても残り、`--rejection-log clear`で消します。

プラグイン（下記）から移る場合: Setupは旧プラグインの`dist`などを指す同名の`plumivox`登録を、導入先を指す登録へ置き換えます。プラグイン自体が持つMCP（`plumivox@plumivox-local`）は別の登録なので、Plumivoxが2経路で起動しないよう、無効にするかプラグインを外してください。Codexでは[Toolportの節](#toolport経由で使う場合)と同じ`enabled = false`を追加する（またはプラグインを外す）、Claude Codeでは`/plugin`からプラグインを無効化または削除します。Codexは同名の登録をプラグインのMCPより優先するため、Setupの登録がある間は1つしか起動しないはずですが、Claude Codeでは両方が起動し得ます。SetupはCodex・Claude Codeの設定を書き換えず、プラグインが有効なままなら警告だけを出します（Plumivoxを起動したときの案内、`app-update.log`、`app-update\setup-registration.json`の`legacyPlugins`）。Toolportの登録も自動では書き換えないので、Toolportを使う場合は[Toolportの節](#toolport経由で使う場合)を見て自分で確認してください。

## プラグインで導入する（macOS・従来の方法）

macOS（Apple Silicon）はSetupの配信を始めるまでこの方法を使います。Windowsでも引き続き使えます。

GitHubリポジトリの `main` の最新コミットからインストールします。利用者の開発用クローンは参照せず、タグ・コミットSHAには固定しません。

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` はマーケットプレイスの識別子です。取得元は上記のGitHub URLです。Windows x64用の`dist/plumivox.exe`とApple Silicon Mac用の`dist/plumivox`（ARM64）を同梱しているため、利用者による依存インストールやMCP設定の手動追加は不要です。

開発用CIではWindows・macOSで検証した成果物をCloudflare R2へ保存し、配布準備時に受け渡します。[成果物の保存・取得手順](../../distribution.md)は開発者向けです。プラグイン利用者にR2の設定や資格情報は不要で、インストール・更新は引き続きGitHubの`main`から行います。

更新するときは `main` の最新状態を取得してインストールし、Codexを再起動して読み込みます。

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

一覧の`installed`に表示される版番号を確認します。導入済みのプラグインは更新後に再度`add`せず、Codexを再起動して新しいMCPとスキルを読み込みます。

GitHubを追跡する登録・更新方法は[Codexのプラグイン資料](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli)を参照してください。

## Toolport経由で使う場合

MCPだけをToolportへ登録できます。PlumivoxはToolportの登録を自動では書き換えないため、導入方法に合わせて次のstdioサーバーを追加・変更します。開発用クローンや版番号付きのインストールキャッシュは指定しません。

- 名前: `plumivox`
- コマンド: WindowsのSetup版は`%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`（`%LOCALAPPDATA%`は実際のパスへ展開して指定。`current`は更新で中身が入れ替わってもパスが変わりません）。プラグイン版は`codex plugin marketplace list`で`plumivox-local`の取得済みルートを確認し、Windowsは`<取得済みルート>/dist/plumivox.exe`、macOSは`<取得済みルート>/dist/plumivox`
- 引数: `--stdio`
- 作業ディレクトリ: 利用者のホームディレクトリ
- 要求タイムアウト: 300000ミリ秒（ログインや中継の準備時間を含む）

ToolportでCodexが利用するプロファイルに有効化し、`toolport_search_tools`の`server="plumivox"`で11ツールを確認します。`list_accounts`を検索・呼出しし、設定が読み込めてから直接接続を無効にします。Setup版は、新規導入時にクライアントが既にToolportゲートウェイを使い、そのプロファイルで`plumivox`が有効なら、直接の登録を自動で省きます（`app-update.json`の`mcp_registration`で制御します。既定は`auto`、ほかに`direct`・`off`。詳細は[Toolport configuration](../configuration.md)）。Toolportを後から足した場合は、既存の直接登録を`codex mcp remove plumivox`と`claude mcp remove plumivox --scope user`で自分で外します。Setupが直接登録を外すことはありません（一度登録を済ませた後は、Setupで入れ直すまで自動では登録し直しません。後から戻すのはSetupが省いたクライアントだけで、たとえばToolportゲートウェイがクライアントから消えた場合に、次にPlumivoxが起動したときかサインイン時に直接登録します）。Windows以外では判定せず、常に直接登録します。プラグイン版はCodexのユーザー設定`config.toml`へ次を追加し、プラグイン本体は有効なままにして設定用スキルを残します。

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Codexを再起動すると直接接続が無効になります。以後の操作はToolportでツールを検索し、返された実ツール名を`toolport_call_tool`へ渡します。APIキー・Cookie・SSH鍵はPlumivoxの既存設定から読み込み、Toolportの引数へ移しません。

プラグイン版の更新は上記の`codex plugin marketplace upgrade plumivox-local`で行います。Toolportが実行中のサーバーは読込み済みコードを使うため、更新後にToolportの`plumivox`を無効化・再有効化して、新しい実行ファイルを読み込みます。取得済みルートが変わった場合は登録したコマンドも変更してください。起動時のGit取得や依存インストールは行いません。
