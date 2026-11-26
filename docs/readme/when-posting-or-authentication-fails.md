# When posting or authentication fails

On success, it returns the post ID and the X URL. A communication failure while creating a post or a missing post ID is returned as uncertain and is not automatically resent. Asking to post the same text again sends it once per call; nothing stops duplicates based on history or a text hash.

Only image uploads are retried, once and only for transient failures; post creation and login never are. See [Image upload retries](image-upload-retries.md).

Posting errors report the failing `stage`, whether a post was attempted, weighted text length when too long, and a fixed `nextStep` for AI clients. See [Posting error fields](posting-error-fields.md). Rejections include the HTTP status and categorized candidate reasons; see [Error code troubleshooting](troubleshooting.md). Raw provider errors and credentials are never returned or saved.

If an API response exceeds 10MiB, receiving is aborted and a failure is returned; a post is not resent even then, because it may already exist.

A saved cookie verification failure stops only posting. Reading and searching public posts use only the API key and username. For obtaining and refreshing cookies, see the [configuration guide](../configuration.md).

`login_account` reuses saved cookies if their format is usable, without verifying them on X. If expiration or a settings mismatch is confirmed, call `refresh_session`. New cookies are saved encrypted with a key from DPAPI on Windows or the Keychain on macOS.
