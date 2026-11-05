# Groupes locaux et clés API — 19 octobre 2026

Les groupes locaux sont des collections de comptes et de bibliothèques. Ils n'attribuent aucun rôle. Les accès effectifs sont l'union des permissions individuelles et des groupes ; retirer un groupe conserve les permissions individuelles. Une bibliothèque désactivée ou un Node révoqué reste inaccessible. La vérification utilise une vue SQL en lecture, à chaque requête catalogue/média/progression et vérification de lecture directe par un Node.

## Groupes automatiques par rôle — 5 novembre 2026

Deux groupes système sont créés à l'ouverture de la base : **Utilisateurs** et
**Administrateurs**. Tous les comptes appartiennent automatiquement au groupe de
leur rôle actuel, y compris le premier administrateur, les comptes existants et
les comptes SSO. Un changement de rôle déplace immédiatement cette appartenance.
Les groupes personnalisés et leurs permissions restent conservés. Si un groupe
personnalisé utilise déjà ce nom, le groupe système reçoit un suffixe distinct.

L'appartenance est calculée en SQL depuis `users.role`, sans copie ni
promotion de rôle par le groupe. Les groupes système ne peuvent pas être
supprimés, devenir globaux (`isDefault:true`) ni recevoir des membres manuels.
Leurs noms et bibliothèques restent configurables ; aucune bibliothèque n'est
accordée lors de leur création. L'API expose `systemRole` (`user`,
`administrator` ou `null`) et `userIds` inclut les membres calculés. L'interface
affiche l'affectation automatique et réserve sa modification à la fiche du compte.

Les identifiants réservés sont `builtin-role-user` et
`builtin-role-administrator`. La vue `effective_user_group_members` sert à
l'administration et à l'héritage des permissions. La migration est idempotente
et commune SQLite/PostgreSQL ; les validations locales sont précisées dans le
bilan du changement.

Contrôles ciblés : `cargo test -p media-poc --test admin_access` couvre les
comptes bootstrap/locaux, les insertions sans identifiants locaux, les changements
de rôle, les refus de modification des membres, ainsi que la réouverture d'une
base avec groupes homonymes et permissions existantes. Un contrôle SQL sur une
base PostgreSQL 18 jetable a également exécuté la migration deux fois et vérifié
l'affectation ainsi que le retrait des droits après changement de rôle.

## API des groupes

Session `administrator` requise ; CSRF requis pour les mutations. Corps JSON limité à 32 Kio, nom de 1 à 80 caractères, au plus 500 membres et bibliothèques par groupe, 500 groupes au total.

| Route | Résultat |
|---|---|
| `GET /api/v1/admin/groups` | `{groups:[{id,name,isDefault,systemRole,userIds,libraryIds}]}` |
| `POST /api/v1/admin/groups` | Corps `{name,isDefault?,userIds,libraryIds}`, 201 `{id}` |
| `PUT /api/v1/admin/groups/{id}` | Remplacement transactionnel, même corps, 200 `{id}` |
| `DELETE /api/v1/admin/groups/{id}` | 204 ; comptes conservés |

Noms dupliqués : 409 ; référence inexistante : 400 ; groupe inexistant : 404. Le rôle/session de l'administrateur est revérifié dans la transaction d'écriture.
Suppression d'un groupe système : 409 ; `isDefault:true` sur un groupe système : 400.

## Clés API

Les clés ont uniquement la portée `catalog:read`. Elles permettent `GET /api/v1/integrations/catalog?offset=0&limit=24` avec l'en-tête `Authorization: Bearer CLE`. Cet endpoint fournit des identifiants, titres, types, années et identifiants de bibliothèque, sans chemins disque, secrets ni URL de streaming. Pagination bornée à 100 œuvres et offset 1 000 000. Les fixtures éditoriales non persistées ne sont pas incluses.

Une clé appartient à un compte et à une liste explicite de bibliothèques. Les droits actuels du compte, la désactivation du compte/bibliothèque, l'état révoqué du Node, l'expiration et la révocation de la clé sont vérifiés en base à chaque appel. Les nouveaux droits du compte n'élargissent pas la liste de bibliothèques de la clé.

| Route | Résultat |
|---|---|
| `GET /api/v1/admin/api-keys` | `{keys:[{id,name,userId,createdAt,expiresAt,revokedAt,scope,libraryIds}]}` ; aucun jeton |
| `POST /api/v1/admin/api-keys` | `{name,userId,libraryIds,expiresInDays,neverExpires?}` ; 201 `{id,token,expiresAt,scope}` |
| `DELETE /api/v1/admin/api-keys/{id}` | Révocation définitive, 204 |

Gestion réservée aux administrateurs avec CSRF pour création/révocation. Par défaut, expiration entre 1 et 365 jours. Le choix explicite `neverExpires:true` avec `expiresInDays:null` crée une clé sans expiration, toujours révocable : les réponses exposent alors `expiresAt:null` (stockage interne : `expires_at=0`, sans migration). Une durée omise sans ce choix, 0 jour, ou une durée jointe à `neverExpires:true` est refusée. Au plus 100 bibliothèques et 1 000 clés conservées. Le jeton aléatoire n'est affiché qu'une fois, stocké uniquement sous forme SHA-256 et jamais dans `localStorage`. Réponses `private, no-store`. Une clé n'authentifie pas les routes d'administration, les sessions utilisateur ni le streaming. Les intégrations doivent employer HTTPS hors réseau local de confiance.

Dans **Administration → Clés API**, donner un nom à l'application, choisir son propriétaire et ses bibliothèques, puis cocher **Sans expiration** si nécessaire et créer la clé. Copier le jeton immédiatement. Une clé Aether n'est pas une clé de fournisseur externe : elle ne supprime pas les quotas ou tarifs éventuels de TMDB, Fanart ou d'un autre service. Il n'y a pas de quota d'appels facturé par Aether ; les limites de pagination, de portée et les capacités du serveur restent applicables.

Les tests HTTP `admin_access` exercent les autorisations réelles, les accès hérités, leur retrait sans perte des droits directs, le hachage, les limites de portée et les changements de droits en direct. Ils n'évaluent pas une charge d'intégrations concurrentes.
