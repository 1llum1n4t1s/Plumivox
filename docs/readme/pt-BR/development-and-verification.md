# Desenvolvimento e verificação

O Node.js também não é necessário para desenvolvimento, build e testes. Use `cargo xtask build`, `cargo xtask check` e `cargo xtask test-e2e` do Rust/Cargo. Registros que indicam o antigo launcher em Node devem ser alterados para a inicialização nativa descrita acima.

No Windows, `cargo xtask build` atualiza `dist/plumivox.exe` depois de assinar e verificar a assinatura com o Sigilune (assinatura em nuvem do Certum SimplySign). A CI de distribuição também assina os artefatos do Windows com o Sigilune antes de armazená-los. Para a configuração da máquina de desenvolvimento e os Secrets da CI necessários para a assinatura, consulte os [artefatos de distribuição](../../distribution.md#windows配布物のコード署名). O Setup do Windows é empacotado com `cargo xtask velopack-pack` e verificado de forma isolada com `cargo xtask test-setup-velopack-e2e`, sem tocar na instalação real. A distribuição é feita somente pelo job de distribuição do Setup de `distribution.yml`, iniciado por um push de uma tag leve `v<versão>` em um commit de `main` ([distribuição do Setup (Velopack)](../../distribution.md#setupvelopackの配信)).

Consulte o [procedimento de build e verificação para desenvolvedores](../../verification.md), as [regras de trabalho](../../../AGENTS.md) e o [design do sistema](../../../DESIGN.md).
