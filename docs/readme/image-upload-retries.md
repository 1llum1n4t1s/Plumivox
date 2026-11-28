# Image upload retries

The communication time window is 45 seconds for normal API requests and 120 seconds per image upload. Only image uploads retry the same image, at most once, and only for transient failures: HTTP 502, 503, and 504 (250 ms wait), and proxy or upstream failures, JSON rejections of unknown cause, and disconnections (1 second wait). Authentication, balance, invalid image, duplicate, rate limit, and timeout errors are not retried. A rejection that a numeric X error code (such as 226, 88, 344, or 324) or an `x_status` classifies is not retried either, so a rejection that X made is not sent again; a rejection that stays `unknown` (for example a code not in the table) is still retried once.

Waiting and retrying fit within the per-image time window. Previous images, post creation, and login are never resent automatically. If an image fails, the post is not sent; if the outcome becomes unknown after post creation, `POST_UNCERTAIN` is returned.
