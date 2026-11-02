# Sauvegarde NAS : permissions et stockage — 2 novembre 2026

Le diagnostic SSH a trouvé `PermissionDenied` dans les logs de lecture de
`/backups`. Le Core tourne sous `1039:65539`, tandis que ce répertoire avait
pour propriétaire `10001:10001` et pour mode `0700`. `pg_dump 18.6` est présent.
Ce décalage empêche la lecture et la création des sauvegardes.

Les montages réellement inspectés sont les volumes Docker nommés
`aether_data` vers `/data` et `aether_backups` vers `/backups`. Sur ce NAS,
le second correspond à `/volume2/@docker/volumes/aether_backups/_data`.
Le dossier `data` à côté du Compose n'est pas monté sur `/data`.

Correction autorisée effectuée : changement non récursif du seul propriétaire
du répertoire du volume `aether_backups` vers `1039:65539`, mode `0700` conservé.
Le changement depuis `docker exec -u 0` a été refusé par les capacités du
conteneur ; la même opération ciblée a été effectuée par root SSH sur le chemin
du volume, après vérification du montage. L'ancien propriétaire était
`10001:10001`. Un petit fichier temporaire privé a ensuite été créé, relu et
immédiatement supprimé sous l'utilisateur effectif du service : résultat réussi.
Aucun média, session, autre volume ou conteneur n'a été modifié.

La création d'une archive complète via l'interface reste à retenter avec une
session utilisateur existante. Le diagnostic n'a créé aucune session artificielle.
Le code local affiche désormais une indisponibilité réelle lors d'un échec de
lecture et distingue les étapes d'échec de création ; ce code doit être déployé
pour bénéficier des nouveaux messages.

Le stockage des images est déjà sur fichiers : `user_avatars` contient
uniquement `user_id` et `sha256`, et les octets passent par `AssetStore`
(`MEDIA_ASSETS_PATH`, `/data/assets` dans l'image Docker). Les affiches et
autres images durables utilisent également les références SHA-256. Les archives
complètes incluent ces fichiers et les vérifient par empreinte et taille.

Inventaire NAS observé : base PostgreSQL environ 270 Mo, 1 043 œuvres, table
`assets` vide et aucun `user_avatars` référencé au moment du diagnostic ;
environ 18 Mo et 210 fichiers sont cependant présents sous `/data/assets`,
et les plugins occupent environ 3,8 Mo. Ces fichiers n'ont pas été supprimés
ni supposés tous référencés. Il n'y a donc pas de migration d'avatar effectuée.
Le schéma conserve des blocs binaires de 16 Kio dans `extension_object_chunks`
pour les objets privés des extensions, ainsi que des paquets Wasm en JSON/base64 ;
ces données restent incluses dans la sauvegarde SQL et ne sont pas des photos
de profil. Une base volumineuse ne prouve pas à elle seule un stockage d'images
en BLOB SQL.
