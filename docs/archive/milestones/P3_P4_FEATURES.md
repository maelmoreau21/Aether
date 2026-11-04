# Pistes externes et séances utilisateur — 18 octobre 2026

## Extension lecteur et catalogue

Le nouveau parcours local ajoute les pistes Opus internes préparées, les sous-titres
intégrés textuels, le repli texte ASS/SSA, les offsets signés et la date d'ajout
persistante. Les limites historiques ci-dessous sont remplacées uniquement pour
ces parcours : [contrat et limites actuels](PLAYER_TRACK_PREPARATION.md).

## Parcours implémenté

Le Node associe les fichiers du même dossier par **stem vidéo exact et unique**.
Exemple : `Film UHD.mp4`, `Film UHD.fr.forced.srt`, `Film UHD.en.opus`.
Les suffixes de langue usuels `fr/fra/fre`, `en/eng`, `de/deu/ger`, `es/spa`,
`it/ita`, `ja/jpn` et les codes de deux lettres sont reconnus, avec `forced`,
`sdh` et `cc`. Sans langue connue : `und`, sans supposition de français.
Deux vidéos `Film.mkv` et `Film.mp4` rendent `Film.fr.srt` ambigu : avertissement,
aucune association arbitraire. Utiliser des stems distincts pour les versions.
Pas d'association vague par titre ou de déduction d'offset depuis un nom libre.

SRT UTF-8 et WebVTT sont normalisés en cues WebVTT texte. Les balises sont
échappées ; styles et positionnement ne sont pas reproduits. ASS/SSA est détecté
mais refusé avec motif explicite : aucun moteur ASS ni fausse conversion annoncée.
Limites : 256 Kio par entrée, 512 Kio normalisés par vidéo, 16 pistes, 20 000 cues.
La liste du dossier est partagée avec les découvertes NFO/images pour éviter des
lectures de répertoire répétées par vidéo. Les lots sont vidés aussi selon le
volume de sous-titres pour limiter la taille des messages de contrôle distants.

Le Node publie des instantanés texte dans les lots de scan. Le Core les stocke
transactionnellement, séparément des probes techniques, avec le catalogue.
Les rescans relisent les sidecars même lorsque les probes vidéo sont réutilisés.
Le lecteur reçoit uniquement des IDs, des propriétés de piste et des URLs
autorisées ; aucun chemin local. Les instantanés restent ceux du dernier scan
publié, y compris si un sidecar est ensuite modifié/supprimé : rescanner pour
actualiser. Aucun original n'est écrit.

Les audios `.opus`/`.ogg` sont analysés via le même ffprobe confiné et borné.
Un seul flux Opus dans un conteneur Ogg est accepté par fichier. Langue du nom,
puis métadonnées connues si absente. Taille, modification, timebase, canaux,
durée et initialisation codec restent dans l'inventaire privé. Les audios sont
réanalysés au scan ; pas de transcodage ni de remux pendant la lecture.
Le Core sert les audios locaux par handle vérifié et relaie les audios distants
depuis l'inventaire opérationnel signé du Node. La vidéo peut rester en direct
Node ; les octets audio externes passent actuellement par le Core.
Les URLs audio utilisent une clé de fichier opaque, indépendante de l'ordre
des pistes au rescan. Un instantané obsolète est refusé plutôt que substitué.

Le moteur natif utilise deux éléments média : vidéo comme horloge, audio externe
avec correction de dérive au-delà de 150 ms toutes les 100 ms, pause, seek,
volume et vitesse couplés. Changement de piste par remplacement borné avec
position conservée. Ce mécanisme n'est pas une synchronisation à l'échantillon.
Une piste détectée ne garantit pas le décodage navigateur ; erreurs explicites,
sans transcodage vidéo ni récupération répétée. Un offset est représenté dans le
contrat mais vaut zéro dans ce jalon ; pas encore d'édition manuelle des offsets.
Les pistes internes MKV/MP4 restent inventoriées ; leur commutation arbitraire
et le packaging généralisé multi-pistes ne sont **pas** ajoutés ici.

Les sous-titres forcés français sont choisis par défaut si présents. Les choix
audio et sous-titres manuels (y compris désactivés) sont mémorisés par compte
dans le navigateur, sans jeton d'authentification dans ce stockage.

## Progression et séries

La progression est liée à `(user_id, work_id)`, commune aux versions de l'œuvre,
avec position, durée, vu/non vu, date et révision. Le lecteur reprend les médias
non terminés ; sauvegarde périodique toutes les cinq secondes, sur pause et
fermeture. La fin ou 95 % lus marque vu. Une coupure brutale peut perdre la
dernière tranche non envoyée. L'API persiste dans SQLite et PostgreSQL et ne
dépend pas du navigateur d'origine. Un conflit de révision refuse une écriture
ancienne (409), avec message invitant à rouvrir le lecteur, sans écrasement.

Accueil « Continuer à regarder », statut et action manuelle vu/non vu sur films
et épisodes. Historique borné aux 100 dernières œuvres ; absence de progression
n'invente pas une séance. Les durées inconnues empêchent le marquage manuel.
La navigation série/saison/épisode, recherche, NFO et versions existantes sont
réutilisées. Le lecteur propose l'épisode suivant et un enchaînement automatique
opt-in, y compris entre saisons ; l'accès est revalidé au lancement.

Les rescans conservent l'historique tant que le Work reste identifié. Un déplacement
dont l'identité éditoriale est déjà établie garde ce lien ; aucun rapprochement
approximatif ou transfert automatique d'historique entre identités différentes.
Restent notamment : récence d'ajout persistante, miniature d'épisode dédiée,
profils facultatifs, édition d'offset, packaging multi-pistes complet et ASS.

## Raw : POC non intégré

`scripts/raw-poc.ps1` extrait uniquement des flux de la fixture synthétique par
copie de paquets, dans un dossier temporaire ignoré. Rapport :
`evidence/2026-10-18/raw-poc/report.json`.
288 paquets AV1 sont détectés ; leur dernier PTS passe de 11,958 s dans le MP4
à 11,480 s dans l'OBU avec une autre timebase. Les timestamps affichés par le
demuxer raw ne reproduisent donc pas ceux de référence. Les 389 987 octets Opus
concaténés ne sont pas reconnus seuls (ffprobe : sortie 1). Le témoin conserve
48 kHz, six canaux et 27 octets d'initialisation ; ces informations ne suffisent
pas à restituer les frontières de paquets et l'alignement après concaténation.
Il manque un index conservant tailles/frontières, timestamps/timebases,
initialisation, pre-skip et synchronisation, puis une preuve de rendu navigateur.
Pas de format propriétaire ni de support raw de production annoncé ; P4 continue.

## Vérification

Commandes et résultats finaux consignés dans MEMORY.md. Les preuves navigateur
utilisent des fixtures synthétiques de 12 secondes sous Chrome Windows headless.
Elles ne prouvent ni film long, ni restitution physique 5.1/10 bits, ni Safari,
Firefox, mobile physique ou robustesse réseau prolongée.
