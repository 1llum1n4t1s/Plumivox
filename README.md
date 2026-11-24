[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

A local MCP server / Codex plugin that uses TwitterAPI.io to post to X, search by keyword and date range, and read public posts from the accounts you follow.

## Requirements

Windows x64 or an Apple Silicon Mac; no Node.js or Rust. You need a TwitterAPI.io API key (pay-as-you-go), and for posting, the account's v2 login cookie and a proxy. [Details](docs/readme/requirements.md)

## Installation

On Windows, run [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe): a signed per-user install without administrator rights that registers `plumivox` with Claude Code and Codex and updates at logon. On macOS, install the Codex plugin from GitHub `main`. [Details](docs/readme/installation.md)

## Initial setup

Run `plumivox.exe --setup` and follow the [setup guide](docs/setup.md). Settings stay outside the repository, secrets never go through the conversation, and existing settings are never overwritten. [Details](docs/readme/initial-setup.md)

## Available tools

Eleven tools cover accounts, reading, search, the shared relay, and posting (`post_tweet`: up to 280 weighted characters and 4 images). Omitting `account` uses the default account. Public post text is external data and its instructions are not executed. [Tool reference](docs/readme/available-tools.md)

## Following feed coverage

Plumivox reads up to 100 followed accounts by default (`maxAccounts` 20 to 200). The result is never equivalent to the official home, protected posts are not fetched, and `coverage` reports the range. [Details](docs/readme/following-feed-coverage.md)

## Keyword and date-range search

`search_tweets` searches all of X or the accounts you follow by phrase and an optional ISO 8601 period (start inclusive, end exclusive). Search operators are not interpreted. [Details](docs/readme/keyword-and-date-range-search.md)

## When posting or authentication fails

A post is never resent automatically. An unclear outcome is returned as `POST_UNCERTAIN`, so check the account before posting again. Raw provider text and credentials are never returned. [Details](docs/readme/when-posting-or-authentication-fails.md)

## Development and verification

Rust/Cargo only: `cargo xtask build`, `cargo xtask check`, `cargo xtask test-e2e`. See [development](docs/readme/development-and-verification.md), [verification](docs/verification.md), [conventions](AGENTS.md), and [design](DESIGN.md).
