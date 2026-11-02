## UID/GID personnalises et volumes applicatifs

`user: "1039:65539"` remplace l'utilisateur de l'image (10001:10001), mais ne
change pas les proprietaires des fichiers dans les volumes Docker existants.
Un /data appartenant a 10001:10001 en mode 750 refuse donc l'acces a 1039:65539
sans droit supplementaire. Le montage rw ne contourne pas les droits Unix/ACL.

Conserver l'UID/GID NAS peut etre utile pour lire les medias. Arreter le conteneur,
relever les sources exactes de /data et /backups avec `docker inspect`, puis
aligner leur proprietaire sur l'UID/GID choisi depuis l'hote NAS. Cette operation
concerne exclusivement les volumes applicatifs identifies, jamais les dossiers
Films/Series/Music ou ceux de Jellyfin. Ne pas appliquer chmod 777. Un chown dans
le conteneur avec cap_drop ALL peut echouer, meme via docker exec -u root.
Redemarrer puis verifier l'identite et l'ecriture en tant qu'utilisateur normal
dans /data, /data/plugins, /data/preparation-cache et /backups. Si un refus reste
present apres alignement, examiner les ACL DSM des volumes concernes.

> Mise à jour sécurité P2 du 17 octobre : lire [le contrat et les preuves](P2_DOCKER_VALIDATION.md).
> Les anciennes affirmations de sécurité distribuée ne prouvent pas HTTPS entre machines.

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
docker compose up -d
```
*Note : Docker Compose télécharge directement l'image pré-compilée `ghcr.io/maelmoreau21/aether:latest`. Ajoutez `--build` uniquement si vous souhaitez compiler localement depuis les sources.*

### 2.3 Création du premier administrateur local

Aucun compte ni mot de passe par défaut n'existe. Initialisez le premier administrateur local :

```bash
docker compose exec -it aether media-bootstrap --login admin
```

---

## 3. Mode 2 : Déploiement Distribué Multi-Node (`aether-core` + `aether-node`)

Ce mode sépare physiquement et logiquement :
- **`aether-core`** : API publique `/api/v1/...`, comptes utilisateurs, RBAC, catalogue global SQLite et interface Web statique SvelteKit. **Ne monte aucun dossier média.**
- **`aether-node`** (Node polyvalent par défaut, ou plusieurs conteneurs si besoin) : monte à la fois `/media/films:ro` et `/media/series:ro`, scanne ses bibliothèques en `:ro`, analyse les pistes via `ffprobe`, lit les `.nfo`/posters et sert les flux vidéos/images autorisés par jeton court (`PlaybackToken`) ou via relais Core borné.

### 3.1 Préparation des dossiers et secret d'appairage

```bash
sudo mkdir -p \
  /volume1/docker/aether-core/data \
  /volume1/docker/aether-core/backups \
  /volume1/docker/aether-node/data

sudo chown -R 10001:10001 \
  /volume1/docker/aether-core \
  /volume1/docker/aether-node
```

Créez un fichier `.env` avec le secret d'appairage aléatoire (`openssl rand -hex 24`) :

```dotenv
MEDIA_PUBLIC_URL=http://127.0.0.1:3000
MEDIA_CORE_INTERNAL_URL=http://aether-core:3000

MEDIA_FILMS_DIR=/volume1/video/Films
MEDIA_SERIES_DIR=/volume1/video/Series

MEDIA_NODE_NAME=Node Principal Synology
MEDIA_NODE_PORT=4101
MEDIA_NODE_PUBLIC_URL=http://127.0.0.1:4101
MEDIA_NODE_PAIRING_TOKEN=remplacer_par_secret_aleatoire_hex_48_caracteres
```

### 3.2 Démarrage des conteneurs et bootstrap administrateur

```bash
docker compose -f compose.distributed.yaml up -d
docker compose -f compose.distributed.yaml exec -it aether-core media-bootstrap --login admin
```
*Note : Docker Compose télécharge automatiquement les images officielles `ghcr.io/maelmoreau21/aether-core:latest` et `ghcr.io/maelmoreau21/aether-node:latest`. Ajoutez `--build` pour compiler localement.*

### 3.3 Appairage du Node et configuration des bibliothèques depuis l'interface Web

1. Ouvrez `/app/` et connectez-vous avec votre compte administrateur.
2. Accédez à **Bibliothèques** (`/app/admin/libraries/`).
3. Dans la section **Media Nodes distribués (`aether-node`)**, appairez le Node polyvalent :
   - Nom : `Node Principal Synology`
   - URL interne Node : `http://aether-node:4101`
   - URL publique directe : `http://127.0.0.1:4101` (ou l'URL HTTPS de votre proxy inverse pour ce Node ; laissez vide pour forcer le relais via Core)
   - Jeton d'appairage : valeur de `MEDIA_NODE_PAIRING_TOKEN`
4. Une fois appairé, le Node expose automatiquement ses deux racines autorisées (`/media/films` et `/media/series`).
5. Dans le formulaire **Nouvelle bibliothèque**, ajoutez les deux types de bibliothèques sur ce même Node :
   - Bibliothèque 1 : Nom `Films`, Type `Films`, Node `Node Principal Synology`, Racine `/media/films`.
   - Bibliothèque 2 : Nom `Séries`, Type `Séries`, Node `Node Principal Synology`, Racine `/media/series`.
6. Cliquez sur **Scanner** sur chaque bibliothèque pour indexer les médias.

*Note : La topologie permet également d'ajouter d'autres Nodes indépendants sur le réseau si vous disposez de plusieurs machines de stockage, sans jamais imposer un Node dédié par type de média.*

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

## Procédure P2 après validation locale, sans déploiement automatique

Conserver les volumes de données existants ; ne pas lancer `down -v`. Les deux
modes restent utilisables. Le test local n'autorise aucun accès au NAS.

### Préparer les images sur la machine de construction x86_64

```sh
docker build --target runtime -t aether:latest .
docker build --target runtime-core -t aether-core:latest .
docker build --target runtime-node -t aether-node:latest .
docker save -o aether-images.tar aether:latest aether-core:latest aether-node:latest
```

Transférer manuellement l'archive sur le NAS et l'importer dans Container Manager
ou avec `docker load -i aether-images.tar`. Aucun registre ou push requis. Le
DS1522+ est x86_64 ; les tests de ce jalon ne prouvent pas une image arm64.

### Standalone, un conteneur

Copier `docker-compose.yml`, `.env.example` et les documents. Préparer `.env` en privé,
régler `MEDIA_LIBRARY_DIR=/volume1/video` et `MEDIA_PUBLIC_URL` à l'origine HTTPS
exacte pour un accès distant. Pour un essai local sur le NAS, garder l'origine
loopback. Le volume nommé existant `aether_data` est conservé et les médias restent
`:ro`. Ne pas réutiliser une base de production pour un essai.

```sh
docker compose up -d
docker compose exec -it aether media-bootstrap
```

Le bootstrap est réservé à une base neuve ; une base déjà initialisée utilise son
compte existant. Aucune identité par défaut. Le proxy DSM préserve Host et ne
publie que l'origine HTTPS ; le port Core reste lié à `127.0.0.1`.

### Distribué mono-hôte, bridge de confiance explicite

Créer un secret d'appairage aléatoire sans l'afficher :

```sh
umask 077
cp .env.example .env
node_secret=$(openssl rand -hex 32)
sed -i "s/^MEDIA_NODE_PAIRING_TOKEN=.*/MEDIA_NODE_PAIRING_TOKEN=$node_secret/" .env
unset node_secret
```

Ne pas activer `set -x`. Régler ensuite les deux chemins médias et l'origine publique
dans ce fichier privé. Le Compose exige le secret, le Node refuse un secret non
hexadécimal ou trop court (< 24 octets).

```sh
docker compose -f compose.distributed.yaml up -d
docker compose -f compose.distributed.yaml exec -it aether-core media-bootstrap
```

Dans l'administration, appairer le Node polyvalent avec `http://aether-node:4101`
et son secret. Créer les bibliothèques Films (`/media/films`) et Séries (`/media/series`)
depuis les racines annoncées puis scanner. Ce HTTP est limité au bridge du même hôte
déclaré fiable dans le Compose. Plusieurs Nodes indépendants peuvent également être ajoutés.

### Plusieurs machines, HTTPS obligatoire

Utiliser les services correspondants de `compose.multi-machine.example.yaml`,
un par machine. Remplacer tous les domaines par les domaines réels. Core doit
mettre exactement `node-films.<domaine>:443,node-series.<domaine>:443` dans
`MEDIA_INTERNAL_ALLOWED_AUTHORITIES`; chaque Node autorise seulement
`media.<domaine>:443`. Garder `MEDIA_INTERNAL_TRUSTED_HTTP=false`.

Chaque proxy écoute en HTTPS avec un certificat dont le SAN couvre son domaine.
Si CA privée : monter `./ca.pem:/run/certs/internal-ca.pem:ro` dans Core et chaque
Node, puis définir `MEDIA_INTERNAL_CA_FILE=/run/certs/internal-ca.pem` dans ces
services. Ne jamais installer la clé privée de la CA dans les conteneurs.
Le proxy Core doit transmettre `/internal/v1/core/*` aux Nodes autorisés et les
routes publiques au navigateur ; le proxy Node transmet les routes internes
uniquement au Core autorisé et `/stream/v1/*` aux clients autorisés. Filtrer les
sources par pare-feu ; ne pas publier automatiquement les ports internes.

Configurer `MEDIA_CORE_INTERNAL_URL=https://media.<domaine>` sur Core, appairer
avec les deux origines HTTPS des Nodes. Pour le direct, leur URL publique doit
être joignable en HTTPS par le navigateur, certificat approuvé aussi côté client.
Pour le relais uniquement, vider aussi `MEDIA_NODE_PUBLIC_URL` dans le service Node,
le recréer avec son volume existant, puis reconnecter le Node côté Core ; pour
un premier appairage, garder également le champ URL publique vide. Le Core reste joignable
par les Nodes pour valider les capacités directes : une panne Core refuse toute
nouvelle requête directe. Les flux déjà ouverts suivent la politique P1.

Dans les logs du proxy, exclure les queries de `/stream/v1/` : par exemple Nginx,
utiliser `$uri` sans `$args` ni `$request_uri` pour cette location. Ne jamais
partager `.env`, identités Node, cookies ou URLs porteuses complètes.

Avant tout usage réel, refaire sur les machines cibles : appairage HTTPS, scan,
film/épisode, direct/relais, arrêt/reconnexion, rotation et révocation hors ligne,
puis restauration d'une sauvegarde sur volumes séparés. Le banc Docker local
valide un transport TLS entre conteneurs, pas le pare-feu, DNS, proxy DSM ou le
stockage du NAS réel.
