# PWA et téléchargements hors ligne

L’interface `/app/` dispose d’un manifeste pré-généré depuis `DEFAULT_NAME`,
de deux icônes PNG 192/512 et d’un service worker SvelteKit.
Le nom installé suit le nom de repli du build ; une personnalisation du nom
du serveur via l’API ne renomme pas l’application installée.

Sur HTTPS (ou localhost), ouvrir l’interface puis utiliser l’action
d’installation du navigateur. Les possibilités et libellés dépendent du client.
Un NAS accessible seulement par HTTP sur une adresse LAN reste utilisable dans
un onglet ; l’installation PWA nécessite une origine sécurisée.

Le worker conserve uniquement le HTML public de Téléchargements et les assets
du build dans un cache versionné. Les autres navigations utilisent le réseau et
affichent un message explicite si celui-ci échoue. Les réponses 401/403/500 ne sont
pas remplacées et les API/catalogue/affiches/médias ne sont pas mis en CacheStorage.
Aucune activation forcée du nouveau worker pendant une lecture.

L’icône de téléchargement enregistre désormais le fichier original avec le
mécanisme habituel du navigateur. Si plusieurs versions sont disponibles, elle
propose les fichiers correspondants ; sinon, le téléchargement commence en un
clic. Aucun mot de passe, aucune phrase et aucune copie supplémentaire dans
IndexedDB ne sont créés par cette action. Le nom proposé reprend le titre et
l’extension du format disponible. Le fichier se trouve dans les téléchargements
du navigateur et se lit ensuite avec le lecteur choisi sur l’appareil. Le
navigateur gère la progression, l’annulation et le disque ; l’ancienne limite de
4 Gio du stockage hors ligne de l’application ne s’applique pas à ce parcours.

La page Téléchargements de la PWA conserve la lecture des copies IndexedDB
existantes. Les copies munies d’une clé locale automatique s’ouvrent sans phrase,
même après rechargement hors connexion. Les copies à phrase restent récupérables
dans « Récupérer mes anciens téléchargements », replié et absent si aucune
ancienne copie n’existe. Aucun ancien média n’est supprimé lors de la mise à
jour. Les données du site et leurs clés restent propres au profil navigateur ;
les effacer supprime ces copies. La suppression d’une copie attend l’effacement
durable des blocs, métadonnées et clé avant de la retirer de l’interface.

Cette lecture des anciennes copies gère HEAD et Range ; une révocation sur le
serveur ne retire pas une copie déjà hors ligne. La compatibilité du fichier
original reste celle du lecteur utilisé. Détails : [PRODUCT_FEATURES.md](PRODUCT_FEATURES.md).

Validation locale : `npm run check`, `npm run lint`, `npm test`, `npm run build`,
`node scripts/test-pwa.mjs`. Ce dernier utilise Chrome headless et le build statique
avec une CSP restrictive : manifeste, dimensions PNG décodées, worker actif,
navigation hors ligne 503, cache public uniquement, événement de téléchargement
natif avec nom et octets exacts sans copie IndexedDB, lecture après reload d’une
copie locale existante, conservation et récupération d’une ancienne copie v1,
lecture Opus, pause/seek, Range exact, suppression durable et reconnexion.
L’analyseur Web Audio contrôle aussi le PCM du signal synthétique 440 Hz ; ce
n’est pas une preuve sur des haut-parleurs physiques. Ce test utilise un
serveur HTTP de test ; le service statique Core est vérifié séparément par
`cargo test --locked -p media-poc --test static_web -j 1 -- --test-threads=1`.
L’installation réelle desktop/iOS/Android et le NAS HTTPS ne sont pas validés.

Référence : [service workers SvelteKit](https://svelte.dev/docs/kit/service-workers).
