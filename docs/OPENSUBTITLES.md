# Sous-titres téléchargés — parcours manuel

Le fournisseur est un plugin Wasm optionnel dans `Aether-Plugins-OpenSubtitles`,
sans clé ni compte intégré. Son [contrat](../../Aether-Plugins/Aether-Plugins-OpenSubtitles/README.md)
et ses [preuves hors ligne](../../Aether-Plugins/docs/OPENSUBTITLES_2026_11_03.md)
décrivent les conditions du service consultées. Une clé API et, selon les droits
et quotas du compte, une authentification OpenSubtitles sont nécessaires.
Les tests simulés ne valident ni un abonnement ni un accès réel au service.

## Parcours et responsabilités

L'administrateur installe un paquet signé compatible, configure ses secrets dans
Extensions, l'active et lui accorde la bibliothèque. La permission signée
`subtitles` autorise les opérations `subtitles.search` et `subtitles.download`.
Le binaire `media-extension-worker` doit être présent à côté du Core ; le test
vertical avec Wasm exige également ce binaire construit au préalable.

Dans la bibliothèque, activer le téléchargement manuel, autoriser le fournisseur
dans sa liste éventuelle et désactiver l'exigence de hash pour utiliser ce lot.
L'exigence de hash bloque explicitement les requêtes : aucune empreinte ni
correspondance exacte de fichier n'est inventée. Les langues automatiques,
« uniquement manquants » et « correspondre à l'audio » restent réservées à une
automatisation indisponible. La langue manuelle est choisie dans le dialogue.

Depuis les actions d'un film ou d'un épisode, choisir une copie, rechercher,
examiner les correspondances puis télécharger explicitement un candidat.
L'identité IMDb/TMDb est prioritaire ; une recherche par titre est annoncée
comme telle. Pour un épisode sans identité compatible, le Core fournit le titre
de sa série avec les numéros de saison et d'épisode. Ni un identifiant ni un
titre ne garantissent la synchronisation des sous-titres.

Le Core possède l'autorisation, les données et les requêtes au plugin ; il ne
parcourt aucun disque distant. Les contenus UTF-8 SRT/WebVTT sont normalisés
en WebVTT inerte par le parseur existant, puis conservés dans la base applicative,
jamais à côté des médias. Les archives, autres encodages et formats téléchargés
ne sont pas pris en charge. Le lecteur reçoit une piste `downloaded-<id>` dans
son descripteur habituel. Après acquisition ou suppression, le frontend invalide
le cache du descripteur ; relancer le lecteur actualise la sélection des pistes.

La recherche, le téléchargement et la suppression sont réservés au rôle
`administrator` pour ce premier lot de dérivés partagés. La lecture de la piste
est autorisée aux comptes ayant les droits actuels sur le média ; session,
bibliothèque active, Node non révoqué et règles parentales sont vérifiés par le
middleware commun à chaque lecture. Les mutations et recherches exigent CSRF.

## API

| Route | Entrée | Résultat |
|---|---|---|
| `GET /api/v1/admin/subtitles/{assetId}` | Copie inventoriée | `enabled`, `unavailableCode`, fournisseurs accordés, téléchargements et quotas connus |
| `POST /api/v1/admin/subtitles/{assetId}/search` | `providerId`, `language`, `forced?`, `sdh?`, `page?` | `searchId`, candidats bornés, `totalPages` |
| `POST /api/v1/admin/subtitles/{assetId}/download` | `searchId`, `fileId` | Identifiant du dérivé, état, quota restant et échéance si fournis |
| `DELETE /api/v1/admin/subtitles/{assetId}/{downloadId}` | Identifiants existants | `204` si supprimé |
| `GET` / `HEAD /api/v1/assets/{assetId}/subtitles/downloaded-{downloadId}` | Lecture autorisée | WebVTT, `private, no-store` |

Le client ne choisit ni une URL de téléchargement ni des paramètres de compte.
Une recherche est liée à l'acteur, la copie, la génération de l'extension et
l'empreinte taille/date de modification. Sa durée est de 15 minutes ; les
résultats sont limités à 60 fichiers par page et 100 pages demandables. La
validation de sortie supprime les champs inconnus et les liens privés avant
persistance ou réponse publique. Les erreurs exposent des codes, sans message
brut du fournisseur. Les liens temporaires ne sont pas conservés dans l'historique.

## Quotas, concurrence et interruption

Une réservation transactionnelle durable précède la requête qui consomme un
téléchargement chez le fournisseur. L'unicité copie/fournisseur/fichier empêche
les doublons, y compris entre requêtes concurrentes. Une seule réservation
active par fournisseur est admise. Le plafond local est de 16 dérivés par copie,
256 au total, chacun limité à 1 Mio après normalisation ; les tentatives en
attente ou échouées occupent aussi une place jusqu'à suppression explicite.
Il s'agit de bornes fixes, pas de réglages administrateur supplémentaires.

Les réponses `429`/`503` du broker conservent leur délai borné. Un quota
`remaining: 0` est persistant et bloque les autres téléchargements jusqu'à
l'échéance RFC3339 fournie. Sans échéance valide, ou après un refus explicite
`download_quota_exceeded`, le repli prudent est de 24 heures. La recherche
reste indépendante du quota de téléchargement. L'interface affiche le dernier
quota connu et ne revendique pas une valeur actualisée sans nouvelle réponse.
Ces limites sont conservées par identifiant de fournisseur dans ce premier lot :
changer de clé ou de compte n'efface pas un délai existant. Cette borne prudente
peut donc retarder un nouveau compte jusqu'à l'échéance connue.

Le worker est borné à 65 secondes, puis la récupération HTTPS du contenu à
12 secondes ; le client accorde 90 secondes au téléchargement. Fermer le
dialogue interrompt l'attente et le worker lorsque la requête disparaît ; une
requête externe déjà envoyée peut néanmoins avoir consommé du quota. Aucun
retry automatique n'est effectué, même après une erreur réseau ambiguë.

Une réservation sans résultat devient visiblement `interrupted` après
120 secondes. Elle reste durable et ne se rejoue pas après redémarrage.
L'administrateur doit l'examiner puis la supprimer avant une nouvelle tentative
explicite. Une tentative encore active ne peut pas être supprimée. Si la copie
change, les pistes associées à son ancienne empreinte sont retirées du
descripteur et refusées en lecture ; la liste les annonce `stale`, supprimables.
Les administrateurs peuvent encore lister et supprimer les dérivés conservés
d'une copie devenue manquante ou d'une bibliothèque désactivée. La liste indique
alors `enabled: false` et `subtitle_asset_unavailable` ; aucune recherche ou
requête fournisseur n'est autorisée, et les droits de lecture restent inchangés.

Le téléchargement du lien utilise exclusivement HTTPS/443 et un hôte exact du
manifeste signé. Toutes les réponses DNS doivent être publiques, puis sont
épinglées pour la connexion. Aucun proxy implicite, redirection ou en-tête
d'authentification fournisseur n'est transmis au CDN. Le téléchargement est lu
par blocs et arrêté à sa limite ; aucun chemin disque libre n'est accepté.

## Vérifications, risques et retour arrière

Les tests ciblés sont dans `crates/core/src/extensions/subtitles/tests.rs`, `provider_download.rs`,
`subtitle-download-api.test.ts`, `browser-subtitle-downloads.cjs` et les scripts
Wasm du dépôt des plugins. Ils distinguent contrats hors ligne, vrai worker
Wasm signé, persistance SQLite, API synthétiques du navigateur et lecture réelle.
Voir la [matrice de mission](archive/missions/MISSION_FEATURES_2026_11_03.md) pour les résultats
effectivement exécutés ; la seule présence des tests ne clôture pas la couverture.

Le 3 novembre, le parcours HTTP avec worker Wasm signé et réouverture SQLite
a réussi (1/1), puis le runner PostgreSQL ciblé a réussi (1/1) avec deux pools
et un conteneur PostgreSQL 18 jetable. Les récupérations réseau de sous-titres
étaient synthétiques ; ces résultats ne valident pas un accès OpenSubtitles réel.

Le schéma est additif et idempotent sur SQLite/PostgreSQL. La présence des SQL
équivalents ne constitue pas une validation PostgreSQL réelle. Le runner ciblé
`pwsh -NoProfile -File scripts/test-postgres.ps1 -OnlySubtitles` crée une base
PostgreSQL 18 jetable en tmpfs, un mot de passe aléatoire transmis par
environnement et un port lié seulement à la boucle locale, puis supprime
uniquement son conteneur. Il ne vise aucune base existante. L'ajout d'une
table ne modifie ni l'inventaire ni les originaux. Le principal risque produit
reste la qualité/synchronisation du sous-titre choisi et les quotas consommés
avant une interruption ; le parcours demande une sélection explicite et permet
de supprimer uniquement le dérivé applicatif.

Pour revenir en arrière, désactiver le téléchargement manuel et le plugin ;
les dérivés existants restent lisibles et supprimables par leur parcours dédié.
Un retour à un binaire antérieur ne les propose plus au lecteur ; il conserve
leurs tables sans les rejouer. Ne pas supprimer les médias ni la base pour ce
retour arrière. Les petits lots Core/API/données et plugin peuvent être revus
séparément, mais un paquet utilisant `subtitles` exige un Core compatible.
