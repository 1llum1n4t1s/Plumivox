[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Serveur MCP local / plugin Codex qui s'appuie sur TwitterAPI.io pour publier sur X, effectuer des recherches par mot-clé et par période, et lire les publications publiques des comptes que vous suivez.

## Prérequis

Windows x64 ou un Mac Apple Silicon ; ni Node.js ni Rust ne sont nécessaires. Il faut une clé d'API TwitterAPI.io (paiement à l'usage) et, pour publier, le cookie de connexion v2 du compte et un proxy. [Détails](docs/readme/fr/requirements.md)

## Installation

Sous Windows, exécutez [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe) : une installation signée par utilisateur, sans droits d'administrateur, qui enregistre `plumivox` auprès de Claude Code et de Codex et se met à jour à l'ouverture de session. Sous macOS, installez le plugin Codex depuis la branche `main` de GitHub. [Détails](docs/readme/fr/installation.md)

## Configuration initiale

Exécutez `plumivox.exe --setup` et suivez le [guide de configuration](docs/setup.md). Les paramètres restent hors du dépôt, les secrets ne transitent jamais par la conversation et les paramètres existants ne sont jamais écrasés. [Détails](docs/readme/fr/initial-setup.md)

## Outils disponibles

Onze outils couvrent les comptes, la lecture, la recherche, le relais partagé et la publication (`post_tweet` : jusqu'à 280 caractères pondérés et 4 images). Sans `account`, le compte par défaut est utilisé. Le texte des publications publiques est une donnée externe et ses instructions ne sont pas exécutées. [Référence des outils](docs/readme/fr/available-tools.md)

## Portée de la récupération du fil des abonnements

Plumivox lit par défaut jusqu'à 100 comptes suivis (`maxAccounts` de 20 à 200). Le résultat n'équivaut jamais à l'accueil officiel, les publications protégées ne sont pas récupérées et `coverage` indique la portée. [Détails](docs/readme/fr/following-feed-coverage.md)

## Recherche par mot-clé et par période

`search_tweets` recherche dans tout X ou dans les comptes que vous suivez, par expression et avec une période ISO 8601 facultative (début inclus, fin exclue). Les opérateurs de recherche ne sont pas interprétés. [Détails](docs/readme/fr/keyword-and-date-range-search.md)

## Échecs de publication et d'authentification

Une publication n'est jamais renvoyée automatiquement. Un résultat incertain est renvoyé sous la forme `POST_UNCERTAIN` : vérifiez le compte avant de publier à nouveau. Le texte brut du fournisseur et les identifiants ne sont jamais renvoyés. [Détails](docs/readme/fr/when-posting-or-authentication-fails.md)

## Développement et vérification

Rust/Cargo uniquement : `cargo xtask build`, `cargo xtask check`, `cargo xtask test-e2e`. Voir [développement](docs/readme/fr/development-and-verification.md), [vérification](docs/verification.md), [conventions](AGENTS.md) et [conception](DESIGN.md).
