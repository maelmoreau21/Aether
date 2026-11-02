# Préférences utilisateur persistantes

Le Core conserve les préférences d'interface et de lecture dans la base, par
identité authentifiée. Elles sont donc incluses dans les sauvegardes SQLite et
PostgreSQL et reviennent avec la base restaurée. Aucun token d'authentification
n'est écrit dans le stockage navigateur.

## Contrat

`GET /api/v1/preferences` renvoie l'objet du compte connecté, ou `{}` avant son
premier réglage. `PUT /api/v1/preferences` fusionne les champs fournis avec ses
préférences existantes et renvoie 204. Le Core déduit l'identité de la session :
aucun paramètre `userId` ni champ arbitraire n'est accepté, même pour un
administrateur. La mutation requiert origine et CSRF valides.

Champs autorisés : `theme` (`light`, `dark`, `system`), `locale` (`fr`, `en`),
`audioLanguage` (code de langue borné), `subtitles` (`forced`, `always`, `off`),
`audioTrack` (`mediaId`, `trackId`, chacun au plus 256 octets), `offsets`
(`audio`, `subtitle`, chacun de −30 à +30 secondes) et `subtitlePreference`
(`enabled`, `language`, `forced`, `sdh`). Les propriétés imbriquées inconnues
sont refusées. Le corps est limité à 8 Kio et le document enregistré à 4 Kio.

Les choix de pistes sont des préférences, jamais des droits d'accès : le lecteur
et le Core continuent de vérifier disponibilité et permissions de la ressource.
Les langues/valeurs non prises en charge sont refusées, pas transformées en
commandes ni en chemins de disque.

## Commandes de lecture (2 novembre 2026)

### Nouveautés de l'accueil

`hidePlayedInLatest` masque les œuvres explicitement marquées vues dans les
requêtes `GET /api/v1/catalog?scope=latest`. Valeur absente : désactivé. L'accueil
utilise ce périmètre et le tri `added-desc` ; les pages Films, Séries et Recherche
restent complètes. Le Core lit la préférence du compte authentifié et applique
le filtre SQL avant total, sélection des copies regroupées et pagination.
Les séries ne sont pas calculées automatiquement « vues » à partir de tous leurs
épisodes dans ce lot ; leur statut explicite reste la référence.

Les pages filtrées ne réutilisent pas le cache Core basé uniquement sur les
changements d'inventaire. Le cache navigateur invalide seulement les nouveautés
après changement de progression ou sauvegarde réussie du réglage ; les autres
pages sont conservées. La limite de page et le mode sans comptage restent actifs.

`autoplayNextEpisode` (booléen, défaut `false`) conserve le choix d’enchaîner
l’épisode suivant entre les séances et les appareils du compte. Le réglage est
accessible dans Lecture et dans le lecteur lorsqu’un épisode suivant existe.
L’enchaînement intervient à la fin du média, reste soumis au parcours autorisé
de l’épisode suivant, et reste désactivé pour une séance SyncPlay.

`skipBackSeconds` et `skipForwardSeconds` acceptent indépendamment 5, 10, 15,
30 ou 60 secondes (défaut 10). Les boutons et les flèches gauche/droite du
lecteur vidéo utilisent ces durées, bornées entre zéro et la durée du média.
Les anciens comptes gardent les boutons de dix secondes ; les flèches, auparavant
fixées à cinq secondes, suivent désormais la même préférence. Les données restent
dans les préférences du compte, sans nouvelle clé globale du navigateur.

Référence fonctionnelle consultée : `jellyfin-web/src/components/playbackSettings/`
(`EnableNextEpisodeAutoPlay`, durées avant/arrière). Aucun code Jellyfin repris.

## Mémorisation des pistes et accueil (2 novembre 2026)

`rememberAudioSelections` et `rememberSubtitleSelections` sont des booléens,
actifs par défaut. Désactivés, les anciennes sélections sont ignorées au prochain
épisode ou à la prochaine ouverture et les nouveaux choix du lecteur ne sont pas
enregistrés. Les choix manuels restent actifs pendant la séance, y compris après
un changement de version. Désactiver la mémorisation ne supprime pas les anciennes
valeurs : les réactiver permet de les retrouver.

Les choix audio enregistrent `audioTrack` avec `mediaId`, `trackId` et une langue
optionnelle `language`, validée comme `audioLanguage`. Un choix manuel ne change
plus la langue configurée par le compte. Les anciennes données sans `language`
restent acceptées. Le choix précis est restauré pour le même média ; ailleurs,
sa langue est préférée si la mémorisation est active. Les sous-titres mémorisent
la langue, les drapeaux forced/SDH et l’arrêt explicite dans `subtitlePreference`.
Ce champ, auparavant sans raccord effectif dans le lecteur immersif, est désormais
chargé et sauvegardé après sélection réussie.

`playDefaultAudioTrack` est un booléen, inactif par défaut pour conserver la
préférence française d’Aether. Activé, il choisit une piste concrète dont le
descripteur déclare `disposition.default=1` avant de chercher la langue configurée.
Un choix précis mémorisé pour le média ou un choix manuel de la séance reste
prioritaire. Sans piste par défaut identifiable, la stratégie linguistique
existante s’applique ; aucune capacité audio du navigateur n’est inventée.

`hidePlayedInLatest`, booléen inactif par défaut, se règle dans Affichage → Accueil.
L’enregistrement réussi invalide seulement le cache des nouveautés. Le filtrage
réel côté catalogue est décrit dans le contrat de l’accueil. Ces quatre réglages
s’inspirent de `UserConfiguration.cs` et `playbackSettings` Jellyfin ; aucune
source Jellyfin n’a été modifiée ou copiée.

## Chargement et changements de compte

Le layout vérifie la session, charge les préférences, applique thème/langue,
puis monte les vues. Une génération de session protège les réponses tardives et
les écritures en attente contre les changements de compte. Les mises à jour
navigateur sont sérialisées ; un échec d'enregistrement serveur est affiché.

Les anciennes clés locales restent des miroirs de compatibilité, et les choix
de lecture sont mis en cache sous l'identifiant du compte. Le serveur constitue
la source au chargement : les anciennes valeurs globales d'un navigateur partagé
ne sont pas automatiquement importées dans un autre compte. Les anciennes clés
de lecture portant exactement l'identifiant du compte authentifié sont validées
et migrées uniquement pour les champs encore absents du serveur. Elles sont
préservées tant que l'enregistrement CSRF n'a pas réussi ; un échec bloque le
chargement avec un message explicite permettant de réessayer. Thème/langue et
préférences audio/sous-titres globales historiques ne sont jamais attribués
automatiquement à un compte.

Une sauvegarde capture les réglages effectivement enregistrés au moment de sa
création ; une modification dont la requête est encore en cours ou a échoué n'y
figure pas. Les préférences système du navigateur ne sont pas des paramètres
serveur : `system` conserve ce choix, puis suit le système du client courant.

Tests dédiés : isolation entre utilisateurs, session/CSRF, bornes et champs
inconnus, fusion partielle et réouverture de base ; côté Web, refus d'importer un
cache hérité, réponses tardives et abandon d'écritures après changement de compte.
La validation PostgreSQL réelle et navigateur est consignée séparément par le
jalon principal.

## Interface des paramètres (19 octobre 2026)

Navigation Compte / Apparence / Lecture / Connexion SSO. Les fragments
`#account`, `#appearance`, `#playback`, `#sso` permettent les accès directs et le
retour navigateur. Lecture permet d’enregistrer les deux décalages de −30 à
+30 secondes via `offsets`, sans changer le contrat Core ni créer de nouveaux
champs. Les erreurs de synchronisation restent affichées par le layout ; aucune
bannière permanente ne prétend qu’une mutation a réussi.

## Extension Général / Réseau / profil / actualisation (19 octobre 2026)

Voir [contrats et limites](SETTINGS_AND_REFRESH.md) pour les paramètres persistants du serveur,
la configuration réseau effective, les avatars de compte et les quatre modes de scan.
Les fournisseurs de métadonnées en ligne restent à développer.
