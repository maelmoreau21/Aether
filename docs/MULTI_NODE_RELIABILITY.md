# Un Core, plusieurs Media Nodes : fiabilité

> 7 novembre 2026. Audit du code, correctifs, scénarios réels (Core et Nodes sur TCP local) et
> mesures **avant / après**. Périmètre : **un seul Core actif**, SQLite conservé. Le multi-Core
> n'est pas activé ; sa conception est à part dans [`MULTI_CORE_ROADMAP.md`](MULTI_CORE_ROADMAP.md).
> Preuves brutes : [`evidence/2026-11-07/multi-node-reliability/`](evidence/2026-11-07/multi-node-reliability/).

## Résumé

| Constat | Gravité | Traitement |
|---|---|---|
| Une commande mise en file derrière la première, ou acceptée entre deux requêtes de poll, était **perdue** (le récepteur était recréé à chaque poll). | Élevée | Boîte aux lettres par Node, durable pour le processus |
| Aucune échéance sur les commandes : une commande livrée tard s'exécutait telle quelle. | Moyenne | `expiresAt`, rétrocompatible |
| `MEDIA_NODE_STALE_SECS` valait 0 par défaut : un Node coupé sans erreur (câble, courant) restait « connecté » et recevait des URL de lecture directe. | Élevée | Défaut de 75 s ; même règle pour classer **et** pour émettre une URL directe |
| Le failover considérait toute copie de la **même œuvre** comme équivalente (autre montage, autre durée) et y reprenait la position enregistrée. | Élevée | Équivalence prouvée (empreinte ou durée) |
| Le lecteur complétait la liste serveur avec n'importe quelle entrée du catalogue de la même œuvre. | Moyenne | Le lecteur n'utilise que les candidats validés par le Core |
| Un Node révoqué recevait `403 node_revoked` à chaque poll et réessayait indéfiniment. | Faible | Le Node s'arrête (`revoked`) — non testé automatiquement |
| `session_hash` figure dans le jeton de lecture alors que la doc affirmait l'inverse. | Faible, à traiter | Commentaire corrigé ; référence opaque proposée (§3) |

Rien n'a été trouvé de défaillant dans la validation des jetons, des `Range`, de l'expiration
et de la révocation : ces points sont désormais couverts par un test, qui passe avant comme
après (le code d'origine était sain).

## 1. Orchestration et transfert des données

Le Core **orchestre** (appairage, droits, descripteur de lecture, tâches, choix de source). Les
**octets** sortent du Node :

- *Direct* : le descripteur contient `directUrl` (jeton signé, 15 min maximum). Le navigateur lit
  chez le Node ; le Node demande au Core d'autoriser **chaque nouvelle requête**.
- *Relais* : `/api/v1/assets/{id}/content` sur le Core, en flux borné (jamais l'actif en mémoire),
  utilisé quand le Node n'est pas joignable par le navigateur ou en repli.

Mesuré (`direct_streaming_bypasses_the_core_and_the_relay_costs_are_measured`) : lecture d'une
fenêtre de 16 Mio, octets identiques par les deux voies, et le compteur de relais du Core
(`core_active_relays`) reste à **0** en direct et monte à 1 par le relais.

| Voie | 1er octet | Durée 16 Mio | Relais Core actifs (pic) |
|---|---|---|---|
| Directe | 7 ms | 49 ms | 0 |
| Relais Core | 9 ms | 44 ms | 1 |

Un seul passage, compilation debug, Core et Nodes dans le même processus, boucle locale : ces
débits ne disent rien de ce que donnerait un réseau réel et l'ordre des lectures (la seconde
profite du cache disque) les biaise. Seule l'absence d'octets portés par le Core en direct est
une conclusion. Le relais n'est jamais obligatoire : le Core ne l'impose que si le Node n'a pas
d'URL publique, est périmé ou ne répond pas.

## 2. Jetons de streaming, `Range`, expiration, droits, révocation

Constaté dans le code puis **vérifié par test** (`direct_tokens_are_bound_authorized_online_and_revoked_with_the_session`
pour le comportement de bout en bout ; la rotation de clé et la version du jeton le sont par les
tests unitaires existants de `verify_playback_token`) :

| Propriété | Résultat |
|---|---|
| Signature HMAC-SHA256 du Node, comparaison à temps constant | jeton altéré → 403 |
| Liaison Node / actif / action / clé | jeton d'un autre actif → 403 ; mauvaise action → 403 ; clé tournée → refus |
| Durée de vie | `exp` dépassé → 403 ; durée annoncée supérieure au plafond de 900 s → 403 |
| Jeton absent | 401 |
| Bibliothèque du jeton ≠ celle de l'actif | 403 (Node) |
| Droits | contrôlés **en ligne** par le Core à chaque requête : un jeton correctement signé pour un spectateur sans droit → 403 |
| Révocation | suppression de la session → les **nouvelles** requêtes directes sont refusées 403, alors que le jeton n'a pas expiré |
| `Range` | fenêtre 206 exacte, suffixe `-N` 206, début hors fichier 416, plusieurs plages 416 |

Limites assumées :

- Une réponse **déjà commencée** n'est pas coupée par une révocation : seules les nouvelles
  requêtes sont réautorisées. Un `GET` complet d'un long film continue jusqu'à sa fin. Le
  lecteur utilise des requêtes `Range` bornées ; cette limite n'a pas été mesurée sur un
  client réel.
- Si le Core est injoignable, les nouvelles requêtes directes sont refusées (échec fermé, voulu).
- `session_hash` (SHA-256 du jeton de session, pas le jeton) est dans la charge du jeton, qui est
  **signée mais pas chiffrée** : quiconque détient l'URL le lit. L'URL est déjà une capacité
  porteuse pour 15 minutes tant que la session existe, donc le gain d'un attaquant est
  faible, mais la charge ne devrait pas contenir de dérivé du secret de session. Proposition :
  une référence opaque non secrète (colonne dédiée de `sessions`). Changement de protocole,
  **non fait** dans ce lot.

## 3. Déconnexion, reconnexion, version de protocole, expiration des commandes

### 3.1 Commandes : le défaut et le correctif

Avant : `node_channel` créait un `mpsc` à chaque poll et le détruisait en répondant. `try_send`
réussissait alors que la commande suivante ou celle arrivée entre deux polls disparaissait avec le
récepteur. Le test d'origine, rejoué sur le code non modifié, le montre : trois commandes
acceptées, le Node reçoit la première puis deux `ping` (voir `evidence/…/reliability-before.txt`).

Après : une boîte aux lettres par Node (`NodeChannel`) qui survit aux polls :

- livraison en ordre, une commande par poll, le Node repolle aussitôt (deux
  polls successifs reçoivent les deux commandes suivantes en moins de 5 s, borne vérifiée par le test) ;
- une commande acceptée **entre** deux polls attend le suivant (grâce de 5 s après le dernier poll) ;
- file bornée à 32 ; au-delà ou si le Node ne poll plus, la commande est **refusée à l'émission**
  au lieu d'être mise en file pour rien ;
- un poll plus récent remplace un poll plus ancien du même Node ;
- `expiresAt` (secondes Unix) : 15 s pour un relais, 60 s pour scan/tâche/annulation/synchro,
  300 s pour rotation/révocation. Le Core jette les commandes périmées (compteur
  `expired_count`) ; le Node refuse une commande en retard de plus de 300 s (tolérance de dérive
  d'horloge identique aux requêtes signées).

Compatibilité : `expiresAt` est facultatif. Un ancien Core n'en envoie pas (jamais périmé) ; un
ancien Node l'ignore. Tests : `command_envelope_without_deadline_stays_wire_compatible`.

**Reste vrai** : la livraison est « au plus une fois ». Une réponse perdue sur le réseau perd la
commande, sans accusé de réception. Les travaux durables s'en remettent à leur bail (§4), un relais
expire côté navigateur. Un accusé par commande est le sujet de la boîte d'envoi durable de la
feuille de route multi-Core.

### 3.2 Péremption d'un Node silencieux

Un Node appairé envoie un battement toutes les 15 s (et poll toutes les ≤ 25 s). Sans signe de vie
depuis 75 s (cinq battements), le Node est « déconnecté » pour le classement **et** pour l'émission
d'URL directes. `MEDIA_NODE_STALE_SECS=0` rétablit l'ancien comportement.

Fenêtre résiduelle : entre la panne et 75 s (+ jusqu'à 15 s de battement), le Core croit encore le
Node vivant. Pendant ce temps une URL directe peut pointer vers un Node mort ; le lecteur retombe
alors sur sa gestion d'erreur (§6). Un relais, lui, échoue en ≤ 3 s et marque le Node déconnecté
immédiatement (comportement existant, testé).

### 3.3 Version de protocole

Le Core exige `x-media-protocol: v1` **avant** toute vérification de signature ; un autre numéro,
vide ou absent → `401 unsupported_protocol` sur le poll, le battement et les routes signées. Test :
une requête correctement signée avec `v2` est refusée, **n'est pas un signe de vie** (le Node reste
classé périmé), et le premier appel valide suivant le remet en service. Les jetons portent aussi la
version (`unsupported_token_version`). Le Node, lui, retente avec un recul de 1 à 5 s ; il ne
signale pas l'incompatibilité à l'administrateur — amélioration utile, non faite.

### 3.4 Révocation

Un Node révoqué est refusé à la porte (`403 node_revoked`) et ne peut donc jamais recevoir la
commande `Revoke`. Il traite maintenant ce `403` comme une révocation, cesse de poller et persiste
son état. Les autorisations de lecture directe échouaient déjà (le Core refuse un Node révoqué).
**Ce changement côté Node n'est couvert par aucun test automatique** (le contrôle d'échéance
côté Node non plus, hors la logique pure de `NodeCommandEnvelope`).

## 4. Tâches distribuées : baux, reprises, annulations, idempotence

Aucune modification : les tests existants sont rejoués et passent (`distributed_tasks` : 5 verts,
dont `leases_fence_old_workers_and_survive_reopen`, `concurrent_claims_never_overbook_or_run_same_library`,
`controls_hold_capacity_until_stop_ack_and_retries_are_bounded`). Constaté dans le code :

- bail renouvelé seulement pour un exécutant vivant (Node vu il y a ≤ 30 s **et** scan déclaré actif
  dans son battement) ; numéro de tentative = jeton de fencing ; résultats d'une ancienne tentative
  refusés (`stale_task_result`, `stale_scan_attempt`) ;
- côté Node, une commande de scan ou de tâche déjà enregistrée est ignorée (`duplicate_scan`), donc
  une double livraison ne lance pas deux exécutions ;
- annulation : résultats fencés immédiatement, créneau conservé jusqu'à expiration du bail (une
  livraison n'est pas la preuve d'un arrêt).

## 5. Choisir une source équivalente et autorisée

### 5.1 Règle

Même œuvre ≠ même contenu. Une copie n'est candidate que si :

1. **identique** : même taille et même empreinte partielle (`inventory_asset_fingerprints`), ou
2. **même durée** : les deux sondes donnent une durée et elles diffèrent de moins de
   `max(1 s, 0,2 %)`, de sorte qu'une position enregistrée garde son sens.

Un montage plus long, une durée inconnue ou invalide : **jamais** substitué. Tests unitaires de la
règle (`inventory::equivalence`) et scénario réel `a_copy_with_another_runtime_is_never_substituted`
(copie de 3 s contre 8 s de la même œuvre : ni alternative ni failover).

### 5.2 Permissions non fusionnées

La liste est filtrée par les bibliothèques visibles du spectateur et par la politique parentale.
Quand l'appelant n'a pas résolu de bibliothèques visibles, seules les copies de **la bibliothèque de
l'actif demandé** sont considérées (échec fermé). Le chemin de contenu et l'autorisation du Node
revérifient les droits sur l'actif choisi. Scénario réel (`equivalent_copy_is_a_failover_target_only_for_viewers_allowed_to_read_it`) :
deux bibliothèques, une par Node, deux spectateurs avec des droits différents.

| Spectateur | Droits | Voit l'autre copie ? | Si son Node tombe |
|---|---|---|---|
| Administrateur | les deux | oui, `alternatives[0]` (`identical`) | basculé sur l'autre Node |
| Alice | bibliothèque 1 seule | non | « indisponible », pas de `failoverFrom`, pas d'URL directe |
| Bob | bibliothèque 2 seule | non | idem côté Node 2 |

### 5.3 Contrat

Le descripteur expose `fallbackMediaIds` (copies équivalentes, autorisées, sur un Node connecté) et
`alternatives` (toutes les copies équivalentes autorisées, avec `nodeName`, `nodeStatus`,
`connectionMode`, `score`, `equivalence`). **Choisir** une source revient à demander le descripteur
de son identifiant ; il n'existe pas de second mécanisme de contournement. L'interface n'affiche
pas encore de sélecteur : seuls les types (`MediaAlternative`) sont déclarés.

## 6. Failover avant lecture et entre requêtes — et ce qui n'est pas promis

- **Avant la lecture** : le descripteur bascule vers la meilleure copie équivalente connectée si le
  Node demandé est déconnecté ou périmé (`failoverFrom`). Testé, y compris sans qu'aucune requête
  n'ait échoué.
- **Entre deux requêtes** : après la mort d'un Node, la requête suivante tombe sur la copie
  équivalente et **reprend au même octet**. Testé (`a_node_killed_mid_read_…`) : 128 Kio reçus du
  Node 1, Node tué, reprise par `Range` sur le Node 2 depuis l'offset reçu, octets identiques au
  fichier.
- **En plein flux** : **non transparent, non promis.** La réponse en cours s'arrête court (mesuré :
  le corps est tronqué avant `Content-Length`). La reprise dépend du lecteur, qui, sur erreur,
  appelle `tryAutomaticFailover` : il redemande un descripteur et recharge à la position observée.
  Ce chemin n'a pas été rejoué dans un navigateur réel dans ce lot. Le lecteur n'utilise plus que
  les candidats validés par le Core (§5).

## 7. Arrêts brutaux : scans, préparations, lectures

| Scénario | Résultat mesuré |
|---|---|
| Lecture : Node tué à mi-réponse | réponse interrompue, octets reçus intacts, reprise exacte sur la copie équivalente |
| Scan : Node tué en plein scan (150 titres) | le scan passe en `error` après **58,2 s** (expiration du bail de 60 s, mesuré 58,6 / 58,1 / 58,2 s sur trois passes) ; **aucun** titre de la bibliothèque interrompue n'est publié ; la bibliothèque de l'autre Node scanne normalement |
| Préparation : arrêt brutal | **non testé** : les préparations exigent l'encodeur du projet, absent de cet environnement (les deux tests de préparation existants échouent à l'identique avant et après) |

Le scan interrompu ne se termine que si le planificateur tourne (comme dans le binaire `aether-core`) :
le harnais de test le démarre désormais. Le scénario de scan dure ~2 min : il est `#[ignore]` et se
lance explicitement (§9).

## 8. Mesures avant / après

Environnement : Docker (`rust:slim-bookworm`), compilation debug, boucle locale, Core et Nodes dans un
seul processus de test, SQLite. Ce ne sont pas des capacités de production.

### 8.1 Sélection de source (`scale_benchmark_1_10_and_100_simulated_nodes_warm_selection`, 50 itérations)

| Nodes simulés | Avant p50 / p95 | Première version p50 / p95 | **Finale p50 / p95** | RSS avant → final |
|---|---|---|---|---|
| 1 | 2,25 / 3,16 ms | 2,77 / 3,72 ms | 2,54 / 3,65 ms | 59,7 → 60,0 Mio |
| 10 | 2,42 / 2,58 ms | 3,13 / 3,48 ms | 2,97 / 3,15 ms | 61,9 → 62,4 Mio |
| 100 (89 copies prêtes) | 3,88 / 4,43 ms | 7,53 / 9,92 ms | **5,30 / 6,00 ms** | 65,8 → 66,6 Mio |

La première version décodait la sonde complète de chaque copie : la latence à 100 Nodes doublait.
Elle ne lit plus que `format.duration` (et seulement si l'empreinte ne tranche pas). Il reste un
surcoût de ~1,4 ms au p50 à 100 copies, dû aux colonnes et à la jointure ajoutées ; la cible de la
suite (< 50 ms sans appel réseau par Node) est respectée.

CPU : le benchmark rapporte 0,0 ms (résolution insuffisante sur cet hôte). **Aucune mesure fiable de
CPU, de disque ni de concurrence de Nodes n'a été produite dans ce lot** ; seules la latence de
sélection, le RSS, le temps de reprise d'un scan et le compteur de relais sont mesurés. Le routage
(classement des Nodes) n'a pas été modifié : seul le critère de péremption l'a été.

### 8.2 Scénarios de fiabilité (`multi_node_reliability`)

| Scénario | Code d'origine | Code modifié |
|---|---|---|
| Copie équivalente, droits différents, Node silencieux, reconnexion | échec (champ `alternatives` absent) | **réussi** |
| Copie d'une autre durée jamais substituée | **échec** (substituée comme repli) | **réussi** |
| Commandes en file / entre deux polls | **échec** (`["ping","ping"]` au lieu de `["cmd-b","cmd-c"]`) | **réussi** |
| Version de protocole refusée, reprise après appel valide | **échec** (un Node périmé reste choisi) | **réussi** |
| Jetons, `Range`, droits, révocation | réussi | réussi |
| Lecture : Node tué à mi-réponse | non écrit | **réussi** |
| Scan : Node tué (ignoré par défaut) | non écrit | **réussi** (58,2 s) |
| Lecture directe vs relais (compteur Core) | non écrit | **réussi** |

### 8.3 Non-régression

| Suite | Avant | Après |
|---|---|---|
| `distributed_tasks` | 5 verts | 5 verts |
| `multi_node_scale_and_failover` (2) | verts | verts |
| `chapter_images_remote`, `healthcheck_resilience` | non rejoués avant | verts |
| `distributed_multi_node`, `node_enrollment_and_relay` | échec ligne 923 / 484 (`unsupported_preparation`) | **même échec, mêmes lignes** |
| `media-node` lib | 34 échecs (`mp4_audio`…, encodeurs externes) | **mêmes 34** (liste comparée) |
| `media-poc` lib | non comptée avant | 294 verts, dont 19 nouveaux |
| `media-contracts` lib | non comptée avant | 15 verts, dont 2 nouveaux |
| `apps/web` : tests du lecteur | non rejoués avant | 173 verts ; `svelte-check` : 1 erreur préexistante (`format.test.ts`, types Node) |

Les échecs communs à l'avant et à l'après viennent de l'environnement (encodeurs/packaging du projet
absents), pas de ces changements. `cargo clippy --workspace --all-targets -- -D warnings` et
`cargo fmt --check` passent.

## 9. Reproduire

Fixtures synthétiques (`scripts/generate-fixtures.ps1`) et `ffmpeg` requis ; sinon les scénarios
s'annoncent ignorés et rendent la main.

```bash
cargo test -p media-poc --test multi_node_reliability -- --test-threads=1 --nocapture
# scénario de scan (~2 min)
cargo test -p media-poc --test multi_node_reliability -- --test-threads=1 --ignored --nocapture
cargo test -p media-poc --lib nodes::channel nodes::registry inventory::equivalence
cargo test -p media-contracts command_
```

## 10. Hors périmètre et suites

- Sélecteur de source dans l'interface (les données `alternatives` sont prêtes).
- Référence de session opaque à la place de `session_hash` dans le jeton (§2).
- Accusé de réception des commandes et boîte d'envoi durable → feuille de route multi-Core.
- Signalement à l'administrateur d'une incompatibilité de protocole côté Node.
- Test d'arrêt brutal pendant une préparation, dans un environnement muni des encodeurs du projet.
- Mesure de CPU, de disque et de concurrence de Nodes sur matériel réel.
