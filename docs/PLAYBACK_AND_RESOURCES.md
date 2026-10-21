# Démarrage de lecture et ressources — 21 octobre 2026

## Attentes supprimées côté Web

L’import du lecteur utilise désormais une promesse commune. Deux demandes
concurrentes attendent le même import au lieu de continuer avant le montage du
lecteur. La fiche s’affiche sans attendre cet import ; au clic, import et
descripteur autorisé sont chargés en parallèle, puis le lecteur est monté avant
son ouverture. Le descripteur est toujours revalidé et les requêtes médias
restent soumises aux droits vivants côté serveur.

Un épisode qui ne possède qu’une version n’effectue plus un premier chargement
de descripteur pour choisir sa version avant de la revalider une seconde fois.
Pour plusieurs versions, le choix automatique conserve ses informations de
compatibilité et de qualité. Une déclaration navigateur reste un indice, pas
une preuve de décodage.

L’approche du bouton Lecture à la souris ou au clavier prépare la progression
pendant au plus cinq secondes. Ce résultat est consommé une seule fois, limité
à 32 œuvres, supprimé au changement de session et avant toute écriture. Un autre
appareil peut modifier la progression pendant cette courte fenêtre ; le conflit
de révision serveur empêche toujours son écrasement. Aucune reprise tardive ne
déplace la vidéo une fois la lecture lancée. La vidéo en mode immersif peut
précharger ses données pendant la préparation de la reprise ; aucun fichier
n’est chargé avant ouverture du lecteur.

Les contrôles Version disparaissent pour un épisode ayant une seule version.
Le menu Audio apparaît seulement pour plusieurs identifiants de sources audio
opérationnelles ; les pistes simplement détectées dans le conteneur ne sont pas
comptées. Les informations techniques vidéo restent consultables, sans liste de
versions lorsqu’il n’y a qu’un choix. Les décalages d’une piste externe unique
restent réglables dans les préférences de lecture générales.

Les fiches utilisent également les représentations d’images bornées du Node :
1280 px pour le fond, 640 px pour les logos/galeries et 320 px pour saisons et
épisodes. Les sources externes ne sont pas réécrites.

## Docker et mémoire : frontière de mesure

Le moteur Docker Desktop n’est pas disponible dans cet environnement (pipe
`dockerDesktopLinuxEngine` absent). Les 228 Mo d’image et les 256 Mo après cinq
minutes signalés ne sont donc pas reproduits ni comparés ici. Aucun conteneur,
volume ou réseau utilisateur n’a été modifié.

Les runtimes Debian fixent `MALLOC_ARENA_MAX=2`. La glibc peut conserver des
arènes distinctes pour les allocations des travailleurs ; sa limite par défaut
dépend du nombre de processeurs. Borner ce nombre vise à limiter la rétention
après scan/décodage/hachage. Ce réglage ne réduit ni le budget Argon2, ni les
limites média, ni un quota RSS : le gain et l’effet sur la contention doivent
être mesurés sous Docker. L’opérateur peut remplacer cette variable au lancement.
Référence : [paramètres de malloc, manuel GNU libc](https://sourceware.org/glibc/manual/2.35/html_node/Malloc-Tunable-Parameters.html).

L’image conserve les commandes de récupération/sauvegarde, le client PostgreSQL
optionnel et les capacités AV1/Opus. Le runtime standalone ne copie plus une
seconde implémentation complète du Core sous le nom `media-poc` : les commandes
historiques `media-poc` et `aether` sont des liens vers `aether-core`, configuré
avec son Node local. Le binaire expérimental de fixtures reste disponible dans
les builds depuis les sources, sans être nécessaire dans l’image de production.
Cela retire un exécutable du contenu final ; aucun gain chiffré de taille
compressée ou de RSS n’est revendiqué sans build Docker réel. Une comparaison
des couches et de la mémoire en environnement Linux reste nécessaire.

## Vérifications ciblées

Les tests de progression couvrent la consommation unique, l’expiration, les
écritures et les changements de compte. Les tests audio distinguent sources et
pistes détectées, dédupliquent un même identifiant et excluent les Nodes révoqués
ou déconnectés. Le script navigateur `browser-series.cjs` vérifie que l’épisode
S01E02 mono-version n’affiche pas de sélecteur Version et que son original et sa
préparation d’une même piste ne font pas apparaître le bouton Audio. L’épisode
S01E01 conserve son sélecteur multi-version. Le code d’assertion doit être
exécuté par le jalon principal avant d’en annoncer le résultat. Les mesures
locales ne constituent pas une comparaison avec Jellyfin sur le NAS.
