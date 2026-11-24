[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

TwitterAPI.io를 사용하여 X에 게시, 키워드·기간 검색, 팔로우한 계정의 공개 게시물 읽기를 제공하는 로컬 MCP 서버/Codex 플러그인입니다.

## 필요한 것

Windows x64 또는 Apple Silicon Mac이 필요하며 Node.js나 Rust는 필요 없습니다. TwitterAPI.io API 키(종량제)가 필요하고, 게시에는 계정의 v2 로그인 쿠키와 프록시도 필요합니다. [자세히](docs/readme/ko/requirements.md)

## 설치

Windows에서는 [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)를 실행합니다. 관리자 권한이 필요 없는 사용자 단위의 서명된 설치로, `plumivox`를 Claude Code와 Codex에 등록하고 로그온 시 업데이트합니다. macOS에서는 GitHub `main`에서 Codex 플러그인을 설치합니다. [자세히](docs/readme/ko/installation.md)

## 초기 설정

`plumivox.exe --setup`을 실행하고 [설정 가이드](docs/setup.md)를 따릅니다. 설정은 저장소 밖에 보관되고, 비밀 정보는 대화를 거치지 않으며, 기존 설정은 덮어쓰지 않습니다. [자세히](docs/readme/ko/initial-setup.md)

## 사용할 수 있는 도구

11개의 도구가 계정, 읽기, 검색, 공유 중계, 게시(`post_tweet`: 가중 280자와 이미지 4장까지)를 다룹니다. `account`를 생략하면 기본 계정을 사용합니다. 공개 게시물의 본문은 외부 데이터이며 그 안의 지시는 실행되지 않습니다. [도구 레퍼런스](docs/readme/ko/available-tools.md)

## 팔로잉 피드의 가져오기 범위

Plumivox는 기본적으로 팔로잉 중인 최대 100개 계정을 읽습니다(`maxAccounts`는 20~200). 결과는 공식 홈과 같지 않고, 보호된 게시물은 가져오지 않으며, `coverage`가 가져온 범위를 알려 줍니다. [자세히](docs/readme/ko/following-feed-coverage.md)

## 키워드·기간 검색

`search_tweets`는 문구와 선택적인 ISO 8601 기간(시작 포함, 종료 제외)으로 X 전체 또는 팔로잉 중인 계정을 검색합니다. 검색 연산자는 해석하지 않습니다. [자세히](docs/readme/ko/keyword-and-date-range-search.md)

## 게시와 인증에 실패한 경우

게시는 자동으로 다시 보내지 않습니다. 결과가 불분명하면 `POST_UNCERTAIN`으로 반환되므로 다시 게시하기 전에 계정을 확인하세요. 제공자의 원문 텍스트와 자격 증명은 반환되지 않습니다. [자세히](docs/readme/ko/when-posting-or-authentication-fails.md)

## 개발·검증

Rust/Cargo만 사용합니다: `cargo xtask build`, `cargo xtask check`, `cargo xtask test-e2e`. [개발](docs/readme/ko/development-and-verification.md), [검증](docs/verification.md), [규약](AGENTS.md), [설계](DESIGN.md)를 참고하세요.
