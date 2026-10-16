# Guide de déploiement Docker autonome & Synology DS1522+

Ce document décrit le déploiement opérationnel d'Aether en conteneur autonome « tout-en-un » via Docker et Docker Compose, avec les spécificités matérielles du NAS Synology DS1522+ et les règles de durcissement en environnement de production.

---

## 1. Vue d'ensemble & Architecture

La distribution Docker autonome d'Aether rassemble dans une image unique :
- L'application Web pré-compilée SvelteKit 5 (servie directement sous `/app/` par Axum).
- Le Core Aether (authentification locale, sessions persistantes, RBAC `administrator`/`user`, catalogue SQLite).
- Le Media Node local (scanner récursif de bibliothèques, extraction NFO, gestion des affiches locales, streaming HTTP Range 64 Kio).
- L'outil système `ffprobe` (analyse technique des conteneurs MKV/MP4, AV1 et Opus).
- Le gestionnaire de processus `tini` (gestion des signaux `SIGINT`, arrêt propre et moissonnage des processus zombies).

### Invariants architecturaux
1. **Zéro runtime Node.js permanent** : Le frontend SvelteKit est compilé statiquement lors de la première étape de build (`adapter-static`) et servi en fichiers statiques par Axum. Aucun interpréteur Node.js n'est présent dans l'image d'exécution.
2. **Processus unique** : Core et Media Node local s'exécutent au sein d'un seul binaire natif (`media-poc`), sans serveur HTTP secondaire ni dépendance externe obligatoire (pas de PostgreSQL ni Valkey pour le mode autonome).
3. **Moindre privilège** : L'image s'exécute sous un utilisateur non-root dédié `aether` (`UID:GID 10001:10001`). Les capacités Linux sont révoquées (`cap_drop: ALL`) et l'élévation de privilèges est interdite (`no-new-privileges: true`).
4. **Volumes étanches** : Séparation stricte entre les bibliothèques médias en lecture seule (`:ro`) et l'état persistant de l'application (`/data`).

---

## 2. Spécificités matérielles du Synology DS1522+

Le Synology DiskStation DS1522+ est équipé d'un processeur **AMD Ryzen Embedded R1600** (architecture Zen 1, 2 cœurs / 4 threads, 2.6 GHz base / 3.1 GHz boost).

### Contraintes matérielles critiques :
- **Absence totale d'iGPU** : Le Ryzen R1600 ne possède aucun processeur graphique intégré (pas d'AMD VCN/VCE, pas d'Intel QuickSync).
- **Interdiction de passthrough `/dev/dri`** : Ne JAMAIS déclarer de montage `/dev/dri:/dev/dri` dans `compose.yaml`. Sur DSM, ce nœud de périphérique n'existe pas et provoquerait le crash immédiat du conteneur au démarrage.
- **Lecture directe (Direct Play) prioritaire** : Aether privilégie la lecture directe AV1 vidéo + Opus audio dans les conteneurs `.mkv` et `.mp4`. Aucun transcodage vidéo en temps réel n'est imposé, garantissant une charge CPU minimale sur le DS1522+.

---

## 3. Topologie des volumes & Données

Le conteneur utilise deux montages distincts :

| Montage | Cible conteneur | Permissions | Usage |
|---|---|---|---|
| `aether_data` | `/data` | `rw` (`0750`, `aether:aether`) | Base SQLite (`core.sqlite3`), journaux WAL/SHM, sessions et index de scan |
| `${MEDIA_LIBRARY_DIR}` | `/media` | `ro` (`0755`) | Dossier racine des médias utilisateur (films, séries) |

### Intégrité des médias en lecture seule (`:ro`)
- Le montage `:ro` garantit qu'aucun fichier vidéo, sous-titre, fichier NFO ou affiche de votre bibliothèque ne peut être modifié, renommé ou altéré par Aether.
- Toute tentative d'écriture dans `/media` est rejetée au niveau du noyau Linux (`Read-only file system`).
- Les métadonnées découvertes et les affiches analysées sont indexées exclusivement dans la base de données `/data/core.sqlite3`.

---

## 4. Configuration réseau & Sécurité

### 4.1 Liaison d'écoute par défaut (Loopback sécurisé)
Dans `compose.yaml`, le port hôte est configuré par défaut sur :
```yaml
ports:
  - "127.0.0.1:3000:3000"
```
Cette liaison restreint l'accès à la boucle locale de l'hôte, empêchant toute exposition accidentelle non authentifiée sur le réseau local (LAN).

### 4.2 Accès direct sur le réseau local (LAN)
Pour autoriser les appareils de votre réseau local à accéder directement à Aether sans proxy inverse :
1. Dans `compose.yaml`, modifiez la redirection de port :
   ```yaml
   ports:
     - "0.0.0.0:3000:3000"
   ```
2. Mettez à jour la variable `MEDIA_PUBLIC_URL` avec l'adresse IP de votre NAS ou machine :
   ```yaml
   environment:
     - MEDIA_PUBLIC_URL=http://192.168.1.100:3000
   ```
3. Les en-têtes HTTP `Host` et `Origin` reçus doivent correspondre exactement à `MEDIA_PUBLIC_URL` pour franchir la protection CSRF et DNS-rebinding.

### 4.3 Exposition via Proxy Inverse HTTPS (Synology Reverse Proxy, Nginx, Caddy, Traefik)
Pour un accès sécurisé depuis Internet ou votre domaine local en HTTPS :
1. Conservez la liaison loopback locale `"127.0.0.1:3000:3000"` sur le conteneur.
2. Définissez `MEDIA_PUBLIC_URL` avec l'URL publique HTTPS canonique :
   ```yaml
   environment:
     - MEDIA_PUBLIC_URL=https://aether.mondomaine.fr
   ```
3. **Obligation de transfert d'en-tête `Host`** : Configurez votre proxy inverse pour relayer l'en-tête `Host` réel du client :
   - **Nginx** :
     ```nginx
     proxy_set_header Host $host;
     proxy_set_header X-Forwarded-Proto $scheme;
     proxy_set_header X-Forwarded-For $remote_addr;
     proxy_pass http://127.0.0.1:3000;
     ```
   - **Synology DSM Reverse Proxy** : Activé automatiquement via le portail de connexion / proxy inverse DSM.
   - **Caddy** :
     ```caddy
     aether.mondomaine.fr {
         reverse_proxy 127.0.0.1:3000
     }
     ```
4. **Sonde de santé découplée** : L'endpoint interne `/api/v1/health` est découplé de la validation stricte d'hôte externe, permettant à Docker d'interroger `http://127.0.0.1:3000/api/v1/health` avec succès même lorsque `MEDIA_PUBLIC_URL` est configuré sur un domaine HTTPS externe.

---

## 5. Variables d'environnement de référence

| Variable | Valeur par défaut conteneur | Description |
|---|---|---|
| `MEDIA_PORT` | `3000` | Port d'écoute interne du serveur HTTP |
| `MEDIA_BIND_ADDR` | `0.0.0.0` | Adresse IP d'écoute à l'intérieur du conteneur |
| `MEDIA_PUBLIC_URL` | `http://127.0.0.1:3000` | Origine publique canonique validée pour `Host`, `Origin` et cookies de session |
| `MEDIA_DB_PATH` | `/data/core.sqlite3` | Chemin du fichier SQLite dans le volume persistant |
| `MEDIA_LIBRARY_ROOT` | `/media` | Racine absolue du dossier des bibliothèques montées en lecture seule |
| `MEDIA_WEB_DIST` | `/app/apps/web/build` | Répertoire des assets Web statiques pré-compilés |

---

## 6. Procédure de déploiement pas à pas

### Étape 1 : Préparation de l'arborescence (ex. sur Synology DSM)
Connectez-vous en SSH à votre machine ou Synology, ou créez un projet dans **Synology Container Manager** :
```bash
mkdir -p /volume1/docker/aether
cd /volume1/docker/aether
```
Placez-y les fichiers `compose.yaml` (ou `docker-compose.yml`) et le fichier `.env` suivant :
```env
# Chemin vers votre dossier de vidéos existant
MEDIA_LIBRARY_DIR=/volume1/video
```

### Étape 2 : Construction et lancement du service
Si vous construisez l'image localement à partir des sources :
```bash
docker compose build
docker compose up -d
```
Vérifiez l'état de santé du conteneur :
```bash
docker compose ps
```
Le conteneur doit afficher l'état `Up ... (healthy)` après environ 6 à 10 secondes.

### Étape 3 : Amorçage sécurisé du premier administrateur (Bootstrap)
Aether n'inclut **aucun identifiant ni mot de passe par défaut**. Le premier compte administrateur local doit être créé de façon interactive via le terminal :

```bash
docker compose exec -it aether media-bootstrap
```
Le programme vous invite à saisir :
1. L'identifiant de connexion en minuscules (ex. `admin`).
2. Le mot de passe (saisie masquée sans écho).
3. La confirmation du mot de passe (saisie masquée).

```
Local administrator bootstrap. Protect the database directory with OS permissions.
Login (lowercase): admin
Password: [masqué]
Confirm password: [masqué]
Administrator created. Sign in through /app/login/.
```

> **Sécurité renforcée de l'amorçage :**
> - L'exécution non interactive sans terminal échoue immédiatement avec le code 1 (`bootstrap requires a local interactive terminal and accepts no arguments`).
> - Toute tentative ultérieure de réexécution de `media-bootstrap` est définitivement verrouillée par la base SQLite (`initial bootstrap is already closed`).

### Étape 4 : Connexion et création de bibliothèque
1. Ouvrez votre navigateur sur l'adresse configurée (ex. `http://127.0.0.1:3000/app/login/` ou votre URL publique).
2. Connectez-vous avec les identifiants créés.
3. Rendez-vous dans **Administration > Bibliothèques** (`/app/admin/libraries/`).
4. Créez une bibliothèque de type Films avec la racine `/media` (ou un sous-dossier de `/media`).
5. Déclenchez le scan. `ffprobe` analyse les fichiers sans modifier le dossier `:ro`, et peuple le catalogue ainsi que les affiches.

---

## 7. Exploitation, Maintenance & Sauvegarde

### Arrêt propre et reprise
Aether est configuré avec `stop_signal: SIGINT` et `stop_grace_period: 15s`. Lors d'un arrêt :
```bash
docker compose down
```
- Le processus `tini` transmet le signal `SIGINT` à `media-poc`.
- Les tâches de scan en cours sont annulées proprement.
- Les connexions SQLite sont vidées et le journal WAL est synchronisé.
- Au redémarrage (`docker compose up -d`), les scans interrompus sont automatiquement marqués en état `error` avec le libellé `"Scan interrompu par un redémarrage"` et l'inventaire existant est intégralement préservé.

### Sauvegarde et restauration
Toutes les données applicatives (comptes, mots de passe hashés Argon2id, sessions, catalogue, droits) résident dans `/data/core.sqlite3`.
- **Sauvegarde à froid** (recommandée) :
  ```bash
  docker compose stop
  cp /volume1/docker/aether_data/core.sqlite3* /volume1/backups/
  docker compose start
  ```
- **Sauvegarde à chaud** via l'API SQLite :
  ```bash
  docker compose exec aether sqlite3 /data/core.sqlite3 ".backup '/data/backup.sqlite3'"
  ```

---

## 8. Métriques de performance mesurées en environnement réel

Mesures réelles enregistrées lors de la validation automatisée de bout en bout (AC-11) :

| Métrique | Valeur mesurée | Note |
|---|---|---|
| **Taille du contenu de l'image** | **213 Mo** | Image optimisée multi-stage Debian bookworm-slim avec `ffprobe` et `tini` |
| **Empreinte disque des couches** | 790 Mo | Couches d'image décompressées sur l'hôte Docker |
| **Délai de démarrage vers `healthy`** | **6,34 secondes** | Migrations SQLite automatiques, initialisation du pool et première sonde HTTP réussie |
| **Mémoire RSS du conteneur au repos (idle)** | **2,88 Mo** | Empreinte mémoire minimale après démarrage complet et service HTTP actif |
| **Mémoire RSS pic pendant le scan ffprobe** | **12,56 Mo** | Parcours récursif d'une bibliothèque et extraction de métadonnées |

Ces mesures confirment qu'Aether s'exécute avec une empreinte système particulièrement légère, parfaitement dimensionnée pour le Synology DS1522+ et les serveurs personnels à ressources modérées.
