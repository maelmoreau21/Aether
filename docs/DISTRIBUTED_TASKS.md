# Moteur de tâches distribué — 24 octobre 2026

Le canal sortant d'un Node applique un délai de reconnexion progressif, plafonné
à cinq secondes, aux erreurs réseau et aux réponses signées mal formées. Une
commande valide rétablit le délai initial. Ce mécanisme ne prouve pas à lui seul
la reprise d'un travail interrompu ni la résilience sur un NAS.

Le Core possède désormais une file persistante de travaux multimédias. Il admet les tâches,
attribue un lease au Node qui possède la bibliothèque, suit leur exécution et
refuse les résultats d'une ancienne tentative. Le scanner Node existant effectue
le travail : découverte incrémentale, analyse technique, NFO et images locales
selon les options de bibliothèque déjà prises en charge. Les médias restent en
lecture seule. Aucun Redis, broker, service JavaScript permanent ou nouveau
service n'est nécessaire ; SQLite et le Node local restent le fonctionnement
par défaut. Les migrations PostgreSQL équivalentes sont présentes, sans transfert
automatique de données entre moteurs.

Les catégories admises sont `scan`, `analyze`, `prepare` et `image`.
`analyze` cible un asset inventorié ; `prepare` prépare une composante avec
les outils média existants ; `image` produit un aperçu d'une image locale
inventoriée. Le Core conserve un résultat JSON borné et applique les mêmes
leases et contrôles que pour les scans. Les futures tâches de métadonnées
déclenchées par des extensions restent à intégrer. Aucune nouvelle dépendance
de transcodage vidéo n'est imposée.

## Attribution et limites

`durable_tasks` conserve l'identité, le propriétaire, la bibliothèque physique,
le Node, la priorité, l'état, le compteur de tentative, les échecs, les dates,
le lease, les compteurs de traitement et une erreur bornée à 1 024 caractères.
Les états exposés sont `queued`, `running`, `paused`, `completed`, `cancelled`
et `error`.

La file accepte au maximum 100 000 tâches en attente, en cours ou suspendues.
Les tâches terminées ne sont pas effacées silencieusement ; leur rétention et
leur purge administrative restent à définir. Il n'y a ni future Tokio ni
sémaphore en attente par tâche en file : seuls les travaux admis obtiennent
un exécuteur. Les lectures de liste, les passes de dispatch et les leases
consultés sont bornés. Des index distincts couvrent les candidats, les leases,
leur expiration et les bibliothèques.

Le worker cherche au maximum 100 Nodes candidats par passe. La concurrence
de production est limitée à une tâche de fond par Node et à une exécution physique par
bibliothèque. La priorité est un entier de 0 à 1 000, les valeurs élevées
passent d'abord ; à priorité égale, la date puis l'identifiant départagent les
tâches. Une transaction d'écriture sérialise l'admission et empêche le
dépassement de capacité par des claimers concurrents. Cela reste une
architecture à Core unique pour le déploiement pris en charge. Les admissions
PostgreSQL utilisent désormais un verrou transactionnel commun entre Core ;
les autres bloqueurs multi-Core sont consignés dans
[MULTI_CORE_READINESS.md](MULTI_CORE_READINESS.md).
Les mutations de la file attendent leur tour avant d'acquérir une connexion
SQL : elles ne monopolisent pas les quatre connexions SQLite en attendant le
verrou d'écriture. Une passe de production attribue jusqu'à 100 leases dans
une seule transaction, avant de lancer les exécuteurs. Les contrôles DB
restent nécessaires si plusieurs connexions tentent de réclamer le même job.


Un scan reste attaché au Node qui possède son dossier. Une bibliothèque
désactivée ou un Node révoqué ne reçoit plus de tâche. Un Node hors ligne
conserve les tâches dans la file. Il n'existe pas encore de redistribution
automatique d'un scan vers une autre copie physique : la copie et ses droits
doivent être explicitement modélisés avant de pouvoir changer de destination.
Les bibliothèques à plusieurs dossiers utilisent un job par emplacement dans
le parcours des scans planifiés et dans l'admission HTTP des jobs.

## Leases, interruptions et effets

Chaque admission incrémente `attempt`, utilisé comme fencing token dans
l'identifiant de scan `jobId:attempt`. Un lease dure 60 secondes. Le worker
renouvelle seulement un exécuteur local encore suivi ou un Node connecté
ayant récemment annoncé cet identifiant exact dans `activeScanIds`.
Le protocole de commande et les retours d'inventaire conservent leur
authentification HMAC et leurs contrôles de bibliothèque.

Avant d'accepter des résultats ou de publier l'inventaire, le Core vérifie
l'identifiant actif, la destination et le lease encore valide. Un verrou
partage cette vérification et la publication avec les contrôles et la
récupération des leases expirés. Un ancien worker ne peut pas valider une
nouvelle tentative avec son ancien identifiant. Cela ne promet pas une
exécution physique exactement une fois : un processus lent ou isolé peut
continuer à calculer avant d'observer son arrêt, mais ses résultats périmés
sont refusés.

`failureCount` compte les échecs, `maxAttempts` représente leur budget, fixé
à trois à l'admission. Un échec ordinaire ou l'expiration d'un lease en cours
consomme un échec ; la tâche repart avec un délai borné ou devient `error`
lorsque le budget est atteint. `attempt` reste monotone indépendamment de ce
budget. Une pause, une annulation ou une préemption pour lecture ne consomme
pas d'échec. La préemption utilise le résultat interne `playback_preempted`
et un délai de réadmission d'une seconde.

La pause et l'annulation invalident immédiatement le droit de publication.
Le slot reste réservé jusqu'à l'arrêt constaté ou l'expiration du lease.
Un accusé de transport seul ne constitue pas une preuve d'arrêt. La reprise
est refusée tant qu'un lease de la tentative suspendue reste présent.
La reprise relance un scan incrémental ; elle ne restaure pas l'instruction
d'un processus interrompu ni un décodage en cours. Les caches et inventaires
déjà validés sont réutilisables, mais aucun checkpoint durable de chaque
fichier encore en staging n'est promis.

Après redémarrage, les tâches restent en base. Les tâches non commencées
restent en attente et les leases abandonnés expirent avant retry. Les anciens
retours Node restent soumis au fencing. Les erreurs de base interrompent la
passe et sont réessayées ; un succès n'est pas simulé en mémoire.

Chaque plan conserve la clé de sa dernière demande durable. Son résultat
agrège les tâches de tous ses emplacements : un échec terminal domine, une
tentative encore en cours ou en attente conserve l'état actif et la réussite
exige la réussite de tous les jobs. Une erreur temporaire du scanner ne
devient donc pas le résultat définitif d'un job en attente de retry. Les plans
antérieurs sans clé de demande gardent la règle de récupération historique.

## Priorité à la lecture et télémétrie

Les flux actifs empêchent l'admission de scans sur leur Node. Le Node refuse
un nouveau scan lorsque la lecture est active et observe l'arrivée d'un flux
pendant le travail pour préempter le scan. L'annulation est coopérative ; elle
peut interrompre un futur de scan et les probes configurés pour s'arrêter à
l'abandon. Elle ne garantit pas une interruption instantanée de tout appel
système ou de toute opération de disque.

Les heartbeats peuvent fournir une télémétrie optionnelle signée : CPU,
mémoire disponible/totale, espace disponible du stockage applicatif, pression
I/O et débit réseau. Le sampler coalesce les demandes pendant une seconde et
effectue les appels OS dans `spawn_blocking`. La première mesure de CPU/réseau
reste inconnue tant qu'aucun delta temporel n'est disponible.

Sur Linux, les sources sont `/proc/stat`, `/proc/meminfo`, `/proc/pressure/io`
et `/proc/net/dev`, avec `statvfs` pour l'espace du dossier applicatif. Les
limites RAM cgroup v2 sont prises en compte lorsqu'elles sont exposées aux
chemins usuels. Le CPU reste une mesure agrégée de l'hôte ; la capacité CPU
des quotas de conteneurs n'est pas calculée. Le réseau additionne les octets
reçus/émis hors loopback ; les interfaces virtuelles peuvent compter deux
fois le même trafic. Ce compteur ne mesure pas la capacité du lien.

Sous Windows, CPU, mémoire physique et espace du dossier applicatif utilisent
respectivement `GetSystemTimes`, `GlobalMemoryStatusEx` et
`GetDiskFreeSpaceExW`. Le CPU ne couvre que le groupe processeur courant sur
les machines de plus de 64 processeurs logiques. Les métriques I/O et réseau
ne sont pas fournies sous Windows. Sources :
[temps CPU](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getsystemtimes),
[mémoire](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-globalmemorystatusex).

Pour une mesure disponible datant de moins de 30 secondes, l'admission
diffère le scan dans les cas suivants :

| Ressource | Seuil de report |
| --- | --- |
| CPU | utilisation au moins 85 % |
| Mémoire disponible | moins de 128 Mio ou moins de 5 % du total connu |
| Pression I/O Linux | `some avg10` au moins 50 % |
| Stockage applicatif | moins de 128 Mio disponibles |
| Réseau | débit au moins égal au budget facultatif configuré |

Une métrique absente ou périmée reste inconnue : elle ne devient ni zéro ni
une preuve de capacité libre. Les compteurs de lecture et les limites de
concurrence restent appliqués. Ces seuils sont des garde-fous d'admission,
pas une garantie de temps réel ou de stabilité de lecture mesurée.

La seule nouvelle option opérationnelle est `MEDIA_TASK_NETWORK_LIMIT_BPS`,
un entier strictement positif en **octets par seconde** lu au démarrage du
Core. Exemple PowerShell avant le lancement habituel du Core :

```powershell
$env:MEDIA_TASK_NETWORK_LIMIT_BPS = '52428800'
```

Cela reporte les nouveaux scans à partir de 50 Mio/s rapportés ; cela ne
limite pas les flux média et ne réserve pas 50 Mio/s de réseau. Sans cette
variable, aucune capacité de lien n'est supposée.

La boucle de tâches attend 500 ms lorsqu'elle suit du travail actif et cinq
secondes au repos. Une notification réveille la boucle après admission,
contrôle ou terminaison. Les plans historiques restent examinés toutes les
15 secondes. Il ne faut donc pas confondre la durée d'une requête de claim
avec le délai bout en bout d'un plan périodique ou d'une tâche différée.

## API et administration

Toutes les routes ci-dessous exigent une session administrateur vivante.
Les mutations exigent le jeton CSRF et les protections d'origine du Core.
Le corps est limité à 4 Kio ; les champs inconnus sont refusés. Aucun chemin
disque, commande shell ou nom d'exécutable n'est accepté.

| Méthode et route | Contrat |
| --- | --- |
| `GET /api/v1/admin/tasks/jobs?limit=100&offset=0` | `{ jobs, limit, offset }`, au plus 200 résultats ; offset borné à 100 000 |
| `POST /api/v1/admin/tasks/jobs` | `{ libraryId, idempotencyKey, priority?, mode?, category?, targetId?, component?, width? }`, réponse 202 avec le job principal |
| `GET /api/v1/admin/tasks/jobs/{id}` | job détaillé, y compris son résultat JSON borné |
| `POST /api/v1/admin/tasks/jobs/{id}/pause` | suspension, 204 si transition acceptée |
| `POST /api/v1/admin/tasks/jobs/{id}/resume` | reprise après libération du lease, 204 |
| `POST /api/v1/admin/tasks/jobs/{id}/cancel` | annulation, 204 |

La priorité vaut 100 et la catégorie vaut `scan` par défaut. Le mode vaut `incremental` ; `new_only`,
`missing_metadata` et `full` sont également conservés dans le job et transmis
au scanner. Les autres catégories utilisent le mode `incremental`.

| Catégorie | Cible et options |
| --- | --- |
| `scan` | aucune cible ; étend une bibliothèque à ses dossiers actifs |
| `analyze` | `targetId` = identifiant d'asset disponible dans cette bibliothèque |
| `prepare` | identifiant d'asset et `component` = `video`, `audio-{streamIndex}` ou `subtitle-{streamIndex}` |
| `image` | `targetId` = identifiant d'image disponible ; `width` = 320, 640 ou 1 280, défaut 640 |

Les cibles sont vérifiées contre la bibliothèque et sa racine actuelle.
Les composantes audio/sous-titre doivent correspondre à une piste inventoriée
du bon type. Aucun chemin n'est accepté comme cible. L'analyse conserve son
résultat ; elle ne remplace pas automatiquement les métadonnées éditoriales.
Les préparations et aperçus utilisent les caches existants : la persistance
d'un résultat de job ne rend pas un cache applicatif éternel après redémarrage
ou éviction. La génération d'une planche de miniatures vidéo n'est pas
équivalente au redimensionnement d'une image locale et reste distincte.

La clé d'idempotence contient 1 à 128
caractères ASCII parmi lettres, chiffres, `-`, `_`, `:`, `.`. Elle est unique
par propriétaire pendant toute la durée de vie du job. Répéter la même
requête renvoie le même job ; réutiliser la clé avec une autre bibliothèque
priorité, mode, catégorie, cible ou options renvoie 409. Une bibliothèque ou
cible indisponible produit 404, une
entrée invalide 422, une file saturée 429 et une transition impossible 409.
Les erreurs d'authentification et d'autorisation restent 401/403.

Pour une bibliothèque à plusieurs dossiers, l'admission crée le job principal
et un job par dossier actif. Les clés des enfants sont dérivées par SHA-256,
y compris si la clé principale atteint déjà 128 caractères. La réponse
contient le job principal ; chaque enfant apparaît dans la liste et possède
ses propres contrôles. L'admission utilise une transaction par emplacement :
une erreur à mi-parcours peut laisser certains jobs admis ; répéter la même
requête avec la même clé complète les emplacements sans les dupliquer.

L'état expose `owner`, `nodeId`, `category`, `targetId`, `component`, `width`,
`refreshMode`, `attempt`, `failureCount`, `maxAttempts`,
`processedItems`, `progress`, les dates Unix UTC en secondes, le lease et
l'erreur. Le nombre total de fichiers n'étant pas connu d'avance,
`processedItems` donne le travail traité ; le pourcentage n'est pas estimé
artificiellement et atteint 100 à la terminaison réussie. La durée affichée
est une durée murale, qui peut inclure des pauses ou attentes entre retries.
Le champ `resultJson` est une chaîne JSON de 64 Kio au maximum ; les résultats
trop volumineux sont refusés explicitement. La liste utilise toujours `{}`
pour ce champ afin de ne pas charger les blobs à chaque actualisation ; le
détail authentifié permet de consulter le résultat.

Les routes des plans existants restent disponibles :
[contrat historique des planifications](SCHEDULED_TASKS.md). Administration →
Tâches présente les plans et les exécutions persistantes, leurs propriétaires,
Nodes, erreurs et contrôles.

## Validation reproductible et périmètre restant

Commandes ciblées, à exécuter séquentiellement sous Windows :

```powershell
cargo test -p media-poc -j 1 --test distributed_tasks --test task_engine_http --test scheduler --test task_operations -- --test-threads=1
cargo test -p media-node -j 1 resources -- --test-threads=1
cargo test -p media-poc -j 1 --test distributed_tasks ten_thousand_jobs_on_one_hundred_simulated_nodes -- --ignored --nocapture --test-threads=1
cargo test -p media-poc -j 1 --features query-metrics --test distributed_tasks_scale -- --ignored --nocapture --test-threads=1
```

Le benchmark de file est une simulation SQLite de 10 000 jobs et 100 Nodes.
Il ne prouve pas 100 machines physiques, un transport réseau, un scan massif
réel ni un décodage vidéo stable. Les fixtures de charge peuvent peupler la
base en transaction : leur vitesse d'insertion ne mesure pas celle de
l'admission HTTP authentifiée. Les résultats observés, commandes réellement
terminées et limites doivent être consignés séparément après exécution.
Aucune cible de 50 ms ni aucun débit de production n'est déclaré acquis ici.

Restent à livrer ou à établir : catégories de travail supplémentaires,
planches de miniatures vidéo et budgets administratifs propres à chaque
catégorie ; choix entre plusieurs Nodes équivalents et
redistribution sûre ; checkpoint de staging par fichier ; intégration des
extensions ; configuration administrative des budgets ; purge de
l'historique ; profilage des allocations et des I/O physiques SQLite ; comparaisons CPU/RSS,
navigation et décodage AV1/Opus avec et sans scans concurrents ; preuve sur
réseau et machines réels. La présence des garde-fous et des tests de file ne
remplace pas ces mesures.

La feature facultative `query-metrics`, désactivée par défaut, compte les
constructions de requêtes via les wrappers SQL partagés et les frontières
BEGIN/COMMIT instrumentées du moteur. `statementCalls` décrit ces appels ;
il ne compte pas les pages disque SQLite, les I/O du système ou les appels
SQLx bruts externes aux wrappers. Les chiffres CPU/RSS des bancs concernent
le processus de test Windows, pas un NAS ni un processus Node distant.

`MEDIA_TEST_TASK_BACKLOG=1` ajoute au scénario navigateur `browser_scanner`
une file persistante de 10 000 scans de bibliothèque vide, avant ses deux
parcours MP4/MKV et le vrai redémarrage du Core. La vidéo AV1 et l'audio Opus
restent soumis aux assertions de pixels, PCM, seeks et synchronisation du
scénario existant. Le test exige du travail réellement terminé et aucune
perte de jobs après redémarrage. Ce banc isole la pression de la file et du
scheduler : il ne représente pas 10 000 probes vidéo ni une charge disque
massive. `MEDIA_REPORT_DIR` sélectionne le dossier des preuves.

## Résultats observés le 25 octobre 2026

Preuves brutes et synthèse : [dossier de validation](evidence/2026-10-25/distributed-tasks/summary.json).
Hôte Windows 11, Core Ultra 7 256V, 8 processeurs logiques, Rust 1.99.1,
profil `test` non optimisé, SQLite WAL. Les 100 Nodes de charge sont des
identités simulées. Les limites de ces mesures s'appliquent aux chiffres suivants.

| Scénario | Résultat mesuré |
|---|---|
| 10 000 admissions réelles et durables | 49,95 s ; 200,2 jobs/s ; p50 4,45 ms ; p95 7,34 ms ; max 132,12 ms |
| Liste administrateur pendant ces admissions | 1 752 requêtes ; p95 16,05 ms ; max 56,47 ms |
| CPU et mémoire de ce processus de test | CPU agrégé 55,45 s ; RSS final 19,52 Mio ; instantané, pas un pic |
| Appels SQL instrumentés pendant admissions + navigation | 73 510 appels, incluant les transactions instrumentées et la navigation |
| Attribution unitaire sur 10 000 tâches / 100 Nodes | p50 5,83 ms ; p95 9,13 ms ; p99 21,58 ms ; max 201,96 ms |
| Attribution groupée des 100 Nodes | p50 213,99 ms ; p95 383,89 ms ; max 818,06 ms |
| 100 claimers concurrents, 1 000 jobs exécutés | 65,36 jobs/s ; claim p95 1 639,69 ms ; navigation p95 17,27 ms |

La réserve de connexions pour les lecteurs a corrigé une contention observée :
la liste administrateur était à p95 959,43 ms dans le premier essai concurrent,
puis à 17,27 ms dans l'essai final. Ces essais locaux ne constituent pas une
comparaison matérielle contrôlée ni une garantie de latence.

**L'objectif de 50 ms n'est pas atteint pour l'attribution groupée à 100 Nodes
ni sous contention maximale.** Le p95 unitaire est inférieur à 50 ms ; les
valeurs maximales restent supérieures. Aucun résultat Release, PostgreSQL,
NAS ou 100 machines physiques n'est annoncé.

Vérifications fonctionnelles terminées :

- 15 scénarios Core d'intégration ciblés réussis : états, retries, pause/reprise,
  annulation, doublons, courses d'attribution, dossiers regroupés, scans et
  opérations réelles standalone, Core + deux Nodes sur TCP local et canal sortant.
- 8 tests Core ciblés réussis : autorisations/fencing/configuration modifiée,
  SQL normalisé et arrêt du runner. 84 tests unitaires Node réussis ; 4 tests
  auxiliaires ou bancs optionnels ignorés. Les trois bancs de charge explicites
  ont été lancés séparément et réussissent.
- Chrome 155 headless : vrais pixels AV1 et PCM Opus, MP4 puis MKV après
  redémarrage, seek/pause/pistes/sous-titres/progression et récupération sur erreur.
  Le backlog de 10 000 jobs est conservé : 138 scans de bibliothèque vide
  terminés avant redémarrage, 256 après. Cela valide la pression de la file,
  sans représenter une campagne de 10 000 probes média.
- 32 lectures HTTP Range directes/relais sous backlog, octets identiques aux
  témoins, pendant un scan distant réellement terminé. Cela reste distinct
  du décodage navigateur et ne mesure pas le débit d'un NAS.
- Web : 152 tests réussis, Svelte/TypeScript sans erreur ni avertissement,
  ESLint ciblé et build statique réussis. UI réelle avec API simulée : actions,
  catégories, résultat JSON échappé, clair/sombre et largeurs 390/1440.
- Clippy tous targets avec avertissements refusés : réussi.

Les Core et Nodes doivent être mis à jour ensemble pour les nouvelles
commandes et les identifiants d'exécution annoncés par heartbeat. Les jobs
restent liés au Node propriétaire de leur emplacement ; une redistribution
entre copies, un checkpoint de staging par fichier, des budgets administratifs
configurables et les tâches de métadonnées d'extensions restent des jalons
suivants. Les catégories analyse/préparation ciblent actuellement les assets
films/séries ; les scans incluent aussi les bibliothèques musicales.
