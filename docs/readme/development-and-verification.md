# Development and verification

Node.js is not required for development, building, or testing either. Use Rust/Cargo's `cargo xtask build`, `cargo xtask check`, and `cargo xtask test-e2e`. Change any registration that specifies the old Node launcher to the native startup described above.

On Windows, `cargo xtask build` updates `dist/plumivox.exe` after signing and signature verification with Sigilune (Certum SimplySign cloud signing). Distribution CI also signs Windows artifacts with Sigilune before storing them. For the development machine settings and CI Secrets required for signing, see [distribution artifacts](../distribution.md#windows配布物のコード署名). Windows Setup is packaged with `cargo xtask velopack-pack` and verified in isolation with `cargo xtask test-setup-velopack-e2e` without touching a real installation. Distribution is performed only by the Setup distribution job of `distribution.yml`, which starts on a push of a lightweight `v<version>` tag on a commit of `main` ([Distributing Setup (Velopack)](../distribution.md#setupvelopackの配信)).

See the [build and verification procedure for developers](../verification.md), the [working conventions](../../AGENTS.md), and the [system design](../../DESIGN.md).
