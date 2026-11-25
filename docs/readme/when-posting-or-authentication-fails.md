# When posting or authentication fails

On success, it returns the post ID and the X URL. A communication failure while creating a post or a missing post ID is returned as uncertain and is not automatically resent. If you ask to post the same text again, each call sends it once. There is no mechanism that stops based on history or a hash of the text.

The default communication time window is 45 seconds for normal API requests and 120 seconds per image for image uploads. Only image uploads retry the same image at most once, and only for transient failures. These are HTTP 502, 503, and 504 (250 ms wait), and proxy or upstream failures, JSON rejections of unknown cause, and disconnections (1 second wait); authentication, balance, invalid image, duplicate, rate limit, and timeout errors are not retried. Waiting and retrying fit within the per-image communication time window, and previous images are not resent, nor are post creation or login automatically resent. If an image fails, the post is not sent, and if the outcome becomes unknown after post creation, `POST_UNCERTAIN` is returned.

Known posting errors include `stage` (`input_validation`, `pre_send`, `image_upload`, `post_creation`), `postAttempted` indicating whether a post creation request was attempted, and `uploadAttempts`, the total number of image upload requests made. Missing accounts or cookies and relay preparation failures are `pre_send`. These are not returned for unknown internal errors or rejections of the MCP input format. `postAttempted: true` does not mean the post succeeded. Raw provider errors and credentials are not returned.

Rejections include the HTTP status and categorized candidate reasons, and `API_UNAVAILABLE` for images includes `failureKind`. For their values and the authentication error codes, see [Error code troubleshooting](troubleshooting.md).

If an API response exceeds 10MiB, receiving is aborted and a failure is returned. Even if a post response exceeds the limit, it is not resent because the post may already have been made.

A saved cookie verification failure stops only posting. Reading and searching public posts use only the API key and username, and work without loading saved cookies. For obtaining and refreshing authenticated cookies, see the [configuration guide](../configuration.md).

`login_account` reuses saved cookies if they are in a usable format, and does not report that their validity on X has been verified. If expiration or a settings mismatch is confirmed, call `refresh_session`. New cookies are saved encrypted to Plumivox's storage location with a key from DPAPI on Windows or the Keychain on macOS, and failed posts are not automatically resent.
