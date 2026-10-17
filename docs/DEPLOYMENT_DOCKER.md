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
| `MEDIA_SQLITE_BUSY_TIMEOUT_SECS` | `15` | Délai d'attente max sur verrou d'écriture SQLite avant erreur BUSY (45s+ recommandé sur 10k+ médias) |
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

### Sauvegarde et restauration fiables (SQLite WAL & Docker)

Toutes les données applicatives (comptes administrateur/utilisateurs, hachages Argon2id, sessions, graphe média, catalogue, bibliothèques et autorisations) résident dans `/data/core.sqlite3` avec le journal WAL SQLite.

> [!CAUTION]
> **Une sauvegarde stockée dans `/data/backups` NE PROTÈGE PAS contre la perte du volume Docker.**
> Le dossier `/data/backups` réside à l'intérieur même du volume nommé Docker `aether_data`. Si ce volume est supprimé (`docker volume rm`), corrompu ou détruit avec le disque hôte, **toutes les sauvegardes internes à `/data/backups` sont irrémédiablement perdues**.
> Pour assurer une véritable reprise après sinistre, toute sauvegarde doit être **exportée hors du volume `aether_data`**, vers un stockage externe (partage NAS `/volume1/backups`, disque externe, ou copie via `docker cp`).

> [!WARNING]
> **Ne jamais effectuer de copie naïve du seul fichier `core.sqlite3` à chaud** : en mode SQLite WAL, les transactions récentes résident dans `core.sqlite3-wal` et un instantané fichier direct produit une base corrompue ou incomplète.

Aether intègre deux binaires natifs dédiés, légers et autonomes : `media-backup` et `media-restore` (aucun service tiers ou dépendance externe requis).

#### Périmètre et confidentialité des sauvegardes
- **Aucun média original inclus** : La sauvegarde contient exclusivement l'état applicatif SQLite (schéma, utilisateurs, hachages, sessions, métadonnées éditoriales, catalogue et permissions). Les fichiers volumineux de vidéos, pistes audio, sous-titres et affiches situés dans `/media` ne sont **jamais** dupliqués dans la sauvegarde.
- **Confidentialité des données sensibles** : SQLite contient des données sensibles (hachages de mots de passe Argon2id, jetons de sessions). Les fichiers de sauvegarde sont créés avec des permissions strictes `0600` (`rw-------`, propriétaire seul) et le dossier en `0750`. Les utilitaires `media-backup` et `media-restore` n'affichent **aucun hachage, mot de passe ou secret dans les journaux ou la console** (uniquement les compteurs globaux).

#### 1. Sauvegarde à chaud (cohérente WAL, sans coupure de service)
Exécutable en direct sans interrompre les utilisateurs connectés ni le streaming en cours :

```bash
# Sauvegarde interne (horodatée automatiquement)
docker compose exec aether media-backup

# Sauvegarde exportable directement vers un dossier hôte externe (ex. /volume1/backups sur Synology)
# Requiert le montage du dossier hôte dans compose.yaml (ex. - /volume1/backups:/backups) :
docker compose exec aether media-backup /backups/core_backup_$(date +%Y%m%d_%H%M%S).sqlite3

# Ou avec remplacement explicite et sécurisé d'une sauvegarde existante :
docker compose exec aether media-backup --replace /backups/core_backup_latest.sqlite3
```

Si le dossier hôte n'est pas monté dans le conteneur, exportez immédiatement la sauvegarde via `docker cp` :
```bash
# 1. Sauvegarde dans le volume
docker compose exec aether media-backup /data/backups/export_temp.sqlite3

# 2. Exportation vers le système de fichiers hôte
docker cp $(docker compose ps -q aether):/data/backups/export_temp.sqlite3 /volume1/backups/aether_backup_$(date +%Y%m%d_%H%M%S).sqlite3

# 3. Nettoyage du fichier temporaire du conteneur
docker compose exec aether rm /data/backups/export_temp.sqlite3
```

- **Mécanisme et protection contre la perte de la copie précédente** :
  - `media-backup` refuse par défaut d'écraser un fichier existant (`TargetExists`).
  - En cas de remplacement demandé (`--replace`), l'utilitaire **ne supprime jamais la copie existante** au préalable. Il génère d'abord un instantané temporaire unique (`.backup-<pid>-<uuid>.tmp`) dans le même répertoire via SQLite `VACUUM INTO`.
  - Il valide ensuite l'intégrité (`PRAGMA integrity_check`) et le schéma (présence des tables `users`, `local_credentials`, `libraries`, `inventory_works`), applique les permissions `0600` et synchronise les blocs sur disque (`sync_all()`).
  - La publication finale remplace l'ancienne copie de façon atomique (`std::fs::rename`).
  - En cas d'erreur ou d'interruption à n'importe quelle étape, le fichier temporaire est immédiatement nettoyé et la sauvegarde existante reste **strictement intacte**.

#### 2. Sauvegarde à froid du volume Docker (Synology DSM ou Linux)
Si vous préférez archiver l'intégralité du volume Docker lors d'une fenêtre de maintenance :
```bash
docker compose stop aether
docker run --rm -v aether_data:/data -v /volume1/backups:/backup alpine tar czf /backup/aether_data_$(date +%Y%m%d_%H%M%S).tar.gz -C /data .
docker compose start aether
```

#### 3. Restauration sûre : volume neuf sans écraser l'ancien
Pour prévenir toute perte accidentelle de données, **`media-restore` interdit strictement d'écraser une base existante** et refuse de s'exécuter si des journaux résiduels (`-wal`, `-shm`) ou des symlinks sont détectés dans le dossier cible. Il n'existe aucune option `--force` permettant de contourner cette sécurité.

Pour restaurer après un incident ou changer de version, appliquez la procédure hors ligne dans un **volume neuf** :

1. **Arrêtez le conteneur Aether existant** (l'ancien volume `aether_data` reste intact) :
   ```bash
   docker compose stop aether
   ```

2. **Créez un volume Docker neuf et vide** :
   ```bash
   docker volume create aether_data_restored
   ```

3. **Exécutez `media-restore` depuis la sauvegarde externe vers le nouveau volume** :
   ```bash
   docker run --rm \
     -v aether_data_restored:/data \
     -v /volume1/backups:/backup:ro \
     aether media-restore /backup/core_backup_latest.sqlite3 /data/core.sqlite3
   ```

4. **Basculez le service sur le volume restauré** :
   - Dans votre fichier `compose.yaml`, modifiez la déclaration du volume persistant :
     ```yaml
     volumes:
       aether_data:
         external: true
         name: aether_data_restored
     ```
   - Ou conservez le nom `aether_data` en archivant préalablement l'ancien volume.

5. **Démarrez Aether et vérifiez le catalogue** :
   ```bash
   docker compose up -d
   ```

6. **Garanties opérationnelles vérifiées après restauration** :
   - **Intégrité vérifiée avant écriture** : `media-restore` contrôle `PRAGMA integrity_check` et valide les tables de schéma avant de toucher au dossier cible.
   - **Zéro journal incompatible** : les fichiers `-wal` et `-shm` résiduels ne sont jamais supprimés silencieusement ; la restauration exige une destination vierge.
   - **Préservation déterministe** : Les identifiants UUID des œuvres, éditions, versions, sources et assets sont conservés à l'identique.
   - **Authentification préservée** : L'administrateur et les utilisateurs se reconnectent immédiatement avec leurs mots de passe d'origine (hachages Argon2id intacts).
   - **Permissions et bibliothèques intactes** : Les droits d'accès utilisateurs (`library_permissions`) et les chemins de montage sont restaurés sans modification.

---

### Concurrence des écritures et disponibilité des lectures authentifiées sous publication SQLite
Lors de l'ingestion initiale d'une volumineuse bibliothèque (ex. 10 000 nouveaux médias), la transaction d'écriture exclusive de publication (`publish_staged_scan` via `BEGIN IMMEDIATE`) peut être maintenue active pendant environ 30 secondes.

- **Lectures authentifiées et non authentifiées non bloquées (WAL)** :
  - `AuthState::authenticate()` valide chaque requête via un `SELECT` SQLite en lecture seule joignant `sessions`, `users` et `local_credentials` (vérification de l'existence de la session, de l'expiration absolue 12 h, de l'expiration inactive 30 min et de l'état actif `disabled_at IS NULL`, sans aucun cache positif d'autorisation).
  - En mode SQLite WAL, `/api/v1/auth/me`, la navigation dans le catalogue Films/Séries, les descripteurs d'épisodes et le streaming média HTTP Range n'attendent plus le verrou d'écriture (latences mesurées sous verrou `BEGIN IMMEDIATE` de 30 s : `p50 = 3,90 ms`, `p95 = 6,44 ms`, `p99 = 8,54 ms`, `0` erreur HTTP 500).
- **Mise à jour d'activité de session regroupée et différée (`SessionToucher`)** :
  - Le rafraîchissement de `sessions.last_seen_at` est coalescé par session (`TOUCH_INTERVAL_SECONDS = 60 s`) dans une file bornée (`MAX_PENDING_TOUCHES = 4096`) traitée par un worker unique par lots (`TOUCH_BATCH_SIZE = 64`).
  - En cas de verrou d'écriture actif, le worker n'immobilise pas les connexions du pool (`busy_timeout` court de 15 ms par tentative puis backoff de 150 ms) et applique un `UPDATE` conditionnel dès la libération du verrou sans jamais ressusciter une session expirée ou révoquée.
  - **Compromis fail-closed sur l'expiration glissante** : SQLite restant l'unique source de vérité d'autorisation, une activité survenant dans la dernière minute avant l'expiration d'inactivité de 30 min qui ne peut pas être persistée avant l'échéance (en raison d'un verrou d'écriture prolongé jusqu'au-delà de la limite de 30 min ou d'un redémarrage brutal avant flush) échoue de manière fermée (`401 Unauthorized`).
- **Mutations explicites et `MEDIA_SQLITE_BUSY_TIMEOUT_SECS`** :
  - Les routes d'écriture explicites (connexion `POST /api/v1/auth/login`, déconnexion `POST /api/v1/auth/logout`, création/modification de bibliothèque) doivent acquérir le verrou d'écriture SQLite et patientent jusqu'à `MEDIA_SQLITE_BUSY_TIMEOUT_SECS` (défaut 15 secondes).

---

### Diagnostics des scans et purge du staging
Aether optimise l'espace disque et la clarté des diagnostics :
- **Plafond mémoire et affichage borné** : Les 100 premières anomalies (erreurs d'analyse de conteneurs, XML NFO invalides, conflits de métadonnées) sont conservées et renvoyées dans le tableau `errors` de l'API `/api/v1/admin/libraries/{id}/scan` pour affichage dans l'interface web.
- **Compteurs totaux fiables** : Les champs `errorsCount` et `warningsCount` indiquent le nombre total exact d'anomalies détectées lors de la passe, même lorsque celles-ci dépassent le plafond de 100 éléments.
- **Purge systématique du staging** : Dès la fin de la publication atomique, les tables temporaires `scan_staging_files` et `scan_staging_issues` sont intégralement vidées (`DELETE FROM ...`), empêchant toute croissance non maîtrisée du fichier SQLite `/data/core.sqlite3`.
- **Protection contre les fuites de chemins** : Les messages d'anomalie n'exposent que les chemins relatifs à la racine de la bibliothèque. Tout chemin absolu d'hôte système en cas d'erreur fatale est automatiquement assaini (`/media`).

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
