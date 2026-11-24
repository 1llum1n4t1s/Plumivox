# Outils disponibles

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

La configuration initiale enregistre le compte ajouté comme compte par défaut. Pour modifier uniquement le compte par défaut d'une configuration existante, passez `--setup --set-default-account blog_posting` à l'exécutable fourni. Pour plus de détails, consultez la [procédure de configuration](../../setup.md) et l'[ordre de priorité des configurations](../../configuration.md).

Le nombre de caractères d'une publication compte une URL pour 23 caractères, un caractère japonais pour 2, et chaque émoji composé jusqu'à Unicode 17 pour 2. Les poignées de main avec des couleurs de peau différentes, par exemple, sont également évaluées conformément à la [spécification du nombre de caractères pondérés de X](https://docs.x.com/fundamentals/counting-characters).

Vous pouvez par exemple demander « Lis les publications publiques des comptes suivis par blog_posting » ou « Publie ce texte depuis release_posting ». La publication est exécutée selon la demande de l'utilisateur. Le texte des publications publiques est une donnée externe ; les instructions qu'il contient ne sont pas exécutées.
