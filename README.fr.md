[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Serveur MCP local / plugin Codex qui s'appuie sur TwitterAPI.io pour publier sur X, effectuer des recherches par mot-clé et par période, et lire les publications publiques des comptes que vous suivez.

## Prérequis

- Windows x64 ou un Mac Apple Silicon (macOS ARM64). Le cœur MCP est un exécutable natif écrit en Rust ; il n'est pas nécessaire d'installer Node.js ni Rust.
- La configuration initiale sous Windows et macOS utilise l'exécutable fourni. PowerShell 7 (`pwsh`) n'est pas nécessaire.
- Une clé API TwitterAPI.io. L'utilisation de l'API est facturée à l'usage par ce service.
- Pour publier : les cookies de connexion v2 du compte concerné et un proxy. En plus d'un proxy externe existant, sous Windows et macOS, la sortie via une ligne résidentielle et le relais SSH gérés par Plumivox peuvent être partagés entre plusieurs conversations. Une préparation initiale du serveur de relais public est nécessaire.

## Installation

### Windows : installation avec Setup (recommandée)

1. Téléchargez et exécutez [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). L'installation se fait par utilisateur et ne requiert pas de droits administrateur. Le Setup porte la signature de code de l'auteur (`Open Source Developer Yuichiro Shinozaki`).
2. Plumivox est installé dans `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` et, si les CLI `claude` et `codex` se trouvent dans le PATH, le serveur MCP `plumivox` (argument `--stdio`) est enregistré automatiquement dans Claude Code (par utilisateur) et dans Codex. Si les CLI sont introuvables, installez-les ultérieurement : l'enregistrement aura lieu à la prochaine ouverture de session ou au lancement de Plumivox depuis le menu Démarrer.
3. En suivant les indications affichées juste après l'installation, exécutez `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` dans un terminal pour effectuer la [configuration initiale](#configuration-initiale). Claude Code et Codex peuvent l'utiliser à partir d'une nouvelle session.

Les mises à jour sont automatiques. Pendant que le MCP est en cours d'exécution, la nouvelle version est récupérée depuis R2 (`https://plumivox-updates.kagayoi.com/velopack/`), puis appliquée à l'ouverture de session sur l'ordinateur, uniquement lorsque Plumivox (MCP ou relais partagé) n'est pas en cours d'exécution. S'il est en cours d'exécution, l'application est reportée à la prochaine ouverture de session, sans arrêt forcé. Le signataire est vérifié avant l'application ; s'il ne correspond pas, la mise à jour n'est pas appliquée. Pour désactiver la mise à jour automatique, mettez `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` à `{"auto_update": false}`. Le journal de fonctionnement se trouve dans `app-update.log`, dans le même dossier.

Pour désinstaller, sélectionnez Plumivox dans la liste « Paramètres » → « Applications ». Seuls les enregistrements `plumivox` de Claude Code et de Codex qui pointent vers cette installation, ainsi que l'enregistrement de la mise à jour à l'ouverture de session, sont supprimés ; le dossier de données des paramètres et des sessions (`%LOCALAPPDATA%\Kagayoi\Plumivox`) est conservé. Pour supprimer uniquement les enregistrements au préalable, vérifiez ce qui est prévu avec `plumivox.exe --uninstall-setup`, puis exécutez avec `--uninstall-setup --apply`.

Si vous migrez depuis le plugin (ci-dessous) : le Setup remplace l'enregistrement homonyme `plumivox` qui pointe vers le `dist` de l'ancien plugin ou un emplacement similaire par un enregistrement pointant vers l'emplacement d'installation. Le MCP fourni par le plugin lui-même (`plumivox@plumivox-local`) est un enregistrement distinct ; pour éviter de lancer Plumivox par deux voies, désactivez-le ou supprimez le plugin. Dans Codex, ajoutez le même `enabled = false` que dans [section Toolport](#utilisation-via-toolport) (ou supprimez le plugin) ; dans Claude Code, désactivez ou désinstallez le plugin depuis `/plugin`. Codex donne la priorité à l'enregistrement homonyme sur le MCP du plugin, donc tant que l'enregistrement du Setup existe, il ne devrait en lancer qu'un seul, mais Claude Code peut lancer les deux. Le Setup ne modifie jamais les paramètres de Codex ni de Claude Code ; si le plugin reste activé, il se contente d'avertir (dans le guide affiché au lancement de Plumivox, dans `app-update.log` et via `legacyPlugins` dans `app-update\setup-registration.json`). Les enregistrements Toolport ne sont pas non plus réécrits automatiquement ; si vous utilisez Toolport, vérifiez-le vous-même comme indiqué dans [section Toolport](#utilisation-via-toolport).

### Installation par plugin (macOS, méthode historique)

Sur macOS (Apple Silicon), utilisez cette méthode jusqu'au lancement de la distribution du Setup. Elle reste également utilisable sous Windows.

L'installation se fait à partir du dernier commit de `main` du dépôt GitHub. Elle ne fait pas référence au clone de développement de l'utilisateur et n'est figée ni sur une étiquette ni sur un SHA de commit.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` est l'identifiant de la marketplace. La source est l'URL GitHub ci-dessus. Comme `dist/plumivox.exe` pour Windows x64 et `dist/plumivox` (ARM64) pour Mac Apple Silicon sont inclus, l'utilisateur n'a ni dépendance à installer ni configuration MCP à ajouter manuellement.

La CI de développement enregistre sur Cloudflare R2 les artefacts vérifiés sous Windows et macOS, puis les transmet lors de la préparation de la distribution. La [procédure d'enregistrement et de récupération des artefacts](docs/distribution.md) s'adresse aux développeurs. Les utilisateurs du plugin n'ont besoin d'aucune configuration ni d'aucun identifiant R2 ; l'installation et la mise à jour continuent de se faire depuis `main` sur GitHub.

Pour mettre à jour, récupérez et installez l'état le plus récent de `main`, puis redémarrez Codex pour le charger.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Vérifiez le numéro de version affiché dans `installed` de la liste. Pour un plugin déjà installé, n'exécutez pas à nouveau `add` après la mise à jour ; redémarrez Codex pour charger le nouveau MCP et les nouvelles skills.

Pour la méthode d'enregistrement et de mise à jour qui suit GitHub, consultez la [documentation des plugins Codex](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

### Utilisation via Toolport

Il est possible d'enregistrer uniquement le MCP dans Toolport. Plumivox ne modifie pas automatiquement l'enregistrement Toolport ; ajoutez ou modifiez donc le serveur stdio suivant selon votre méthode d'installation. N'indiquez ni le clone de développement ni un cache d'installation comportant un numéro de version.

- Nom : `plumivox`
- Commande : pour la version Setup sous Windows, `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (indiquez `%LOCALAPPDATA%` développé en chemin réel ; le chemin de `current` ne change pas même lorsque son contenu est remplacé par une mise à jour). Pour la version plugin, vérifiez la racine récupérée de `plumivox-local` avec `codex plugin marketplace list`, puis utilisez `<racine récupérée>/dist/plumivox.exe` sous Windows et `<racine récupérée>/dist/plumivox` sous macOS
- Arguments : `--stdio`
- Répertoire de travail : le répertoire personnel de l'utilisateur
- Délai d'expiration des requêtes : 300000 millisecondes (temps de connexion et de préparation du relais compris)

Activez-le dans le profil Toolport utilisé par Codex, puis vérifiez les 11 outils avec `server="plumivox"` dans `toolport_search_tools`. Recherchez et appelez `list_accounts`, puis, une fois la configuration chargée, désactivez la connexion directe. Pour la version Setup, retirez les enregistrements directs avec `codex mcp remove plumivox` et `claude mcp remove plumivox --scope user` (une fois l'enregistrement effectué, il n'est plus refait automatiquement jusqu'à une réinstallation via le Setup). Pour la version plugin, ajoutez ce qui suit au fichier de configuration utilisateur de Codex `config.toml`, en laissant le plugin lui-même activé afin de conserver la skill de configuration.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Après le redémarrage de Codex, la connexion directe est désactivée. Par la suite, recherchez les outils dans Toolport et transmettez le nom réel de l'outil renvoyé à `toolport_call_tool`. La clé API, les cookies et les clés SSH sont lus depuis la configuration existante de Plumivox et ne sont pas transférés dans les arguments de Toolport.

La version plugin se met à jour avec `codex plugin marketplace upgrade plumivox-local` mentionné ci-dessus. Comme le serveur exécuté par Toolport utilise le code déjà chargé, désactivez puis réactivez `plumivox` dans Toolport après la mise à jour pour charger le nouvel exécutable. Si la racine récupérée a changé, modifiez également la commande enregistrée. Aucune récupération Git ni installation de dépendances n'a lieu au démarrage.

## Configuration initiale

Pour une première utilisation, passez à la [procédure de configuration](docs/setup.md). Un assistant de configuration natif pour Windows et macOS ainsi qu'un installateur VPS statique en Rust pour Linux x86_64 et aarch64 sont fournis. Un chemin compatible Bash est conservé pour les autres processeurs. Vous pouvez aussi demander à Codex « Configure Plumivox » pour être guidé par la skill fournie `setup-plumivox`.

Le fichier de configuration est placé hors du dépôt. Les nouveaux utilisateurs utilisent `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` sous Windows et `~/Library/Application Support/Kagayoi/Plumivox/settings.json` sous macOS. Les utilisateurs disposant déjà de `~/dev/Secret/secrets.json` voient la section `twitterapi_io` de ce fichier lue. Les entrées `release_posting` et `blog_posting` de vava/blbl sont également conservées. Pour utiliser un autre emplacement, définissez `PLUMIVOX_SECRETS_FILE`. Il n'est pas nécessaire de transmettre des valeurs secrètes dans la conversation ni dans les arguments MCP.

Si le fichier de configuration sélectionné ne peut pas être lu, une erreur est renvoyée sans basculer vers une autre configuration. Si vous avez changé l'emplacement d'enregistrement, redémarrez Codex. La configuration initiale n'écrase pas les clés existantes, les comptes de même nom ni la configuration du relais partagé. Pour modifier une configuration existante, consultez la [procédure de configuration](docs/setup.md).

[Première configuration](docs/setup.md) · [Configuration des comptes et des cookies](docs/configuration.md) · [Gérer le proxy avec Plumivox](docs/managed-proxy.md)

## Outils disponibles

| Outil | Description |
| --- | --- |
| `list_accounts` | Noms des comptes et état de préparation pour la lecture et la publication. Aucune communication externe |
| `login_account` | Réutilise les cookies enregistrés ; s'ils n'existent pas encore, se connecte avec les identifiants configurés et les enregistre chiffrés |
| `refresh_session` | Se reconnecte explicitement et met à jour la session de publication |
| `get_my_posts` | Publications de votre propre profil. Inclusion des réponses et pagination configurables |
| `get_tweets` | Récupère des publications publiques par ID. 20 au maximum |
| `get_following_feed` | Récupère les publications publiques des comptes suivis et les trie par date décroissante |
| `search_tweets` | Recherche des publications publiques sur l'ensemble de X ou parmi les comptes suivis, par mot-clé et par période |
| `get_proxy_status` | État du relais partagé utilisé par ce MCP. Ne démarre pas le relais |
| `start_proxy` | Démarre la sortie via ligne résidentielle et le relais SSH configurés. Ne publie pas et ne se connecte pas |
| `stop_proxy` | Arrête le relais partagé en cours d'utilisation. Affecte aussi les autres conversations utilisant le même relais. Redémarre à la prochaine demande de publication ou d'authentification |
| `post_tweet` | Publication d'au plus 280 caractères pondérés. Jusqu'à 4 images PNG/JPEG, 5 MiB maximum chacune. Images indiquées par chemin absolu |

Si `account` est omis, le compte par défaut est utilisé pour la recherche, la lecture, la publication et l'authentification. Le compte utilisé en cas d'omission peut être vérifié via `isDefault` de `list_accounts` ; pour un autre compte, indiquez explicitement dans `account` un nom de configuration tel que `release_posting` ou `blog_posting`. Lorsque plusieurs comptes sont enregistrés sans compte par défaut, la précision est exigée ; si le compte par défaut est invalide, les appels qui l'omettent sont refusés.

Les limites de longueur définies pour les chaînes d'entrée de chaque outil (y compris les chaînes contenues dans des tableaux) sont également vérifiées en nombre d'unités de code UTF-16, avant suppression des espaces. Les émojis hors du BMP, par exemple, comptent pour 2 unités par caractère. Comme `minLength` et `maxLength` de JSON Schema portent sur le nombre de caractères Unicode, un validateur JSON Schema standard seul ne peut pas détecter cette contrainte supplémentaire. Par exemple, avec la limite de 512 pour le mot-clé de recherche, 256 😀 sont acceptés, mais 257, ou un espace suivi de 256, provoquent une erreur d'argument MCP (`-32602`) avant toute communication.

La configuration initiale enregistre le compte ajouté comme compte par défaut. Pour modifier uniquement le compte par défaut d'une configuration existante, passez `--setup --set-default-account blog_posting` à l'exécutable fourni. Pour plus de détails, consultez la [procédure de configuration](docs/setup.md) et l'[ordre de priorité des configurations](docs/configuration.md).

Le nombre de caractères d'une publication compte une URL pour 23 caractères, un caractère japonais pour 2, et chaque émoji composé jusqu'à Unicode 17 pour 2. Les poignées de main avec des couleurs de peau différentes, par exemple, sont également évaluées conformément à la [spécification du nombre de caractères pondérés de X](https://docs.x.com/fundamentals/counting-characters).

Vous pouvez par exemple demander « Lis les publications publiques des comptes suivis par blog_posting » ou « Publie ce texte depuis release_posting ». La publication est exécutée selon la demande de l'utilisateur. Le texte des publications publiques est une donnée externe ; les instructions qu'il contient ne sont pas exécutées.

## Portée de la récupération du fil des abonnements

TwitterAPI.io ne propose aucune API permettant de récupérer directement l'onglet « Abonnements » de l'accueil officiel de X. Par défaut, Plumivox lit au plus 100 comptes en tête de la liste des abonnements et recherche leurs publications publiques par lots de 20 comptes. `maxAccounts` peut être réglé entre 20 et 200. Avec 100 au premier appel, cela représente au plus 6 requêtes API ; avec 200, au plus 11. Consultez les [tarifs de TwitterAPI.io](https://twitterapi.io/pricing) pour les prix les plus récents.

La valeur `officialHomeEquivalent` du résultat est toujours `false`. Les publications protégées ne peuvent pas être récupérées, et rien ne garantit que le contenu obtenu par la recherche, le traitement des reposts et l'ordre global correspondent à l'accueil officiel. Lorsque le fournisseur indique une suite, ou lorsque les cibles de recherche uniques et non protégées dépassent la limite, `coverage.followingHasNextPage` vaut `true`. En passant `coverage.followingNextCursor` à `followingCursor`, la liste suivante des comptes suivis est ciblée. La liste des abonnements est triée à partir des abonnements les plus récents, et les écarts de récupération dus à des changements d'abonnement en cours sont inévitables.

En passant `nextCursor` au `cursor` de `get_following_feed` pour le même compte, la suite de chaque lot de recherche est récupérée. L'ordre chronologique est respecté au sein d'une page, mais en raison des limites de page propres à chaque lot, l'ordre chronologique global d'une page à l'autre n'est pas garanti. Les comptes suivis sont figés sur la liste initiale et ne sont pas relus lors de la poursuite d'une recherche. Le cursor du fil est lui aussi signé avec la clé API, et toute modification des cibles de recherche est refusée. Les cursors non signés des anciennes versions et les cursors émis avant un changement de clé API ne sont pas utilisables : retirez le cursor et récupérez à nouveau. `cursor` et `followingCursor` sont deux paginations distinctes et ne peuvent pas être indiqués simultanément.

La [spécification officielle de la réponse de la liste des abonnements](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings) ne permet pas de confirmer de champ d'état de protection. Au sein d'une même réponse de liste des abonnements, les noms d'utilisateur valides sont regroupés sans tenir compte de la casse, puis évalués dans l'ordre protégé, public, inconnu. Les cibles sont choisies jusqu'à la limite dans l'ordre de première apparition, en excluant celles identifiées comme protégées, et le nombre de cibles sélectionnées dont l'état n'a pas pu être déterminé est renvoyé dans `coverage.protectionStatusUnknownAccounts`. `protectedAccountsExcluded` est le nombre de comptes uniques identifiés comme protégés dans cette réponse. Les noms d'utilisateur en double ou invalides ne sont pas comptés, mais les comptes protégés situés au-delà de la limite de récupération sont inclus ; la valeur peut donc dépasser `maxAccounts`. Une valeur de 0 ne signifie pas pour autant que toutes les cibles sont des comptes publics.

La suite d'un cursor signé émis par l'ancienne implémentation reprend, outre les cibles de recherche et le nombre d'éléments, l'évaluation de l'ancienne implémentation pour la plage non recherchée et la possibilité de poursuivre ; des cibles au-delà de la limite peuvent donc être ignorées. Pour récupérer à nouveau de manière fiable avec le nouveau regroupement des états de protection, le nouveau calcul des nombres et la nouvelle évaluation de la poursuite, retirez le cursor.

Même s'il existe une suite pour les publications ou la liste des abonnements, la poursuite est impossible lorsque le cursor du fournisseur ou le cursor combiné dépasse la limite de longueur. Les publications déjà récupérées et `hasNextPage` sont conservés, `nextCursor` vaut `null`, `coverage.tweetContinuationAvailable` vaut `false` et `tweetContinuationUnavailableReason` vaut `cursor_too_long`. Pour la liste des abonnements, vérifiez `followingNextCursor`, `followingContinuationAvailable` et `followingContinuationUnavailableReason`. Le cursor n'est pas tronqué, et le contenu déjà récupéré n'est pas rejeté comme un échec.

Lorsque le fournisseur renvoie en une seule réponse plus de cibles uniques non protégées que `maxAccounts`, les cibles au-delà de la limite ne sont pas recherchées. `followingHasNextPage` vaut `true`, `followingContinuationUnavailableReason` vaut `account_limit`, et la poursuite de la liste des abonnements n'est pas proposée (`followingContinuationAvailable: false`, `followingNextCursor: null`). Cela évite d'ignorer des cibles non recherchées via le cursor suivant du fournisseur. Si `maxAccounts` est inférieur à 200, vous pouvez augmenter la limite jusqu'à 200 et récupérer à nouveau sans cursor. Si le fournisseur renvoie trop d'éléments même avec la limite de 200, les cibles non recherchées de cette réponse ne peuvent pas être récupérées par cette méthode. Le dépassement de la limite par des lignes en double de même nom ou uniquement par des noms protégés ou invalides ne relève pas de cette restriction. La suite des publications des cibles sélectionnées peut être récupérée normalement, et l'existence de cibles non recherchées est également transmise.

## Recherche par mot-clé et par période

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

## Échecs de publication et d'authentification

En cas de succès, l'ID de la publication et son URL X sont renvoyés. Un échec de communication lors de la création de la publication ou l'absence d'ID de publication est renvoyé comme indéterminé, sans renvoi automatique. Si vous demandez à nouveau de publier le même texte, chaque appel envoie une fois. Aucun mécanisme ne bloque l'envoi en raison de l'historique ou d'un hachage du texte.

Le délai de communication par défaut est de 45 secondes pour les requêtes API ordinaires et de 120 secondes par image pour le téléversement d'images. Seul le téléversement d'images est réessayé, au plus une fois pour la même image, et uniquement en cas d'échec temporaire. Sont concernés les codes HTTP 502, 503 et 504 (attente de 250 ms), ainsi que les pannes du proxy ou de l'amont, les refus JSON de cause inconnue et les coupures de communication (attente d'une seconde) ; l'authentification, le solde, les images invalides, les doublons, la limitation de débit et les dépassements de délai ne sont pas réessayés. L'attente et la nouvelle tentative restent dans le délai de communication de l'image concernée ; il n'y a ni renvoi de l'image précédente ni renvoi automatique de la création de la publication ou de la connexion. En cas d'échec d'une image, la publication n'est pas envoyée ; si l'issue devient incertaine après la création de la publication, `POST_UNCERTAIN` est renvoyé.

Les erreurs connues de traitement de publication incluent `stage` (`input_validation`, `pre_send`, `image_upload`, `post_creation`), `postAttempted` qui indique si une requête de création de publication a été tentée, et `uploadAttempts`, le nombre total de requêtes de téléversement d'images exécutées. L'absence de compte ou de cookies et l'échec de la préparation du relais relèvent de `pre_send`. Ces informations ne sont pas renvoyées pour les erreurs internes inconnues ni pour les refus de format d'entrée MCP. `postAttempted: true` ne signifie pas que la publication a réussi. Les erreurs brutes du fournisseur et les identifiants ne sont pas renvoyés.

`API_UNAVAILABLE` lors de la communication d'images inclut `failureKind`, qui distingue `timeout` (délai dépassé), `network` (panne de communication) et `cancelled` (annulation). Dans ces cas non plus, ni les images ni la publication ne sont renvoyées automatiquement.

Les erreurs de refus de l'API (`API_REJECTED`, `API_HTTP_ERROR`) incluent `httpStatus`, le tableau de classifications fixes `rejectionReasons` et la source de classification `rejectionReasonSource`. Ces informations de diagnostic sont conservées même lorsqu'une erreur 5xx lors de la création de la publication devient `POST_UNCERTAIN`. La source de classification est `provider_message` (candidats déduits du message JSON du fournisseur) ou `http_status` (classification d'après le statut HTTP). Les candidats ne signifient ni que la cause est établie ni que le renvoi est autorisé ; un texte impossible à identifier donne `unknown`. Le texte brut de la réponse, les codes d'erreur arbitraires et les request ID ne sont ni renvoyés ni enregistrés.

Les candidats de `provider_message` sont `api_key`, `authentication`, `account_locked`, `account_suspended`, `rate_limit`, `credits`, `proxy`, `media`, `duplicate`, `upstream_timeout`, `upstream_unavailable`, `automation_suspected`, `daily_limit`, `text_too_long`, `session_mismatch`, `post_id_missing` et `unknown`. Les cinq précédant `unknown` sont des candidats fondés sur des formulations publiquement connues ; il n'est pas confirmé que le fournisseur renvoie exactement ce texte. Ils ne sont pas retentés. `post_id_missing` signifie que la publication a peut-être été créée ; vérifiez le compte avant de renvoyer. `http_status` comporte en outre `invalid_request` (400), `unauthorized` (401), `forbidden` (403) et `endpoint_not_found` (404). Par exemple, `rejectionReasons: ["proxy", "upstream_timeout"]` avec `postAttempted: false` lors d'un téléversement d'image indique qu'un problème de proxy ou un dépassement de délai en amont est suspecté et que le traitement s'est arrêté avant la création de la publication. Le corps des réponses non 2xx n'est pas lu, et les limites de communication et conditions de nouvelle tentative existantes sont maintenues.

Si une réponse de l'API dépasse 10MiB, la réception est interrompue et un échec est renvoyé. Si la limite est dépassée dans la réponse d'une publication, il n'y a pas non plus de renvoi, car la publication a peut-être déjà été effectuée.

Un échec de vérification des cookies enregistrés n'arrête que la publication. La lecture et la recherche de publications publiques n'utilisent que la clé API et le nom d'utilisateur, et fonctionnent sans charger les cookies enregistrés. Pour obtenir ou mettre à jour des cookies authentifiés, consultez la [procédure de configuration](docs/configuration.md).

`login_account` réutilise les cookies enregistrés s'ils sont dans un format utilisable, sans prétendre avoir vérifié leur validité sur X. Lorsqu'une expiration ou une incohérence de configuration est constatée, appelez `refresh_session`. Les nouveaux cookies sont enregistrés à l'emplacement de stockage de Plumivox, chiffrés avec DPAPI sous Windows et avec une clé du Keychain sous macOS ; la publication ayant échoué n'est pas renvoyée automatiquement.

En cas d'abandon avant le remplacement de l'enregistrement, `AUTH_CANCELLED` est renvoyé ; si l'exclusivité de la mise à jour de l'authentification ne peut pas être maintenue, `AUTH_LOCK_LOST` ; et pour des cookies impossibles à enregistrer, `LOGIN_FAILED`. Si le résultat ne peut pas être confirmé après l'ordre de remplacement de l'enregistrement, `AUTH_SAVE_UNCERTAIN` est renvoyé, et la mise à jour a peut-être eu lieu. `SESSION_STORAGE_UNAVAILABLE` indique un échec de l'emplacement de stockage ou du traitement d'enregistrement, comme DPAPI sous Windows ou le Keychain sous macOS.

## Développement et vérification

Node.js n'est pas non plus nécessaire pour le développement, la compilation et les tests. Utilisez `cargo xtask build`, `cargo xtask check` et `cargo xtask test-e2e` de Rust/Cargo. Les enregistrements qui indiquent l'ancien lanceur Node doivent être modifiés pour utiliser le démarrage natif décrit ci-dessus.

Sous Windows, `cargo xtask build` met à jour `dist/plumivox.exe` après signature et vérification de la signature avec Sigilune (signature dans le cloud de Certum SimplySign). La CI de distribution signe elle aussi les artefacts Windows avec Sigilune avant de les enregistrer. Pour la configuration du poste de développement et les Secrets de la CI nécessaires à la signature, consultez les [artefacts de distribution](docs/distribution.md#windows配布物のコード署名). Le Setup Windows est empaqueté avec `cargo xtask velopack-pack` et vérifié de façon isolée avec `cargo xtask test-setup-velopack-e2e`, sans toucher à une installation réelle. La distribution est effectuée uniquement par le job de distribution du Setup de `distribution.yml`, déclenché par un push d'un tag léger `v<version>` sur un commit de `main` ([distribution du Setup (Velopack)](docs/distribution.md#setupvelopackの配信)).

Consultez la [procédure de compilation et de vérification pour les développeurs](docs/verification.md), les [règles de travail](AGENTS.md) et la [conception du système](DESIGN.md).
