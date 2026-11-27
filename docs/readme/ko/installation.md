# 설치

## Windows: Setup으로 설치하기(권장)

1. [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)를 다운로드하여 실행합니다. 사용자 단위 설치이므로 관리자 권한이 필요하지 않습니다. Setup에는 본인의 코드 서명(`Open Source Developer Yuichiro Shinozaki`)이 포함되어 있습니다.
2. `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`에 설치되며, PATH에 `claude`·`codex` CLI가 있으면 Claude Code(사용자 단위)와 Codex에 MCP 서버 `plumivox`(인수 `--stdio`)를 자동으로 등록합니다. CLI를 찾지 못한 경우에는 나중에 CLI를 설치한 뒤 다음 로그온 시 또는 시작 메뉴의 Plumivox를 실행하면 등록됩니다.
3. 설치 직후 화면의 안내에 따라 터미널에서 `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup`을 실행하여 [초기 설정](../../../README.ko.md#초기-설정)을 진행합니다. Claude Code·Codex는 새 세션부터 사용할 수 있습니다.

업데이트는 자동입니다. MCP가 실행되는 동안 새 버전을 R2(`https://plumivox-updates.kagayoi.com/velopack/`)에서 받아 오고, PC에 로그온할 때 Plumivox(MCP·공유 중계)가 실행 중이지 않은 경우에만 적용합니다. 실행 중이면 다음 로그온으로 미루며 강제 종료하지 않습니다. 적용 전에 서명자를 확인하고, 다르면 적용하지 않습니다. 자동 업데이트를 끄려면 `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json`을 `{"auto_update": false}`로 설정합니다. 동작 기록은 같은 폴더의 `app-update.log`에 있습니다.

제거하려면 "설정" → "앱" 목록에서 Plumivox를 선택합니다. 자신을 가리키는 Claude Code·Codex의 `plumivox` 등록과 로그온 시 업데이트 등록만 해제하고, 설정·세션 데이터 폴더(`%LOCALAPPDATA%\Kagayoi\Plumivox`)는 남겨 둡니다. Toolport의 `plumivox` 항목은 남으므로 Toolport에서 직접 삭제하세요. 등록만 먼저 해제하려면 `plumivox.exe --uninstall-setup`으로 예정 내용을 확인하고 `--uninstall-setup --apply`로 실행합니다.

`plumivox.exe`의 그 밖의 명령줄 옵션: `--setup`(초기 설정), `--uninstall-setup`(등록 해제), `--rejection-log on|off|status|show|clear`(가림 처리한 거부 문구의 기간 한정 로컬 기록, 기본은 꺼짐, [로컬 거부 기록](when-posting-or-authentication-fails.md#로컬-거부-기록) 참조). 기록은 제거해도 남으며 `--rejection-log clear`로 삭제합니다.

플러그인(아래)에서 옮겨 오는 경우: Setup은 기존 플러그인의 `dist` 등을 가리키는 같은 이름의 `plumivox` 등록을 설치 위치를 가리키는 등록으로 교체합니다. 플러그인 자체가 가진 MCP(`plumivox@plumivox-local`)는 별도의 등록이므로, Plumivox가 두 경로로 시작되지 않도록 비활성화하거나 플러그인을 제거하세요. Codex에서는 [Toolport 절](#toolport를-통해-사용하는-경우)과 같은 `enabled = false`를 추가하고(또는 플러그인을 제거), Claude Code에서는 `/plugin`에서 플러그인을 비활성화하거나 삭제합니다. Codex는 같은 이름의 등록을 플러그인의 MCP보다 우선하므로 Setup 등록이 있는 동안에는 하나만 시작되어야 하지만, Claude Code는 둘 다 시작할 수 있습니다. Setup은 Codex·Claude Code 설정을 수정하지 않으며, 플러그인이 계속 활성화되어 있으면 경고만 표시합니다(Plumivox를 실행할 때 나오는 안내, `app-update.log`, `app-update\setup-registration.json`의 `legacyPlugins`). Toolport 등록도 자동으로 고치지 않으므로, Toolport를 사용한다면 [Toolport 절](#toolport를-통해-사용하는-경우)을 참고하여 직접 확인하세요. Setup으로 옮길 때는 Toolport의 `plumivox` 명령을 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`(실제 경로로 펼침)로 바꾸세요. 실행 파일을 다른 위치로 복사하지 마세요. 복사본은 업데이트되지 않습니다. Toolport가 다른 실행 파일로 `plumivox`를 제공하는 경우에도 Setup은 직접 등록을 생략한 채 경고합니다(안내와 `app-update.log`, Toolport 쪽 명령은 기록하지 않음). 명령을 위 경로로 바꾸면 시작할 때마다 하는 판정으로 경고가 사라집니다.

## 플러그인으로 설치하기(macOS·기존 방법)

macOS(Apple Silicon)는 Setup 배포를 시작할 때까지 이 방법을 사용합니다. Windows에서도 계속 사용할 수 있습니다.

GitHub 저장소 `main`의 최신 커밋에서 설치합니다. 사용자의 개발용 클론은 참조하지 않으며, 태그·커밋 SHA에 고정하지 않습니다.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local`은 마켓플레이스의 식별자입니다. 가져오는 곳은 위의 GitHub URL입니다. Windows x64용 `dist/plumivox.exe`와 Apple Silicon Mac용 `dist/plumivox`(ARM64)를 동봉하고 있으므로, 사용자가 의존성을 설치하거나 MCP 설정을 수동으로 추가할 필요가 없습니다.

개발용 CI에서는 Windows·macOS에서 검증한 산출물을 Cloudflare R2에 저장하고 배포 준비 시 전달합니다. [산출물 저장·가져오기 절차](../../distribution.md)는 개발자용입니다. 플러그인 사용자에게는 R2 설정이나 자격 증명이 필요하지 않으며, 설치·업데이트는 계속 GitHub의 `main`에서 진행합니다.

업데이트할 때는 `main`의 최신 상태를 가져와 설치하고, Codex를 재시작하여 불러옵니다.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

목록의 `installed`에 표시되는 버전 번호를 확인합니다. 이미 설치된 플러그인은 업데이트 후 다시 `add`하지 말고, Codex를 재시작하여 새 MCP와 스킬을 불러옵니다.

GitHub를 추적하는 등록·업데이트 방법은 [Codex 플러그인 문서](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli)를 참조하세요.

## Toolport를 통해 사용하는 경우

MCP만 Toolport에 등록할 수 있습니다. Plumivox는 Toolport의 등록을 자동으로 수정하지 않으므로, 설치 방법에 맞춰 다음 stdio 서버를 추가·변경합니다. 개발용 클론이나 버전 번호가 붙은 설치 캐시는 지정하지 않습니다.

- 이름: `plumivox`
- 명령: Windows의 Setup 버전은 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`(`%LOCALAPPDATA%`는 실제 경로로 펼쳐서 지정. `current`는 업데이트로 내용이 바뀌어도 경로가 변하지 않습니다). 플러그인 버전은 `codex plugin marketplace list`로 `plumivox-local`의 가져온 루트를 확인하고, Windows는 `<가져온 루트>/dist/plumivox.exe`, macOS는 `<가져온 루트>/dist/plumivox`
- 인수: `--stdio`
- 작업 디렉터리: 사용자의 홈 디렉터리
- 요청 타임아웃: 300000밀리초(로그인이나 중계 준비 시간 포함)

Toolport에서 Codex가 사용하는 프로필에 활성화하고, `toolport_search_tools`의 `server="plumivox"`로 11개 도구를 확인합니다. `list_accounts`를 검색·호출하여 설정을 읽을 수 있게 된 뒤 직접 연결을 비활성화합니다. Setup 버전은 클라이언트가 이미 Toolport 게이트웨이를 사용하고 해당 프로필에서 `plumivox`가 활성화되어 있으면 직접 등록을 자동으로 생략합니다(`app-update.json`의 `mcp_registration`으로 제어하며 기본값은 `auto`, 그 외 `direct`·`off`. 자세한 내용은 [Toolport configuration](../configuration.md)). Toolport를 나중에 추가한 경우에는 기존 직접 등록을 `codex mcp remove plumivox`와 `claude mcp remove plumivox --scope user`로 직접 해제하세요. Setup이 직접 등록을 스스로 해제하는 일은 없습니다(한 번 등록을 마친 뒤에는 Setup으로 다시 설치하기 전까지 자동으로 다시 등록하지 않습니다. 나중에 되돌리는 것은 Setup이 생략한 클라이언트뿐이며, 예를 들어 Toolport 게이트웨이가 클라이언트에서 사라지면 다음에 Plumivox가 시작될 때나 로그인할 때 직접 등록합니다). 판정은 OS와 무관하게 Windows·macOS·Linux에서 같은 규칙으로 수행하며, 그 밖의 OS에서는 판정하지 않고 항상 직접 등록합니다(Setup 배포 자체는 현재 Windows뿐입니다). Toolport가 다른 실행 파일(기존 플러그인의 `dist`나 복사본 등)로 `plumivox`를 제공하는 경우에도 직접 등록은 생략하고 `toolport-foreign`으로 기록하며 경고합니다. Toolport의 `plumivox` 명령을 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`로 바꿔 주세요. 판정은 시작할 때마다 다시 합니다. 플러그인 버전은 Codex 사용자 설정 `config.toml`에 다음을 추가하고, 플러그인 본체는 활성 상태로 두어 설정용 스킬을 남겨 둡니다.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Codex를 재시작하면 직접 연결이 비활성화됩니다. 이후의 작업은 Toolport에서 도구를 검색하고, 반환된 실제 도구 이름을 `toolport_call_tool`에 전달합니다. API 키·Cookie·SSH 키는 Plumivox의 기존 설정에서 읽어 들이며, Toolport의 인수로 옮기지 않습니다.

플러그인 버전을 업데이트할 때는 실행 파일이 잠겨 교체되지 않는 일을 막기 위해, 먼저 Toolport에서 `plumivox`를 비활성화하고, 위의 `codex plugin marketplace upgrade plumivox-local`을 실행한 뒤, 다시 활성화하여 새 실행 파일을 불러옵니다. 가져온 루트가 바뀐 경우에는 등록한 명령도 변경해 주세요. 시작 시 Git 가져오기나 의존성 설치는 하지 않습니다.
