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
| 設定ファイル | `PLUMIVOX_SECRETS_FILE` → `createAccountStore({secretsPath})` → 存在する `~/dev/Secret/secrets.json` → `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` |
| アカウント | `getAccount(account)` の引数 → `PLUMIVOX_DEFAULT_ACCOUNT` → `createAccountStore({profile})` → `twitterapi_io.default_account` → 唯一のプロファイル |
| 複数アカウント | `default_account`に省略時の設定名を保存。別の設定を使う呼出しでは`account`を明示 |

`default_account`はXユーザー名ではなく、`main`・`blog_posting`など登録済みプロファイルの設定名です。初回セットアップは登録したアカウントを既定にします。既存利用者はセットアップの`-SetDefaultAccount blog_posting`で既定だけを変更できます。不正な型・空文字・未登録名の既定では、省略呼出しを`ACCOUNT_DEFAULT_INVALID`として通信前に拒否します。明示した`account`は既定より優先します。複数登録・既定なしの既存設定は`ACCOUNT_REQUIRED`を返し、先頭の登録先へ自動では送信しません。

`list_accounts`の各項目の`isDefault`は、環境変数などの優先設定も含めた有効な省略先だけで`true`になります。既定が未設定・不正でも一覧は取得でき、全項目が`false`になります。`default_account`自体はアカウントとして列挙しません。

設定ファイルの選択は起動時に固定し、各呼出しではそのファイルを読み直します。選択済みファイルが不正・削除された場合はエラーにし、別のファイルへ黙って切り替えません。`~`は現在の利用者のホームです。`LOCALAPPDATA`がない場合は`~/AppData/Local/Kagayoi/Plumivox/settings.json`を使います。Windows以外ではログインとセッション更新を利用できません。

ユーザー名は英数字と `_` の1～15文字です。読取りは APIキーとユーザー名だけで利用できます。投稿には加えて HTTP/HTTPS の有効なプロキシと v2 Cookie が必要です。`getAccount(account, {requirePosting:true})` は投稿設定が不足している場合に拒否します。Cookieの期限やAPIによる受入れはローカルの設定確認だけでは判定できません。

`list_accounts` はアカウント名、ユーザー名、読取り・投稿・ログインの準備状態、Cookieの取得元、取得日時だけを返します。取得元は `plumivox_dpapi`、`configuration`、`windows_dpapi`、`null` のいずれかです。APIキー、プロキシ、Cookie、パスワード、2FA秘密鍵、設定ファイルの内容を返しません。エラーも定型のメッセージとコードだけです。内部用のアカウント情報には秘密値を含むため、ツール応答やログへ出さないでください。

## 既存の Windows DPAPI Cookie

Plumivoxの新しい保存セッションを最優先で読みます。新しい保存先がまだ無い場合だけ、設定の `login_cookies`、既存のvava／blblの保存Cookieの順に読みます。旧保存先は初回のセッション更新を終えるまでの移行互換用途で、更新成功後は通常経路から外れます。既存のPlumivoxセッションが不正・設定不一致の場合は、旧Cookieへ黙って戻りません。

| プロファイル | 状態ディレクトリ |
| --- | --- |
| `release_posting` | `%LOCALAPPDATA%\Kagayoi\TwitterApiRelease` |
| `blog_posting` | `%LOCALAPPDATA%\Kagayoi\TwitterApiBlog` |
| その他 | `createAccountStore({sessionPaths:{profile_name:'状態ディレクトリの絶対パス'}})` で指定 |

旧ディレクトリの `SHA256(小文字のuser_name)\session.dpapi` はPowerShell 7 (`pwsh`) で読み取るだけで、上書きしません。セッションの `schema_version` は `2`、`user_name` は設定アカウントと大文字小文字を除き一致、`binding` は `SHA256(api_key + "\n" + proxy)` と一致している必要があります。旧形式、不一致、復号失敗、PowerShellがない環境ではCookieを使用しません。読取りは有効なAPIキーとユーザー名で継続でき、投稿は拒否します。

子プロセスは読取り専用です。入力は標準入力、復号結果は親プロセスの内部メモリだけで使用し、生エラーを応答へ転載しません。Windows 以外では DPAPI は使用できません。

## ログインとセッション更新

`login_account(account="release_posting")` は利用できる形式の保存Cookieがあれば再利用し、なければ[ログインAPI](https://docs.twitterapi.io/api-reference/endpoint/user_login_v2)を一回呼んで保存します。`refresh_session(account="release_posting")` は保存Cookieがあっても明示的に更新します。ブログ用は `account="blog_posting"` を指定します。

`totp_secret` は6桁の認証コードではなく、認証アプリに登録したBase32の秘密鍵です。2FA秘密鍵が不要なアカウントでは省略できます。資格情報は設定正本に置き、会話やMCP引数へ渡しません。

新セッションの保存先は `%LOCALAPPDATA%\Kagayoi\Plumivox\SHA256(account + "\n" + 小文字のuser_name)\session.dpapi`。必要ならリポジトリとCodexユーザーディレクトリの外にある絶対パスを `PLUMIVOX_STATE_DIR` で指定できます。保存ディレクトリは現在のWindowsユーザーだけにアクセスを許可し、CookieはDPAPIで暗号化します。ログイン要求の待機中に対象アカウント・APIキー・プロキシが変わった場合は保存を拒否します。

更新前の暗号化セッションは同じディレクトリに一意名の `session-<ID>.backup.dpapi` として保持します。新セッションの利用結果を確認した後、復元に必要な最小世代を残し、不要な世代を清掃してください。保存失敗時の `.pending` も暗号化済みで、原因確認・復元が終わるまで保持します。

同じ保存先・アカウントの更新はWindowsの名前付きMutexでAPI呼出前から保存完了まで排他し、複数チャットからの同時更新も拒否します。Mutexを所有するPowerShell子プロセスが終了すると、待機中のログインを中断し、その処理から保存しません。本体の置換も所有子プロセスで行い、置換前の失敗では既存セッションを維持します。置換が完了してから成功応答前に終了した場合は更新済みの可能性があり、旧セッションのバックアップを保持します。ログイン・更新だけでは投稿を実行せず、ログイン失敗や投稿失敗後の認証更新で元の要求を自動再送しません。

置換前の中止は `AUTH_CANCELLED`、Mutex所有喪失は `AUTH_LOCK_LOST`、保存に使えないCookieは `LOGIN_FAILED` で区別します。置換を指示した後に成功応答を確認できない場合は `AUTH_SAVE_UNCERTAIN` となり、保存内容が更新済みの可能性があります。これらを保存環境の故障として案内せず、保存先・DPAPIなどの障害だけを `SESSION_STORAGE_UNAVAILABLE` とします。

## プロキシの管理方式

アカウントの `proxy_mode` は省略時 `external` で、既存プロキシをそのまま使います。`managed` を指定したアカウントは、共通の `plumivox.managed_proxy` 設定から住宅回線出口とSSH中継を起動します。APIキー、ログインCookie、proxy URLの保存形式と優先順位は変わりません。

中継設定・初回準備・停止時の動作は [管理するプロキシ](managed-proxy.md) を参照してください。接続先やSSH鍵はMCP引数へ渡しません。`list_accounts` の `proxyMode` は設定の方式で、通信成功やX上の認証を保証しません。
