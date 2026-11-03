# Mission 5 — préparation de plusieurs Core actifs

> Mise à jour du 3 novembre : **multi-Core toujours expérimental, non prêt pour
> production**. Le lot de propriété de l'historique des extensions est détaillé
> plus bas. Il ne résout pas le dispatch des Nodes ni le fencing de tous les
> effets durables et externes.

État au 26 octobre 2026. Le mode SQLite avec un Core reste le mode autonome.
Deux Core actifs exigent une même base PostgreSQL ; cette configuration n'est
pas encore validée comme déploiement pris en charge. Aucune donnée SQLite n'est
transférée automatiquement vers PostgreSQL.

## Premier parcours vérifiable

- L'admission des tâches et des automatisations prend désormais un verrou
  transactionnel PostgreSQL commun avant de lire les capacités puis d'attribuer
  les leases. SQLite conserve `BEGIN IMMEDIATE`. Les renouvellements, résultats
  et expirations restent conditionnés par `id`, `attempt`, Node et lease.
- Le paquet signé conservé dans `extensions.package_json` est la source durable
  du Wasm exécuté. L'empreinte SHA-256 et la taille sont revérifiées au chargement.
  Le cache local est reconstruit depuis la base s'il est ancien ; une copie locale
  périmée ne fournit plus le code exécuté.
- Le démarrage d'un second Core PostgreSQL ne marque plus immédiatement toutes
  les exécutions d'extensions en cours comme interrompues. Les nouvelles
  exécutions manuelles de catalogue disposent d'un propriétaire, d'un jeton et
  d'un heartbeat persistants. Les anciennes lignes sans propriétaire conservent
  la fenêtre prudente d'une heure sur PostgreSQL. Ce n'est pas un mécanisme de
  rejeu des opérations externes.
- Une tâche système relancée après expiration de son lease peut reprendre un
  plan resté `running` après un arrêt brutal, sous contrôle de sa tentative
  courante. Cela ne rend pas les effets externes exactement une fois.

Le verrou global privilégie la correction des admissions. Il peut augmenter la
latence sous charge. Un benchmark SQLite avec 100 Nodes simulés et 100 000 tâches
persistantes a mesuré 596 ms au p95 pour un lot de 100 attributions avant
optimisation de la lecture des liens d'automatisation. Ce résultat ne fixe pas
une limite de capacité multi-Core PostgreSQL.

## Bloqueurs restants avant deux Core en production

Le schéma PostgreSQL neuf fournit désormais les fonctions de compatibilité
`unixepoch`, `instr`, `json_valid` et `json_extract` nécessaires aux parcours
testés. Les contrats HTTP, OIDC et deux Nodes TCP passent sur PostgreSQL 18.
Deux processus Core ont aussi démarré sur la même base et le survivant a servi
`/api/v1/config` après l'arrêt forcé de l'autre. Ces essais ne couvrent pas
encore tous les parcours SQL ni les garanties de reprise sous charge.

1. Les connexions et l'état de présence des Nodes sont locaux à chaque Core.
   Il faut une propriété durable du canal Node, un transfert sûr après panne et
   un dispatch qui ne réclame que les Nodes joignables par le Core propriétaire.
2. `EFFECT_GATE` et certains verrous de mutation des extensions sont locaux au
   processus. Il faut lier dans une transaction PostgreSQL la vérification du
   fencing token et l'application de chaque effet durable (inventaire, cache,
   métadonnées, état d'extension), avec clé d'idempotence.
3. Les tâches système doivent disposer d'une exclusion et d'un renouvellement
   valables entre Core, y compris pour les effets non transactionnels tels que
   les sauvegardes et fichiers locaux. Les caches/fichiers dérivés locaux ne
   sont pas automatiquement partagés par PostgreSQL.
4. Les sessions sont en base, mais les parcours OIDC, le bootstrap, les secrets,
   la configuration publique et les URL derrière répartiteur doivent être
   éprouvés avec deux processus et un proxy réel. Les changements de droits
   doivent être relus ou invalidés de façon bornée sur chaque Core.
5. Le propriétaire et le heartbeat de l'historique manuel sont implémentés,
   mais ne couvrent pas un journal unifié de toutes les opérations d'extensions.
   Les effets des automatismes continuent de dépendre de leurs propres leases
   et des points de fencing restant à compléter. Le cache de paquets nécessite
   aussi une écriture atomique si plusieurs Core partagent son volume.

La preuve de sortie reste un essai reproductible avec deux vrais processus Core,
une PostgreSQL commune, 100 Nodes simulés, 100 000 tâches, arrêt brutal pendant
les traitements, comptage des effets durables et distribution des temps de
reprise, CPU et RSS. Le smoke test à deux Core ne mesure ni tâches en vol,
ni reprise, ni absence de double effet.

## Vérifications PostgreSQL 18 locales

- `scripts/test-fresh-schema.py` et `scripts/test-fresh-postgres-schema.ps1` :
  création neuve du schéma unique sur SQLite et PostgreSQL.
- `scripts/test-postgres.ps1` : contrat HTTP/réouverture (1 test), OIDC
  (7 tests) et parcours TCP à deux Nodes (1 test), tous réussis.
- `cargo test --locked -p media-poc --test automation_flow hundred_thousand_persistent_tasks_admission -j 1 -- --ignored --nocapture --test-threads=1` :
  SQLite Debug local, 100 Nodes simulés et 100 000 tâches persistantes ;
  insertion 3 572 ms, attribution groupée p50 477 ms/p95 596 ms, admission
  unitaire p50 10,3 ms/p95 15,1 ms. Mesure antérieure à la lecture groupée
  des liens d'automatisation et sans deux processus Core. Après ce regroupement,
  même scénario : insertion 3 524 ms, lot p50 509 ms/p95 582 ms, admission
  unitaire p50 9,8 ms/p95 12,2 ms. Le gain p95 reste modeste et ne démontre
  pas une accélération générale.
- `scripts/test-two-cores.ps1` : deux processus Core sur une PostgreSQL commune,
  réponse des deux sur `/api/v1/config`, création d'un administrateur et d'une
  session sur Core 1, lecture de cette session sur Core 2, puis réponse du
  survivant après arrêt forcé du premier. Un passage Windows Debug local :
  deux Core prêts en 1 894 ms, RSS 21,7 Mio par Core, réponse du survivant
  11 ms après la commande d'arrêt. Cette mesure concerne une requête de configuration sans tâche en
  vol ; elle n'est pas une distribution de latence de reprise. Les données de
  test sont en mémoire dans Docker et les caches locaux sont isolés.
  Après consolidation du schéma : deux Core prêts en 8 591 ms, RSS 21,8 et
  21,6 Mio, session inter-Core valide, réponse du survivant en 35 ms. Les
  valeurs de démarrage varient avec la machine et les compilations concurrentes.

## Lot du 3 novembre — propriété de l'historique des extensions

`extension_run_owners` est une table additive SQLite/PostgreSQL liée par clé
étrangère à `extension_runs`. La création de la ligne d'exécution et de son
propriétaire est transactionnelle. Le propriétaire de processus est un UUID
aléatoire stable pendant sa vie ; chaque exécution possède un jeton distinct.
Son lease dure 30 secondes et est renouvelé toutes les 5 secondes pendant
l'invocation, sans bloquer la progression de son worker sur une attente SQL.

La publication terminale de l'historique et son nettoyage vérifient ensemble
l'identifiant, le propriétaire, le jeton, l'état `running` et un lease encore
valide dans une écriture conditionnelle transactionnelle. Un ancien propriétaire
ne peut ni ressusciter son lease expiré, ni écraser un résultat déjà terminal.
Le garde d'annulation utilise le même fencing. Un worker ayant perdu son lease
est interrompu ; son résultat n'est pas retourné comme une réussite.

Le démarrage et la lecture administrateur de l'historique récupèrent les leases
expirés en marquant leurs lignes `interrupted`, sans rejouer l'invocation. Un
second Core ne marque pas une exécution possédant un lease vivant comme morte.
Les anciennes lignes sans propriétaire sont traitées avec le délai antérieur
d'une heure sur PostgreSQL ; SQLite reste réservé à un Core unique.

Ce lot concerne les lignes créées par le parcours manuel de métadonnées.
L'acquisition de sous-titres possède ses réservations durables séparées ; les
avatars et tous les automatismes ne sont pas artificiellement ajoutés au même
journal. En particulier, le cache d'automatisation garde actuellement une
transaction d'écriture pendant son appel worker : une centralisation naïve des
insertions d'historique sur une autre connexion créerait un interblocage SQLite.
Ce refactoring est différé plutôt que de changer ce parcours dans un lot de
propriété de l'historique. Aucune URL temporaire de sous-titres ni configuration
secrète n'est ajoutée à `extension_runs`.

Les écritures privées du guest et les effets catalogue ou fichiers ne sont pas
tous dans la même transaction que la vérification de ce lease. L'invariant
garanti par le code porte donc sur **la publication de l'historique**, pas sur
une exécution exactement une fois des effets ni une reprise multi-Core complète.
`ACTIVE_CHANNELS`, `EFFECT_GATE`, les mutations d'extensions, les fichiers
partagés/reconstructibles et les parcours OIDC derrière répartiteur restent les
bloqueurs listés plus haut. Deux pools PostgreSQL ne sont pas deux processus Core.

Tests ciblés préparés : SQLite avec deux pools, et
`pwsh -NoProfile -File scripts/test-postgres.ps1 -OnlyExtensionRuns` pour une
PostgreSQL 18 jetable et deux pools indépendants. Ils couvrent lease vivant,
expiration, refus du jeton périmé, publication unique et conservation du résultat.
La présence de ces tests ne signifie pas qu'ils ont été exécutés ; les résultats
réels sont consignés dans la matrice de mission après passage.

Retour arrière : un ancien binaire ignore la table additive, mais ne sait pas
renouveler ou respecter ses leases. Arrêter toutes les instances avant un retour
de version et utiliser un Core unique ; ne pas mélanger anciens et nouveaux
binaires derrière un répartiteur pour prétendre conserver ce fencing.

## Historique de consolidation du schéma

Les bases neuves sont créées depuis un seul fichier SQL source contenant une
section SQLite et une section PostgreSQL. Le Core n'exécute plus d'historique
de migrations. Une base déjà présente n'est jamais réinitialisée ; une ancienne
structure incompatible produit une erreur explicite et conserve ses données.
La version initiale du schéma est enregistrée dans `schema_version`.

Tant que le produit est en phase de construction et que les bases sont jetables,
le schéma initial peut évoluer par recréation. Si des installations contenant
des données doivent ensuite être mises à jour sans perte, une stratégie de mise
à jour du schéma devra être décidée explicitement.
