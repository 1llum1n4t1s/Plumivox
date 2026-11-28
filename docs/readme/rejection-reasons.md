# Rejection reasons

API rejection errors (`API_REJECTED`, `API_HTTP_ERROR`) carry `httpStatus`, an array of fixed categories `rejectionReasons`, and their source `rejectionReasonSource`. These fields are kept even when a 5xx during post creation becomes `POST_UNCERTAIN`. Categories are candidates only: they neither establish the cause nor permit resending.

## Sources

- `provider_message`: inferred from the provider's JSON message.
- `provider_code`: a numeric X error code or X HTTP status in the provider's body, alone or with the message.
- `http_status`: inferred from the HTTP status.

When a recognized X error code or X HTTP status is present, the error also has `xErrorCode` (the first recognized code) and `xHttpStatus`. Raw response text, unknown codes, and request IDs are never returned, and are stored only as redacted text in the [local rejection log](when-posting-or-authentication-fails.md#local-rejection-log).

## Categories

From messages: `api_key`, `authentication`, `account_locked`, `account_suspended`, `rate_limit`, `credits`, `proxy`, `media`, `duplicate`, `upstream_timeout`, `upstream_unavailable`, `automation_suspected`, `daily_limit`, `text_too_long`, `session_mismatch`, `post_id_missing`, and `unknown` for anything unclassified. `automation_suspected` through `post_id_missing` match publicly known wording that the provider is not confirmed to use; none of them is retried. `post_id_missing` means the post may exist, so check the account before resending.

From HTTP status only: `invalid_request` (400), `unauthorized` (401), `forbidden` (403), `endpoint_not_found` (404).

For example, `rejectionReasons: ["proxy", "upstream_timeout"]` with `postAttempted: false` during image upload means a suspected proxy or upstream timeout, stopped before post creation.

## X error codes

Codes in the body of a rejection returned with a 2xx status (top-level `code` or `errors[].code`):

| Code | Category |
| --- | --- |
| 226 | `automation_suspected` |
| 344, 185 | `daily_limit` |
| 187 | `duplicate` |
| 186 | `text_too_long` |
| 326 | `account_locked` |
| 64 | `account_suspended` |
| 88 | `rate_limit` |
| 32, 215 | `authentication` |
| 89, 353 | `session_mismatch` |
| 324 | `media` |

Only 226 is documented by the provider; the rest follow X's conventions. If nothing else matches, an `x_status` of 429 or 401 becomes `rate_limit` or `authentication`.

The body of a non-2xx response is never used for categorization, so communication limits and retry conditions are unchanged; it is read afterwards only for the redacted record while the local rejection log is on.
