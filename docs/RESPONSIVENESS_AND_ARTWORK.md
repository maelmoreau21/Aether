# Navigation, images et réactivité — 21 octobre 2026

La navigation affiche les bibliothèques actives autorisées par le Core, avec leur nom réel. Accueil, listes et téléchargements restent accessibles. Le filtre de bibliothèque est appliqué avant la pagination, y compris à la musique et aux représentants des groupes.

## Contrat

- `GET /api/v1/libraries` : session requise, `{libraries:[{id,name,kind}]}` ; aucun chemin disque ni droit d’un autre compte. Les nœuds révoqués et bibliothèques désactivées sont exclus.
- `GET /api/v1/catalog?library=ID` : bibliothèque autorisée requise, sinon 403. `GET /api/v1/music?library=ID` restreint les résultats aux droits existants.
- `GET /api/v1/works/{work}/images` : `{images:[{id,workId,kind,position,url,origin,width,height}]}` ; images effectives, plusieurs arrière-plans possibles. Les dimensions locales non connues sont nulles.
- `POST /api/v1/works/{work}/images/{kind}?replace=ID` : corps binaire PNG/JPEG/WebP, 2 Mio maximum, dimensions 4096 px maximum, administrateur et CSRF. Types : poster, thumb, logo, illustration, banner, disc, landscape, backdrop. Maximum 16 images personnalisées par œuvre. Remplacement facultatif de même type.
- `GET/HEAD /api/v1/works/{work}/images/{image}` : image privée, `width=320|640|1280` ou original (0). Contrôle d’accès avant toute réponse 304. Image absente : 404 ; largeur invalide : 400.
- `DELETE /api/v1/works/{work}/images/{image}` : administrateur et CSRF ; retire les données applicatives ou masque une image locale, sans supprimer le fichier média.
- `PUT /api/v1/works/{work}/images/order` : `{ids:[...]}`, ordre des arrière-plans existants (32 identifiants uniques maximum), administrateur et CSRF.

L’ancien endpoint d’affiche accepte également les trois largeurs. Ses aperçus sont conservés en base par révision. Les nouvelles images produisent leurs aperçus lors de l’import. Deux décodeurs au maximum, attente bornée à huit secondes ; une saturation répond 503. Les lectures privées utilisent `private, no-cache` avec ETag et nouvelle autorisation lors de la revalidation.

L’éditeur partagé affiche des cartes par type, dimensions originales des imports, agrandissement, remplacement, suppression et ordre des fonds. Les dimensions d’images locales sont celles de l’aperçu affiché. Les bibliothèques conservent leur contrat existant limité à affiche/fond. Aucun fournisseur de recherche d’images en ligne n’est ajouté.

## Réactivité

Le catalogue évite le parcours de toutes les œuvres pour déterminer les représentants de groupes et regroupe les lectures d’images et groupes par page. Des index couvrent les groupes et le tri des titres. Les bibliothèques du menu sont mutualisées en mémoire durant 30 secondes et invalidées au changement de session ; le serveur reste la source des droits.

Chaque lancement revérifie le descripteur sur le serveur, même s’il était en cache ; les octets restent également protégés par les autorisations du Core/Node. La reprise attend au maximum 2,5 secondes ; en cas d’échec, une notice prévient et la progression distante inconnue n’est pas écrasée. Une première connexion directe sans métadonnées bascule vers le relais après 2,5 secondes, puis attend au maximum 7,5 secondes. Le Core borne la connexion Node et l’attente des en-têtes, sans imposer cette limite à la durée du film.

Les préparations audio/vidéo indépendantes démarrent en parallèle. Une préparation facultative peut toujours durer jusqu’à environ deux minutes ; son état et une annulation sont maintenant visibles. Une connexion rapide n’implique pas qu’un fichier soit décodable par le navigateur. Les erreurs réseau de catalogue sont traduites en message compréhensible ; le délai n’est pas simplement allongé.

## Mesures locales

Test Rust debug, SQLite jetable, 10 000 films synthétiques, utilisateur authentifié, 24 résultats, cinq lectures : médiane catalogue 369,57 → 92,21 ms (environ 75 % de réduction), recherche « 999 » 165,60 → 12,06 ms (environ 93 %). Ce test mesure la requête catalogue, pas un temps de chargement complet ni un serveur NAS.

Une requête publique de santé sur le serveur indiqué répondait HTTP 200 en 322 ms, réseau/TLS compris. Elle ne reproduit pas la recherche authentifiée en production. Aucun déploiement effectué ; les gains sur ce serveur restent à mesurer après installation.
