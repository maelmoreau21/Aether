# Administration des comptes

Les éditeurs de comptes et de groupes placent le focus dans la fenêtre, le
maintiennent à l'intérieur pendant la navigation au clavier et le rendent au
contrôle d'origine à la fermeture. Échap et le clic sur le fond ferment l'éditeur.
Le test `node scripts/browser-admin-modal-focus.cjs` vérifie ce comportement
dans un document synthétique avec Chrome headless ; il ne teste pas les API Core.

Parcours local dans la console : création de comptes locaux, modification du nom affiché et du rôle, désactivation/réactivation sans suppression, remplacement du mot de passe local. Les deux rôles restent `administrator` et `user`. Un nouveau compte n'obtient aucune bibliothèque implicitement. Les accès hérités de groupes sont signalés dans l'annuaire.

L’annuaire présente une grille de comptes avec avatar personnel (ou initiales), rôle, type de connexion, état et nombre de bibliothèques actives accessibles. Une fiche séparée regroupe Profil, Accès, Contrôle parental et Mot de passe. Les droits directs restent distingués des droits hérités des groupes. La sélection de toutes les bibliothèques ne concerne que les bibliothèques actuelles ; elle n’accorde pas automatiquement les futures bibliothèques.

La dernière activité est le maximum des `last_seen_at` des sessions encore conservées en base. Elle n’est pas un historique durable : après révocation ou nettoyage des sessions, elle peut redevenir indisponible. La grille n’invente aucune date en l’absence de session.

## Contrat

Toutes les routes exigent une session administrateur. Toute mutation exige l'origine configurée et `x-csrf-token`. Corps JSON limités à 4 Kio, champs inconnus rejetés.

- `GET /api/v1/admin/users` : `{users:[{id,login,role,displayName,sso,disabledAt,lastSeenAt,hasAvatar}]}`, incluant les comptes désactivés.
- `GET /api/v1/admin/users/{id}/avatar` : avatar PNG normalisé du compte ciblé, accès administrateur uniquement ; `404` en l’absence d’avatar, `private, no-store`, `nosniff`.
- `POST /api/v1/admin/users` : `{login,password,role,displayName?}`, réponse `201 {id}`. Login local ASCII minuscule 3–64 caractères (lettres, chiffres, `.`, `_`, `-`), nom 128 caractères maximum, mot de passe 15–128 caractères et au plus 512 octets, sans contrôles. Aucun identifiant par défaut.
- `PUT` ou `PATCH /api/v1/admin/users/{id}` : `{role,disabled,displayName?}` ; c'est une représentation complète des champs modifiables, réponse `204`.
- `PUT /api/v1/admin/users/{id}/password` : `{password}`, `204`. Refuse les comptes sans credential local, sans leur en créer un. Les mots de passe SSO restent gérés par le fournisseur.

Les changements de rôle/état et les remplacements de mot de passe suppriment toutes les sessions du compte dans la même transaction. Un changement de nom seul conserve les sessions. Remplacer son propre mot de passe déconnecte aussi l'administrateur courant. Les médias déjà en cours de streaming ne sont pas interrompus ; les requêtes suivantes revérifient la session.

Le compte administrateur courant ne peut pas se désactiver ni se rétrograder. La vérification du dernier administrateur actif et la modification sont sérialisées par `begin_write` (verrou d'écriture SQLite / verrou transactionnel PostgreSQL partagé). L'identité de l'administrateur est revérifiée dans la transaction pour empêcher deux administrateurs concurrents de se désactiver mutuellement. Aucun effacement définitif de compte.

Erreurs : `401` session absente/expirée, `403` rôle/CSRF, `400` champs invalides, `404` compte absent pour mise à jour, `409` login existant/protection administrateur/credential local absent, `429` deux travailleurs de hachage déjà occupés. Argon2id réutilise les paramètres du bootstrap : 64 Mio, trois passes, sel aléatoire. Aucun secret n'est renvoyé ou journalisé.

Les rôles attribués au callback OIDC par une politique de groupes configurée continuent de suivre cette politique lors de la prochaine connexion : la console ne modifie pas le fournisseur d'identité.

## Vérification

`cargo test -p media-poc --test admin_users` couvre les requêtes HTTP anonymes et utilisateur, CSRF, doublons, rôles invalides, protection de son compte, désactivation/réactivation, révocation des sessions et mot de passe SSO refusé. Un test concurrent conserve un administrateur actif lorsque deux administrateurs tentent de se désactiver mutuellement. Exécution et résultats globaux consignés par le jalon principal ; le code de test seul ne constitue pas une preuve d'exécution.
