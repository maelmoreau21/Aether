# Correctifs du parcours NAS — 2 novembre 2026

Diagnostic SSH en lecture seule : Core `0.2.0-beta.21`, PostgreSQL, quatre
extensions encore en `0.1.0`. Les plugins sont bien dans le volume Docker
`aether_data`, chemin NAS `/volume2/@docker/volumes/aether_data/_data/plugins`.
Le dossier `data` à côté du Compose n'est pas monté sur `/data`.

Un résultat TMDb vide passe, une recherche Inception fait quitter le worker
sans réponse. Le module déployé et la vraie réponse reproduisent un dépassement
du budget Wasm de 2 millions ; 50 millions renvoient les 12 résultats. Le Core
utilise désormais un plafond de 200 millions, toujours avec mémoire, délais et
nombre de requêtes bornés. Les erreurs du worker sont remontées explicitement.
TMDb est interrogé avant les fournisseurs de repli. Les détails TMDb ne demandent
plus toutes les traductions et images dans la même réponse ; les images disposent
de leur opération dédiée. OMDb renvoie `unauthorized` sur le NAS : sa clé est
refusée ; cela n'empêche pas TMDb de fonctionner.

Une affiche réellement présente sur le NAS est passée de 855658 à 577606 octets
après le scan. Le Core rejetait son ancienne empreinte. La lecture locale
revalide maintenant uniquement cette image via le Node, actualise sa taille,
son empreinte et son MIME, puis réessaie. Les fichiers invalides, liens hors
racine et images dépassant les limites restent refusés. Aucun média n'est écrit.
Le bandeau essaie l'image originale si sa miniature est indisponible.

Publication plugins : voir le README du dépôt voisin. Les nouveaux paquets
`develop` sont construits/signés après la CI et leurs versions incrémentées.
Le catalogue officiel préconfiguré suit une fois le canal bêta du Core ; les
réglages personnalisés et suppressions de dépôts sont préservés. Les mises à jour
en place des fournisseurs sans stockage privé conservent les paramètres et
sauvegardent l'ancien paquet dans `.revisions`.

Aucun conteneur, secret, réglage ou média du NAS n'a été modifié pendant le
diagnostic. Ces changements doivent être envoyés sur GitHub puis les nouvelles
images/versions déployées avant de pouvoir annoncer le NAS corrigé.
