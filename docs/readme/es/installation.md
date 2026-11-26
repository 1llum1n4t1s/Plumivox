# Instalación

## Windows: instalar con Setup (recomendado)

1. Descarga y ejecuta [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). Se instala por usuario y no requiere permisos de administrador. Setup lleva la firma de código del autor (`Open Source Developer Yuichiro Shinozaki`).
2. Se instala en `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` y, si las CLI `claude` o `codex` están en el PATH, registra automáticamente el servidor MCP `plumivox` (argumento `--stdio`) en Claude Code (por usuario) y en Codex. Si no se encontraron las CLI, se registrará después de instalarlas, en el siguiente inicio de sesión o al iniciar Plumivox desde el menú Inicio.
3. Siguiendo las indicaciones de la pantalla que aparece justo después de la instalación, ejecuta `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` en una terminal para realizar la [configuración inicial](../../../README.es.md#configuración-inicial). Claude Code y Codex pueden usarlo a partir de una nueva sesión.

Las actualizaciones son automáticas. Mientras el MCP está en ejecución, descarga la nueva versión desde R2 (`https://plumivox-updates.kagayoi.com/velopack/`) y la aplica al iniciar sesión en el equipo, solo cuando Plumivox (MCP y relé compartido) no se está ejecutando. Si está en ejecución, la aplicación se pospone al siguiente inicio de sesión y no se fuerza su cierre. Antes de aplicarla verifica el firmante y, si no coincide, no la aplica. Para desactivar las actualizaciones automáticas, deja `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` como `{"auto_update": false}`. El registro de actividad está en `app-update.log`, en la misma carpeta.

Para desinstalar, selecciona Plumivox en la lista de "Configuración" → "Aplicaciones". Solo se eliminan los registros `plumivox` de Claude Code y Codex que apuntan a esta instalación y el registro de actualización al iniciar sesión; la carpeta de datos de configuración y sesiones (`%LOCALAPPDATA%\Kagayoi\Plumivox`) se conserva. Para quitar solo los registros por adelantado, revisa el plan con `plumivox.exe --uninstall-setup` y ejecútalo con `--uninstall-setup --apply`.

Si migras desde el plugin (ver más abajo): Setup sustituye el registro `plumivox` del mismo nombre que apunta a `dist` u otra ubicación del plugin anterior por uno que apunta a la ubicación de instalación. El MCP que aporta el propio plugin (`plumivox@plumivox-local`) es un registro distinto; para que Plumivox no se inicie por dos vías, desactívalo o elimina el plugin. En Codex, añade el mismo `enabled = false` que en [sección de Toolport](#uso-a-través-de-toolport) (o elimina el plugin); en Claude Code, desactiva o desinstala el plugin desde `/plugin`. Codex da prioridad al registro del mismo nombre sobre el MCP del plugin, así que mientras exista el registro de Setup debería iniciar solo uno, pero Claude Code puede iniciar ambos. Setup nunca modifica la configuración de Codex ni de Claude Code; si el plugin sigue activado, solo muestra una advertencia (en la guía que aparece al iniciar Plumivox, en `app-update.log` y como `legacyPlugins` en `app-update\setup-registration.json`). Tampoco reescribe automáticamente los registros de Toolport; si usas Toolport, revísalo tú mismo como se explica en [sección de Toolport](#uso-a-través-de-toolport).

## Instalar como plugin (macOS, método anterior)

En macOS (Apple Silicon) se usa este método hasta que empiece la distribución de Setup. En Windows también sigue disponible.

Se instala desde el último commit de `main` del repositorio de GitHub. No hace referencia al clon de desarrollo del usuario ni se fija a una etiqueta o SHA de commit.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` es el identificador del marketplace. El origen es la URL de GitHub anterior. Incluye `dist/plumivox.exe` para Windows x64 y `dist/plumivox` (ARM64) para Mac con Apple Silicon, por lo que el usuario no necesita instalar dependencias ni añadir manualmente la configuración del MCP.

En la CI de desarrollo, los artefactos verificados en Windows y macOS se guardan en Cloudflare R2 y se entregan durante la preparación de la distribución. El [procedimiento para guardar y obtener artefactos](../../distribution.md) está dirigido a desarrolladores. Los usuarios del plugin no necesitan configuración ni credenciales de R2; la instalación y las actualizaciones se siguen haciendo desde `main` en GitHub.

Para actualizar, obtén e instala el estado más reciente de `main` y reinicia Codex para cargarlo.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Comprueba el número de versión que aparece en `installed` de la lista. No vuelvas a ejecutar `add` en un plugin ya instalado tras actualizarlo; reinicia Codex para cargar el nuevo MCP y las nuevas skills.

Para registrar y actualizar siguiendo GitHub, consulta la [documentación de plugins de Codex](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

## Uso a través de Toolport

Puedes registrar solo el MCP en Toolport. Plumivox no modifica automáticamente el registro de Toolport, así que añade o modifica el siguiente servidor stdio según el método de instalación. No indiques el clon de desarrollo ni una caché de instalación con número de versión.

- Nombre: `plumivox`
- Comando: en la versión Setup de Windows, `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (indica `%LOCALAPPDATA%` expandido a la ruta real; la ruta de `current` no cambia aunque su contenido se reemplace al actualizar). En la versión plugin, comprueba con `codex plugin marketplace list` la raíz obtenida de `plumivox-local`; en Windows, `<raíz obtenida>/dist/plumivox.exe`, y en macOS, `<raíz obtenida>/dist/plumivox`
- Argumentos: `--stdio`
- Directorio de trabajo: el directorio personal del usuario
- Tiempo de espera de solicitud: 300000 milisegundos (incluye el tiempo de inicio de sesión y de preparación del relé)

Actívalo en el perfil de Toolport que usa Codex y comprueba las 11 herramientas con `server="plumivox"` en `toolport_search_tools`. Busca y llama a `list_accounts` y, una vez que la configuración se haya cargado, desactiva la conexión directa. En la versión Setup, en una instalación nueva el registro directo se omite automáticamente si el cliente ya usa la pasarela de Toolport y `plumivox` está habilitado en el perfil que usa (se controla con `mcp_registration` en `app-update.json`: `auto` por defecto, `direct` u `off`; consulta [Toolport configuration](../configuration.md)). Si añades Toolport más tarde, quita tú mismo los registros directos existentes con `codex mcp remove plumivox` y `claude mcp remove plumivox --scope user`; Setup nunca los quita por su cuenta (una vez completado el registro, no se vuelve a registrar automáticamente hasta que reinstales con Setup; solo se retoman los clientes que Setup omitió, por ejemplo si la pasarela de Toolport desaparece más tarde del cliente, en el siguiente inicio de Plumivox o inicio de sesión). En sistemas distintos de Windows no se comprueba nada y siempre se registra directamente. En la versión plugin, añade lo siguiente a la configuración de usuario de Codex `config.toml`, mantén el plugin activado y conserva la skill de configuración.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Al reiniciar Codex, la conexión directa queda desactivada. A partir de entonces, busca las herramientas en Toolport y pasa el nombre real de la herramienta devuelto a `toolport_call_tool`. La clave de API, las cookies y las claves SSH se leen de la configuración existente de Plumivox y no se trasladan a los argumentos de Toolport.

La versión plugin se actualiza con el `codex plugin marketplace upgrade plumivox-local` anterior. Los servidores que Toolport tiene en ejecución usan el código ya cargado, así que, tras actualizar, desactiva y vuelve a activar `plumivox` en Toolport para cargar el nuevo ejecutable. Si cambia la raíz obtenida, modifica también el comando registrado. No se realiza ninguna obtención de Git ni instalación de dependencias al iniciar.
