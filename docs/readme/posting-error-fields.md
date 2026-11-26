# Posting error fields

Known posting errors include `stage` (`input_validation`, `pre_send`, `image_upload`, `post_creation`), `postAttempted` (whether a post creation request was attempted), and `uploadAttempts` (total image upload requests). Missing accounts or cookies and relay preparation failures are `pre_send`. These fields are not returned for unknown internal errors or MCP input format rejections. `postAttempted: true` does not mean the post succeeded.

## Weighted text length

For text over the weighted limit of 280, `INVALID_TEXT` (`stage: input_validation`, `postAttempted: false`) also includes `weightedLength`, `maxWeightedLength` (280), and `excessWeightedLength`; not for empty text or invalid characters. Each URL counts as 23; characters up to U+10FF and dashes, curly quotes, and primes (U+2010-201F, U+2032-2037) as 1; other punctuation such as … and •, Japanese, Chinese, Korean, full-width punctuation, and emoji as 2. The `post_tweet` description gives the same rule and advises leaving a few minutes between posts to the same account; Plumivox enforces no cooldown.

## Next step

`API_REJECTED`, `API_HTTP_ERROR`, and `POST_UNCERTAIN` with `postAttempted: true` include `nextStep`, a fixed English sentence for AI clients:

- `post_id_missing` or `POST_UNCERTAIN`: the post may already exist; wait a few minutes, then check with `get_my_posts`.
- `automation_suspected`, `rate_limit`, or `daily_limit`: X may be restricting posting; tell the user, wait several minutes or more, and check with `get_my_posts`.
- Otherwise: check with `get_my_posts` before resending, and do not resend repeatedly.

Image upload, read, and login errors have no `nextStep`.
