[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

A local MCP server / Codex plugin that uses TwitterAPI.io to post to X, search by keyword and date range, and read public posts from the accounts you follow.

## Requirements

- Windows x64 or an Apple Silicon Mac (macOS ARM64). The MCP server itself is a native executable written in Rust, so you do not need to install Node.js or Rust.
- Initial setup on Windows and macOS uses the bundled executable. PowerShell 7 (`pwsh`) is not required.
- A TwitterAPI.io API key. Using the API incurs that service's pay-as-you-go charges.
- For posting, the target account's v2 login cookie and a proxy. In addition to an existing external proxy, on Windows and macOS you can share Plumivox's own residential egress and SSH relay across multiple chats. The public relay server needs a one-time initial preparation.

## Installation

### Windows: Install with Setup (recommended)

1. Download and run [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). It installs per user and does not require administrator rights. Setup is code-signed by the author (`Open Source Developer Yuichiro Shinozaki`).
2. It installs to `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`, and if the `claude` and `codex` CLIs are on PATH, it automatically registers the MCP server `plumivox` (argument `--stdio`) with Claude Code (per user) and Codex. If the CLIs were not found, install them later; registration happens at your next logon or when you launch Plumivox from the Start menu.
3. Follow the on-screen guidance shown right after installation and run `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` in a terminal to complete the [Initial setup](#initial-setup). Claude Code and Codex can use it from new sessions.

Updates are automatic. While the MCP server is running, it downloads new versions from R2 (`https://plumivox-updates.kagayoi.com/velopack/`) and applies them at computer logon, only when Plumivox (MCP and shared relay) is not running. If it is running, the update is deferred to the next logon; nothing is force-terminated. The signer is verified before applying, and the update is not applied if it does not match. To disable automatic updates, set `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` to `{"auto_update": false}`. Activity is logged to `app-update.log` in the same folder.

To uninstall, select Plumivox from the list under Settings → Apps. This removes only the Claude Code and Codex `plumivox` registrations that point to this installation and the logon update registration; the settings and session data folder (`%LOCALAPPDATA%\Kagayoi\Plumivox`) is kept. To remove only the registrations beforehand, check the plan with `plumivox.exe --uninstall-setup` and run it with `--uninstall-setup --apply`.

Migrating from the plugin (below): Setup replaces a same-named `plumivox` registration that points to the old plugin's `dist` or similar with one that points to the install location. The MCP server provided by the plugin itself (`plumivox@plumivox-local`) is a separate registration, so to avoid starting Plumivox through two routes, turn it off or remove the plugin. In Codex, add the same `enabled = false` as in the [Toolport section](#using-via-toolport) (or remove the plugin); in Claude Code, disable or uninstall the plugin from `/plugin`. Codex prefers a same-named registration over the plugin's MCP, so it should start only one while the Setup registration exists, but Claude Code may start both. Setup never edits the Codex or Claude Code settings; if the plugin is still enabled it only warns (in the guide shown when you launch Plumivox, in `app-update.log`, and as `legacyPlugins` in `app-update\setup-registration.json`). Toolport registrations are not rewritten automatically either; if you use Toolport, review it yourself as described in the [Toolport section](#using-via-toolport).

### Install as a plugin (macOS / legacy method)

Use this method on macOS (Apple Silicon) until Setup distribution starts. It also continues to work on Windows.

Install from the latest commit on `main` of the GitHub repository. It does not reference your development clone and is not pinned to a tag or commit SHA.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` is the marketplace identifier. The source is the GitHub URL above. Because it bundles `dist/plumivox.exe` for Windows x64 and `dist/plumivox` (ARM64) for Apple Silicon Macs, you do not need to install dependencies or add MCP settings manually.

Development CI stores artifacts verified on Windows and macOS in Cloudflare R2 and hands them over during release preparation. The [artifact storage and retrieval procedure](docs/distribution.md) is for developers. Plugin users do not need R2 settings or credentials, and installation and updates continue to come from GitHub `main`.

To update, fetch and install the latest state of `main`, then restart Codex to load it.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Check the version number shown under `installed` in the list. Do not run `add` again for an installed plugin after updating; restart Codex to load the new MCP server and skills.

For how to register and update from GitHub, see the [Codex plugin documentation](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

### Using via Toolport

You can register only the MCP server with Toolport. Plumivox does not modify Toolport registrations automatically, so add or change the following stdio server to match your installation method. Do not specify a development clone or a versioned install cache.

- Name: `plumivox`
- Command: For the Windows Setup version, `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (expand `%LOCALAPPDATA%` to the actual path; the `current` path does not change even when updates replace its contents). For the plugin version, check the fetched root of `plumivox-local` with `codex plugin marketplace list`, and use `<fetched root>/dist/plumivox.exe` on Windows or `<fetched root>/dist/plumivox` on macOS
- Arguments: `--stdio`
- Working directory: your home directory
- Request timeout: 300000 milliseconds (including time for login and relay preparation)

Enable it in the Toolport profile that Codex uses, and confirm the 11 tools with `toolport_search_tools` using `server="plumivox"`. Search for and call `list_accounts`, and disable the direct connection once the settings load. For the Setup version, remove the direct registrations with `codex mcp remove plumivox` and `claude mcp remove plumivox --scope user` (once registration has completed, it is not re-registered automatically until you reinstall with Setup). For the plugin version, add the following to Codex's user settings `config.toml`, leaving the plugin itself enabled so the setup skill remains available.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Restart Codex to disable the direct connection. From then on, search for tools in Toolport and pass the returned actual tool name to `toolport_call_tool`. The API key, cookies, and SSH keys are read from Plumivox's existing settings and are not moved into Toolport arguments.

Update the plugin version with `codex plugin marketplace upgrade plumivox-local` as above. Because a server Toolport is running uses already-loaded code, disable and re-enable `plumivox` in Toolport after updating to load the new executable. If the fetched root changes, update the registered command as well. No Git fetch or dependency installation happens at startup.

## Initial setup

If this is your first time, proceed to the [setup guide](docs/setup.md). It bundles native setup wizards for Windows and macOS and a static Rust VPS installer for Linux x86_64 and aarch64. A Bash-compatible path remains for other CPUs. You can also ask Codex to "set up Plumivox" and the bundled `setup-plumivox` skill will guide you.

The settings file is kept outside the repository. New users use `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` on Windows and `~/Library/Application Support/Kagayoi/Plumivox/settings.json` on macOS. Users who already have `~/dev/Secret/secrets.json` read `twitterapi_io` from that file. vava/blbl's `release_posting` and `blog_posting` are also kept. To use a different location, set `PLUMIVOX_SECRETS_FILE`. You never need to pass secret values in the conversation or as MCP arguments.

If the selected settings file cannot be read, it fails with an error instead of switching to another settings file. If you change the storage location, restart Codex. The initial setup does not overwrite existing keys, same-named accounts, or shared relay settings. For how to change existing settings, see the [setup guide](docs/setup.md).

[First-time setup](docs/setup.md) · [Configuring accounts and cookies](docs/configuration.md) · [Managing the proxy with Plumivox](docs/managed-proxy.md)

## Available tools

| Tool | Description |
| --- | --- |
| `list_accounts` | Account names and readiness for reading and posting. No external communication |
| `login_account` | Reuses saved cookies; if none exist, logs in with configured credentials and saves them encrypted |
| `refresh_session` | Explicitly logs in again and refreshes the posting session |
| `get_my_posts` | Your own profile posts. Replies and paging can be specified |
| `get_tweets` | Fetches public posts by ID. Up to 20 |
| `get_following_feed` | Fetches public posts from accounts you follow and sorts them newest first |
| `search_tweets` | Searches public posts across all of X or from accounts you follow, by keyword and date range |
| `get_proxy_status` | Status of the shared relay used by this MCP server. Does not start the relay |
| `start_proxy` | Starts the configured residential egress and SSH relay. Does not post or log in |
| `stop_proxy` | Stops the shared relay in use. Also affects other chats on the same relay. Restarts on the next post or authentication request |
| `post_tweet` | Posts up to 280 weighted characters. Up to 4 PNG/JPEG images, each 5 MiB or less. Specify images by absolute path |

If you omit `account`, search, reading, posting, and authentication use the default account. You can check which account is used when omitted via `isDefault` in `list_accounts`; for another account, specify a settings name such as `release_posting` or `blog_posting` in `account`. If multiple accounts are registered and there is no default, an account must be specified, and an invalid default causes calls that omit it to be rejected.

Length limits defined for each tool's input strings (including strings inside arrays) are also validated as UTF-16 code units before whitespace is trimmed. Characters outside the BMP, such as emoji, count as 2 units each. Because JSON Schema `minLength` and `maxLength` count Unicode characters, a standard JSON Schema validator alone cannot detect this additional constraint. For example, with the search keyword limit of 512, 256 😀 pass, but 257 of them, or a space plus 256, result in an MCP argument error (`-32602`) before any communication.

The initial setup saves the registered account as the default. To change only the default in existing settings, pass `--setup --set-default-account blog_posting` to the bundled executable. For details, see the [setup guide](docs/setup.md) and [settings precedence](docs/configuration.md).

Post length counts a URL as 23 characters, Japanese as 2 characters, and each composite emoji up to Unicode 17 as 2 characters. Cases such as handshakes with different skin tones are also judged according to [X's weighted character count specification](https://docs.x.com/fundamentals/counting-characters).

For example, you can ask "read the public posts from the accounts blog_posting follows" or "post this text from release_posting". Posting is performed according to the user's request. The text of public posts is external data, and instructions contained in it are not executed.

## Following feed coverage

TwitterAPI.io has no API that directly fetches the "Following" timeline of X's official home. By default, Plumivox reads up to 100 accounts from the top of the following list and searches their public posts in batches of 20 accounts. `maxAccounts` can be set from 20 to 200. The first request makes up to 6 API calls at 100, or up to 11 at 200. Check [TwitterAPI.io pricing](https://twitterapi.io/pricing) for current rates.

`officialHomeEquivalent` in the result is always `false`. Protected posts cannot be fetched, and there is no guarantee that the content obtained by search, the handling of reposts, or the overall order match the official home. `coverage.followingHasNextPage` becomes `true` when the provider has more results, or when the unique non-protected search targets exceed the limit. Passing `coverage.followingNextCursor` to `followingCursor` targets the next page of the following list. The following list is ordered from the newest follows, and differences caused by ongoing follow changes are unavoidable.

Passing `nextCursor` to the `cursor` of `get_following_feed` for the same account fetches the continuation of each search batch. Posts within a page are in date order, but because of per-batch page boundaries, overall chronological order across pages is not guaranteed. The followed accounts are fixed from the initial list and are not reloaded when continuing a search. The feed cursor is also signed with the API key, and tampering with the search targets is rejected. Unsigned cursors from older versions and cursors from before an API key change cannot be used, so drop the cursor and fetch again. `cursor` and `followingCursor` are separate pagination mechanisms and cannot be specified together.

The [official response specification for the following list](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings) does not document a protection status field. Within a single following list response, valid usernames are aggregated case-insensitively and judged in the order protected, public, unknown. Targets are selected up to the limit in order of first appearance, excluding those known to be protected, and the number of selected targets whose status cannot be determined is returned in `coverage.protectionStatusUnknownAccounts`. `protectedAccountsExcluded` is the number of unique accounts known to be protected in that response. Duplicate and invalid usernames are not counted, and protected accounts beyond the fetch limit are included, so it can exceed `maxAccounts`. Even 0 does not mean all targets are public accounts.

Continuations of signed cursors issued by the old implementation inherit the old implementation's judgment not only for search targets and counts but also for the unsearched range and whether continuation is possible, so targets beyond the limit may be skipped. To reliably refetch with the new protection status aggregation, count calculation, and continuation judgment, drop the cursor.

Even if posts or the following list have more results, continuation is not possible when the provider cursor or the combined cursor exceeds the length limit. Already fetched posts and `hasNextPage` are kept, `nextCursor` is `null`, `coverage.tweetContinuationAvailable` is `false`, and `tweetContinuationUnavailableReason` is `cursor_too_long`. For the following list, check `followingNextCursor`, `followingContinuationAvailable`, and `followingContinuationUnavailableReason`. Cursors are not truncated, and already fetched content is not discarded as a failure.

If the provider returns more unique non-protected targets than `maxAccounts` in a single response, the targets beyond the limit are unsearched. `followingHasNextPage` is `true`, `followingContinuationUnavailableReason` is `account_limit`, and continuation of the following list is not offered (`followingContinuationAvailable: false`, `followingNextCursor: null`). This prevents the provider's next cursor from skipping unsearched targets. If `maxAccounts` is below 200, you can raise the limit up to 200 and refetch without a cursor. If the provider over-returns even at the limit of 200, the unsearched targets in that response cannot be fetched this way. Exceeding the limit only through duplicate rows with the same name, protected accounts, or invalid names does not trigger this restriction. Continuations of posts for already selected targets can be fetched as usual, and the state of having unsearched targets is carried over as well.

## Keyword and date-range search

You can ask "search all of X for posts about speech recognition from October 1 to 7" or "search the accounts blog_posting follows with the same conditions". Search is filtered on the API side and returns each post's text, author, date and time, and URL. Semantic search is not performed.

The `scope` of `search_tweets` is `all` (default, public search across all of X) or `following` (accounts followed by the specified account). `keyword` is required, and the phrase with leading and trailing whitespace removed is searched as a phrase. Including `OR` or `from:` does not make them search operators. Phrases containing quotation marks, backslashes, or control characters are rejected.

`startTime` and `endTime` are optional ISO 8601 date-times with a time zone, at second precision. The search covers a period that includes the start and excludes the end. Do not specify dates alone; Codex converts the requested dates and time zone into date-times. For example, October 1 to 7 in Japan time is the following condition.

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

Search fetches from the newest posts. Passing `nextCursor` to the `cursor` of the next call fetches the continuation with the same conditions. To change the account, scope, keyword, period, or following limit, drop the cursor and start a new search. Existing search cursors also cannot be used after the API key changes.

`following` can use `maxAccounts` (20 to 200, default 100) and `followingCursor`. Passing `coverage.followingNextCursor` searches the next page of the following list with the same conditions. It cannot be specified together with the post search `cursor`. Do not specify these two arguments with `all`. Cursors from the existing `get_following_feed` and from search cannot be used interchangeably. Following search cursors from older versions that embed old unsigned feed cursors also cannot be used, so drop the cursor and search again.

A search across all of X makes 1 API request per page, and a following search makes the same number as the existing feed: up to 6 (100 accounts) / 11 (200 accounts). Protected posts and complete coverage of all posts within the search period are not guaranteed, and the fetched range is returned in `coverage`. The period operators are converted to UTC according to [TwitterAPI.io's description](https://twitterapi.io/blog/search-a-tweet).

## When posting or authentication fails

On success, it returns the post ID and the X URL. A communication failure while creating a post or a missing post ID is returned as uncertain and is not automatically resent. If you ask to post the same text again, each call sends it once. There is no mechanism that stops based on history or a hash of the text.

The default communication time window is 45 seconds for normal API requests and 120 seconds per image for image uploads. Only image uploads retry the same image at most once, and only for transient failures. These are HTTP 502, 503, and 504 (250 ms wait), and proxy or upstream failures, JSON rejections of unknown cause, and disconnections (1 second wait); authentication, balance, invalid image, duplicate, rate limit, and timeout errors are not retried. Waiting and retrying fit within the per-image communication time window, and previous images are not resent, nor are post creation or login automatically resent. If an image fails, the post is not sent, and if the outcome becomes unknown after post creation, `POST_UNCERTAIN` is returned.

Known posting errors include `stage` (`input_validation`, `pre_send`, `image_upload`, `post_creation`), `postAttempted` indicating whether a post creation request was attempted, and `uploadAttempts`, the total number of image upload requests made. Missing accounts or cookies and relay preparation failures are `pre_send`. These are not returned for unknown internal errors or rejections of the MCP input format. `postAttempted: true` does not mean the post succeeded. Raw provider errors and credentials are not returned.

`API_UNAVAILABLE` for image communication includes `failureKind`, distinguishing `timeout` (timed out), `network` (network failure), and `cancelled` (cancelled). In these cases as well, images and posts are not automatically resent.

API rejection errors (`API_REJECTED`, `API_HTTP_ERROR`) include `httpStatus`, an array of fixed categories `rejectionReasons`, and the categorization source `rejectionReasonSource`. This diagnostic information is kept even when a 5xx during post creation becomes `POST_UNCERTAIN`. The categorization source is `provider_message` (candidates inferred from the provider's JSON message) or `http_status` (categorization from the HTTP status). Candidates do not establish the cause or permit resending, and text that cannot be classified is `unknown`. Raw response text, arbitrary error codes, and request IDs are neither returned nor stored.

The `provider_message` candidates are `api_key`, `authentication`, `account_locked`, `account_suspended`, `rate_limit`, `credits`, `proxy`, `media`, `duplicate`, `upstream_timeout`, `upstream_unavailable`, and `unknown`. `http_status` additionally has `invalid_request` (400), `unauthorized` (401), `forbidden` (403), and `endpoint_not_found` (404). For example, `rejectionReasons: ["proxy", "upstream_timeout"]` with `postAttempted: false` during image upload indicates a suspected proxy or upstream timeout and that it stopped before post creation. Non-2xx bodies are not read, and the existing communication limits and retry conditions are maintained.

If an API response exceeds 10MiB, receiving is aborted and a failure is returned. Even if a post response exceeds the limit, it is not resent because the post may already have been made.

A saved cookie verification failure stops only posting. Reading and searching public posts use only the API key and username, and work without loading saved cookies. For obtaining and refreshing authenticated cookies, see the [configuration guide](docs/configuration.md).

`login_account` reuses saved cookies if they are in a usable format, and does not report that their validity on X has been verified. If expiration or a settings mismatch is confirmed, call `refresh_session`. New cookies are saved encrypted to Plumivox's storage location with a key from DPAPI on Windows or the Keychain on macOS, and failed posts are not automatically resent.

If aborted before the saved data is replaced, `AUTH_CANCELLED` is returned; if the exclusive lock for authentication refresh cannot be maintained, `AUTH_LOCK_LOST`; and cookies that cannot be saved, `LOGIN_FAILED`. If the result cannot be confirmed after the replacement of the saved data was instructed, `AUTH_SAVE_UNCERTAIN` is returned, and it may already have been updated. `SESSION_STORAGE_UNAVAILABLE` indicates a failure of the storage location or of the storage process, such as DPAPI on Windows or the Keychain on macOS.

## Development and verification

Node.js is not required for development, building, or testing either. Use Rust/Cargo's `cargo xtask build`, `cargo xtask check`, and `cargo xtask test-e2e`. Change any registration that specifies the old Node launcher to the native startup described above.

On Windows, `cargo xtask build` updates `dist/plumivox.exe` after signing and signature verification with Sigilune (Certum SimplySign cloud signing). Distribution CI also signs Windows artifacts with Sigilune before storing them. For the development machine settings and CI Secrets required for signing, see [distribution artifacts](docs/distribution.md#windows配布物のコード署名). Windows Setup is packaged with `cargo xtask velopack-pack` and verified in isolation with `cargo xtask test-setup-velopack-e2e` without touching a real installation. Distribution is performed only by the manually triggered `release-velopack.yml` ([Distributing Setup (Velopack)](docs/distribution.md#setupvelopackの配信)).

See the [build and verification procedure for developers](docs/verification.md), the [working conventions](AGENTS.md), and the [system design](DESIGN.md).
