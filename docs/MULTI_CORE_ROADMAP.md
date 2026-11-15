# Feuille de route multi-Core

> 7 novembre 2026. **Document de conception, pas une fonctionnalité livrée.** Aether reste
> en production sur **un seul Core actif**. Le mode SQLite autonome n'est pas modifié et
> restera exclusivement mono-Core. Cette feuille de route décrit ce qu'il faudrait construire,
> dans quel ordre et avec quelles preuves avant d'autoriser deux Core actifs sur PostgreSQL.
> L'audit qui l'a motivée est dans [`MULTI_NODE_RELIABILITY.md`](MULTI_NODE_RELIABILITY.md) ;
> l'état antérieur reste dans [`MULTI_CORE.md`](MULTI_CORE.md) et
> [`MULTI_CORE_READINESS.md`](MULTI_CORE_READINESS.md).

## 0. Ce qui est déjà acquis (et ne dépend pas du multi-Core)

Le lot du 7 novembre (un Core, plusieurs Nodes) a posé les briques dont le multi-Core a
besoin, sans activer plusieurs Core :

- boîte aux lettres de commandes par Node, avec **échéance** (`expiresAt`) et file bornée ;
- péremption d'un Node silencieux fondée sur des battements manqués ;
- équivalence prouvée des sources avant tout basculement ;
- jetons de lecture directe vérifiés en ligne à chaque requête (révocation immédiate des
  nouvelles requêtes).

Les tâches durables (`durable_tasks`) possèdent déjà un bail, un numéro de tentative servant de
jeton de fencing et des clés d'idempotence ; leurs écritures conditionnelles sont valides entre
Core. Cette base n'est pas à refaire.

## 1. Inventaire de l'état local au processus

| État | Où | Pourquoi il bloque deux Core | Section |
|---|---|---|---|
| `ACTIVE_CHANNELS` (boîtes aux lettres) | `nodes/channel.rs` | en partie levé (schémas v5 et v6) : la propriété du canal et son époque sont en base, le Node refuse les commandes d'un ancien propriétaire, les commandes de contrôle passent par la boîte d'envoi durable et peuvent être émises depuis n'importe quel Core ; seules les demandes de relais restent en mémoire, chez le propriétaire | §2 |
| `PENDING_STREAMS` | `nodes/channel.rs` | le relais exige que la requête navigateur et le flux montant du Node arrivent sur le **même** Core | §2.4 |
| `NodeRuntimeRegistry` (télémétrie, latence, flux actifs) | `nodes/registry.rs` | chaque Core ne voit que les battements qu'il reçoit : la sélection de source et le failover divergent | §4.1 |
| `ReplayWindow` (nonces) | `shared/protocol.rs` | un message signé rejoué sur l'autre Core passe la fenêtre anti-rejeu | §4.3 |
| `EFFECT_GATE`, `WAKE` | `task_engine.rs` | exclusion mutuelle des effets limitée au processus | §3 |
| Nettoyage au démarrage de `scans::router` | `scans.rs`, `core_instances.rs` | levé (schéma v4) : le nettoyage ne touche plus que le travail sans bail vivant ; voir §3.3 | §3.3 |
| `CATALOGUE_CACHE` | `inventory/catalogue/cache.rs` | cache périmé après une écriture sur l'autre Core | §4.2 |
| Cache de sessions / `touch` | `auth/state.rs`, `auth/touch.rs` | une révocation n'est vue qu'après expiration du cache de l'autre Core | §4.4 |
| `MUTATIONS`, paquets d'extension locaux, `AssetStore` | `extensions/`, `assets.rs` | installations non sérialisées, fichiers locaux | §4.5 |
| Tâches système et sauvegardes planifiées | `system_task_queue.rs`, `admin_backups.rs`, `task_scheduler.rs` | levé : file et bail en base sous verrous consultatifs PostgreSQL ; voir §3.2 (point 4) | §3.2 |

## 2. Propriété durable des canaux de Node

### 2.1 Principe

À tout instant, **un seul Core possède le canal de commande d'un Node**. La propriété est une
ligne en base, à bail, avec une **époque** strictement croissante. Elle remplace la présence
« j'ai un `mpsc` ouvert » par un fait que tout Core peut lire.

```
node_channel_owner(
  node_id      TEXT PRIMARY KEY REFERENCES media_nodes(id),
  core_id      TEXT NOT NULL,          -- UUID aléatoire du processus
  epoch        BIGINT NOT NULL,        -- +1 à chaque changement de propriétaire
  lease_until  BIGINT NOT NULL,        -- renouvelé à chaque poll (bail ~ 3 × 25 s)
  uplink_url   TEXT NOT NULL           -- URL interne où joindre ce Core
)
```

Acquisition ou renouvellement : un seul `INSERT … ON CONFLICT(node_id) DO UPDATE … WHERE
core_id = excluded.core_id OR lease_until < now` (comparer-et-échanger). L'époque n'augmente
que lors d'un **changement** de propriétaire. Le Core perdant constate zéro ligne modifiée et
répond au Node par un refus « autre propriétaire » (le Node se reconnecte à l'URL logique).

**Réalisé (étape 3, schéma v5, migration `0005_node_channel_owner`)** :

- table `node_channel_owner(node_id → media_nodes ON DELETE CASCADE, core_id, epoch, lease_until,
  uplink_url DEFAULT '')`. `core_id` est `IdentityStore::instance_id()` ;
- chaque poll de canal (`node_channel`, avant de servir la boîte aux lettres) exécute **une**
  instruction `INSERT … ON CONFLICT(node_id) DO UPDATE … WHERE`, valide sur SQLite et PostgreSQL,
  puis relit `(core_id, epoch)`. Prise ou renouvellement si : ce Core est déjà propriétaire, le bail
  est expiré (`lease_until < now`), le propriétaire n'est pas un Core vivant (aucune ligne
  `core_instances` battue depuis moins de 45 s : mort, ou retiré par un arrêt propre, donc reprise
  **immédiate** après un arrêt propre), ou la base est SQLite (mono-Core par construction : un
  redémarrage, même après un plantage de quelques secondes, n'attend jamais l'ancien bail).
  Bail : `CHANNEL_LEASE_SECONDS` = 3 × `POLL_WAIT` (25 s) = 75 s ;
- époque : inchangée tant que le propriétaire est le même ; à chaque changement de propriétaire,
  `max(époque + 1, now)`. Écart assumé par rapport au simple `+1` : l'époque reste strictement
  croissante, et une ligne perdue (Node supprimé puis réappairé, base restaurée depuis une
  sauvegarde) repart au-dessus de ce que le Node a déjà vu ; avec `+1` depuis une valeur restaurée
  plus petite, le Node refuserait toutes les commandes indéfiniment ;
- Core perdant : `409 Conflict` signé `channel_owned_elsewhere` ; s'il tenait une boîte aux lettres
  pour ce Node, elle est retirée de `ACTIVE_CHANNELS` et **fermée** (`NodeChannel::fence` : file
  vidée, émission refusée, poll en attente terminé). Une boîte dont l'époque change entre deux polls
  jette aussi les commandes mises en file sous l'ancienne époque ;
- `uplink_url` reste vide : il n'existe pas encore d'URL interne propre à chaque Core
  (`MEDIA_CORE_INTERNAL_URL` est l'URL logique) ; son remplissage dépend du choix de §2.4 ;
- la liste d'administration des Nodes expose `channelOwner` et `channelEpoch` (champs facultatifs),
  et `pendingRotation` (booléen omis quand faux) pendant une rotation de secret en deux temps (§2.3) ;
- au passage, un poll dont le chemin `/nodes/{id}/channel` ne nomme pas le Node signataire est
  refusé (`403 node_id_mismatch`) : un Node correctement signé pouvait jusque-là lire la boîte aux
  lettres d'un autre Node.

Reprise après **arrêt brutal** en PostgreSQL : un Core redémarré sur le **même hôte** (même
`HOSTNAME`, par exemple le même conteneur relancé par `restart: unless-stopped`) reconnaît son
prédécesseur (même nom d'hôte, démarré avant lui) et reprend aussitôt son canal et ses baux locaux
(`core_instances::alive_sql`, partagé avec `reclaim_orphans`). Deux Core sur un même hôte doivent
donc avoir des `HOSTNAME` distincts. Un **autre** Core attend que le battement du mort soit périmé
(45 s), au plus la fin du bail (75 s) ; pendant ce temps le Node reçoit des `409` et repolle toutes
les 500 ms. Un arrêt propre libère le canal immédiatement. Derrière un répartiteur sans affinité, un poll qui
tombe sur un Core non propriétaire est refusé puis rejoué : l'affinité par Node est souhaitable.

### 2.2 Commandes : de la mémoire à la boîte d'envoi durable

Aujourd'hui une commande vit dans la mémoire du Core propriétaire (livraison au plus une
fois, cf. `MULTI_NODE_RELIABILITY.md`). Pour qu'un Core puisse commander un Node possédé par un
autre :

```
node_commands(id, node_id, epoch, type, payload, expires_at, state, attempts, created_at)
  state ∈ queued | delivered | acked | expired | cancelled
```

- **Émission** (n'importe quel Core) : `INSERT` avec l'époque lue ; jamais d'envoi direct.
- **Livraison** (Core propriétaire seulement) : sélection `queued AND expires_at > now AND
  epoch = époque courante`, marquage `delivered` dans la même transaction. Réveil par
  `LISTEN/NOTIFY` PostgreSQL, avec un sondage de secours de 1 s.
- **Accusé** : le Node renvoie `commandId` ; sans accusé avant échéance → `expired`, jamais
  rejoué après changement d'époque pour les commandes non idempotentes (relais, annulation).
- **Idempotence côté Node** : un `commandId` déjà exécuté est ignoré (le Node conserve les N
  derniers identifiants).

L'échéance (`expiresAt`) du protocole actuel est exactement le champ dont cette table a besoin ;
le passage au durable ne casse pas le fil.

**Réalisé (étape 4, schéma v6, migration `0006_node_commands`)** :

- table `node_commands(id, node_id → media_nodes ON DELETE CASCADE, command_type, payload_json,
  state, epoch, attempts, idempotent, expires_at, created_at, seq, delivered_at, acked_at,
  origin_core)`, index `(node_id, state, seq)`. Écart par rapport au plan : la colonne `seq`
  (microsecondes, strictement croissante dans un processus) ordonne les commandes, car
  `created_at` (secondes) ne départage pas plusieurs émissions d'une même seconde. Entre deux Core
  l'ordre suit leurs horloges (prérequis NTP, §3.3) ; il n'est strict que pour un même émetteur ;
- **émission** (`nodes::outbox::enqueue`, n'importe quel Core) : une insertion, sans lire
  l'époque. Écart : l'époque est celle de la **livraison**, posée par le propriétaire ; une
  commande livrée sous une époque antérieure et non accusée est remise en file au premier poll du
  nouveau propriétaire. Émetteurs passés à la boîte durable : scan, annulation de scan, tâche,
  synchronisation d'inventaire, rotation de secret, révocation. Leurs contrats HTTP ne changent pas :
  le contrôle « Node connecté » (sinon repli HTTP direct ou `503`) lit la mémoire de ce Core pour
  les Nodes qu'il sert, et sinon la base (bail de canal valide d'un autre Core et `last_seen_at` de
  moins de 30 s), jamais le seul `ACTIVE_CHANNELS` local. File bornée à 256 commandes en attente
  par Node (contrôle avant insertion, approximatif sous concurrence) ;
- **livraison** (Core propriétaire, dans le poll, après `acquire()` à l'époque E) : la maintenance
  du Node (une instruction : périmées → `expired`, livrées sans accusé depuis 30 s ou sous une
  époque < E → `queued` si relivrables et encore valides, sinon `expired`), puis la plus ancienne
  commande `queued` non périmée passe à `delivered` (époque E, `delivered_at`, `attempts + 1`) par
  un `UPDATE` conditionnel qui exige aussi que ce Core possède encore le canal à l'époque E. Écart :
  pas de transaction englobante, chaque transition est une instruction atomique (comparer-et-
  échanger) ; deux Core ne livrent jamais la même commande. **Une commande par réponse** : le
  format de réponse n'en porte qu'une et un lot casserait les anciens Nodes ; le Node repolle
  aussitôt. Les demandes de relais de lecture (boîte en mémoire, échéance courte) passent avant
  l'arriéré durable, qui ne peut donc pas les faire expirer ;
- **réveil** : une émission de ce Core réveille le poll en attente dans le processus (aucune
  latence ajoutée en mono-Core SQLite, vérifié par le test de bout en bout) ; en PostgreSQL, un
  poll en attente sonde aussi la table chaque seconde (`SELECT id … LIMIT 1` indexé) pour les
  commandes écrites par un autre Core. Coût : une lecture par seconde et par Node connecté sur son
  propriétaire (100 Nodes : 100 lectures légères par seconde), à mesurer contre le seuil du §7.
  `LISTEN/NOTIFY` n'est pas fait : le pool `Any` n'expose pas d'écouteur, il faudrait une connexion
  PostgreSQL dédiée par Core ;
- **accusés** : le poll suivant du Node porte `commandAcks: true` et `ackedCommandIds` (corps signé,
  au plus 256 identifiants, omis quand vide). Le Core les applique pour ce Node seulement. Le Node
  accuse les commandes exécutées, ignorées comme doublon ou périmées, jamais celles refusées pour
  une époque inférieure ;
- **relivraison et idempotence par type** : tous les types durables sont relivrables (scan et tâche
  par leur `id:tentative`, gardés côté Node par `register_scan` et côté Core par le fencing des lots
  et résultats ; annulation, rotation vers la même clé et révocation sont rejouables ; une
  synchronisation d'inventaire annule à l'émission les synchronisations plus anciennes de la même
  bibliothèque ; une annulation annule à l'émission le démarrage non accusé du même scan ou de la
  même tâche, qui ne peut donc jamais être relivré après elle). Le Node retient les 512 derniers
  identifiants traités, en mémoire, et ne déduplique que les commandes de la boîte d'envoi (elles
  portent une époque et un identifiant unique) : un Core antérieur réutilise des identifiants
  déterministes (`sync-{bibliothèque}`, `cancel-{scan}`) pour de nouvelles commandes, qui doivent
  s'exécuter comme avant (persister
  coûterait une écriture synchrone par commande) : une relivraison après un redémarrage du Node est
  réexécutée. Un Node qui n'accuse pas (version antérieure) reçoit chaque commande au plus une fois ;
- **purge** : la passe périodique du répartiteur de tâches (toutes les 60 s, chaque Core) applique
  la même maintenance à tous les Nodes et supprime les lignes terminées de plus de 24 h. Le contenu
  d'une commande est effacé dès qu'elle est accusée, périmée ou annulée ;
- **relais** inchangé : boîte aux lettres en mémoire du propriétaire, avec `PENDING_STREAMS` (§2.4) ;
- **révocation** : mise en file puis livrée par le poll en attente (signé avec le secret encore
  valide), les autres commandes en attente du Node sont annulées.

Preuve actuelle : tests unitaires SQLite (ordre, époque posée à la livraison, refus d'un Core non
propriétaire ou d'une époque ancienne, accusés limités au Node, relivraison après 30 s ou changement
d'époque, au plus une fois sans accusé, péremption, borne, remplacement des synchronisations,
purge, joignabilité), test Node (`CommandLog` : doublons, liste d'accusés bornée), tests de
compatibilité serde, un test de bout en bout avec un vrai Node et son vrai `start_channel_worker`
(un Core, SQLite : commande exécutée moins de 5 s après l'émission, accusée, relivrée une seconde
fois sans être réexécutée) et un test PostgreSQL (`#[ignore]`,
`postgres_outbox_delivers_every_command_once_across_ownership_changes`) : deux stores sur un schéma
isolé, **dans un seul processus**. B, non propriétaire, émet ; A, propriétaire, livre. Puis 10 000
commandes émises alternativement par les deux Core pendant qu'un **Node simulé** (le vrai
`CommandLog`, la marque d'époque simulée) polle alternativement les deux Core sous une horloge
simulée, en appelant les fonctions du poll (`acquire`, `acknowledge`, `maintain_node`,
`claim_next`) ; injections : une réponse perdue toutes les 97 livraisons, accusés retenus 40 s
toutes les 499 exécutions, à chaque changement une commande livrée par l'ancien propriétaire
arrive après que le nouveau a parlé ; trois changements de propriétaire (battement du propriétaire
périmé, désinscription propre, battement périmé). Exécution du 15 novembre : 10 000 commandes
exécutées exactement une fois, 10 263 livraisons, 105 réponses perdues relivrées, 158 doublons
ignorés par le Node, 3 commandes d'ancienne époque refusées, environ 116 s. **Ce test ne prouve
pas** : deux vrais processus Core, le chemin HTTP du poll sur PostgreSQL (sonde d'une seconde
comprise), un vrai réseau, un arrêt brutal réel ni le redémarrage d'un Node (mémoire des doublons
perdue).

### 2.3 Fencing côté Node

Chaque commande porte l'`epoch` du propriétaire. Le Node mémorise la plus haute époque vue
(persistée) et **refuse toute commande d'époque inférieure** ; il refuse aussi les requêtes
signées d'un Core d'époque ancienne. Un ancien Core isolé du réseau (donc ignorant qu'il a perdu
son bail) ne peut plus agir sur ce Node dès que le nouveau propriétaire a parlé une fois.

**Réalisé (étape 3) pour le canal de commandes** :

- `NodeCommandEnvelope.epoch` : champ facultatif (`serde(default)`, omis quand absent), dans le corps
  de la réponse signée du poll, donc authentifié par l'empreinte de corps existante sans changer la
  chaîne signée. Le Core le pose sur **toute** commande livrée, `ping` compris (le `ping` sert de
  poignée de main qui élève la marque) ;
- Node : `ChannelEpochFence` (`crates/node/src/server/epoch.rs`), plus haute époque dans
  `node_channel_epoch.json` (fichier temporaire, `fsync`, renommage). Époque inférieure → commande
  non exécutée, journal `node_command_stale_epoch` (le protocole n'a pas d'accusé de refus) ; égale
  → acceptée ; supérieure → marque élevée et persistée **avant** l'exécution ; absente (ancien Core)
  → acceptée comme avant. Fichier illisible → marque 0 (journalisé), comme un Node sans fencing.
  Une marque plus d'une heure en avance sur l'horloge du Node (écrite sous une horloge fausse puis
  corrigée, base restaurée) est abandonnée au profit de l'époque reçue, journal
  `node_channel_epoch_mark_in_future` : sinon le Node refuserait toute commande légitime jusqu'à ce
  que l'horloge la rattrape. Une époque d'ancien propriétaire reste sous une marque légitime ;
  Un nouvel appairage (`/internal/v1/node/pair`) ou enrôlement remet la marque à zéro (autre
  installation, époques sans rapport) ;
- compatibilité : ancien Core → nouveau Node (aucune époque, tout est accepté) ; nouveau Core →
  ancien Node (champ ignoré, `409` traité comme une erreur ordinaire avec recul de 1 à 5 s).

- boîte aux lettres : la mise sous fencing (`NodeChannel::fence`) et l'envoi (`try_send`)
  testent l'état sous le même verrou : une commande poussée juste avant la perte du canal est
  refusée à l'émetteur, jamais acceptée puis perdue en silence.

**Réalisé — rotation de secret en deux temps** (même étape de schéma v6, colonnes
`media_nodes.pending_key_id`, `pending_secret`, `pending_since`) :

- par le canal, `rotate_node_secret` n'écrit plus la nouvelle clé comme clé courante : elle est
  enregistrée **en attente** (avant la mise en file, car le Node peut signer avec elle dès la
  livraison), puis la commande `RotateSecret` passe par la boîte d'envoi durable. La clé courante
  reste valide et les appels directs du Core vers le Node (jetons de lecture, sondes, tâches)
  continuent de l'utiliser ;
- vérification des requêtes signées par un Node (`transport::resolve_node_signing_key`, utilisée
  par `verify_signed_node_call` — poll, battement, lots et fin de scan, résultats de tâche,
  autorisation de lecture — et par le flux montant du relais) : la clé courante **ou** la clé en
  attente. La première requête valide signée avec la clé en attente la **promeut** par un `UPDATE`
  conditionnel (`WHERE id=? AND pending_key_id=?`) ; l'ancienne clé est refusée ensuite. Une
  promotion concurrente de la même clé (autre requête, autre Core) est acceptée. La réponse signée
  par le Core utilise la clé avec laquelle le Node a signé sa requête ;
- les clés sont relues en base à chaque requête de Node (une lecture par clé primaire, à côté de
  la mise à jour de `last_seen_at` déjà faite à chaque appel vérifié) : un Core dont le cache de
  registre date d'avant une promotion ou une révocation faite par un autre Core refuse quand même
  l'ancienne clé et accepte la nouvelle. La clé en attente n'est jamais mise en cache ni renvoyée ;
  la vue d'administration n'expose qu'un booléen facultatif `pendingRotation` ;
- une nouvelle rotation **remplace** une rotation en attente dont la commande n'a jamais été
  livrée (commande annulée). Dès que la commande de la clé en attente a été livrée **une fois**,
  toute nouvelle rotation est refusée (`409`), même après péremption de la commande : le Node a
  peut-être appliqué la clé avant de se couper. Elle reste en attente jusqu'à sa promotion par
  usage, un réappairage ou une révocation. L'enregistrement de la clé en attente et la livraison
  d'une rotation sont sérialisés par un verrou consultatif par Node (`node-rotation`), ce qui
  ferme l'écart d'écriture entre deux administrateurs ; les lignes de commande qui en gardent la
  trace ne sont pas purgées tant que leur clé est en attente. Le propriétaire du canal ne livre une rotation que si sa clé est encore la clé en attente
  (condition dans l'`UPDATE` de livraison) ; sinon il l'annule : une rotation remplacée, un
  réappairage ou une rotation directe intervenus entre-temps ne basculent jamais le Node sur une
  clé que le Core n'accepte plus ;
- commande perdue, retardée ou périmée : le Node garde l'ancienne clé, toujours acceptée ; la
  commande est relivrée tant qu'elle est valide (300 s), puis la rotation reste en attente
  (`pendingRotation`) ;
- la révocation et le réappairage effacent la clé en attente ; la révocation annule aussi la
  commande de rotation en file ;
- Node : une rotation pour la clé déjà en usage (relivraison après accusé perdu ou redémarrage)
  ne change rien et ne réécrit pas l'identité ; elle est accusée. Une rotation adressée à un autre
  Node, ou sans clé, est ignorée ;
- rotation directe (Node appairé avec `baseUrl` et injoignable par le canal) : inchangée,
  synchrone, confirmée par la preuve mutuelle du Node ; la nouvelle clé devient courante aussitôt
  et toute clé en attente est effacée.

Limites : entre l'application de la nouvelle clé par le Node et sa promotion (le poll suivant,
immédiat), un appel direct du Core signé avec l'ancienne clé, ou un jeton de lecture émis avec
elle, peut être refusé une fois par le Node ; une requête du Node encore en vol signée avec
l'ancienne clé peut être refusée après la promotion. Les autres Core suivent la promotion à la
première requête de Node qu'ils vérifient ; leurs appels directs et jetons de lecture utilisent
leur cache de registre jusqu'à son rafraîchissement (état local au processus, §4.1). Une
rotation livrée à un Node qui ne revient jamais bloque les rotations suivantes : réappairer ou
révoquer le Node.

Preuve : tests unitaires SQLite (`nodes::rotation_tests` : clé en attente acceptée, promotion au
premier usage puis refus de l'ancienne clé, réponse signée avec la clé de la requête, commande
perdue puis relivrée puis périmée sans déconnexion, nouvelle rotation qui remplace une rotation
non livrée, refus tant qu'une rotation livrée n'est pas réglée, rotation périmée ou remplacée
jamais livrée, révocation qui efface la clé en attente, promotions concurrentes, second Core au
cache périmé), test Node (`server::outbound::tests` : rotation idempotente pour la clé en usage),
migration v5 → v6, un test de bout en bout avec un vrai Node et son vrai `start_channel_worker`
(`a_rotation_over_the_channel_is_promoted_by_the_nodes_first_use_of_the_new_key` : rotation,
application par le Node, promotion, accusé, ancienne clé refusée, canal toujours actif), un test
de la rotation directe (`a_direct_rotation_is_confirmed_by_the_node_and_current_at_once`) et un
test PostgreSQL `#[ignore]` (`postgres_rotation_emitted_by_another_core_is_promoted_on_first_use` :
deux stores dans un processus, B émet, A livre et promeut, B refuse l'ancienne clé). Ces tests ne
couvrent pas deux vrais processus Core ni un redémarrage réel du Node pendant la rotation.

**Reste non fencé** (appels signés directs du Core vers le Node, `core-to-node`, hors canal ; ils
ne visent que les Nodes appairés avec une `baseUrl` et, sauf la révocation envoyée par les deux
voies, ne servent que quand le Node n'est joignable par le canal d'aucun Core — depuis l'étape 4,
un Node qui polle un autre Core reçoit ces commandes par la boîte d'envoi durable, donc fencées) :

| Appel | Route Node | Émetteur |
|---|---|---|
| sonde de battement | `POST /internal/v1/node/heartbeat` | `management::reconnect_node` |
| révocation | `POST /internal/v1/node/revoke` | `management::revoke_node_admin` (envoyée aussi par le canal, fencée) |
| rotation de secret | `POST /internal/v1/node/rotate-secret` | `transport::send_signed_rotation_request` |
| tâche | `POST /internal/v1/node/tasks` | `scans::dispatch_remote_task` |
| annulation de scan | `POST /internal/v1/node/scans/{id}/cancel` | `scans::cancel_remote_scan` |
| scan | `POST /internal/v1/node/scans` | `scans::dispatch_remote_scan` |
| synchronisation d'inventaire | `POST /internal/v1/node/inventory-sync` | `scans::sync_all_libraries_for_node` |

Raison : la chaîne signée ne peut pas changer, un en-tête hors signature ne serait pas authentifié,
et les corps de `NodeTaskRequest` et `RemoteScanStartRequest` refusent les champs inconnus (un ancien
Node rejetterait la requête). Les fencer demande soit un en-tête versionné couvert par une nouvelle
version de signature, soit de faire passer ces envois par le canal (boîte d'envoi durable, §2.2).
Le relais (`StreamRelay`) passe par le canal et est donc fencé ; les jetons de lecture directe ne
changent pas (vérifiés en ligne à chaque requête).

Preuve actuelle : tests unitaires SQLite (acquisition, renouvellement à époque constante, reprise
après expiration du bail, reprise immédiate d'un propriétaire mort ou retiré, boîte aux lettres du
Core perdant fermée), tests Node (marque persistée à travers un redémarrage, époque inférieure
refusée, absence d'époque acceptée, époque supérieure élevant la marque), tests de compatibilité
serde, un test PostgreSQL (`#[ignore]`, deux stores sur un schéma isolé, un seul processus : A
possède N, B refusé tant que A vit, B reprend avec une époque supérieure quand le battement de A est
périmé et le renouvellement de A échoue, reprise immédiate après désinscription, une seule victoire
sur deux acquisitions concurrentes) et un test de bout en bout avec un vrai Node et son vrai
`start_channel_worker` (`multi_node_reliability`, un seul Core : l'époque ancienne y est simulée en
abaissant la ligne) : une commande d'époque inférieure n'est pas exécutée. **La preuve de sortie de
l'étape 3 (§6 : arrêt brutal du propriétaire, Node repris par l'autre Core en moins de 3 baux, deux
vrais processus Core derrière un répartiteur) reste à écrire.**

### 2.4 Le relais, cas difficile

Le relais exige que le flux montant du Node et la requête du navigateur se rencontrent. Options,
par ordre de préférence :

1. **Lecture directe** (déjà préférée et jamais obligatoire) : aucun problème de propriété, le
   Core ne porte aucun octet. C'est la voie à privilégier pour le multi-Core.
2. **Affinité de répartiteur** : le répartiteur dirige `/api/v1/assets/{id}/content` vers le
   Core propriétaire du Node qui héberge l'actif (clé `node_id` fournie par une entête
   interne). Aucune nouvelle brique réseau côté Core.
3. **Relais Core-à-Core** : le Core qui reçoit le navigateur relaie vers `uplink_url` du
   propriétaire. À éviter : double saut, mémoire et bande passante doublées.

Décision à prendre **après mesure** : part du trafic qui passe réellement par le relais dans les
déploiements multi-machines (les compteurs `core_active_relays` existent déjà).

## 3. Fencing des effets durables

### 3.1 Règle

Tout effet durable doit vérifier, **dans la même transaction que l'effet**, que son
détenteur est toujours le propriétaire courant : identifiant de tâche, numéro de tentative,
Node et bail encore valide. C'est déjà vrai pour l'enregistrement des résultats de tâches ;
c'est à étendre aux effets listés ci-dessous.

### 3.2 Effets à fencer, par priorité

1. **Publication d'un scan** (`publish_staged_scan`) : vérifier tentative et bail dans la
   transaction de publication. Aujourd'hui `EFFECT_GATE` (processus) l'exclut des autres
   effets mais pas entre Core.
2. **Réception de lots de scan** (`stage_batch`) : refus d'un lot dont la tentative n'est plus la
   courante (existe : `stale_scan_attempt`) ; ajouter l'époque du canal.
3. **Effets d'extension** (métadonnées, état d'extension) : voir `MULTI_CORE_READINESS.md`
   § bloqueur 2.
4. **Sauvegardes et tâches système planifiées** : passer par `durable_tasks` (bail) au lieu d'un
   `Mutex`/`Semaphore` local.

   **Réalisé (étape 2)** :

   - mise en file (`system_task_queue::enqueue_*`) sous verrou transactionnel par tâche
     (`begin_write_scoped`, portée `system-task-enqueue`) : « exécution active ? clé de requête
     déjà vue ? sinon insertion » est atomique entre Core ; une violation d'unicité
     `(owner, idempotency_key)` renvoie la ligne existante. La file indique si une ligne a été
     insérée (`trigger_backup` et `task_scheduler::admit` n'ont plus de pré-contrôle non verrouillé) ;
   - réclamation (`claim`) sous un verrou transactionnel global (`system-task-claim`) : jamais deux
     baux de tâche système à la fois, même sur deux Core. C'est le seul point d'exclusion : le
     `Mutex` par exécution de `system_tasks.rs`, qui n'excluait rien, est supprimé ;
   - sauvegardes : une exclusion commune à la création manuelle (`POST /api/v1/admin/backups`,
     409 `backup_already_running` si occupée) et aux exécutions planifiées (différées de 30 s sans consommer de tentative, `deferred_busy`) :
     sémaphore par base ouverte (partagé, alors qu'il était recréé à chaque exécution planifiée)
     plus, sur PostgreSQL, un verrou consultatif de session sur une connexion dédiée, libéré
     explicitement ou par fermeture de la connexion. Les écritures `running` et finale de
     `backup_schedule` sont fencées par tentative et bail vivant. Le drapeau `running` de la liste
     vient de la base (exécution planifiée sous bail) ou de la sauvegarde en cours dans ce
     processus : une sauvegarde manuelle lancée sur un autre Core n'y apparaît pas ;
   - `task_scheduler::fire_due` : sur PostgreSQL, un seul Core exécute une passe à la fois (verrou
     de session `pg_try_advisory_lock`, passe sautée sinon). Les déclencheurs « démarrage » restent
     propres à chaque Core ; leurs doublons sont absorbés par le verrou de mise en file.

   Preuve actuelle : tests unitaires SQLite et deux tests PostgreSQL (`#[ignore]`) avec deux stores
   sur un même schéma, dans un seul processus (mises en file et réclamations concurrentes,
   sauvegarde occupée sur l'autre Core). La preuve de sortie de l'étape 2 (§6, deux processus,
   1 000 échéances) reste à écrire.

### 3.3 Démarrage d'un second Core

Le nettoyage de démarrage de `scans::router` doit cesser d'être global. Remplacer par :
« ne reprendre que les lignes dont le propriétaire est **ce** Core ou dont le bail est expiré ».
Tant que ce n'est pas fait, démarrer un second Core détruit le travail en vol du premier. C'est
le premier bloqueur à lever, et il est testable seul (deux Core, un scan en cours, démarrage du
second).

**Réalisé (schéma v4, migration `0004_core_instances`)** :

- chaque ouverture de base (`IdentityStore`) est une instance de Core identifiée par un UUID
  aléatoire ; elle s'inscrit dans `core_instances` au montage des routes et rafraîchit
  `heartbeat_at` toutes les 10 s (boucle dédiée du `Scheduler`). Un Core silencieux depuis
  45 s (`HEARTBEAT_STALE_SECONDS`) est réputé mort ; les lignes silencieuses depuis 24 h sont
  supprimées ;
- `durable_tasks.lease_owner` enregistre le Core qui a pris le bail (`NULL` : aucun, ou bail
  antérieur à la v4) et est effacé avec le bail. Un bail local (`node_id='local'`) ne se
  renouvelle que par son propriétaire (ou sans propriétaire) ; le répartiteur d'un Core ne
  renouvelle, ne termine ni n'annule le travail local d'un autre Core ;
- `core_instances::reclaim_orphans` remplace le nettoyage global, au démarrage puis toutes les
  60 s : expiration des baux locaux qui ne peuvent plus être honorés (SQLite, mono-Core : tous au
  démarrage ; PostgreSQL : ceux d'un autre Core mort ou inconnu, jamais ceux sans propriétaire),
  remise en file avec fencing par tentative, puis `scan_jobs` en erreur (« Scan interrompu par un
  redémarrage ») seulement sans tâche de scan sous bail, suppression des `active_scan_runs`,
  `active_task_runs` et des lignes de staging dont `id:tentative` ne correspond à aucun bail ;
- les écritures de démarrage `scheduled_scans` (`dispatching`) et `backup_schedule` (`running`)
  ne marquent plus « interrompu » un travail encore sous bail ;
- les verrous d'exécution (`active_scan_runs`, `active_task_runs`) sont lus **avant** les baux puis
  supprimés par paire exacte (bibliothèque, `id:tentative`) : un verrou réécrit entre-temps pour un
  scan qui vient d'être réclamé n'est jamais effacé par une passe périodique ;
- le battement n'est écrit qu'en PostgreSQL (SQLite, mono-Core, ne s'inscrit qu'au démarrage : pas
  d'écriture toutes les 10 s qui empêcherait la mise en veille d'un disque de NAS) ;
- pendant le drainage d'arrêt (`MEDIA_SHUTDOWN_DRAIN_SECONDS`), le Core ne réclame plus de
  nouvelle tâche ; ses baux en cours continuent d'être renouvelés et terminés ;
- en PostgreSQL, le pool compte au moins 4 connexions (`MEDIA_DB_MAX_CONNECTIONS` plus petit est
  relevé) : une passe de planification ou une sauvegarde tient une connexion pour son verrou de
  session pendant que ses requêtes en prennent d'autres.

**Prérequis d'exploitation : horloges synchronisées (NTP).** Baux et battements sont des
horodatages Unix écrits avec l'horloge de chaque Core. Un écart de plus de quelques secondes
entre machines fait expirer trop tôt les baux d'un Core en avance ou en retard ; au-delà de 45 s,
un Core vivant est jugé mort et son travail local repris. Cette limite est commune à tous les baux
existants (`durable_tasks`, `extension_run_owners`) ; passer à l'horloge de PostgreSQL est une
évolution à mesurer, pas une correction de ce lot.

Preuve actuelle : tests unitaires SQLite et un test PostgreSQL (`#[ignore]`) avec deux stores sur
un même schéma, dans un seul processus. Le test d'intégration à deux processus de l'étape 1 (§6)
reste à écrire.

### 3.4 Réveil

`WAKE` (réveil du moteur de tâches) devient `LISTEN/NOTIFY` avec sondage de secours ; un réveil
manqué retarde seulement, il ne perd rien.

## 4. Caches et télémétrie

### 4.1 Santé des Nodes

Le Core propriétaire écrit un instantané de santé (`node_health(node_id, epoch, sampled_at,
json)`, au plus toutes les 15 s, uniquement si la valeur change sensiblement). Les autres Core
lisent cette ligne pour classer les sources. Le classement garde son principe actuel : aucune
E/S réseau dans le chemin chaud, une lecture mémoire ; la ligne sert de source de rafraîchissement
d'un cache local à durée de vie courte. `last_seen_at` en base fait déjà foi pour la péremption.

### 4.2 Catalogue

Le schéma contient déjà des déclencheurs `cache_gen_*` qui incrémentent un compteur de
génération à chaque écriture pertinente. Le cache de catalogue de chaque Core compare cette
génération à la lecture (une lecture d'une ligne) au lieu de se fier à ses propres écritures.
Alternative minimale : désactiver ce cache quand `MEDIA_MULTI_CORE` est actif et mesurer.

### 4.3 Anti-rejeu

Le Node parle à son propriétaire ; le rejeu entre Core ne survient qu'autour d'un transfert. Deux
options : table de nonces partagée (coût d'écriture par requête signée, à mesurer) ou refus de
toute requête d'époque inférieure + fenêtre de rejeu locale (suffisant si l'époque est vérifiée
avant la signature). Retenir la seconde, la mesure décidant.

### 4.4 Sessions et droits

Les sessions sont en base et lues à la volée par les requêtes de lecture (testé : la fermeture de
session refuse immédiatement les nouvelles requêtes directes). Le cache d'activité (`touch`) ne
porte que `last_seen_at`. À éprouver à deux Core : changement de droits et suppression de session
propagés en moins de la durée maximale de vie d'un cache de lecture (cible : ≤ 5 s), sinon
lecture directe sans cache pour les contrôles de sécurité.

### 4.5 Fichiers locaux

Les paquets d'extension sont déjà reconstruits depuis la base (empreinte vérifiée). Les fichiers
dérivés locaux (artwork, caches de préparation côté Core) restent par Core, reconstruits à la
demande ; le ramasse-miettes d'`AssetStore` doit prendre un verrou consultatif PostgreSQL avant de
supprimer, sinon un Core efface ce que l'autre vient d'écrire.

## 5. Effets externes

Un effet externe (appel fournisseur de métadonnées, téléchargement de sous-titres, notification,
envoi d'un webhook, sauvegarde distante) ne peut pas être annulé par un retour en arrière SQL.

| Classe | Exemple | Stratégie |
|---|---|---|
| Idempotent par nature | lecture de métadonnées | rejouable ; cache indexé par clé de requête |
| Idempotent par clé | création distante avec clé fournie | `external_effects.idempotency_key` transmise au tiers |
| Non idempotent | notification, quota payant | réservation → exécution → confirmation |

```
external_effects(id, idempotency_key UNIQUE, kind, owner_core, fence, state,
                 request_hash, response, reserved_at, completed_at)
  state ∈ reserved | performed | completed | unknown
```

- **Réserver** (insertion unique, fencée) avant l'appel ; seul le gagnant exécute.
- **Confirmer** après l'appel dans une transaction fencée.
- **`unknown`** : un Core meurt entre l'appel et la confirmation. Chaque `kind` déclare sa
  réconciliation (interroger le tiers avec la clé, ou accepter le doublon documenté). Aucun rejeu
  silencieux d'un effet non idempotent.
- **Budgets de fournisseurs** (quotas) : jeton de seau en base, pas en mémoire de processus.

## 6. Ordre de réalisation et preuves de sortie

Chaque étape se livre seule, derrière `MEDIA_MULTI_CORE=experimental`, et sa preuve doit passer
avant la suivante.

| # | Étape | Preuve de sortie |
|---|---|---|
| 0 | Fait : boîte aux lettres à échéance, péremption, équivalence des sources | `multi_node_reliability` |
| 1 | Identité de Core (`core_instances`), nettoyage de démarrage non global (§3.3) | un second Core démarre pendant un scan sans l'interrompre ; test d'intégration deux processus |
| 2 | Tâches système et sauvegardes via `durable_tasks` (§3.2.4) | deux Core, une seule exécution par échéance, sur 1 000 échéances |
| 3 | Propriété des canaux + époque (§2.1, §2.3) — fait pour le canal, appels directs non fencés ; preuve de sortie à écrire | arrêt brutal du propriétaire : le Node est repris par l'autre Core en moins de 3 baux, aucune commande d'époque ancienne exécutée |
| 4 | Boîte d'envoi durable (§2.2) — fait ; preuve dans un seul processus avec Node simulé (10 000 commandes, 3 changements de propriétaire, zéro perte, zéro double exécution), preuve à deux processus à écrire ; rotation de secret en deux temps faite (§2.3) | commandes émises depuis le Core non propriétaire ; aucune perte, aucun doublon sur 10 000 commandes avec 3 arrêts brutaux |
| 5 | Fencing des publications et lots (§3.2.1-2) | arrêt brutal pendant la publication : zéro publication partielle, zéro double publication |
| 6 | Caches : génération de catalogue, santé de Node (§4) | latence de propagation d'une écriture et d'une révocation, p50/p95/max mesurés à deux Core |
| 7 | Effets externes (§5) | injection de pannes entre appel et confirmation : état `unknown` observé, jamais de rejeu non idempotent |
| 8 | Chaos à deux Core | scénarios de `MULTI_NODE_RELIABILITY.md` rejoués à deux Core avec coupures réseau, redémarrages, tâches concurrentes, droits différents |

La cible des mesures reprend la preuve de sortie existante de `MULTI_CORE_READINESS.md` : deux
vrais processus Core, une PostgreSQL commune, 100 Nodes simulés, 100 000 tâches, arrêt brutal
pendant les traitements, comptage des effets durables, distribution des temps de reprise,
CPU et RSS.

## 7. Contraintes tenues

- **SQLite autonome conservé.** Rien ici ne s'applique au mode autonome ; un second Core sur
  SQLite reste refusé.
- **Pas de Redis ni de courtier imposé.** PostgreSQL suffit au départ : bail par comparer-et-
  échanger, `LISTEN/NOTIFY`, boîte d'envoi en table. **Seuil de réexamen mesurable** : si, à 100
  Nodes et 10 commandes/s, le p95 de réclamation de la boîte d'envoi dépasse 50 ms ou si la
  charge PostgreSQL attribuable au sondage de secours dépasse 10 % d'un cœur, évaluer un
  courtier — avec les mesures en main, pas avant.
- **Relais jamais obligatoire.** La lecture directe reste la voie nominale.
- **Aucune permission contournée.** Un Core qui reprend un Node ne reprend pas ses droits : les
  vérifications de lecture restent en ligne sur chaque requête.
- **Pas de basculement transparent en plein flux.** Aucune étape de cette feuille de route ne le
  promet ; le lecteur redemande après une coupure.

## 8. Questions ouvertes

1. Le Node doit-il connaître plusieurs URL de Core, ou seulement l'URL logique du répartiteur ?
   (L'URL logique est déjà le modèle documenté dans `ARCHITECTURE.md`.)
2. Durée du bail de canal : 3 × 25 s est prudent ; la mesure du temps de reprise dira si 2 × suffit.
3. Part réelle du relais en production multi-machines (décide §2.4).
4. Faut-il un journal unifié des opérations d'extension avant l'étape 7 ? (`MULTI_CORE_READINESS.md`,
   bloqueur 5.)
