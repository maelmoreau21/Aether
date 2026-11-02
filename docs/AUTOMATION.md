# Automatisation événementielle — 26 octobre 2026

Le Core réutilise `durable_tasks` : aucune nouvelle file de travail ni boucle
indépendante. Les scans, déclencheurs système et extensions alimentent cette file.
Les opérations physiques restent dans les Nodes autorisés. Le Wasm reçoit le
contexte éditorial et la configuration de sa seule extension, jamais un chemin
média, une connexion SQL ou un droit d'écriture dans les originaux.

## Événements et reprise

La publication de l'inventaire insère les événements ajout/modification/retrait
et les observations dans la même transaction. Les événements supportés incluent
aussi analyse terminée, métadonnées manquantes, activation, manuel et programmé.
Le worker existant consomme l'outbox par lots bornés ; avancement du curseur et
admission des tâches sont atomiques. Les demandes de fanout conservent un curseur
persistant par asset. Aucun futur n'est créé pour chaque élément du backlog.

Livraison au moins une fois, effets validés idempotents : aucune promesse
d'exécution physique exactement une fois. Les leases et tentatives existants
refusent les retours périmés. Dépendances acycliques, propagation des prérequis en
échec, retries, pause, annulation et reprise utilisent les contrôles existants.

L'empreinte tient compte de l'asset, taille, horodatage, disponibilité, racine,
configuration, options de bibliothèque/langue, version/génération du paquet,
opération et version d'algorithme. Ce n'est pas un hash du contenu média : une
modification conservant taille et horodatage peut échapper à cette détection.
Les résultats et leur fin de tâche sont persistés dans la même transaction avec
revalidation des droits, du lease et de l'empreinte.

## Concurrence et fournisseurs

Quatre processus Wasm maximum par Core, une invocation par extension, dont trois
automatiques maximum pour réserver une place interactive. Admission immédiate,
sans attente en mémoire ; les tâches différées restent dans SQLite/PostgreSQL.
Les limites Node et bibliothèque existantes restent actives ; les réservations
par extension, bibliothèque logique et fournisseur sont transactionnelles.

Le broker impose une seconde entre deux requêtes d'une invocation. Une réponse
429/503 produit un signal hôte Retry-After (secondes ou date HTTP, délai borné à
24 heures). Le Core conserve le cooldown partagé du fournisseur en base. Retries
avec backoff exponentiel et jitter bornés. L'identité fournisseur utilise le
premier domaine signé : les fournisseurs multidomaines demandent un raffinement.
La priorité à la lecture reporte/préempte le travail de fond ; aucune absence de
ralentissement de lecture n'est établie par ces seules protections.

## Périmètre fonctionnel

Le parcours automatique livré concerne les métadonnées : `metadata.propose` ou
`metadata.search`. Le réseau utilise par défaut la recherche ; une unique
correspondance exacte de titre permet de demander les détails. Les réponses
ambiguës restent des résultats à examiner. Les autres capacités des extensions
(images, paroles, empreintes/intro, avatars) conservent leurs parcours existants ;
leur orchestration automatique complète reste un prochain jalon.

L'extension doit être active et autorisée pour la bibliothèque ; son éditeur doit
être actif. L'exécution utilise un administrateur existant, sans compte implicite.
Les résultats restent séparés des corrections manuelles. `metadata.write` est
nécessaire pour compléter les champs éditoriaux absents (description, résumé,
genres). Titres corrigés, droits, pistes et choix de langue ne sont pas écrasés.
Les résultats valides sont réappliqués après rescan. Le catalogue suit son
rechargement existant ; aucun nouveau canal push vers le lecteur n'est ajouté.

## Administration

Administration > Tâches expose le mode par extension/bibliothèque : automatique,
manuel, désactivé ou programmé. API administrateur avec CSRF pour les mutations,
corps limités à 4 Kio et champs inconnus refusés :

| Route | Réponse |
| --- | --- |
| GET `/api/v1/admin/tasks/automation/policies` | Jusqu'à 1000 politiques |
| PUT `/api/v1/admin/tasks/automation/policies` | 204, politique invalide 422 |
| POST `/api/v1/admin/tasks/automation/run` | 202 avec requestId ; cible interdite/absente 404 |
| GET `/api/v1/admin/tasks/automation/results` | 100 derniers résultats |

Politique : extensionId, libraryId, mode, priority (0–1000), operation,
configRevision, algorithmVersion, intervalSeconds (60–31536000), nextRunAt,
maxConcurrent (1–16, réduit par les limites globales). Demande manuelle :
`{libraryId, extensionId, idempotencyKey}`. Répéter la même clé conserve la demande.
Les résultats inchangés terminés sont réutilisés ; les erreurs terminales peuvent
être relancées manuellement après libération du lease.

## Migrations et limites

0002 ajoute l'outbox. 0003 étend la file aux tâches système : bibliothèque NULL,
identité locale existante et catégorie system. Le worker système indépendant est
retiré ; échéance et admission sont atomiques. SQLite reconstruit la table en
restaurant tâches, dépendances, liens et résultats dans la transaction de
migration ; PostgreSQL adapte les contraintes. Le test de migration vérifie la
conservation des données et les clés étrangères.

Le schéma initial est en cours de consolidation par le chantier stockage : le
déploiement sur une ancienne base doit suivre sa procédure de migration.
Le snapshot est paginé en mémoire (256 assets), mais sa comparaison complète
reste sous la transaction de publication. Prochain petit jalon : n'émettre que
les deltas sans allonger le verrou d'écriture sur les grandes bibliothèques.

Preuves et limites de mesure : `docs/evidence/2026-10-26/automation/results.json`.
Les essais locaux SQLite et Nodes TCP sur une machine ne prouvent ni PostgreSQL,
ni un NAS, ni 100 machines physiques, ni les percentiles du lecteur.


## Mesures locales du jalon

Tests ciblés : 7 contrats d'automatisation et 1 parcours TCP à deux Nodes réussis ;
5 tests runtime et 1 test de migration réussis. Vérification Web : 0 erreur,
5 avertissements existants. Scan AV1/Opus + Wasm signé : 3412,335 ms ;
deux Nodes TCP et trois assets enrichis : 2246,817 ms ; nouveau média sur Node2
pendant la panne de Node1 : 721,655 ms.

100000 tâches persistantes, 100 Nodes simulés : admission unitaire p95 18,869 ms ;
attribution groupée aux 100 Nodes p95 1534,107 ms. La cible de 50 ms est atteinte
sur l'admission unitaire de cet essai, pas sur l'attribution groupée. Ces mesures
Debug/SQLite locales ne représentent pas 100 machines ni un percentile de lecture.
