# 配布成果物の保存と取得

開発用の配布CIはWindows x64、macOS ARM64（Apple Silicon）、Linux x86_64・aarch64の静的リンクVPSインストーラーをCloudflare R2へ保存します。OSごとのビルド・検証結果を配布準備へ渡すための保管先です。WindowsのSetup版は別の公開バケットから配信し（[Setup（Velopack）の配信](#setupvelopackの配信)）、プラグイン版はGitHubの最新`main`に同梱した`dist`から起動し、VPSインストーラーは同梱した`setup/vps`の実行物を使います。R2の資格情報、Rust、Node.js、Wrangler、起動時ダウンロードは要求しません。

## 管理者の設定

専用の非公開バケット`plumivox-artifacts`を使用し、そのバケットに限定したObject Read & Write権限のアクセスキーを発行します。公開URLは不要です。GitHubリポジトリのSettings → Secrets and variables → Actionsへ次を設定します。

| 種別 | 名前 | 値 |
| --- | --- | --- |
| Variable | `PLUMIVOX_R2_ACCOUNT_ID` | CloudflareアカウントID（32桁） |
| Variable | `PLUMIVOX_R2_BUCKET` | バケット名 |
| Secret | `PLUMIVOX_R2_ACCESS_KEY_ID` | R2アクセスキーID |
| Secret | `PLUMIVOX_R2_SECRET_ACCESS_KEY` | R2シークレットアクセスキー |
| Environment `release`のSecret | `SIGILUNE_EMAIL`・`SIGILUNE_TOTP_URI` | Windows署名用のCertumアカウントとTOTP登録URI（承認者なし・`release/*`限定のEnvironmentに置く） |

キーをリポジトリ、コマンド引数、ログへ書かないでください。CIでは資格情報の実値をアップロードstepにだけ渡します。ビルド開始前はVariablesの形式とSecretsが設定済みかという真偽だけを確認し、実際の認証・権限はアップロード時に検証します。取得専用の端末には、同じバケットに限定したObject Read権限の別キーを推奨します。

## CIで保存する

GitHub Actionsの「Distribution」（`distribution.yml`）は、`release/<版>`ブランチへのpush（同じブランチでの手動再実行を含む）で起動します。元リポジトリ`1llum1n4t1s/Plumivox`だけが対象で、PRやforkからR2へ書き込みません。`release/<版>`のpushは公開操作なので、`vava`（版の更新・公開の依頼）でだけ行います。ワークフローは3つのjobで構成します。
- `unix`：macOS（GitHub提供のApple Siliconランナー）とLinux x86_64・aarch64。Environmentを付けず、署名用Secretsは届きません。
- `windows`：Windows x64。Environment `release`（`release/*`限定、承認待ちなし）で実行し、署名用Secretsはこのjobの署名stepだけへ渡します。
- `velopack`：`windows`の成功後に、Environment `r2-velopack`（`release/*`限定）でWindowsのSetupを梱包・署名して公開R2へ配信します（[Setup（Velopack）の配信](#setupvelopackの配信)）。

`main`の手動起動で動くのは`unix`のmacOSとLinuxだけで、非公開R2へ成果物を保存します（`windows`と`velopack`は`release/*`以外では動きません）。CIは固定Git URLから対象のcommit SHAを取得・照合し、各runnerでRust/Cargo 1.100.0を使って次を実行します。Node製Actionは使いません（Windowsの署名に使うSigiluneのactionはPowerShellのcomposite actionです）。

1. 依存の固定取得、`cargo xtask check`、`cargo xtask test-e2e`。
2. macOSは専用の一時KeychainでOSの保存機能を検証し、既定Keychainと検索一覧の不変を確認。終了時は成功・失敗を問わず専用Keychainを清掃。
3. fixture機能なしで準備用成果物を再ビルド。Windowsは続くstepでSigiluneが署名し、`cargo xtask ci-windows-verify-signature`が本人の署名・発行者・タイムスタンプを確認してから保存する（[コード署名](#windows配布物のコード署名)）。macOSはaarch64-apple-darwinを明示してビルドし、ARM64単独を確認。
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

開発端末にはRust/Cargo 1.100.0とcurl（AWS SigV4対応）が必要です。上の4変数をプロセスの環境変数へ設定します。値は秘密ストアから取り込み、シェル履歴に直接書かないでください。

成功したジョブのSummaryに表示されるコマンドの`<new-directory>`を新しい出力先に置き換えます。次はPowerShellの例です。`<commit>`は40桁の小文字SHA、`<run>`と`<attempt>`はその実行の正整数です。

```powershell
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target windows-x64 --output D:\Artifacts\Plumivox\<run>-<attempt>-windows
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target macos-arm64 --output D:\Artifacts\Plumivox\<run>-<attempt>-macos
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target vps-linux-x86_64 --output D:\Artifacts\Plumivox\<run>-<attempt>-vps-x86_64
cargo xtask artifact-download --commit <commit> --run <run> --attempt <attempt> --target vps-linux-aarch64 --output D:\Artifacts\Plumivox\<run>-<attempt>-vps-aarch64
```

取得先は未存在のディレクトリにし、その親フォルダーは先に用意します。取得時はmanifestのidentity・ファイル名・サイズ・SHA256を検証します。SHA256は保存内容との一致確認で、独立したコード署名ではありません。失敗時は出力を配布に使わず、原因を確認してください。macOS上の取得では実行権限も設定します。

保存先には本人が管理する信頼できる階層を使います。Windowsは出力先とファイルを本人限定の権限で作り、ディレクトリハンドルを保持します。Unixは親の所有者・書込み権限・リンクを検査し、出力先を700で作成して、開いた同じファイルハンドルで実行権限を設定します。Unixの拡張ACLと、同一ユーザーによる悪意ある並行変更までは検査・防止しません。

`main`の手動起動で取得できるのは`macos-arm64`と`vps-linux-x86_64`・`vps-linux-aarch64`です。`windows-x64`は`release/<版>`のpushで起動したCIだけが保存するため、`main`の配布準備ではWindowsの実行ファイルを手元で署名して作ります（`cargo xtask build`）。

取得成功だけではプラグインへ反映されません。配布準備では対象commitと各OS・CPUのCI結果を確認し、取得した実行ファイルを上表の同梱先へ、ライセンスを`dist`へ反映してレビューします。同じcommitのライセンス内容が全targetで一致することも確認します。コミット・pushは別の明示依頼に従い、利用者はGitHub mainの更新後に通常のプラグイン更新を行います。バージョン更新やリリース告知はこのCIに含めません。

## Windows配布物のコード署名

署名は[Sigilune](https://github.com/1llum1n4t1s/Sigilune)で行います。SigiluneはCertum SimplySignのクラウド鍵をsigntoolの`/dlib`として使うCLIで、SimplySign Desktopと手入力のワンタイムコードは不要です。本人のCertum証明書でAuthenticode署名とSHA256タイムスタンプを付け、Sigiluneの検証（有効性・署名者・タイムスタンプ）に加えて、xtaskが信頼チェーン・本人の共通名・Certumの発行者・タイムスタンプを独立に確認します。Sigiluneがない場合や認証・署名・検証の失敗はエラーとし、未署名へ自動で切り替えません。

**配布CI**：「Distribution」の`windows` jobは（署名を`unix` jobから別jobに分け、macOS・Linuxのrunnerへ署名用Secretsを届けません）、ビルド後に署名stepだけへEnvironment `release`のSecrets `SIGILUNE_EMAIL`・`SIGILUNE_TOTP_URI`を渡してSigiluneで`dist/plumivox.exe`へ署名し、Secretsを渡さないstepの`cargo xtask ci-windows-verify-signature`で確認してからR2へ保存します。SigiluneはCertumが同じTOTP時間枠の並行ログインを拒否するため、Setupの署名を行う`velopack` jobは`needs: windows`で直列にします。R2のmanifestは署名後の内容を示します。Cargoのビルド（依存crateのbuild script）には署名用Secretsを渡しません。署名の失敗を自動で再実行しません。

**開発端末**：`cargo xtask build`はreleaseビルド後にSigiluneでCargoの出力物へ署名・検証し、成功した場合だけ`dist/plumivox.exe`へ反映します。失敗時は既存の`dist/plumivox.exe`を保持します。事前にSigiluneを導入し、`sigilune setup --email <Certumアカウント>`で資格情報マネージャーへ登録します。実行ファイルは環境変数`SIGILUNE_EXE`→PATH→`%USERPROFILE%\.local\Sigilune\bin\sigilune.exe`の順に探します。Windows SDK（signtool）とPowerShell 7.4以降も必要です。利用者側には追加の依存関係を要求しません。

`cargo xtask build-unsigned`はWindows署名を明示的に省くコマンドで、依存検証CIなどの検証用です。macOSは従来どおりARM64実行物へadhoc署名します。R2から取得したWindows成果物は署名済みです。配布前に`sigilune verify --signer "Open Source Developer Yuichiro Shinozaki" <実行ファイルの絶対パス>`でも確認できます。

失敗した場合は配布へ進みません。Defenderは定義を更新し、`MpCmdRun.exe -Scan -ScanType 3 -File <実行ファイルの絶対パス> -DisableRemediation`で検査します。この個別検査は除外設定を無視し、検出ファイルの自動削除・隔離を行いません。署名は発行元と改変有無の確認に役立ちますが、将来の誤検出を防ぐ保証ではありません。検出が残る場合は保護設定を弱めず、本人の外部送信許可を得てMicrosoftの[ファイル提出窓口](https://www.microsoft.com/en-us/wdsi/filesubmission)へ再判定を依頼します。

## 保持と検証範囲

CIはlatestを更新せず、古い成果物を自動削除しません。管理者が必要なcommit・run・attemptを確認して保持し、不要と判断したprefixだけを手動で削除します。失敗したattemptの不完全なprefixも対象を照合してから清掃します。バケットのライフサイクルルールを追加する場合は、必要な配布候補を消さない保持期間を別途決めてください。

ローカルでは`cargo test --locked --package xtask`で入力・manifest・サイズ・ハッシュと、偽HTTPサーバーを使うcurl転送の条件を確認できます。実R2への認証・保存・取得とGitHub Actionsの全工程は実行結果で確認する必要があります。この構成の追加だけでは実クラウド・CIの実行確認は完了していません。

過去の`macos-universal` identityは既存成果物のダウンロードにのみ対応します。新規アップロードは`macos-arm64`を使用し、旧identityへのアップロードは拒否します。

## Setup（Velopack）の配信

WindowsのSetupは、上記の非公開成果物とは別の公開バケット`plumivox-updates`の`velopack/`から配信します（`https://plumivox-updates.kagayoi.com/velopack/`）。アプリはこの`releases.win.json`と`*-full.nupkg`を読んで自動更新します。macOS用の`releases.osx.json`はまだ配信しません。

### 管理者が用意するもの

| 項目 | 値 |
| --- | --- |
| R2バケット | `plumivox-updates`（公開。カスタムドメイン`plumivox-updates.kagayoi.com`を割り当てる） |
| アクセスキー | `plumivox-updates`だけに限定したObject Read & Write権限。成果物用`plumivox-artifacts`のキーと分ける |
| GitHub Environment | `r2-velopack`。Deployment branchesは`release/*`だけ（Selected branches）。承認者なし |
| `r2-velopack`のVariables | `PLUMIVOX_UPDATES_R2_ACCOUNT_ID`（32桁）、`PLUMIVOX_UPDATES_R2_BUCKET`（`plumivox-updates`） |
| `r2-velopack`のSecrets | `PLUMIVOX_UPDATES_R2_ACCESS_KEY_ID`、`PLUMIVOX_UPDATES_R2_SECRET_ACCESS_KEY`、`SIGILUNE_EMAIL`、`SIGILUNE_TOTP_URI` |

署名はWindows配布物と同じSigilune（本人のCertum証明書`Open Source Developer Yuichiro Shinozaki`）です。アプリは適用前にnupkg内の全PEの署名者をこのCNで照合するため、未署名・別の署名者の一式を配信すると、利用者の更新は適用前に拒否されます。

### CIで梱包・公開する

`release/<版>`ブランチ（Cargo.tomlの版と一致）へのpushで、「Distribution」の`velopack` jobが起動します。`windows` jobの成功後に実行されるため、`windows`が失敗すると配信されません（`workflow_run`は既定ブランチのrefで動き、`release/*`限定のEnvironmentに拒否されるため使いません）。公開済みの版を同じ`release/<版>`へ再pushすると、版付きの成果物の照合で失敗して公開バケットは何も変わりません。ジョブは次を順に行います。

1. 固定Git URLから対象commitを取得・照合し、Rust 1.100.0とVelopack CLI 1.2.161（`dotnet tool install --global vpk --version 1.2.161`）を用意する。
2. `cargo xtask ci-velopack-branch`でブランチ名と版の一致、`cargo xtask check`、`cargo xtask test-setup-velopack-e2e`（未署名のテスト用packIdで隔離E2E）。
3. 既定の機能だけで`plumivox.exe`をreleaseビルドし、`cargo xtask licenses`とxtask自体のビルドも済ませる。
4. 署名用Secretsを渡すstepでは**cargoを動かさず**、先にビルドしたxtaskを直接実行して`velopack-pack --sign --binary <plumivox.exe>`を行う（Sigiluneの署名テンプレートで全PEとSetupに署名し、Setupとnupkg内の全PEの署名者・Certumの発行者・タイムスタンプを検証する）。
5. Sigiluneからサインアウトし、R2のSecretsだけを渡すstepで`velopack-publish`を実行する。版付きの成果物（nupkg・版付きSetup）は`If-None-Match: *`の新規作成のみで、既存なら内容を照合して違えば失敗する。固定名（`PlumivoxApp-win-Setup.exe`・`assets.win.json`・`RELEASES`）は置き換え、`releases.win.json`を最後に置く。置いた全オブジェクトを読み直してSHA256を照合する。

### 手元で確認する

```powershell
cargo xtask velopack-pack --work $env:TEMP\plumivox-pack            # 未署名の本番packId（配信には使えない）
cargo xtask velopack-pack --work $env:TEMP\plumivox-pack --sign     # Sigilune で署名・検証
cargo xtask velopack-publish --packages $env:TEMP\plumivox-pack\packages --dry-run
cargo xtask test-setup-velopack-e2e
```

`--work`は空の既存フォルダー（リポジトリの外、リンクを含まない）にします。`--sign`は`--pack-id`・`--version`（隔離検証用）と併用できず、`update-feed-test`機能付きのビルドを`--binary`に渡すと拒否します。`velopack-publish`は`--dry-run`以外では`PLUMIVOX_UPDATES_R2_*`の4つの環境変数を要求し、バケット名が`plumivox-updates`でなければ書き込みません。実際の公開は明示の依頼があるときだけ行います。

`cargo xtask test-setup-velopack-e2e`はテスト用packId`PlumivoxVelopackTest`の9.0.0・9.0.1を未署名で梱包し、本物の`Setup.exe --silent --installto`で`PLUMIVOX_SETUP_VELOPACK_E2E_OUTPUT_DIR`（未指定時は一時フォルダーの`Codex/Plumivox/velopack-*`）の配下へ導入して、登録・更新の取得と適用・busy・未署名の拒否・アンインストールを確かめます。隔離の方法と項目は[検証方針](verification.md#setupvelopackの隔離e2e)を参照してください。ローカルの合格は、署名済みパッケージ・R2からの実取得・実際のClaude Code／Codex CLIでの動作を確かめたことになりません。
