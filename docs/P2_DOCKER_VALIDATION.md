# Fiabilisation Docker P2 — 17 octobre 2026

Résultats réellement exécutés et mesures : [P2_DOCKER_RESULTS.md](P2_DOCKER_RESULTS.md).

Les deux déploiements restent distincts : `docker-compose.yml` pour un conteneur
standalone ; `compose.distributed.yaml` pour un Core et un Node polyvalent (ou multi-nodes). Backend Rust,
frontend SvelteKit statique. Aucun OIDC, P3 ni déploiement NAS dans ce jalon.

## Contrat de transport

Toutes les connexions sortantes passent par `media-node::transport::connect` :
appairage, contrôle signé, callbacks, rotation et relais. HTTPS négocie réellement
TLS 1.2 minimum, vérifie chaîne et nom d'hôte via le magasin système. Une CA PEM
supplémentaire peut être montée `:ro` et référencée par `MEDIA_INTERNAL_CA_FILE`.
Aucun contournement de validation ni repli HTTPS vers HTTP.

Les URL sont des origines sans userinfo, chemin, query ou fragment. Les clients
ne suivent aucune redirection. Hors loopback, chaque `host:port` doit figurer
exactement dans `MEDIA_INTERNAL_ALLOWED_AUTHORITIES` (séparateur virgule).
Les résultats DNS sont vérifiés puis utilisés directement, sans seconde
résolution ; destinations link-local/multicast/indéfinies interdites, y compris
IPv4 encapsulé dans IPv6. Autoriser une origine signifie faire confiance à son
opérateur et à sa résolution DNS. Cette liste est une configuration serveur,
pas une valeur fournie par le navigateur.

HTTP hors loopback exige en plus `MEDIA_INTERNAL_TRUSTED_HTTP=true`. Le Compose
mono-hôte configure explicitement cette confiance pour son bridge et ses trois
autorités fixes. Ce choix ne convient pas à une liaison entre machines : utiliser
HTTPS, même sur le LAN. Un proxy TLS est une frontière de confiance ; son dernier
saut HTTP doit rester isolé et local au serveur destinataire.

Les réponses JSON signées exigent un MAC incluant SHA-256 des octets réellement
reçus, le nonce de requête, le statut, le Node, la clé et le temps. Absence ou
altération échoue. Appairage et rotation vérifient aussi leur preuve mutuelle.
Les réponses JSON sont bornées à 8 Mio. La signature du relais lie les métadonnées
de représentation ; **les octets du flux sont protégés par TLS**, pas par un MAC
par segment. Sur un bridge HTTP déclaré fiable, leur intégrité dépend donc de
cette confiance réseau. Le flux reste borné et n'est jamais collecté intégralement.
Les anciens clients utilisant le MAC sans digest doivent être mis à jour avec
Core et Nodes ensemble ; aucune compatibilité silencieuse avec ce MAC faible.

## Capacités et révocation

Chaque nouveau GET/HEAD/Range direct vérifie signature, clé, Node, asset/type,
action, bibliothèque exacte de l'inventaire et expiration. Le Node demande
ensuite une autorisation interne signée au Core : session SQLite encore vivante,
compte actif, droits actuels, bibliothèque active appartenant au Node, clé et
Node non révoqués. Aucun cache positif. Core indisponible = refus de lecture
directe. La capacité contient le hash de session, jamais son cookie brut.

Déconnexion, retrait de droits, désactivation de bibliothèque/compte ou révocation
de session prennent effet à la **prochaine requête**, même avec une ancienne URL.
Rotation de clé invalide les anciennes capacités ; révocation Core reste efficace
si sa notification au Node hors ligne échoue. Comme P1, un corps déjà ouvert et
les octets déjà reçus ne sont pas interrompus ni repris. Le TTL reste 15 minutes,
mais ne remplace plus le contrôle vivant. Les capacités expirent à `now >= exp`.

Le secret d'amorçage est utilisable seulement pour une identité jamais appairée.
Un Node appairé se reconnecte avec son secret actuel. Une récupération après perte
de l'identité requiert une intervention opérateur ; ne pas effacer son volume de
données automatiquement. Rotation/révocation ne signalent plus de succès si la
persistance échoue ; remplacement du fichier d'identité par renommage atomique.

`<video>` utilise une URL porteuse `?token=...` : toute personne qui la possède
peut la rejouer dans les limites de la session et des droits. Ne pas enregistrer
la query dans les journaux du proxy, analytics, captures réseau ou rapports.
Les rapports de navigateur de ce jalon retirent la query. Ne pas utiliser
`docker compose config` ou `docker inspect` sans filtrage dans des logs publics :
les secrets d'environnement seraient visibles à l'administrateur Docker.

## Reproduction locale jetable

```powershell
cargo fmt --check
cargo clippy --locked --workspace --all-features --all-targets -- -D warnings
cargo test --locked --workspace --all-features
python scripts/generate_p2_tls_tests.py
$env:MEDIA_TLS_TEST_DIR = "$PWD/target/p2-tls-tests"
$env:MEDIA_INTERNAL_CA_FILE = "$PWD/target/p2-tls-tests/ca.pem"
cargo test --locked -p media-node --test internal_transport -- --include-ignored
docker build --target runtime -t aether:p2-qa .
docker build --target runtime-core -t aether-core:p2-qa .
docker build --target runtime-node -t aether-node:p2-qa .
python scripts/test_p2_docker.py
```

Le banc Windows utilise le pipe Engine pour piloter le vrai bootstrap TTY sans
afficher le mot de passe. Prérequis : Docker Linux, Python + `cryptography`,
Node.js, dépendances Web installées, Chrome et fixtures synthétiques générées.
Il crée exclusivement des noms `p2qa-<aléatoire>`, volumes et fichiers sous
`target/`. Les réussites nettoient leurs conteneurs/volumes ; un échec les conserve
pour diagnostic. Aucun conteneur ou volume utilisateur existant n'est remplacé.
Les certificats, clés privées et secrets générés ne doivent pas être ajoutés à Git.

Les mesures cgroup incluent cache fichier et processus ffprobe. Le pic cgroup
est depuis le démarrage ; le pic échantillonné concerne la phase, avec une cadence
de meilleur effort (Docker exec + 200 ms). CPU = delta usage_usec / temps mural,
100 % représente un cœur. Réseau = delta `/proc/net/dev`, contrôle et streaming
inclus ; ce n'est pas un débit soutenu de production. Les fixtures courtes ne
représentent pas un long métrage ou un NAS. Voir les résultats exécutés associés.

## Image Node

Le runtime conserve le binaire Debian `ffprobe` et sa fermeture complète de
bibliothèques dynamiques. Les codecs et démuxeurs ne sont pas recompilés.
Les pilotes graphiques non liés, LLVM, utilitaires FFmpeg et données inutilisées
par le probe ne sont plus copiés. L'image standalone conserve FFmpeg pour sa
compatibilité existante. Cette copie de bibliothèques impose de **reconstruire
l'image** pour leurs mises à jour ; le seul catalogue dpkg du runtime ne décrit
pas ces bibliothèques copiées. Conserver l'inventaire du stage de construction
pour les audits de dépendances. Aucun dispositif GPU ni privilège ajouté.
