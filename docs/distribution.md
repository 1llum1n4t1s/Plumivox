# 配布成果物の保存と取得

開発用の配布CIはWindows x64、macOS ARM64（Apple Silicon）、Linux x86_64・aarch64の静的リンクVPSインストーラーをCloudflare R2へ保存します。OSごとのビルド・検証結果を配布準備へ渡すための保管先です。利用者向けプラグインはGitHubの最新`main`に同梱した`dist`から起動し、VPSインストーラーは同梱した`setup/vps`の実行物を使います。R2の資格情報、Rust、Node.js、Wrangler、起動時ダウンロードは要求しません。

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

1. 依存の固定取得、`cargo xtask check`、`cargo xtask test-e2e`。
2. macOSは専用の一時KeychainでOSの保存機能を検証し、既定Keychainと検索一覧の不変を確認。終了時は成功・失敗を問わず専用Keychainを清掃。
3. fixture機能なしで準備用成果物を再ビルド。Windowsは未署名で保存し、取得後に開発端末で署名・検証する。macOSはaarch64-apple-darwinを明示してビルドし、ARM64単独を確認。
4. `cargo xtask artifact-upload`で成果物をR2へ保存。

Linuxはx86_64とaarch64それぞれのnative runnerを使います。`sshd`・`ssh-keygen`を導入し、`cargo xtask ci-vps-test`でインストーラーのunit・CLIテストと実sshdを使うignoredテストを明示実行します。不足ツールはskipせず失敗させます。続いて各CPUのmuslターゲットをrust-lld・self-containedでビルドし、ELF64のCPU、動的ローダー・共有ライブラリ依存がないことと、生成した実行物の起動を確認してR2へ保存します。これは実VPSへの導入やsystemd・UFWの適用確認を代用しません。

Linux開発端末での単独ビルドは、対象のRust muslターゲットを導入したうえで`cargo xtask build-vps x86_64`または`cargo xtask build-vps aarch64`を使います。Cargoが返した実行物を検証して`setup/vps`へコピーするため、独自の`CARGO_TARGET_DIR`も使用できます。

保存先は`plumivox/ci/{commit}/{run}/{attempt}/{target}/`です。各targetの実行ファイルと`THIRD-PARTY-LICENSES.txt`の2件を保存し、identity・サイズ・SHA256を含む`manifest.json`を最後に保存します。

| target | 実行ファイル | 同梱先 |
| --- | --- | --- |
| `windows-x64` | `plumivox.exe` | `dist` |
| `macos-arm64` | `plumivox` | `dist` |
| `vps-linux-x86_64` | `plumivox-vps-setup-x86_64` | `setup/vps` |
| `vps-linux-aarch64` | `plumivox-vps-setup-aarch64` | `setup/vps` |

ライセンスの入力・同梱先は全targetで`dist`です。ファイル名と入力先は固定し、manifestに任意の相対パスを許可しません。条件付きPUTで既存オブジェクトへの再書込みを拒否します。途中で失敗した場合は同じidentityへ追記して直さず、GitHubの再実行で増えるattemptを使います。

## 検証して取得する

開発端末にはRust/Cargo 1.99.1とcurl（AWS SigV4対応）が必要です。上の4変数をプロセスの環境変数へ設定します。値は秘密ストアから取り込み、シェル履歴に直接書かないでください。

成功したジョブのSummaryに表示されるコマンドの`<new-directory>`を新しい出力先に置き換えます。次はPowerShellの例です。`<commit>`は40桁の小文字SHA、`<run>`と`<attempt>`はその実行の正整数です。

```powershell
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target windows-x64 --output D:\Artifacts\Plumivox\<run>-<attempt>-windows
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target macos-arm64 --output D:\Artifacts\Plumivox\<run>-<attempt>-macos
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target vps-linux-x86_64 --output D:\Artifacts\Plumivox\<run>-<attempt>-vps-x86_64
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target vps-linux-aarch64 --output D:\Artifacts\Plumivox\<run>-<attempt>-vps-aarch64
```

取得先は未存在のディレクトリにし、その親フォルダーは先に用意します。取得時はmanifestのidentity・ファイル名・サイズ・SHA256を検証します。SHA256は保存内容との一致確認で、独立したコード署名ではありません。失敗時は出力を配布に使わず、原因を確認してください。macOS上の取得では実行権限も設定します。

保存先には本人が管理する信頼できる階層を使います。Windowsは出力先とファイルを本人限定の権限で作り、ディレクトリハンドルを保持します。Unixは親の所有者・書込み権限・リンクを検査し、出力先を700で作成して、開いた同じファイルハンドルで実行権限を設定します。Unixの拡張ACLと、同一ユーザーによる悪意ある並行変更までは検査・防止しません。

取得成功だけではプラグインへ反映されません。配布準備では対象commitと各OS・CPUのCI結果を確認し、取得した実行ファイルを上表の同梱先へ、ライセンスを`dist`へ反映してレビューします。同じcommitのライセンス内容が全targetで一致することも確認します。コミット・pushは別の明示依頼に従い、利用者はGitHub mainの更新後に通常のプラグイン更新を行います。バージョン更新やリリース告知はこのCIに含めません。

## Windows配布物のコード署名

Windowsの`cargo xtask build`は、releaseビルド後にSimplySignへ自動接続し、本人のCertum証明書でAuthenticode署名とSHA256タイムスタンプを付けます。Windows SDKの最新バージョンのx64版`signtool.exe`を使い、信頼チェーン・発行者・タイムスタンプの検証が成功した場合だけ`dist/plumivox.exe`へ反映します。接続ヘルパー・SDK・証明書がない場合や認証・署名・検証の失敗はビルドエラーとし、既存の`dist/plumivox.exe`を保持します。未署名へ自動で切り替えません。

この開発端末では、他アプリと共通の`D:\Dev\skills\vava\scripts\Connect-SimplySign.ps1`で公式SimplySign Desktopへ自動接続します。別の配置では`PLUMIVOX_SIMPLYSIGN_CONNECT`へ同じ共通ヘルパーの絶対パスを指定します。登録情報はヘルパーだけが`D:\Dev\Secret\secrets.json`の`certum_simplysign`から読み、認証コードをログや引数へ出しません。Windowsの署名用開発端末にはPowerShell 7.4以降・公式SimplySign Desktop・Windows SDKと本人の有効な証明書が必要です。利用者側には追加の依存関係を要求しません。

`cargo xtask build-unsigned`はCIの検証・R2受渡し用にWindows署名を明示的に省くコマンドです。依存検証CIと配布CIは個人のSimplySign環境を持たないため、この経路を使います。macOSは従来どおりARM64実行物へadhoc署名します。R2から取得したWindows成果物を配布に使う場合は、最終配置前に開発端末で次を行います。SDKのx64版`signtool.exe`にPATHを通し、各コマンドが成功してから次へ進んでください。R2のmanifestは署名前の内容を示すため、署名後のハッシュは異なります。

```powershell
pwsh -NoProfile -File D:\Dev\skills\vava\scripts\Connect-SimplySign.ps1
# Windows SDKのx64版signtool.exeを使用。<thumbprint>は使用する証明書のSHA1拇印。
signtool sign /sha1 <thumbprint> /fd SHA256 /tr http://time.certum.pl /td SHA256 /d Plumivox D:\Dev\Plumivox\dist\plumivox.exe
signtool verify /pa /all /v D:\Dev\Plumivox\dist\plumivox.exe
```

各コマンドの終了コードを確認し、失敗した場合は配布へ進みません。Defenderは定義を更新し、`MpCmdRun.exe -Scan -ScanType 3 -File <実行ファイルの絶対パス> -DisableRemediation`で検査します。この個別検査は除外設定を無視し、検出ファイルの自動削除・隔離を行いません。署名は発行元と改変有無の確認に役立ちますが、将来の誤検出を防ぐ保証ではありません。検出が残る場合は保護設定を弱めず、本人の外部送信許可を得てMicrosoftの[ファイル提出窓口](https://www.microsoft.com/en-us/wdsi/filesubmission)へ再判定を依頼します。

## 保持と検証範囲

CIはlatestを更新せず、古い成果物を自動削除しません。管理者が必要なcommit・run・attemptを確認して保持し、不要と判断したprefixだけを手動で削除します。失敗したattemptの不完全なprefixも対象を照合してから清掃します。バケットのライフサイクルルールを追加する場合は、必要な配布候補を消さない保持期間を別途決めてください。

ローカルでは`cargo test --locked --package xtask`で入力・manifest・サイズ・ハッシュと、偽HTTPサーバーを使うcurl転送の条件を確認できます。実R2への認証・保存・取得とGitHub Actionsの全工程は実行結果で確認する必要があります。この構成の追加だけでは実クラウド・CIの実行確認は完了していません。

過去の`macos-universal` identityは既存成果物のダウンロードにのみ対応します。新規アップロードは`macos-arm64`を使用し、旧identityへのアップロードは拒否します。
