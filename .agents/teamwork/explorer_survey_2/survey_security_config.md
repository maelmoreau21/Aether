# Rapport d'investigation : Sécurité, Configuration, SQLite et Bootstrap Docker

> Date : 16 octobre 2026  
> Auteur : Security and Config Explorer  
> Rôle : Investigation en lecture seule du socle Aether pour la distribution Docker autonome  
> Statut : Vérifié sur code source réel, migrations SQLx et bancs de tests

---

## 1. Configuration et variables d'environnement (`MEDIA_*`)

### 1.1 Variables d'environnement reconnues et analysées

Le Core d'Aether et son binaire de bootstrap lisent la configuration au démarrage via `std::env`. Aucune bibliothèque tierce lourde de configuration (comme Viper ou Config-rs) n'est utilisée ; les variables sont lues directement avec des valeurs de repli sécurisées et strictes.

| Variable | Fichier & Lignes | Type / Format | Valeur par défaut | Rôle et impact |
|---|---|---|---|---|
| `MEDIA_PORT` | `crates/core/src/main.rs:13-15` | Entier non signé `u16` | `"3000"` | Port TCP d'écoute du serveur HTTP Core. |
| `MEDIA_LIBRARY_ROOT` | `crates/core/src/main.rs:9-10`<br>`crates/core/src/bootstrap.rs:22-26` | Chemin absolu (chaîne) | `None` (repli vers `MEDIA_FIXTURE_DIR`) | Racine autorisée pour les bibliothèques multimédias. Si définie, Core bascule en mode production autonome (`standalone-local`, `Inventory::local`). |
| `MEDIA_FIXTURE_DIR` | `crates/core/src/main.rs:11-12`<br>`crates/core/src/bootstrap.rs:24-25` | Chemin vers dossier fixtures | `"tests/fixtures/generated"` | Racine de repli utilisée uniquement pour les tests de banc historiques (mode `localhost-fixtures`). |
| `MEDIA_DB_PATH` | `crates/core/src/identity.rs:177-180` | Chemin de fichier (chaîne/OsString) | `"data/core.sqlite3"` | Chemin vers la base de données SQLite unique. |
| `MEDIA_PUBLIC_URL` | `crates/core/src/main.rs:28-29`<br>`crates/core/src/auth.rs:70-82` | URL absolue (`http` ou `https`) | `format!("http://127.0.0.1:{port}")` | Origine publique validée pour le filtrage `Host`, `Origin`, la politique CSRF et l'attribut `Secure` des cookies. |

#### Variables réservées aux tests d'intégration et scripts d'automatisation
- `MEDIA_TEST_DB` (`crates/core/tests/identity.rs:134, 161`) : Injection du chemin DB pour le test de concurrence multi-processus.
- `MEDIA_TEST_ORIGIN` (`crates/core/tests/browser_scanner.rs:76`, `browser_posters.rs:95`, `browser_nfo.rs:85`, `browser_auth.rs:41`) : Origine passée aux runners Playwright/Puppeteer.
- `MEDIA_TEST_RESTART` (`crates/core/tests/browser_scanner.rs:77`) : Drapeau pour tester la persistance après arrêt/relance du Core.
- `MEDIA_TEST_PHASE` (`crates/core/tests/browser_posters.rs:96`, `browser_nfo.rs:86`) : Étape de validation des affiches et métadonnées NFO.
- `MEDIA_REPORT_DIR`, `MEDIA_BROWSER_CHANNEL` (`scripts/*.cjs`) : Dossiers de sortie de preuves de navigation.

---

### 1.2 Configuration et confinement des chemins (Données, Cache, Bibliothèques, Frontend)

#### A. Racine des bibliothèques multimédias (`MEDIA_LIBRARY_ROOT`)
- **Vérification d'accès** : Implémentée dans `crates/core/src/node.rs:124-154` (`validate_library_root`).
- **Contraintes absolues** :
  - `root.is_absolute()` et `allowed_root.is_absolute()` sont requis (`node.rs:125-126`). **Un chemin relatif échoue immédiatement avec `io::ErrorKind::InvalidInput`**.
  - Tout composant parent (`..`) est interdit (`node.rs:127-130`).
  - Le chemin doit exister et être un répertoire canonique (`tokio::fs::metadata(&canonical).await?.is_dir()`, `node.rs:145-151`).
  - Toute bibliothèque créée via l'API (`/api/v1/admin/libraries`, `libraries.rs:192`) doit avoir son chemin situé sous cette racine autorisée (`canonical.starts_with(&allowed)`).

#### B. Emplacement de la base de données (`MEDIA_DB_PATH`)
- **Vérification et isolation** : Implémentée dans `crates/core/src/identity.rs:177-203` (`configured_path`).
- **Comportement vérifié** :
  - La valeur par défaut est `"data/core.sqlite3"`.
  - **Le répertoire parent doit déjà exister** sur le système de fichiers (`identity.rs:185` : `let parent = parent.canonicalize()?;`). Si le dossier `data/` n'existe pas, Core et le bootstrap échouent immédiatement avec `io::ErrorKind::NotFound`.
  - **Isolation stricte avec les médias** (`identity.rs:195-201`) :
    ```rust
    if let Ok(root) = media_root.canonicalize() && resolved.starts_with(root) {
        return Err(std::io::Error::other("database must be outside the media directory"));
    }
    ```
    La base de données et son dossier ne doivent **JAMAIS** résider à l'intérieur de la racine des bibliothèques multimédias. Cette séparation garantit que le volume média peut rester monté en lecture seule (`:ro`) sans bloquer SQLite.

#### C. Chemins de cache
- **Absence de dossier de cache disque dédié** :
  L'inspection détaillée de `crates/core/src/inventory.rs:19-65` démontre que le cache d'inspection technique (`CachedFile`, `probe_json`) et le cache des affiches (`LocalImage`) sont stockés **exclusivement dans les tables SQLite** (`inventory_assets` et `inventory_images`).
  Le backend ne configure ni n'écrit dans aucun dossier de cache temporaire sur disque.

#### D. Distribution du frontend Web statique
- **Chemin statique hardcodé** :
  Dans `crates/core/src/lib.rs:152-156` :
  ```rust
  .nest_service(
      "/app",
      tower_http::services::ServeDir::new("apps/web/build")
          .append_index_html_on_directories(true),
  )
  ```
  Le service HTTP recherche les fichiers du frontend SvelteKit compilé dans le sous-dossier relatif `"apps/web/build"` par rapport au répertoire de travail courant (`WORKDIR`) du processus Core.
  *Implication Docker* : Dans l'image Docker, soit le `WORKDIR` est placé à la racine de l'application avec les assets placés dans `apps/web/build`, soit le code Rust doit supporter une variable optionnelle (ex. `MEDIA_WEB_DIST`) ou une copie conforme des fichiers compilés dans ce chemin relatif.

---

## 2. Liaison réseau, sécurité Host/Origin/CORS/CSRF et contraintes Docker/Proxy

### 2.1 Mécanisme de liaison réseau actuel

Dans `crates/core/src/main.rs:25-27` :
```rust
let listener =
    tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;
let port = listener.local_addr()?.port();
```
- Le serveur écoute **exclusivement sur `127.0.0.1` (`Ipv4Addr::LOCALHOST`)**.
- Il n'existe actuellement aucune variable d'environnement pour modifier l'adresse IP d'écoute (`MEDIA_HOST` ou `MEDIA_BIND_ADDR` n'existent pas encore dans le code).

#### Conséquence critique en conteneur Docker :
1. Dans un réseau Docker standard de type bridge, le démon Docker mappe les ports de l'hôte vers l'interface virtuelle `eth0` du conteneur (ex. `172.17.0.2`).
2. Si le binaire écoute uniquement sur `127.0.0.1` à l'intérieur du conteneur, les connexions transférées depuis l'hôte par Docker échouent avec `Connection refused` (`ECONNREFUSED`).
3. **Adaptation nécessaire** :
   - Pour Docker, le binaire doit pouvoir écouter sur `0.0.0.0` à l'intérieur du conteneur (soit par défaut en conteneur, soit via une variable comme `MEDIA_BIND_ADDR=0.0.0.0`), pendant que la configuration Docker Compose mappe ce port sur le loopback de l'hôte (`127.0.0.1:3000:3000` sur la machine hôte ou le Synology NAS).
   - L'alternative sans modification de bind est l'usage du mode réseau hôte (`network_mode: host`), mais celui-ci n'isole pas les ports, n'est pas portable sur Docker Desktop Windows/macOS et peut entrer en conflit avec d'autres services sur Synology DSM.

---

### 2.2 Filtrage de sécurité : Host, Origin, Sec-Fetch-Site et CORS

La couche de sécurité réseau est implémentée dans `crates/core/src/lib.rs:169-193` (`local_security`) et `crates/core/src/auth.rs:136-145` (`check_origin`).

```rust
// crates/core/src/lib.rs:170-188
let host = origin
    .parse::<axum::http::Uri>()
    .ok()
    .and_then(|uri| uri.authority().map(|value| value.as_str().to_owned()));
router
    .layer(middleware::from_fn(move |req: Request, next: Next| {
        let host = host.clone();
        let origin = origin.clone();
        async move {
            // Preserve the loopback-only origin boundary for the local Core.
            if req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) != host.as_deref()
                || req.headers().get("sec-fetch-site").is_some_and(|h| h == "cross-site")
                || req.headers().get(header::ORIGIN).is_some_and(|h| h.as_bytes() != origin.as_bytes())
            {
                return StatusCode::FORBIDDEN.into_response();
            }
            next.run(req).await
        }
    }))
```

#### A. Contrôle strict du header `Host`
- L'autorité attendue (`host`) est extraite de `MEDIA_PUBLIC_URL` (défaut : `127.0.0.1:{port}`).
- Tout en-tête `Host` reçu qui ne correspond pas exactement à cette autorité est rejeté avec un code **`403 FORBIDDEN`**.
- *Exemple* : Si `MEDIA_PUBLIC_URL` vaut `http://127.0.0.1:3000`, une requête avec `Host: localhost:3000` est rejetée avec `403`.

#### B. Contrôle du header `Origin` et `Sec-Fetch-Site`
- Si un en-tête `Origin` est présent (cas des requêtes POST/PUT/DELETE et requêtes fetch du navigateur), sa valeur doit être rigoureusement identique à `origin` (`MEDIA_PUBLIC_URL`).
- Si l'en-tête `sec-fetch-site` vaut `"cross-site"`, la requête est systématiquement rejetée (`403 FORBIDDEN`).

#### C. Absence totale de CORS
- Aucun en-tête `Access-Control-Allow-Origin` n'est configuré.
- Tout appel cross-origin depuis un autre domaine est bloqué au niveau du navigateur et du middleware.

#### D. Protection CSRF (Cross-Site Request Forgery)
- Implémentée dans `crates/core/src/auth.rs:115-135` (`require_csrf`).
- Tout appel avec mutation d'état (login, déconnexion, création/modification de bibliothèque, lancement de scan) exige l'en-tête `x-csrf-token`.
- Le jeton CSRF est dérivé de manière déterministe du secret de session via SHA-256 (`csrf(raw) = hash("csrf:{raw}")`, `auth.rs:290-292`).
- La comparaison s'effectue en temps constant (`constant_equal`, `auth.rs:293-295`) sur les empreintes de 64 caractères hexadécimaux pour prévenir les attaques temporelles.

---

### 2.3 Règles d'exposition : Reverse Proxies et restrictions HTTP / HTTPS

Dans `crates/core/src/auth.rs:70-82` (`AuthState::new`) :
```rust
let uri: axum::http::Uri = origin.parse().map_err(|_| AuthConfigError::Origin)?;
let scheme = uri.scheme_str().ok_or(AuthConfigError::Origin)?;
let host = uri.host().ok_or(AuthConfigError::Origin)?;
if uri.authority().is_none_or(|a| a.as_str().contains('@'))
    || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
    || !(scheme == "https"
        || (scheme == "http" && matches!(host, "127.0.0.1" | "localhost" | "[::1]")))
{
    return Err(AuthConfigError::Origin);
}
```

1. **Interdiction stricte du HTTP en clair hors loopback** :
   Si un opérateur tente de configurer `MEDIA_PUBLIC_URL=http://192.168.1.50:3000` pour un accès LAN direct non chiffré, Core **refuse de démarrer** avec l'erreur `AuthConfigError::Origin` ("public origin must be HTTPS or HTTP on loopback, without path or credentials").
2. **Accès distant ou LAN via Reverse Proxy HTTPS obligatoire** :
   Toute exposition en dehors du poste local requiert une terminaison TLS (HTTPS), par exemple via un reverse proxy (Nginx, Caddy, Traefik, Synology DSM Reverse Proxy) avec `MEDIA_PUBLIC_URL=https://media.mondomaine.fr`.
3. **Contraintes obligatoires pour la configuration du Reverse Proxy** :
   - **Préservation de `Host`** : Le proxy doit transmettre le `Host` public (`proxy_set_header Host $host;`). Si le proxy remplace l'en-tête par `127.0.0.1:3000`, la requête sera rejetée en 403 par le middleware Core.
   - **Préservation de `Origin`** : Les requêtes avec `Origin` doivent être transmises intactes.
   - **Cookies sécurisés** : Lorsque `MEDIA_PUBLIC_URL` commence par `https://`, `AuthState::cookie` active automatiquement l'attribut `; Secure` (`auth.rs:149`).
   - **Streaming Range** : Le proxy ne doit pas mettre en mémoire tampon les flux volumineux (`proxy_buffering off;`).

---

## 3. Base de données SQLite et exécution des migrations

### 3.1 Exécution des migrations au démarrage

- **Mécanisme** : Implémenté dans `crates/core/src/identity.rs:61-99` (`IdentityStore::open`).
- **Migrations SQLx embarquées** :
  ```rust
  // crates/core/src/identity.rs:92-96
  let mut conn = pool.acquire().await?;
  let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
  sqlx::migrate!("./migrations").run(&mut *tx).await?;
  tx.commit().await?;
  ```
  La macro `sqlx::migrate!("./migrations")` compile l'ensemble des fichiers `.sql` de `crates/core/migrations/` directement dans le binaire Rust. Aucun fichier SQL externe n'est nécessaire au moment de l'exécution dans le conteneur.
- **Sérialisation stricte** :
  Le verrou de migration de SQLx étant inopérant sur SQLite (`no-op`), Aether sérialise explicitement la vérification et l'application des migrations avec une transaction SQLite exclusive (`BEGIN IMMEDIATE`), évitant toute corruption lors de démarrages concurrents.

### 3.2 Inventaire des migrations existantes

1. `0001_identity.sql` : Tables `users` (rôles `administrator` et `user`), `local_credentials` (login, hash Argon2id PHC), `bootstrap_state` (marqueur singleton scellant le bootstrap).
2. `0002_sessions.sql` : Tables `sessions` (token_hash, user_id, last_seen_at, expires_at), `login_throttle` (limiteur de débit 10 tentatives/min persistant).
3. `0003_libraries.sql` : Tables `media_nodes` (`local`), `libraries` (kind `films` ou `series`), `library_permissions` (grants par utilisateur), `library_initialization` (marqueur singleton).
4. `0004_inventory.sql` : Modèle de données `inventory_works`, `inventory_editions`, `inventory_versions`, `inventory_sources`, `inventory_assets`, `inventory_tracks`, et table `scan_jobs`.
5. `0005_film_editorial.sql` : Ajout colonne `editorial_json` sur `inventory_works` et table `inventory_editorial` pour les métadonnées NFO.
6. `0006_local_posters.sql` : Table `inventory_images` pour les affiches locales (JPEG, PNG, WebP) associées aux œuvres.

### 3.3 Mode WAL, résilience et persistance Docker

- **Configuration SQLite** (`crates/core/src/identity.rs:62-67`) :
  - `create_if_missing(true)`
  - `foreign_keys(true)`
  - `journal_mode(SqliteJournalMode::Wal)` (Write-Ahead Logging)
  - `busy_timeout(Duration::from_secs(15))`
  - Pool de connexions borné à 4 connexions max (`max_connections(4)`, `identity.rs:74`).
  - Gestion de retry sur SQLite `BUSY`/`LOCKED` au premier démarrage (`identity.rs:79-88`).
- **Fichiers compagnons** : En mode WAL, SQLite génère les fichiers `<nom>.sqlite3-wal` et `<nom>.sqlite3-shm` dans le même dossier que la base.
- **Persistance en conteneur Docker** :
  Monter un volume Docker persistant sur le dossier contenant la base (ex. volume `aether_data:/data`) garantit la conservation complète des comptes administrateurs, des sessions actives, de l'inventaire des médias et des droits d'accès après arrêt, redémarrage ou mise à jour de l'image du conteneur.
  Cette persistance est vérifiée par les tests de régression existants (`crates/core/tests/core_restart.rs:56-110`).

---

## 4. Bootstrap de l'administrateur local initial

### 4.1 Caractéristiques du binaire de bootstrap

- **Binaire dédié** : `media-bootstrap` (défini dans `crates/core/Cargo.toml:32-34`, implémenté dans `crates/core/src/bootstrap.rs`).
- **Absence totale d'API HTTP** : Il n'existe **aucun endpoint HTTP de bootstrap**, d'enregistrement initial ou de création d'administrateur (`docs/LOCAL_ADMIN.md:68`, `crates/core/src/identity.rs:1`).
- **Exigences de sécurité strictes** (`crates/core/src/bootstrap.rs:17-21`) :
  ```rust
  if std::env::args_os().len() != 1 || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
      return Err(
          "bootstrap requires a local interactive terminal and accepts no arguments".into(),
      );
  }
  ```
  1. **Aucun argument CLI accepté** (`len() != 1`) : évite toute transmission de secret ou mot de passe sur la ligne de commande (visible dans `ps` ou l'historique shell).
  2. **Terminal interactif TTY obligatoire** (`is_terminal()`) : bloque l'exécution par script aveugle, redirection stdin ou pipe non contrôlé.
  3. **Saisie masquée sans écho** : utilise `rpassword::prompt_password` avec confirmation.
  4. **Effacement mémoire** : le mot de passe est encapsulé dans `Zeroizing<String>` (`bootstrap.rs:34-35`) pour écraser les octets en RAM dès la sortie de portée.

### 4.2 Hachage et verrouillage du bootstrap

- **Algorithme** : Argon2id v19 (`identity.rs:134-139`) avec 64 MiB de mémoire (`m=65536`), 3 passes (`t=3`), 1 voie (`p=1`) et sel cryptographique aléatoire de 32 octets.
- **Règles de validation des identifiants** (`identity.rs:119-131`) :
  - Login : 3 à 64 caractères, uniquement ASCII minuscules, chiffres, points, tirets bas et traits d'union (`[a-z0-9._-]`).
  - Mot de passe : 15 à 128 caractères, max 512 octets, sans caractères de contrôle.
- **Verrouillage durable (Singleton)** (`identity.rs:145-152, 167-169`) :
  Dans une transaction SQLite `BEGIN IMMEDIATE`, le code vérifie :
  `SELECT EXISTS(SELECT 1 FROM bootstrap_state) OR EXISTS(SELECT 1 FROM users)`
  Si le résultat est non nul, le bootstrap est rejeté avec `IdentityError::AlreadyInitialized`.
  L'insertion dans la table `bootstrap_state(singleton=1)` scelle définitivement le bootstrap. Même si tous les utilisateurs venaient à être supprimés manuellement, le bootstrap initial reste fermé.

### 4.3 Exécution sécurisée en environnement Docker (Sans identifiants par défaut)

Pour respecter la règle stricte interdisant tout mot de passe par défaut dans l'image ou le `docker-compose.yml`, le bootstrap sous Docker doit être exécuté par l'administrateur via un pseudo-TTY Docker :

```bash
# Exemple via Docker Compose sur le conteneur en cours d'exécution
docker compose exec -it aether media-bootstrap

# Ou via exécution d'un conteneur éphémère partageant le même volume de données
docker compose run --rm -it aether media-bootstrap
```

- L'argument `-it` alloue un pseudo-TTY interactif, ce qui valide `io::stdin().is_terminal()` et `io::stdout().is_terminal()`.
- L'administrateur saisit son login et son mot de passe de manière masquée.
- Aucun identifiant n'est écrit dans le fichier Compose, ni dans les variables d'environnement, ni dans les couches Docker.

---

## 5. Scanner multimédia, gestion du montage en lecture seule (`:ro`) et reprise après panne

### 5.1 Comportement sur montage en lecture seule (`:ro`)

- **Absence totale d'écriture sur le disque média** :
  L'inspection détaillée de `crates/core/src/node/scanner.rs`, `crates/core/src/node/images.rs`, `crates/core/src/node/nfo.rs` et `crates/core/src/inventory.rs` prouve formellement que **le scanner et le serveur ne réalisent aucune écriture sur le système de fichiers média**.
  Les seules opérations effectuées sont :
  - `tokio::fs::symlink_metadata` (lecture des métadonnées).
  - `tokio::fs::read_dir` (parcours des dossiers).
  - `std::fs::File::open` (lecture des fichiers `.nfo` et `.jpg`/`.png`/`.webp`).
  - Lancement de `ffprobe` en lecture seule sur le fichier média (`crates/core/src/media.rs:41-67`).
- **Protection contre liens et points de jonction** (`scanner.rs:53-66, 90-120`) :
  Le scanner ignore systématiquement les liens symboliques et les points de réanalyse/jonctions NTFS (`linked()`), et revalide le confinement strict à la racine (`confined_file()`).
- **Validation du montage `:ro`** :
  Monter la bibliothèque média avec le drapeau `:ro` (ex. `-v /volume1/video:/media:ro` dans Compose) est **pleinement supporté et fonctionnel** sans générer la moindre erreur d'I/O.

### 5.2 Emplacement des affiches et caches

- **Pas d'extraction d'images depuis la vidéo** :
  Le système n'extrait pas de miniature depuis les conteneurs vidéo (aucune commande `ffmpeg -ss ... -vframes 1`).
- **Découverte des affiches compagnons locales** (`crates/core/src/node/images.rs:138-146`) :
  Le scanner recherche les fichiers d'affiches existants dans le dossier du film (`poster.jpg`, `folder.jpg`, etc.).
- **Validation et stockage** :
  L'image est validée en mémoire via la crate `image` (JPEG/PNG/WebP, dimensions <= 4096, taille <= 10 MiB, budget décodeur <= 32 MiB).
  Les métadonnées (`relative_path`, `size`, `modified_ns`, `mime`, `priority`) sont enregistrées dans la table SQLite `inventory_images`.
- **Distribution des affiches (`inventory::image`)** (`crates/core/src/inventory.rs:505-558`) :
  Les affiches sont servies directement depuis leur emplacement d'origine dans le dossier média via `tower_http::services::ServeFile::new(path)`. Aucun dossier d'affiches extraites n'est créé sur disque.

### 5.3 Reprise après interruption ou plantage du scan

Le gestionnaire de tâches de scan (`crates/core/src/scans.rs`) est spécifiquement conçu pour gérer les arrêts inopinés et les redémarrages :

1. **Nettoyage automatique au redémarrage** (`crates/core/src/scans.rs:79-80`) :
   ```rust
   // A stopped process cannot resume a probe. Keep its last inventory and report interruption.
   sqlx::query("UPDATE scan_jobs SET status='error',errors_json='[\"Scan interrompu par un redémarrage\"]' WHERE status IN ('queued','running')")
       .execute(&auth.store.pool).await?;
   ```
   Lors de l'initialisation du routeur de scan au redémarrage du Core, toute tâche restée à l'état `queued` ou `running` lors du crash est automatiquement passée à l'état `error` avec le message `"Scan interrompu par un redémarrage"`.
2. **Arrêt propre (Graceful Shutdown)** (`scans.rs:43-56`, `main.rs:38-41`) :
   Lors de la réception du signal d'arrêt (Ctrl-C ou SIGTERM), `shutdown_jobs.shutdown().await` active le drapeau atomique `cancel`, ferme les créneaux d'exécution et attend la terminaison ou l'abandon des tâches en cours.
3. **Préservation de l'inventaire existant** (`crates/core/src/inventory.rs:86-90`) :
   ```rust
   // Missing files become unavailable only after complete discovery; failed traversal preserves inventory.
   if report.complete {
       sqlx::query("UPDATE inventory_assets SET available=0 WHERE library_id=?")
           .bind(library_id)
           .execute(&mut *tx)
           .await?;
   }
   ```
   Si un scan est interrompu, `report.complete` vaut `false`. Par conséquent, les entrées existantes du catalogue ne sont **pas** invalidées ni marquées indisponibles. L'inventaire précédent est préservé intact.

---

## 6. Point de terminaison Healthcheck

### 6.1 État du point de terminaison existant

Dans `crates/core/src/lib.rs:163-167` :
```rust
.route(
    "/api/v1/health",
    get(move || async move { Json(serde_json::json!({"status":"ok", "mode":mode})) }),
)
```
- Le point d'entrée existant est **`/api/v1/health`**.
- La route `/health` **n'existe pas**.
- **Nature de la vérification** : Il s'agit d'une réponse JSON statique (`status: "ok"`, `mode: "standalone-local"` ou `"localhost-fixtures"`).
- **Éléments non vérifiés** :
  - Aucune vérification de la disponibilité du pool SQLite (`SELECT 1`).
  - Aucune vérification de l'accessibilité du système de fichiers ou du volume de données.
  - Aucune vérification de l'état des sous-systèmes ou de `ffprobe`.

---

### 6.2 Piège critique : Conflit de sécurité avec le HEALTHCHECK Docker

Comme observé à la section 2.2, le routeur public (contenant `/api/v1/health`) est encapsulé dans le middleware `local_security` (`crates/core/src/lib.rs:111-118`).
Ce middleware exige une égalité stricte entre le header `Host` de la requête entrante et l'autorité de `MEDIA_PUBLIC_URL` :
```rust
if req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) != host.as_deref() ... {
    return StatusCode::FORBIDDEN.into_response();
}
```

#### Scénario d'échec constaté :
1. L'opérateur configure `MEDIA_PUBLIC_URL=https://aether.mondomaine.local` pour un usage avec reverse proxy.
2. La commande Docker `HEALTHCHECK` à l'intérieur du conteneur exécute :
   `curl -f http://127.0.0.1:3000/api/v1/health`
3. `curl` envoie par défaut l'en-tête `Host: 127.0.0.1:3000`.
4. Le middleware `local_security` compare `"127.0.0.1:3000"` avec `"aether.mondomaine.local"` : il constate un écart et **renvoie immédiatement `403 FORBIDDEN`**.
5. `curl -f` échoue sur le code 403 et sort avec un code d'erreur non nul.
6. Docker déclare le conteneur en état **`unhealthy`** alors que le serveur fonctionne parfaitement !

#### Solutions architecturales recommandées :
1. **Au niveau du code Core** :
   Exempter la route `/api/v1/health` du filtre strict de `Host` (ou autoriser explicitement `127.0.0.1` et `localhost` sur la route de healthcheck uniquement).
   Optionnellement, ajouter une vérification active de la base de données (`sqlx::query("SELECT 1")`).
2. **Au niveau de la commande Healthcheck Docker (si le code n'est pas modifié)** :
   La commande de sonde doit impérativement injecter l'en-tête `Host` correspondant à la valeur configurée dans `MEDIA_PUBLIC_URL`.

---

## 7. Synthèse des exigences pour la phase d'implémentation Docker

| Composant | Situation actuelle | Action requise pour l'étape d'implémentation |
|---|---|---|
| **Liaison TCP** | Écoute uniquement sur `127.0.0.1` (`main.rs:26`) | Supporter l'écoute sur `0.0.0.0` en conteneur (ex. via `MEDIA_BIND_ADDR` ou conditionnement d'environnement), tout en liant à `127.0.0.1:3000:3000` côté hôte dans `docker-compose.yml`. |
| **Frontend Web** | Fichiers attendus dans `"apps/web/build"` relatif au `WORKDIR` (`lib.rs:154`) | Aligner la structure des dossiers dans le conteneur (ex. `WORKDIR /aether` avec `apps/web/build`) ou rendre le chemin configurable. |
| **Base SQLite** | Fichier dans `MEDIA_DB_PATH` (`identity.rs:179`) ; dossier parent requis | Créer le dossier parent (ex. `/data`) dans le Dockerfile / script de démarrage avec les permissions appropriées pour l'utilisateur non-root. |
| **Bootstrap Admin** | Binaire CLI `media-bootstrap` interactif avec TTY obligatoire (`bootstrap.rs:17`) | Documenter la commande `docker compose exec -it aether media-bootstrap` pour l'initialisation sans mot de passe par défaut. |
| **Scanner & Médias** | 100 % lecture seule, caches en base SQLite | Monter les volumes médias avec le drapeau `:ro` (`/media:ro`) et réserver un volume dédié persistant pour les données (`/data`). |
| **Healthcheck** | Endpoint `/api/v1/health` protégé par vérification `Host` stricte | Adapter la commande `HEALTHCHECK` dans Dockerfile / Compose ou assouplir le contrôle `Host` local pour `/api/v1/health`. |
