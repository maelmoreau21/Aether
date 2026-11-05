# Administration, aperçus fournisseurs et chapitres

Le planificateur expose directement les plans, leur activation et leur intervalle
effectif. Les erreurs structurées sont visibles ; les états sont relus pendant
que l'écran est ouvert. La désactivation d'un scan conserve son dernier résultat.
Les déclencheurs quotidiens, hebdomadaires et au démarrage de Jellyfin ne sont pas
implémentés : les contrôles actuels règlent des intervalles persistants.

Les miniatures Trickplay possèdent leur rubrique `#trickplay-section` dans
l'administration. Leur formulaire ne réenregistre pas les autres politiques de
lecture ; Général ne réécrit pas les paramètres Trickplay.

Les images des résultats Identifier et de la recherche d'images utilisaient
directement une URL distante, refusée par la politique `img-src 'self'` du Core.
Elles utilisent maintenant :

`GET /api/v1/admin/extensions/{id}/image-preview?workId=...&url=...`

Cette route exige une session administrateur, l'accès à l'œuvre et les droits
vivants de l'extension sur la bibliothèque. HTTPS/443 et les domaines déclarés
du paquet sont obligatoires ; IP privées et redirections sont refusées. Les
adresses DNS publiques sont épinglées pour le téléchargement. Les images PNG,
JPEG ou WebP sont vérifiées par signature et type MIME ; maximum 10 Mio,
quatre téléchargements simultanés, attente et réseau bornés. Aucun aperçu n'est
persisté dans la base ou sur disque. Les réponses privées ne sont pas mises en
cache HTTP. Les droits sont revérifiés avant envoi.

Le menu flottant Chapitres du lecteur est retiré. La barre présente leurs repères,
le titre au survol et un libellé accessible de la position courante. Les boutons
chapitre précédent/suivant restent disponibles. Les segments intro/générique
restent distincts des chapitres de conteneur.

La correction réelle des droits du volume de sauvegardes du NAS et la répartition
base/fichiers sont détaillées dans [le diagnostic sauvegardes](BACKUP_NAS_FIX_2026_11_02.md).
Les erreurs de sauvegarde renvoyées par étape sont aussi affichées par les deux
parcours Web. Un nouveau binaire déployé est nécessaire pour ces messages.

Vérifications ciblées : test Core de refus des aperçus non autorisés/hôtes privés,
tests sauvegardes, navigation et parcours planificateur ; Chrome avec API simulées
vérifie que les deux dialogues décodent leurs aperçus sous `img-src 'self'` sans
requête navigateur vers le fournisseur. Check Svelte, lint et build passent.
Ce dernier scénario ne constitue pas une preuve du téléchargement fournisseur
réel sur NAS.
