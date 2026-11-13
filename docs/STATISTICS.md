# Statistiques de lecture natives

Module Rust/Svelte facultatif, désactivé à l'installation et sans service externe.
Dans **Administration → Statistiques**, seul « Activer les statistiques » est
présenté tant que le module est désactivé. Une fois activé, **Paramètres** permet
d'autoriser les utilisateurs à consulter leurs statistiques (`usersCanView`,
désactivé par défaut). La collecte et la visibilité sont indépendantes : activer
la collecte ne donne pas automatiquement accès aux bilans personnels.

Le menu personnel **Paramètres → Moi → Mes statistiques** apparaît uniquement
lorsque la collecte et la visibilité personnelle sont actives. Les routes de
lecture personnelles, détails, analyses, export et Wrapped renvoient 403 aux
utilisateurs dans les autres cas. L'administrateur conserve l'accès aux rapports
par API ; désactiver ne supprime pas l'historique. L'effacement personnel reste
possible par API. Les collecteurs de lecture continuent à suivre `enabled`.

La page se répartit en **Vue d'ensemble**, **Activité**, **Contenus**,
**Technique**, **Historique** et **Paramètres**. Les filtres avancés sont repliés.
La rubrique Paramètres regroupe visibilité, conservation, Wrapped et purge.
Le bandeau global annonçant la collecte a été retiré.

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
et temps décrivent cette sélection. Les agrégats quotidiens stockés
(`statistics_daily`) sont répartis à minuit UTC ; les analyses et le Wrapped
recalculent jours, heures, jours de semaine et mois dans le fuseau du navigateur
(paramètre `tz`). Les notes et favoris indiquent
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

## Analyses détaillées et Wrapped annuel

Les analyses (`…/statistics/insights`) et le Wrapped sont calculés dans le Core
à partir des mêmes sessions filtrées que les bilans : compte, permissions des
bibliothèques et contrôle parental actuels s'appliquent à l'identique.

- **Lecture comptée (règle JellyTrack)** : au moins **60 s** de temps consommé,
  ou un média terminé (seul cas utile pour un média de moins d'une minute), ou une
  session encore active. Les sessions plus courtes (« zapping ») restent dans les
  **lectures ouvertes** (`summary.plays`, `totals.opened`) mais jamais dans
  `summary.qualifiedPlays` ni `totals.plays`. Le temps consommé des analyses inclut
  toutes les sessions ; le Wrapped exclut entièrement les sessions de moins de 60 s.
- **Fuseau** : `tz` accepte un nom IANA (`Europe/Paris`, heure d'été comprise, base
  embarquée via `jiff`) ou un décalage en minutes à l'est d'UTC (`-840` à `840`).
  Une session est rattachée au jour, à l'heure et au mois locaux de son début.
- **Croissance** : chaque indicateur est comparé à la fenêtre précédente de même
  longueur (`growth`, en %, `null` sans base de comparaison).
- **Séries** : un épisode est regroupé sous sa série (`inventory_episodes` →
  `inventory_seasons`) ; il hérite du titre et des genres de la série quand il
  n'en a pas.
- **Complétion** : par personne et par titre, couverture cumulée de toutes ses
  sessions rapportée à la durée connue : terminé ≥ 90 %, partiel 50–90 %,
  décroché 25–50 %, abandonné < 25 %. Les « titres délaissés » listent les titres
  les plus souvent arrêtés avant 50 % (et 25 %).
- **Lectures simultanées** : nombre maximal de sessions dont les intervalles
  [début, fin ou dernier signal] se chevauchent, au global et par jour local.
- **Résolution** : meilleure hauteur disponible de l'œuvre (`max_height` : SD,
  720p, 1080p, 4K), pas la source effectivement servie.
- **Méthode et clients** : dernière méthode observée par session ; la part
  « préparée » regroupe `remux`, `separate_audio` et `prepared`. La famille de
  client (navigateur · système) est dérivée de l'appareil enregistré.
- **Personnes** : réalisateurs et six premiers interprètes des fiches éditoriales
  (corrections manuelles prioritaires) des 50 titres les plus regardés. Les
  studios ne sont pas fournis par les métadonnées actuelles.
- **Bornes** : au plus 100 000 sessions agrégées par réponse (`truncated`).

Le **Wrapped** (`/app/statistics/wrapped/`) résume une année civile locale :
heures, lectures, titres distincts (série = un titre), équivalent en jours, jours
actifs, heures par mois, heure / jour / mois favoris, plus longue série de jours
consécutifs, appareil favori, genres, films, séries, marathon (le plus d'épisodes
distincts d'une série en un jour), artistes, albums, titres, personnes, temps par
type, première et dernière lecture et comparaison avec l'année précédente.
L'année par défaut est l'année en cours, ou la précédente en janvier. Les
administrateurs règlent sa visibilité (`statistics_wrapped_settings` : activé par
défaut, fenêtre annuelle facultative pouvant passer le Nouvel An, par défaut du
1er décembre au 31 janvier) et consultent celui de chaque compte depuis son
profil, même hors fenêtre. L'interface est un récit plein écran : défilement
automatique toutes les 8 s, flèches, Espace, appui gauche/droite, maintien pour
mettre en pause, résumé copiable ; `prefers-reduced-motion` coupe les animations.

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
| `GET /api/v1/playback/statistics` | Activation, `usersCanView`, `canView` effectif pour le compte, première activation, conservation, signal 15 s et expiration 90 s. |
| `POST /api/v1/playback/sessions` | `{id: UUID client, mediaId: identifiant asset vidéo ou titre musique, music: bool}`. Création idempotente, compte et bibliothèque déduits côté Core. |
| `POST /api/v1/playback/sessions/{id}/events` | `{events:[...]}` : lot ordonné de 1 à 100 événements, corps au plus 64 Kio. |
| `GET /api/v1/admin/statistics` | Bilans globaux, classements, activité, historique et périodes. Administrateur seulement. |
| `GET /api/v1/account/statistics` | Même contrat, restreint au compte courant et à ses droits actuels. |
| `GET …/statistics/sessions/{id}?after=sequence` | Bilan et jusqu'à 2 000 événements ; `nextSequence` permet de continuer. Détail soumis aux mêmes droits. |
| `GET …/statistics/export` | CSV protégé contre les formules, mêmes filtres ; lots de 2 000 lignes, indicateur `x-export-truncated`. L'interface propose le lot suivant. |
| `GET/PUT /api/v1/admin/statistics/settings` | `{enabled:bool,usersCanView:bool,eventRetentionDays:int,summaryRetentionDays:int}`. |
| `POST /api/v1/admin/statistics/purge` | Purge bornée selon la conservation actuelle. |
| `DELETE /api/v1/account/statistics` | Suppression de tout l'historique statistique personnel. |
| `GET /api/v1/admin/statistics/insights` · `GET /api/v1/account/statistics/insights` | Mêmes filtres plus `tz` : totaux, croissance, jours de semaine, grille heure × jour, mois, calendrier, simultanéité, genres, séries, titres, personnes, complétion et titres délaissés, résolution, méthodes, clients. |
| `GET /api/v1/admin/statistics/users/{id}` | Profil d'un compte (administrateur) : analyses ci-dessus restreintes au compte, 10 dernières lectures, première et dernière lecture de la période. |
| `GET /api/v1/account/statistics/wrapped?year=&kind=all\|film\|episode\|music&tz=` | Wrapped du compte courant ; `{visible:false,settings}` hors visibilité. `availableYears` va de la première lecture à l'année en cours. |
| `GET /api/v1/admin/statistics/wrapped/{userId}?year=&kind=&tz=` | Wrapped d'un compte pour l'administrateur, quelle que soit la visibilité. |
| `GET/PUT /api/v1/admin/statistics/wrapped-settings` | `{visible,periodEnabled,startMonth,startDay,endMonth,endDay}`. |

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
[validation locale du 4 novembre 2026](archive/ci/STATISTICS_VALIDATION_2026_11_04.md).

Les tests ciblés couvrent les calculs de pause/déplacement/répétition/vitesse et
chargement, les pistes et leur origine, le même contrat HTTP sur SQLite et
PostgreSQL 18 jetable, les permissions et restrictions parentales, l'isolation des
comptes, la déduplication, la clôture après expiration, la conservation et l'effacement.
`statistics::aggregate` et `statistics::wrapped_tests` couvrent l'exclusion des
sessions de moins de 60 s, le filtre d'année et les fuseaux (minuit du Nouvel An
à Paris, heure d'été), le regroupement des épisodes par série, la série de jours,
le marathon, la complétion, la simultanéité, la visibilité du Wrapped et l'accès
administrateur ; `insights.test.ts` couvre les aides du calendrier, des écrans et
du résumé copiable.
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
les imports d'historique JellyTrack et les recommandations sont différés.
Le NAS, les fournisseurs, les appareils physiques et la charge de production ne
sont pas attestés par ces tests locaux.
