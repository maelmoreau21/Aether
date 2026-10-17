# Aether — Plateforme multimédia auto-hébergeable

Plateforme multimédia **100 % auto-hébergeable et open source**, inspirée de l'ergonomie Netflix,
conçue pour coexister sans danger avec Jellyfin et fonctionner en lecture directe AV1 vidéo + Opus audio.

> **16 octobre 2026 — Distribution Docker Standalone P1 livrée & vérifiée :**
> Image multi-stage « tout-en-un » non-root (`10001:10001`) intégrant le frontend SvelteKit compilé,
> le Core Axum/Tokio, le Media Node local, SQLite persistant en mode WAL et `ffprobe`,
> sans processus Node.js permanent.
> Documentation complète : [docs/DEPLOYMENT_DOCKER.md](docs/DEPLOYMENT_DOCKER.md).

---

## 1. Déploiement Docker Standalone (Recommandé)

### Lancement rapide avec Docker Compose
Depuis la racine du dépôt :

```bash
docker compose up -d
```

Vérifiez l'état du conteneur :
```bash
docker compose ps
```
Le conteneur passe en état `(healthy)` en ~6 secondes via la sonde interne `/api/v1/health`.

### Amorçage du premier administrateur local (Bootstrap)
Aether n'inclut **aucun mot de passe par défaut**. L'initialisation du premier compte administrateur
exige un terminal pseudo-TTY interactif :

```bash
docker compose exec -it aether media-bootstrap
```
Saisissez l'identifiant et confirmez le mot de passe masqué.
L'amorçage est alors définitivement clos en base SQLite ; toute tentative ultérieure est rejetée.
Connectez-vous ensuite sur [http://127.0.0.1:3000/app/login/](http://127.0.0.1:3000/app/login/).

### Sauvegarde et restauration SQLite (WAL)
Aether fournit deux outils natifs pour sauvegarder et restaurer la base sans arrêter le service :

```bash
# Sauvegarde cohérente à chaud (SQLite WAL via VACUUM INTO)
docker compose exec aether media-backup /data/backups/backup.sqlite3

# Restauration dans un volume vierge (ou après arrêt du conteneur)
docker compose run --rm aether media-restore /data/backups/backup.sqlite3
```
Consultez la [documentation complète de sauvegarde et restauration](docs/DEPLOYMENT_DOCKER.md#7-sauvegarde-et-restauration-sqlite-wal-fiables) pour les procédures de reprise après sinistre sur Docker Compose et Synology DSM.

---

## 2. Spécificités Synology DS1522+

Le Synology DS1522+ embarque un processeur **AMD Ryzen Embedded R1600** (x86_64, 2C/4T) **sans iGPU** :
- **Ne pas ajouter de montage `/dev/dri`** : aucun processeur graphique Intel/AMD VCN n'est présent. Une déclaration `/dev/dri` provoque l'échec immédiat du conteneur sur DSM.
- **Direct Play AV1 + Opus** : Aether privilégie la lecture directe dans `.mkv` et `.mp4`. Aucun transcodage vidéo gourmand n'est requis.
- **Bibliothèques en lecture seule (`:ro`)** : Configurez votre dossier de partage existant dans `.env` (`MEDIA_LIBRARY_DIR=/volume1/video`). Le conteneur monte ce volume en `:ro` strict et n'écrit jamais dans vos médias.
- **Persistance applicative** : La base SQLite, les sessions et les index résident dans le volume dédié `aether_data` (`/data`).

---

## 3. Configuration & Variables d'environnement

| Variable | Défaut conteneur | Description |
|---|---|---|
| `MEDIA_PORT` | `3000` | Port d'écoute interne du serveur HTTP |
| `MEDIA_BIND_ADDR` | `0.0.0.0` | Adresse d'écoute interne (`127.0.0.1` par défaut hors conteneur) |
| `MEDIA_PUBLIC_URL` | `http://127.0.0.1:3000` | Origine publique validée pour `Host`, `Origin` et CSRF |
| `MEDIA_DB_PATH` | `/data/core.sqlite3` | Chemin du fichier SQLite (volume persistant) |
| `MEDIA_SQLITE_BUSY_TIMEOUT_SECS` | `15` | Délai d'attente max SQLite en cas de concurrence d'écriture (15s par défaut, 45s+ recommandé pour les très grandes bibliothèques 10k+) |
| `MEDIA_LIBRARY_ROOT` | `/media` | Racine de montage en lecture seule des bibliothèques |
| `MEDIA_WEB_DIST` | `/app/apps/web/build` | Répertoire des assets Web statiques SvelteKit |

---

## 4. Exposition réseau & Proxy Inverse HTTPS

- **Liaison locale sécurisée par défaut** : `compose.yaml` lie le port hôte sur `"127.0.0.1:3000:3000"` pour éviter toute exposition non authentifiée sur le LAN.
- **Accès LAN direct** : Ajustez le port sur `"0.0.0.0:3000:3000"` et définissez `MEDIA_PUBLIC_URL=http://<IP_NAS>:3000`.
- **Proxy Inverse HTTPS (Nginx, Synology DSM, Caddy)** :
  1. Conservez la liaison locale `127.0.0.1:3000:3000`.
  2. Définissez `MEDIA_PUBLIC_URL=https://aether.mondomaine.fr`.
  3. **Obligatoire** : Configurez le proxy inverse pour relayer l'en-tête `Host` du client (`proxy_set_header Host $host;`).
  4. La sonde interne `/api/v1/health` est découplée de `MEDIA_PUBLIC_URL` et continue de répondre 200 OK aux vérifications Docker en boucle locale.

---

## 5. Métriques réelles de performance (Mesurées en conteneur)

Mesures réelles enregistrées sur le banc de vérification de bout en bout (AC-11) :

| Métrique | Valeur mesurée |
|---|---|
| **Taille contenu image (`aether:latest`)** | **213 Mo** (753,84 Mo couches décompressées) |
| **Délai de démarrage vers `healthy`** | **6,34 secondes** |
| **Mémoire RSS au repos (idle)** | **2,88 Mo** |
| **Mémoire RSS pic pendant scan ffprobe** | **12,56 Mo** |

---

## 6. Développement local & Banc P0 historique (Hors conteneur)

Pour développer directement sur l'hôte sans Docker :

Prérequis testés : Windows x64, Rust/Cargo 1.99.1, FFmpeg/ffprobe avec libaom-av1/libopus, Node.js 22, PowerShell 7, Chrome.

Depuis la racine du dépôt, PowerShell :

```powershell
./scripts/generate-fixtures.ps1
./scripts/prepare-language-remux.ps1
python scripts/generate-representative-fixtures.py
cargo fmt --check
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo test --locked --all-features
New-Item -ItemType Directory -Path data -ErrorAction SilentlyContinue
cargo run --locked -p media-poc --bin media-bootstrap
cargo run --locked -p media-poc
```

Ouvrir [le banc local](http://127.0.0.1:3000/app/), choisir une source, charger, puis lancer la lecture. Arrêt : Ctrl+C.
Les tests Rust utilisent le dossier standard `tests/fixtures/generated`.

### Tests automatisés navigateur (Playwright)

Dans un second terminal PowerShell :

```powershell
npm install --no-save --package-lock=false playwright@1.62.1
node scripts/browser-smoke.cjs
```

Variables de contrôle de tests navigateur :
- `MEDIA_TEST_SCENARIO` : `native` ou `remux`
- `MEDIA_BROWSER_CHANNEL` : `chrome`, `msedge`, `firefox`
- `MEDIA_REPORT_DIR` : destination des rapports (ex. `artifacts/browser-chrome`)

---

## 7. Documentation de référence

- Architecture et frontières : [ARCHITECTURE.md](ARCHITECTURE.md)
- Guide de déploiement Docker : [docs/DEPLOYMENT_DOCKER.md](docs/DEPLOYMENT_DOCKER.md)
- Modèle de menaces et sécurité : [docs/SECURITY.md](docs/SECURITY.md)
- Format média et pistes : [docs/MEDIA_FORMATS.md](docs/MEDIA_FORMATS.md)
- Preuves de compatibilité et streaming : [docs/STREAMING_COMPATIBILITY.md](docs/STREAMING_COMPATIBILITY.md)
- Plan de tests : [docs/TEST_PLAN.md](docs/TEST_PLAN.md)
- État et décisions : [MEMORY.md](MEMORY.md)
