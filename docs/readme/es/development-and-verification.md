# Desarrollo y verificación

El desarrollo, la compilación y las pruebas tampoco requieren Node.js. Se usan `cargo xtask build`, `cargo xtask check` y `cargo xtask test-e2e` de Rust/Cargo. Los registros que indican el antiguo lanzador de Node deben cambiarse al inicio nativo descrito arriba.

En Windows, `cargo xtask build` actualiza `dist/plumivox.exe` tras firmarlo con Sigilune (firma en la nube de Certum SimplySign) y verificar la firma. La CI de distribución también firma los artefactos de Windows con Sigilune antes de guardarlos. Para la configuración del equipo de desarrollo y los Secrets de la CI necesarios para la firma, consulta los [artefactos de distribución](../../distribution.md#windows配布物のコード署名). El Setup de Windows se empaqueta con `cargo xtask velopack-pack` y se verifica de forma aislada, sin tocar la instalación real, con `cargo xtask test-setup-velopack-e2e`. La distribución la realiza únicamente el job de distribución de Setup de `distribution.yml`, que se inicia con un push de una etiqueta ligera `v<versión>` en un commit de `main` ([distribución de Setup (Velopack)](../../distribution.md#setupvelopackの配信)).

Consulta el [procedimiento de compilación y verificación para desarrolladores](../../verification.md), las [normas de trabajo](../../../AGENTS.md) y el [diseño del sistema](../../../DESIGN.md).
