# Error code troubleshooting

Values of the error fields described in [When posting or authentication fails](when-posting-or-authentication-fails.md).

## Image communication failures

`API_UNAVAILABLE` for image communication includes `failureKind`, distinguishing `timeout` (timed out), `network` (network failure), and `cancelled` (cancelled). Images and posts are still not resent automatically.

## API rejections

API rejection errors (`API_REJECTED`, `API_HTTP_ERROR`) include `httpStatus`, an array of fixed categories `rejectionReasons`, and the categorization source `rejectionReasonSource`. This diagnostic information is kept even when a 5xx during post creation becomes `POST_UNCERTAIN`. The categorization source is `provider_message` (candidates inferred from the provider's JSON message), `provider_code` (a numeric X error code or X HTTP status in the provider's body, alone or together with the message), or `http_status` (categorization from the HTTP status). Candidates do not establish the cause or permit resending, and text that cannot be classified is `unknown`. When a recognized numeric X error code or an X HTTP status is present, the error also includes `xErrorCode` (the first recognized code) and `xHttpStatus`. Raw response text, codes outside the known table, and request IDs are not returned. By default they are not stored either; only if you enable the local rejection log is redacted text stored (see [Local rejection log](when-posting-or-authentication-fails.md#local-rejection-log)).

The `provider_message` candidates are `api_key`, `authentication`, `account_locked`, `account_suspended`, `rate_limit`, `credits`, `proxy`, `media`, `duplicate`, `upstream_timeout`, `upstream_unavailable`, `automation_suspected`, `daily_limit`, `text_too_long`, `session_mismatch`, `post_id_missing`, and `unknown`. The last five before `unknown` are candidates based on publicly known wording; the provider is not confirmed to return exactly that wording and they are not retried. `post_id_missing` means the post may have been created, so check the account before resending. `http_status` additionally has `invalid_request` (400), `unauthorized` (401), `forbidden` (403), and `endpoint_not_found` (404). For example, `rejectionReasons: ["proxy", "upstream_timeout"]` with `postAttempted: false` during image upload indicates a suspected proxy or upstream timeout and that it stopped before post creation. Numeric codes in the body of a rejection returned with a 2xx status (the top-level `code` or `errors[].code`) are categorized too: 226 `automation_suspected`; 344 and 185 `daily_limit`; 187 `duplicate`; 186 `text_too_long`; 326 `account_locked`; 64 `account_suspended`; 88 `rate_limit`; 32 and 215 `authentication`; 89 and 353 `session_mismatch`; 324 `media`. Of these, the provider documents only 226; the others follow X's conventions and it is not confirmed that they appear through the provider. If `x_status` is 429 or 401 and nothing else matches, it is categorized as `rate_limit` or `authentication`. The body of a non-2xx response is not used for categorization, so the existing communication limits and retry conditions are maintained; it is read afterwards, for the redacted record only, while the local rejection log is on.

## Text length and next step

Weighted length fields of `INVALID_TEXT` and the fixed `nextStep` are described in [Posting error fields](posting-error-fields.md).

## Authentication errors

If aborted before the saved data is replaced, `AUTH_CANCELLED` is returned; if the exclusive lock for authentication refresh cannot be maintained, `AUTH_LOCK_LOST`; and cookies that cannot be saved, `LOGIN_FAILED`. If the result cannot be confirmed after the replacement of the saved data was instructed, `AUTH_SAVE_UNCERTAIN` is returned, and it may already have been updated. `SESSION_STORAGE_UNAVAILABLE` indicates a failure of the storage location or of the storage process, such as DPAPI on Windows or the Keychain on macOS.
