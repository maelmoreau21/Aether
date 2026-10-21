# Réglages de bibliothèque et permissions médias

## Périmètre réalisé le 21 octobre 2026

Ce lot ne réalise pas toute la liste de fonctions Jellyfin demandée. Il ajoute
un parcours de configuration locale, de personnalisation des cartes et de
restriction des utilisateurs, avec migrations SQLite et PostgreSQL 0037.
Les originaux, les dossiers Jellyfin et leurs fichiers annexes restent en lecture seule.

### Bibliothèques

`GET /api/v1/admin/libraries` retourne désormais `options`, `coverUrls`,
`imageKinds` et `imageVersions`. POST/PUT acceptent un objet `options` :

```json
{
  "preferredMetadataLanguage": "fr",
  "metadataCountryCode": "FR",
  "readLocalNfo": true,
  "fetchPoster": true,
  "fetchBackdrop": true,
  "fetchLogo": true,
  "fetchThumb": true,
  "fetchBanner": true,
  "fetchDisc": true,
  "automaticRefreshIntervalDays": 0
}
```

Les deux préférences linguistiques sont persistées, mais aucun fournisseur
en ligne ne les consomme encore dans ce parcours. Elles ne traduisent pas un NFO.
Le type de contenu conserve le contrat `kind: films | series | music` ; la musique
utilise les tags intégrés et ne déclenche pas de scraping externe.

Les options d'import NFO et d'images sont appliquées par le Core au staging local
et distant, puis à la publication. Le Node peut encore lire/valider un fichier
annexe désactivé, mais le Core ne l'importe pas. Les corrections manuelles et
affiches téléversées restent indépendantes du scan. Une modification des options
demande un nouveau scan pour actualiser les données déjà indexées.
Les types locaux banner, disc/discart et thumb sont découverts avec les règles
existantes de confinement et d'association non ambiguë.

L'intervalle autorisé est 0, 30, 60 ou 90 jours. Zéro désactive l'actualisation.
Le prochain lancement est persistant, et utilise la file de scans existante,
sans multiplier les workers ni les analyses techniques inchangées. Les scans
planifiés manuellement sont prioritaires. Une actualisation locale relit les
NFO/images ; elle n'appelle aucun fournisseur externe. Les bibliothèques musique
et les fixtures sont exclues de cette actualisation de métadonnées.
Une erreur de lancement est signalée au journal du scheduler ; le prochain
essai reste à l'échéance suivante, sans boucle de rattrapage.

Les anciens clients qui omettent `options` lors d'un PUT conservent les options
existantes. Champs inconnus et valeurs invalides sont refusés (400).
La modification d'une bibliothèque en cours de scan est refusée (409).

### Images et cartes

`GET/PUT/DELETE /api/v1/admin/libraries/{id}/images/{kind}` : `poster` ou `backdrop`.
Session administrateur requise, CSRF pour les mutations ; 404 pour un identifiant
absent, 400 pour un type inconnu. PUT reçoit les octets PNG/JPEG/WebP et le
Content-Type approprié, maximum 2 Mio. Les images sont décodées avec limites,
réencodées PNG et réduites proportionnellement à 1000 × 1500 au maximum.
Elles sont conservées en base applicative. Deux workers de décodage maximum,
429 si occupés, 422 pour un format ou des dimensions non acceptés.
GET/HEAD est privé et non cachable. Une révision opaque renouvelle le visuel
de la carte après remplacement.

Sans personnalisation, la carte d'administration assemble jusqu'à quatre
affiches réellement disponibles, en biais. Il s'agit d'un assemblage CSS des
images protégées, pas d'un fichier généré sur disque. Le menu contient le scan
direct, la modale des modes existants de rafraîchissement et l'éditeur d'images.
Pendant le scan, la progression est indéterminée et accompagnée des compteurs
réels ; aucun pourcentage n'est inventé.

### Téléchargements

`GET/PUT /api/v1/admin/users/{id}/media-policy` accepte
`{"allowMediaDownload":true}`. GET personnel : `/api/v1/me/media-policy`.
Le comportement historique reste autorisé lorsqu'aucune règle n'existe.
Seul un administrateur avec CSRF peut modifier le droit ; inconnu = 404.

Les téléchargements vidéo utilisent désormais
`GET/HEAD /api/v1/assets/{id}/download` ; musique :
`GET/HEAD /api/v1/music/tracks/{id}/download`.
Ils reprennent les contrôles de session, bibliothèque, classification et Range
du contenu original, et ajoutent le contrôle vivant du droit de téléchargement.
Le Core relaie les originaux distants pour cette action : un lien Node signé
ne permet pas de contourner la permission de téléchargement de l'application.
Les nouvelles copies chiffrées du coffre utilisent aussi cette route.

La route `/content` reste lisible pour le lecteur. Un client qui reçoit les
octets peut les enregistrer : ce réglage n'est pas un DRM. Les copies déjà
présentes sur un appareil et les réponses déjà ouvertes ne sont pas révoquées.

### Contrôle parental

`GET/PUT/DELETE /api/v1/admin/users/{id}/parental` conserve `maximumAge`
et `allowUnrated` et ajoute `blockedTags` et `accessSchedule` :

```json
{
  "maximumAge": 12,
  "allowUnrated": false,
  "blockedTags": ["horreur"],
  "accessSchedule": [
    {"day": 0, "startMinute": 480, "endMinute": 1200}
  ]
}
```

Jours lundi=0 à dimanche=6, minutes UTC. Début inclus, fin exclue, 1440 = fin
de journée. Une liste vide autorise toute la semaine ; sinon seules les plages
indiquées sont autorisées. Une plage traversant minuit doit être scindée.
Maximum 56 plages ; début strictement inférieur à fin.
Les horaires sont relus à chaque authentification et validation d'un lien Node,
ainsi que pour le catalogue par clé API. Les administrateurs conservent leur
accès de récupération. Une réponse vidéo ouverte n'est pas coupée à l'échéance.

Maximum 64 tags de 100 octets, correspondance exacte après normalisation de
casse. Les tags viennent des éléments NFO `<tag>` ou de l'éditeur manuel.
Les tags de la série s'appliquent aussi à ses épisodes. Le filtre s'applique
avant pagination et aux ressources dérivées, en GET/HEAD/Range.
Les formes françaises `-10/-12/-16/-18`, `FR-10/FR-12/FR-16/FR-18` et
`Tous publics` sont reconnues, en plus des classifications déjà gérées.
Ce n'est pas un dictionnaire exhaustif de tous les systèmes de classification.

## Partie de la demande restant à réaliser

- Plusieurs racines/Nodes dans une bibliothèque logique : évolution des identités,
  sources, droits, scans et résolution des ressources encore nécessaire.
- Fournisseurs de métadonnées/images activables et ordonnés par bibliothèque :
  les extensions existantes ne sont pas encore orchestrées par cette configuration.
- Sauvegarde NFO et artwork dans les médias, suppression physique et téléversement
  de sous-titres : aucun droit d'écriture accordé par ce lot, aucune route ajoutée.
- Limite réseau distante et écrans simultanés : aucun quota appliqué par ce lot.
- Nombre et largeur minimum des fonds, surveillance native temps réel, extraction
  d'images de chapitres/trickplay et détection des génériques : non implémentés.
- Fusion automatique des versions et collections automatiques : les regroupements
  explicites existants sont conservés ; aucune heuristique ajoutée.
- Sous-titres automatiques, langues, priorité OpenSubtitles/SubDL, audio correspondant
  et hash strict : non implémentés.
- Case de remplacement des images et bannière générée utilisable dans tout le
  catalogue : non implémentées. La mosaïque actuelle concerne les cartes admin.

Ces capacités ne sont pas présentées comme activables. Ce document distingue
les préférences stockées, les traitements exécutés et le reste de la demande.
