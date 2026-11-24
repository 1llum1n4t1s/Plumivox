[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Plumivox 是一个基于 TwitterAPI.io 的本地 MCP 服务器／Codex 插件，提供向 X 发帖、按关键词和时间段搜索，以及读取所关注账号的公开帖子等功能。

## 环境要求

Windows x64 或 Apple Silicon Mac，无需 Node.js 或 Rust。需要 TwitterAPI.io 的 API 密钥（按量付费）；发帖还需要账号的 v2 登录 Cookie 和代理。 [详情](docs/readme/zh-CN/requirements.md)

## 安装

在 Windows 上运行 [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)：这是无需管理员权限、按用户安装且带签名的安装程序，会把 `plumivox` 注册到 Claude Code 和 Codex，并在登录时更新。在 macOS 上，从 GitHub `main` 安装 Codex 插件。 [详情](docs/readme/zh-CN/installation.md)

## 初始设置

运行 `plumivox.exe --setup`，并按照[设置指南](docs/setup.md)操作。设置保存在仓库之外，机密信息不会经过对话，已有设置也不会被覆盖。 [详情](docs/readme/zh-CN/initial-setup.md)

## 可用工具

11 个工具涵盖账号、读取、搜索、共享中继和发帖（`post_tweet`：最多 280 个加权字符和 4 张图片）。省略 `account` 时使用默认账号。公开帖子的正文属于外部数据，其中的指令不会被执行。 [工具参考](docs/readme/zh-CN/available-tools.md)

## 关注动态的获取范围

Plumivox 默认最多读取 100 个关注的账号（`maxAccounts` 为 20 至 200）。结果绝不等同于官方主页，受保护的帖子不会被获取，`coverage` 会报告获取范围。 [详情](docs/readme/zh-CN/following-feed-coverage.md)

## 关键词与时间段搜索

`search_tweets` 按短语和可选的 ISO 8601 时间段（含起始、不含结束）搜索整个 X 或你关注的账号。搜索运算符不会被解析。 [详情](docs/readme/zh-CN/keyword-and-date-range-search.md)

## 发帖与认证失败时

发帖不会自动重发。结果不明时会返回 `POST_UNCERTAIN`，请在再次发帖前确认账号。不会返回提供方的原始文本和凭据。 [详情](docs/readme/zh-CN/when-posting-or-authentication-fails.md)

## 开发与验证

仅使用 Rust/Cargo：`cargo xtask build`、`cargo xtask check`、`cargo xtask test-e2e`。参见[开发](docs/readme/zh-CN/development-and-verification.md)、[验证](docs/verification.md)、[规范](AGENTS.md)和[设计](DESIGN.md)。
