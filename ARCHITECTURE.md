# ARCHITECTURE.md — Architecture cible Aether

> **Architecture cible, pas un état de livraison.** Un banc P0 expérimental et le changement FR/EN par remux sont testés localement ; les composants produit ci-dessous restent des intentions. Voir `MEMORY.md` et les preuves de compatibilité pour l'état réel.

## 1. Objectif

Une interface Web moderne et **un catalogue global** peuvent piloter un ou plusieurs serveurs de stockage multimédia. La V1 privilégie un service **autonome** avec un Media Node local ; le multi-Node suit. Les réplicas Core/Web, la haute disponibilité et les très grandes charges sont **anticipés mais exclus du MVP**.

## 2. Vue logique

```text
           Navigateur / SvelteKit 5 (statique, réplicable plus tard)
                                  |
                        HTTPS /api/v1 + session BFF
                                  v
                 +----------------------------------+
                 | CORE Rust (Axum)                 |
                 | comptes, rôles, OIDC, catalogue, |
                 | historique, droits, sessions,    |
                 | registry des Media Nodes         |
                 +-----------------+----------------+
                                   | contrôle authentifié
                       +-----------+-----------+
                       v                       v
             +--------------------+  +--------------------+
             | Media Node LOCAL   |  | Media Node DISTANT |
             | scan, index, I/O   |  | scan, index, I/O   |
             +---------+----------+  +---------+----------+
                       |                       |
              médias MKV / MP4       autres MKV / MP4 puis pistes externes

           PLAN DE DONNÉES : client -> Node direct si possible,
                 sinon -> relais explicite -> Node.
           Le Core NE DOIT PAS relayer toutes les vidéos par défaut.
```

### Modes

| Mode | Déploiement | Contraintes |
|---|---|---|
| `standalone` — premier | Core + Node local + Web, **un seul service possible** | SQLite ; accès local sans infra additionnelle ; volumes médias en `ro` |
| `distributed` — phase P2 | 1 Core + N Nodes, même API/UI | Appairage, routes de lecture, santé, auth et inventaire |
| `cluster` — futur | N Core derrière répartiteur, N Web, N Nodes | PostgreSQL, sessions partagées, jobs distribués, failover **à concevoir plus tard** |

Les réplicas Web partagent la même API et le même catalogue. Le mode autonome **réutilise la même logique métier et les mêmes garde-fous**, sans dupliquer le code en deux implémentations.

## 3. Responsabilités strictes

**Web (`apps/web`)** : SvelteKit, navigation, accessibilité, lecteur et préférences ; reçoit des **descripteurs de lecture** sans chemins NAS ; aucune autorisation de sécurité finale dans le navigateur. Statique et séparable.

**Core (`crates/core`)** : API versionnée, compte local/OIDC, deux rôles `administrator|user`, bibliothèques/permissions, catalogue global, métadonnées, statut des Nodes, progression, découverte des pistes, émission de sessions de lecture autorisées et stratégie de source.

**Node (`crates/node`)** : bibliothèques montées `ro`, scan local, analyse conteneurs/codecs/pistes, indexation locale, streaming des fichiers ou segments autorisés, refus des accès par chemin arbitraire, health/heartbeat, reprise en cas de Core indisponible. Ne connaît pas les identifiants/mots de passe d'utilisateurs.

**Media (`crates/media`)** : lecture des informations Matroska / ISO BMFF, index et timebases, descriptions de pistes ; plus tard association d'assets séparés, remux/packaging bornés et éventuellement interprétation de flux élémentaires.

**Shared (`crates/shared`)** : types versionnés, IDs, erreurs, feature/protocol version. **Aucune dépendance circulaire**.

## 4. Modèle de données conceptuel

```text
Work (film/série/épisode : identité éditoriale)
 └── Edition (montage / textes localisés dans l'image)
      └── MediaVersion (encodage/résolution/HDR/bitrate)
           └── MediaSource (copie logique disponible sur un Node)
                ├── Asset 1..N (fichier conteneur, piste externe ou raw)
                ├── Track 1..N (vidéo/audio/sous-titres)
                └── SourceTrack (piste -> asset / stream index / timestamps)

User (compte) --role--> {administrator, user}
  └── Profile (facultatif, plus tard)
       └── PlaybackProgress / LanguagePreferences

Node -> Library -> MediaSource(s)
```

Définitions :
- **Work** : identité d'une œuvre, pas simple titre de fichier.
- **Edition** : montage, vidéo alternative avec textes incrustés, coupe director's cut ; ne pas confondre avec résolution.
- **MediaVersion** : combinaison éditoriale encodée (1080p / 4K / profils codecs).
- **MediaSource** : localisation/copie sur un Node (le même encodage peut exister sur plusieurs Nodes).
- **Asset** : un fichier réel ou un flux déclaré indépendamment ; peut porter 1 ou plusieurs pistes selon son format.
- **Track** : essence vidéo, audio ou sous-titre, **jamais supposée unique** ; distinguer piste « alternative » de flux « simultané ».
- **PlaybackProgress** : lié à l'identité éditoriale et au profil/compte, pas seulement au chemin de fichier.

Identifiants stables globaux, pas d'unicité fondée sur le nom de fichier ni de fusion automatique au titre identique. Les changements de dossiers, copies et Nodes ne doivent pas effacer inutilement l'historique. Les relations/schémas définitifs sont précisés en `docs/MEDIA_FORMATS.md`.

## 5. Deux générations de formats et contrats de lecture

### 5.1 Priorité P0 — conteneurs actuels

```text
/Films/Exemple (2026)/Exemple (2026).mkv   # AV1 + Opus, 1..N pistes
/Films/Exemple (2026)/Autre film.mp4       # AV1 + Opus, 1..N pistes
```

- Détecter les vrais codecs et les pistes, sans se fier à l'extension.
- Prendre en compte index/cues, seek, durées et sélection de pistes.
- **Conteneur analysé ≠ lecture directe web validée.** Le moteur peut nécessiter remux/segmentation conforme (sans réencoder).
- La priorité n'est **pas** la prise en charge universelle de tous les anciens codecs.

### 5.2 Futur — assets externes indépendants

```text
Exemple (2026)/
  media.json                    # manifeste facultatif à valider
  video_main.mkv               # piste vidéo isolée dans un fichier
  video_alt.mp4                # autre piste vidéo indépendante
  audio_fr.opus                # Ogg/Opus AUDIO SEUL (pas raw)
  audio_en.opus                # autre langue
  subs_fr.forced.vtt           # sous-titres
  subs_en.ass
```

Ce format est **démultiplexé** : chaque piste est autonome au niveau des fichiers, mais les fichiers `.mkv`, `.mp4`, `.opus` peuvent toujours utiliser de petits conteneurs mono-piste.

### 5.3 Ultérieurement — essences réellement sans conteneur

**Besoin explicitement prévu** : données vidéo AV1 élémentaires et paquets Opus sans Matroska/MP4/Ogg. Pour que ces bytes soient utilisables, spécifier et valider :
- découpage/limites des unités AV1 (OBU / temporal units) et paquets Opus ;
- **index** : offset, longueur, PTS/DTS ou horodatage équivalent, timebase, durée, points d'accès aléatoire (keyframes) ;
- paramètres codec et initialisation (configuration AV1, OpusHead ou informations équivalentes, `pre-skip`, mapping canaux, gain) ;
- synchronisation et opérations de seek, compensation de décalage et segmentation/packaging web à la demande ;
- manifeste d'association **versionné** et protection des chemins.

**Ne pas inventer** immédiatement un nouveau format `.raw` propriétaire ni supposer qu'un navigateur sait lire des octets élémentaires AV1/Opus. `docs/MEDIA_FORMATS.md` sépare les hypothèses des décisions validées. POC dédiée **après** la lecture MKV/MP4 réussie.

## 6. Pipeline et plan de lecture

```text
GET fiche -> catalogue déjà indexé
POST /api/v1/playback/sessions
  -> auth utilisateur + contrôle ressource
  -> profil capacités du client (non fiable pour les droits)
  -> inspection des MediaSources disponibles
  -> stratégie prouvée : direct | remux/segments | préparation | rejet
  -> PlaybackPlan (URLs courtes, pistes disponibles, motifs/limites)
  -> Node direct authentifié / relais restreint si nécessaire
```

Mode **direct** uniquement si le browser accepte l'assemblage complet conteneur + codecs ; sinon packaging/remux testé. `Range` / `206` seul ne garantit pas MSE, DASH ou la prise en charge native des MKV. Un MP4 non fragmenté n'est pas un segment fMP4 valide par sa seule extension. Aucun transcodage vidéo obligé. La mise en cache et les queues restent bornées.

Le client doit gérer une capacité connue (« supportée/testée »), plutôt que supposer la compatibilité à partir du MIME ; mesurer les combinaisons réelles Chrome/Edge/Firefox/Safari. Voir `docs/STREAMING_COMPATIBILITY.md`.

## 7. Identité / sécurité

- **Comptes : deux rôles uniquement** : `administrator`, `user`. Les profils éventuels sont des sous-identités de lecture, sans privilèges serveur.
- Premier démarrage : bootstrap administrateur local interactif/sécurisé, **jamais de compte prédéfini** ; possibilité de connexion locale d'urgence si IdP indisponible.
- OIDC Authentik ou fournisseur standard : Authorization Code + PKCE, BFF Core, cookie `HttpOnly`, `Secure` en HTTPS, SameSite/CSRF adaptés, valide issuer/audience/nonce/state/signature.
- Assignation admin explicite par admin existant ; mapping de groupes OIDC **configuré**, pas implicitement par email ou chaîne non validée.
- Node appairé sur action admin, identité par nœud, renouvellement/révocation, routes internes non publiques, TLS sur réseau non sûr.
- Streaming : capacités de lecture temporaires limitées à un asset, tenant compte des droits, pas de transmission de chemins réels par l'API ; Node autorise avant de servir.
- Défenses contre SSRF, IDOR, traversal/symlinks, injections, XSS, CSRF, CORS abusif, replay, attaques Range et épuisement CPU/réseau.
- Média `ro` dans tous les modes par défaut. Détails : `docs/SECURITY.md`.

## 8. Scalabilité future sans complexité prématurée

Pendant P0/P1/P2 : **un Core unique**, SQLite, cache local, jobs locaux avec état durable si besoin. Ne pas piloter tous les scans dans la même requête HTTP ; inventaire de Node stocké/actualisé et paginé ; index DB.

Pour N Core plus tard : PostgreSQL partagé, sessions et révocations accessibles à tous les Core, cache/queue distribués, verrous/jobs idempotents, schémas/API rétrocompatibles, migrations coordonnées, health/readiness et tests de panne. Les Nodes s'adressent à une **URL logique de Core**. Les fichiers statiques SvelteKit sont répliquables indépendamment. L'état persistant ne doit pas vivre seulement en RAM ; **SQLite partagé entre plusieurs Core actifs est exclu**.

**Scalabilité ≠ capacité garantie.** Bande passante et I/O des Nodes, relais, disque, débit sortant et codecs clients dictent la limite de spectateurs. Benchmarks progressifs 1/10/100 lectures avant tout chiffre marketing.

## 9. Monorepo visé (créer les modules au fil des besoins)

```text
/
├── AGENTS.md
├── ARCHITECTURE.md
├── MEMORY.md
├── README.md
├── PROMPT_DEMARRAGE_IA.md
├── Cargo.toml                 # après initialisation effective
├── apps/web/                  # SvelteKit, frontend indépendant
├── crates/
│   ├── core/                 # Axum, API, identité, catalogues
│   ├── node/                 # scans, lecture et contrôle d'accès
│   ├── media/                # parsing, index, streaming
│   ├── auth/                 # RBAC, local et OIDC
│   ├── database/             # SQLx et migrations
│   └── shared/               # modèles/types/API/brand.rs
├── deploy/                   # à créer au besoin
├── tests/fixtures/           # échantillons synthétiques légaux
└── docs/
    ├── SPECIFICATION.md
    ├── MEDIA_FORMATS.md
    ├── STREAMING_COMPATIBILITY.md
    ├── SECURITY.md
    ├── API.md
    ├── TEST_PLAN.md
    ├── DEVELOPMENT_PLAN.md
    └── DEPLOYMENT.md
```

Ne pas créer tous les dossiers vides pour faire semblant d'avoir terminé l'architecture.

## 10. Choix à **ne pas** figer sans POC

- Video.js 10 vs Shaka Player vs autre moteur évalué de la même façon.
- Outil de démultiplexage, parseur MKV/MP4 et remux/packager ; FFmpeg CLI uniquement si besoin explicite, pas comme prérequis permanent.
- Mode d'association des fichiers externes (manifeste explicite recommandé pour le raw ; règles auto limitées et vérifiables).
- Pistes AV1 élémentaires OBU + Opus raw : grammaire/index et format de packaging résultant ; les schémas d'exemple sont **illustratifs**, pas un standard validé.
- REST/WebSocket/gRPC pour contrôle Node après tests réseau.
- PostgreSQL/Valkey/multi-Core/HA, systèmes de plugins : **futurs**.

## 11. Découpage d'implémentation

Voir `docs/DEVELOPMENT_PLAN.md`. **La première réalisation** est un banc d'essai reproductible de MKV et MP4 AV1+Opus, pas un catalogue complet.

### État P0C Shaka

Le banc Rust sert aussi un MPD fixe et un bundle Shaka5.2.13 préparés hors
lecture. Les flux AV1/Opus sont les copies identiques du témoin MSE conservé.
La comparaison reste un outil P0, sans nouvelle couche produit ni dépendance
FFmpeg de lecture. Voir docs/P0C_SHAKA_EXPERIMENT.md pour mesures et limites.

## État P1 autorisé (15 octobre 2026)

Première intégration locale concrète : apps/web SvelteKit statique, servi par
le backend existant sous /app/ ; pas de service Node permanent. API démo sources/
pistes indépendante du lecteur. Interface Player TypeScript et adaptateur natif
avec remplacement de source borné, pas d'abstraction multi-backends. Marque par
crates/shared/src/brand.rs, repli Web config/brand.ts puis configuration Core.
Préparation FR/EN hors lecture ; streaming réutilise le Node interne P0.
Pas encore de Core produit avec comptes/SQLite/bibliothèques. Les expériences
Shaka/MSE sont conservées séparément et ne sont pas chargées par l'application.
Voir docs/P1_WEB_INTEGRATION.md ; les comparaisons P0 sont suspendues.

### Direction visuelle P1 v1

Tokens CSS partagés et composants de présentation légers dans apps/web/src/lib/ui.
L'écran principal reste client de Player et de l'API démo existante ; pas de
couplage visuel au remux ou à Shaka/MSE. Illustration originale locale ; la
fixture de lecture reste explicitement une mire. Voir docs/UI_DIRECTION.md.
