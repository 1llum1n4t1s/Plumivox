[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Servidor MCP local / plugin de Codex que, mediante TwitterAPI.io, permite publicar en X, buscar por palabra clave y periodo, y leer las publicaciones públicas de las cuentas seguidas.

## Requisitos

- Windows x64 o un Mac con Apple Silicon (macOS ARM64). El núcleo MCP es un ejecutable nativo escrito en Rust, por lo que no es necesario instalar Node.js ni Rust.
- La configuración inicial en Windows y macOS usa el ejecutable incluido. No se necesita PowerShell 7 (`pwsh`).
- Una clave de API de TwitterAPI.io. El uso de la API se factura por consumo en ese servicio.
- Para publicar, las cookies de inicio de sesión v2 de la cuenta de destino y un proxy. Además de un proxy externo existente, en Windows y macOS la salida por línea residencial y el relé SSH integrados en Plumivox pueden compartirse entre varios chats. Es necesario preparar una vez el servidor de relé público.

## Instalación

### Windows: instalar con Setup (recomendado)

1. Descarga y ejecuta [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). Se instala por usuario y no requiere permisos de administrador. Setup lleva la firma de código del autor (`Open Source Developer Yuichiro Shinozaki`).
2. Se instala en `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` y, si las CLI `claude` o `codex` están en el PATH, registra automáticamente el servidor MCP `plumivox` (argumento `--stdio`) en Claude Code (por usuario) y en Codex. Si no se encontraron las CLI, se registrará después de instalarlas, en el siguiente inicio de sesión o al iniciar Plumivox desde el menú Inicio.
3. Siguiendo las indicaciones de la pantalla que aparece justo después de la instalación, ejecuta `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` en una terminal para realizar la [configuración inicial](#configuración-inicial). Claude Code y Codex pueden usarlo a partir de una nueva sesión.

Las actualizaciones son automáticas. Mientras el MCP está en ejecución, descarga la nueva versión desde R2 (`https://plumivox-updates.kagayoi.com/velopack/`) y la aplica al iniciar sesión en el equipo, solo cuando Plumivox (MCP y relé compartido) no se está ejecutando. Si está en ejecución, la aplicación se pospone al siguiente inicio de sesión y no se fuerza su cierre. Antes de aplicarla verifica el firmante y, si no coincide, no la aplica. Para desactivar las actualizaciones automáticas, deja `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` como `{"auto_update": false}`. El registro de actividad está en `app-update.log`, en la misma carpeta.

Para desinstalar, selecciona Plumivox en la lista de "Configuración" → "Aplicaciones". Solo se eliminan los registros `plumivox` de Claude Code y Codex que apuntan a esta instalación y el registro de actualización al iniciar sesión; la carpeta de datos de configuración y sesiones (`%LOCALAPPDATA%\Kagayoi\Plumivox`) se conserva. Para quitar solo los registros por adelantado, revisa el plan con `plumivox.exe --uninstall-setup` y ejecútalo con `--uninstall-setup --apply`.

Si migras desde el plugin (ver más abajo): Setup sustituye el registro `plumivox` del mismo nombre que apunta a `dist` u otra ubicación del plugin anterior por uno que apunta a la ubicación de instalación. El MCP que aporta el propio plugin (`plumivox@plumivox-local`) es un registro distinto; para que Plumivox no se inicie por dos vías, desactívalo o elimina el plugin. En Codex, añade el mismo `enabled = false` que en [sección de Toolport](#uso-a-través-de-toolport) (o elimina el plugin); en Claude Code, desactiva o desinstala el plugin desde `/plugin`. Codex da prioridad al registro del mismo nombre sobre el MCP del plugin, así que mientras exista el registro de Setup debería iniciar solo uno, pero Claude Code puede iniciar ambos. Setup nunca modifica la configuración de Codex ni de Claude Code; si el plugin sigue activado, solo muestra una advertencia (en la guía que aparece al iniciar Plumivox, en `app-update.log` y como `legacyPlugins` en `app-update\setup-registration.json`). Tampoco reescribe automáticamente los registros de Toolport; si usas Toolport, revísalo tú mismo como se explica en [sección de Toolport](#uso-a-través-de-toolport).

### Instalar como plugin (macOS, método anterior)

En macOS (Apple Silicon) se usa este método hasta que empiece la distribución de Setup. En Windows también sigue disponible.

Se instala desde el último commit de `main` del repositorio de GitHub. No hace referencia al clon de desarrollo del usuario ni se fija a una etiqueta o SHA de commit.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` es el identificador del marketplace. El origen es la URL de GitHub anterior. Incluye `dist/plumivox.exe` para Windows x64 y `dist/plumivox` (ARM64) para Mac con Apple Silicon, por lo que el usuario no necesita instalar dependencias ni añadir manualmente la configuración del MCP.

En la CI de desarrollo, los artefactos verificados en Windows y macOS se guardan en Cloudflare R2 y se entregan durante la preparación de la distribución. El [procedimiento para guardar y obtener artefactos](docs/distribution.md) está dirigido a desarrolladores. Los usuarios del plugin no necesitan configuración ni credenciales de R2; la instalación y las actualizaciones se siguen haciendo desde `main` en GitHub.

Para actualizar, obtén e instala el estado más reciente de `main` y reinicia Codex para cargarlo.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Comprueba el número de versión que aparece en `installed` de la lista. No vuelvas a ejecutar `add` en un plugin ya instalado tras actualizarlo; reinicia Codex para cargar el nuevo MCP y las nuevas skills.

Para registrar y actualizar siguiendo GitHub, consulta la [documentación de plugins de Codex](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

### Uso a través de Toolport

Puedes registrar solo el MCP en Toolport. Plumivox no modifica automáticamente el registro de Toolport, así que añade o modifica el siguiente servidor stdio según el método de instalación. No indiques el clon de desarrollo ni una caché de instalación con número de versión.

- Nombre: `plumivox`
- Comando: en la versión Setup de Windows, `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (indica `%LOCALAPPDATA%` expandido a la ruta real; la ruta de `current` no cambia aunque su contenido se reemplace al actualizar). En la versión plugin, comprueba con `codex plugin marketplace list` la raíz obtenida de `plumivox-local`; en Windows, `<raíz obtenida>/dist/plumivox.exe`, y en macOS, `<raíz obtenida>/dist/plumivox`
- Argumentos: `--stdio`
- Directorio de trabajo: el directorio personal del usuario
- Tiempo de espera de solicitud: 300000 milisegundos (incluye el tiempo de inicio de sesión y de preparación del relé)

Actívalo en el perfil de Toolport que usa Codex y comprueba las 11 herramientas con `server="plumivox"` en `toolport_search_tools`. Busca y llama a `list_accounts` y, una vez que la configuración se haya cargado, desactiva la conexión directa. En la versión Setup, quita los registros directos con `codex mcp remove plumivox` y `claude mcp remove plumivox --scope user` (una vez quitados, no se vuelven a registrar automáticamente hasta que reinstales con Setup). En la versión plugin, añade lo siguiente a la configuración de usuario de Codex `config.toml`, mantén el plugin activado y conserva la skill de configuración.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Al reiniciar Codex, la conexión directa queda desactivada. A partir de entonces, busca las herramientas en Toolport y pasa el nombre real de la herramienta devuelto a `toolport_call_tool`. La clave de API, las cookies y las claves SSH se leen de la configuración existente de Plumivox y no se trasladan a los argumentos de Toolport.

La versión plugin se actualiza con el `codex plugin marketplace upgrade plumivox-local` anterior. Los servidores que Toolport tiene en ejecución usan el código ya cargado, así que, tras actualizar, desactiva y vuelve a activar `plumivox` en Toolport para cargar el nuevo ejecutable. Si cambia la raíz obtenida, modifica también el comando registrado. No se realiza ninguna obtención de Git ni instalación de dependencias al iniciar.

## Configuración inicial

Si lo usas por primera vez, sigue el [procedimiento de configuración](docs/setup.md). Se incluyen asistentes de configuración nativos para Windows y macOS, y un instalador VPS estático en Rust para Linux x86_64 y aarch64. Para otras CPU se mantiene una ruta compatible con Bash. También puedes pedirle a Codex "configura Plumivox" y la skill incluida `setup-plumivox` te guiará.

El archivo de configuración se guarda fuera del repositorio. Los usuarios nuevos usan `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` en Windows y `~/Library/Application Support/Kagayoi/Plumivox/settings.json` en macOS. Los usuarios que ya tienen `~/dev/Secret/secrets.json` leen `twitterapi_io` de ese archivo. También se mantienen `release_posting` y `blog_posting` de vava/blbl. Para usar otra ubicación, indica `PLUMIVOX_SECRETS_FILE`. No es necesario pasar valores secretos en la conversación ni en los argumentos del MCP.

Si no se puede leer el archivo de configuración seleccionado, se produce un error sin cambiar a otra configuración. Si cambias la ubicación de guardado, reinicia Codex. La configuración inicial no sobrescribe claves existentes, cuentas con el mismo nombre ni la configuración del relé compartido. Para modificar una configuración existente, consulta el [procedimiento de configuración](docs/setup.md).

[Primera configuración](docs/setup.md) · [Configuración de cuentas y cookies](docs/configuration.md) · [Gestionar el proxy con Plumivox](docs/managed-proxy.md)

## Herramientas disponibles

| Herramienta | Descripción |
| --- | --- |
| `list_accounts` | Nombres de cuenta y estado de preparación para lectura y publicación. Sin comunicación externa |
| `login_account` | Reutiliza las cookies guardadas; si no existen, inicia sesión con las credenciales configuradas y las guarda cifradas |
| `refresh_session` | Vuelve a iniciar sesión explícitamente y actualiza la sesión de publicación |
| `get_my_posts` | Publicaciones del propio perfil. Permite indicar si se incluyen respuestas y la paginación |
| `get_tweets` | Obtiene publicaciones públicas por ID. Hasta 20 |
| `get_following_feed` | Obtiene las publicaciones públicas de las cuentas seguidas y las ordena por fecha descendente |
| `search_tweets` | Busca publicaciones públicas en todo X o en las cuentas seguidas, indicando palabra clave y periodo |
| `get_proxy_status` | Estado del relé compartido que usa este MCP. No inicia el relé |
| `start_proxy` | Inicia la salida por línea residencial y el relé SSH configurados. No publica ni inicia sesión |
| `stop_proxy` | Detiene el relé compartido en uso. También afecta a otros chats que usan el mismo relé. Se reinicia en la siguiente solicitud de publicación o autenticación |
| `post_tweet` | Publicación de hasta 280 caracteres ponderados. Hasta 4 imágenes PNG/JPEG, de 5 MiB o menos cada una. Las imágenes se indican con ruta absoluta |

Si se omite `account`, se usa la cuenta predeterminada para búsqueda, lectura, publicación y autenticación. Puedes comprobar la cuenta que se usa al omitirlo con `isDefault` de `list_accounts`; para otra cuenta, indica explícitamente en `account` un nombre de configuración como `release_posting` o `blog_posting`. Si hay varias cuentas registradas y no hay predeterminada, se exige indicarla, y si la predeterminada no es válida, se rechazan las llamadas que la omiten.

Los límites de longitud definidos para las cadenas de entrada de cada herramienta (incluidas las cadenas dentro de arrays) también se verifican como número de unidades de código UTF-16 antes de eliminar los espacios. Los emojis fuera del BMP, por ejemplo, cuentan como 2 unidades por carácter. `minLength` y `maxLength` de JSON Schema son números de caracteres Unicode, por lo que un validador estándar de JSON Schema por sí solo no puede detectar esta restricción adicional. Por ejemplo, con el límite de 512 de la palabra clave de búsqueda, 256 😀 se aceptan, pero 257, o un espacio más 256, producen un error de argumento MCP (`-32602`) antes de la comunicación.

La configuración inicial guarda como predeterminada la cuenta registrada. Para cambiar solo la predeterminada de una configuración existente, indica `--setup --set-default-account blog_posting` al ejecutable incluido. Para más detalles, consulta el [procedimiento de configuración](docs/setup.md) y la [prioridad de la configuración](docs/configuration.md).

El recuento de caracteres de una publicación cuenta las URL como 23 caracteres, el japonés como 2 caracteres y cada emoji compuesto hasta Unicode 17 como 2 caracteres. Casos como el apretón de manos con distintos tonos de piel también se evalúan conforme a la [especificación de caracteres ponderados de X](https://docs.x.com/fundamentals/counting-characters).

Por ejemplo, puedes pedir "lee las publicaciones públicas de las cuentas que sigue blog_posting" o "publica este texto desde release_posting". La publicación se ejecuta según la petición del usuario. El texto de las publicaciones públicas es un dato externo y no se ejecutan las instrucciones que contenga.

## Alcance del feed de cuentas seguidas

TwitterAPI.io no tiene una API que obtenga directamente la pestaña "Siguiendo" de la página de inicio oficial de X. De forma predeterminada, Plumivox lee hasta 100 cuentas desde el principio de la lista de cuentas seguidas y busca sus publicaciones públicas en grupos de 20 cuentas. `maxAccounts` puede configurarse entre 20 y 200. Con 100 en la primera solicitud son hasta 6 solicitudes a la API, y con 200, hasta 11. Consulta el precio unitario actual en las [tarifas de TwitterAPI.io](https://twitterapi.io/pricing).

`officialHomeEquivalent` en el resultado es siempre `false`. No se pueden obtener publicaciones protegidas, y no se garantiza que el contenido obtenido por búsqueda, el tratamiento de los reposts ni el orden general coincidan con la página de inicio oficial. Si el proveedor tiene más resultados, o si los destinos de búsqueda únicos no protegidos superan el límite, `coverage.followingHasNextPage` pasa a ser `true`. Si pasas `coverage.followingNextCursor` a `followingCursor`, se usa la siguiente lista de cuentas seguidas. La lista de cuentas seguidas se ordena desde los seguimientos más recientes, y no se pueden evitar diferencias causadas por cambios de seguimiento en curso.

Si pasas `nextCursor` al `cursor` de `get_following_feed` de la misma cuenta, se obtiene la continuación de cada lote de búsqueda. Dentro de una página el orden es cronológico, pero debido a los límites de página de cada lote no se garantiza el orden cronológico global entre páginas. Las cuentas seguidas quedan fijadas en la lista inicial y no se vuelven a cargar en la continuación de la búsqueda. El cursor del feed también se firma con la clave de API y se rechaza cualquier alteración de los destinos de búsqueda. Los cursores sin firma de versiones anteriores y los cursores anteriores a un cambio de clave de API no se pueden usar, así que quita el cursor y vuelve a obtener los resultados. `cursor` y `followingCursor` son paginaciones distintas y no pueden indicarse a la vez.

La [especificación oficial de la respuesta de la lista de cuentas seguidas](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings) no permite confirmar un campo de estado de protección. Dentro de una misma respuesta de la lista de cuentas seguidas, los nombres de usuario válidos se agrupan sin distinguir mayúsculas y minúsculas, y se evalúan en el orden protegida, pública, desconocida. Se eligen hasta el límite en orden de primera aparición, excluyendo las identificadas como protegidas, y el número de cuentas seleccionadas que no se pudieron evaluar se devuelve en `coverage.protectionStatusUnknownAccounts`. `protectedAccountsExcluded` es el número de cuentas únicas identificadas como protegidas en esa respuesta. No cuenta nombres duplicados ni no válidos, e incluye también las cuentas protegidas fuera del límite de destinos, por lo que puede superar `maxAccounts`. Aunque sea 0, no significa necesariamente que todos los destinos sean cuentas públicas.

La continuación de un cursor firmado emitido por la implementación anterior hereda, además de los destinos de búsqueda y el número de elementos, el rango no buscado y la evaluación de continuidad de la implementación anterior, por lo que podría saltarse destinos fuera del límite. Para volver a obtener los resultados con seguridad usando la nueva agrupación del estado de protección, el nuevo cálculo de recuentos y la nueva evaluación de continuidad, quita el cursor.

Aunque haya más publicaciones o más cuentas seguidas, no se puede continuar si el cursor del proveedor o el cursor combinado superan el límite de longitud. Se conservan las publicaciones obtenidas y `hasNextPage`; `nextCursor` es `null`, `coverage.tweetContinuationAvailable` es `false` y `tweetContinuationUnavailableReason` es `cursor_too_long`. Para la lista de cuentas seguidas, consulta `followingNextCursor`, `followingContinuationAvailable` y `followingContinuationUnavailableReason`. El cursor no se trunca y el contenido ya obtenido no se descarta como un fallo.

Si el proveedor devuelve en una sola respuesta más destinos únicos no protegidos que `maxAccounts`, los destinos fuera del límite quedan sin buscar. `followingHasNextPage` es `true`, `followingContinuationUnavailableReason` es `account_limit` y no se ofrece continuación de la lista de cuentas seguidas (`followingContinuationAvailable: false`, `followingNextCursor: null`). Esto evita que el siguiente cursor del proveedor se salte destinos no buscados. Si `maxAccounts` es menor que 200, puedes aumentar el límite hasta 200 y volver a obtener los resultados sin cursor. Si el proveedor sigue devolviendo de más incluso con el límite de 200, los destinos no buscados de esa respuesta no se pueden obtener por este método. Superar el límite solo con filas duplicadas del mismo nombre o con nombres protegidos o no válidos no activa esta restricción. La continuación de las publicaciones de los destinos seleccionados se obtiene de la forma habitual, y también se hereda el estado de que hay destinos sin buscar.

## Búsqueda por palabra clave y periodo

Puedes pedir "busca publicaciones sobre reconocimiento de voz en todo X del 1 al 7 de octubre" o "busca con las mismas condiciones entre las cuentas que sigue blog_posting". La búsqueda se filtra en el lado de la API y devuelve el texto, el autor, la fecha y la URL de las publicaciones. No se realiza búsqueda semántica.

El `scope` de `search_tweets` es `all` (predeterminado, búsqueda pública en todo X) o `following` (cuentas seguidas por la cuenta indicada). `keyword` es obligatorio, y la expresión, tras quitar los espacios iniciales y finales, se busca como frase. Aunque incluya `OR` o `from:`, no se tratan como operadores de búsqueda. Se rechazan las expresiones que contienen comillas, barras invertidas o caracteres de control.

`startTime` y `endTime` son fechas y horas opcionales en ISO 8601 con zona horaria y precisión de segundos. Se busca en el periodo que incluye el inicio y excluye el final. No se indican solo con la fecha; Codex convierte la fecha y la zona horaria de la petición en fecha y hora. Por ejemplo, del 1 al 7 de octubre en hora de Japón, las condiciones son las siguientes.

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

La búsqueda obtiene primero las publicaciones más recientes. Si pasas `nextCursor` al `cursor` de la siguiente llamada, puedes obtener la continuación con las mismas condiciones. Si cambias la cuenta, el ámbito, la palabra clave, el periodo o el límite de cuentas seguidas, quita el cursor y empieza una nueva búsqueda. Si cambias la clave de API, tampoco se pueden usar los cursores de búsqueda existentes.

`following` admite `maxAccounts` (20 a 200, predeterminado 100) y `followingCursor`. Si pasas `coverage.followingNextCursor`, se busca en la siguiente lista de cuentas seguidas con las mismas condiciones. No puede indicarse a la vez que el `cursor` de la búsqueda de publicaciones. Con `all` no se indican estos dos argumentos. Los cursores de `get_following_feed` existentes y los de búsqueda no son intercambiables. Tampoco se pueden usar los cursores de búsqueda de cuentas seguidas de versiones anteriores que contienen cursores de feed sin firma antiguos, así que quita el cursor y vuelve a buscar.

La búsqueda en todo X hace 1 solicitud a la API por página, y la búsqueda en cuentas seguidas, igual que el feed existente, hasta 6 (100 cuentas) u 11 (200 cuentas) solicitudes. No se garantiza la cobertura de publicaciones protegidas ni de todas las publicaciones del periodo de búsqueda, y el alcance obtenido se devuelve en `coverage`. Los operadores de periodo se convierten a UTC conforme a la [explicación de TwitterAPI.io](https://twitterapi.io/blog/search-a-tweet).

## Si fallan la publicación o la autenticación

Si tiene éxito, devuelve el ID de la publicación y la URL de X. Un fallo de comunicación al crear la publicación o la falta del ID de la publicación se devuelven como no confirmados, y no se reenvían automáticamente. Si pides publicar de nuevo el mismo texto, se envía una vez en cada llamada. No hay ningún mecanismo que lo detenga por el historial o por un hash del texto.

Los márgenes de tiempo de comunicación predeterminados son de 45 segundos para las solicitudes normales a la API y de 120 segundos por imagen para la subida de imágenes. Solo la subida de imágenes se reintenta, una vez como máximo y con la misma imagen, y únicamente ante fallos transitorios. Esto abarca HTTP 502, 503 y 504 (espera de 250 ms), así como fallos del proxy o del servidor de origen, rechazos JSON de causa desconocida y cortes de comunicación (espera de 1 segundo); no se reintentan autenticación, saldo, imagen no válida, duplicado, límite de frecuencia ni tiempo de espera agotado. La espera y el reintento se mantienen dentro del margen de tiempo de comunicación de cada imagen, y no se reenvían imágenes anteriores ni se reintenta automáticamente la creación de la publicación o el inicio de sesión. Si falla una imagen, la publicación no se envía, y si el resultado es incierto después de crear la publicación, se devuelve `POST_UNCERTAIN`.

Los errores conocidos del procesamiento de publicaciones incluyen `stage` (`input_validation`, `pre_send`, `image_upload`, `post_creation`), `postAttempted`, que indica si se intentó la solicitud de creación de la publicación, y `uploadAttempts`, el número total de solicitudes de subida de imágenes ejecutadas. La falta de cuenta o de cookies y los fallos de preparación del relé son `pre_send`. Los errores internos desconocidos y los rechazos por formato de entrada MCP no devuelven estos campos. `postAttempted: true` no significa que la publicación haya tenido éxito. No se devuelven los errores en bruto del proveedor ni las credenciales.

`API_UNAVAILABLE` en la comunicación de imágenes incluye `failureKind`, que distingue `timeout` (tiempo agotado), `network` (fallo de comunicación) y `cancelled` (cancelación). En estos casos tampoco se reenvían automáticamente las imágenes ni la publicación.

Los errores de rechazo de la API (`API_REJECTED`, `API_HTTP_ERROR`) incluyen `httpStatus`, el array de clasificación fija `rejectionReasons` y el origen de la clasificación `rejectionReasonSource`. Este diagnóstico se conserva también cuando un 5xx en la creación de la publicación se convierte en `POST_UNCERTAIN`. El origen de la clasificación es `provider_message` (candidatos deducidos del mensaje JSON del proveedor) o `http_status` (clasificación a partir del estado HTTP). Los candidatos no confirman la causa ni autorizan el reenvío, y los textos que no se pueden identificar se marcan como `unknown`. No se devuelven ni se guardan el texto de respuesta en bruto, códigos de error arbitrarios ni ID de solicitud.

Los candidatos de `provider_message` son `api_key`, `authentication`, `account_locked`, `account_suspended`, `rate_limit`, `credits`, `proxy`, `media`, `duplicate`, `upstream_timeout`, `upstream_unavailable` y `unknown`. `http_status` añade además `invalid_request` (400), `unauthorized` (401), `forbidden` (403) y `endpoint_not_found` (404). Por ejemplo, `rejectionReasons: ["proxy", "upstream_timeout"]` con `postAttempted: false` durante la subida de imágenes indica que se sospecha del proxy o de un tiempo de espera del servidor de origen, y que el proceso se detuvo antes de crear la publicación. No se lee el cuerpo de las respuestas que no son 2xx, y se mantienen los límites de comunicación y las condiciones de reintento existentes.

Si la respuesta de la API supera 10 MiB, se interrumpe la recepción y se devuelve un fallo. Si la respuesta de una publicación supera el límite, tampoco se reenvía, porque es posible que ya se haya publicado.

Un fallo en la verificación de las cookies guardadas solo detiene la publicación. La lectura y la búsqueda de publicaciones públicas usan únicamente la clave de API y el nombre de usuario, y funcionan sin cargar las cookies guardadas. Para obtener y actualizar cookies autenticadas, consulta el [procedimiento de configuración](docs/configuration.md).

`login_account` reutiliza las cookies guardadas si están en un formato utilizable, y no informa de que se haya verificado su validez en X. Si se confirma que han caducado o que no coinciden con la configuración, llama a `refresh_session`. Las nuevas cookies se guardan cifradas en la ubicación de Plumivox con DPAPI en Windows y con una clave de Keychain en macOS, y la publicación fallida no se reenvía automáticamente.

Si se cancela antes de reemplazar lo guardado, se devuelve `AUTH_CANCELLED`; si no se puede mantener la exclusión mutua de la actualización de autenticación, `AUTH_LOCK_LOST`; y si las cookies no se pueden guardar, `LOGIN_FAILED`. Si no se puede confirmar el resultado después de ordenar el reemplazo de lo guardado, se devuelve `AUTH_SAVE_UNCERTAIN`, y es posible que ya se haya actualizado. `SESSION_STORAGE_UNAVAILABLE` indica un fallo en la ubicación de guardado o en el proceso de guardado, como DPAPI en Windows o Keychain en macOS.

## Desarrollo y verificación

El desarrollo, la compilación y las pruebas tampoco requieren Node.js. Se usan `cargo xtask build`, `cargo xtask check` y `cargo xtask test-e2e` de Rust/Cargo. Los registros que indican el antiguo lanzador de Node deben cambiarse al inicio nativo descrito arriba.

En Windows, `cargo xtask build` actualiza `dist/plumivox.exe` tras firmarlo con Sigilune (firma en la nube de Certum SimplySign) y verificar la firma. La CI de distribución también firma los artefactos de Windows con Sigilune antes de guardarlos. Para la configuración del equipo de desarrollo y los Secrets de la CI necesarios para la firma, consulta los [artefactos de distribución](docs/distribution.md#windows配布物のコード署名). El Setup de Windows se empaqueta con `cargo xtask velopack-pack` y se verifica de forma aislada, sin tocar la instalación real, con `cargo xtask test-setup-velopack-e2e`. La distribución la realiza únicamente el job de distribución de Setup de `distribution.yml`, que se inicia con un push a una rama `release/<versión>` ([distribución de Setup (Velopack)](docs/distribution.md#setupvelopackの配信)).

Consulta el [procedimiento de compilación y verificación para desarrolladores](docs/verification.md), las [normas de trabajo](AGENTS.md) y el [diseño del sistema](DESIGN.md).
