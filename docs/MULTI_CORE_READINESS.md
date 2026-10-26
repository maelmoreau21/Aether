# Mission 5 — préparation de plusieurs Core actifs

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
  les exécutions d'extensions en cours comme interrompues. En l'absence de
  propriétaire/heartbeat dans `extension_runs`, les lignes vieilles d'une heure
  sont marquées interrompues. Cette fenêtre est une limite provisoire, non un
  mécanisme de reprise des tâches.
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
5. `extension_runs` nécessite un propriétaire et un heartbeat pour distinguer
   un Core mort d'une exécution vivante. Le cache de paquets nécessite aussi
   une écriture atomique entre processus si plusieurs Core partagent son volume.

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

## Schéma et données neuves

Les bases neuves sont créées depuis un seul fichier SQL source contenant une
section SQLite et une section PostgreSQL. Le Core n'exécute plus d'historique
de migrations. Une base déjà présente n'est jamais réinitialisée ; une ancienne
structure incompatible produit une erreur explicite et conserve ses données.
La version initiale du schéma est enregistrée dans `schema_version`.

Tant que le produit est en phase de construction et que les bases sont jetables,
le schéma initial peut évoluer par recréation. Si des installations contenant
des données doivent ensuite être mises à jour sans perte, une stratégie de mise
à jour du schéma devra être décidée explicitement.
