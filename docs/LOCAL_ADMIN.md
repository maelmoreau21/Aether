# SQLite et premier administrateur local — P1

Le Core ouvre `MEDIA_DB_PATH` (défaut `data/core.sqlite3`) et applique les
migrations SQLx embarquées avant de démarrer HTTP. Créer au préalable un dossier
de données privé, distinct des médias et du build Web. Utiliser un disque local,
pas un partage SQLite multi-Core. Le parent doit exister ; aucune recréation
silencieuse d'une base corrompue. DB/WAL/SHM contiennent des données sensibles.

Depuis la racine du dépôt, terminal local interactif :

```powershell
New-Item -ItemType Directory -Path data
$env:MEDIA_DB_PATH = Join-Path (Get-Location) 'data/core.sqlite3'
cargo run --locked -p media-poc --bin media-bootstrap
cargo run --locked -p media-poc --bin media-poc
```

Choisir son propre login et son propre mot de passe ; aucune valeur prédéfinie.
Login : 3–64 caractères ASCII minuscules, chiffres, `.`, `_`, `-`.
Mot de passe : 15–128 caractères, au plus 512 octets, sans contrôles, sans trim
ni normalisation silencieuse. Saisie masquée et confirmation, chaînes effacées
au drop par Zeroizing. Aucun argument, variable d'environnement ni pipe pour
le secret ; stdin/stdout doivent être des terminaux. Annulation/erreur : exit 1.
Ne pas enregistrer une session de terminal contenant la saisie.

L'autorisation de bootstrap est l'accès OS au terminal et au dossier de données.
Protéger ce dossier avec les ACL du compte opérateur sous Windows ; sous Unix,
utiliser un dossier 0700 et umask 077 avant la première création. Un utilisateur
local capable de remplacer la base ou d'exécuter des programmes sous ce compte
est dans la frontière de confiance. Une session SSH autorisée équivaut à un accès
opérateur ; ce mécanisme n'essaie pas de distinguer SSH d'une console physique.
Ne pas mettre la DB dans un répertoire servi statiquement ou dans une bibliothèque.
Le chemin canonique est rejeté dans la racine des fixtures lorsqu'elle existe ;
ce contrôle ne protège pas des courses symlink/hardlink d'un opérateur hostile.
Une erreur après création du fichier peut laisser une base vide migrée : relancer
est possible tant que la transaction de bootstrap n'a pas été validée.

Tables STRICT : users (UUID stable, rôle, création, désactivation),
local_credentials (login unique, hash PHC, date), bootstrap_state (marqueur unique).
Les contraintes SQL et l'enum Rust autorisent exactement administrator/user.
Compte séparé de l'identifiant local : futures sessions liées à users.id ; future
identité OIDC à identifier par issuer+subject, jamais par simple égalité de login.
Aucune table/session/OIDC ou promotion automatique anticipée.

Argon2id v19 : 65536 KiB, t=3, p=1, sortie 32 octets, sel aléatoire OS généré
par la bibliothèque. Hash PHC seulement persisté, travail CPU hors exécuteur async.
Ce coût dépasse le [minimum OWASP](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#argon2id),
mais n'est pas un benchmark Synology ; mesurer avant les futures connexions.
Versions contrôlées via cargo info/search : SQLx 0.9.0, Argon2 0.6.0,
rpassword 7.5.4 ; Cargo.lock versionné, SQLite embarqué sans service externe.

Le bootstrap prend BEGIN IMMEDIATE avant de vérifier l'absence de comptes et du
marqueur, puis insère compte, credential et marqueur dans la même transaction.
Un seul gagnant ; les autres obtiennent AlreadyInitialized (ou erreur de verrou
si le délai de 15 s est dépassé). La suppression du compte ne rouvre pas le
bootstrap. Aucun outil de reset automatique. Aucun mot de passe ni hash affiché.

Aucun endpoint de bootstrap ou de compte. HTTP reste lié à 127.0.0.1 et aux
fixtures avec Host/Origin/Fetch-Site existants. Créer l'administrateur ne protège
pas encore les routes de lecture par une session : ni proxy/tunnel/publication,
ni médias personnels. Aucun changement de lecteur ou de design.

Tests : `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
`cargo test --locked`. Les tests d'identité couvrent migrations/réouverture,
hash correct/mauvais secret, contraintes, entrées invalides, panne avec rollback,
concurrence entre pools et processus, corruption/parent absent, refus CLI sans TTY.
Le helper ignoré est exécuté explicitement par le test de concurrence processus.
La saisie masquée interactive n'est pas prouvée par le test CLI sans terminal.

Prochain jalon à valider : connexion, sessions révocables sécurisées, protection
CSRF/rate limiting et permissions backend. Ils ne font pas partie de ce jalon.
