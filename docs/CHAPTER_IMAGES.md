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
Les transports direct et inverse ont un contrôle TCP local détaillé ci-dessous ;
une machine distante physique et les pannes réseau restent à qualifier.

Retour arrière : désactiver `chapterImagesEnabled` dans la bibliothèque ; les
images déjà stockées restent des dérivés évictables. Aucun schéma catalogue n'est
modifié pour ce lot. Prochain lot : tâches Core durables, annulation, génération
planifiée et tests distribués avec pannes.

## Complément du 4 novembre : transports TCP direct et inverse

Défaut confirmé par lecture du chemin inverse : la branche `chapter-image:`
ne passait pas ses réponses HEAD dans `encode_head_uplink`, contrairement aux
autres dérivés. Elle transmettait une longueur JPEG avec un corps POST vide.
La branche réutilise désormais cet encodage existant, y compris HEAD avec Range.
Le protocole et les signatures ne changent pas.

`crates/core/tests/chapter_images_remote.rs` lance un serveur Core et un serveur
Node sur deux ports TCP, dans le même processus de test Windows. FFmpeg 9.0.2
remuxe sans réencoder la fixture AV1/Opus en ajoutant deux chapitres explicites.
Le Node scanne et extrait réellement le JPEG de largeur 320. Le test vérifie le
lien direct signé, refuse l'appel Node non signé, puis supprime `base_url`,
actualise le registre Core et attend la présence effective du canal inverse.
Le repli direct est ainsi impossible. GET, HEAD, GET+Range et HEAD+Range sont
exercés dans les deux modes ; les octets du JPEG correspondent, les corps HEAD
sont vides et les longueurs sont conservées. Le média readonly est inchangé,
aucun dérivé n'apparaît à côté de lui. Le test Core de droits déjà validé reste
la preuve distincte des refus bibliothèque/parental/Node révoqué.

**Résultat final : 1/1 réussi, 13,66 s**, journal
`target/mission-chapter-remote-20261104-fixed.log`. Commande après compilation :

```powershell
& target/debug/deps/chapter_images_remote-c7e94c515b2d22b5.exe --ignored --nocapture
```

Cette exécution portait encore l'attribut ignored opt-in. Après réussite,
l'attribut a été retiré : le sharder Cargo existant découvre ce nouveau target
et la CI prépare déjà FFmpeg et la fixture sur chaque shard. Aucun workflow
supplémentaire n'a été ajouté ; une CI distante sur le futur commit reste à faire.

Résultats défavorables conservés : le premier harnais passait en 12,60 s mais
pouvait revenir au direct ; il n'est pas retenu comme preuve inverse. Le premier
essai avec canal imposé a expiré sur login pendant les compilations concurrentes
(27,93 s au total), sans atteindre HEAD ; il ne reproduit donc pas le défaut HEAD.
Le délai HTTP borné du harnais est passé de 12 à 30 secondes. L'essai final passe
avec la correction de production. Aucune causalité du timeout ni amélioration
de performance n'est déduite de ces observations.

Ce test ne prouve ni deux machines, ni TLS à travers un proxy, ni NAS, ni panne
en cours d'extraction. Retour arrière de ce correctif : désactiver les images
de chapitres sur canal inverse avant de revenir au binaire précédent.
