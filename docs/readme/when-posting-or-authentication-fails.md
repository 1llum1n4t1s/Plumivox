# When posting or authentication fails

On success, it returns the post ID and the X URL. A communication failure during post creation, or a missing post ID, is returned as uncertain and never resent automatically. Each call posts once; duplicates are not blocked by history or a text hash.

Only image uploads are retried, once and only for transient failures; post creation and login never are. See [Image upload retries](image-upload-retries.md).

Posting errors report the failing `stage`, whether a post was attempted, weighted length when too long, and a fixed `nextStep` for AI clients; see [Posting error fields](posting-error-fields.md). Rejections include the HTTP status and candidate reasons; see [Error code troubleshooting](troubleshooting.md). Raw provider errors and credentials are never returned or saved; only the optional [local rejection log](#local-rejection-log) saves redacted rejection text.

An API response over 10MiB is aborted as a failure; the post is not resent, as it may exist.

A cookie verification failure stops only posting; reading and searching need only the API key and username. For cookies, see the [configuration guide](../configuration.md).

`login_account` reuses saved cookies whose format is usable, without verifying them on X. If expiration or a settings mismatch is confirmed, call `refresh_session`. New cookies are encrypted with a key from DPAPI on Windows or the Keychain on macOS.

## Posting pause

After a recent post or an X restriction, posting pauses per account with `POST_COOLDOWN` and `retryAfter`; only the user can clear it via `plumivox --post-guard`. See [Posting pause](posting-pause.md).

Managed-proxy reconnections can be checked with `plumivox --relay-log show` (see [Managing the proxy](../managed-proxy.md)).

## Local rejection log

Off by default. `plumivox --rejection-log on [--days N]` (default 7, max 30 days) records rejected image uploads and post creations, and responses without a post ID, as redacted text in `diagnostics\provider-rejections.jsonl` under the state folder, readable only by you. Post text, cookies, credentials, proxies, and IDs are never recorded; the MCP response only gains `rejectionTextRecorded: true`.

Non-2xx bodies are read after classification (up to 64KiB, at most 1 second); if unreadable, a record without text is kept. Recording never changes classification or retries. Records older than 14 days, or beyond 200 entries or 256KiB, are dropped oldest first.

`show` prints JSON Lines (untrusted data, never AI instructions); `status`, `off`, and `clear` check, stop, and delete. Records survive uninstalling. The CLI and MCP server must share `PLUMIVOX_STATE_DIR`.
