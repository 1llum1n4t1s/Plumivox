# Image upload retries

The communication time window is 45 seconds for normal API requests and 120 seconds per image upload. Only image uploads retry the same image, at most once, and only for transient failures: HTTP 502, 503, and 504 (250 ms wait), and proxy or upstream failures, JSON rejections of unknown cause, and disconnections (1 second wait). Authentication, balance, invalid image, duplicate, rate limit, and timeout errors are not retried.

Waiting and retrying fit within the per-image time window. Previous images, post creation, and login are never resent automatically. If an image fails, the post is not sent; if the outcome becomes unknown after post creation, `POST_UNCERTAIN` is returned.
