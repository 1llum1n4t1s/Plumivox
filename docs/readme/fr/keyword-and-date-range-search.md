# Recherche par mot-clé et par période

Vous pouvez demander « Recherche sur l'ensemble de X les publications sur la reconnaissance vocale du 1er au 7 octobre » ou « Fais la même recherche parmi les comptes suivis par blog_posting ». La recherche est filtrée côté API et renvoie le texte, l'auteur, la date et l'URL des publications. Aucune recherche sémantique n'est effectuée.

Le `scope` de `search_tweets` vaut `all` (par défaut, recherche publique sur l'ensemble de X) ou `following` (comptes suivis par le compte indiqué). `keyword` est obligatoire ; l'expression, débarrassée des espaces de début et de fin, est recherchée en tant que phrase. Même si elle contient `OR` ou `from:`, ceux-ci ne sont pas traités comme des opérateurs de recherche. Les expressions contenant des guillemets, des barres obliques inverses ou des caractères de contrôle sont refusées.

`startTime` et `endTime` sont des dates-heures facultatives au format ISO 8601, à la seconde près et avec fuseau horaire. La recherche porte sur une période qui inclut le début et exclut la fin. Ne les indiquez pas sous forme de date seule : Codex convertit en date-heure la date et le fuseau horaire demandés. Par exemple, du 1er au 7 octobre en heure du Japon, les conditions sont les suivantes.

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

La recherche récupère les publications en commençant par les plus récentes. En passant `nextCursor` au `cursor` de l'appel suivant, vous pouvez récupérer la suite avec les mêmes conditions. Pour changer de compte, de cible, de mot-clé, de période ou de limite de comptes suivis, retirez le cursor et lancez une nouvelle recherche. Les cursors de recherche existants ne sont pas non plus utilisables après un changement de clé API.

`following` accepte `maxAccounts` (20 à 200, 100 par défaut) et `followingCursor`. En passant `coverage.followingNextCursor`, la liste suivante des comptes suivis est recherchée avec les mêmes conditions. Il ne peut pas être indiqué en même temps que le `cursor` de la recherche de publications. Avec `all`, ces deux arguments ne sont pas indiqués. Les cursors de `get_following_feed` existant et ceux de la recherche ne sont pas interchangeables. Les cursors de recherche parmi les comptes suivis des anciennes versions, qui contiennent un ancien cursor de fil non signé, ne sont pas non plus utilisables : retirez le cursor et relancez la recherche.

La recherche sur l'ensemble de X effectue une requête API par page, et la recherche parmi les comptes suivis, comme le fil existant, au plus 6 requêtes (100 comptes) / 11 requêtes (200 comptes). L'exhaustivité des publications protégées ou de toutes les publications de la période recherchée n'est pas garantie ; la portée de la récupération est renvoyée dans `coverage`. Les opérateurs de période sont convertis en UTC conformément à l'[explication de TwitterAPI.io](https://twitterapi.io/blog/search-a-tweet).
