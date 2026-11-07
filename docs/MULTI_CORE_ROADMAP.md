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
| `ACTIVE_CHANNELS` (boîtes aux lettres) | `nodes/channel.rs` | un Node n'est connecté qu'à un Core ; une commande émise par l'autre Core est refusée ou perdue | §2 |
| `PENDING_STREAMS` | `nodes/channel.rs` | le relais exige que la requête navigateur et le flux montant du Node arrivent sur le **même** Core | §2.4 |
| `NodeRuntimeRegistry` (télémétrie, latence, flux actifs) | `nodes/registry.rs` | chaque Core ne voit que les battements qu'il reçoit : la sélection de source et le failover divergent | §4.1 |
| `ReplayWindow` (nonces) | `shared/protocol.rs` | un message signé rejoué sur l'autre Core passe la fenêtre anti-rejeu | §4.3 |
| `EFFECT_GATE`, `WAKE` | `task_engine.rs` | exclusion mutuelle des effets limitée au processus | §3 |
| Nettoyage au démarrage de `scans::router` | `scans.rs` | un Core qui démarre efface `scan_batch_receipts`, `active_scan_runs`, `active_task_runs`, `scan_staging_*` **de tous**, donc le travail en vol de l'autre Core | §3.3 |
| `CATALOGUE_CACHE` | `inventory/catalogue/cache.rs` | cache périmé après une écriture sur l'autre Core | §4.2 |
| Cache de sessions / `touch` | `auth/state.rs`, `auth/touch.rs` | une révocation n'est vue qu'après expiration du cache de l'autre Core | §4.4 |
| `MUTATIONS`, paquets d'extension locaux, `AssetStore` | `extensions/`, `assets.rs` | installations non sérialisées, fichiers locaux | §4.5 |
| Tâches système et sauvegardes planifiées | `system_tasks.rs`, `admin_backups.rs` | deux Core lanceraient la même tâche | §3.4 |

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

### 2.3 Fencing côté Node

Chaque commande porte l'`epoch` du propriétaire. Le Node mémorise la plus haute époque vue
(persistée) et **refuse toute commande d'époque inférieure** ; il refuse aussi les requêtes
signées d'un Core d'époque ancienne. Un ancien Core isolé du réseau (donc ignorant qu'il a perdu
son bail) ne peut plus agir sur ce Node dès que le nouveau propriétaire a parlé une fois.

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

### 3.3 Démarrage d'un second Core

Le nettoyage de démarrage de `scans::router` doit cesser d'être global. Remplacer par :
« ne reprendre que les lignes dont le propriétaire est **ce** Core ou dont le bail est expiré ».
Tant que ce n'est pas fait, démarrer un second Core détruit le travail en vol du premier. C'est
le premier bloqueur à lever, et il est testable seul (deux Core, un scan en cours, démarrage du
second).

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
| 3 | Propriété des canaux + époque (§2.1, §2.3) | arrêt brutal du propriétaire : le Node est repris par l'autre Core en moins de 3 baux, aucune commande d'époque ancienne exécutée |
| 4 | Boîte d'envoi durable (§2.2) | commandes émises depuis le Core non propriétaire ; aucune perte, aucun doublon sur 10 000 commandes avec 3 arrêts brutaux |
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
