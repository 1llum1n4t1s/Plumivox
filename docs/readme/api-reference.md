# Following feed pagination

Cursor rules and continuation fields of `get_following_feed`. For what the feed covers, see [Following feed coverage](following-feed-coverage.md).

## Continuing posts

Passing `nextCursor` to the `cursor` of `get_following_feed` for the same account fetches the continuation of each search batch. Posts within a page are in date order, but because of per-batch page boundaries, overall chronological order across pages is not guaranteed. The followed accounts are fixed from the initial list and are not reloaded when continuing a search. The feed cursor is also signed with the API key, and tampering with the search targets is rejected. Unsigned cursors from older versions and cursors from before an API key change cannot be used, so drop the cursor and fetch again. `cursor` and `followingCursor` are separate pagination mechanisms and cannot be specified together.

Continuations of signed cursors issued by the old implementation inherit the old implementation's judgment not only for search targets and counts but also for the unsearched range and whether continuation is possible, so targets beyond the limit may be skipped. To reliably refetch with the new protection status aggregation, count calculation, and continuation judgment, drop the cursor.

## Cursor length limit

Even if posts or the following list have more results, continuation is not possible when the provider cursor or the combined cursor exceeds the length limit. Already fetched posts and `hasNextPage` are kept, `nextCursor` is `null`, `coverage.tweetContinuationAvailable` is `false`, and `tweetContinuationUnavailableReason` is `cursor_too_long`. For the following list, check `followingNextCursor`, `followingContinuationAvailable`, and `followingContinuationUnavailableReason`. Cursors are not truncated, and already fetched content is not discarded as a failure.

## Account limit

If the provider returns more unique non-protected targets than `maxAccounts` in a single response, the targets beyond the limit are unsearched. `followingHasNextPage` is `true`, `followingContinuationUnavailableReason` is `account_limit`, and continuation of the following list is not offered (`followingContinuationAvailable: false`, `followingNextCursor: null`). If `maxAccounts` is below 200, you can raise the limit up to 200 and refetch without a cursor. If the provider over-returns even at the limit of 200, the unsearched targets in that response cannot be fetched this way. Exceeding the limit only through duplicate rows with the same name, protected accounts, or invalid names does not trigger this restriction. Continuations of posts for already selected targets can be fetched as usual, and the state of having unsearched targets is carried over as well.
