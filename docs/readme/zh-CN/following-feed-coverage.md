# 关注动态的获取范围

TwitterAPI.io 没有直接获取 X 官方首页“正在关注”的 API。Plumivox 默认从关注列表开头读取最多 100 个账号，每 20 个账号为一组搜索公开帖子。`maxAccounts` 可设为 20〜200。首次为 100 时最多 6 次 API 请求，为 200 时最多 11 次。最新单价请查看 [TwitterAPI.io 的价格](https://twitterapi.io/pricing)。

结果中的 `officialHomeEquivalent` 始终为 `false`。受保护的帖子无法获取，搜索得到的内容、转帖的处理方式以及整体排序都不保证与官方首页一致。当提供方还有后续数据，或唯一的非受保护搜索对象超过上限时，`coverage.followingHasNextPage` 为 `true`。把 `coverage.followingNextCursor` 传给 `followingCursor`，即可以下一批关注列表为对象。关注列表按最新关注排序，持续进行的关注变更所造成的获取差异无法避免。

把 `nextCursor` 传给同一账号的 `get_following_feed` 的 `cursor`，即可获取各搜索批次的后续内容。页面内按时间排序，但由于各批次的页面边界，不保证跨页的整体时间顺序。所关注账号固定使用首次获取的列表，在继续搜索时不会重新读取。动态的 cursor 也用 API 密钥签名，拒绝对搜索对象的篡改。旧版本的无签名 cursor 以及更换 API 密钥之前的 cursor 无法使用，请去掉 cursor 重新获取。`cursor` 和 `followingCursor` 是不同的翻页方式，不能同时指定。

在[关注列表的官方响应规范](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings)中无法确认受保护状态的字段。在同一个关注列表响应中，会对有效的用户名不区分大小写地进行汇总，并按受保护、公开、未知的顺序进行判定。排除已判明为受保护的对象后，按首次出现的顺序选取到上限为止，并在 `coverage.protectionStatusUnknownAccounts` 中返回所选对象中无法判定的数量。`protectedAccountsExcluded` 是该响应中已判明为受保护的唯一账号数。重复和无效的用户名不计入，同时也包括获取对象上限之外的受保护账号，因此可能超过 `maxAccounts`。即使为 0，也不代表所有对象都是公开账号。

旧实现签发的已签名 cursor 的后续获取，除了搜索对象和数量之外，还会沿用旧实现对未搜索范围和是否可继续的判定，因此可能会跳过上限之外的对象。要按新的受保护状态汇总、数量计算和继续判定可靠地重新获取，请去掉 cursor。

即使帖子或关注列表还有后续，如果提供方 cursor 或组合后的 cursor 超过长度上限，也无法继续。已获取的帖子和 `hasNextPage` 会保留，`nextCursor` 为 `null`，`coverage.tweetContinuationAvailable` 为 `false`，`tweetContinuationUnavailableReason` 为 `cursor_too_long`。对于关注列表，请查看 `followingNextCursor` 以及 `followingContinuationAvailable`、`followingContinuationUnavailableReason`。cursor 不会被截断，已获取的内容也不会被当作失败而丢弃。

如果提供方在一次响应中返回了超过 `maxAccounts` 的唯一非受保护对象，则上限之外的对象尚未搜索。此时 `followingHasNextPage` 为 `true`，`followingContinuationUnavailableReason` 为 `account_limit`，不提供关注列表的继续获取（`followingContinuationAvailable: false`、`followingNextCursor: null`）。这是为了防止通过提供方的下一个 cursor 跳过未搜索的对象。如果 `maxAccounts` 小于 200，可以在 200 以内提高上限，不带 cursor 重新获取。即使上限为 200，提供方仍过量返回时，该响应中未搜索的对象无法用此方法获取。仅因同名重复行或受保护、无效名称而超过上限时，不属于此限制。已选对象的帖子后续可以照常获取，存在未搜索对象的状态也会被沿用。
