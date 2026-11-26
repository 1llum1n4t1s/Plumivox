# アカウント設定

Plumivox は設定ファイルの `twitterapi_io` 以下にある各プロファイルを読みます。新規利用者は[設定ウィザード](setup.md)を利用できます。環境変数へ秘密値を設定する必要はありません。読み込みには `api_key` と `user_name`、投稿には加えて `proxy` とログインCookieを使います。ログイン・セッション更新では `email`、`password`、必要に応じて `totp_secret` をサーバー内部で読みます。

アカウント引数には `list_accounts` が返す設定名をそのまま指定できます。設定名に64文字の上限は設けません。設定ファイル全体の上限は2MiBです。

```json
{
  "twitterapi_io": {
    "default_account": "release_posting",
    "release_posting": {
      "api_key": "YOUR_API_KEY",
      "user_name": "your_account",
      "proxy": "http://user:password@proxy.example:8080",
      "email": "your-account@example.com",
      "password": "YOUR_X_PASSWORD",
      "totp_secret": "YOUR_BASE32_2FA_SECRET"
    }
  }
}
```

この例は説明用です。秘密値をリポジトリへ保存しないでください。

| 設定 | 解決方法 |
| --- | --- |
| 設定ファイル | `PLUMIVOX_SECRETS_FILE` → 内部起動設定の`secrets_path` → 存在する`~/dev/Secret/secrets.json` → OS別の標準保存先 |
| アカウント | ツールの`account`引数 → `PLUMIVOX_DEFAULT_ACCOUNT` → 内部起動設定の`profile` → `twitterapi_io.default_account` → 唯一のプロファイル |
| 複数アカウント | `default_account`に省略時の設定名を保存。別の設定を使う呼出しでは`account`を明示 |
| 表示言語 | `PLUMIVOX_LANGUAGE` → `LC_ALL` → `LC_MESSAGES` → `LANG`（空・`C`・`POSIX`は飛ばす）→ OSの表示言語 → 英語。対応はja・en・zh-CN・zh-TW・ko・es・fr・de・pt-BR。対象はCLIの使い方・ヘルプ、`--setup`の質問・表示・エラー、Setup版の案内、`--uninstall-setup`の説明。MCPのツール定義と応答（エラー文を含む）は言語によらず英語 |

`default_account`はXユーザー名ではなく、`main`・`blog_posting`など登録済みプロファイルの設定名です。初回セットアップは登録したアカウントを既定にします。既存利用者は実行ファイルの`--setup --set-default-account blog_posting`で既定だけを変更できます。不正な型・空文字・未登録名の既定では、省略呼出しを`ACCOUNT_DEFAULT_INVALID`として通信前に拒否します。明示した`account`は既定より優先します。複数登録・既定なしの既存設定は`ACCOUNT_REQUIRED`を返し、先頭の登録先へ自動では送信しません。

`list_accounts`の各項目の`isDefault`は、環境変数などの優先設定も含めた有効な省略先だけで`true`になります。既定が未設定・不正でも一覧は取得でき、全項目が`false`になります。`default_account`自体はアカウントとして列挙しません。

設定ファイルの選択は起動時に固定し、各呼出しではそのファイルを読み直します。選択済みファイルが不正・削除された場合はエラーにし、別のファイルへ黙って切り替えません。`~`は現在の利用者のホームです。標準保存先はWindowsでは`%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`（`LOCALAPPDATA`がない場合は`~/AppData/Local/Kagayoi/Plumivox/settings.json`）、macOSでは`~/Library/Application Support/Kagayoi/Plumivox/settings.json`です。セットアップも同じ選択順を使います。

ユーザー名は英数字と `_` の1～15文字です。読取りはAPIキーとユーザー名だけで利用できます。投稿には加えてHTTP/HTTPSの有効なプロキシとv2 Cookieが必要です。投稿設定が不足している場合は通信前に拒否します。Cookieの期限やAPIによる受入れはローカルの設定確認だけでは判定できません。

`list_accounts` はアカウント名、ユーザー名、読取り・投稿・ログインの準備状態、Cookieの取得元、取得日時だけを返します。取得元は`plumivox_dpapi`、`plumivox_keychain`、`configuration`、`windows_dpapi`、`null`のいずれかです。APIキー、プロキシ、Cookie、パスワード、2FA秘密鍵、設定ファイルの内容を返しません。エラーも定型のメッセージとコードだけです。内部用のアカウント情報には秘密値を含むため、ツール応答やログへ出さないでください。

## 既存の Windows DPAPI Cookie

Plumivoxの新しい保存セッションを最優先で読みます。新しい保存先がまだ無い場合だけ、設定の `login_cookies`、既存のvava／blblの保存Cookieの順に読みます。旧保存先は初回のセッション更新を終えるまでの移行互換用途で、更新成功後は通常経路から外れます。既存のPlumivoxセッションが不正・設定不一致の場合は、旧Cookieへ黙って戻りません。

| プロファイル | 状態ディレクトリ |
| --- | --- |
| `release_posting` | `%LOCALAPPDATA%\Kagayoi\TwitterApiRelease` |
| `blog_posting` | `%LOCALAPPDATA%\Kagayoi\TwitterApiBlog` |
| その他 | 旧保存Cookieを読みません |

旧ディレクトリの`SHA256(小文字のuser_name)\session.dpapi`はWindowsのネイティブDPAPIで読み取るだけで、上書きしません。セッションの`schema_version`は`2`、`user_name`は設定アカウントと大文字小文字を除き一致、`binding`は`SHA256(api_key + "\n" + proxy)`と一致している必要があります。旧形式、不一致、復号失敗ではCookieを使用しません。読取りは有効なAPIキーとユーザー名で継続でき、投稿は拒否します。

復号結果は本体の内部メモリだけで使用し、生エラーを応答へ転載しません。初期設定・認証更新・DPAPI読取りは本体で処理します。macOSではWindowsのDPAPI保存Cookieを読み込めないため、設定済みCookieを使うか、`login_account`でmacOS用のセッションを作成します。

## ログインとセッション更新

`login_account(account="release_posting")` は利用できる形式の保存Cookieがあれば再利用し、なければ[ログインAPI](https://docs.twitterapi.io/api-reference/endpoint/user_login_v2)を一回呼んで保存します。`refresh_session(account="release_posting")` は保存Cookieがあっても明示的に更新します。ブログ用は `account="blog_posting"` を指定します。

`totp_secret` は6桁の認証コードではなく、認証アプリに登録したBase32の秘密鍵です。2FA秘密鍵が不要なアカウントでは省略できます。資格情報は設定正本に置き、会話やMCP引数へ渡しません。

保存先はOS別の状態ディレクトリの`SHA256(account + "\n" + 小文字のuser_name)`配下です。

| OS | 標準の状態ディレクトリ | セッションの保護 |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\Kagayoi\Plumivox` | 本人限定ACLとCurrentUser DPAPI。ファイル名は`session.dpapi`で、従来の保存形式と互換 |
| macOS | `~/Library/Application Support/Kagayoi/Plumivox` | 本人所有の700ディレクトリ・600ファイルとAES-256-GCM。ファイル名は`session.keychain`。暗号鍵はログイン利用者のKeychainに保存 |

状態ディレクトリは`PLUMIVOX_STATE_DIR`、内部起動設定の`state_directory`、OS標準の順に選びます。変更する場合はリポジトリとCodex・Claude Codeなどのユーザー設定ディレクトリ（`~/.codex`・`~/.claude`、`CODEX_HOME`・`CLAUDE_CONFIG_DIR`の指定先）の外にある絶対パスを指定します。macOSのKeychain項目のサービス名は`com.kagayoi.plumivox.session.aes256gcm.v1`で、正規化した状態ディレクトリの絶対パスから項目を分けます。暗号化ファイルだけを別のMac・利用者・保存先へコピーしても利用できません。Keychainを利用できない場合は平文保存へ切り替えません。ログイン要求の待機中に対象アカウント・APIキー・プロキシが変わった場合は保存を拒否します。

更新前の暗号化セッションは同じディレクトリに一意名の`session-<ID>.backup.dpapi`（Windows）または`session-<ID>.backup.keychain`（macOS）として保持します。新セッションの利用結果を確認した後、復元に必要な最小世代を残し、不要な世代を清掃してください。新しい暗号文の全量書込みと同期が完了し、その後の保存処理が失敗して`session-<ID>.pending`が残っている場合は、原因調査用に保持します。部分書込み、明示取消、ロック所有喪失で中止したファイルは削除を試みます。ただし、異常終了や削除失敗によって不完全な`.pending`や`.backup.*`が残る可能性があり、名前だけで完全性は保証できません。これらの完全性を変更なしで確認する公開コマンドはありません。自動で復元せず、既存セッションと候補ファイルを保持して原因を調査してください。置換結果が不明な場合は、既存セッションが未変更とは断定できません。

同じ保存先・アカウントの更新は、Windowsの名前付きMutex、macOSの`.auth.lock`のファイルロックでAPI呼出前から保存完了まで排他し、複数チャットからの同時更新を拒否します。WindowsではMutexを所有するネイティブスレッドが置換も担当します。ロックの所有確認と保存直前の設定再確認を行い、置換前の失敗では既存セッションを維持します。置換が完了してから成功応答前に終了した場合は更新済みの可能性があり、旧セッションのバックアップを保持します。ログイン・更新だけでは投稿を実行せず、ログイン失敗や投稿失敗後の認証更新で元の要求を自動再送しません。

置換前の中止は`AUTH_CANCELLED`、ロック所有喪失は`AUTH_LOCK_LOST`、保存に使えないCookieは`LOGIN_FAILED`で区別します。置換を指示した後に成功応答を確認できない場合は`AUTH_SAVE_UNCERTAIN`となり、保存内容が更新済みの可能性があります。これらを保存環境の故障として案内せず、保存先・DPAPI・Keychainなどの障害だけを`SESSION_STORAGE_UNAVAILABLE`とします。

## プロキシの管理方式

アカウントの `proxy_mode` は省略時 `external` で、既存プロキシをそのまま使います。`managed` を指定したアカウントは、共通の `plumivox.managed_proxy` 設定から住宅回線出口とSSH中継を起動します。APIキー、ログインCookie、proxy URLの保存形式と優先順位は変わりません。

中継設定・初回準備・停止時の動作は [管理するプロキシ](managed-proxy.md) を参照してください。接続先やSSH鍵はMCP引数へ渡しません。`list_accounts` の `proxyMode` は設定の方式で、通信成功やX上の認証を保証しません。

## Setup版の自動更新の設定

WindowsのSetup版は、データフォルダーの子`%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\`に更新の状態を置きます（macOSのSetup版は`~/Library/Application Support/Kagayoi/Plumivox/app-update/`の予定）。秘密値は含みません。

| ファイル | 内容 |
| --- | --- |
| `app-update.json` | `auto_update`（既定`true`。`false`で確認・取得・適用を止める）と、前回の確認・失敗の時刻（UNIX秒）。壊れた値は項目ごとに既定へ戻す |
| `app-update.log` | 確認・取得・適用・登録の結果の短い記録（64KiBを超えると作り直す） |
| `setup-registration.json` | 登録した実行ファイル・版・登録を済ませたCLI（`claude`・`codex`）・ログオン時の適用の登録の有無 |
| `app-update.lock`・`setup-registration.lock` | 複数のPlumivoxが同時に確認・登録しないための排他 |

確認は前回の確認（成功・失敗とも）から24時間たってから行います。利用者が足したキーは時刻の更新で消しません。アンインストールしてもこのフォルダーと設定・セッションは残ります。

## 拒否文面のローカル記録の設定

既定では、提供元の拒否文面を返さず、保存もしません。利用者が`plumivox --rejection-log on [--days N]`で期限付きに有効にしたときだけ、伏字処理した拒否文面を、本人だけが読めるローカルファイルへ記録します。原因の調査に使うための機能で、MCPの応答には文面を出しません。

状態ルートの子`diagnostics\`に置きます。状態ルートはWindowsの既定が`%LOCALAPPDATA%\Kagayoi\Plumivox\`、macOSの既定が`~/Library/Application Support/Kagayoi/Plumivox/`で、`PLUMIVOX_STATE_DIR`があればそれに従います。

| ファイル | 内容 |
| --- | --- |
| `rejection-log.json` | 有効化の状態（期限と有効にした時刻。UNIX秒）。読めない・不正な値は無効として扱う |
| `provider-rejections.jsonl` | 伏字処理した拒否文面の記録（JSON Lines、1行1件） |
| `provider-rejections.lock` | 複数のPlumivoxが同時に書かないための排他 |

- 有効化と期限: 既定は無効です。`--rejection-log on`の期限は既定7日、`--days N`で1〜30日を指定できます。期限が切れると自動で記録を止めます。`off`で止め（記録は残す）、`status`で状態を確認します。
- 記録する場面: `post_tweet`の画像アップロードと投稿作成の拒否（`API_REJECTED`）、および投稿IDが返らない場合（`POST_UNCERTAIN`）だけです。読取り・ログインは記録しません。MCPの応答に`rejectionTextRecorded: true`を足すのは、返す最終エラーそのもの（同じ段階・同じ要求）を記録できたときだけです。
- 残さないもの: 本文・Cookie・資格情報・プロキシ・IDは残しません。記録する`account`は設定名だけで、Xユーザー名と同じ（大文字小文字は無視）か、伏せる値を含む設定名は`<user>`に置き換えます。URL・メール・トークン様の文字列などは伏字にします。
- 上限: 1件は14日、全体は200件・256KiBまでで、超えたら古いものから消します。
- 閲覧: `--rejection-log show`だけがJSON Linesで標準出力へ出します。出力は提供元由来の信頼できないデータなので、AIへ渡すときは指示として扱わせないでください。
- `PLUMIVOX_STATE_DIR`の注意: CLI（`--rejection-log`）とMCPサーバーで`PLUMIVOX_STATE_DIR`が違うと、保存先がずれてCLIから記録が見えません。同じ値で起動してください。
- アンインストール後: 記録はアンインストールしても残ります。14日で消えるか、`plumivox --rejection-log clear`で消します。
