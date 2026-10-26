# Guide du versionnement et des releases — Aether

Ce document décrit en détail l'architecture SemVer automatisée, la gestion des branches protégées, la publication Docker GHCR et le cycle de vie des versions du projet **Aether**.

---

## 1. Principes et format des versions

Aether respecte strictement le standard **Semantic Versioning 2.0.0** :

- **Versions stables** : `MAJOR.MINOR.PATCH` (exemples : `0.1.0`, `0.2.0`, `0.2.1`, `1.0.0`).
- **Versions de développement** : `MAJOR.MINOR.PATCH-beta.N` (exemples : `0.2.0-beta.0`, `0.2.0-beta.1`).
- **Tags Git** : Préfixés par la lettre `v` (exemples : `v0.1.0`, `v0.2.0-beta.1`).
- **Builds locaux non publiés** : Version explicite avec suffixe de développement et commit Git (exemple : `0.2.0-beta.0-dev+b08c635`), sans prétendre être une release officielle.

```
       [ main ]  ────────────────────► v0.1.0 (Stable) ──────► v0.2.0 (Stable) ──────► v0.2.1 (Hotfix)
                                         │                       ▲                      ▲
                                         │ Promotion PR          │ Promotion PR         │ Hotfix PR
                                         ▼                       │                      │
    [ develop ]  ──► v0.2.0-beta.0 ──────┴──► v0.2.0-beta.1 ─────┴──► v0.3.0-beta.0 ───┴──► v0.3.0-beta.1
```

---

## 2. Source de vérité unique pour toute l'application

Tous les composants de la plateforme partagent rigoureusement la **même version produit** :
1. **Backend Rust Aether Core** (`aether-core`)
2. **Media Node Rust** (`aether-node`)
3. **Serveur autonome** (`aether` / `media-poc`)
4. **Interface Web SvelteKit** (`apps/web`)
5. **Images Docker GHCR** (`ghcr.io/maelmoreau21/aether`, `aether-core`, `aether-node`)

### Mécanisme de transmission et de détection
- Lors du build Docker ou CI : la variable `MEDIA_VERSION` est transmise comme build-arg et transmise au script de compilation Rust `crates/shared/build.rs`.
- `crates/shared/build.rs` compile cette version dans le binaire Rust (`AETHER_APP_VERSION`), ainsi que le canal (`AETHER_BUILD_CHANNEL`), le commit Git court (`AETHER_GIT_COMMIT`) et la date de compilation (`AETHER_BUILD_DATE`).
- Les binaires exposent leur version effective via l'argument `--version` :
  ```bash
  aether --version
  aether-core --version
  aether-node --version
  ```
- L'API REST Core expose ces métadonnées sur `/api/v1/config` :
  ```json
  {
    "name": "Aether",
    "mode": "standalone",
    "version": "0.2.0-beta.1",
    "channel": "beta",
    "commit": "b08c635",
    "build_date": "2026-10-26",
    "release_url": "https://github.com/maelmoreau21/Aether/releases/tag/v0.2.0-beta.1"
  }
  ```
- L'interface Web SvelteKit interroge ce point de terminaison et présente un affichage discret dans **Paramètres > À propos** (avec lien direct vers la release GitHub officielle le cas échéant).

---

## 3. Cycle de vie automatique sur la branche `develop`

À chaque Pull Request fusionnée dans `develop` :
1. **Contrôles CI complets** : Rust formatting, Clippy strict, tests unitaires et intégration (3 shards), audits de sécurité, compilation SvelteKit, tests Playwright navigateur, et vérifications d'images Docker.
2. **Calcul SemVer bêta** (`python scripts/calculate_release.py --json --channel develop`) :
   - Détermine le cycle en cours (ex : `0.2.0`).
   - Identifie le prochain numéro bêta : `beta.0` → `beta.1` → `beta.2`.
   - **Idempotence garantie** : si le commit courant porte déjà un tag bêta, aucun incrément n'est réalisé et la publication est ignorée.
   - Les commits contenant `[skip ci]` ou `[skip release]` ne génèrent aucun tag.
3. **Création du tag Git** : Tag immutable `v0.2.0-beta.N` pointant vers le commit exact fusionné.
4. **Publication Docker GHCR** :
   - Images publiées : `aether`, `aether-core`, `aether-node`.
   - Tags appliqués : `develop`, `0.2.0-beta.N`, et le SHA long du commit.
   - Tag `latest` n'est **jamais** appliqué sur `develop`.

---

## 4. Cycle de publication stable sur la branche `main`

La branche `main` est protégée. Les modifications et promotions sont réalisées via Pull Request.

### Étapes après fusion d'une release :
1. **Détection de release** :
   - Identifiée par le titre de PR (`release: v0.2.0`), un label GitHub `release`, ou un déclenchement manuel `workflow_dispatch` (avec choix `AUTO`, `PATCH`, `MINOR`, `MAJOR`).
   - Si la fusion sur `main` est purement documentaire ou de maintenance sans intention de release, aucune release n'est créée silencieusement.
2. **Calcul de la version stable** :
   - Première initialisation du dépôt : `0.1.0` (tag `v0.1.0`).
   - Cycle suivant : déterminé selon Conventional Commits (`feat:` -> `MINOR`, `fix:` -> `PATCH`, breaking -> `MAJOR`).
   - Contrôle d'absence de collision : si le tag existe déjà sur un commit différent, le workflow s'arrête avec une erreur explicite.
3. **Construction et vérification des 3 images Docker** :
   - Vérification du démarrage et de l'exécution de `--version`.
   - Vérification de l'absence de bibliothèques partagées superflues sur le runtime Node et vérification de la taille (< 200 Mo).
   - Collecte des digests SHA-256 de chaque image.
4. **Publication GHCR** :
   - Tags appliqués : la version exacte `0.2.0`, la série `0.2`, et le SHA.
5. **Tagging Git et GitHub Release** :
   - Tag Git immutable `v0.2.0` créé sur le commit exact de `main`.
   - GitHub Release créée avec le changelog complet et la table des empreintes Docker (`digests`).
6. **Mise à jour du tag `latest`** :
   - Le tag `latest` est mis à jour sur Docker GHCR **uniquement lorsque l'ensemble de la chaîne est validé**.
7. **Préparation automatique du cycle suivant sur develop** :
   - Le workflow calcule automatiquement la version initiale du cycle suivant (ex : `0.3.0-beta.0`) et pose le tag correspondant sur `develop`, garantissant une continuité immédiate pour les futurs développements.

---

## 5. Procédure des hotfixes (correctifs sur version stable)

Lorsqu'un correctif urgent doit être apporté à une version stable (ex: `0.2.0` -> `0.2.1`) :
1. Créer une branche `hotfix/nom-du-bug` depuis `main`.
2. Appliquer la correction et ajouter les tests de non-régression.
3. Ouvrir une Pull Request ciblant `main` intitulée `fix: description (hotfix 0.2.1)`.
4. À la fusion, le calcul automatique (ou le déclenchement de `release.yml` avec l'option `bump_type: PATCH`) publiera `0.2.1`.
5. Reporter les correctifs dans `develop` via une Pull Request de synchronisation.

---

## 6. Permissions et secrets GitHub Actions

Les workflows utilisent le principe du moindre privilège :
- `contents: read` par défaut pour tous les jobs de vérification et de build.
- `contents: write` uniquement pour les étapes de création de tags Git et de GitHub Releases.
- `packages: write` pour la publication d'images dans GitHub Packages (GHCR).
- Les pull requests non fusionnées ou non fiables ne reçoivent jamais de permission d'écriture ni accès aux secrets de signature.
- Aucun secret n'est loggé ni stocké en clair.

---

## 7. Déploiement et gestion des versions Docker

### Déployer une version précise
Pour garantir la reproductibilité et éviter les surprises de mise à jour intempestive, spécifiez toujours un tag précis :
```yaml
services:
  aether:
    image: ghcr.io/maelmoreau21/aether:0.2.0
    restart: unless-stopped
    ports:
      - "3000:3000"
    volumes:
      - aether_data:/data
      - aether_backups:/backups
      - /volume1/video:/media:ro
```

### Déployer une version de test bêta
```yaml
services:
  aether:
    image: ghcr.io/maelmoreau21/aether:0.2.0-beta.1
```

### Revenir à une version antérieure (Rollback)
Toutes les versions stables et bêtas publiées restent disponibles de manière permanente dans GHCR. Pour revenir en arrière, modifiez le tag dans votre fichier `docker-compose.yml` (ex : remplacer `0.2.1` par `0.2.0`) et exécutez :
```bash
docker compose pull
docker compose up -d
```
Les bases SQLite et PostgreSQL disposent de sauvegardes automatiques dans `/backups` avant toute migration de schéma.
