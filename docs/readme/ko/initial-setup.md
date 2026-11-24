# 초기 설정

처음 사용하는 경우에는 [설정 절차](../../setup.md)로 진행하세요. Windows·macOS용 네이티브 설정 마법사와 Linux x86_64·aarch64용 정적 Rust VPS 설치 프로그램을 동봉합니다. 다른 CPU를 위해서는 Bash 호환 경로를 남겨 두었습니다. Codex에 "Plumivox를 설정해 줘"라고 요청하면 동봉된 `setup-plumivox` 스킬로도 안내받을 수 있습니다.

설정 파일은 저장소 밖에 둡니다. 신규 사용자는 Windows에서는 `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`, macOS에서는 `~/Library/Application Support/Kagayoi/Plumivox/settings.json`을 사용합니다. 기존 `~/dev/Secret/secrets.json`이 있는 사용자는 해당 파일의 `twitterapi_io`를 읽습니다. vava/blbl의 `release_posting`과 `blog_posting`도 유지합니다. 다른 위치를 사용하려면 `PLUMIVOX_SECRETS_FILE`을 지정합니다. 비밀 값을 대화나 MCP 인수로 전달할 필요는 없습니다.

선택된 설정 파일을 읽을 수 없는 경우에는 다른 설정으로 전환하지 않고 오류가 됩니다. 저장 위치를 변경한 경우에는 Codex를 재시작해 주세요. 최초 설정은 기존 키·같은 이름의 계정·공유 중계 설정을 덮어쓰지 않습니다. 기존 설정을 변경하는 방법은 [설정 절차](../../setup.md)를 참조하세요.

[처음 설정하기](../../setup.md) · [계정과 Cookie 설정](../../configuration.md) · [Plumivox로 프록시 관리하기](../../managed-proxy.md)
