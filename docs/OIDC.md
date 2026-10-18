# SSO OpenID Connect — Authentik

Implémentation Core commune aux images standalone (`aether`) et distribuée
(`aether-core`). Le login local et son administrateur de secours restent actifs.
Aucun secret, code ou jeton OIDC n'est envoyé aux Media Nodes ou stocké dans
localStorage. Le Node continue à vérifier les sessions Core et permissions vivantes.

## Configuration Authentik

1. Créer une application et un fournisseur **OAuth2/OpenID Provider** confidentiel.
2. Choisir Authorization Code, une clé de signature **RSA / RS256**, et le mode
   issuer **par fournisseur**. Copier exactement l'issuer annoncé par la découverte,
   avec son slash final : `https://auth.example.test/application/o/media/`.
3. Enregistrer une seule URI de redirection **stricte**, sans joker :
   `https://media.example.test/api/v1/auth/oidc/callback`.
4. Associer les scopes `openid`, `profile`, `email` ; inclure un mapping `groups`
   dans l'ID token, sous forme de tableau de noms de groupes. Autoriser l'application
   avec les politiques/groupes voulus. Les comptes désactivés doivent être refusés
   par Authentik lors de l'autorisation et de l'échange de code.
5. Configurer sur **Core seulement** :

```dotenv
MEDIA_PUBLIC_URL=https://media.example.test
MEDIA_OIDC_ENABLED=true
MEDIA_OIDC_ISSUER=https://auth.example.test/application/o/media/
MEDIA_OIDC_CLIENT_ID=client-id-fourni-par-authentik
MEDIA_OIDC_CLIENT_SECRET=secret-confidentiel-fourni-par-authentik
MEDIA_OIDC_USER_GROUPS=media-users,media-operators
```

Les deux Compose transmettent ces variables au Core. `.env` doit être protégé,
non versionné ; ne jamais publier `docker compose config` contenant un vrai secret.
Redémarrer le Core après modification. Une configuration activée mais incomplète
empêche le démarrage, avec une erreur sans secret. Une panne du fournisseur
pendant l'utilisation ne bloque pas la connexion locale.

Le Core calcule l'URI de callback depuis l'origine exacte `MEDIA_PUBLIC_URL` ;
aucune URI de retour n'est acceptée du navigateur. Le reverse proxy HTTPS conserve
Host et termine TLS. HTTP n'est accepté que sur loopback pour les essais locaux.
Les certificats du fournisseur doivent être approuvés par le système du Core.
Les endpoints découverts doivent partager l'origine de l'issuer ; aucun suivi de
redirection HTTP. Le mode issuer global Authentik n'est pas pris en charge : son
URL de découverte ne se déduit pas de l'issuer comme en mode par fournisseur.

Documentation de référence :
[provider Authentik](https://docs.goauthentik.io/add-secure-apps/providers/oauth2/),
[création du provider](https://docs.goauthentik.io/add-secure-apps/providers/oauth2/create-oauth2-provider/).

## Politique des comptes et des groupes

`MEDIA_OIDC_USER_GROUPS` est une liste explicite de noms exacts, séparés par
virgules. L'appartenance à au moins un groupe donne accès au rôle **user** pour
un nouveau compte. Elle n'accorde aucune bibliothèque ; un administrateur attribue
ensuite les permissions dans l'administration existante (les comptes SSO y sont
identifiés par `sso-<UUID>`). Aucun rôle ni groupe fournisseur ne crée/promouvrait
un administrateur. Les rôles Core existants sont conservés et relus à chaque
requête ; les claims de rôle sont ignorés.

L'identité durable est uniquement `(issuer, sub)` avec unicité SQL ; `email`,
`preferred_username` et nom ne sont jamais utilisés pour fusionner ou retrouver
un compte. Deux sujets avec le même email restent deux comptes distincts, sauf
liaison contrôlée explicite préalable. Une reconnexion au même sujet réutilise
le même UUID et ses permissions, même si l'email change. Un changement d'issuer
est une nouvelle identité et nécessite une liaison explicite.

Pour conserver un compte local existant : se connecter localement, ouvrir
**Paramètres → Connexion SSO → Lier mon compte local au SSO**, confirmer le mot
de passe local puis s'authentifier chez Authentik. L'opération nécessite session,
Origin, CSRF, mot de passe correct et OIDC valide. Le callback revalide la session
locale d'origine dans la transaction SQL. Une identité déjà liée à un autre
compte est refusée ; un compte ne peut avoir qu'un sujet par issuer. Aucune fusion
ni déplacement silencieux. La session d'origine est révoquée, une nouvelle session
Core est créée ; le mot de passe et le rôle local sont conservés. La liaison d'un
administrateur confirme une identité de ce compte déjà administrateur, sans
promotion. Un administrateur local actif est requis avant de commencer OIDC.

## API et cookies

| Route | Contrat |
| --- | --- |
| `GET /api/v1/auth/oidc/status` | Public : `{enabled}` uniquement ; ne contacte pas Authentik |
| `GET /api/v1/admin/oidc` | Session administrator ; enabled, issuer, clientId, redirectUri, userGroups, localLoginEnabled ; jamais de secret |
| `POST /api/v1/auth/oidc/start` | Origin exact ; retourne `{url}`, pose le cookie temporaire ; 404 si désactivé |
| `POST /api/v1/auth/oidc/link` | JSON `{password}` ; session locale + Origin + CSRF + revalidation Argon2 ; retourne `{url}` |
| `GET /api/v1/auth/oidc/callback` | state + code ; consommation unique, validation fournisseur, session Core, document de fin puis navigation `/app/` |

Échecs : 401 réponse/état/nonce/signature/claims invalides, 403 groupe refusé ou
compte local désactivé, 409 identité déjà liée, 429 budget épuisé, 502 fournisseur
indisponible. Le callback retourne une erreur JSON générique sans recopier le code,
les claims, l'URL fournisseur ou les descriptions d'erreur du fournisseur.

Code flow confidentiel avec HTTP Basic au token endpoint et PKCE S256. State,
nonce, verifier et liaison navigateur utilisent chacun 244 bits d'aléa système.
Le state est haché en SQLite ; verifier/nonce sont conservés temporairement dans
`oidc_flows` (5 min). Le verifier ne figure jamais dans une URL. State et nonce
figurent dans la requête d’autorisation OIDC ; ils ne sont pas journalisés par
le Core. Le cookie temporaire
`media_oidc_flow` est HttpOnly, Secure en HTTPS, SameSite=Lax, limité au chemin
OIDC ; il permet le retour de navigation externe. Le callback exact est la seule
exception au refus global cross-site et exige GET/navigation document (si Fetch
Metadata est présent), state unique et cookie navigateur correspondant. Le callback termine la chaîne
externe par un document Core minimal : un script
externe de même origine lance une nouvelle navigation vers `/app/` (lien manuel
si JavaScript désactivé). Cela conserve les refus cross-site existants sur `/app/`
et les cookies de session Strict, sans élargir les exceptions aux pages publiques.
Le cookie temporaire est effacé après callback réussi ou refusé. Un autre onglet lançant un
nouveau flux remplace ce cookie et invalide le parcours navigateur précédent.

Le cookie de session `media_session` garde HttpOnly, Secure en HTTPS et
SameSite=Strict, les limites absolue 12 h / inactive 30 min, et le CSRF existant.
Logout reste POST + Origin + CSRF et révoque la session Core. Les jetons d'accès,
ID tokens et refresh tokens ne sont pas persistés ; ils ne sont pas nécessaires
pour la lecture média. Les secrets temporaires sont protégés par les mêmes ACL
SQLite/sauvegardes que les sessions. Les flux expirés sont nettoyés au prochain
start ; capacité 256 flux, 8 opérations fournisseur simultanées, délais HTTP 10 s,
connexion 4 s, JSON limité à 256 Kio. Argon2 de liaison partage les 2 workers et
le budget persistant du login local (10/min global).

## Rapport de sécurité et limites

- Signature RS256 imposée, JWKS récupéré à chaque callback : rotation effective
  immédiatement, sans cache de clés périmées ; issuer/audience/exp/sub/iat requis,
  nonce exact, `azp` contrôlé (obligatoire si audience multiple), iat futur refusé.
- Rejeu et login CSRF : state consommé atomiquement avec le cookie et la date,
  avant tout échange de code ; PKCE vérifié côté fournisseur ; nonce côté Core.
- Création/lien transactionnels, contraintes d'unicité, utilisateur actif requis
  pour la session ; rôle et désactivation relus sans cache positif côté Core.
- Aucun rapprochement par email, pas de privilèges implicites ni de bibliothèque
  accordée par les groupes. Le compte de secours garde ses credentials locaux.
- Sorties réseau : issuer uniquement configuré par l'opérateur, HTTPS hors
  loopback, origine des endpoints vérifiée, pas de proxy système ni redirection,
  vérification TLS système ; aucune URL fournisseur fournie par un utilisateur.
- Aucun jeton OIDC dans les réponses Web, capacités Node ou logs applicatifs.
  Les proxys ne doivent pas journaliser les query strings du callback (code/state).
- Une désactivation **dans Core** ou modification de rôle est immédiate à la
  prochaine requête. Une désactivation/retrait de groupe **chez Authentik** est
  contrôlée lors de la prochaine authentification OIDC ; elle ne révoque pas à
  elle seule les sessions Core existantes. Pour révocation immédiate, désactiver
  le compte Core/révoquer ses sessions. Pas de back-channel logout, introspection,
  synchronisation périodique des groupes ni fermeture globale de la session IdP.
- La déconnexion Core laisse la session navigateur Authentik active : un nouveau
  clic SSO peut reconnecter sans demander le mot de passe IdP. Ce comportement
  n'est pas une absence de révocation du cookie Core.
- L'opérateur doit conserver un administrateur local fonctionnel. La liaison ne
  supprime aucun mot de passe ; aucune récupération d'identifiant n'est ajoutée.
- Les tests de fournisseur synthétique prouvent le protocole exercé, pas une
  configuration Authentik réelle ni un audit de pénétration externe.

## Vérification reproductible

```powershell
cargo fmt --check
cargo clippy --locked --workspace --all-features --all-targets -- -D warnings
cargo test --locked --workspace --all-features
npm --prefix apps/web run check
npm --prefix apps/web run lint
npm --prefix apps/web test
npm --prefix apps/web run build
docker build --target runtime-core -t aether-core:oidc-qa .
docker build --target runtime -t aether:oidc-qa .
python scripts/test_oidc_docker.py
```

Les deux clés privées sous `crates/core/tests/fixtures/oidc` sont exclusivement
synthétiques et publiques, générées pour ces tests. Ne jamais les utiliser comme
clés de fournisseur réel. Le banc Docker crée puis supprime uniquement ses
conteneurs/volumes anonymes `oidcqa-<id>`, sans toucher aux déploiements existants.
Il utilise un fournisseur HTTP loopback synthétique et Chrome headless, avec
PKCE effectivement vérifié, découverte et signatures RSA réelles. Ports réservés
sur loopback : 48887/48888 ; fournisseur hôte 48889 accessible au proxy de test
Docker (le serveur hôte est lié à toutes les interfaces durant ce test uniquement).
Les secrets aléatoires du banc restent dans `target/oidcqa-*`, jamais dans Git.

## Résultats observés le 18 octobre 2026

- `cargo test --locked --workspace --all-features` : 128 tests réussis,
  aucun échec, 7 ignorés explicites (navigateurs/certificats/aide subprocess).
  Cette passe précède l'ajustement final du document de fin du callback et
  l'ajout du quatrième test OIDC.
- Après cet ajustement : `cargo test --locked -p media-poc --lib oidc::tests --
  --nocapture` : 4/4 réussis ; signatures/algorithmes, claims, state/nonce,
  expiration de flux, refus fournisseur, panne fournisseur, rotation JWKS,
  groupe refusé, rôles, compte désactivé, collision email et liaison locale.
- Non-régression après correction : `cargo test --locked -p media-poc
  --test challenger_security_challenge --test permissions --test distributed_multi_node`
  : 7/7 réussis (Host/Origin/Fetch-Site/CSRF, cookies HTTPS, droits, Core et deux
  Nodes distants sur sockets réelles).
- Clippy workspace/all-features/all-targets avec `-D warnings`, formatage et
  `git diff --check` réussis sur les sources finales.
- Web : check Svelte/TypeScript 0 erreur/0 avertissement, lint, 33 tests Vitest
  et build statique réussis.
- Les deux images `runtime` / `runtime-core` construites localement, aucun push.
  Compose standalone et distribué validés avec un secret d'appairage jetable.
- `python scripts/test_oidc_docker.py` : succès des deux modes, Chrome
  **155.0.8059.40**, Windows **10.0.26300**, headless. Provider loopback
  `localhost`, Core `127.0.0.1` : vrai retour entre sites différents, PKCE S256
  contrôlé par le provider de test, session HttpOnly/Strict, logout/révocation,
  reconnexion même UUID, secours local, état admin sans secret, compte SSO
  visible pour les permissions, pas de jeton localStorage ni erreur JS/CSP.
  Rapport durable : [docker-browser.json](evidence/2026-10-18/oidc/docker-browser.json).
- Le premier essai navigateur a révélé que la chaîne de 303 conservait le
  contexte cross-site jusque `/app/` : refus attendu du garde global. Correction
  par document de fin + script externe de même origine, sans assouplir les gardes
  de `/app/` ni le cookie Strict ; les deux modes ont ensuite été revérifiés.
- Aucun Authentik réel, NAS, réseau public, navigateur Firefox/Safari ni test
  de charge OIDC validé. Le banc distribué OIDC teste le Core dans son image
  distribuée ; il ne constitue pas une nouvelle preuve de streaming OIDC vers
  un Node distant sur deux machines physiques.

### Prévalidation du fournisseur réel

Après fourniture des identifiants dans un fichier local hors dépôt, le domaine
Authentik est joignable (racine et flow HTTP200), mais la découverte indiquée
retourne HTTP404. La requête d'autorisation utilisant le Client ID fourni retourne
HTTP400 « Client ID Error: The client identifier (client_id) is missing or invalid. ».
Aucun secret transmis, échange de code, compte utilisateur ou changement fournisseur.
Le callback enregistré et le groupe utilisateur doivent encore être confirmés.
Corriger l'application/provider associé au slug `aether` et vérifier le Client ID
avant un nouvel essai. Ceci ne valide pas le secret ni la connexion SSO réelle.
Voir [prévalidation sans secrets](evidence/2026-10-18/oidc/real-provider-preflight.json).
