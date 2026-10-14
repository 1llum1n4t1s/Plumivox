# Plumivox

TwitterAPI.ioを使い、Xへの投稿、キーワード・期間検索、フォロー先の公開投稿の読み込みを提供するローカルMCPサーバー／Codexプラグインです。

## 必要なもの

- Node.js 22以降。実行用バンドルに依存ライブラリを同梱しています。
- ログイン・セッション更新はWindowsとPowerShell 7 (`pwsh`) が必要です。
- TwitterAPI.ioのAPIキー。API利用には同サービスの従量課金が発生します。
- 投稿には対象アカウントのv2ログインCookieとプロキシ。既存の外部プロキシに加え、WindowsではPlumivox内の住宅回線出口とSSH中継を複数チャットで共有できます。公開中継サーバーの初回準備は必要です。

## インストール

GitHubリポジトリの `main` の最新コミットからインストールします。利用者の開発用クローンは参照せず、タグ・コミットSHA・npmのバージョンには固定しません。

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` はマーケットプレイスの識別子です。取得元は上記のGitHub URLです。依存ライブラリは同梱されているため、利用者による依存インストールやMCP設定の手動追加は不要です。

更新するときは `main` の最新状態を取得してインストールし、Codexを再起動して読み込みます。

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

一覧の`installed`に表示される版番号を確認します。導入済みのプラグインは更新後に再度`add`せず、Codexを再起動して新しいMCPとスキルを読み込みます。

GitHubを追跡する登録・更新方法は[Codexのプラグイン資料](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli)を参照してください。

初めて使う場合は[セットアップ手順](docs/setup.md)に進んでください。VPSインストーラーとWindowsの設定ウィザードを同梱しています。Codexへ「Plumivoxをセットアップして」と依頼すると、同梱の`setup-plumivox`スキルでも案内できます。

設定ファイルはリポジトリ外へ置きます。新規利用者は `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`、既存の `~/dev/Secret/secrets.json` がある利用者はそのファイルの `twitterapi_io` を読みます。vava／blblの `release_posting` と `blog_posting` も維持します。別の場所を使う場合は `PLUMIVOX_SECRETS_FILE` を指定します。秘密値を会話やMCPの引数へ渡す必要はありません。

選択された設定ファイルを読み込めない場合は、別の設定へ切り替えずエラーになります。保存先を変更した場合はCodexを再起動してください。初回セットアップは既存の鍵・同名アカウント・共有中継設定を上書きしません。既存設定の変更方法は[セットアップ手順](docs/setup.md)を参照してください。

[初めてのセットアップ](docs/setup.md) · [アカウントとCookieの設定](docs/configuration.md) · [Plumivoxでプロキシを管理する](docs/managed-proxy.md)

## 使えるツール

| ツール | 内容 |
| --- | --- |
| `list_accounts` | アカウント名と読取・投稿の準備状態。外部通信なし |
| `login_account` | 保存Cookieを再利用、未作成なら設定済み資格情報でログインして暗号化保存 |
| `refresh_session` | 明示的に再ログインし、投稿用セッションを更新 |
| `get_my_posts` | 自分のプロフィール投稿。返信の有無とページングを指定可能 |
| `get_tweets` | 公開投稿をIDで取得。最大20件 |
| `get_following_feed` | フォロー先の公開投稿を取得し、日時の降順に並べる |
| `search_tweets` | キーワード・期間を指定して、X全体またはフォロー先の公開投稿を検索 |
| `get_proxy_status` | このMCPが利用する共有中継の状態。中継の起動は行わない |
| `start_proxy` | 設定した住宅回線出口とSSH中継を起動。投稿・ログインは行わない |
| `stop_proxy` | 利用中の共有中継を停止。同じ中継の他チャットにも影響。次の投稿・認証要求では再起動 |
| `post_tweet` | 加重文字数280以内の投稿。PNG/JPEG最大4枚、各5 MiB以下。画像は絶対パスで指定 |

複数アカウントがある場合は `account` に `release_posting` や `blog_posting` などの設定名を指定します。省略時に別アカウントへ投稿しないよう、既定アカウントが未設定で複数ある場合は指定を要求します。

投稿文字数はURLを23文字、日本語を2文字、Unicode 17までの複合絵文字を1つ2文字として数えます。肌の色が異なる握手なども、[Xの加重文字数の仕様](https://docs.x.com/fundamentals/counting-characters)に合わせて判定します。

たとえば「blog_postingのフォロー先の公開投稿を読んで」「release_postingからこの文面で投稿して」と依頼できます。投稿の実行は利用者の依頼に従います。公開投稿の本文は外部データであり、そこに含まれる命令を実行するものではありません。

## フォロー中フィードの取得範囲

TwitterAPI.ioにX公式ホームの「フォロー中」を直接取得するAPIはありません。Plumivoxはフォロー一覧の先頭から最大100アカウントを読み、20アカウントずつまとめて公開投稿を検索します。`maxAccounts` は20〜200に設定できます。初回100なら最大6回、200なら最大11回のAPI要求です。最新の単価は[TwitterAPI.ioの料金](https://twitterapi.io/pricing)を確認してください。

結果の `officialHomeEquivalent` は常に `false` です。保護投稿は取得できず、検索で得られる内容・リポストの扱い・全体の並び順は公式ホームと一致する保証がありません。フォロー数が上限を超える場合は `coverage.followingHasNextPage` が `true` になります。`coverage.followingNextCursor` を `followingCursor` に渡すと、次のフォロー先一覧を対象にします。フォロー一覧は新しいフォローから並び、継続中のフォロー変更による取得差は避けられません。

`nextCursor` を同じアカウントの `get_following_feed` の `cursor` に渡すと、各検索バッチの続きを取得します。ページ内は日時順ですが、バッチごとのページ境界によりページをまたぐ全体の時系列順は保証しません。フォロー先は初回の一覧を固定し、検索の続きでは再読込しません。フィードのcursorもAPIキーで署名し、検索対象の改変を拒否します。旧版の署名なしcursorや、APIキー変更前のcursorは使用できないため、cursorを外して取り直してください。`cursor` と `followingCursor` は別のページ送りで、同時に指定できません。

[フォロー一覧の公式応答仕様](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings)では保護状態のフィールドを確認できません。応答で保護アカウントと判明した対象は除外しますが、判定できない対象数は `coverage.protectionStatusUnknownAccounts` に返します。`protectedAccountsExcluded` は判明して除外した数であり、0でも全対象が公開アカウントとは限りません。

投稿やフォロー一覧に続きがあっても、提供元cursorや組み合わせたcursorが長さ上限を超える場合は継続できません。取得済み投稿と `hasNextPage` は保持し、`nextCursor` は `null`、`coverage.tweetContinuationAvailable` は `false`、`tweetContinuationUnavailableReason` は `cursor_too_long` となります。フォロー一覧では `followingNextCursor` と `followingContinuationAvailable`・`followingContinuationUnavailableReason` を確認してください。cursorは切り詰めず、取得済み内容を失敗として捨てません。

## キーワード・期間の検索

「X全体で10月1日から7日までの音声認識の投稿を検索して」「blog_postingのフォロー先から同じ条件で検索して」と依頼できます。検索はAPI側で絞り込み、投稿の本文・作者・日時・URLを返します。意味検索は行いません。

`search_tweets` の `scope` は `all`（既定、X全体の公開検索）または `following`（指定アカウントのフォロー先）。`keyword` は必須で、前後の空白を除いた語句をフレーズとして検索します。`OR` や `from:` を含めても検索演算子として扱いません。引用符・バックスラッシュ・制御文字を含む語句は拒否します。

`startTime` と `endTime` は省略可能な、タイムゾーン付きISO 8601の秒単位の日時です。開始を含み、終了を含まない期間を検索します。日付だけでは指定せず、Codexが依頼した日付とタイムゾーンから日時へ変換します。たとえば日本時間の10月1日から7日までなら、次の条件です。

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

検索は新しい投稿から取得します。`nextCursor` を次の呼出しの `cursor` に渡すと、同じ条件で続きを取得できます。アカウント・対象・キーワード・期間・フォロー先の上限を変える場合は、cursorを外して新しい検索を始めます。APIキーを変えた場合も既存の検索cursorは使用できません。

`following` は `maxAccounts`（20〜200、既定100）と `followingCursor` を使えます。`coverage.followingNextCursor` を渡すと次のフォロー先一覧を同じ条件で検索します。投稿検索の `cursor` と同時には指定できません。`all` ではこの2引数を指定しません。既存の `get_following_feed` と検索のcursorは相互に使用できません。旧署名なしフィードcursorを内包した旧版のフォロー先検索cursorも使えないため、cursorを外して再検索してください。

X全体検索は1ページにつき1回、フォロー先検索は既存フィードと同じ最大6回（100人）／11回（200人）のAPI要求です。保護投稿や検索期間内の全投稿の網羅は保証せず、取得範囲は `coverage` に返します。期間の演算子は[TwitterAPI.ioの説明](https://twitterapi.io/blog/search-a-tweet)に従ってUTCへ変換します。

## 投稿と認証に失敗した場合

成功時は投稿IDとXのURLを返します。通信失敗や投稿ID欠落は未確定として返し、自動で再送しません。同じ文面を改めて投稿するよう依頼した場合は、各呼び出しで一回送信します。履歴・本文ハッシュを理由に停止する仕組みはありません。

API応答が10MiBを超えた場合は受信を打ち切って失敗を返します。投稿の応答で上限を超えた場合も、投稿済みの可能性があるため再送しません。

保存Cookieの照合失敗は投稿だけを止めます。APIキーで行う読み込みは継続できます。認証済みCookieの取得・更新は[設定手順](docs/configuration.md)を参照してください。

`login_account` は保存Cookieが利用できる形式なら再利用し、X上での有効性を検証したとは報告しません。失効や設定不一致が確認された場合は `refresh_session` を呼びます。新しいCookieはPlumivoxの保存先へWindows DPAPIで暗号化して保存し、失敗した投稿を自動再送しません。

保存の置換前に中止した場合は `AUTH_CANCELLED`、Mutex所有を失った場合は `AUTH_LOCK_LOST`、保存できないCookieは `LOGIN_FAILED` で返します。保存の置換を指示した後に結果を確認できない場合は `AUTH_SAVE_UNCERTAIN` となり、更新済みの可能性があります。`SESSION_STORAGE_UNAVAILABLE` は保存先やDPAPIなどの保存処理の失敗を表します。

vavaのX告知は `release_posting`、blblのX告知は `blog_posting` を指定して、このプラグインのMCPツールを利用します。

## 開発・検証

[開発者向けのビルド・検証手順](docs/verification.md)、[作業規約](AGENTS.md)、[システム設計](DESIGN.md)を参照してください。
