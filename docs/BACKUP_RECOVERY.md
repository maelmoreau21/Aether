# Récupération d'une sauvegarde complète

Le parcours comprend deux opérations distinctes : extraire la sauvegarde dans
un dossier de récupération privé, puis restaurer hors ligne la base et remettre
en place la configuration après examen. `media-recover` réalise la première
opération seulement ; il ne modifie ni l'environnement du Core ni sa base active.

## Préparer la récupération

Le format courant `MDBK0002` est non chiffré : protéger strictement l'archive
et ses copies. `media-recover` ne prend aucune clé et refuse les anciennes
archives `MDBK0001`. Leur récupération exige un ancien binaire et la clé
d'origine ; cette suppression ne convertit ni n'efface les archives existantes.

Utiliser un poste et un dossier contrôlés par l'opérateur. L'archive contient la
base applicative et une configuration pouvant inclure des secrets : sessions,
identités, réglages OIDC, identifiants de connexion et secrets de Nodes sont des
données sensibles. Le dossier de récupération ne doit pas être exposé par HTTP,
monté dans une bibliothèque média ou synchronisé dans un emplacement public.

Le chemin de destination doit désigner un nouveau dossier, avec son parent
existant. L'extraction refuse d'écraser un dossier ou des fichiers existants.
Sous Windows, contrôler les ACL héritées du parent pour réserver l'accès au
compte opérateur ; des permissions Unix ne démontrent pas une isolation ACL NTFS.

Exemple PowerShell, à partir de binaires construits :

```powershell
media-recover 'D:\Sauvegardes\serveur.media-backup' 'D:\Recuperation\serveur-2026-10-19'
```

Cette commande ne remplace pas la protection de la session opérateur et du
stockage des archives. Le nom et la date du dossier sont des exemples.

## Examiner le contenu extrait

Une extraction réussie fournit `config.json` et la sauvegarde de base correspondant
au moteur annoncé par l'archive : `database.sqlite3` pour SQLite ou `database.dump`
pour PostgreSQL. Le fichier de configuration sert de référence pour rétablir les
variables documentées dans les outils d'exploitation ; ce n'est pas un script à
exécuter ni un fichier shell à sourcer.

Si `MEDIA_INTERNAL_CA_FILE` était configuré et son certificat inclus, l'extraction
fournit aussi `internal-ca.pem`. La configuration conserve son ancien chemin :
remonter le certificat à ce chemin ou régler explicitement
`MEDIA_INTERNAL_CA_FILE` vers la copie restaurée avant de démarrer le Core.
Les certificats PostgreSQL configurés dans l'URL peuvent également être extraits :
`postgres-ca.pem` pour la CA (`sslrootcert`/`ssl-root-cert`/`ssl-ca`),
`postgres-cert.pem` pour le certificat client (`sslcert`/`ssl-cert`) et
`postgres-key.pem` pour sa clé privée (`sslkey`/`ssl-key`). L'URL sauvegardée
conserve les chemins originaux : remonter ces fichiers à leurs anciens chemins
ou adapter explicitement les paramètres de `MEDIA_DB_URL` avant la restauration
et le redémarrage. Protéger particulièrement la clé privée. Les autres secrets
sur fichiers non pris en charge restent à rétablir séparément.

Examiner la configuration localement, sans l'imprimer dans les logs. Adapter les
chemins de volumes, le nom du service PostgreSQL, l'URL publique, les URLs de
redirection OIDC et la connectivité des Nodes au nouvel hôte. Ne pas importer
aveuglément un chemin de l'ancien serveur ni écraser les secrets du serveur
encore en production. Les médias et l'infrastructure externe ne sont pas inclus
dans une sauvegarde de base/configuration.

## Restaurer SQLite hors ligne

Arrêter le Core avant la restauration. Choisir un nouvel emplacement de base,
hors bibliothèque média. La commande existante `media-restore` applique les
vérifications d'intégrité et de schéma avant publication et refuse un remplacement
non autorisé. Exemple :

```powershell
$env:MEDIA_DB_ENGINE = 'sqlite'
media-restore 'D:\Recuperation\serveur-2026-10-19\database.sqlite3' 'D:\Donnees\core-restaure.sqlite3'
```

Configurer ensuite `MEDIA_DB_PATH` et les autres variables nécessaires dans le
service, en conservant une copie privée de sa configuration précédente. Démarrer
le Core, vérifier connexion administrateur, bibliothèques, permissions et
connectivité des Nodes avant de remettre les utilisateurs en service.

## Restaurer PostgreSQL hors ligne

Arrêter le Core. Préparer une base PostgreSQL vide et les outils clients requis
par la procédure existante. Configurer `MEDIA_DB_URL` dans l'environnement protégé
du processus ; ne pas placer une URL contenant un mot de passe dans l'historique
de commandes ni dans des logs. Puis :

```powershell
$env:MEDIA_DB_ENGINE = 'postgres'
media-restore 'D:\Recuperation\serveur-2026-10-19\database.dump'
```

La cible vient de `MEDIA_DB_URL` et doit être vide ; la restauration ne convertit
pas SQLite en PostgreSQL. Remettre en place les paramètres examinés et vérifier
le serveur restauré avant remise en service.

## Limites et validation

`media-recover` ne démarre pas de service, ne remplace pas un volume Docker et ne
relance pas les Nodes. La vérification du checksum ne prouve pas à elle seule que
la configuration convient à la nouvelle infrastructure. Les paramètres externes
du reverse proxy, de l'IdP, du gestionnaire de secrets et les médias demeurent
sous responsabilité opérateur.

Les tests automatiques vérifient séparément intégrité du format et du checksum de
l'archive, refus d'écrasement et chemins confinés. Un exercice réel de restauration
sur un environnement distinct reste la preuve de reprise complète ; aucun
résultat NAS ou PostgreSQL réel n'est déduit de la seule présence du binaire.
