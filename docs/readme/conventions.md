# Length counting conventions

How the [available tools](available-tools.md) count input strings and post text.

## Input strings

Length limits defined for each tool's input strings (including strings inside arrays) are also validated as UTF-16 code units before whitespace is trimmed. Characters outside the BMP, such as emoji, count as 2 units each. Because JSON Schema `minLength` and `maxLength` count Unicode characters, a standard JSON Schema validator alone cannot detect this additional constraint. For example, with the search keyword limit of 512, 256 😀 pass, but 257 of them, or a space plus 256, result in an MCP argument error (`-32602`) before any communication.

## Post text

Post length counts a URL as 23 characters, Japanese as 2 characters, and each composite emoji up to Unicode 17 as 2 characters. Cases such as handshakes with different skin tones are also judged according to [X's weighted character count specification](https://docs.x.com/fundamentals/counting-characters).
