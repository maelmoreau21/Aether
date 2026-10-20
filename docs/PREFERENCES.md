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
