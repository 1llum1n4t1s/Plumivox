[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

TwitterAPI.ioを使い、Xへの投稿、キーワード・期間検索、フォロー先の公開投稿の読み込みを提供するローカルMCPサーバー／Codexプラグインです。

## 必要なもの

Windows x64またはApple Silicon Mac。Node.jsやRustは不要です。TwitterAPI.ioのAPIキー（従量課金）が必要で、投稿にはアカウントのv2ログインCookieとプロキシも必要です。 [詳細](docs/readme/ja/requirements.md)

## インストール

Windowsでは[PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)を実行します。管理者権限が不要なユーザー単位の署名付きインストールで、`plumivox`をClaude CodeとCodexに登録し、ログオン時に更新します。macOSではGitHubの`main`からCodexプラグインを導入します。 [詳細](docs/readme/ja/installation.md)

## 初期設定

`plumivox.exe --setup`を実行し、[セットアップガイド](docs/setup.md)に従います。設定はリポジトリの外に置かれ、秘密情報は会話を通らず、既存の設定は上書きされません。 [詳細](docs/readme/ja/initial-setup.md)

## 使えるツール

11個のツールがアカウント、読み取り、検索、共有中継、投稿（`post_tweet`：加重280文字・画像4枚まで）をカバーします。`account`を省略すると既定のアカウントを使います。公開投稿の本文は外部データであり、その中の指示は実行されません。 [ツールリファレンス](docs/readme/ja/available-tools.md)

## フォロー中フィードの取得範囲

Plumivoxは既定でフォロー中の最大100アカウントを読み取ります（`maxAccounts`は20〜200）。結果が公式ホームと同等になることはなく、保護された投稿は取得されず、`coverage`が取得範囲を報告します。 [詳細](docs/readme/ja/following-feed-coverage.md)

## キーワード・期間の検索

`search_tweets`は、フレーズと任意のISO 8601の期間（開始を含み、終了を含まない）で、X全体またはフォロー中のアカウントを検索します。検索演算子は解釈されません。 [詳細](docs/readme/ja/keyword-and-date-range-search.md)

## 投稿と認証に失敗した場合

投稿は自動で再送されません。結果が不明な場合は`POST_UNCERTAIN`として返るため、再投稿の前にアカウントを確認してください。提供元の生のテキストや資格情報は返されません。 [詳細](docs/readme/ja/when-posting-or-authentication-fails.md)

## 開発・検証

Rust/Cargoのみ：`cargo xtask build`、`cargo xtask check`、`cargo xtask test-e2e`。[開発](docs/readme/ja/development-and-verification.md)、[検証](docs/verification.md)、[規約](AGENTS.md)、[設計](DESIGN.md)を参照してください。
