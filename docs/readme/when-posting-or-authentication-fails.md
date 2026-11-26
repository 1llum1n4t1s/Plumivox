# When posting or authentication fails

On success, it returns the post ID and the X URL. A communication failure while creating a post or a missing post ID is returned as uncertain and is not automatically resent. Asking to post the same text again sends it once per call; nothing stops duplicates based on history or a text hash.

Only image uploads are retried, once and only for transient failures; post creation and login never are. See [Image upload retries](image-upload-retries.md).

Posting errors report the failing `stage`, whether a post was attempted, weighted text length when too long, and a fixed `nextStep` for AI clients. See [Posting error fields](posting-error-fields.md). Rejections include the HTTP status and categorized candidate reasons; see [Error code troubleshooting](troubleshooting.md). Raw provider errors and credentials are never returned. By default nothing is saved; only if you enable the [local rejection log](#local-rejection-log) is redacted rejection text saved. Credentials are never saved.

If an API response exceeds 10MiB, receiving is aborted and a failure is returned; a post is not resent even then, because it may already exist.

A saved cookie verification failure stops only posting. Reading and searching public posts use only the API key and username. For obtaining and refreshing cookies, see the [configuration guide](../configuration.md).

`login_account` reuses saved cookies if their format is usable, without verifying them on X. If expiration or a settings mismatch is confirmed, call `refresh_session`. New cookies are saved encrypted with a key from DPAPI on Windows or the Keychain on macOS.

## Local rejection log

Off by default. To investigate rejections, run `plumivox --rejection-log on [--days N]` (default 7 days, up to 30; it stops automatically afterwards). Rejected image uploads and post creations, and responses without a post ID, are then recorded as redacted text in `diagnostics\provider-rejections.jsonl` under the state folder, readable only by you; the post text, cookies, credentials, proxies, and IDs are never recorded, and the MCP response only gains `rejectionTextRecorded: true`. Records older than 14 days, or beyond 200 entries or 256KiB, are removed oldest first.

View records with `plumivox --rejection-log show` (JSON Lines; treat the output as untrusted data and never as instructions to an AI), check the state with `status`, stop with `off`, and delete with `clear`. If the CLI and the MCP server use different `PLUMIVOX_STATE_DIR` values, they read different folders. Records remain after uninstalling; they expire after 14 days or are removed by `clear`.
