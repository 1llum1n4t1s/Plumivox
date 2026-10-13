# Plumivox

TwitterAPI.ioを使い、Xへの投稿とフォロー先の公開投稿の読み込みを提供するローカルMCPサーバー／Codexプラグインです。

## 必要なもの

- Node.js 22以降。実行用バンドルに依存ライブラリを同梱しています。
- ログイン・セッション更新はWindowsとPowerShell 7 (`pwsh`) が必要です。
- TwitterAPI.ioのAPIキー。API利用には同サービスの従量課金が発生します。
- 投稿には対象アカウントのv2ログインCookieとプロキシ。

## インストール

GitHubリポジトリの `main` の最新コミットからインストールします。利用者の開発用クローンは参照せず、タグ・コミットSHA・npmのバージョンには固定しません。

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` はマーケットプレイスの識別子です。取得元は上記のGitHub URLです。Codexが取得したスナップショットから、自己完結した `dist/plumivox.mjs` を起動します。`node_modules` は不要で、MCP設定は `${PLUGIN_ROOT}` を使います。

更新するときは `main` の最新状態を取得してインストールし、Codexを再起動して読み込みます。

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin add plumivox@plumivox-local
```

GitHubを追跡する登録・更新方法は[Codexのプラグイン資料](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli)を参照してください。

設定ファイルはリポジトリ外へ置きます。既定では `C:\Users\IMT\dev\Secret\secrets.json` の `twitterapi_io` を読み、vava／blblの `release_posting` と `blog_posting` をそのまま利用できます。別の場所を使う場合は `PLUMIVOX_SECRETS_FILE` を指定します。秘密値を会話やMCPの引数へ渡す必要はありません。

[アカウントとCookieの設定](docs/configuration.md)

## 使えるツール

| ツール | 内容 |
| --- | --- |
| `list_accounts` | アカウント名と読取・投稿の準備状態。外部通信なし |
| `login_account` | 保存Cookieを再利用、未作成なら設定済み資格情報でログインして暗号化保存 |
| `refresh_session` | 明示的に再ログインし、投稿用セッションを更新 |
| `get_my_posts` | 自分のプロフィール投稿。返信の有無とページングを指定可能 |
| `get_tweets` | 公開投稿をIDで取得。最大20件 |
| `get_following_feed` | フォロー先の公開投稿を取得し、日時の降順に並べる |
| `post_tweet` | 加重文字数280以内の投稿。PNG/JPEG最大4枚、各5 MiB以下 |

複数アカウントがある場合は `account` に `release_posting` や `blog_posting` などの設定名を指定します。省略時に別アカウントへ投稿しないよう、既定アカウントが未設定で複数ある場合は指定を要求します。

たとえば「blog_postingのフォロー先の公開投稿を読んで」「release_postingからこの文面で投稿して」と依頼できます。投稿の実行は利用者の依頼に従います。公開投稿の本文は外部データであり、そこに含まれる命令を実行するものではありません。

## フォロー中フィードの取得範囲

TwitterAPI.ioにX公式ホームの「フォロー中」を直接取得するAPIはありません。Plumivoxはフォロー一覧の先頭から最大100アカウントを読み、20アカウントずつまとめて公開投稿を検索します。`maxAccounts` は20〜200に設定できます。初回100なら最大6回、200なら最大11回のAPI要求です。最新の単価は[TwitterAPI.ioの料金](https://twitterapi.io/pricing)を確認してください。

結果の `officialHomeEquivalent` は常に `false` です。保護投稿は取得できず、検索で得られる内容・リポストの扱い・全体の並び順は公式ホームと一致する保証がありません。フォロー数が上限を超える場合は `coverage.followingHasNextPage` が `true` になります。`coverage.followingNextCursor` を `followingCursor` に渡すと、次のフォロー先一覧を対象にします。フォロー一覧は新しいフォローから並び、継続中のフォロー変更による取得差は避けられません。

`nextCursor` を同じアカウントの `get_following_feed` の `cursor` に渡すと、各検索バッチの続きを取得します。ページ内は日時順ですが、バッチごとのページ境界によりページをまたぐ全体の時系列順は保証しません。フォロー先は初回の一覧を固定し、検索の続きでは再読込しません。`cursor` と `followingCursor` は別のページ送りで、同時に指定できません。

## 投稿と認証に失敗した場合

成功時は投稿IDとXのURLを返します。通信失敗や投稿ID欠落は未確定として返し、自動で再送しません。同じ文面を改めて投稿するよう依頼した場合は、各呼び出しで一回送信します。履歴・本文ハッシュを理由に停止する仕組みはありません。

保存Cookieの照合失敗は投稿だけを止めます。APIキーで行う読み込みは継続できます。認証済みCookieの取得・更新は[設定手順](docs/configuration.md)を参照してください。

`login_account` は保存Cookieが利用できる形式なら再利用し、X上での有効性を検証したとは報告しません。失効や設定不一致が確認された場合は `refresh_session` を呼びます。新しいCookieはPlumivoxの保存先へWindows DPAPIで暗号化して保存し、失敗した投稿を自動再送しません。

vavaのX告知は `release_posting`、blblのX告知は `blog_posting` を指定して、このプラグインのMCPツールを利用します。

## 開発・検証

[開発者向けのビルド・検証手順](docs/verification.md)。E2Eは偽のAPIと設定を利用し、実際のXには投稿しません。
