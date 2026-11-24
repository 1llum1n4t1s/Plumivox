# フォロー中フィードの取得範囲

TwitterAPI.ioにX公式ホームの「フォロー中」を直接取得するAPIはありません。Plumivoxは既定ではフォロー一覧の先頭から最大100アカウントを読み、20アカウントずつまとめて公開投稿を検索します。`maxAccounts` は20〜200に設定できます。初回100なら最大6回、200なら最大11回のAPI要求です。最新の単価は[TwitterAPI.ioの料金](https://twitterapi.io/pricing)を確認してください。

結果の `officialHomeEquivalent` は常に `false` です。保護投稿は取得できず、検索で得られる内容・リポストの扱い・全体の並び順は公式ホームと一致する保証がありません。提供元に続きがある場合、または一意な非保護の検索対象が上限を超えた場合は `coverage.followingHasNextPage` が `true` になります。`coverage.followingNextCursor` を `followingCursor` に渡すと、次のフォロー先一覧を対象にします。フォロー一覧は新しいフォローから並び、継続中のフォロー変更による取得差は避けられません。

`nextCursor` を同じアカウントの `get_following_feed` の `cursor` に渡すと、各検索バッチの続きを取得します。ページ内は日時順ですが、バッチごとのページ境界によりページをまたぐ全体の時系列順は保証しません。フォロー先は初回の一覧を固定し、検索の続きでは再読込しません。フィードのcursorもAPIキーで署名し、検索対象の改変を拒否します。旧版の署名なしcursorや、APIキー変更前のcursorは使用できないため、cursorを外して取り直してください。`cursor` と `followingCursor` は別のページ送りで、同時に指定できません。

[フォロー一覧の公式応答仕様](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings)では保護状態のフィールドを確認できません。同じフォロー一覧の応答内では、有効なユーザー名を大文字小文字を区別せずに集約し、保護・公開・不明の順で判定します。保護と判明した対象を除いた初出順で上限まで選び、選択した対象のうち判定できない数を `coverage.protectionStatusUnknownAccounts` に返します。`protectedAccountsExcluded` は、その応答内で保護と判明した一意なアカウント数です。重複・無効なユーザー名は数えず、取得対象の上限外の保護アカウントも含めるため、`maxAccounts` を超える場合があります。0でも全対象が公開アカウントとは限りません。

旧実装が発行した署名済みcursorの続きは、検索対象・件数に加え、未検索範囲・継続可否も旧実装の判定を引き継ぐため、上限外の対象を飛ばす可能性があります。新しい保護状態の集約・件数計算・継続判定で確実に再取得するには、cursorを外してください。

投稿やフォロー一覧に続きがあっても、提供元cursorや組み合わせたcursorが長さ上限を超える場合は継続できません。取得済み投稿と `hasNextPage` は保持し、`nextCursor` は `null`、`coverage.tweetContinuationAvailable` は `false`、`tweetContinuationUnavailableReason` は `cursor_too_long` となります。フォロー一覧では `followingNextCursor` と `followingContinuationAvailable`・`followingContinuationUnavailableReason` を確認してください。cursorは切り詰めず、取得済み内容を失敗として捨てません。

提供元が1回の応答で `maxAccounts` を超える一意な非保護対象を返した場合は、上限外の対象が未検索です。`followingHasNextPage` は `true`、`followingContinuationUnavailableReason` は `account_limit` となり、フォロー一覧の継続は提供しません（`followingContinuationAvailable: false`、`followingNextCursor: null`）。提供元の次cursorで未検索対象を飛ばすことを防ぐためです。`maxAccounts` が200未満なら、200までの範囲で上限を増やし、cursorなしで再取得できます。上限200でも提供元が過剰返却する場合は、その応答の未検索対象をこの方法では取得できません。同名の重複行や保護・無効名だけで上限を超えても、この制限には該当しません。選択済み対象の投稿の続きは通常どおり取得でき、未検索対象がある状態も引き継ぎます。
