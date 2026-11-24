# Configuration initiale

Pour une première utilisation, passez à la [procédure de configuration](../../setup.md). Un assistant de configuration natif pour Windows et macOS ainsi qu'un installateur VPS statique en Rust pour Linux x86_64 et aarch64 sont fournis. Un chemin compatible Bash est conservé pour les autres processeurs. Vous pouvez aussi demander à Codex « Configure Plumivox » pour être guidé par la skill fournie `setup-plumivox`.

Le fichier de configuration est placé hors du dépôt. Les nouveaux utilisateurs utilisent `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` sous Windows et `~/Library/Application Support/Kagayoi/Plumivox/settings.json` sous macOS. Les utilisateurs disposant déjà de `~/dev/Secret/secrets.json` voient la section `twitterapi_io` de ce fichier lue. Les entrées `release_posting` et `blog_posting` de vava/blbl sont également conservées. Pour utiliser un autre emplacement, définissez `PLUMIVOX_SECRETS_FILE`. Il n'est pas nécessaire de transmettre des valeurs secrètes dans la conversation ni dans les arguments MCP.

Si le fichier de configuration sélectionné ne peut pas être lu, une erreur est renvoyée sans basculer vers une autre configuration. Si vous avez changé l'emplacement d'enregistrement, redémarrez Codex. La configuration initiale n'écrase pas les clés existantes, les comptes de même nom ni la configuration du relais partagé. Pour modifier une configuration existante, consultez la [procédure de configuration](../../setup.md).

[Première configuration](../../setup.md) · [Configuration des comptes et des cookies](../../configuration.md) · [Gérer le proxy avec Plumivox](../../managed-proxy.md)
