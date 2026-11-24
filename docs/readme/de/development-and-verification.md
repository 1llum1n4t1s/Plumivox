# Entwicklung und Verifizierung

Auch für Entwicklung, Build und Tests wird kein Node.js benötigt. Verwendet werden `cargo xtask build`, `cargo xtask check` und `cargo xtask test-e2e` mit Rust/Cargo. Registrierungen, die den alten Node-Launcher angeben, stellen Sie bitte auf den oben beschriebenen nativen Start um.

`cargo xtask build` unter Windows aktualisiert `dist/plumivox.exe` nach dem Signieren und der Signaturprüfung mit Sigilune (Cloud-Signatur über Certum SimplySign). Auch die Auslieferungs-CI signiert die Windows-Artefakte mit Sigilune, bevor sie sie ablegt. Die für das Signieren nötige Konfiguration des Entwicklungsrechners und die CI-Secrets finden Sie unter [Auslieferungsartefakte](../../distribution.md#windows配布物のコード署名). Das Windows-Setup wird mit `cargo xtask velopack-pack` gepackt und mit `cargo xtask test-setup-velopack-e2e` isoliert verifiziert, ohne eine echte Installation zu berühren. Die Auslieferung erfolgt ausschließlich über den Setup-Auslieferungs-Job von `distribution.yml`, der bei einem Push eines leichten `v<Version>`-Tags auf einem Commit von `main` startet ([Auslieferung des Setups (Velopack)](../../distribution.md#setupvelopackの配信)).

Siehe [Build- und Verifizierungsanleitung für Entwickler](../../verification.md), [Arbeitsregeln](../../../AGENTS.md) und [Systemdesign](../../../DESIGN.md).
