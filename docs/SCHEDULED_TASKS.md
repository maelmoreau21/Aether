> Le moteur persistant décrit dans [DISTRIBUTED_TASKS.md](DISTRIBUTED_TASKS.md) remplace le dispatch globalement sériel historique : leases par Node, reprises bornées, priorités, états persistants et suivi collectif des dossiers. Les routes des plans ci-dessous sont conservées.

# Planificateur de tâches — refonte du 8 novembre 2026

**Une seule source de vérité.** Les déclencheurs (`task_triggers` : quotidien, hebdomadaire,
intervalle, démarrage) sont la seule configuration. Un passage unique,
`task_scheduler::fire_due`, exécuté toutes les 15 secondes, lit les déclencheurs échus, met
une exécution en file par tâche (clé idempotente), puis avance par compare-and-set uniquement
les déclencheurs qui ont tiré. Les trois pompes séparées (tâches système, scans, sauvegarde)
qui lisaient les colonnes historiques `next_due_at` / `next_run_at` ont disparu ; ces colonnes
ne sont plus qu'un miroir pour les anciennes routes `PUT .../schedule`.

Règles d'exécution :
- une tâche déjà en attente ou en cours quand son déclencheur tombe est sautée (comme
  Jellyfin) ; un instant manqué pendant un arrêt ne tire qu'une fois, jamais en rafale ;
- les déclencheurs de bibliothèques supprimées ou de tâches retirées sont nettoyés ;
- la maintenance légère (sessions, statistiques, journaux, caches, catalogues) démarre dès
  qu'aucune autre maintenance ne tourne, même si des scans ou des lectures sont en cours, et
  n'occupe plus l'emplacement de scan du Media Node local ;
- l'optimisation de la base et la sauvegarde attendent un serveur calme (aucun scan local,
  aucune lecture) au plus une heure, puis démarrent quand même ;
- les sondages de surveillance temps réel ne comptent plus comme des analyses de
  bibliothèque : ils ne bloquent plus le bouton Lancer et ne remplacent plus la dernière
  exécution affichée ; celle-ci couvre aussi les bibliothèques à plusieurs dossiers ;
- mettre une tâche en file n'efface plus son dernier résultat ; un marqueur « en cours »
  laissé par un arrêt brutal s'affiche « Interrompue » et n'empêche plus de relancer ;
- la sauvegarde ne passe « en cours » qu'au démarrage effectif, plus à la mise en file.

L'API unifiée renvoie des codes explicites (`task_already_active`, `task_unavailable`,
`task_not_active`, `task_not_found`, `trigger_invalid`, `trigger_duplicate`,
`trigger_limit`, `trigger_not_found`) traduits par l'interface. La liste affiche la prochaine
exécution de chaque tâche.

La file « Suivi des opérations » (`/api/v1/admin/tasks/jobs`) n'était pas montée dans
l'application : la requête tombait sur `/api/v1/admin/tasks/{id}` et répondait 404
(« Opération impossible »). Elle est désormais montée, avec un test sur l'application complète.

Tests : `crates/core/tests/task_scheduler.rs` (montage des routes, instant manqué, tâche
occupée, famine, surveillance, marqueurs périmés) et `crates/core/tests/scheduler.rs`.

# Scans planifiés — 19 octobre 2026

## Parcours du planificateur corrigé — 2 novembre 2026

Les plans sont désormais visibles en premier et ouverts à l’arrivée. La file
d’exécution reste accessible dans une rubrique distincte ; elle ne charge ni ne
sonde ses opérations tant qu’elle est repliée. Les plans et résultats système,
scans et sauvegardes sont relus toutes les dix secondes lorsque le planificateur
est actif et le document visible. Les requêtes simultanées sont partagées et les
mutations attendent la lecture précédente pour éviter de réafficher un ancien
intervalle après enregistrement. Les erreurs de chaque source restent distinctes.

La boîte de configuration expose l’activation et les bornes effectives du backend
(5 minutes à 7 jours pour scans/système ; 60 minutes à 7 jours pour sauvegardes).
Désactiver un intervalle de scan utilise désormais PUT `enabled:false`, comme les
plans système et sauvegardes : intervalle, date du dernier lancement et résultat
restent conservés. L’ancienne route DELETE reste disponible pour ses autres clients.
Une planification désactivée peut être réactivée depuis le même formulaire.

Un lancement système accepté est présenté comme mis en file, sans le confondre
avec son démarrage ou son succès. Les détails montrent `lastMessage`, la classe
d’erreur et sa portée transmises par l’API. Une ouverture directe attend le
chargement avant d’annoncer une tâche introuvable. Le paramètre `?task` du fragment
est pris en compte uniquement dans `#tasks-section`, sans contaminer les autres
rubriques d’administration.

Sources Jellyfin consultées : `features/tasks/hooks/useLiveTasks.ts` pour les
changements d’état, `components/NewTriggerForm.tsx` pour les types de déclencheur.
Aether continue ici d’exposer uniquement ses intervalles effectivement exécutés ;
les horaires quotidiens/hebdomadaires, limites de durée et plusieurs déclencheurs
de Jellyfin ne sont pas simulés dans l’interface.

Validation ciblée : `navigation.test.ts` (2 cas), ESLint des fichiers touchés et
`scripts/browser-scheduler-settings.cjs` avec API simulées aux largeurs 390/1440
(enregistrement scan, désactivation/réactivation, file système, actualisation
périodique, erreurs structurées, bornes sauvegardes). Aucun changement au worker
ou preuve d’exécution réelle NAS n’est déduit de ces scénarios.

## Libellé des déclencheurs — 27 octobre 2026

L'interface nomme les déclencheurs actuels « intervalles » et affiche « Toutes les 24 heures » pour 1440 minutes. Le moteur part de l'heure d'enregistrement ou du dernier lancement ; il ne garantit pas « chaque jour à 7 h ». Un horaire civil fixe requiert une règle persistée avec fuseau horaire et un calcul explicite des changements d'heure pour les scans, sauvegardes et tâches système.

Le Core possède un plan persistant par bibliothèque Films/Séries, locale ou distante.
L’administrateur active le plan et choisit un intervalle de 5 à 10 080 minutes dans
Administration → Tâches. Les médias restent en lecture seule. Le scanner et la
commande de scan manuel partagent exactement le même dispatch ; aucun shell ni
commande arbitraire ne peut être planifié.

## Contrat HTTP

- `GET /api/v1/admin/tasks/schedules` : liste des plans. Session administrateur obligatoire.
- `PUT /api/v1/admin/tasks/schedules/{libraryId}` : `{ "intervalMinutes": 1440, "enabled": true }`.
- `DELETE /api/v1/admin/tasks/schedules/{libraryId}` : suppression idempotente du plan.

Mutations : session administrateur, même origine et jeton CSRF obligatoires.
PUT renvoie 204, 404 si bibliothèque absente/révoquée ou fixture réservée, 422 si
intervalle hors limites. Le corps est limité à 4 Kio et les champs inconnus refusés.
La réponse GET expose libraryId, intervalMinutes, enabled (0/1), nextRunAt,
lastRunAt (null avant premier lancement) et lastOutcome. Dates Unix UTC en secondes,
affichées par le navigateur dans son fuseau local.

## Exécution et reprise

Le binaire conserve un garde du worker, démarré après la récupération des scans
interrompus. Aucun worker n’est lancé par le constructeur HTTP utilisé dans les
tests. Son abandon arrête la boucle ; l’arrêt des scans existant reste responsable
des probes. Toutes les 15 secondes : mise à jour des résultats terminaux, puis au
maximum un lancement si aucun scan n’est actif. La concurrence locale reste bornée
par le sémaphore existant ; les scans distants suivent le canal Node existant.
Les bibliothèques désactivées et Nodes révoqués ne sont pas exécutés.

La prochaine échéance est enregistrée par compare-and-set avant le dispatch.
Un redémarrage ne produit pas de rafale de rattrapage : une échéance dépassée
entraîne un seul scan, puis un nouvel intervalle à partir du lancement effectif.
Une tentative durable expirée peut reprendre un plan resté `running` après
arrêt brutal, sous contrôle du lease courant. Un arrêt pendant le dispatch produit `interrupted` au redémarrage. Un échec est
conservé et réessayé seulement à la prochaine échéance. Modifier/enregistrer un
plan recalcule sa prochaine échéance. Le bouton Actualiser relit les résultats.

Ce jalon concerne les scans seulement. Les horaires calendaires/cron, l’historique
complet des exécutions, les sauvegardes automatiques et plusieurs Core restent
hors de ce parcours. Les résultats réels des tests sont consignés par le jalon
principal ; aucune validation NAS ou navigateur réel n’est déduite du test HTTP.

## Extension Général / Réseau / profil / actualisation (19 octobre 2026)

Voir [contrats et limites](SETTINGS_AND_REFRESH.md) pour les paramètres persistants du serveur,
la configuration réseau effective, les avatars de compte et les quatre modes de scan.
Les fournisseurs de métadonnées en ligne restent à développer.
