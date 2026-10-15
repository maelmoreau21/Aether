# MEMORY.md — Mémoire & Décisions d'Architecture

## Contexte Matériel Cible
- Serveur hôte principal de référence : **Synology DS1522+** (CPU AMD Ryzen R1600 sans iGPU).
- Conséquence : Le transcodage vidéo est strictement banni. Le serveur ne doit faire que de l'I/O réseau/disque et de la génération de manifestes en mémoire (< 30 Mo RAM au repos).
- Coexistence : Tourne en parallèle d'une instance Jellyfin existante (montage des volumes médias en `:ro`).

## Format Cible des Médias (Pistes Découplées)
Un média peut être composé de :
1. **Mode Classique (Compatibilité) :** Un fichier `.mkv` ou `.mp4` unique contenant AV1 + Opus.
2. **Mode Optimisé (Cible) :** Dossier contenant des fichiers séparés non fragmentés :
   - 1 ou plusieurs fichiers vidéo **AV1** (ex: `video_main.mkv`, `video_fr.mkv` si textes traduits à l'écran).
   - 1 fichier audio **Opus** par langue (ex: `audio_fr.opus`, `audio_ja.opus`).
   - 1 fichier sous-titre par langue/format (`.vtt`, `.srt`, `.ass`).

## Décisions Architecturales Validées
- [x] **Nom provisoire :** Aether (isolé dans `brand.rs` / `brand.ts` pour renommage instantané).
- [x] **Frontend :** Svelte 5 statique embarqué dans le binaire Rust (pas de Node.js en prod).
- [x] **BDD :** SQLite embarqué par défaut, bascule transparente vers PostgreSQL 18 via `MEDIA_DB_URL`.
- [x] **Cache :** `moka` (RAM) par défaut, bascule vers Valkey/Redis via `MEDIA_CACHE_URL`.
- [x] **Auth :** OIDC prioritaire.
- [x] **Plugins :** Sandbox WebAssembly (Extism) pour les scrapers de métadonnées et webhooks.

## Feuille de Route (Étapes)
- [ ] **Phase 1 (Core Streaming) :** Squelette Axum + Config hybride + Lecture d'index Matroska/MP4/Opus + Streaming HTTP Range + Manifeste DASH.
- [ ] **Phase 2 (Database & Scanner) :** Schéma SeaORM (SQLite/PG18) + Scanner de bibliothèques non-bloquant.
- [ ] **Phase 3 (Web UI & Player) :** Interface Svelte 5 + Lecteur Vidstack/Shaka synchronisant vidéo AV1 + audio Opus séparés.
- [ ] **Phase 4 (Auth & Plugins) :** Intégration OIDC + Runtime Wasm Extism.