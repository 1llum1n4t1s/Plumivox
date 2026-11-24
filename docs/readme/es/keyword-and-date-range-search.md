# Búsqueda por palabra clave y periodo

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
