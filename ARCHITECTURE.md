# ARCHITECTURE.md — Structure du Monorepo

```text
/
├── AGENTS.md                  # Instructions strictes pour l'IA
├── MEMORY.md                  # État d'avancement et décisions
├── Cargo.toml                 # Workspace Rust
├── docker-compose.yml         # Déploiement (Standalone ou HA avec PG18 + Valkey)
├── crates/
│   ├── server/                # Binaire principal (Axum, routes HTTP, embed Frontend)
│   │   └── src/
│   │       ├── brand.rs       # Seul endroit contenant le nom "Aether"
│   │       ├── config.rs      # Variables MEDIA_*
│   │       └── main.rs
│   ├── stream-engine/         # Analyseur AV1/Opus (matroska/mp4), générateur DASH (.mpd) & Byte-Range
│   ├── db/                    # Entités SeaORM et migrations (SQLite & PostgreSQL 18)
│   ├── auth/                  # Client OIDC & middlewares de session (Moka / Valkey)
│   └── plugin-host/           # Hôte WebAssembly (Extism) pour les extensions
└── web/                       # Frontend Svelte 5 + Vite (compilé en statique)
    └── src/
        └── lib/
            └── config/
                └── brand.ts   # Constante du nom de l'application côté UI