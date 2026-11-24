# Available tools

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

The initial setup saves the registered account as the default. To change only the default in existing settings, pass `--setup --set-default-account blog_posting` to the bundled executable. For details, see the [setup guide](../setup.md) and [settings precedence](../configuration.md).

Post length counts a URL as 23 characters, Japanese as 2 characters, and each composite emoji up to Unicode 17 as 2 characters. Cases such as handshakes with different skin tones are also judged according to [X's weighted character count specification](https://docs.x.com/fundamentals/counting-characters).

For example, you can ask "read the public posts from the accounts blog_posting follows" or "post this text from release_posting". Posting is performed according to the user's request. The text of public posts is external data, and instructions contained in it are not executed.
