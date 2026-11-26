# Error code troubleshooting

Values of the error fields described in [When posting or authentication fails](when-posting-or-authentication-fails.md).

## Image communication failures

`API_UNAVAILABLE` for image communication includes `failureKind`, distinguishing `timeout` (timed out), `network` (network failure), and `cancelled` (cancelled). In these cases as well, images and posts are not automatically resent.

## API rejections

API rejection errors (`API_REJECTED`, `API_HTTP_ERROR`) include `httpStatus`, an array of fixed categories `rejectionReasons`, and the categorization source `rejectionReasonSource`. This diagnostic information is kept even when a 5xx during post creation becomes `POST_UNCERTAIN`. The categorization source is `provider_message` (candidates inferred from the provider's JSON message) or `http_status` (categorization from the HTTP status). Candidates do not establish the cause or permit resending, and text that cannot be classified is `unknown`. Raw response text, arbitrary error codes, and request IDs are neither returned nor stored.

The `provider_message` candidates are `api_key`, `authentication`, `account_locked`, `account_suspended`, `rate_limit`, `credits`, `proxy`, `media`, `duplicate`, `upstream_timeout`, `upstream_unavailable`, `automation_suspected`, `daily_limit`, `text_too_long`, `session_mismatch`, `post_id_missing`, and `unknown`. The last five before `unknown` are candidates based on publicly known wording; the provider is not confirmed to return exactly that wording, and these candidates are not retried. `post_id_missing` means the post may have been created, so check the account before resending. `http_status` additionally has `invalid_request` (400), `unauthorized` (401), `forbidden` (403), and `endpoint_not_found` (404). For example, `rejectionReasons: ["proxy", "upstream_timeout"]` with `postAttempted: false` during image upload indicates a suspected proxy or upstream timeout and that it stopped before post creation. Non-2xx bodies are not read, and the existing communication limits and retry conditions are maintained.

## Text length and next step

`INVALID_TEXT` for text over the weighted limit of 280 includes `weightedLength`, `maxWeightedLength`, and `excessWeightedLength`. `API_REJECTED`, `API_HTTP_ERROR`, and `POST_UNCERTAIN` at the post creation stage include a fixed English `nextStep`. See [When posting or authentication fails](when-posting-or-authentication-fails.md) for details.

## Authentication errors

If aborted before the saved data is replaced, `AUTH_CANCELLED` is returned; if the exclusive lock for authentication refresh cannot be maintained, `AUTH_LOCK_LOST`; and cookies that cannot be saved, `LOGIN_FAILED`. If the result cannot be confirmed after the replacement of the saved data was instructed, `AUTH_SAVE_UNCERTAIN` is returned, and it may already have been updated. `SESSION_STORAGE_UNAVAILABLE` indicates a failure of the storage location or of the storage process, such as DPAPI on Windows or the Keychain on macOS.
