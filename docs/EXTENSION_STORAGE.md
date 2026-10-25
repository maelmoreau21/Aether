# Stockage privé des extensions — contrat v1

Mission du 25 octobre 2026. Le Core possède les connexions SQLx, l'identité de
l'extension et les autorisations. Le guest ne reçoit ni SQL, ni URL de connexion,
ni chemin disque. SQLite/WAL et PostgreSQL utilisent le même contrat SDK Rust.
Les migrations 0049/0050 ajoutent des tables dans la base existante, sans base par
plugin ni Redis. Elles ne transfèrent pas les installations SQLite vers PostgreSQL.

## Parcours vertical

Le guest Rust appelle `media_extension_sdk::storage::host_storage` ou
`objects::host_objects`. Imports Wasm strictement autorisés :
`media:media_host_storage_v1` et `media:media_host_objects_v1`. L'ABI historique
HTTP reste compatible. Le worker jetable dialogue avec le Core par ses pipes
privés, avec trames de longueur bornée. Aucun socket de stockage supplémentaire.
La permission signée `storage.private` s'ajoute aux capacités existantes et
réutilise activation, génération, éditeur, utilisateur et grants de bibliothèque.
Ce n'est pas un deuxième système de grants.

Le Core construit le contexte depuis l'appel autorisé : bibliothèque et œuvre,
épisode/saison dérivés du catalogue, compte authentifié. Le guest indique une
portée et son ID, mais ne peut changer les IDs du contexte. Un ID vide signifie
la ressource courante liée par l'hôte ; un ID explicite doit lui correspondre.
Portées disponibles :
`global` (ID vide, administrateur), `library`, `work`, `season`, `episode`, `user`
(son propre compte seulement). Un appel sans contexte de bibliothèque ne permet
pas d'en choisir une. Les portées sans contexte réel sont refusées.

À chaque opération, le moteur vérifie activation, génération, éditeur actif,
capacité du manifeste, compte actif, droits de bibliothèque directs ou par
groupe, état du Node et correspondance de portée. L'hôte revérifie également la
session. `get` regroupe autorisation et valeur dans une requête atomique ;
les listes utilisent une transaction de lecture ; les mutations
utilisent le verrou transactionnel existant (`BEGIN IMMEDIATE` ou advisory lock
PostgreSQL). Les données ne sont pas mises en cache en mémoire par extension.
La projection SQL du manifeste évite de transférer/désérialiser le Wasm à chaque
lecture. Une opération déjà autorisée est un instantané transactionnel ; aucune
promesse d'annuler rétroactivement une opération déjà validée.

## Petites valeurs et concurrence

`StorageRequest.apiVersion = 1`, opérations `get`, `put`, `delete`, `list`.
Clés non vides, 256 octets maximum ; valeurs UTF-8 jusqu'à 16 Kio, avec borne
supplémentaire de 32 Kio sur leur représentation JSON. `get` renvoie zéro ou une entrée.
`put.expected_version = None` crée seulement si aucune valeur vivante n'existe ;
pour remplacer, fournir la version opaque retournée par la lecture. Chaque
écriture produit une nouvelle version UUID, y compris après suppression, pour
éviter le problème ABA. Une version périmée produit un conflit, jamais un
écrasement silencieux. `delete` accepte une version conditionnelle ; sans version,
la suppression est idempotente et inconditionnelle. Les transactions libres et
les mutations atomiques sur plusieurs clés ne sont pas exposées : utiliser les
opérations conditionnelles.

L'atomicité porte sur chaque opération, pas sur toute l'invocation : des écritures
validées restent persistantes si le guest échoue ensuite ou perd son autorisation.

`expires_at` est une date Unix en secondes ; une entrée expirée n'est plus lue
ni énumérée. `list` utilise un curseur exclusif sur la clé, de 1 à 100 entrées,
et une limite de réponse de 48 Kio. La pagination n'est pas un instantané couvrant
plusieurs appels : une modification concurrente peut changer les pages suivantes.
SHA-256 et longueur sont contrôlés à la lecture ; une valeur altérée est refusée.

Limites par extension, toutes portées cumulées : 100 000 entrées, 64 Mio de
valeurs. Les compteurs sont transactionnels, sans calcul intégral à chaque
écriture. Les lignes expirées occupent leur quota physique jusqu'à réécriture,
suppression ou récupération bornée de 128 lignes sous pression de quota.
Le quota n'est pas une mesure exacte des octets SQLite/PostgreSQL : index, clés,
métadonnées, WAL et espace libre de la base s'ajoutent.

## Objets binaires distincts

Tables séparées de métadonnées et de blocs binaires : `begin`, `append`, `finish`,
`read`, `delete`. Références UUID générées par l'hôte ; elles ne sont pas des
capacités partageables entre extensions/portées. Maximum 16 Mio par objet,
128 Mio et 64 objets par extension, uploads en cours inclus. `begin` réserve la
taille annoncée. Les blocs font au plus 16 Kio, transportés en Base64.
`append` exige l'offset courant : deux écritures concurrentes au même offset ne
réussissent pas toutes les deux. `finish` vérifie la longueur et l'empreinte
SHA-256 totale en lisant un seul bloc à la fois. Une fois publié, l'objet est
immuable ; remplacement par nouvel objet puis CAS de la référence dans une clé.
Un objet de 16 Mio nécessite 1 024 blocs : l'upload peut s'étaler sur plusieurs
invocations, en conservant sa référence et son offset, sous sa durée de vie bornée.
La publication de l'objet et le CAS de la référence sont deux opérations : un
échec du CAS peut laisser un objet à supprimer explicitement.

Un upload incomplet expire après une heure. La récupération est déclenchée par
une nouvelle création d'objet. Les lectures vérifient l'empreinte de chaque bloc.
Un objet cache exige une expiration. Les objets durables ne sont pas effacés
implicitement ; une expiration explicitement demandée reste applicable.

Compromis : les blocs résident dans la base, pas dans les lignes de petites
valeurs ni dans des fichiers arbitraires. Les sauvegardes existantes incluent
donc les objets et PostgreSQL peut partager les octets entre Core. En contrepartie,
taille des sauvegardes, WAL, contention, bloat/vacuum doivent être surveillés.
Ce contrat borné n'est pas un stockage d'objets illimité ; un backend externe
pour de plus gros artefacts demanderait un jalon mesuré distinct.

## Budgets et cycle de vie

Au plus 128 appels de stockage/objets par invocation, avec entrée/sortie ABI de
64 Kio, mémoire Wasm 16 Mio et fuel existant. Délai par opération : 2 s pour KV,
5 s pour objets ; deadline globale du worker 65 s. Une erreur de délai peut
survenir près d'un commit : relire la version avant de réessayer. Le quota
d'opérations est par invocation, pas un débit global par heure entre plusieurs
Core. L'admission existante des exécutions reste bornée.

- Désactivation/révocation : accès refusé, données conservées.
- Désinstallation `deleteData=false` : résultats durables conservés, caches et
  uploads incomplets supprimés ; compteurs recalculés transactionnellement.
- Désinstallation `deleteData=true` : suppression explicite des valeurs, objets
  et références de namespace, en plus de l'historique concerné.
- Mise à jour : le produit conserve son parcours désinstallation/réinstallation,
  réactivation et nouveaux grants ; pas de migration privée automatique ni
  rollback promis. Réinstaller avec même ID/éditeur/clé retrouve les résultats
  conservés. Un éditeur ou une clé différente ne peut les lire ; une purge
  explicite est nécessaire avant réutilisation de ce namespace.

Les deux moteurs offrent CAS et compteurs persistants, sans état local autoritaire.
Le verrou d'écriture PostgreSQL actuel est commun aux mutations du Core : il
prépare la cohérence, mais peut limiter le débit. Cela ne prouve pas un cluster
multi-Core complet, sa révocation globale ou sa disponibilité.

## Reproduction

Extension Rust synthétique hors catalogue :
`Aether-Plugins/templates/persistent-storage`, compteur durable avec CAS, sans
réseau. Compiler avec `cargo build --manifest-path
templates/persistent-storage/Cargo.toml --target wasm32-unknown-unknown --release`
depuis Aether-Plugins. Le manifeste demande `metadata.read` et `storage.private`.
Le paquet doit suivre le parcours de signature normal pour être installé.

Les tests synthétiques ne prouvent ni un NAS ni une charge de production.

## Validation locale du 25 octobre

Windows 11 10.0.26300, Intel Core Ultra 7 256V (8 cœurs), Rust 1.99.1.
PostgreSQL 18 dans un conteneur temporaire, base en tmpfs et port loopback ;
SQLite/WAL sur le disque local. Profil debug, pool SQLx de quatre connexions.

Commandes exécutées :

```powershell
cargo test --locked -p media-poc --features query-metrics --lib extension_data::tests -j 1 -- --nocapture
cargo test --locked -p media-poc --test extensions -j 1 -- --nocapture --test-threads=1
cargo clippy --locked -p media-poc --features query-metrics --all-targets -j 1 -- -D warnings
cargo fmt --all --check
```

Sept tests moteur SQLite et le contrat commun PostgreSQL passent. Les tests
d'objets passent : trois SQLite, un PostgreSQL. Le contrat partagé vérifie les
six portées, CAS concurrent, grants, révocation, quotas, expiration, corruption
et identité d'éditeur. Les trames tronquées ou trop grandes sont refusées.
Les projections SQL bornent également l'allocation sur valeurs persistées
anormalement grandes, avant validation de leur taille et de leur empreinte.

Les treize autres intégrations d'extensions passent ; quatre tests ciblés finaux
passent avec `--features query-metrics --test extensions storage --include-ignored`
et `MEDIA_TEST_STORAGE_WASM` pointant vers le guest compilé : installation HTTP
signée, refus de `storage.sql`, activation séparée, deux vrais appels du SDK Rust
avec compteur durable, nouveaux workers/réouverture DB et refus HTTP 429 au
129e appel du guest malveillant. Le test complet avait d'abord révélé un champ
obligatoire absent de la fixture SQL ; corrigé avant ces quatre succès finaux.

Cycle de vie : deux tests SQLite et le scénario PostgreSQL explicite passent.
Ils couvrent valeurs et objets : rétention durable, suppression des caches et
uploads incomplets, purge explicite, timeout réel vers 2 s sous verrou puis
reprise sans écriture partielle. Deux processus séparés quittent brutalement
avec le code 79, sans destructeurs, avant/après commit : persistance et rollback
sont vérifiés après réouverture SQLite et PostgreSQL. Cela teste un crash du
client/Core, pas un crash du serveur PostgreSQL, une coupure électrique ou une
panne de disque. La source demeure figée durant le benchmark final.

Le premier benchmark authentifié sur 100 000 entrées, avant regroupement de
`get` en une requête, donnait SQLite p50 1,094 ms / p95 1,698 ms à un lecteur :
cible manquée. Preuve conservée dans
`evidence/2026-10-25/extension-storage/sqlite-baseline.log`. Le benchmark utilise
deux extensions avec le vrai guest Rust de 131 331 octets dans chaque paquet,
100 000 écritures via l'API moteur, puis lectures et mélange 90 % lectures /
10 % CAS à un et quatre clients. Il ne raccourcit pas les contrôles d'accès.

Reproduction de la mesure finale avec un PostgreSQL jetable fourni par
`MEDIA_TEST_POSTGRES_URL` :

```powershell
./scripts/measure-extension-storage.ps1 -Postgres -WasmPath ../Aether-Plugins/templates/persistent-storage/target/wasm32-unknown-unknown/release/synthetic_persistent_extension.wasm
```

Les temps SQLx incluent pool, contrôles et SHA-256, mais excluent compilation,
Wasm et IPC worker. PostgreSQL inclut aussi le transport loopback. Une mesure
`EXPLAIN ANALYZE` séparée réutilise exactement la requête et ses paramètres :
elle exclut réseau, SQLx et validations Rust et ne constitue donc pas une
mesure complète du moteur. Les caches sont naturellement réchauffés, sans
cache de valeurs en mémoire ni purge du cache OS. RSS/CPU sont ceux du
processus de benchmark entier, chargement inclus, serveur PostgreSQL exclu.

SQLite final, 100 000 entrées réellement créées :

| Charge | Clients | p50 lecture | p95 lecture | p99 lecture | p95 écriture CAS |
|---|---:|---:|---:|---:|---:|
| Lecture | 1 | 0,787 ms | 2,160 ms | 7,061 ms | — |
| Lecture | 4 | 1,252 ms | 6,124 ms | 13,050 ms | — |
| 90 % lecture / 10 % CAS | 1 | 0,778 ms | 1,386 ms | 1,988 ms | 3,985 ms |
| 90 % lecture / 10 % CAS | 4 | 1,037 ms | 8,255 ms | 28,470 ms | 113,812 ms |

Un statement par lecture confirmé par instrumentation. La médiane unitaire
passe sous 1 ms, mais la cible n'est pas tenue au p95 ni sous concurrence.
Les latences de queue régressent par rapport au premier passage : ces deux
mesures Windows/debug sur hôte non dédié ne démontrent pas une amélioration
générale. Aucune extrapolation Release ou NAS. Pic RSS échantillonné du processus
26,60 Mio, CPU échantillonné160,94 s, initialisation100000 valeurs incluse.
Preuves complètes et empreintes sources/binaire :
`evidence/2026-10-25/extension-storage/sqlite-final*`.
