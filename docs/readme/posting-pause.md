# Posting pause

Plumivox pauses posting per X account, shared by all sessions and processes on the PC, so rapid posts or quick resends do not worsen X's restrictions. While paused, `post_tweet` sends nothing, incurs no charge, and returns `POST_COOLDOWN` (`stage: post_guard`, `postAttempted: false`) with `retryAfter` and `retryAfterSeconds`. Identical text never pauses. Values are fixed.

## Reasons

- `min_interval`: within 5 minutes of the last attempt that reached X (including a proxy rejection during post creation). `post_in_progress`: another post is being sent.
- X restrictions: `automation_suspected` 3 hours, `daily_limit` 6 hours, `rate_limit` 15 minutes, doubling per consecutive rejection up to 24 hours; the count resets 48 hours after the last one. Only a rate limit known to come from X (code 88 or X HTTP status 429) doubles; a bare HTTP 429 pauses 15 minutes uncounted. `account_locked` pauses 24 hours.
- 15 minutes: `previous_uncertain`, `interrupted` (stopped while sending), `guard_state_reset` (unreadable state), `unclassified_rejection`. State written by a newer version is left untouched and posts are refused with `guard_state_reset`.

Failures that never reached X (before sending, or stopped at the provider for the API key, balance, an image-upload proxy rejection, or HTTP 401/402) start no pause. Rejected posts also carry `cooldownReason` and `retryAfter`. If the state cannot be read or written, nothing is sent and `POST_GUARD_UNAVAILABLE` is returned.

## What the AI should do

Do not post to the account before `retryAfter` or bypass the pause. For `min_interval` and `post_in_progress`, `nextStep` allows posting the same content once after `retryAfter`; otherwise it says to tell the user and wait. After an uncertain outcome, checking `get_my_posts` early is fine.

## Checking and clearing

State lives under `post-guard\` in the state folder as codes, times, and counts only; never text, names, or credentials.

`plumivox --post-guard status` shows each account's reason, remaining time, and history.

`plumivox --post-guard clear [--account NAME | --user NAME]` clears it (profile name or X user name; neither means all). A profile whose name contains the X user name is saved as `<user>` and needs `--user` (names under 4 characters only exactly). You must type the shown name (or `all`) at an interactive terminal; an in-progress post blocks clearing. Exit codes: 0 success (also an unmatched `--account`, with a hint), 1 storage error or post in progress, 2 invalid arguments, non-terminal, or mismatched input. MCP cannot clear it, and clearing does not lift X's restriction. The CLI and MCP server must share `PLUMIVOX_STATE_DIR`.
