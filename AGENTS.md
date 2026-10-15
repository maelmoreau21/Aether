# AGENTS.md — Règles de développement IA

## Identité du Projet & Règle de Nommage (White-Label)
- Nom de code actuel : **Aether**
- **RÈGLE CRITIQUE :** Le nom du projet est susceptible de changer. Ne JAMAIS écrire "Aether" en dur dans la logique métier, les routes API ou les composants UI.
- Toujours utiliser :
  - Côté Rust : la constante `crate::config::APP_NAME` (définie dans un seul fichier `src/brand.rs`).
  - Côté Frontend (Svelte) : la constante `APP_NAME` importée depuis `$lib/config/brand.ts`.
  - Variables d'environnement : préfixées génériquement par `MEDIA_` (ex: `MEDIA_DB_URL`, `MEDIA_PORT`) et non par `AETHER_`.

## Objectif Principal
Serveur média ultra-léger en Rust conçu pour le **Zero-Transcode** sur navigateur web :
- Lecture native de fichiers séparés (**AV1** pour la vidéo, **Opus** pour l'audio par langue, **WebVTT/ASS** pour les sous-titres) sans pré-fragmentation sur le disque.
- Génération dynamique de manifestes **MPEG-DASH (`.mpd`)** et streaming via `HTTP 206 Partial Content` (Byte-Range) avec remuxing virtuel en RAM si nécessaire.
- **Zéro processus `ffmpeg` en lecture.**

## Stack Technique Imposée
- **Backend :** Rust (Edition 2024), `Axum` (Web/HTTP), `Tokio` (Async), `Tower`.
- **Base de données (Hybride) :** `SeaORM` + `SQLx` compilé avec les features `sqlx-sqlite` (par défaut, embarqué) et `sqlx-postgres` (compatible PostgreSQL 18 pour la Haute Disponibilité).
- **Cache & Sessions (Hybride) :** Trait abstrait `CacheStore` avec implémentation mémoire locale (`moka` par défaut) et implémentation distribuée (`fred` / Valkey / Redis pour la HA).
- **Authentification :** SSO via **OpenID Connect (OIDC)** (`openidconnect` crate) + auth locale optionnelle de secours.
- **Système d'extensions :** Plugins compilés en **WebAssembly (Wasm)** exécutés via `Extism`.
- **Frontend :** **Svelte 5** (Runes) + `SvelteKit` (`adapter-static`) + `Vidstack` + `Shaka Player` (moteur DASH/MSE). Incorporé dans le binaire Rust via `rust-embed`.

## Standards de Code & Performance
1. **Empreinte mémoire minimale :** Éviter les clones inutiles (`Arc`, slices `&[u8]`, streaming I/O asynchrone via `tokio::fs` et `ReaderStream`). Ne jamais charger un fichier média entier en RAM.
2. **Stateless par design :** Aucun état de session ne doit dépendre d'un nœud local lorsque le mode HA (PostgreSQL 18 + Valkey) est activé.
3. **Zéro `unwrap()` / `expect()` en production :** Utiliser `thiserror` pour les erreurs typées et retourner des réponses HTTP propres via `IntoResponse` d'Axum.
4. **Accès disque Media :** Toujours traiter les dossiers de médias comme étant en **lecture seule (`ro`)** par défaut.