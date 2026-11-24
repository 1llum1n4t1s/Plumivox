# 개발·검증

개발·빌드·테스트에도 Node.js는 필요하지 않습니다. Rust/Cargo의 `cargo xtask build`, `cargo xtask check`, `cargo xtask test-e2e`를 사용합니다. 이전 Node 런처를 지정한 등록은 위의 네이티브 실행으로 변경해 주세요.

Windows의 `cargo xtask build`는 Sigilune(Certum SimplySign의 클라우드 서명)으로 서명·서명 검증한 뒤 `dist/plumivox.exe`를 갱신합니다. 배포 CI도 Windows 산출물을 Sigilune으로 서명한 뒤 저장합니다. 서명에 필요한 개발 단말 설정과 CI의 Secrets는 [배포 산출물](../../distribution.md#windows配布物のコード署名)을 참조하세요. Windows의 Setup은 `cargo xtask velopack-pack`으로 패키징하고, `cargo xtask test-setup-velopack-e2e`로 실제 설치에 영향을 주지 않고 격리 검증합니다. 배포는 `main`의 커밋에 붙인 경량 태그 `v<버전>`을 push할 때 시작되는 `distribution.yml`의 Setup 배포 job만 수행합니다([Setup(Velopack) 배포](../../distribution.md#setupvelopackの配信)).

[개발자용 빌드·검증 절차](../../verification.md), [작업 규약](../../../AGENTS.md), [시스템 설계](../../../DESIGN.md)를 참조하세요.
