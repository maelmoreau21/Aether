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
Expiration absolue 12 h et après 30 min sans requête authentifiée, contrôlées
atomiquement dans SQLite ; une session expirée ne peut pas être réactivée.
Le compte désactivé est refusé au prochain accès authentifié.

Le jeton CSRF est dérivé du secret de session avec séparation de domaine ;
il est transmis par l'API, conservé uniquement en mémoire Web et doit être fourni
dans l'en-tête de déconnexion. Origin et Fetch-Site sont aussi contrôlés.
La connexion exige Origin afin d'empêcher le login CSRF. Aucune session ni
aucun jeton d'authentification n'est conservé dans localStorage.
Une déconnexion révoque la session courante, pas toutes les connexions du compte.

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
