# Affiches locales Films — P1

## Complément du 23 octobre 2026

Affiches Films : après les noms historiques, `cover`, `default`, `movie`, puis
`<vidéo>-folder`, `-cover`, `-default`, `-movie`. Séries : `poster`, `folder`,
`cover`, `default`, `show`. Fonds : `backdrop`, `fanart`, `background`, `art`.
Un fond préfixé par le nom vidéo est aussi recherché lorsque le dossier permet
le partage mais ne contient aucune image générique. Les ambiguïtés entre œuvres
et entre extensions de même base continuent d'interdire le partage.
Ces alias reprennent une partie des [conventions locales Jellyfin](https://jellyfin.org/docs/general/server/media/movies/).
Un nouveau scan est nécessaire pour les découvrir dans un inventaire existant.
Aucun téléchargement depuis un fournisseur de métadonnées n'est ajouté.
Voir les mesures et limites dans [Navigation](NAVIGATION_PERFORMANCE.md).

## Chargement initial — 20 octobre 2026

L’accueil affiche chaque rayon dès que sa requête catalogue répond, puis charge
ses images sans attendre l’autre type de média. Une indication signale le rayon
encore en cours ; une panne partielle conserve les résultats disponibles.
Les six premières affiches du premier rayon et des grilles sont chargées
immédiatement avec priorité haute ; les suivantes restent différées. Le bandeau
et les cartes suivent l’URL effectivement chargée, sans réinitialisation tardive
de leur visibilité après une réponse du cache navigateur.

Le test navigateur `browser_posters` retient volontairement la réponse Séries
et exige une affiche Film décodée et visible avant de la libérer, à 390 et
1440 pixels. Ce scénario valide la suppression d’une attente côté interface.
Il ne mesure pas les cinq secondes signalées sur l’installation personnelle.
Les originaux restent servis sans réduction de dimensions ; le poids des images,
le stockage NAS et le réseau peuvent encore dominer le premier chargement.
Les contrôles session, bibliothèque et cache conditionnel restent appliqués.

Le Node détecte les affiches sans écrire dans les bibliothèques ni appeler une API
externe. Le Core conserve les associations et contrôle les accès. Aucun changement
de lecteur, miniature, conversion ou illustration autre qu’une affiche.

## Sélection

Ordre des noms : `poster`, `folder`, `<base vidéo>-poster`, `<base vidéo>`.
Pour chaque nom : `.jpg`, `.jpeg`, `.png`, `.webp`. Casse selon le système de
fichiers. La première image présente est déterminante ; invalide, elle produit
un avertissement et un repli, sans basculer silencieusement vers une autre.
`poster`/`folder` ne sont partagés que dans un sous-dossier où toutes les vidéos
ont la même identité locale, sans conflit explicite avec le dossier. Aucun
partage à la racine de bibliothèque. Les noms exacts restent propres à la vidéo.
Au niveau Work, priorité puis chemin relatif départagent plusieurs candidats.
Aucun rapprochement entre bibliothèques ou à partir des titres NFO.

## Inventaire et API

Migration additive `0006_local_posters.sql` : table `inventory_images` distincte
des assets vidéo, UUID stable par Work/racine/chemin, taille, mtime, MIME, priorité
et disponibilité. Migrations0004/0005 et identifiants du graphe vidéo inchangés.
Un scan complet réconcilie ajout/modification/disparition ; il ne supprime jamais
une œuvre pour une affiche manquante. Les images inchangées sont réutilisées
après confinement et comparaison taille/mtime. Aucun ffprobe supplémentaire.

Le catalogue et la fiche exposent `posterUrl` ou null, jamais le chemin disque.
GET/HEAD `/api/v1/assets/{id}/image` : session et permission de bibliothèque
réévaluées avant toute réponse, y compris conditionnelle/Range. Bibliothèque
désactivée, grant ou session révoqué : accès refusé. UUID inconnu refusé sans
révéler son existence ; fichier disparu/invalide/modifié hors scan :404.
URL révisée par mtime/taille et `Cache-Control: private, no-cache` (revalidation obligatoire, ETag et réponse 304 sans corps après contrôle des droits et du fichier) ; MIME réel,
`X-Content-Type-Options: nosniff`, CSP inchangée. Les erreurs gardent les chemins
privés. Une rescan actualise les données, puis une nouvelle requête catalogue
ou un rechargement de page actualise l’image ; aucun push temps réel.

## Bornes et limites

JPEG/PNG/WebP uniquement, extension cohérente avec le format décodé, maximum
10Mio compressés et4096 pixels par dimension. Décodeur image0.25.10, budget
allocation32Mio de meilleur effort ; un seul décodeur bloquant simultané, permis
conservé jusqu’à sa fin même après annulation HTTP. Validation complète avant
service puis flux ServeFile64Kio pour les originaux ; pas de chargement intégral
du film ni copie persistante des images. Le décodage de validation implique
un tampon compressé et une image décodée bornés ; ce n’est pas une garantie de
RSS globale. Les demandes attendent le permis : pas de quota agrégé ou de preuve
sous forte charge. GET et HEAD revalident le fichier, coût CPU assumé ici.

### Représentations réduites — 21 octobre 2026

La même route accepte `width=320`, `640` ou `1280`, exclusivement. Les cartes
demandent 320 pixels, les fiches 640 et les fonds 1280. La hauteur est bornée
à deux fois la largeur et les petites sources ne sont pas agrandies. JPEG
qualité 78 pour les images opaques, PNG pour préserver la transparence ; le MIME
de réponse décrit la représentation obtenue. Une petite source déjà adaptée
reste dans son format initial si sa conversion augmenterait le poids. Une
représentation dépassant 2 Mio est réduite davantage ; sinon elle est refusée.
Le Web peut revenir à l’original après un échec de représentation.

Un cache LRU en mémoire conserve au plus 128 représentations et 8 Mio de corps
compressés, indexés par chemin confiné, taille, date de modification et largeur.
Les corps sont partagés sans recopie ; ce budget ne borne pas le RSS du processus
ni les réponses encore en cours. Les droits et l’état du fichier sont contrôlés
avant chaque accès, y compris un succès du cache. Le décodeur est partagé avec
les scans et la validation des originaux : une opération de décodage à la fois.
Les médias et les répertoires de bibliothèque restent en lecture seule.

GET, HEAD, Range, ETag et 304 sont couverts pour ces représentations. La largeur
est aussi transmise aux Nodes distants, y compris par canal sortant. La couche
de relais distante actuelle ne transmet pas l’ETag de réponse : son bénéfice de
revalidation HTTP n’est donc pas établi pour les images distantes.

Le fond décoratif statique de presque 2 Mo est remplacé par un dégradé CSS pour
éviter son téléchargement dès l’état de chargement initial. Les mesures d’une affiche synthétique et les
tests HTTP/navigateur figurent dans les preuves du 21 octobre ; ils ne constituent
pas une mesure sur la médiathèque ou le NAS de l’utilisateur.

Liens symboliques et points de réanalyse/jonctions refusés à découverte et lecture.
Confinement, validation et ouverture du flux ne sont pas atomiques : TOCTOU
résiduel face à un écrivain OS hostile, ACL opérateur requises. Taille/mtime ne
sont pas un hash cryptographique. Une révocation bloque la prochaine requête,
pas un corps déjà ouvert ni les pixels déjà affichés par le navigateur.

## Web et preuves

Le composant Poster existant couvre Accueil, Films, recherche et fiches : ratio
2:3, object-fit cover, cartes lazy/decoding async, fiche eager, repli discret
si absent ou erreur. Les originaux restent en lecture seule. Les tests utilisent
des copies synthétiques jetables, avec mutations explicites des fixtures pour
simuler les changements de l’opérateur. Résultats exécutés : MEMORY.md et TEST_PLAN.md.

Fichiers principaux : `node/images.rs`, `node/scanner.rs`, `node.rs`,
`inventory.rs`, `permissions.rs`, `lib.rs`, `scans.rs`, migration0006,
`Cargo.toml`/`Cargo.lock`, `Poster.svelte`, `Catalog.svelte`, `catalog/api.ts`.
Tests : `posters_flow.rs`, `browser_posters.rs`, `browser-posters.cjs`, extensions
unitaires inventaire/scanner/Node ; preuves dans `evidence/2026-10-16/posters/`.
Architecture/API/sécurité/scanner/NFO/spécification/plan de tests et mémoire
signalent ce jalon ; les jalons historiques sont conservés.

Voir [Fiches, saisons, visuels et préférences privées](CATALOGUE_PRESENTATION.md) pour les extensions du 19 octobre 2026.
