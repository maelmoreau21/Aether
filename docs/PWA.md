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

Les téléchargements explicites sont chiffrés par blocs dans IndexedDB, avec une
phrase de déverrouillage qui ne quitte pas le navigateur. La page Téléchargements
reste accessible sans serveur, exige la phrase pour lire, et permet la suppression
locale. Les clés restent en mémoire pendant cette visite. La lecture locale gère
HEAD et Range ; une révocation sur le serveur ne retire pas une copie déjà hors
ligne. Taille connue, quota navigateur et limite de 4 Gio par fichier. Il s’agit
du fichier original : la compatibilité et les pistes disponibles dépendent du
navigateur. Détails : [PRODUCT_FEATURES.md](PRODUCT_FEATURES.md).

Validation locale : `npm run check`, `npm run lint`, `npm test`, `npm run build`,
`node scripts/test-pwa.mjs`. Ce dernier utilise Chrome headless et le build statique
avec une CSP restrictive : manifeste, dimensions PNG décodées, worker actif,
navigation hors ligne 503, cache public uniquement, téléchargement chiffré,
mauvaise phrase refusée, lecture Opus, pause/seek, Range exact et reconnexion.
L’analyseur Web Audio contrôle aussi le PCM du signal synthétique 440 Hz ; ce
n’est pas une preuve sur des haut-parleurs physiques. Ce test utilise un
serveur HTTP de test ; le service statique Core est vérifié séparément par
`cargo test --locked -p media-poc --test static_web -j 1 -- --test-threads=1`.
L’installation réelle desktop/iOS/Android et le NAS HTTPS ne sont pas validés.

Référence : [service workers SvelteKit](https://svelte.dev/docs/kit/service-workers).
