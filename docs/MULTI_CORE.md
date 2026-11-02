# Préparation multi-Core (audit, sans déploiement)

Statut : **audit par inspection du code (grep + lecture ciblée), aucun test multi-Core exécuté.** Le multi-Core reste hors MVP. SQLite = un seul Core. Les Media Nodes ne parlent jamais à PostgreSQL.

## Déjà compatible (constaté)

- Tâches `durable_tasks` : lease (`lease_until`), numéro de tentative qui sert de jeton de fencing sur `record_result`/`renew`/`finish`, reprise après expiration, clés d'idempotence. Les requêtes d'écriture sont conditionnelles en SQL, donc valides entre Core.
- Planification des scans (`scheduler.rs`) : revendication par `UPDATE ... WHERE next_run_at=?` (compare-and-set), pas de verrou mémoire.
- Pool PostgreSQL sans verrou global ; verrous consultatifs ciblés (`begin_write_scoped`).

## État uniquement en mémoire (bloquants pour plusieurs Core)

| Élément | Fichier | Effet avec 2 Core | Piste |
|---|---|---|---|
| `EFFECT_GATE`, `WAKE` | `task_engine.rs` | exclusion et réveil limités au processus ; l'exclusion mutuelle des effets de scan n'existe pas entre Core | verrou consultatif PG ou CAS sur la ligne de tâche ; réveil par polling/`LISTEN` |
| `MUTATIONS` | `extensions.rs` | installations/mises à jour d'extensions non sérialisées entre Core ; fichiers `/data/plugins` locaux à chaque Core | verrou PG + distribution des paquets (stockage partagé ou re-téléchargement vérifié par signature) |
| `ACTIVE_CHANNELS`, `PENDING_STREAMS` | `nodes.rs` | un Node est connecté à un seul Core ; un relais ne traverse pas les Core | affinité Node→Core enregistrée en base, redirection |
| `CATALOGUE_CACHE` | `inventory.rs` | cache périmé après écriture sur un autre Core | clé de version en base, ou cache désactivé en multi-Core |
| `AssetStore` (`gate`, `commit`, `leases`) | `assets.rs` | collecteur et écritures non coordonnés ; `LocalFs` local à chaque Core | backend partagé + verrou de collecte en base (cf. en-tête du module) |
| `system_tasks` `gate` (Mutex) et sauvegarde planifiée (`admin_backups.rs`, Semaphore) | — | deux Core pourraient lancer la même tâche/sauvegarde | passer par `durable_tasks` (lease) |
| `scans.rs` annulations, `auth.rs` `pending` | — | état local au processus | **à examiner** : non audité en détail |

## Non vérifié

Sessions et propagation des révocations : les sessions sont en base (voir `docs/SECURITY.md`), mais un éventuel cache mémoire de session n'a pas été audité ici.

## Ordre recommandé

1. Verrous PG / CAS pour `EFFECT_GATE`, `MUTATIONS`, collecteur d'assets.
2. Sauvegarde et tâches système planifiées via `durable_tasks`.
3. Backend d'assets partagé (interface `BlobBackend` déjà en place).
4. Tests : deux Core sur un PG, kill d'un Core en pleine tâche, double revendication.
