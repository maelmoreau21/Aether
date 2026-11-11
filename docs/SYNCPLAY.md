# Watch Party (SyncPlay) — Documentation Technique et Architecture

## Corrections vérifiables du 2 novembre 2026

- La création HTTP de séance échouait en `500`, même pour un film : sa requête
  SQL référençait `inventory_episodes.title`, une colonne inexistante. Le titre
  d'un épisode est désormais lu dans `inventory_works`, comme le prévoit le schéma.
- Le protocole JSON utilise `camelCase` pour les types **et les champs** :
  `clientEpochMs`, `serverEpochMs`, `mediaId`, `queueId`, etc. Les variantes Rust
  seules ne suffisaient pas à convertir les champs des enums Serde.
- Les instantanés transmettent la position de base avec son horodatage, afin que
  le client interpole une seule fois. Une reconnexion réapplique l'état collectif.
- Les commandes reçues s'appliquent directement au moteur du lecteur, y compris
  pour un participant sans contrôle. Elles ne repassent pas par les commandes
  utilisateur, évitant les refus de recalage et les boucles de `seek`.
- Le chargement du lecteur respecte l'attente/la pause de la séance. Le départ
  différé utilise `targetServerEpochMs` et peut être annulé par une pause ultérieure.
- Le lecteur signale le buffering ; une pause causée par ce buffering reprend
  après récupération des participants connectés. Une pause manuelle reste prioritaire.
- La correction de vitesse passe par le moteur pour conserver les pistes audio
  externes synchronisées. Les sauts automatiques personnels et l'épisode suivant
  personnel sont désactivés pendant une séance collective.

Validation ciblée : `cargo test -p aether-core --lib parties::tests`,
`cargo test -p aether-core --test syncplay_websocket`,
`npm --prefix apps/web test -- --run src/lib/syncplay`.
Le test WebSocket utilise un vrai serveur TCP local, deux comptes authentifiés,
les routes HTTP de création/adhésion et deux clients WebSocket. Il ne prouve pas
la lecture vidéo/audio dans deux navigateurs ni le comportement d'un NAS distant.
Résultats locaux observés : 10 tests Rust du module réussis, 1 test TCP/WebSocket
réussi, 7 tests Web réussis et `svelte-check` sans erreur ni avertissement.

> **Module :** Watch Party & Synchronisation collective (`parties.rs`, `syncplay/*`)  
> **Composants concernés :** Core (Rust/Axum/WebSockets), Media Node (flux HTTP Range directs), Frontend (SvelteKit runes, Svelte 5).

---

## 1. Principes et Architecture

### 1.1 Séparation stricte Core / Media Node
- **Le Core coordonne exclusivement les événements de synchronisation** (Play, Pause, Seek, Ready, Buffering, Chat, Réactions, Sondages, File d'attente collaborative) via WebSockets (`/api/v1/parties/{id}/ws`).
- **Aucun flux vidéo/audio ne transite par le Core** : chaque participant stream son média directement depuis son Media Node assigné via requêtes HTTP standard (Range requests).
- **Indépendance locale de lecture** : chaque participant conserve ses propres réglages personnels (piste audio, sous-titres, volume, style de sous-titres, qualité) sans impacter les autres membres.

### 1.2 Horloge et Synchronisation (NTP + Drift Compensation)
- **Horloge de référence serveur** : le serveur maintient la position temporelle (`position_s`) et l'horodatage serveur de dernière mise à jour (`position_updated_ms`).
- **Synchronisation d'horloge NTP** :
  - Le client envoie périodiquement `{"type":"ping","clientEpochMs":...}`.
  - Le serveur répond avec `{"type":"pong","clientEpochMs":...,"serverEpochMs":...}`.
  - Le client calcule le délai d'aller-retour (RTT) : $\text{rtt} = t_3 - t_0$ et le décalage d'horloge (offset) : $\text{offset} = t_1 - t_0 - \frac{\text{rtt}}{2}$.
  - La position cible interpolée est calculée selon l'état :
    - En pause / en attente : $\text{targetPosition} = \text{lastPosition}$
    - En lecture : $\text{targetPosition} = \text{position\_s} + \frac{(\text{now\_ms} + \text{offset} - \text{updated\_ms})}{1000.0}$
- **Compensation de dérive douce (Smooth Drift Correction)** :
  - Décalage $< 0.25\,\text{s}$ : aucune correction (`playbackRate = 1.0`).
  - Décalage modéré ($0.25\,\text{s} \le |\text{drift}| \le 1.5\,\text{s}$) : micro-ajustement de la vitesse de lecture sans coupure sonore (`0.95x` si en avance, `1.05x` si en retard).
  - Décalage important ($> 1.5\,\text{s}$) : recalage précis via `seek()` instantané.

---

## 2. Contrôles et Permissions

### 2.1 Rôles et Modes de Contrôle
- **Rôles dans la séance** :
  - `host` : Organisateur de la séance.
  - `participant` : Participant invité ou connecté.
- **Modes de contrôle (`control_mode`)** :
  1. `host_only` (Recommandé) : Seul l'organisateur peut lancer la lecture, mettre en pause ou naviguer dans la vidéo.
  2. `shared` : Tous les membres présents peuvent contrôler la lecture collective.
  3. `request` : Contrôle délégué dynamiquement par l'hôte aux utilisateurs autorisés.

### 2.2 Sécurité et Contrôle d'Accès
- **Vérification d'accès média obligatoire** : tant pour créer une séance que pour générer une invitation ou rejoindre une séance, le Core valide :
  1. La politique parentale (`parental_allowed`) de l'utilisateur sur l'œuvre ciblée.
  2. Les permissions de bibliothèque (`effective_library_permissions` ou rôle `administrator`).
  3. L'état activé de la bibliothèque et le statut non révoqué du nœud média hébergeant l'œuvre.
- **Jetons d'invitation éphémères** : jetons aléatoires de 64 caractères hexadécimaux, révocables et expirant par défaut sous 7 jours.
- **Rôles autorisés** : conformité stricte avec `AGENTS.md` — uniquement `administrator` et `user`. Aucun rôle `guest` n'est créé.

---

## 3. Fonctionnalités Collaboratives

### 3.1 Salon d'attente et synchronisation collective
- **Lobby d'attente** : les participants rejoignent le salon, chargent le média et signalent leur état de préparation (`SetReady { ready: true }`).
- **Détection de buffering** : détection automatique des états de chargement (`SetBuffering { buffering: true }`). Si activé (`wait_for_buffering = true`), le lecteur se met en pause automatique pour attendre les participants en difficulté.
- **Compte à rebours collectif** : compte à rebours visuel de 3 secondes avant le lancement de la lecture pour garantir un départ simultané.

### 3.2 Chat, Réactions, Sondages et File d'attente
- **Chat textuel en direct** : messages d'utilisateurs et messages système (connexions, pauses, changements de position).
- **Réactions rapides flottantes** : emojis animés apparaissant sans interrompre la vidéo. Option de masquage / silence overlay disponible.
- **Sondages en temps réel** : création de votes (ex: choix du prochain média), calcul instantané des pourcentages par option et clôture par l'hôte.
- **File d'attente collaborative (Queue)** : ajout d'épisodes ou films à regarder ensuite, avec enchaînement automatique possible à la fin du média.

---

## 4. API Endpoints

### 4.1 REST Endpoints
| Méthode | Route | Description |
| :--- | :--- | :--- |
| `POST` | `/api/v1/parties` | Créer une séance privée avec configuration et permissions média. |
| `GET` | `/api/v1/parties` | Lister les séances actives (membre ou invité). |
| `GET` | `/api/v1/parties/{id}` | Consulter l'état complet d'une séance. |
| `POST` | `/api/v1/parties/{id}/join` | Rejoindre une séance. |
| `POST` | `/api/v1/parties/{id}/leave` | Quitter une séance. |
| `POST` | `/api/v1/parties/{id}/end` | Clôturer définitivement une séance (hôte/admin). |
| `POST` | `/api/v1/parties/{id}/invites` | Générer une invitation révocable pour un utilisateur. |
| `GET` | `/api/v1/parties/{id}/invites` | Lister les invitations d'une séance. |
| `DELETE` | `/api/v1/parties/{id}/invites/{invite_id}` | Révoquer une invitation. |
| `GET` | `/api/v1/parties/invites` | Lister les invitations reçues par l'utilisateur connecté. |
| `POST` | `/api/v1/parties/invites/{token}/accept` | Accepter une invitation par son jeton. |
| `GET` | `/api/v1/parties/eligible-users` | Obtenir la liste des utilisateurs ayant accès au média. |

### 4.2 Protocole WebSocket (`/api/v1/parties/{id}/ws`)
- **Messages Client -> Serveur** :
  `Ping`, `SetReady`, `SetBuffering`, `Play`, `Pause`, `Seek`, `ChatMessage`, `Reaction`, `CreatePoll`, `VotePoll`, `ClosePoll`, `AddToQueue`, `RemoveFromQueue`, `KickMember`, `TransferHost`, `UpdateSettings`, `ChangeMedia`.
- **Messages Serveur -> Client** :
  `InitState`, `Pong`, `MemberJoined`, `MemberLeft`, `MemberUpdated`, `StateChanged`, `Countdown`, `ChatMessage`, `Reaction`, `PollUpdated`, `QueueUpdated`, `Kicked`, `MediaChanged`, `Error`.

---

## 5. Maintenance et Nettoyage
- Tâche d'arrière-plan `clean_expired_parties(&IdentityStore)` exécutée périodiquement :
  - Clôture les séances ouvertes créées depuis plus de 24 heures sans activité.
  - Supprime les invitations expirées depuis plus de 7 jours.
