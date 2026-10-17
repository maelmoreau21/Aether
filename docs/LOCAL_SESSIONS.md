> Mise à jour P1 bibliothèques : droits explicites SQLite et administration
> session/rôle/CSRF livrés sur fixtures. Les restrictions administrator seule
> ci-dessous sont historiques. Voir [LIBRARIES_AND_SCANNER.md](LIBRARIES_AND_SCANNER.md).

# Connexion et sessions locales — P1

Ce jalon réutilise le compte créé par `media-bootstrap` et SQLite. Aucun compte
par défaut, inscription publique, OIDC, multi-Node ou média personnel ajouté.
Le lecteur et les fichiers synthétiques restent les mêmes.

## Contrat HTTP

| Route | Comportement |
|---|---|
| `POST /api/v1/auth/login` | JSON `{login,password}`, Origin exact requis ; réponse `{user:{id,login,role},csrfToken}` et cookie |
| `GET /api/v1/auth/me` | Même identité/CSRF ; `401` sans session valide |
| `POST /api/v1/auth/logout` | Origin exact et `x-csrf-token` requis ; suppression SQLite, cookie expiré, `204` |

Mauvais secret, compte absent ou désactivé : même `401 invalid_credentials`.
Un compte absent déclenche aussi une vérification Argon2id factice. Taille JSON
limitée à 4096 octets, identifiant à 64 octets, secret à 512 octets.
Deux calculs Argon2 au maximum simultanément ; budget global persistant de
10 tentatives par fenêtre de 60 secondes, indépendamment du login. Le dépassement
renvoie `429`. Cette politique locale peut bloquer temporairement tout le monde
si un acteur épuise le budget ; elle ne constitue pas une défense réseau distribuée.

## Session et CSRF

À chaque connexion, nouveau jeton aléatoire composé de deux UUID v4 OS
(244 bits aléatoires). SQLite ne conserve que SHA-256 du jeton, user_id et dates.
Cookie `media_session`, `Path=/`, `HttpOnly`, `SameSite=Strict`, durée maximale
12 heures ; `Secure` pour une origine HTTPS. HTTP est limité au loopback.
L'origine locale par défaut est `http://127.0.0.1:MEDIA_PORT` (3000 par défaut).
`MEDIA_PUBLIC_URL` permet de déclarer une origine exacte sans chemin, par exemple
`https://media.example.test` ; Host et Origin doivent correspondre à cette
configuration. Les en-têtes Forwarded/X-Forwarded-* ne sont pas utilisés pour
décider des cookies. Le Core ne termine pas TLS et reste lié au loopback :
HTTPS nécessite une terminaison TLS de confiance avec Host conservé, à valider
dans un futur jalon de déploiement. Aucun proxy n'a été installé ou publié ici.
Expiration absolue 12 h (`43 200 s`) et expiration par inactivité après 30 min (`1 800 s`)
sans requête authentifiée, contrôlées à chaque requête directement dans SQLite via un
`SELECT` joint (`sessions`, `users`, `local_credentials`) en mode WAL sans verrou d'écriture ;
aucun cache d'autorisation positif n'est utilisé, de sorte qu'une déconnexion, révocation,
modification de rôle ou désactivation de compte (`disabled_at IS NOT NULL`) est effective
immédiatement dès la requête suivante.

La mise à jour d'activité (`last_seen_at`) est découplée de la lecture d'authentification :
- Un worker unique par `AuthState` regroupe les rafraîchissements par lots (`TOUCH_BATCH_SIZE = 64`)
  depuis une file en mémoire bornée (`MAX_PENDING_TOUCHES = 4096`) avec cadence de coalescence
  par session (`TOUCH_INTERVAL_SECONDS = 60 s`) et micro-fenêtre de regroupement (`20 ms`).
- L'`UPDATE` SQLite est strictement conditionnel (`expires_at > unixepoch() AND last_seen_at > unixepoch() - 1800 AND disabled_at IS NULL`) :
  une session déjà expirée ou révoquée ne peut jamais être prolongée ni ressuscitée.
- Sous verrou d'écriture SQLite prolongé (ex. `BEGIN IMMEDIATE` maintenu 30 s lors d'une publication),
  le worker utilise un `busy_timeout` court (`15 ms`) et un backoff (`150 ms`) sans immobiliser
  les connexions de lecture du pool ; les lectures authentifiées (`/auth/me`, catalogue, descripteurs,
  streaming Range) répondent immédiatement sur le snapshot WAL.
- **Compromis explicite sur l'expiration glissante (choix fail-closed)** : en raison du seuil de
  coalescence de 60 s et de la persistance différée, un arrêt brutal avant flush ou un verrou
  d'écriture SQLite couvrant les toutes dernières secondes des 30 minutes d'inactivité ne prolonge
  pas la session en mémoire au-delà du `last_seen_at` commité dans SQLite. Ce choix fail-closed
  peut avancer l'expiration inactive d'au plus 60 s (ou de la durée du verrou si la session était
  déjà à moins de 30 s de l'échéance), mais garantit qu'aucune session expirée ou révoquée n'est
  jamais acceptée ni réactivée.

Le jeton CSRF est dérivé du secret de session avec séparation de domaine ;
il est transmis par l'API, conservé uniquement en mémoire Web et doit être fourni
dans l'en-tête de déconnexion. Origin et Fetch-Site sont aussi contrôlés.
La connexion exige Origin afin d'empêcher le login CSRF. Aucune session ni
aucun jeton d'authentification n'est conservé dans localStorage.
Une déconnexion révoque la session courante, pas toutes les connexions du compte.

Côté frontend (`apps/web/src/lib/auth/session.ts` et `apps/web/src/lib/catalog/api.ts`),
le cache en mémoire des descripteurs médias (`descriptorCache`, borné à 128 entrées) est
strictement isolé par une génération de session monotone (`sessionGeneration`) :
- Toute connexion (`login`), déconnexion (`logout`), expiration (`HTTP 401`) ou modification
  du compte authentifié (`readSession`, `session.set`) incrémente la génération et purge
  immédiatement le cache des descripteurs.
- Les réponses tardives de requêtes ou préchargements initiés sous une génération antérieure
  (ou avant un `clearDescriptorCache()`) sont ignorées à leur retour et ne peuvent jamais
  repeupler le cache de la nouvelle session.
- Un `HTTP 401` issu d'une ancienne génération reçu après une reconnexion est ignoré et ne
  déconnecte pas la nouvelle session.
- Un `HTTP 403` invalide immédiatement les descripteurs en cache pour la génération courante.
- Le cache sert uniquement à accélérer l'affichage des libellés de versions et le préchargement
  au survol/focus ; chaque lancement de lecture (`revalidateMediaDescriptor`) revalide les
  droits auprès du serveur avant d'ouvrir le lecteur vidéo.

## Permissions et isolation P0

Le constructeur `secure_app` du binaire impose une session puis le droit de
lecture **avant** les handlers catalogue, descripteurs, fichiers, sous-titres,
MSE/DASH et leurs dérivés. Même contrôle pour GET, HEAD et Range : `401` sans
session ; `403` pour un compte `user` sans droits. L'administrateur peut lire
l'unique inventaire synthétique actuel. Les deux rôles restent exclusivement
`administrator` et `user`.

Les droits par bibliothèque et par média ne sont pas encore stockés : aucun
utilisateur ordinaire n'a de permission implicite et aucune bibliothèque réelle
ne peut être connectée. L'administration Web est une mention réservée au rôle,
sans écran de gestion. Les futures ressources devront être liées explicitement
à une bibliothèque et vérifiées sur chaque chemin de lecture.

Le constructeur historique sans authentification n'est compilé qu'avec la
feature explicite `fixture-bench` pour les tests P0. Le binaire utilise toujours
`secure_app`, même compilé avec cette feature ; aucune variable d'environnement
ne désactive son authentification.

## Limites

Le service reste lié à 127.0.0.1. Pas de déploiement NAS/LAN/public validé, pas de
garantie contre un opérateur OS hostile. Les ACL du dossier SQLite et les limites
TOCTOU du banc restent applicables. Un flux HTTP déjà autorisé n'est pas interrompu
au milieu de son corps après logout/expiration : les prochaines requêtes sont
refusées. Une lecture continue sans nouvelle requête peut dépasser le délai idle.
Les sessions expirées sont nettoyées à la connexion suivante, sans tâche de fond.
Pas d'interface de révocation globale ou de récupération de compte dans ce jalon.

Référence de conception : [OWASP Session Management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).
Les preuves locales et commandes exécutées sont consignées dans `MEMORY.md`.

## Rejouer les vérifications

Depuis la racine, avec les fixtures synthétiques déjà préparées :

```powershell
cargo fmt --check
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo test --locked --all-features
npm --prefix apps/web run check
npm --prefix apps/web run lint
npm --prefix apps/web test
npm --prefix apps/web run build
cargo test --locked --test browser_auth -- --ignored --nocapture
```

Le dernier test utilise Chrome/Playwright installés, une base temporaire et un
compte synthétique jetable. Ne pas lancer simultanément d'autres tests dans ce
même processus de test : il positionne le répertoire courant pour ServeDir.
`core_restart` démarre réellement trois processus du binaire : session conservée
après arrêt/reprise, puis révocation conservée après un second redémarrage.
Les fixtures et les bases réelles de l'opérateur ne sont pas modifiées.
