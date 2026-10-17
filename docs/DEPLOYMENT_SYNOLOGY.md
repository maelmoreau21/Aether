# Déploiement d'Aether sur Synology DS1522+ (Standalone & Distribué P2)

Ce guide fournit les instructions exactes pour installer et exploiter **Aether** sur un NAS **Synology DS1522+** (processeur AMD Ryzen Embedded R1600 `x86_64` sans iGPU) sous DSM 7.2+ avec **Container Manager** (ou `docker compose` en SSH), en coexistence totale et sans risque avec Jellyfin.

---

## 1. Particularités matérielles et garanties de sécurité sur Synology DS1522+

1. **Absence d'iGPU (`AMD Ryzen R1600`)** :
   - Le DS1522+ ne possède pas d'accélération matérielle vidéo (`/dev/dri` n'existe pas).
   - **Ne jamais ajouter `devices: ["/dev/dri:/dev/dri"]`** dans vos fichiers Compose sur DS1522+ : cela empêcherait le démarrage du conteneur.
   - Aether fonctionne **sans aucun transcodage vidéo obligatoire** (lecture directe `GET`/`HEAD`/HTTP `206 Partial Content` par tranches bornées de `64 KiB`).
2. **Coexistence sûre avec Jellyfin (montages `:ro`)** :
   - Vos dossiers partagés Synology (`/volume1/video/Films`, `/volume1/video/Series`, etc.) sont montés **strictement en lecture seule (`:ro`)**.
   - Aether lit les fichiers `.mkv`, `.mp4`, `.nfo` et posters existants sans jamais écrire, renommer ou supprimer quoi que ce soit dans vos dossiers médias.
3. **Utilisateur non-privilégié (`10001:10001`)** :
   - Les conteneurs `aether`, `aether-core` et `aether-node` s'exécutent sous l'utilisateur non-root `aether` (`UID=10001`, `GID=10001`) avec `cap_drop: [ALL]` et `no-new-privileges: true`.
   - Sur DSM, assurez-vous que vos dossiers médias `/volume1/video/...` disposent des permissions de lecture pour « Tout le monde » (`Everyone: Read` ou `chmod o+rX` sur les dossiers médias), tandis que les dossiers techniques `/volume1/docker/aether-...` appartiennent à `10001:10001`.

---

## 2. Mode 1 : Déploiement Standalone (un seul conteneur `aether`)

Idéal lorsque le Core et toutes vos bibliothèques résident sur le même Synology DS1522+.

### 2.1 Préparation des dossiers sur le Synology (en SSH)

```bash
sudo mkdir -p /volume1/docker/aether/data /volume1/docker/aether/backups
sudo chown -R 10001:10001 /volume1/docker/aether/data /volume1/docker/aether/backups
sudo chmod 0750 /volume1/docker/aether/data /volume1/docker/aether/backups
```

### 2.2 Construction et lancement

Copiez le dépôt sur le NAS (ex. `/volume1/docker/aether/src`), créez votre fichier `.env` à partir de [`.env.example`](file:///c:/Users/Mael/Documents/GitHub/Aether/.env.example) :

```dotenv
MEDIA_PUBLIC_URL=https://media.votre-domaine.lan
MEDIA_LIBRARY_DIR=/volume1/video
```

Puis lancez :

```bash
docker compose -f compose.yaml up -d --build
```

### 2.3 Création du premier administrateur local

Aucun compte ni mot de passe par défaut n'existe. Initialisez le premier administrateur local :

```bash
docker compose -f compose.yaml exec -it aether media-bootstrap --login admin
```

---

## 3. Mode 2 : Déploiement Distribué Multi-Node (`aether-core` + `aether-node`)

Ce mode sépare physiquement et logiquement :
- **`aether-core`** : API publique `/api/v1/...`, comptes utilisateurs, RBAC, catalogue global SQLite et interface Web statique SvelteKit. **Ne monte aucun dossier média.**
- **`aether-node`** (un ou plusieurs conteneurs sur le même NAS ou sur d'autres machines) : scanne ses propres bibliothèques montées en `:ro`, analyse les pistes via `ffprobe`, lit les `.nfo`/posters et sert les flux vidéos/images autorisés par jeton court (`PlaybackToken`) ou via relais Core borné.

### 3.1 Préparation des dossiers et secrets d'appairage

```bash
sudo mkdir -p \
  /volume1/docker/aether-core/data \
  /volume1/docker/aether-core/backups \
  /volume1/docker/aether-node-films/data \
  /volume1/docker/aether-node-series/data

sudo chown -R 10001:10001 \
  /volume1/docker/aether-core \
  /volume1/docker/aether-node-films \
  /volume1/docker/aether-node-series
```

Créez un fichier `.env` avec deux jetons d'appairage aléatoires (`openssl rand -hex 24`) :

```dotenv
MEDIA_PUBLIC_URL=http://127.0.0.1:3000
MEDIA_CORE_INTERNAL_URL=http://aether-core:3000

MEDIA_FILMS_DIR=/volume1/video/Films
MEDIA_SERIES_DIR=/volume1/video/Series

MEDIA_NODE_FILMS_NAME=Node Films Synology
MEDIA_NODE_FILMS_PORT=4101
MEDIA_NODE_FILMS_PUBLIC_URL=http://127.0.0.1:4101
MEDIA_NODE_FILMS_PAIRING_TOKEN=remplacer_par_secret_aleatoire_films

MEDIA_NODE_SERIES_NAME=Node Séries Synology
MEDIA_NODE_SERIES_PORT=4102
MEDIA_NODE_SERIES_PUBLIC_URL=http://127.0.0.1:4102
MEDIA_NODE_SERIES_PAIRING_TOKEN=remplacer_par_secret_aleatoire_series
```

### 3.2 Démarrage des 3 conteneurs et bootstrap administrateur

```bash
docker compose -f compose.distributed.yaml up -d --build
docker compose -f compose.distributed.yaml exec -it aether-core media-bootstrap --login admin
```

### 3.3 Appairage des Nodes et scan des bibliothèques depuis l'interface Web

1. Ouvrez `/app/` et connectez-vous avec votre compte administrateur.
2. Accédez à **Bibliothèques** (`/app/admin/libraries/`).
3. Dans la section **Media Nodes distribués (`aether-node`)**, appairez chaque Node :
   - **Node 1 (Films)** :
     - Nom : `Node Films Synology`
     - URL interne Node : `http://aether-node-films:4101`
     - URL publique directe : `http://127.0.0.1:4101` (ou l'URL HTTPS de votre proxy inverse pour ce Node ; laissez vide pour forcer le relais via Core)
     - Jeton d'appairage : valeur de `MEDIA_NODE_FILMS_PAIRING_TOKEN`
   - **Node 2 (Séries)** :
     - Nom : `Node Séries Synology`
     - URL interne Node : `http://aether-node-series:4102`
     - URL publique directe : `http://127.0.0.1:4102`
     - Jeton d'appairage : valeur de `MEDIA_NODE_SERIES_PAIRING_TOKEN`
4. Dans le formulaire **Nouvelle bibliothèque** :
   - Créez la bibliothèque **Films** en sélectionnant `Node Films Synology` et la racine `/media/films`.
   - Créez la bibliothèque **Séries** en sélectionnant `Node Séries Synology` et la racine `/media/series`.
5. Cliquez sur **Scanner** sur chacune des bibliothèques : chaque Node parcourt ses fichiers locaux en `:ro`, transmet les observations par lots signés (`HMAC-SHA256`) au Core, et les œuvres apparaissent dans le catalogue global.

---

## 4. Accès LAN / Internet via le Reverse Proxy HTTPS de Synology DSM

Par sécurité, le Core refuse de démarrer si `MEDIA_PUBLIC_URL` utilise `http://` sur une adresse IP non-loopback (afin d'interdire la transmission en clair des mots de passe et cookies de session sur le réseau local).

Pour accéder à Aether depuis vos appareils sur le LAN ou à distance :
1. Dans **DSM → Panneau de configuration → Portail de connexion → Avancé → Proxy inversé**, créez une règle :
   - **Source** : Protocole `HTTPS`, Nom d'hôte `media.votre-nas.lan`, Port `8443` (ou `443`).
   - **Destination** : Protocole `HTTP`, Nom d'hôte `127.0.0.1`, Port `3000`.
   - Dans l'onglet **En-tête personnalisé**, vérifiez que l'en-tête `Host` est bien conservé.
2. Configurez `MEDIA_PUBLIC_URL=https://media.votre-nas.lan:8443` dans `.env` et redémarrez `aether` / `aether-core`.
3. Si vous utilisez des Nodes distants en lecture directe depuis le navigateur, exposez uniquement leurs routes de streaming derrière un proxy HTTPS dédié (ou laissez `publicUrl` vide lors de l'appairage pour que le Core relaie les flux en `64 KiB` via l'origine HTTPS principale sans ouvrir de port supplémentaire). **N'exposez jamais les ports internes `4101`/`4102` directement sur Internet.**
