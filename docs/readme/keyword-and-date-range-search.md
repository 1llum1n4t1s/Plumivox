# Keyword and date-range search

You can ask "search all of X for posts about speech recognition from October 1 to 7" or "search the accounts blog_posting follows with the same conditions". Search is filtered on the API side and returns each post's text, author, date and time, and URL. Semantic search is not performed.

The `scope` of `search_tweets` is `all` (default, public search across all of X) or `following` (accounts followed by the specified account). `keyword` is required, and the phrase with leading and trailing whitespace removed is searched as a phrase. Including `OR` or `from:` does not make them search operators. Phrases containing quotation marks, backslashes, or control characters are rejected.

`startTime` and `endTime` are optional ISO 8601 date-times with a time zone, at second precision. The search covers a period that includes the start and excludes the end. Do not specify dates alone; Codex converts the requested dates and time zone into date-times. For example, October 1 to 7 in Japan time is the following condition.

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

Search fetches from the newest posts. Passing `nextCursor` to the `cursor` of the next call fetches the continuation with the same conditions. To change the account, scope, keyword, period, or following limit, drop the cursor and start a new search. Existing search cursors also cannot be used after the API key changes.

`following` can use `maxAccounts` (20 to 200, default 100) and `followingCursor`. Passing `coverage.followingNextCursor` searches the next page of the following list with the same conditions. It cannot be specified together with the post search `cursor`. Do not specify these two arguments with `all`. Cursors from the existing `get_following_feed` and from search cannot be used interchangeably. Following search cursors from older versions that embed old unsigned feed cursors also cannot be used, so drop the cursor and search again.

A search across all of X makes 1 API request per page, and a following search makes the same number as the existing feed: up to 6 (100 accounts) / 11 (200 accounts). Protected posts and complete coverage of all posts within the search period are not guaranteed, and the fetched range is returned in `coverage`. The period operators are converted to UTC according to [TwitterAPI.io's description](https://twitterapi.io/blog/search-a-tweet).
