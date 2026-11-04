# Statistiques de lecture natives

Module Rust/Svelte facultatif, désactivé à l'installation et sans service externe.
L'administrateur l'active dans **Administration → Statistiques → Collecte et
conservation**. **Menu profil → Mes statistiques** donne accès à l'activité du
compte courant. Une information visible signale la collecte sur le serveur.

## Données et interprétation

Les lecteurs vidéo et musique transmettent des faits observés : ouverture,
lecture, pause, déplacement, changement réussi de piste, vitesse, chargement,
erreur codifiée et fin. Les choix initiaux et les changements audio/sous-titres
portent les identifiants, langues et codecs disponibles. Les origines sont
`manual`, `automatic` et `sync` (Watch Party). Les commandes de synchronisation
ne sont pas assimilées aux choix linguistiques manuels. Les pauses sont les
pauses observées du lecteur, y compris celles nécessaires à une préparation.

- **Lectures ouvertes** : sessions créées, y compris une tentative sans lecture
  effective. **Spectateurs et médias distincts** du récapitulatif excluent les
  sessions sans temps consommé.
- **Temps consommé** : somme des intervalles observés pendant lesquels le lecteur
  avançait, hors pause et chargement. Pour un intervalle de temps monotone `dt`,
  de déplacement média `dp` et de vitesse précédente `v`, contribution
  `min(dt, dp/v)` si `0 < dp <= dt*v + 0,75 s` et `dt <= 90 s` ; sinon zéro.
  Le point avant un déplacement explicite termine l'intervalle précédent ; la
  destination ne remplit jamais les portions sautées.
- **Couverture** : longueur de l'union des portions effectivement traversées dans
  cette session. Regarder deux fois la même portion augmente le temps consommé,
  sans augmenter deux fois la couverture. **Terminée** signifie couverture
  supérieure ou égale à 90 % de la durée connue. Atteindre le dernier instant,
  sauter le générique ou marquer manuellement « vu » ne suffit pas.
- **Sessions supplémentaires** : nombre de sessions au-delà de la première pour
  chaque couple compte/média dans la sélection. Ce nombre inclut reprises et
  nouvelles lectures ; il ne prétend pas distinguer leur intention.
- **Langues et méthodes** : le temps est attribué à la langue et à la méthode
  observées pendant chaque intervalle, pas entièrement à la dernière piste.
  Les distributions de choix initiaux restent des distributions de sessions.
- **Démarrage** : délai client observé depuis l'ouverture de la fenêtre de
  collecte jusqu'au premier échantillon en lecture sans chargement. Il n'inclut
  pas la récupération préalable du réglage d'activation et ne mesure ni sortie
  physique audio ni affichage effectif d'une image.

Les dates des filtres portent sur le **début des sessions** ; bilans, classements
et temps décrivent cette sélection. Les agrégats quotidiens de ces sessions sont
répartis à minuit UTC et les horaires sont en UTC. Les notes et favoris indiquent
leur état actuel, indépendamment de la période et du nombre de lectures. Les
notes ne sont pas pondérées par le nombre de sessions. Les classements restent
explicites : personnes, temps, complétion, favoris ou notes ; aucun score de goût
ni déduction automatique « arrêt = désintérêt ».

Un changement de source d'une même œuvre conserve la session ; le Core valide
la nouvelle source et son identité éditoriale canonique. Les identifiants musique
restent distincts de ceux du catalogue vidéo. Chaque titre suivant et chaque
nouvelle lecture après une fin ouvre une session. Une présence est envoyée toutes
les 15 secondes ; après 90 secondes sans réception la session est interrompue.
Une reprise de collecte crée un nouvel identifiant, sans inventer le temps perdu.

## Confidentialité, stockage et arrêt

Le Core déduit le compte de la session authentifiée et protège chaque mutation
par CSRF. Une session analytique appartient également à la session de connexion
qui l'a ouverte. Le propriétaire est conservé sous forme de hachage, sans clé
étrangère vers la connexion, pour préserver les bilans après expiration de celle-ci.
Ce hachage n'est jamais renvoyé. Les événements ne contiennent ni URL de streaming,
ni chemin média, ni jeton, ni texte brut d'erreur. Le nom d'appareil existant dans
la connexion est réutilisé ; aucune IP brute, géolocalisation, empreinte ou sortie
réseau vers un collecteur tiers n'est ajoutée.

L'administrateur voit l'ensemble ; la vue personnelle, ses détails et ses exports
appliquent le compte courant, les permissions des bibliothèques et le contrôle
parental courant, y compris les horaires. Une bibliothèque retirée ou un média
devenu restreint disparaît de cette vue. Les instantanés restent consultables par
l'administrateur après retrait du média. La suppression d'un compte supprime ses
sessions et leurs contributions par cascade.

Par défaut, les événements détaillés expirent à **365 jours** ; sessions et
agrégats restent **sans expiration** (`summaryRetentionDays=0`). La conservation
des détails est réglable de 1 à 3 650 jours ; celle des bilans de 0 à 36 500 jours.
La tâche `clean-statistics`, toutes les cinq minutes par défaut, ferme au plus
500 sessions obsolètes, purge au plus 5 000 événements et 500 bilans expirés par
passage. Les détails expirés ne sont plus servis, même avant leur purge physique.
Une purge manuelle applique ces mêmes règles et limites. Les agrégats quotidiens
et les répartitions linguistiques sont conservés avec leur session, ce qui permet
de retirer exactement les contributions d'un compte.

**Désactiver** ferme les sessions actives et arrête l'ingestion ; l'historique et
les tâches de conservation restent accessibles. Réactiver change la génération
de collecte : une ancienne requête ne peut pas rouvrir une session antérieure.
**Supprimer mon historique** efface sessions, événements et contributions aux
bilans, sans modifier progression, notes ni favoris. La collecte future dépend
toujours du réglage du serveur. Les périodes d'activation sont affichées ; elles
ne constituent pas une garantie de livraison par chaque client.

Les tables `statistics_*` sont ajoutées dans la transaction d'initialisation du
schéma existant, sur SQLite et PostgreSQL. Aucun ancien historique de progression
n'est converti en sessions supposées. Les Media Nodes ne collectent pas les
actions du lecteur ; le navigateur contacte le Core même lorsque le média est
servi directement par un Node distant. Pas de dépendance supplémentaire.

## API v1

Toutes les réponses sont `private, no-store` et nécessitent une connexion.
Les mutations exigent origine autorisée et `x-csrf-token`.

| Méthode et chemin | Contrat |
| --- | --- |
| `GET /api/v1/playback/statistics` | Activation, première activation, conservation, signal 15 s et expiration 90 s. |
| `POST /api/v1/playback/sessions` | `{id: UUID client, mediaId: identifiant asset vidéo ou titre musique, music: bool}`. Création idempotente, compte et bibliothèque déduits côté Core. |
| `POST /api/v1/playback/sessions/{id}/events` | `{events:[...]}` : lot ordonné de 1 à 100 événements, corps au plus 64 Kio. |
| `GET /api/v1/admin/statistics` | Bilans globaux, classements, activité, historique et périodes. Administrateur seulement. |
| `GET /api/v1/account/statistics` | Même contrat, restreint au compte courant et à ses droits actuels. |
| `GET …/statistics/sessions/{id}?after=sequence` | Bilan et jusqu'à 2 000 événements ; `nextSequence` permet de continuer. Détail soumis aux mêmes droits. |
| `GET …/statistics/export` | CSV protégé contre les formules, mêmes filtres ; lots de 2 000 lignes, indicateur `x-export-truncated`. L'interface propose le lot suivant. |
| `GET/PUT /api/v1/admin/statistics/settings` | `{enabled:bool,eventRetentionDays:int,summaryRetentionDays:int}`. |
| `POST /api/v1/admin/statistics/purge` | Purge bornée selon la conservation actuelle. |
| `DELETE /api/v1/account/statistics` | Suppression de tout l'historique statistique personnel. |

Filtres : `from` inclusif et `to` exclusif en secondes Unix, `userId` (admin),
`libraryId`, `nodeId`, `kind` (`film`, `episode`, `music`), `device`, `rank`
(`users`, `seconds`, `completion`, `favorites`, `rating`), `offset`, `limit`.
Défaut : 30 jours, 50 sessions, classement par personnes. `limit` va de 1 à 100,
`offset` de 0 à 100 000. Les classements affichent au plus 30 médias et artistes /
albums, 100 utilisateurs et 200 valeurs par filtre. L'agrégation n'est pas limitée
à la page d'historique. La chronologie et les exports sont paginés.

Chaque événement porte `sequence`, `kind`, `elapsedMs` monotone, `position`,
`duration`, `playing`, `buffering`, `rate` et `origin`. Champs facultatifs :
`fromPosition`, `previousTrack` et `track` (`id`, `language`, `codec`), `method`
(`direct`, `remux`, `separate_audio`, `prepared`, `unknown`), `sourceMediaId`
uniquement pour `source_change`, `stopReason` et `errorCode` codifié. Les types
réalisés sont définis dans `statistics/calculation.rs` et le collecteur TypeScript.
La séquence est contiguë ; les numéros déjà acceptés restent des no-op même
après purge des événements. Un lot invalide est intégralement annulé. Une session
fermée renvoie `410`, une collecte désactivée `409` ; le lecteur repart ensuite
avec une nouvelle session autorisée. Pas de persistance locale des événements :
file mémoire de 100 échantillons, délais réseau 2,5 s et clôture en dernier recours.
Une panne de collecte n'affecte pas la lecture ni la progression.

## Vérification et limites

Résultats réellement obtenus et parcours restant :
[validation locale du 4 novembre 2026](STATISTICS_VALIDATION_2026_11_04.md).

Les tests ciblés couvrent les calculs de pause/déplacement/répétition/vitesse et
chargement, les pistes et leur origine, le même contrat HTTP sur SQLite et
PostgreSQL 18 jetable, les permissions et restrictions parentales, l'isolation des
comptes, la déduplication, la clôture après expiration, la conservation et l'effacement.
Les tests Web couvrent aussi désactivation, changement de compte pendant une
requête, échec réseau et séparation des lectures musicales.

Le parcours `browser_statistics` utilise des fixtures locales légales AV1/Opus,
le vrai lecteur vidéo, le vrai lecteur musique, l'administration, la vue
personnelle et un redémarrage du Core. Ses captures et rapports vont dans
`output/playwright/statistics/`. L'état des exécutions figure dans le jalon de
validation, pas dans les seules descriptions de tests.

Il s'agit de mesures déclarées par des clients instrumentés, pas d'une preuve
anti-fraude ni de l'attention d'une personne. Un client tiers non instrumenté,
un onglet tué, un appareil hors ligne ou une réponse perdue laisse des lacunes.
Les files et les intervalles sont bornés ; une session dépassant 7 jours ou
4 096 portions disjointes doit être renouvelée. La navigation, les recherches,
les imports JellyTrack, les recommandations et les bilans annuels sont différés.
Le NAS, les fournisseurs, les appareils physiques et la charge de production ne
sont pas attestés par ces tests locaux.
