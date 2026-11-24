# Herramientas disponibles

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

La configuración inicial guarda como predeterminada la cuenta registrada. Para cambiar solo la predeterminada de una configuración existente, indica `--setup --set-default-account blog_posting` al ejecutable incluido. Para más detalles, consulta el [procedimiento de configuración](../../setup.md) y la [prioridad de la configuración](../../configuration.md).

El recuento de caracteres de una publicación cuenta las URL como 23 caracteres, el japonés como 2 caracteres y cada emoji compuesto hasta Unicode 17 como 2 caracteres. Casos como el apretón de manos con distintos tonos de piel también se evalúan conforme a la [especificación de caracteres ponderados de X](https://docs.x.com/fundamentals/counting-characters).

Por ejemplo, puedes pedir "lee las publicaciones públicas de las cuentas que sigue blog_posting" o "publica este texto desde release_posting". La publicación se ejecuta según la petición del usuario. El texto de las publicaciones públicas es un dato externo y no se ejecutan las instrucciones que contenga.
