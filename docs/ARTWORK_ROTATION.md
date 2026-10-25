# Images multiples et rotation par session — 25 octobre 2026

## Décision

La conservation des variantes, leurs représentations privées, la déduplication et
la sélection d'affichage sont natives. Les fournisseurs restent des extensions
Wasm facultatives et activées explicitement. Il n'est pas nécessaire d'installer
un autre plugin de rotation. Le Core ne contient pas de table de fournisseurs ni
de clés intégrées. Aucun changement du lecteur AV1/Opus ou des médias en lecture
seule n'est requis.

L'audit du [Poster Rotator Jellyfin, commit 6640be2](https://github.com/maelmoreau21/jellyfin-plugin-poster-rotator/tree/6640be2dd685026749d990d991b90e155879fb3b)
a servi de référence fonctionnelle. Son remplacement quotidien de l'image
globale devient une sélection privée à la session de navigation. Son
[ImageHash.cs](https://github.com/maelmoreau21/jellyfin-plugin-poster-rotator/blob/6640be2dd685026749d990d991b90e155879fb3b/src/Jellyfin.Plugin.PosterRotator/Helpers/ImageHash.cs#L116)
échantillonne des octets compressés ; Aether calcule une empreinte sur les pixels
décodés. Aucun code C# n'a été transplanté et le dépôt Jellyfin n'est pas modifié.

## Parcours livré

Dans **Modifier les images → Enrichir les images**, l'administrateur choisit
jusqu'à cinq langues dans l'ordre, les images neutres et lance la collecte.
L'œuvre doit posséder des identifiants externes connus, éventuellement corrigés
manuellement. Aucun résultat de recherche par titre n'est adopté automatiquement.
Seuls les fournisseurs installés, actifs et autorisés pour la bibliothèque sont
appelés ; une erreur d'un fournisseur est isolée des autres.

Les extensions TMDb et fanart.tv du dépôt voisin `Aether-Plugins` ont été adaptées :
logos TMDb, variantes préférées/de secours/neutres, conservation des tags
régionaux, résolution des identifiants externes et distinction TVDB pour les
séries fanart.tv. Ces sources doivent être reconstruites et empaquetées/signées
pour mettre à jour une installation existante ; les anciens paquets distribués
n'ont pas été remplacés par cette intervention.
Leur sélection répartit les réponses trop volumineuses entre rôles et langues,
avec un plafond de200 images et60Kio sérialisés, sous la limite de64Kio du runtime.

Le catalogue conserve `artwork`, `posterUrl` et `thumbUrl` pour les clients
existants, et ajoute `imageVariants`, des descripteurs sans octets d'image.
L'accueil, les cartes, les fiches film/série, les épisodes et la reprise utilisent
la sélection commune. Les rôles disponibles sont poster, backdrop, logo,
illustration, thumb, banner, disc et landscape ; seuls les emplacements adaptés
au rôle consomment l'image. Une image ajoutée manuellement, y compris l'ancien
poster personnalisé, reste prioritaire. Les pools téléchargés sont conservés
lors de l'ajout d'un remplacement manuel.

Les choix sont stables pendant la navigation et les rechargements. Lors d'une
nouvelle session de navigation, le choix précédent est exclu lorsqu'au moins
deux candidats correspondent à la langue retenue. L'historique est borné à
512 couples œuvre/type par compte dans le navigateur ; aucun jeton brut n'est
persisté par ce mécanisme. Cet historique est local au navigateur, sans garantie
de synchronisation entre appareils. Une seule variante disponible reste stable.

## API et bornes

`POST /api/v1/admin/extensions/images/collect` exige administrateur, session et
CSRF. Corps :

```json
{"workId":"ID","languages":["fr-CA","en"],"includeNeutral":true,"maxImages":12}
```

Réponse : `providers`, `downloaded` (nouveaux imports acceptés), `duplicates`
(similarités détectées après transfert), `skipped` (URL connues ou pool plein),
`attempts`, `errors` (fournisseur/statut HTTP uniquement) et `partial`.
`partial:true` invite à relancer explicitement ; les imports acquis restent
persistés. Maximum 64 extensions examinées, 200 images par réponse fournisseur,
24 tentatives d'image et 12 nouveaux imports par appel ; budget global 55 s,
15 s par découverte fournisseur. Aucun appel ne promet une collecte exhaustive.
Une erreur de langue/limite répond 400, absence d'œuvre 404, identifiants
externes absents 422 ; session/CSRF/droits invalides restent refusés.

La route d'import d'une extension accepte les champs additionnels `append` et
`language`. Le champ `provider` est fixé par l'hôte et ne peut pas être fourni
dans une requête d'upload. `metadata.images` accepte `id:""` uniquement si les
identifiants externes de l'œuvre existent ; l'extension décide lesquels elle
sait résoudre. Les détails de métadonnées continuent d'exiger un candidat.

Les imports réutilisent le téléchargement HTTPS existant : hôte du manifeste
signé, port443, DNS public vérifié et épinglé, sans redirection, proxy ni secret
transmis au serveur d'image ; 2Mio compressés, JPEG/PNG/WebP et dimensions4096px.
Les représentations originales et320/640/1280px restent protégées par les routes
autorisées avant toute réponse conditionnelle. Maximum64 images applicatives
par œuvre et12 par type pour les ajouts multiples. Deux décodeurs simultanés au
maximum et budget de décodage existant conservés.

Les migrations additives0045 SQLite/PostgreSQL conservent langue, fournisseur,
empreinte et hash d'URL. Les alias d'URL d'un doublon sont persistés et supprimés
avec l'image correspondante. Aucune URL fournisseur complète n'est ajoutée à
ces tables. L'empreinte combine dHash9×8 en luminance, couleur4×4 avec alpha et
rapport de dimensions. La comparaison est limitée à une œuvre, un rôle et une
langue, entre fournisseurs. C'est une heuristique : des images très proches
peuvent être conservées ou rejetées à tort. Les anciens imports sans empreinte
et les originaux locaux ne sont pas rétroactivement décodés pour cette étape.

## Limites et prochain jalon

La collecte est une action explicite par œuvre. Aucun balayage automatique de
toute la médiathèque, quota disque administrable ou tâche planifiée de collecte
distante n'est annoncé. La catégorie `image` du moteur distribué existant
prépare des images locales inventoriées ; elle ne constitue pas un contrat
d'exécution des fournisseurs de métadonnées. Le prochain jalon est un job de
collecte durable reprenable, avec budgets par bibliothèque et fournisseur.

Les vérifications locales utilisent des images synthétiques et des fournisseurs
simulés. Elles ne prouvent pas un téléchargement avec des clés réelles, une
validation PostgreSQL exécutée ou un déploiement NAS. Les paquets installés et
les services en cours d'exécution ne sont pas mis à jour automatiquement.

## Vérifications exécutées

- `cargo test --locked -p media-poc --lib artwork::tests -j 1 -- --nocapture` :
  2 réussis, pixels redimensionnés/JPEG, alpha, proportions, couleurs uniformes,
  représentations et formats invalides.
- `cargo test --locked -p media-poc --lib image_collection -j 1 -- --nocapture` :
  2 réussis, ordre exact/base/secours/neutre et protection de la configuration privée.
- `cargo test --locked -p media-poc --test artwork_variants -j 1 -- --nocapture` :
  1 réussi avec migrations SQLite, vraie authentification et contrats d'erreur.
- `cargo test --locked -p media-poc --test extensions image_collection_uses_only_enabled_providers_and_skips_known_sources -j 1 -- --nocapture` :
  1 réussi avec paquet signé synthétique, vrai worker Wasm, désactivation et zéro
  tentative de transfert pour une URL connue.
- Binaires de tests fraîchement construits exécutés avec les filtres
  `extensions::adapters::tests` et `rich_operations_and_account_avatars_use_signed_permissions` :
  respectivement2 et1 réussis ; aucun changement du contrat métadonnées/avatars.
- `cargo clippy --locked -p media-poc --lib --tests -j 1 -- -D warnings` et
  `cargo fmt --all -- --check` : réussis.
- Web : tests ciblés de sélection/URL, ESLint ciblé, `npm run check` et
  `npm run build` réussis. Svelte : zéro erreur, six avertissements préexistants.
- `node scripts/test-artwork-rotation.mjs` contre le serveur Web local : Chrome
  155.0.8059.86 Windows, API simulée, images décodées poster/backdrop/logo/illustration,
  stabilité au reload, changement à la reconnexion et dans un nouvel onglet avec
  la même authentification, collecte/CSRF/retour partiel et formats1440/390px.
  Rapport et captures : `target/artwork-rotation-ui/`. Dernier passage contre le
  build de production avec `vite preview`, libellés langue/provenance inclus.
- Dépôt `Aether-Plugins` : `cargo test -p aether-plugin-api -p aether-plugin-tmdb -p aether-plugin-fanart`
  et Clippy ciblé `--lib --tests -- -D warnings` réussis :20 tests, dont un jeu
  de1050 images vérifiant la répartition des rôles/langues et la borne JSON.
  Les fournisseurs HTTP y sont simulés localement. README vérifié UTF-8 strict.
