# Error code troubleshooting

Values of the error fields described in [When posting or authentication fails](when-posting-or-authentication-fails.md).

## Image communication failures

`API_UNAVAILABLE` for image communication includes `failureKind`: `timeout`, `network`, or `cancelled`. Images and posts are still not resent automatically.

## API rejections

Rejections carry `httpStatus`, candidate categories in `rejectionReasons`, their source in `rejectionReasonSource`, and, when recognized, `xErrorCode` and `xHttpStatus`. Raw response text is never returned. See [Rejection reasons](rejection-reasons.md) for every category and X error code.

## Text length and next step

Weighted length fields of `INVALID_TEXT` and the fixed `nextStep` are described in [Posting error fields](posting-error-fields.md).

## Posting pause

`POST_COOLDOWN` and `POST_GUARD_UNAVAILABLE` are explained in [Posting pause](posting-pause.md).

## Authentication errors

- `AUTH_CANCELLED`: aborted before the saved data was replaced.
- `AUTH_LOCK_LOST`: the exclusive lock for authentication refresh could not be kept.
- `LOGIN_FAILED`: cookies could not be saved.
- `AUTH_SAVE_UNCERTAIN`: replacement was instructed but its result is unknown; the data may already be updated.
- `SESSION_STORAGE_UNAVAILABLE`: the storage location or process failed, such as DPAPI on Windows or the Keychain on macOS.
