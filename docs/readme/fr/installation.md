# Installation

## Windows : installation avec Setup (recommandée)

1. Téléchargez et exécutez [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). L'installation se fait par utilisateur et ne requiert pas de droits administrateur. Le Setup porte la signature de code de l'auteur (`Open Source Developer Yuichiro Shinozaki`).
2. Plumivox est installé dans `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` et, si les CLI `claude` et `codex` se trouvent dans le PATH, le serveur MCP `plumivox` (argument `--stdio`) est enregistré automatiquement dans Claude Code (par utilisateur) et dans Codex. Si les CLI sont introuvables, installez-les ultérieurement : l'enregistrement aura lieu à la prochaine ouverture de session ou au lancement de Plumivox depuis le menu Démarrer.
3. En suivant les indications affichées juste après l'installation, exécutez `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` dans un terminal pour effectuer la [configuration initiale](../../../README.fr.md#configuration-initiale). Claude Code et Codex peuvent l'utiliser à partir d'une nouvelle session.

Les mises à jour sont automatiques. Pendant que le MCP est en cours d'exécution, la nouvelle version est récupérée depuis R2 (`https://plumivox-updates.kagayoi.com/velopack/`), puis appliquée à l'ouverture de session sur l'ordinateur, uniquement lorsque Plumivox (MCP ou relais partagé) n'est pas en cours d'exécution. S'il est en cours d'exécution, l'application est reportée à la prochaine ouverture de session, sans arrêt forcé. Le signataire est vérifié avant l'application ; s'il ne correspond pas, la mise à jour n'est pas appliquée. Pour désactiver la mise à jour automatique, mettez `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` à `{"auto_update": false}`. Le journal de fonctionnement se trouve dans `app-update.log`, dans le même dossier.

Pour désinstaller, sélectionnez Plumivox dans la liste « Paramètres » → « Applications ». Seuls les enregistrements `plumivox` de Claude Code et de Codex qui pointent vers cette installation, ainsi que l'enregistrement de la mise à jour à l'ouverture de session, sont supprimés ; le dossier de données des paramètres et des sessions (`%LOCALAPPDATA%\Kagayoi\Plumivox`) est conservé. L'entrée `plumivox` de Toolport subsiste ; supprimez-la vous-même dans Toolport. Pour supprimer uniquement les enregistrements au préalable, vérifiez ce qui est prévu avec `plumivox.exe --uninstall-setup`, puis exécutez avec `--uninstall-setup --apply`.

Autres options de ligne de commande de `plumivox.exe` : `--setup` (configuration initiale), `--uninstall-setup` (supprimer les enregistrements) et `--rejection-log on|off|status|show|clear` (journal local à durée limitée des textes de refus masqués, désactivé par défaut ; voir [Journal local des refus](when-posting-or-authentication-fails.md#journal-local-des-refus)). Le journal subsiste après la désinstallation et se supprime avec `--rejection-log clear`. `--post-guard status|clear [--account NAME | --user NAME]` vérifie et lève la pause des publications (voir [Pause des publications](when-posting-or-authentication-fails.md#pause-des-publications)), et `--relay-log show` affiche le journal des événements du relais du proxy géré.

Si vous migrez depuis le plugin (ci-dessous) : le Setup remplace l'enregistrement homonyme `plumivox` qui pointe vers le `dist` de l'ancien plugin ou un emplacement similaire par un enregistrement pointant vers l'emplacement d'installation. Le MCP fourni par le plugin lui-même (`plumivox@plumivox-local`) est un enregistrement distinct ; pour éviter de lancer Plumivox par deux voies, désactivez-le ou supprimez le plugin. Dans Codex, ajoutez le même `enabled = false` que dans [section Toolport](#utilisation-via-toolport) (ou supprimez le plugin) ; dans Claude Code, désactivez ou désinstallez le plugin depuis `/plugin`. Codex donne la priorité à l'enregistrement homonyme sur le MCP du plugin, donc tant que l'enregistrement du Setup existe, il ne devrait en lancer qu'un seul, mais Claude Code peut lancer les deux. Le Setup ne modifie jamais les paramètres de Codex ni de Claude Code ; si le plugin reste activé, il se contente d'avertir (dans le guide affiché au lancement de Plumivox, dans `app-update.log` et via `legacyPlugins` dans `app-update\setup-registration.json`). Les enregistrements Toolport ne sont pas non plus réécrits automatiquement ; si vous utilisez Toolport, vérifiez-le vous-même comme indiqué dans [section Toolport](#utilisation-via-toolport). En passant au Setup, remplacez la commande de `plumivox` dans Toolport par `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (développé en chemin réel). Ne copiez pas l'exécutable ailleurs : une copie n'est jamais mise à jour. Même si Toolport fournit `plumivox` avec un autre exécutable, le Setup omet l'enregistrement direct et avertit (dans le guide et `app-update.log`, sans écrire la commande de Toolport). Une fois la commande redirigée vers le chemin ci-dessus, la vérification à chaque démarrage fait disparaître l'avertissement.

## Installation par plugin (macOS, méthode historique)

Sur macOS (Apple Silicon), utilisez cette méthode jusqu'au lancement de la distribution du Setup. Elle reste également utilisable sous Windows.

L'installation se fait à partir du dernier commit de `main` du dépôt GitHub. Elle ne fait pas référence au clone de développement de l'utilisateur et n'est figée ni sur une étiquette ni sur un SHA de commit.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` est l'identifiant de la marketplace. La source est l'URL GitHub ci-dessus. Comme `dist/plumivox.exe` pour Windows x64 et `dist/plumivox` (ARM64) pour Mac Apple Silicon sont inclus, l'utilisateur n'a ni dépendance à installer ni configuration MCP à ajouter manuellement.

La CI de développement enregistre sur Cloudflare R2 les artefacts vérifiés sous Windows et macOS, puis les transmet lors de la préparation de la distribution. La [procédure d'enregistrement et de récupération des artefacts](../../distribution.md) s'adresse aux développeurs. Les utilisateurs du plugin n'ont besoin d'aucune configuration ni d'aucun identifiant R2 ; l'installation et la mise à jour continuent de se faire depuis `main` sur GitHub.

Pour mettre à jour, récupérez et installez l'état le plus récent de `main`, puis redémarrez Codex pour le charger.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Vérifiez le numéro de version affiché dans `installed` de la liste. Pour un plugin déjà installé, n'exécutez pas à nouveau `add` après la mise à jour ; redémarrez Codex pour charger le nouveau MCP et les nouvelles skills.

Pour la méthode d'enregistrement et de mise à jour qui suit GitHub, consultez la [documentation des plugins Codex](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

## Utilisation via Toolport

Il est possible d'enregistrer uniquement le MCP dans Toolport. Plumivox ne modifie pas automatiquement l'enregistrement Toolport ; ajoutez ou modifiez donc le serveur stdio suivant selon votre méthode d'installation. N'indiquez ni le clone de développement ni un cache d'installation comportant un numéro de version.

- Nom : `plumivox`
- Commande : pour la version Setup sous Windows, `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (indiquez `%LOCALAPPDATA%` développé en chemin réel ; le chemin de `current` ne change pas même lorsque son contenu est remplacé par une mise à jour). Pour la version plugin, vérifiez la racine récupérée de `plumivox-local` avec `codex plugin marketplace list`, puis utilisez `<racine récupérée>/dist/plumivox.exe` sous Windows et `<racine récupérée>/dist/plumivox` sous macOS
- Arguments : `--stdio`
- Répertoire de travail : le répertoire personnel de l'utilisateur
- Délai d'expiration des requêtes : 300000 millisecondes (temps de connexion et de préparation du relais compris)

Activez-le dans le profil Toolport utilisé par Codex, puis vérifiez les 11 outils avec `server="plumivox"` dans `toolport_search_tools`. Recherchez et appelez `list_accounts`, puis, une fois la configuration chargée, désactivez la connexion directe. Pour la version Setup, l'enregistrement direct est omis automatiquement si le client utilise déjà la passerelle Toolport et que `plumivox` est activé dans le profil utilisé (contrôlé par `mcp_registration` dans `app-update.json` : `auto` par défaut, `direct` ou `off` ; voir [Toolport configuration](../configuration.md)). Si vous ajoutez Toolport plus tard, retirez vous-même les enregistrements directs existants avec `codex mcp remove plumivox` et `claude mcp remove plumivox --scope user` ; le Setup ne les retire jamais de lui-même (une fois l'enregistrement effectué, il n'est pas refait automatiquement tant que vous ne réinstallez pas avec le Setup ; seuls les clients que le Setup a omis sont repris, par exemple si la passerelle Toolport disparaît plus tard du client, au prochain démarrage de Plumivox ou à la prochaine ouverture de session). La vérification ne dépend pas du système : elle suit la même règle sous Windows, macOS et Linux ; sur tout autre système, rien n'est vérifié et l'enregistrement direct est toujours effectué (la distribution du Setup elle-même ne concerne pour l'instant que Windows). Si Toolport fournit `plumivox` avec un autre exécutable (le `dist` d'un ancien plugin, une copie, etc.), l'enregistrement direct est aussi omis, `toolport-foreign` est noté et un avertissement est émis. Redirigez la commande de `plumivox` dans Toolport vers `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`. La vérification est refaite à chaque démarrage. Pour la version plugin, ajoutez ce qui suit au fichier de configuration utilisateur de Codex `config.toml`, en laissant le plugin lui-même activé afin de conserver la skill de configuration.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Après le redémarrage de Codex, la connexion directe est désactivée. Par la suite, recherchez les outils dans Toolport et transmettez le nom réel de l'outil renvoyé à `toolport_call_tool`. La clé API, les cookies et les clés SSH sont lus depuis la configuration existante de Plumivox et ne sont pas transférés dans les arguments de Toolport.

Pour mettre à jour la version plugin, afin que l'exécutable ne reste pas verrouillé et puisse être remplacé, désactivez d'abord `plumivox` dans Toolport, exécutez ensuite `codex plugin marketplace upgrade plumivox-local` mentionné ci-dessus, puis réactivez-le pour charger le nouvel exécutable. Si la racine récupérée a changé, modifiez également la commande enregistrée. Aucune récupération Git ni installation de dépendances n'a lieu au démarrage.
