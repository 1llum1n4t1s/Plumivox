# 配布成果物の保存と取得

開発用の配布CIはWindows x64とmacOS universal（Apple Silicon・Intel）の成果物をCloudflare R2へ保存します。OSごとのビルド・検証結果を配布準備へ渡すための保管先です。利用者向けプラグインはGitHubの最新`main`に同梱した`dist`から起動し、R2の資格情報、Rust、Node.js、Wranglerを要求しません。

## 管理者の設定

専用の非公開バケット`plumivox-artifacts`を使用し、そのバケットに限定したObject Read & Write権限のアクセスキーを発行します。公開URLは不要です。GitHubリポジトリのSettings → Secrets and variables → Actionsへ次を設定します。

| 種別 | 名前 | 値 |
| --- | --- | --- |
| Variable | `PLUMIVOX_R2_ACCOUNT_ID` | CloudflareアカウントID（32桁） |
| Variable | `PLUMIVOX_R2_BUCKET` | バケット名 |
| Secret | `PLUMIVOX_R2_ACCESS_KEY_ID` | R2アクセスキーID |
| Secret | `PLUMIVOX_R2_SECRET_ACCESS_KEY` | R2シークレットアクセスキー |

キーをリポジトリ、コマンド引数、ログへ書かないでください。CIでは資格情報の実値をアップロードstepにだけ渡します。ビルド開始前はVariablesの形式とSecretsが設定済みかという真偽だけを確認し、実際の認証・権限はアップロード時に検証します。取得専用の端末には、同じバケットに限定したObject Read権限の別キーを推奨します。

## CIで保存する

GitHub Actionsの「Build distribution artifacts」を既定ブランチから手動実行します。元リポジトリ`1llum1n4t1s/Plumivox`の既定ブランチだけが対象で、PRやforkからR2へ書き込みません。CIは固定Git URLから対象のcommit SHAを取得・照合し、WindowsとmacOSの各runnerでRust/Cargo 1.99.1を使って次を実行します。Node製Actionは使いません。

1. 依存の固定取得、ネイティブビルド、`cargo xtask check`、`cargo xtask test-e2e`。
2. macOSは専用の一時KeychainでOSの保存機能を検証し、既定Keychainと検索一覧の不変を確認。終了時は成功・失敗を問わず専用Keychainを清掃。
3. fixture機能なしで正式成果物を再ビルド。macOSはarm64とx86_64の両ターゲットからuniversalを作り、両アーキテクチャを確認。
4. `cargo xtask artifact-upload`で成果物をR2へ保存。

保存先は`plumivox/ci/{commit}/{run}/{attempt}/{target}/`です。targetは`windows-x64`または`macos-universal`。実行ファイル（`plumivox.exe`または`plumivox`）と`THIRD-PARTY-LICENSES.txt`を保存し、identity・サイズ・SHA256を含む`manifest.json`を最後に保存します。条件付きPUTで既存オブジェクトへの再書込みを拒否します。途中で失敗した場合は同じidentityへ追記して直さず、GitHubの再実行で増えるattemptを使います。

## 検証して取得する

開発端末にはRust/Cargo 1.99.1とcurl（AWS SigV4対応）が必要です。上の4変数をプロセスの環境変数へ設定します。値は秘密ストアから取り込み、シェル履歴に直接書かないでください。

成功したジョブのSummaryに表示されるコマンドの`<new-directory>`を新しい出力先に置き換えます。次はPowerShellの例です。`<commit>`は40桁の小文字SHA、`<run>`と`<attempt>`はその実行の正整数です。

```powershell
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target windows-x64 --output D:\Artifacts\Plumivox\<run>-<attempt>-windows
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target macos-universal --output D:\Artifacts\Plumivox\<run>-<attempt>-macos
```

取得先は未存在のディレクトリにし、その親フォルダーは先に用意します。取得時はmanifestのidentity・ファイル名・サイズ・SHA256を検証します。SHA256は保存内容との一致確認で、独立したコード署名ではありません。失敗時は出力を配布に使わず、原因を確認してください。macOS上の取得では実行権限も設定します。

保存先には本人が管理する信頼できる階層を使います。Windowsは出力先とファイルを本人限定の権限で作り、ディレクトリハンドルを保持します。Unixは親の所有者・書込み権限・リンクを検査し、出力先を700で作成して、開いた同じファイルハンドルで実行権限を設定します。Unixの拡張ACLと、同一ユーザーによる悪意ある並行変更までは検査・防止しません。

取得成功だけではプラグインへ反映されません。配布準備では対象commitと各OSのCI結果を確認し、取得した実行ファイルとライセンスをリポジトリの`dist`へ反映してレビューします。コミット・pushは別の明示依頼に従い、利用者はGitHub mainの更新後に通常のプラグイン更新を行います。バージョン更新やリリース告知はこのCIに含めません。

## 保持と検証範囲

CIはlatestを更新せず、古い成果物を自動削除しません。管理者が必要なcommit・run・attemptを確認して保持し、不要と判断したprefixだけを手動で削除します。失敗したattemptの不完全なprefixも対象を照合してから清掃します。バケットのライフサイクルルールを追加する場合は、必要な配布候補を消さない保持期間を別途決めてください。

ローカルでは`cargo test --locked --package xtask`で入力・manifest・サイズ・ハッシュと、偽HTTPサーバーを使うcurl転送の条件を確認できます。実R2への認証・保存・取得とGitHub Actionsの全工程は実行結果で確認する必要があります。この構成の追加だけでは実クラウド・CIの実行確認は完了していません。
