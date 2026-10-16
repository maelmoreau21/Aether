# API expérimentale du banc P0

Base exacte par défaut : `http://127.0.0.1:3000`. Contrat du banc uniquement,
pas un contrat de sessions de lecture du futur produit. Pas de chemins disque
acceptés depuis HTTP ; quatre IDs initiaux, quatre remux par langue optionnels
et quatre fixtures représentatives optionnelles, tous explicites.

IDs représentatifs : `fixture-representative-mkv`, `fixture-representative-mp4`,
`fixture-representative-mkv-en`, `fixture-representative-mp4-en`. Même transport,
champ optionnel `representative: {expected_audio_language, expected_audio_channels, method}`
déclarant le cas du banc, jamais la compatibilité client. Profil vidéo 10 bits,
nombre de pistes et Opus FR 6/EN 2 canaux vérifiés au chargement ; un dossier
`representative` existant mais incomplet empêche le démarrage.
Ces variantes se chargent comme sources distinctes : le menu de remux 8 bits
n'est pas présenté comme un changement de piste 10 bits.

Le menu sous-titres utilise uniquement `video.textTracks`, pas le probe comme
preuve de rendu. Zéro piste native : menu désactivé et motif visible. Sinon,
il demande un mode `showing` exclusif ; cette branche reste NT sur les clients
actuels qui n'exposent aucune piste texte intégrée.

| Méthode et route | Résultat |
| --- | --- |
| GET `/api/v1/health` | 200 JSON `status=ok`, `mode=local-p0-bench` |
| GET `/api/v1/assets` | 200 tableau `{id,mime,probe:{streams,format},playback_status}` |
| GET `/api/v1/assets/{id}/content` | 200 fichier, 206 plage unique valide, 416 plage invalide |
| HEAD `/api/v1/assets/{id}/content` | 200 longueur/type/Accept-Ranges, corps vide |
| GET `/` et `/bench.js` | page et script du banc natif |

Le banc expose un sélecteur audio natif conditionnel basé uniquement sur
`HTMLMediaElement.audioTracks`. Désactivé si cette API est absente ou expose
moins de deux pistes ; message technique explicite. Un changement demandé
est journalisé sans être présenté comme une commutation réussie. Aucun nouvel
endpoint de remux/sélection à la volée, aucun média original modifié, aucun moteur MSE ajouté.

P0C ajoute quatre IDs dérivés optionnels : `fixture-mkv-fr`, `fixture-mkv-en`,
`fixture-mp4-fr`, `fixture-mp4-en`, sur les mêmes routes HEAD/GET/Range.
Leur descripteur contient `language_remux: {source_asset, language, source_audio_stream}`.
Langues BCP 47 `fr`/`en` déclarées pour ces fixtures seulement ; index source
1/2, puis audio index 1 dans chaque sortie. Aucun chemin disque public.
Le champ est absent pour les quatre assets initiaux. Le serveur charge les
remux seulement si le dossier de préparation existe ; un dossier partiel ou
une piste/ langue incompatible empêche le démarrage. Pas de scan à la demande.

La page garde les quatre sources initiales dans son menu et propose un choix
FR/EN par remux pour MKV/MP4 préparés. L'URL change réellement ; elle restaure
la position et l'état lecture/pause. Cela ne constitue pas une sélection
interne dans le conteneur. Contrat expérimental détaillé dans `P0C_AUDIO_SWITCHING.md`.

`playback_status=unverified-on-client` signifie que le serveur ne conclut jamais
à la compatibilité depuis le probe. Les résultats observés sont consignés dans
la matrice de compatibilité, pas transformés en autorisation Direct Play universelle.

Plages testées : fermées, suffixes, ouvertes, fin au-delà du fichier (bornée).
Range invalide, hors fichier, inversé, suffixe zéro ou multiple → 416 et
`Content-Range: bytes */taille`. Pas de multipart/byteranges. Les unités Range
autres que bytes et If-Range ne font pas partie des assertions de ce jalon.

Identifiant inconnu → 404 JSON `error=unknown_asset` ; fichier absent → 404
`asset_unavailable` ; cible canonique hors racine → 403
`asset_outside_fixture_root`. Host/Origin non autorisé ou cross-site → 403.
Méthode POST sur content → 405. Les rejets middleware/routage peuvent être sans
corps JSON. Aucun CORS permissif. Les probes sont effectués séquentiellement
au démarrage, jamais dans une requête HTTP ; un échec empêche le démarrage.

## Extension expérimentale P0C MSE

GET /mse et /mse.js : banc minimal, sans API produit. GET/HEAD
/api/v1/experiment/mse/{name} expose exclusivement video.mp4,
audio-fr.mp4, audio-en.mp4, fr-full.vtt, fr-forced.vtt, en-full.vtt,
dans le sous-dossier fixe mse/verified des fixtures ; aucun chemin accepté.
MIME video/mp4, audio/mp4, text/vtt ; Range/206/416 par le même streaming
borné64KiB. Inconnu404 unknown_experiment_asset ; absent/horsracine404
experiment_asset_unavailable. Host/Origin et refus cross-site applicables.
CSP media-src autorise aussi blob: pour MediaSource. Aucune donnée privée.
Chargement borné au clip synthétique12s ; pas de service de packaging à la volée.

## Extension Shaka/DASH P0C

GET /shaka et /shaka.js : banc synthétique. GET/HEAD
/api/v1/experiment/shaka/{name} autorise exactement manifest.mpd (application/dash+xml),
video.mp4 (video/mp4), audio-fr.mp4 et audio-en.mp4 (audio/mp4),
fr-full.vtt, fr-forced.vtt, en-full.vtt (text/vtt), depuis le dossier fixe dash.
shaka-player.compiled.js est le seul bundle autorisé, depuis vendor/shaka-5.2.13.
Pas de chemin disque, URL ou manifeste arbitraire accepté. Canonicalisation,
Host/Origin/cross-site, no-store et nosniff conservés. ServeFile par blocs64KiB,
Range206/416 ; inconnu404 unknown_experiment_asset, absent/horsracine404
experiment_asset_unavailable. Aucun packaging, téléchargement de dépendance
ou encodage à la requête. /mse reste inchangé.

### Paramètre expérimental safeMargin du banc Shaka

window.shakaBench.switchLanguage(language, safeMargin=0) accepte uniquement
0, 0.5, 1, 2 ou4 secondes ; langueFR/EN déjà disponible. Défaut0 des boutons
et des anciens tests conservé. Rejet explicite des autres valeurs, sans arrondir
ni augmenter la configuration de buffers. Aucun nouvel endpointHTTP. Le retour
API indique une demande, pas la preuve du signal ; celle-ci dépend du testPCM.


### Expérience de récupération après seek

GET /audio-seek-policy.js sert uniquement le contrôleur fixe du banc Shaka (JavaScript, même protection Host/Origin/CSP que le banc ; origine étrangère403). Aucun chemin fourni par le client. Option désactivée par défaut dans /shaka. État idle/recovering/error, erreur terminale sans boucle ; rechargement de la présentation après erreur. Le statut de récupération API ne prouve pas le signal audio. Voir P0C_AUDIO_SEEK_RECOVERY.md.

## P1 local — contrat ajouté

GET /api/v1/config : {name,mode:"localhost-fixtures"}.
GET /api/v1/demo/media : {media:[{id,title,sources:[{id,audioTrackId,language,url,mime,tracks}],subtitles:[{id,language,label,url}]}],notice}.
Sources = copies mono-audio FR/EN des fixtures10bits, langue BCP47 explicite ;
tracks conserve les propriétés ffprobe (ISO fra/eng, index et champs techniques).
IDs de démo uniquement, aucune identité de catalogue persistant. Aucun nom de lecteur
imposé. Aucun chemin disque renvoyé. Absence du dossier p1 : media vide ; dossier
partiel : démarrage refusé. Les anciens /api/v1/assets restent le contrat P0.
Les URLs de contenu démo réutilisent GET/HEAD /api/v1/assets/{id}/content, Range206/416.
GET/HEAD /api/v1/demo/subtitles/{name} : fr-full.vtt/fr-forced.vtt/en-full.vtt,
text/vtt UTF8, autres noms404. Ressource préparée absente404. POST405, origine
tierce403. Ce contrat localhost sans authentification ne permet aucune bibliothèque
réelle. Voir P1_WEB_INTEGRATION.md pour tests et limites.

## P1 — Catalogue de fixtures (15 octobre 2026)

Contrat ajouté, toujours sans authentification et limité à localhost :

| Méthode et route | Contrat |
| --- | --- |
| GET `/api/v1/catalog` | `{items,total,offset,limit,mode,notice}` ; œuvres `film` et `series` uniquement dans la liste |
| GET `/api/v1/catalog/{id}` | Fiche Work, `editions` et `seasons` ; épisode accessible par son ID explicite |

Liste : `kind=film|series` facultatif ; `q` facultatif (200 octets maximum),
recherche de sous-chaîne insensible à la casse après trim ;
`sort=title-asc|title-desc` (défaut ascendant), `offset` entier positif ou nul
(défaut 0), `limit=1..100` (défaut 24). Paramètre inconnu, valeur invalide ou
requête non décodable : 400 JSON `error=invalid_catalog_query`.
Liste sans fixture préparée : 200, `items=[]`, `total=0` et notice explicite.
Fiche inconnue ou fixtures absentes : 404 JSON `error=unknown_work`.
POST refusé par le routage (405) ; les protections Host/Origin/cross-site restent
applicables (403). Aucun chemin physique dans ce contrat.

Work : `id,kind,title,year,description,libraryId,synthetic`, avec compteurs
`seasonCount,episodeCount` lorsqu'applicables. `year=null` sur les fixtures.
Une fiche de série expose `seasons:[{id,number,title,episodes}]` ; chaque épisode
porte sa propre identité Work, son numéro et ses éditions. Une édition contient
`id,label,versions` ; une version `id,label,sources` ; une source
`id,assetIds,playbackMediaId`. `playbackMediaId` référence le descripteur existant
de `/api/v1/demo/media`, qui fournit URLs et Tracks. Aucun endpoint de session
autorisée, scan ou création de bibliothèque n'est ajouté.

`fixture-film` associe explicitement MKV et MP4 ; `fixture-series` regroupe une
saison et `fixture-episode-1` réutilisant la même mire. Ces IDs sont fixes,
pas issus d'une base, d'une fusion de titres ou d'un fournisseur de métadonnées.
Voir `CATALOGUE_P1.md` pour les parcours, tests et prérequis de sécurité.

Validation locale : suite Rust 15 tests réussis ; parcours du catalogue sous
Chrome 155.0.8059.40 Windows headless, états vides injectés, 503/réessai, fiche
404 et pagination simulée 72 œuvres bornée à 24 cartes vérifiés.
Preuves `evidence/2026-10-15/catalogue/`. Ces essais ne valident pas un scanner,
une persistance SQLite ou les droits d'accès d'un futur catalogue personnel.

## P1 — persistance et bootstrap local

SQLite/SQLx et création du premier administrateur par CLI locale sont décrits
dans [LOCAL_ADMIN.md](LOCAL_ADMIN.md). Aucune route HTTP sensible ajoutée.
Sessions, connexion et RBAC restent absents ; fixtures uniquement sur loopback.
