# Plugins : installation libre, paramètres et collections d’avatars

## Comportement

Aucun clic d’approbation n’est demandé pour un dépôt ou un auteur. Dépôts affiche
un court avertissement sur les plugins tiers. Les modules restent signés ; le Core
vérifie la clé déclarée par le catalogue, les empreintes, la compatibilité et les
limites Wasm. Cette clé n’est pas une certification de confiance. L’identité
interne de clé est un digest, sans réservation de nom d’auteur.

Installation désactivée, configuration, activation explicite. Pour les fournisseurs
installés sans périmètre antérieur, l’interface propose Toutes les bibliothèques.
`allLibraries:true` est un périmètre durable incluant les futures bibliothèques,
pas une liste figée lors du clic. Un ancien périmètre explicite est conservé.
Le Core envoie le contexte de l’œuvre et reçoit une proposition ; aucun accès
direct du plugin aux chemins/fichiers médias. Certaines bibliothèques reste
facultatif. Les avatars ne présentent aucun contrôle de bibliothèque.

Paramètres : page dédiée, configuration principale, interrupteur d’activation,
accès/identité et désinstallation séparés, outils de test repliés. Le schéma signé
produit les champs sans code spécifique à l’auteur. Modèle indépendant :
`../Aether-Plugins/templates/metadata-provider`, guide CREATING_PLUGINS.md.

## Encodage

La sortie native `--manifest` est lue comme UTF-8 via ProcessStartInfo, sans
transiter par le décodage de console Windows. Les textes déjà corrompus dans les
sources sont corrigés et les paquets sont reconstruits/signés. Une réparation
uniquement visuelle couvre les anciens snapshots et installations ; les octets
stockés et vérifiés ne sont jamais réécrits pour maquiller une signature.
Les nouveaux paquets refusent les motifs de corruption observés au contrôle.

## Collections d’avatars

Dans les paramètres d’une extension ayant la permission `avatars`, coller le lien
d’un dépôt GitHub, parcourir ses collections, choisir un dossier et un nom, importer.
Le dépôt cedev-1/jellyfin-avatars consulté organise ses images par dossier. Le
parcours reprend ce principe de sélection, sans installer de DLL Jellyfin.
Sources consultées : https://github.com/cedev-1/jellyfin-avatars et
https://github.com/cedev-1/jellyfin-plugin-GetAvatar.

Routes administrateur (session/CSRF sur mutations) :
- GET/POST `/api/v1/admin/extensions/{id}/avatar-packs` : collections installées / import `{url,folder,name}`.
- POST `.../avatar-packs/source` : `{url}` → `{folders:[{folder,count}]}`.
- DELETE `.../avatar-packs/{pack}` : retrait de la collection.

Routes authentifiées : GET `/api/v1/account/avatar/collections`, GET
`.../collections/{pack}` ; GET `.../collections/{pack}/{image}` pour le PNG local,
POST à cette même URL pour le choisir (CSRF, uniquement le compte courant).
La désactivation de l’extension ou la révocation d’une ancienne clé coupe les
collections et leurs images. Un avatar déjà choisi reste la photo du compte.

Imports : dépôt GitHub public racine HTTPS, arbre refusé s’il est tronqué,
révision SHA épinglée pour les téléchargements ; aucun chemin local, archive,
SVG externe ou URL d’image arbitraire. Maximum 32 collections, 500 images par
collection, 2 Mio/image, 64 Mio source, 32 Mio normalisés en préparation, deux
images traitées simultanément, un import à la fois, délai 300 s. Le broker
vérifie DNS public/TLS et refuse les redirections/proxies. Normalisation bornée
à 256×256 PNG, données ajoutées atomiquement après téléchargement ; une erreur
ne laisse aucune collection partielle. Migrations 0042 SQLite/PostgreSQL.

## Compatibilité de l’import manuel

POST `/api/v1/admin/extensions` accepte le paquet et `publicKey` au même niveau.
Aucune étape d’approbation requise. L’ancien import sans ce champ reste compatible
lorsqu’une clé de même éditeur avait été enregistrée ; les anciennes routes de
clé sont conservées pour les outils existants, retirées de l’interface. Le contrat
précédent d’approbation ne s’applique plus au catalogue ou au nouvel import.

## Dossier des plugins installés

Chemin par défaut : `data/plugins/<id>/` (dans le conteneur : `/data/plugins/`).
Chaque installation crée `package.json` (paquet signé complet), `manifest.json`
et `module.wasm`. La publication utilise un dossier temporaire puis un renommage.
Le worker lit le module depuis ce dossier avec ouverture confinée et contrôle
SHA-256 avant chaque invocation ; un module altéré n’est pas exécuté.

La base conserve autorisations, paramètres privés, historique et snapshot signé
pour sauvegarde/reprise. Au démarrage des deux binaires Core et après l’import
local, les anciennes installations présentes uniquement en base sont exportées
vers ce dossier. Une installation manquante peut aussi être matérialisée lors
de sa première exécution. La désinstallation retire les trois fichiers et le
dossier ; les fichiers d’un autre nom ne sont pas effacés récursivement.

SQLite : le dossier plugins suit le dossier de la base (défaut data/core.sqlite3).
PostgreSQL : le défaut reste data/plugins ; les images Docker fixent explicitement
MEDIA_PLUGINS_PATH=/data/plugins pour utiliser le volume /data. `MEDIA_PLUGINS_PATH` permet un chemin
opérateur explicite. Ce dossier n’est pas une inbox : y déposer du code à la main
ne l’active pas et ne contourne pas la vérification du paquet. Les médias restent
distincts. Une erreur de stockage est explicite et ne bloque pas tout le Core.


## Vérifications exécutées

- `npm run check --prefix apps/web`, `npm run lint --prefix apps/web`,
  `npm test --prefix apps/web` : 113 tests ; build production réussi.
- `node scripts/browser-extension-catalogue-ui.cjs` : huit manifestes réels,
  APIs simulées, aucune approbation/API éditeur, configuration/activation/import,
  clair/sombre 1440/390, champs mobile lisibles et aucune erreur navigateur.
- `cargo test --locked -p media-poc --test extensions -j 1 -- --test-threads=1` :
  dix tests Core réussis, puis relancés après le complément de stockage : dix réussis
  à nouveau. Test extension_storage : création/reprise/altération/suppression réussi ;
  `cargo clippy --locked --all-targets -j 1 -- -D warnings` et format réussis.
- Navigateurs Core réels : cycle installation/exécution Rust-Wasm/sauvegarde/
  désactivation/désinstallation et dépôts/version/retrait réussis.
- `MEDIA_TEST_AVATAR_LIVE=1 cargo test --locked -p media-poc --lib extensions::collections::tests::imports_normalized -j 1 -- --nocapture --test-threads=1` : vrai HTTPS,
  sept collections découvertes et 26 images sonny importées en SQLite temporaire.
- Aether-Plugins : `cargo test --workspace -j 1 -- --test-threads=1` : 41 tests ;
  `node scripts/test-repository.cjs` : huit paquets/signatures/UTF-8 vérifiés.
- Modèle indépendant compilé wasm32 puis réellement exécuté par le worker Core,
  149077 octets, paramètre prefix transmis et texte français conservé.

Preuves : `docs/evidence/2026-10-23/plugin-settings/`. Le test d’import GitHub est
réel ; les captures des huit fournisseurs utilisent des APIs simulées. Aucun
compte fournisseur/API payante, PostgreSQL réel, NAS ni image Docker déployée ne
sont prouvés. Les fichiers modifiés restent locaux : aucun push ou déploiement.

## Téléchargement serveur et upload local

Deux possibilités dans Collections d’avatars :
- GitHub : le Core parcourt le dépôt et télécharge sur le serveur le dossier choisi.
- Importer depuis cet ordinateur : choisir un nom et plusieurs images PNG/JPEG/WebP,
  puis Envoyer au serveur. Aucun hébergement d’image externe n’est nécessaire.

Les images sont conservées dans les données applicatives du serveur, normalisées
et disponibles même si le dépôt GitHub disparaît. Le navigateur des utilisateurs
ne charge que les PNG locaux authentifiés ; pas d’URL distante dans la galerie.

POST `/api/v1/admin/extensions/{id}/avatar-packs/upload` : multipart/form-data,
champ `name` et champs `images` répétés. Authentification administrateur, CSRF,
permission `avatars`, même slot global que le téléchargement. Limites : 500
images, 2 Mio/image, 64 Mio source cumulée (+ enveloppe bornée à 65 Mio), 32 Mio
normalisés, délai 300 s, stockage atomique. Un échec ne publie pas une demi-collection.
L’upload concerne les fichiers images sélectionnés ; aucune extraction ZIP ni
interprétation de chemin envoyé par le client.

Validation upload : test Core multipart >400 Ko, normalisation 256×256 et refus
anonyme/CSRF ; même garde d’activation pour la galerie. Aucun ZIP annoncé.

Contrôle UI final : upload multipart (Content-Type et nom de fichier vérifiés),
confirmation d’envoi, sauvegarde/activation et choix depuis la galerie du profil
réussis en Chrome 155 ; APIs simulées explicitement. L’ancienne réponse absente
`account/sessions` du scénario a été complétée ; les liens de SettingsView sont
conservés. Le test réel Core valide la réception multipart et la normalisation.

Validation finale après verrouillage des dépendances multipart : les deux tests
collections passent à nouveau ; Clippy all-targets strict et cargo fmt --check
passent. Le build Web, les 113 tests Web et le scénario navigateur final sont
réussis. Aucun push, déploiement ou média personnel modifié.
