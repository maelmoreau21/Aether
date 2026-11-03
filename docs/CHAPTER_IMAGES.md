# Images de chapitres — premier parcours à la demande

Les chapitres proviennent exclusivement du probe du média. Le Core trie et borne
leurs horodatages, puis résout un index opaque ; le client ne fournit ni chemin,
ni timestamp libre. Aucun chapitre n'est inventé. Les planches périodiques
Trickplay restent un produit distinct.

## Parcours et contrats

Fiche film/épisode → `ChapterImages.svelte` →
`GET /api/v1/assets/{asset}/chapter-images` → liste des chapitres et des URL.
`GET|HEAD /api/v1/assets/{asset}/chapter-images/{index}` renvoie une image JPEG,
`202 {status:"generating"}`, ou une indisponibilité explicite. Les routes passent
par les mêmes contrôles session/bibliothèque/parental que les médias. Une
bibliothèque désactivée, un Node révoqué ou un index inexistant n'accorde aucun
accès aux dérivés.

`chapterImagesEnabled` autorise ce parcours. Les valeurs bibliothèque
`trickplayWidth` et `trickplayQuality` déterminent effectivement la largeur et la
qualité JPEG ; elles sont réutilisées plutôt que dupliquer des réglages.
L'option `chapterImagesDuringScan` reste réservée et désactivée dans l'interface.
Les médias sont toujours en lecture seule.

Le Core délègue au Media Node propriétaire. En standalone, l'appel reste dans le
service Node local ; en distribué il passe par le protocole signé direct ou le
canal inverse. Les racines et fichiers sont résolus depuis l'inventaire Node,
jamais depuis un chemin fourni par le navigateur. Les paramètres d'extraction
sont bornés côté Node, y compris timestamp, durée, largeur et qualité.

## Extraction, stockage et reprise

Le `TrickplayManager` existant extrait une seule frame à la frontière du chapitre,
avec une grille 1×1. Son empreinte inclut l'identité/fingerprint source, les
paramètres et le timestamp, avec un discriminant chapitre. Elle ne peut donc pas
réutiliser une planche périodique. Les images et manifestes sont dans le cache
applicatif Node ; les dossiers source sont refusés comme destination de cache.

Les images de chapitres partagent le quota/worker Trickplay. Le worker et son
registre de tâches sont désormais réellement partagés entre les clones :
l'ancienne recréation des Mutex/Semaphore laissait un état Running dans le
manager appelant et ne bornait pas correctement le parallélisme. Le compteur
des lectures standalone est raccordé au manager global ; le Node distant
utilise déjà son compteur de streams. Une lecture active interrompt le travail
FFmpeg en arrière-plan. Chaque extraction de chapitre est bornée à 60 secondes.

Une image terminée est persistante et réutilisable après redémarrage. Une demande
après changement du fingerprint source génère une autre référence. Le quota
évince uniquement les dossiers dérivés aux noms SHA-256 ; les dossiers étrangers
et liens/reparse points sont ignorés. Les dossiers de travail propres, abandonnés
depuis plus de 24 heures et sans tâche active, sont nettoyés. Les médias ne sont
jamais des cibles de nettoyage.

Le profil FFmpeg Docker active désormais `mjpeg`, `fps,scale,format,tile` et un
décodeur AV1 CPU explicite `libdav1d`. Auparavant le profil désactivait les
filtres, swscale et tous les encodeurs sauf WebVTT, alors que Trickplay appelait
ces capacités. Il faut reconstruire les images pour obtenir ce runtime.

## Validation et limites

Les commandes et résultats exécutés sont consignés dans le rapport de lot après
exécution ; la présence du test ne constitue pas un succès.

Tests ciblés prévus : paramètres réseau bornés, fingerprints séparés, propagation
de l'état Failed du worker, quota/nettoyage sélectif et refus d'un cache dans les
médias. Un test explicite avec FFmpeg et la fixture AV1/Opus générée vérifie une
vraie image, une seule frame, la réouverture du cache et les octets source
inchangés. La compilation/présence du décodeur ne prouve pas encore l'extraction
dans l'image Docker : le smoke embarqué doit aussi être exécuté.

Le lot est **partiel** : génération à la demande disponible, pas de planification
durable Core ni de génération pendant le scan. Fermer le panneau annule les
requêtes/polls frontend ; une extraction déjà admise continue sur le Node et peut
être réutilisée ensuite. Il n'existe pas encore d'action administrative d'annulation
de cette extraction. Les échecs conservent un délai de reprise de cinq minutes ;
le frontend attend de manière bornée et propose une actualisation explicite.
La reprise porte sur les fichiers de cache, pas sur une tâche durable en vol.
Le distribué direct/canal inverse nécessite encore une validation dédiée.

Retour arrière : désactiver `chapterImagesEnabled` dans la bibliothèque ; les
images déjà stockées restent des dérivés évictables. Aucun schéma catalogue n'est
modifié pour ce lot. Prochain lot : tâches Core durables, annulation, génération
planifiée et tests distribués avec pannes.
