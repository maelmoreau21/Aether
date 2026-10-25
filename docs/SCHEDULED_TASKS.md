> Le moteur persistant décrit dans [DISTRIBUTED_TASKS.md](DISTRIBUTED_TASKS.md) remplace le dispatch globalement sériel historique : leases par Node, reprises bornées, priorités, états persistants et suivi collectif des dossiers. Les routes des plans ci-dessous sont conservées.

# Scans planifiés — 19 octobre 2026

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
Un arrêt pendant le dispatch produit `interrupted` au redémarrage. Un échec est
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
