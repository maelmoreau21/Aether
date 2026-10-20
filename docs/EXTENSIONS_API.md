# Extensions API v1 — premier parcours local

Contrat indépendant du protocole interne Node, implémenté par le Core existant.
Ce sous-ensemble permet une extension de propositions de métadonnées, installée
sans recompiler le serveur. Il ne livre pas les quinze sections de la plateforme.
Voir `EXTENSIONS_AUDIT.md` pour les écarts et `extensions-v1.openapi.json` pour le
contrat HTTP. SQLite reste le défaut; migrations PostgreSQL équivalentes.

## Paquet et identité

Le paquet est un objet JSON borné à 400 000 octets, sans archive à extraire :
`manifest`, `publisherId`, `wasm` (Base64 standard), `sha256` (hex minuscule),
`signature` (Ed25519, Base64 standard). Le manifeste Rust est dans
`crates/extension-sdk/src/lib.rs`; tous les champs sont obligatoires et les champs
inconnus sont refusés. Seule `permissions: ["metadata.read"]` est admise en v1.
Les identifiants comportent 1–80 caractères ASCII minuscules, chiffres, `._-`.
Version : trois composantes numériques. `apiVersion` doit valoir 1.

La signature couvre les octets UTF-8 de la sérialisation compacte serde JSON du
tuple `["media-extension-package-v1", manifest, publisherId, sha256]` avec les
champs du manifeste dans l'ordre du type Rust. Utiliser `Package::signing_bytes`
ou l'outil de paquetage; il ne s'agit pas d'une canonicalisation JSON universelle.
Le digest est recalculé sur les octets Wasm avant vérification de signature.
Une clé publique doit être approuvée par un administrateur via un canal externe
de confiance. Aucun éditeur préapprouvé, aucune clé privée dans le serveur.
Rotation : nouvel identifiant d'éditeur. Révocation : irréversible pour cet ID.

Le paquet est conservé en base applicative, hors médias; installation atomique
par insertion et unicité. Installation et activation restent séparées.
Une mise à jour en place est refusée : désinstaller puis installer et réaccorder
les droits. Pas de dépendances, migrations privées ni rollback annoncés.
La conservation des résultats à la désinstallation est un choix explicite.

## ABI Rust/Wasm

Compiler pour `wasm32-unknown-unknown`. Aucun import, accès WASI, réseau, disque,
SQL, secret ou appel hôte. Exporter `memory`, `media_alloc(u32)->u32` et
`media_run(u32,u32)->u64`. Le SDK fournit la macro générique `extension!`.
Le host écrit l'entrée JSON dans l'allocation puis appelle `media_run`.
Le résultat encode le pointeur dans les 32 bits hauts et la longueur dans les
32 bits bas. Toutes les lectures et écritures mémoire sont vérifiées par l'hôte.

Enveloppe générique entrante : `{apiVersion,operation,input}`; enveloppe sortante :
`{apiVersion,result}`. Le runtime ne dépend pas du type métier. Chaque adaptateur
Core autorise son opération avant invocation et valide le résultat ensuite.
L’adaptateur actuellement raccordé est `metadata.propose` : entrée métier
`{apiVersion,workId,libraryId,title}` issue du catalogue persistant, résultat
`{title,summary}`. Le résultat est une proposition avec provenance de
l'extension/version/digest, jamais une écriture automatique dans l'œuvre.
Les chaînes sont affichées comme texte échappé, jamais HTML tiers.

## Exécution et révocation

Wasmi 2.0.0 dans un worker temporaire distinct du Core; aucun chargement natif
d'extension. Un sous-processus natif seul laisserait au code invité les droits
de l'utilisateur système, raison de son exclusion pour ce parcours sans I/O.
Le choix ne prouve pas encore l'adéquation de Wasm/WASI pour FFmpeg ou les
empreintes audio; ces opérations Node demandent leur propre contrat contrôlé.

Limites fixes du premier runtime : module 256 Kio, entrée/sortie 32 Kio,
une mémoire linéaire de 16 Mio, une table de 4096 éléments, 2 millions d'unités
de fuel, délai maximal de 3 secondes et une invocation simultanée par Core.
La compilation et l'interprétation du module se font dans le worker, jamais
sur le chemin des requêtes de streaming. Les imports et fonctions start sont
refusés. L'environnement du worker est vidé, stderr supprimé; il est arrêté
si la requête disparaît. Le worker doit être installé à côté du binaire Core.

Les 16 Mio concernent la mémoire linéaire invitée, pas le RSS total du processus.
Ce parcours n'impose pas encore un quota RSS/CPU système configurable; fuel,
bornes structurelles et délai sont des contrôles complémentaires, pas cette
garantie. Le worker n'est pas un bac à sable OS contre une faille de l'interpréteur.

Chaque exécution vérifie session administrateur, bibliothèque active, Node non
révoqué, éditeur actif, extension activée et bibliothèque explicitement accordée.
Le rôle administrateur ne contourne pas le consentement d'extension. Les mêmes
états et la session sont revérifiés toutes les 30 ms et avant retour du résultat.
Un identifiant de génération unique empêche de réutiliser un résultat après
désactivation/réactivation ou désinstallation/réinstallation.

L'exécution est manuelle, courte, liée à la requête; l'historique est persistant.
Les 1 000 exécutions les plus récentes sont conservées; l'API en expose 100 à
la fois. Le journal administratif est séparé des résultats et sa conservation
n'est pas affectée par la suppression des données de l'extension.
Ce n'est pas une seconde file de tâches ni un second planificateur. Les tâches
longues et leur reprise doivent étendre les mécanismes existants dans un jalon
suivant. Les déconnexions Node n'interviennent pas dans ce calcul sur métadonnées
Core; aucun accès aux fichiers distants n'est revendiqué.

## Utilisation de la plateforme

Aucune extension officielle ou de démonstration n'est livrée, installée ou
activée par défaut. Les modules sous `tests/fixtures/extension-sdk` et les paquets
créés dans les tests sont exclusivement des fixtures de validation du SDK,
des signatures, des permissions et du runtime.

Un administrateur apporte le paquet signé d'un auteur tiers. Dans Administration
→ Extensions : approuver l'éditeur après vérification indépendante de sa clé,
importer le paquet JSON, choisir les bibliothèques et activer. L'écran permet
ensuite d'appeler les opérations métier réellement raccordées au Core.

Pour l'auteur d'un paquet, l'outil de signature est générique :

```powershell
cargo build -p media-poc --bin media-extension-pack --bin media-extension-worker -j 1
media-extension-pack keygen CHEMIN_PRIVE.pk8
media-extension-pack pack MANIFEST.json MODULE.wasm CHEMIN_PRIVE.pk8 IDENTIFIANT_EDITEUR PAQUET.json
```

Les chemins sont ceux de l'auteur, jamais des chemins NAS envoyés par le
navigateur. `keygen` affiche la clé publique et refuse d'écraser une clé existante.
Protéger la clé privée avec les permissions du compte opérateur; ne pas la
committer ni la transmettre au serveur. Le worker doit rester à côté du Core.
Contrats du SDK : `crates/extension-sdk/README.md`.

Les modes standalone et distributed hébergent cette même API sur Core. Cela
ne constitue pas encore une preuve de tâches exécutées sur un Node distant.

## Suite non livrée

Catalogue officiel et dépôts personnalisés, mises à jour/rollback, schémas de
configuration, hooks durables, traitements planifiés Node, I/O média contrôlées,
stockage privé généralisé, broker réseau/secrets, traductions FR/EN complètes,
Trickplay, Intro Skipper, fournisseurs externes et statistiques. Leur absence
est signalée dans l'administration; le fonctionnement du serveur n'en dépend pas.
