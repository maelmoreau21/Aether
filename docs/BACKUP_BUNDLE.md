# Sauvegarde complète chiffrée : paquet versionné

`backup_bundle::create` emballe un instantané cohérent de base de données et sa configuration dans un fichier chiffré. L'instantané doit déjà avoir été produit par le service de sauvegarde : SQLite cohérent ou archive PostgreSQL `pg_dump` au format custom. Ce module ne copie jamais directement la base active et ne prend aucune décision sur sa restauration dans une base en service.

La clé `MEDIA_BACKUP_KEY` doit contenir exactement 64 chiffres hexadécimaux, soit 32 octets aléatoires. Elle est fournie par l'opérateur, conservée séparément des paquets et indispensable pour toute récupération. Aucune cle publique de repli (voir BACKUP_KEY.md). Aucune clé de repli, génération temporaire perdue ou clé dans le paquet. Les configurations sensibles (par exemple mots de passe/clients OIDC) sont chiffrées. `MEDIA_BACKUP_KEY` est systématiquement retirée du manifeste. Le service appelant collecte la configuration effective ; le module stocke les valeurs reçues et ne découvre pas seul des fichiers externes, des secrets Docker ou des certificats.

## Format et limites

Le paquet v1 commence par le marqueur `MDBK0001` et un sel aléatoire de 32 octets issu du CSPRNG de `ring`. HKDF-SHA256 dérive une clé AES-256-GCM propre à chaque paquet depuis ce sel et la clé opérateur. Le nonce de chaque enregistrement est un compteur 64 bits préfixé par quatre octets nuls. Le débordement du compteur est refusé. Le chiffrement, l'authentification et la dérivation utilisent exclusivement `ring`, pas de primitive cryptographique maison.

Chaque enregistrement authentifie le header complet, son compteur, son type et sa taille. Un manifeste JSON chiffré contient `engine` (`sqlite` ou `postgres`) et `config`. Il est suivi de fragments de base de données de 64 Kio maximum et d'un enregistrement final vide authentifié. L'absence de fin, la troncature, la modification, la permutation, la répétition ou les octets supplémentaires font échouer l'extraction.

Le manifeste est limité à 64 Kio, chaque clé de configuration à 256 octets et chaque valeur à 16 Kio. Les configurations plus volumineuses échouent explicitement. Le flux de base de données n'est jamais chargé en totalité en mémoire : un fragment de lecture de 64 Kio et un fragment chiffré sont utilisés. Il ne s'agit pas d'une mesure RSS globale du processus.

## Extraction et publication

`extract(source, target_dir, key_hex)` exige une destination absente et un parent existant sans symlink ni point de réanalyse. L'extraction utilise un dossier temporaire frère : `0700` sur Unix, fichiers `0600`. Sous Windows, les ACL sont héritées du parent ; l'opérateur doit choisir un dossier privé. Le dossier temporaire contient nécessairement les données déchiffrées pendant leur authentification ; il n'est jamais exposé sous le chemin final avant validation complète, et il est nettoyé en cas d'échec. La confidentialité vis-à-vis d'un administrateur OS ou d'un utilisateur ayant accès au dossier parent relève des ACL de l'hôte.

Le paquet ne fournit aucun chemin de sortie : seuls `config.json` et `database.sqlite3` (SQLite) ou `database.dump` (PostgreSQL) sont écrits. Après authentification et synchronisation des fichiers, publication par renommage atomique sans remplacement : `renameat2(RENAME_NOREPLACE)` sous Linux, renommage sans remplacement sous Windows. Un autre OS est refusé explicitement. Une destination déjà présente, même vide, est conservée. La publication du paquet chiffré utilise un lien physique atomique sans remplacement sur le même volume, puis retire son nom temporaire.

Les parents doivent rester sous contrôle opérateur : le contrôle préalable des symlinks ne constitue pas une protection contre un acteur OS autorisé à remplacer simultanément des répertoires parents. Un crash brutal peut laisser un dossier temporaire privé à nettoyer manuellement. Aucun média personnel n'est inclus ou modifié.

## Tests

`cargo test -p media-poc --test backup_bundle` vérifie les deux moteurs sur des octets d'archive, configuration sensible chiffrée et exclusion de la clé, fragments multiples, mauvais secret, altération, troncature, permutation, données supplémentaires, cibles existantes, configuration excessive, sels différents et (Unix) droits/symlink parent. Ces tests de format ne remplacent pas les tests réels `pg_dump`/`pg_restore` et l'intégrité SQLite du service appelant. Les résultats effectivement exécutés sont consignés dans le jalon principal.

Le test de CA inclut sa r�cup�ration byte pour byte, le chemin fixe de sortie, la conservation du chemin original de configuration, la d�tection de son alt�ration et le refus de sauvegarde si le fichier configur� manque. Les certificats ne sont pas valid�s comme X.509 par le paquet : il conserve leurs octets ; la validation TLS reste assur�e par le transport au d�marrage.

Les tests PostgreSQL TLS couvrent tous les alias accept�s, la r�cup�ration des trois fichiers, la pr�servation exacte de la configuration originale, le chiffrement de la cl� priv�e et le refus d�un fichier manquant. La liste de pi�ces jointes et leurs tailles sont authentifi�es dans le manifeste, chaque fragment est authentifi� avec son type fixe, et toute taille finale manquante ou excessive est refus�e.

Les bases vides sont refus�es � la cr�ation et � l�extraction, y compris un paquet correctement authentifi� ne contenant que manifeste et fin. `MEDIA_BACKUP_KEY_FILE` est exclu comme `MEDIA_BACKUP_KEY` ; leur pr�sence dans un manifeste re�u est refus�e. Sous Unix, les dossiers sont synchronis�s apr�s publication du paquet, avant le renommage du dossier extrait et apr�s ce renommage pour assurer la durabilit� des entr�es de r�pertoire. Les garanties de persistance apr�s panne mat�rielle restent d�pendantes du syst�me de fichiers et du stockage.
