# Navigation, images et réactivité — 21 octobre 2026

> Complément du25octobre : [images multiples et rotation](ARTWORK_ROTATION.md).
> La liste expose désormais toutes les variantes ; les ajouts multiples et les
> imports fournisseurs conservent les pools. Les bornes historiques16 images et
> l'absence de collecte en ligne ci-dessous sont remplacées par ce contrat.

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

Les chargements indépendants peuvent se chevaucher ; les préparations complètes
restent séquentielles pour respecter l'unique worker du Node. Une préparation
facultative peut toujours durer jusqu’à environ deux minutes ; son état et une
annulation sont visibles. Une connexion rapide n’implique pas qu’un fichier soit
décodable par le navigateur. Les erreurs réseau de catalogue sont traduites en
message compréhensible ; le délai n’est pas simplement allongé.

## Mesures locales

Test Rust debug, SQLite jetable, 10 000 films synthétiques, utilisateur authentifié, 24 résultats, cinq lectures : médiane catalogue 369,57 → 92,21 ms (environ 75 % de réduction), recherche « 999 » 165,60 → 12,06 ms (environ 93 %). Ce test mesure la requête catalogue, pas un temps de chargement complet ni un serveur NAS.

Une requête publique de santé sur le serveur indiqué répondait HTTP 200 en 322 ms, réseau/TLS compris. Elle ne reproduit pas la recherche authentifiée en production. Aucun déploiement effectué ; les gains sur ce serveur restent à mesurer après installation.

## Présentation progressive des détails et playlists — 23 octobre 2026

- Les listes d'épisodes affichent et demandent au plus 24 épisodes à la fois.
  Le détail paginé retourne les descripteurs des saisons et leurs nombres d'épisodes
  autorisés, puis hydrate uniquement la page choisie. L'épisode suivant se charge
  séparément, y compris aux frontières de pages et de saisons. Le contrat historique
  sans paramètres conserve son détail complet pour les clients existants.
- Les playlists affichent 50 choix et 50 éléments à la fois ; leur file musicale
  et l'ordre global sont conservés. L'API limite actuellement à 500 playlists
  visibles et 1 000 éléments combinés par playlist.
- `GET /api/v1/lists/{id}?compact=true` retourne les œuvres sous forme de résumés
  `{id,title,kind}` dans `items` et `entries[].media`. Le titre tient compte des
  métadonnées éditoriales et des corrections manuelles. Les détails musicaux restent
  complets pour la lecture. Autorisations de bibliothèque et contrôle parental
  restent appliqués ; la réponse sans ce paramètre conserve son contrat historique.
  Cela supprime la construction des saisons, pistes et illustrations de chaque
  œuvre d'une playlist pendant son affichage.
- L'administration charge les diagnostics santé/OIDC et l'état des scans sans
  attendre leur résultat pour présenter les formulaires. Les requêtes initiales
  comptes/bibliothèques ne sont plus répétées immédiatement.

Contrôle navigateur synthétique : `node apps/web/scripts/test-progressive-pages.mjs`
avec Vite sur le port 5188 (surcharge `MEDIA_WEB_TEST_URL` possible). Le test injecte
10 000 éléments dans les réponses pour vérifier les bornes du DOM, la navigation
et la remise à zéro des pages. Ce n'est pas une mesure de latence réseau/NAS,
ni une preuve de lecture AV1/Opus ou de traitement serveur de 10 000 éléments.

## Ouverture progressive du catalogue — 23 octobre 2026

Après vérification de la session, la navigation et le catalogue démarrent sans
attendre les préférences. Leur requête est partagée avec le lecteur ; une panne
des préférences est signalée et ne rend plus le catalogue indisponible.
Les fiches Films/Séries et les paramètres sont importés à leur ouverture.
Les espaces des cartes sont affichés pendant l'arrivée des données ; les images
hors du premier écran gardent leur chargement différé.

L'accueil, les grilles et la recherche demandent des pages de 24 cartes avec
`count=false`. `hasMore` permet d'avancer sans compter toute la bibliothèque.
Les libellés indiquent le nombre affiché, pas un total inventé. Les retours
utilisent le cache de session borné existant, invalidé au changement de compte.

Mesure Rust debug sur SQLite jetable, 10 000 films, droits utilisateur actifs :
première page sans comptage 6,23 ms ; avec total exact 51–59 ms lors du même test.
Les pages profondes `offset=9990/10000` prennent encore environ 49–50 ms :
aucune garantie constante avec `offset`, tri et filtres arbitraires.

`node scripts/browser-progressive-loading.cjs` vérifie sur la vraie interface
Chrome 155.0.8059.86 et une API simulée que les films apparaissent avant la
réponse des préférences (retardée de 2,5 s) et celle des séries (1,8 s).
Deux requêtes bornées suffisent pour les deux rangées. Cela ne mesure ni le NAS,
ni le téléchargement d'une bibliothèque réelle de 10 000 médias.

Le scénario `browser-navigation-performance.cjs` mesure séparément l'apparition
de la fenêtre et la première image. Sur la fixture MP4 locale avec descripteur
artificiellement retardé de 300 ms : fenêtre 4,5–10 ms après clic observé dans
la page ; première image 458–945 ms. Ces mesures ne démontrent donc pas une
lecture effective en 50 ms. Les temps Playwright englobant ses clics et attentes
ne sont pas présentés comme des temps de rendu seuls.

Validation locale de cette pagination :
`cargo test --locked -p media-poc --test series_responsiveness -j 1 -- --nocapture`
réussit sur 10 000 épisodes synthétiques SQLite : première page de 24 épisodes
et réponse <64 Kio, passage inter-saisons, épisode interdit ignoré avant pagination,
fin de série et retrait de permission vérifiés. Première requête mesurée à 133,99 ms
sur cette exécution Windows ; ce résultat ne satisfait pas une cible universelle
50 ms et ne mesure ni le réseau NAS ni le décodage média.

Le parcours Core/Chrome réel
`cargo test --locked -p media-poc --test browser_series -j 1 -- --ignored --nocapture`
réussit en 31,19 s : huit images décodées, lecture des épisodes, pause/seek,
enchaînement entre saisons, révocation/restauration et isolation des comptes.
Le lancement d'épisode synthétique prend 1 001 ms lors de cette exécution.
Web : 120 tests Vitest, check Svelte/TypeScript 0/0, ESLint et build réussis.
Rust : formatage et Clippy strict workspace/all-targets réussis. Le texte de
pagination du scénario accepte une page sans total, avec absence du lien
suivant vérifiée pour une bibliothèque d'une seule page.
Les sorties du parcours séries sont conservées dans `target/series-current/` ;
les anciennes preuves datées dans `docs/evidence/` restent inchangées.

Régression réelle supplémentaire : `cargo test --locked -p media-poc --test browser_series -j 1 -- --ignored --nocapture`
réussit en 31,19 s avec Chrome 155 sous Windows, vrai Core/SQLite et fixtures légales :
8 images décodées, seek/pause, enchaînement automatique inter-saisons, seconde saison,
révocation 403/restauration et isolation des comptes. Le lancement mesuré dans ce
scénario est de 1 001 ms. L'ancien contrôle textuel « Page 1 sur 1 » a été adapté
au catalogue sans compte exact, en vérifiant la première page et l'absence de page suivante.
`cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings` réussit également.
