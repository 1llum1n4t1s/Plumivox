[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

TwitterAPI.io를 사용하여 X에 게시, 키워드·기간 검색, 팔로우한 계정의 공개 게시물 읽기를 제공하는 로컬 MCP 서버/Codex 플러그인입니다.

## 필요한 것

- Windows x64 또는 Apple Silicon Mac(macOS ARM64). MCP 본체는 Rust로 만든 네이티브 실행 파일이므로 Node.js나 Rust를 설치할 필요가 없습니다.
- Windows·macOS의 초기 설정은 동봉된 실행 파일을 사용합니다. PowerShell 7 (`pwsh`)은 필요하지 않습니다.
- TwitterAPI.io의 API 키. API 사용 시 해당 서비스의 종량제 요금이 발생합니다.
- 게시하려면 대상 계정의 v2 로그인 Cookie와 프록시가 필요합니다. 기존 외부 프록시 외에도 Windows·macOS에서는 Plumivox 내의 주거용 회선 출구와 SSH 중계를 여러 채팅에서 공유할 수 있습니다. 공개 중계 서버의 최초 준비는 필요합니다.

## 설치

### Windows: Setup으로 설치하기(권장)

1. [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)를 다운로드하여 실행합니다. 사용자 단위 설치이므로 관리자 권한이 필요하지 않습니다. Setup에는 본인의 코드 서명(`Open Source Developer Yuichiro Shinozaki`)이 포함되어 있습니다.
2. `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`에 설치되며, PATH에 `claude`·`codex` CLI가 있으면 Claude Code(사용자 단위)와 Codex에 MCP 서버 `plumivox`(인수 `--stdio`)를 자동으로 등록합니다. CLI를 찾지 못한 경우에는 나중에 CLI를 설치한 뒤 다음 로그온 시 또는 시작 메뉴의 Plumivox를 실행하면 등록됩니다.
3. 설치 직후 화면의 안내에 따라 터미널에서 `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup`을 실행하여 [초기 설정](#초기-설정)을 진행합니다. Claude Code·Codex는 새 세션부터 사용할 수 있습니다.

업데이트는 자동입니다. MCP가 실행되는 동안 새 버전을 R2(`https://plumivox-updates.kagayoi.com/velopack/`)에서 받아 오고, PC에 로그온할 때 Plumivox(MCP·공유 중계)가 실행 중이지 않은 경우에만 적용합니다. 실행 중이면 다음 로그온으로 미루며 강제 종료하지 않습니다. 적용 전에 서명자를 확인하고, 다르면 적용하지 않습니다. 자동 업데이트를 끄려면 `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json`을 `{"auto_update": false}`로 설정합니다. 동작 기록은 같은 폴더의 `app-update.log`에 있습니다.

제거하려면 "설정" → "앱" 목록에서 Plumivox를 선택합니다. 자신을 가리키는 Claude Code·Codex의 `plumivox` 등록과 로그온 시 업데이트 등록만 해제하고, 설정·세션 데이터 폴더(`%LOCALAPPDATA%\Kagayoi\Plumivox`)는 남겨 둡니다. 등록만 먼저 해제하려면 `plumivox.exe --uninstall-setup`으로 예정 내용을 확인하고 `--uninstall-setup --apply`로 실행합니다.

플러그인(아래)에서 옮겨 오는 경우: Setup은 기존 플러그인의 `dist` 등을 가리키는 같은 이름의 `plumivox` 등록을 설치 위치를 가리키는 등록으로 교체합니다. 플러그인 자체가 가진 MCP(`plumivox@plumivox-local`)는 별도의 등록이므로, Plumivox가 두 경로로 시작되지 않도록 비활성화하거나 플러그인을 제거하세요. Codex에서는 [Toolport 절](#toolport를-통해-사용하는-경우)과 같은 `enabled = false`를 추가하고(또는 플러그인을 제거), Claude Code에서는 `/plugin`에서 플러그인을 비활성화하거나 삭제합니다. Codex는 같은 이름의 등록을 플러그인의 MCP보다 우선하므로 Setup 등록이 있는 동안에는 하나만 시작되어야 하지만, Claude Code는 둘 다 시작할 수 있습니다. Setup은 Codex·Claude Code 설정을 수정하지 않으며, 플러그인이 계속 활성화되어 있으면 경고만 표시합니다(Plumivox를 실행할 때 나오는 안내, `app-update.log`, `app-update\setup-registration.json`의 `legacyPlugins`). Toolport 등록도 자동으로 고치지 않으므로, Toolport를 사용한다면 [Toolport 절](#toolport를-통해-사용하는-경우)을 참고하여 직접 확인하세요.

### 플러그인으로 설치하기(macOS·기존 방법)

macOS(Apple Silicon)는 Setup 배포를 시작할 때까지 이 방법을 사용합니다. Windows에서도 계속 사용할 수 있습니다.

GitHub 저장소 `main`의 최신 커밋에서 설치합니다. 사용자의 개발용 클론은 참조하지 않으며, 태그·커밋 SHA에 고정하지 않습니다.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local`은 마켓플레이스의 식별자입니다. 가져오는 곳은 위의 GitHub URL입니다. Windows x64용 `dist/plumivox.exe`와 Apple Silicon Mac용 `dist/plumivox`(ARM64)를 동봉하고 있으므로, 사용자가 의존성을 설치하거나 MCP 설정을 수동으로 추가할 필요가 없습니다.

개발용 CI에서는 Windows·macOS에서 검증한 산출물을 Cloudflare R2에 저장하고 배포 준비 시 전달합니다. [산출물 저장·가져오기 절차](docs/distribution.md)는 개발자용입니다. 플러그인 사용자에게는 R2 설정이나 자격 증명이 필요하지 않으며, 설치·업데이트는 계속 GitHub의 `main`에서 진행합니다.

업데이트할 때는 `main`의 최신 상태를 가져와 설치하고, Codex를 재시작하여 불러옵니다.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

목록의 `installed`에 표시되는 버전 번호를 확인합니다. 이미 설치된 플러그인은 업데이트 후 다시 `add`하지 말고, Codex를 재시작하여 새 MCP와 스킬을 불러옵니다.

GitHub를 추적하는 등록·업데이트 방법은 [Codex 플러그인 문서](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli)를 참조하세요.

### Toolport를 통해 사용하는 경우

MCP만 Toolport에 등록할 수 있습니다. Plumivox는 Toolport의 등록을 자동으로 수정하지 않으므로, 설치 방법에 맞춰 다음 stdio 서버를 추가·변경합니다. 개발용 클론이나 버전 번호가 붙은 설치 캐시는 지정하지 않습니다.

- 이름: `plumivox`
- 명령: Windows의 Setup 버전은 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`(`%LOCALAPPDATA%`는 실제 경로로 펼쳐서 지정. `current`는 업데이트로 내용이 바뀌어도 경로가 변하지 않습니다). 플러그인 버전은 `codex plugin marketplace list`로 `plumivox-local`의 가져온 루트를 확인하고, Windows는 `<가져온 루트>/dist/plumivox.exe`, macOS는 `<가져온 루트>/dist/plumivox`
- 인수: `--stdio`
- 작업 디렉터리: 사용자의 홈 디렉터리
- 요청 타임아웃: 300000밀리초(로그인이나 중계 준비 시간 포함)

Toolport에서 Codex가 사용하는 프로필에 활성화하고, `toolport_search_tools`의 `server="plumivox"`로 11개 도구를 확인합니다. `list_accounts`를 검색·호출하여 설정을 읽을 수 있게 된 뒤 직접 연결을 비활성화합니다. Setup 버전은 `codex mcp remove plumivox`와 `claude mcp remove plumivox --scope user`로 직접 등록을 해제합니다(한 번 등록을 마친 뒤에는 Setup으로 다시 설치할 때까지 자동으로 다시 등록하지 않습니다). 플러그인 버전은 Codex 사용자 설정 `config.toml`에 다음을 추가하고, 플러그인 본체는 활성 상태로 두어 설정용 스킬을 남겨 둡니다.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Codex를 재시작하면 직접 연결이 비활성화됩니다. 이후의 작업은 Toolport에서 도구를 검색하고, 반환된 실제 도구 이름을 `toolport_call_tool`에 전달합니다. API 키·Cookie·SSH 키는 Plumivox의 기존 설정에서 읽어 들이며, Toolport의 인수로 옮기지 않습니다.

플러그인 버전의 업데이트는 위의 `codex plugin marketplace upgrade plumivox-local`로 진행합니다. Toolport가 실행 중인 서버는 이미 읽어 들인 코드를 사용하므로, 업데이트 후 Toolport의 `plumivox`를 비활성화·재활성화하여 새 실행 파일을 불러옵니다. 가져온 루트가 바뀐 경우에는 등록한 명령도 변경해 주세요. 시작 시 Git 가져오기나 의존성 설치는 하지 않습니다.

## 초기 설정

처음 사용하는 경우에는 [설정 절차](docs/setup.md)로 진행하세요. Windows·macOS용 네이티브 설정 마법사와 Linux x86_64·aarch64용 정적 Rust VPS 설치 프로그램을 동봉합니다. 다른 CPU를 위해서는 Bash 호환 경로를 남겨 두었습니다. Codex에 "Plumivox를 설정해 줘"라고 요청하면 동봉된 `setup-plumivox` 스킬로도 안내받을 수 있습니다.

설정 파일은 저장소 밖에 둡니다. 신규 사용자는 Windows에서는 `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`, macOS에서는 `~/Library/Application Support/Kagayoi/Plumivox/settings.json`을 사용합니다. 기존 `~/dev/Secret/secrets.json`이 있는 사용자는 해당 파일의 `twitterapi_io`를 읽습니다. vava/blbl의 `release_posting`과 `blog_posting`도 유지합니다. 다른 위치를 사용하려면 `PLUMIVOX_SECRETS_FILE`을 지정합니다. 비밀 값을 대화나 MCP 인수로 전달할 필요는 없습니다.

선택된 설정 파일을 읽을 수 없는 경우에는 다른 설정으로 전환하지 않고 오류가 됩니다. 저장 위치를 변경한 경우에는 Codex를 재시작해 주세요. 최초 설정은 기존 키·같은 이름의 계정·공유 중계 설정을 덮어쓰지 않습니다. 기존 설정을 변경하는 방법은 [설정 절차](docs/setup.md)를 참조하세요.

[처음 설정하기](docs/setup.md) · [계정과 Cookie 설정](docs/configuration.md) · [Plumivox로 프록시 관리하기](docs/managed-proxy.md)

## 사용할 수 있는 도구

| 도구 | 내용 |
| --- | --- |
| `list_accounts` | 계정 이름과 읽기·게시 준비 상태. 외부 통신 없음 |
| `login_account` | 저장된 Cookie를 재사용하고, 없으면 설정된 자격 증명으로 로그인하여 암호화 저장 |
| `refresh_session` | 명시적으로 다시 로그인하여 게시용 세션을 갱신 |
| `get_my_posts` | 내 프로필 게시물. 답글 포함 여부와 페이지 넘김을 지정 가능 |
| `get_tweets` | 공개 게시물을 ID로 가져옴. 최대 20건 |
| `get_following_feed` | 팔로우한 계정의 공개 게시물을 가져와 날짜·시간 내림차순으로 정렬 |
| `search_tweets` | 키워드·기간을 지정하여 X 전체 또는 팔로우한 계정의 공개 게시물을 검색 |
| `get_proxy_status` | 이 MCP가 사용하는 공유 중계의 상태. 중계를 시작하지는 않음 |
| `start_proxy` | 설정한 주거용 회선 출구와 SSH 중계를 시작. 게시·로그인은 하지 않음 |
| `stop_proxy` | 사용 중인 공유 중계를 중지. 같은 중계를 쓰는 다른 채팅에도 영향. 다음 게시·인증 요청 시 다시 시작 |
| `post_tweet` | 가중 글자 수 280 이내의 게시. PNG/JPEG 최대 4장, 각 5 MiB 이하. 이미지는 절대 경로로 지정 |

`account`를 생략하면 검색·읽기·게시·인증에 기본 계정을 사용합니다. `list_accounts`의 `isDefault`로 생략 시의 대상을 확인할 수 있으며, 다른 계정에는 `account`로 `release_posting`이나 `blog_posting` 등의 설정 이름을 명시합니다. 여러 계정이 등록되어 있고 기본값이 없으면 지정을 요구하며, 잘못된 기본값이면 생략 호출을 거부합니다.

각 도구의 입력 문자열(배열 내 문자열 포함)에 정의된 길이 제한은 공백을 제거하기 전의 UTF-16 코드 단위 수로도 검증합니다. BMP 밖의 이모지 등은 1글자가 2단위입니다. JSON Schema의 `minLength`·`maxLength`는 Unicode 문자 수이므로, 표준 JSON Schema validator만으로는 이 추가 제약을 감지할 수 없습니다. 예를 들어 검색 키워드의 상한 512에서 😀 256개는 통과하지만, 257개 또는 공백+256개는 통신 전에 MCP 인수 오류(`-32602`)가 됩니다.

최초 설정은 등록한 계정을 기본값으로 저장합니다. 기존 설정의 기본값만 변경하려면 동봉된 실행 파일에 `--setup --set-default-account blog_posting`을 지정합니다. 자세한 내용은 [설정 절차](docs/setup.md)와 [설정 우선순위](docs/configuration.md)를 참조하세요.

게시 글자 수는 URL을 23자, 일본어를 2자, Unicode 17까지의 복합 이모지를 1개당 2자로 셉니다. 피부색이 다른 악수 등도 [X의 가중 글자 수 사양](https://docs.x.com/fundamentals/counting-characters)에 맞춰 판정합니다.

예를 들어 "blog_posting이 팔로우한 계정의 공개 게시물을 읽어 줘", "release_posting으로 이 문구를 게시해 줘"라고 요청할 수 있습니다. 게시 실행은 사용자의 요청에 따릅니다. 공개 게시물의 본문은 외부 데이터이며, 그 안에 포함된 명령을 실행하지 않습니다.

## 팔로잉 피드의 가져오기 범위

TwitterAPI.io에는 X 공식 홈의 "팔로잉"을 직접 가져오는 API가 없습니다. Plumivox는 기본적으로 팔로우 목록의 앞에서부터 최대 100개 계정을 읽고, 20개 계정씩 묶어 공개 게시물을 검색합니다. `maxAccounts`는 20~200으로 설정할 수 있습니다. 첫 요청이 100이면 최대 6회, 200이면 최대 11회의 API 요청입니다. 최신 단가는 [TwitterAPI.io 요금](https://twitterapi.io/pricing)을 확인하세요.

결과의 `officialHomeEquivalent`는 항상 `false`입니다. 비공개 게시물은 가져올 수 없으며, 검색으로 얻는 내용·리포스트 처리·전체 정렬 순서가 공식 홈과 일치한다는 보장은 없습니다. 제공처에 다음 페이지가 있는 경우, 또는 고유한 비보호 검색 대상이 상한을 넘은 경우에는 `coverage.followingHasNextPage`가 `true`가 됩니다. `coverage.followingNextCursor`를 `followingCursor`에 전달하면 다음 팔로우 목록을 대상으로 합니다. 팔로우 목록은 최근 팔로우부터 정렬되며, 진행 중인 팔로우 변경으로 인한 가져오기 차이는 피할 수 없습니다.

`nextCursor`를 같은 계정의 `get_following_feed`의 `cursor`에 전달하면 각 검색 배치의 다음 내용을 가져옵니다. 페이지 안은 날짜·시간 순이지만, 배치별 페이지 경계 때문에 페이지를 넘나드는 전체 시간순은 보장하지 않습니다. 팔로우한 계정은 첫 목록으로 고정하며, 검색을 이어 갈 때 다시 읽지 않습니다. 피드의 cursor도 API 키로 서명하여 검색 대상의 변조를 거부합니다. 이전 버전의 서명 없는 cursor나 API 키 변경 전의 cursor는 사용할 수 없으므로, cursor를 빼고 다시 가져와 주세요. `cursor`와 `followingCursor`는 서로 다른 페이지 넘김이며 동시에 지정할 수 없습니다.

[팔로우 목록의 공식 응답 사양](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings)에서는 보호 상태 필드를 확인할 수 없습니다. 같은 팔로우 목록 응답 안에서는 유효한 사용자 이름을 대소문자를 구분하지 않고 집계하며, 보호·공개·불명의 순서로 판정합니다. 보호로 판명된 대상을 제외한 첫 등장 순서로 상한까지 선택하고, 선택한 대상 중 판정할 수 없는 수를 `coverage.protectionStatusUnknownAccounts`로 반환합니다. `protectedAccountsExcluded`는 그 응답 안에서 보호로 판명된 고유 계정 수입니다. 중복·무효한 사용자 이름은 세지 않으며, 가져오기 대상 상한 밖의 보호 계정도 포함하므로 `maxAccounts`를 넘을 수 있습니다. 0이라고 해서 모든 대상이 공개 계정인 것은 아닙니다.

이전 구현이 발행한 서명된 cursor의 후속 요청은 검색 대상·건수뿐 아니라 미검색 범위·계속 여부도 이전 구현의 판정을 이어받으므로, 상한 밖의 대상을 건너뛸 수 있습니다. 새로운 보호 상태 집계·건수 계산·계속 판정으로 확실하게 다시 가져오려면 cursor를 빼 주세요.

게시물이나 팔로우 목록에 다음 내용이 있더라도, 제공처 cursor나 조합한 cursor가 길이 상한을 넘으면 계속할 수 없습니다. 이미 가져온 게시물과 `hasNextPage`는 유지하며, `nextCursor`는 `null`, `coverage.tweetContinuationAvailable`은 `false`, `tweetContinuationUnavailableReason`은 `cursor_too_long`이 됩니다. 팔로우 목록에서는 `followingNextCursor`와 `followingContinuationAvailable`·`followingContinuationUnavailableReason`을 확인하세요. cursor를 잘라내지 않으며, 이미 가져온 내용을 실패로 버리지 않습니다.

제공처가 한 번의 응답으로 `maxAccounts`를 넘는 고유한 비보호 대상을 반환한 경우, 상한 밖의 대상은 미검색 상태입니다. `followingHasNextPage`는 `true`, `followingContinuationUnavailableReason`은 `account_limit`이 되며, 팔로우 목록의 계속은 제공하지 않습니다(`followingContinuationAvailable: false`, `followingNextCursor: null`). 제공처의 다음 cursor로 미검색 대상을 건너뛰는 것을 막기 위해서입니다. `maxAccounts`가 200 미만이면 200까지의 범위에서 상한을 늘려 cursor 없이 다시 가져올 수 있습니다. 상한 200에서도 제공처가 과다 반환하는 경우에는 그 응답의 미검색 대상을 이 방법으로 가져올 수 없습니다. 같은 이름의 중복 행이나 보호·무효 이름만으로 상한을 넘은 경우에는 이 제한에 해당하지 않습니다. 선택된 대상의 게시물 후속 내용은 평소대로 가져올 수 있으며, 미검색 대상이 있는 상태도 이어받습니다.

## 키워드·기간 검색

"X 전체에서 10월 1일부터 7일까지의 음성 인식 게시물을 검색해 줘", "blog_posting이 팔로우한 계정에서 같은 조건으로 검색해 줘"라고 요청할 수 있습니다. 검색은 API 측에서 필터링하며, 게시물의 본문·작성자·날짜·시간·URL을 반환합니다. 의미 검색은 하지 않습니다.

`search_tweets`의 `scope`는 `all`(기본값, X 전체의 공개 검색) 또는 `following`(지정한 계정이 팔로우한 계정)입니다. `keyword`는 필수이며, 앞뒤 공백을 제거한 어구를 구문으로 검색합니다. `OR`이나 `from:`을 포함해도 검색 연산자로 취급하지 않습니다. 따옴표·백슬래시·제어 문자를 포함한 어구는 거부합니다.

`startTime`과 `endTime`은 생략 가능한, 시간대가 포함된 ISO 8601 초 단위 날짜·시간입니다. 시작을 포함하고 종료를 포함하지 않는 기간을 검색합니다. 날짜만으로는 지정하지 않으며, Codex가 요청받은 날짜와 시간대로부터 날짜·시간으로 변환합니다. 예를 들어 일본 시간 10월 1일부터 7일까지라면 다음 조건입니다.

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

검색은 새 게시물부터 가져옵니다. `nextCursor`를 다음 호출의 `cursor`에 전달하면 같은 조건으로 다음 내용을 가져올 수 있습니다. 계정·대상·키워드·기간·팔로우 대상 상한을 바꾸는 경우에는 cursor를 빼고 새 검색을 시작합니다. API 키를 바꾼 경우에도 기존 검색 cursor는 사용할 수 없습니다.

`following`은 `maxAccounts`(20~200, 기본값 100)와 `followingCursor`를 사용할 수 있습니다. `coverage.followingNextCursor`를 전달하면 다음 팔로우 목록을 같은 조건으로 검색합니다. 게시물 검색의 `cursor`와 동시에 지정할 수 없습니다. `all`에서는 이 두 인수를 지정하지 않습니다. 기존 `get_following_feed`와 검색의 cursor는 서로 호환되지 않습니다. 이전 서명 없는 피드 cursor를 포함한 이전 버전의 팔로우 대상 검색 cursor도 사용할 수 없으므로, cursor를 빼고 다시 검색해 주세요.

X 전체 검색은 페이지당 1회, 팔로우 대상 검색은 기존 피드와 같은 최대 6회(100명)/11회(200명)의 API 요청입니다. 비공개 게시물이나 검색 기간 내 모든 게시물의 망라는 보장하지 않으며, 가져오기 범위는 `coverage`로 반환합니다. 기간 연산자는 [TwitterAPI.io의 설명](https://twitterapi.io/blog/search-a-tweet)에 따라 UTC로 변환합니다.

## 게시와 인증에 실패한 경우

성공 시에는 게시물 ID와 X의 URL을 반환합니다. 게시물 작성 통신 실패나 게시물 ID 누락은 미확정으로 반환하며, 자동으로 다시 보내지 않습니다. 같은 문구를 다시 게시하도록 요청한 경우에는 각 호출마다 한 번 전송합니다. 기록·본문 해시를 이유로 중지하는 구조는 없습니다.

일반 API 요청은 45초, 이미지 업로드는 이미지 1개당 120초가 기본 통신 시간 범위입니다. 이미지 업로드만은 일시적인 실패에 한해 같은 이미지를 최대 1회 재시도합니다. 대상은 HTTP 502·503·504(250ms 대기)와 프록시·업스트림 장애나 원인 불명의 JSON 거부, 통신 끊김(1초 대기)이며, 인증·잔액·이미지 오류·중복·속도 제한·타임아웃은 재시도하지 않습니다. 대기와 재시도는 이미지 1개의 통신 시간 범위 안에 맞추며, 이전 이미지의 재전송이나 게시물 작성·로그인의 자동 재전송은 하지 않습니다. 이미지 실패 시에는 게시물을 전송하지 않으며, 게시물 작성 후 성공 여부가 불명확해진 경우에는 `POST_UNCERTAIN`을 반환합니다.

알려진 게시 처리 오류에는 `stage`(`input_validation`·`pre_send`·`image_upload`·`post_creation`), 게시물 작성 요청을 시도했는지 나타내는 `postAttempted`, 실행한 이미지 업로드 요청의 총수 `uploadAttempts`를 포함합니다. 계정·Cookie 부족이나 중계 준비 실패는 `pre_send`입니다. 알 수 없는 내부 오류와 MCP 입력 형식 거부에서는 이 값들을 반환하지 않습니다. `postAttempted: true`는 게시 성공을 의미하지 않습니다. 제공처의 원본 오류나 자격 증명은 반환하지 않습니다.

이미지 통신의 `API_UNAVAILABLE`에는 `failureKind`를 포함하여 `timeout`(시간 초과)·`network`(통신 장애)·`cancelled`(취소)를 구분합니다. 이 경우에도 이미지나 게시물을 자동으로 다시 보내지 않습니다.

API의 거부 오류(`API_REJECTED`·`API_HTTP_ERROR`)에는 `httpStatus`, 고정 분류의 배열 `rejectionReasons`, 분류 출처 `rejectionReasonSource`를 포함합니다. 게시물 작성의 5xx가 `POST_UNCERTAIN`이 되는 경우에도 이 진단 정보를 유지합니다. 분류 출처는 `provider_message`(제공처의 JSON 메시지에서 추정한 후보) 또는 `http_status`(HTTP 상태에 따른 분류)입니다. 후보는 원인의 확정이나 재전송 허가를 의미하지 않으며, 판별할 수 없는 문구는 `unknown`으로 합니다. 원본 응답문·임의의 오류 코드·request ID는 반환하지 않으며 저장하지도 않습니다.

`provider_message`의 후보는 `api_key`·`authentication`·`account_locked`·`account_suspended`·`rate_limit`·`credits`·`proxy`·`media`·`duplicate`·`upstream_timeout`·`upstream_unavailable`·`unknown`입니다. `http_status`에는 추가로 `invalid_request`(400)·`unauthorized`(401)·`forbidden`(403)·`endpoint_not_found`(404)가 있습니다. 예를 들어 이미지 업로드 시 `rejectionReasons: ["proxy", "upstream_timeout"]`과 `postAttempted: false`라면, 프록시·업스트림 타임아웃이 의심되며 게시물 작성 전에 멈췄다는 것을 알 수 있습니다. 2xx가 아닌 본문은 읽지 않으며, 기존 통신 상한·재시도 조건을 유지합니다.

API 응답이 10MiB를 넘으면 수신을 중단하고 실패를 반환합니다. 게시 응답에서 상한을 넘은 경우에도 이미 게시되었을 가능성이 있으므로 다시 보내지 않습니다.

저장된 Cookie의 대조 실패는 게시만 중지합니다. 공개 게시물 읽기·검색은 API 키와 사용자 이름만 사용하며, 저장된 Cookie를 읽지 않고 이용할 수 있습니다. 인증된 Cookie의 취득·갱신은 [설정 방법](docs/configuration.md)를 참조하세요.

`login_account`는 저장된 Cookie가 사용 가능한 형식이면 재사용하며, X에서 유효성을 검증했다고는 보고하지 않습니다. 만료나 설정 불일치가 확인된 경우에는 `refresh_session`을 호출합니다. 새 Cookie는 Plumivox의 저장 위치에 Windows에서는 DPAPI, macOS에서는 Keychain의 키로 암호화하여 저장하며, 실패한 게시를 자동으로 다시 보내지 않습니다.

저장 교체 전에 중단한 경우에는 `AUTH_CANCELLED`, 인증 갱신의 배타 제어를 유지할 수 없는 경우에는 `AUTH_LOCK_LOST`, 저장할 수 없는 Cookie는 `LOGIN_FAILED`로 반환합니다. 저장 교체를 지시한 뒤 결과를 확인할 수 없는 경우에는 `AUTH_SAVE_UNCERTAIN`이 되며, 이미 갱신되었을 가능성이 있습니다. `SESSION_STORAGE_UNAVAILABLE`은 저장 위치나 Windows의 DPAPI·macOS의 Keychain 등 저장 처리의 실패를 나타냅니다.

## 개발·검증

개발·빌드·테스트에도 Node.js는 필요하지 않습니다. Rust/Cargo의 `cargo xtask build`, `cargo xtask check`, `cargo xtask test-e2e`를 사용합니다. 이전 Node 런처를 지정한 등록은 위의 네이티브 실행으로 변경해 주세요.

Windows의 `cargo xtask build`는 Sigilune(Certum SimplySign의 클라우드 서명)으로 서명·서명 검증한 뒤 `dist/plumivox.exe`를 갱신합니다. 배포 CI도 Windows 산출물을 Sigilune으로 서명한 뒤 저장합니다. 서명에 필요한 개발 단말 설정과 CI의 Secrets는 [배포 산출물](docs/distribution.md#windows配布物のコード署名)을 참조하세요. Windows의 Setup은 `cargo xtask velopack-pack`으로 패키징하고, `cargo xtask test-setup-velopack-e2e`로 실제 설치에 영향을 주지 않고 격리 검증합니다. 배포는 수동 실행하는 `release-velopack.yml`만 수행합니다([Setup(Velopack) 배포](docs/distribution.md#setupvelopackの配信)).

[개발자용 빌드·검증 절차](docs/verification.md), [작업 규약](AGENTS.md), [시스템 설계](DESIGN.md)를 참조하세요.
