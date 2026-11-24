# Configuración inicial

Si lo usas por primera vez, sigue el [procedimiento de configuración](../../setup.md). Se incluyen asistentes de configuración nativos para Windows y macOS, y un instalador VPS estático en Rust para Linux x86_64 y aarch64. Para otras CPU se mantiene una ruta compatible con Bash. También puedes pedirle a Codex "configura Plumivox" y la skill incluida `setup-plumivox` te guiará.

El archivo de configuración se guarda fuera del repositorio. Los usuarios nuevos usan `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` en Windows y `~/Library/Application Support/Kagayoi/Plumivox/settings.json` en macOS. Los usuarios que ya tienen `~/dev/Secret/secrets.json` leen `twitterapi_io` de ese archivo. También se mantienen `release_posting` y `blog_posting` de vava/blbl. Para usar otra ubicación, indica `PLUMIVOX_SECRETS_FILE`. No es necesario pasar valores secretos en la conversación ni en los argumentos del MCP.

Si no se puede leer el archivo de configuración seleccionado, se produce un error sin cambiar a otra configuración. Si cambias la ubicación de guardado, reinicia Codex. La configuración inicial no sobrescribe claves existentes, cuentas con el mismo nombre ni la configuración del relé compartido. Para modificar una configuración existente, consulta el [procedimiento de configuración](../../setup.md).

[Primera configuración](../../setup.md) · [Configuración de cuentas y cookies](../../configuration.md) · [Gestionar el proxy con Plumivox](../../managed-proxy.md)
