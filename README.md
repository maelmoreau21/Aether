<div align="center">
  <img src="Aether.svg" alt="Logo Aether" width="100" height="100">

  # Aether

  **Votre médiathèque, sur votre infrastructure.**

  Une plateforme multimédia libre et auto-hébergeable, construite avec **Rust** et **SvelteKit** : catalogue Films & Séries, lecture Web, administration centralisée et stockage réparti sur plusieurs machines.

  [![License: AGPL-3.0](https://img.shields.io/badge/Licence-AGPL--3.0-blue.svg)](LICENSE)
  ![Rust](https://img.shields.io/badge/Backend-Rust-orange)
  ![SvelteKit](https://img.shields.io/badge/Frontend-SvelteKit-ff3e00)
  ![Docker](https://img.shields.io/badge/D%C3%A9ploiement-Docker-2496ed)
  ![Statut](https://img.shields.io/badge/Statut-D%C3%A9veloppement%20actif-yellow)

  [Installation](#-installation-rapide) · [Fonctionnalités](#-fonctionnalités) · [Architecture](#-architecture) · [Documentation](#-documentation) · [Contribuer](#-contribuer)
</div>

---

> [!IMPORTANT]
> **Aether est en développement actif.** Les fonctionnalités décrites ci-dessous correspondent aux parcours implémentés dans le dépôt, mais ne constituent pas une garantie de compatibilité avec tous les médias, navigateurs, appareils ou environnements de production. Vérifiez les limites documentées avant de l'exposer sur Internet.

> [!CAUTION]
> **Respect des droits d’auteur :** Aether est un logiciel de gestion et de lecture de médias personnels. Il ne fournit, ne télécharge et ne distribue aucun film ni aucune série. Vous devez utiliser uniquement des contenus provenant de **sources licites** et disposer des droits ou autorisations nécessaires pour les copier, les héberger et les consulter.
>
> Le fait de posséder un DVD ou un Blu-ray ne donne pas automatiquement le droit de contourner une protection technique, de redistribuer une copie ou de mettre le film à disposition d’autres personnes. En France, l’exception de copie privée est limitée à une copie réalisée depuis une source licite et réservée à l’usage privé du copiste ; elle ne couvre pas la diffusion collective ou publique. Chaque utilisateur est responsable de l’utilisation qu’il fait d’Aether et du respect du droit applicable. Voir [l’article L122-5 du Code de la propriété intellectuelle](https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000043975029).

## Pourquoi Aether ?

Aether vise à offrir une expérience de médiathèque moderne sans dépendre d'un service de streaming propriétaire. Les fichiers restent **sur vos disques**, et l'application ne modifie pas vos bibliothèques : les volumes médias sont montés en **lecture seule**.

Le projet privilégie la **lecture directe** et évite d'imposer un transcodage vidéo côté serveur. Il peut cohabiter avec une installation Jellyfin existante, sans être un fork ni nécessiter Jellyfin pour fonctionner.

## Fonctionnalités

| Domaine | Fonctionnalités |
| --- | --- |
| **Catalogue** | Films et séries, recherche, saisons et épisodes, affiches locales, métadonnées NFO et bibliothèques multiples |
| **Lecture** | Lecteur Web, prise en charge ciblée d'AV1 et Opus en MKV/MP4, pause, avance, changement de piste selon les capacités du client |
| **Pistes externes** | Audio Opus/Ogg associé au média et sous-titres texte SRT/WebVTT ; préférences de langues et sous-titres forcés |
| **Suivi** | Reprise de lecture, progression par utilisateur, éléments vus/non vus et « Continuer à regarder » |
| **Administration** | Comptes, groupes, droits par bibliothèque, clés API limitées au catalogue, tâches et scans planifiés |
| **Authentification** | Comptes locaux et connexion OpenID Connect (OIDC), notamment avec Authentik |
| **Stockage** | SQLite par défaut, PostgreSQL 18 externe en option pour un Core unique |
| **Sauvegardes** | Sauvegardes administratives chiffrées et planifiables, restauration hors ligne |
| **Distribution** | Mode autonome ou Core central avec un ou plusieurs Media Nodes |

**Limites actuelles :** la lecture AV1/Opus dépend de la prise en charge du navigateur et du conteneur ; toutes les combinaisons de pistes ne sont pas garanties. Le support complet des styles ASS/SSA, des flux élémentaires « raw », des extensions exécutables et du cluster multi-Core n'est **pas** livré comme fonctionnalité de production. Aucun transcodage vidéo universel n'est proposé comme solution de repli.

## Technologies

- **Backend :** Rust (édition 2024), Axum, Tokio, SQLx.
- **Frontend :** Svelte 5, SvelteKit, TypeScript, Vite ; application compilée et servie par le Core.
- **Base de données :** SQLite (mode WAL) ou PostgreSQL 18 externe.
- **Médias :** `ffprobe` pour l'inspection des fichiers ; stratégie de lecture directe et de flux autorisés.
- **Déploiement :** Docker / Docker Compose, avec images Linux `amd64`.

## 🚀 Installation rapide

**Sur ce PC Windows, sans Docker :** suivre le [guide local pas à pas](docs/INSTALLATION_LOCALE.md).
Il couvre le premier administrateur, le démarrage et l’installation des extensions.

### Prérequis

- Docker et Docker Compose v2 sur une machine Linux `x86_64` (ou un environnement compatible Docker).
- Un dossier de films ou de séries accessible en lecture par le conteneur.
- Pour l'accès depuis un autre appareil : un **reverse proxy HTTPS** (Traefik, Caddy, Nginx ou Synology DSM).

### 1. Récupérer le dépôt

```bash
git clone https://github.com/maelmoreau21/Aether.git
cd Aether
cp .env.example .env
```

Sur Windows PowerShell, remplacez `cp` par `Copy-Item .env.example .env` si nécessaire.

### 2. Configurer les médias

Dans `.env`, renseignez au minimum le chemin réel de votre médiathèque :

```dotenv
MEDIA_LIBRARY_DIR=/chemin/vers/mes/medias
MEDIA_PUBLIC_URL=http://127.0.0.1:3000
MEDIA_DB_ENGINE=sqlite
```

`MEDIA_LIBRARY_DIR` est monté sur `/media` en **lecture seule**. L'URL HTTP locale convient uniquement à une utilisation depuis la machine hôte elle-même.

### 3. Démarrer Aether

Si les images `ghcr.io/maelmoreau21/aether` sont accessibles dans le registre :

```bash
docker compose pull
docker compose up -d --no-build
```

Alternative pour construire les images depuis le code source :

```bash
docker compose up -d --build
```

> La disponibilité et la visibilité publique des images GHCR dépendent de la publication réussie du workflow GitHub Actions ; rendre le dépôt GitHub public ne rend pas automatiquement ses packages publics.

### 4. Créer le premier administrateur

```bash
docker compose exec -it aether media-bootstrap
```

Définissez un mot de passe lors de l'amorçage interactif. **Aether ne possède aucun compte ni mot de passe par défaut.**

Ouvrez ensuite **[http://127.0.0.1:3000/app/login/](http://127.0.0.1:3000/app/login/)** depuis la machine qui héberge Docker. Ajoutez vos bibliothèques depuis l'administration, puis lancez un scan.

Pour les procédures détaillées, consultez le [guide Docker](docs/DEPLOYMENT_DOCKER.md) ou le [guide Synology](docs/DEPLOYMENT_SYNOLOGY.md).

## Architecture

Aether sépare trois responsabilités, même dans le conteneur autonome :

```text
             Navigateur Web
              SvelteKit
                  │
                  ▼
          ┌───────────────┐
          │  Aether Core  │
          │  API · Auth   │
          │ Catalogue     │
          └──────┬────────┘
                 │
       ┌─────────┴─────────┐
       ▼                   ▼
 ┌────────────┐     ┌────────────┐
 │ Media Node │     │ Media Node │
 │ local      │     │ distant    │
 └──────┬─────┘     └──────┬─────┘
        ▼                  ▼
  Films / Séries      Films / Séries
   (lecture seule)     (lecture seule)
```

### Mode autonome

Un seul conteneur `aether` regroupe l'interface Web, le Core, le Media Node local et `ffprobe`. **Aucun processus Node.js permanent** et **aucune base externe obligatoire**.

```bash
docker compose -f docker-compose.yml up -d
```

### Mode distribué

Un `aether-core` centralise les comptes et le catalogue ; les conteneurs `aether-node` accèdent uniquement à leurs propres bibliothèques. Le déploiement distribué nécessite un **secret d'appairage unique**, une configuration réseau adaptée et, entre machines, un transport HTTPS de confiance.

Le fichier [`compose.distributed.yaml`](compose.distributed.yaml) fournit un exemple mono-hôte. Les exemples [`compose.multi-machine.example.yaml`](compose.multi-machine.example.yaml) et [`compose.postgres-multimachine.example.yaml`](compose.postgres-multimachine.example.yaml) couvrent les déploiements sur plusieurs hôtes. Consultez impérativement le [guide Synology / multi-Node](docs/DEPLOYMENT_SYNOLOGY.md) avant installation.

### Images conteneur

| Image | Usage |
| --- | --- |
| `ghcr.io/maelmoreau21/aether` | Mode autonome |
| `ghcr.io/maelmoreau21/aether-core` | Core du mode distribué |
| `ghcr.io/maelmoreau21/aether-node` | Nœud média du mode distribué |

Les tags `latest` et `develop` correspondent respectivement aux branches `main` et `develop` **lorsqu'un pipeline de publication réussit**. Pour figer une installation, privilégiez un digest d'image.

## Configuration et sécurité

| Paramètre | Rôle |
| --- | --- |
| `MEDIA_PUBLIC_URL` | URL canonique de l'instance ; HTTPS obligatoire hors accès local loopback |
| `MEDIA_LIBRARY_DIR` | Dossier de médias monté en lecture seule en mode autonome |
| `MEDIA_DB_ENGINE` | `sqlite` (défaut) ou `postgres` |
| `MEDIA_DB_URL` | URL PostgreSQL, uniquement si ce moteur est activé |
| `MEDIA_BACKUP_KEY` | Clé externe facultative pour les archives de l'administration (clé par défaut utilisée si omise) |
| `MEDIA_OIDC_ENABLED` | Activation optionnelle de l'authentification OIDC |
| `MEDIA_NODE_PAIRING_TOKEN` | Secret d'appairage pour le mode distribué |

Retrouvez les autres variables dans [`.env.example`](.env.example) et la [documentation PostgreSQL](docs/POSTGRESQL.md).

Principes de sécurité :

- **Médias en lecture seule** (`:ro`) ; aucune réorganisation ou modification des fichiers d'origine.
- **Pas d'identifiants prédéfinis** : le premier administrateur est créé localement.
- **HTTPS obligatoire** pour l'accès réseau ; vérification de l'origine des requêtes et protections des sessions/CSRF.
- **Conteneurs non-root** avec capacités Linux réduites.
- **Secrets hors Git** : ne jamais commiter de `.env` de production, token d'appairage, clé de sauvegarde ou mot de passe.

**Attention :** sélectionner PostgreSQL ne migre **pas** automatiquement une base SQLite existante. Les sauvegardes administratives couvrent la configuration et l'état applicatif, **pas** les fichiers multimédias.

Consultez la [politique de sécurité](SECURITY.md) et les [notes techniques de sécurité](docs/SECURITY.md). Pour signaler une vulnérabilité, utilisez de préférence le signalement privé depuis l'onglet **Security** du dépôt.

## Développement

Prérequis de développement : Rust `1.99+` (édition 2024), Node.js compatible avec le frontend, npm, `ffprobe` et les outils nécessaires aux fixtures multimédias des tests.

```bash
# Qualité du code Rust
cargo fmt --check
cargo clippy --locked --workspace --all-features --all-targets -- -D warnings

# Tests Rust (certaines suites nécessitent des fixtures ou services de test)
cargo test --locked --workspace --all-features

# Frontend
cd apps/web
npm ci
npm run check
npm run lint
npm test
npm run build
```

Les tests multimédias et navigateur nécessitent parfois la génération préalable de fixtures synthétiques et/ou des outils complémentaires. Consultez [`docs/TEST_PLAN.md`](docs/TEST_PLAN.md) et [`docs/CI_GHCR.md`](docs/CI_GHCR.md) pour les parcours complets.

## 📚 Documentation

| Sujet | Document |
| --- | --- |
| Installation Docker | [Guide Docker](docs/DEPLOYMENT_DOCKER.md) |
| NAS Synology et distribution | [Guide Synology](docs/DEPLOYMENT_SYNOLOGY.md) |
| Multi-Node et architecture | [Architecture](ARCHITECTURE.md) |
| PostgreSQL 18 | [Configuration PostgreSQL](docs/POSTGRESQL.md) |
| Authentification SSO | [OIDC](docs/OIDC.md) |
| Formats et limites des médias | [Formats médias](docs/MEDIA_FORMATS.md) · [Compatibilité de lecture](docs/STREAMING_COMPATIBILITY.md) |
| Administration et sauvegardes | [Console](docs/ADMIN_CONSOLE.md) · [Sauvegardes](docs/ADMIN_BACKUPS.md) |
| Qualité et CI | [Plan de tests](docs/TEST_PLAN.md) · [CI / GHCR](docs/CI_GHCR.md) |

## 🤝 Contribuer

Les contributions sont les bienvenues : corrections, tests, documentation et améliorations ciblées. Avant d'ouvrir une Pull Request, consultez [`CONTRIBUTING.md`](CONTRIBUTING.md), [`AGENTS.md`](AGENTS.md) pour les conventions techniques et le [code de conduite](CODE_OF_CONDUCT.md).

Les rapports de bugs et propositions de fonctionnalités peuvent être ouverts dans les [Issues GitHub](https://github.com/maelmoreau21/Aether/issues). Pour les problèmes de sécurité, utilisez plutôt le **signalement privé**.

## Licence

Aether est distribué sous **[GNU Affero General Public License v3.0 (AGPL-3.0)](LICENSE)**. Consultez le fichier `LICENSE` pour les conditions applicables à l'utilisation, à la modification et à la redistribution.

Aether ne fournit aucun film ni aucune série. Chaque utilisateur reste responsable du respect des droits d’auteur et des règles applicables à la copie, à l’hébergement et à la mise à disposition de ses contenus.
