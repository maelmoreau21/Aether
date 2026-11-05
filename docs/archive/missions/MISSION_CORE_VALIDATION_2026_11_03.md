# Validation Core ciblée — 3 novembre 2026

Base observée : `525b61f` sur `develop`, avec des changements locaux concurrents
de présentation conservés. Ce rapport concerne les fonctionnalités et contrats
backend de la mission ; il ne valide pas cette refonte visuelle.

## Résultats réellement exécutés

Contrôles statiques finaux : `cargo fmt --all -- --check` et
`cargo clippy --locked -p media-poc -p media-node --all-targets -j 1 -- -D warnings`
réussis. Les corrections Clippy concernent la forme des conditions et une
initialisation de fixture ; aucune suite globale de tests n'a été lancée.

Compilation mutualisée du binaire de tests Core, puis exécution sérielle des
seuls filtres concernés, sans suite globale. Windows, Rust debug, fixtures
synthétiques, SQLite et récepteurs réseau de test sur boucle locale.

| Filtre | Résultat | Portée |
|---|---|---|
| `recommendations::` | 3 réussis, 8,06 s | Compte/CSRF, opt-in, droits et parental, favoris/notes/historique, exclusion vus, catalogue frais, rétention utilisée et réouverture SQLite ; classement/doublons. |
| `notifications::tests` | 5 réussis, 1 PostgreSQL ignoré, 8,71 s | Adaptateurs HTTP locaux, signature webhook, refus redirection, SMTP local, file/retries/déduplication/reprise SQLite, admin/CSRF et secrets masqués. |
| `extensions::subtitles::tests` | 3 réussis, 1 PostgreSQL ignoré, 12,59 s | Contrats, réservations/quotas, parcours HTTP signé avec vrai worker et CDN substitué uniquement pour le test ; nettoyage d'une copie manquante/bibliothèque désactivée. |
| `provider_download::tests` | 3 réussis, 0,02 s | Hôtes HTTPS exacts, DNS/IP publiques, limites et refus local avant émission HTTP. |
| `extensions::run_ownership::tests::sqlite` | 1 réussi, 0,31 s | Lease vivant, expiration, publication conditionnelle et absence de rejeu avec deux pools SQLite. Ce n'est pas un déploiement multi-Core SQLite. |
| `trickplay::chapter_tests` | 1 réussi, 8,60 s | Vraies frontières de chapitres, absence de chapitre inventé, paramètres bibliothèque et droits vivants des ressources. |

Les 16 tests ci-dessus ont réussi. Le premier passage notifications avait
échoué dans la fixture de tâche terminale : `available_at`, obligatoire dans le
schéma existant, n'était pas renseigné. Seule la fixture a été corrigée ; le
filtre notifications a ensuite été reconstruit et rejoué. Ce premier résultat
défavorable est conservé ici, sans le présenter comme un défaut de livraison.
La reconstruction ciblée a duré 3 min, distinctes des 8,71 s d'exécution. Aucune
comparaison froid/chaud ou promesse de gain de performance n'en est déduite.

Le runner PostgreSQL accepte `-TestBinary` avec un binaire Core de tests compilé
dans `target/debug/deps` ; cela évite de compiler à nouveau les mêmes sources
pour chaque filtre. Les commandes exécutées après cette compilation étaient :

```powershell
pwsh -NoProfile -File scripts/test-postgres.ps1 -OnlyNotifications -TestBinary target/debug/deps/media_poc-30c965d2ffa62abe.exe
pwsh -NoProfile -File scripts/test-postgres.ps1 -OnlyExtensionRuns -TestBinary target/debug/deps/media_poc-30c965d2ffa62abe.exe
```

- Notifications : **1/1 réussi, 1,45 s** ; PostgreSQL 18 réel, deux pools,
  attribution concurrente unique, expiration/reprise et un seul envoi à un
  récepteur HTTP local après réouverture.
- Propriété de l'historique des extensions : **1/1 réussi, 1,25 s** ; PostgreSQL
  18 réel, deux pools indépendants, expiration, refus du jeton périmé et
  conservation d'un résultat terminal unique.

Chaque runner utilise un conteneur jetable tmpfs, un port sur boucle locale et
des identifiants aléatoires en environnement. Les conteneurs ont été supprimés
dans leur bloc `finally`. Aucun service PostgreSQL existant n'est modifié.

## Deux processus Core et panne

Le test ignoré `two_real_cores_fence_an_inflight_extension_after_owner_crash`
et le runner `-OnlyMultiCoreExtension` constituent un scénario distinct des
tests à deux pools. Ils démarrent deux vrais Core et leurs workers, bloquent
l'écriture privée d'un guest Wasm signé avec le verrou PostgreSQL réel,
constatent cette attente en base avant de tuer son Core, puis comptent les
effets durables et l'historique sur le survivant.

Commande réellement exécutée :

```powershell
pwsh -NoProfile -File scripts/test-postgres.ps1 -OnlyMultiCoreExtension
```

**1/1 réussi, 43,48 s hors compilation** ; compilation des exécutables courants
4 min 27 s. Le test a observé la requête guest réellement bloquée dans
`pg_locks`, puis tué son Core. Le survivant a marqué l'ancien historique
`interrupted` après expiration du lease, sans rejouer l'invocation. Il restait
**zéro entrée privée durable** de cette tentative. Une nouvelle invocation
explicite sur le survivant a produit **une entrée KV** et **un historique
`completed`**, en conservant l'ancien historique interrompu. Le conteneur
PostgreSQL et les deux processus ont été arrêtés/nettoyés par le test/runner.

Le premier run affichait le reset HTTP attendu après l'arrêt brutal comme
panic d'une tâche de requête auxiliaire, sans échec du scénario principal.
Le helper a ensuite été corrigé pour traiter cette erreur de transport comme
un résultat ; `cargo check --locked -p media-poc --test multi_core_extension_runs
-j 1` passe (4,65 s). Les assertions d'effets durables n'ont pas été changées.
Ce résultat couvre un arrêt avant publication de l'écriture privée : il ne
prouve ni la reprise de toutes les tâches, ni une partition réseau, ni l'absence
de double effet après acceptation d'une opération externe.

## Limites, risques et suites

Les adaptateurs de notifications ont été exercés uniquement contre des
récepteurs locaux. Les services OpenSubtitles/TVDB, un SMTP externe, ntfy/Gotify
réels, le NAS, le matériel ARM et la pertinence des recommandations ne sont pas
prouvés par ces tests. Les lectures média réelles et les builds Docker font
l'objet de preuves séparées ; une réponse HTTP ici n'en tient pas lieu.

Le multi-Core reste expérimental : propriété des connexions Node, routage vers
leur Core, fencing de tous les effets, mutations d'extensions, fichiers/caches
partagés et OIDC derrière répartiteur restent à garantir. Le lease ajouté ne
porte que sur la publication de l'historique manuel existant des extensions.

Les changements restent divisibles en lots Core/API/données/tests et plugin :
OpenSubtitles, TVDB, chapitres/FFmpeg, recommandations, notifications, Docker,
puis propriété des exécutions. Les modifications visuelles concurrentes et les
réparations CI antérieures doivent rester identifiées séparément dans les PR.
Pour revenir en arrière, désactiver les nouvelles fonctions et revenir au
binaire précédent avec un Core unique, en conservant les tables additives.
Aucun média ni historique de lecture n'est réécrit par ces fonctionnalités.

## 4 novembre 2026 — Nouveau contenu opt-in

Besoin confirmé : les quatre adaptateurs ne recevaient auparavant que des faits
de tâches et scans ; l'option nouveau contenu était explicitement indisponible.
Le lot raccorde `inventory_works.added_at` → collecte bornée → événement
`content.added` → file existante → adaptateurs. `notification_content_preferences`
est additive ; les préférences principales et cette souscription sont mises à
jour dans la même transaction. L'API de compte expose `content`, désactivé par
défaut. Le contrôle Svelte utilise les conventions et styles présents, sans refonte.

Commandes réellement exécutées depuis la racine, contre l'exécutable de tests
neuf produit par la compilation commune du 4 novembre :

```powershell
& target/debug/deps/media_poc-30c965d2ffa62abe.exe 'notifications::tests' --test-threads=1
./scripts/test-postgres.ps1 -OnlyNotifications -TestBinary (Resolve-Path target/debug/deps/media_poc-30c965d2ffa62abe.exe).Path
```

- Filtre SQLite/récepteurs locaux : **6 réussis, 0 échec, 1 ignoré PostgreSQL**,
  durée du harnais **10,26 s**, commande PowerShell **12,79 s**.
- PostgreSQL 18 jetable : **1 réussi**, **2,39 s** de harnais et **8,02 s** de
  runner avec préparation/nettoyage, sans recompilation. Deux pools réels,
  claims/fencing/reprise et le nouveau contrat de contenu dans le même test.
- Le nouveau contrat vérifie ancien catalogue exclu, déduplication, migration
  additive/réouverture SQLite, payload `workId` sans titre/taskId, contrôle
  parental, retrait de droits, contenu indisponible et opt-out. Un seul webhook
  local autorisé est reçu pour les deux nouveaux Work de la fixture, l'autre
  Work étant interdit par le contrôle parental. Aucun message externe envoyé.
- `npx eslint src/lib/catalog/NotificationSettings.svelte` et `npm run check`
  réussissent, ce dernier avec zéro erreur et zéro avertissement. Chrome
  156.0.8078.80 vérifie opt-in utilisateur/CSRF sur Vite dev avec API synthétique ;
  preuve distincte dans le rapport Web. Aucun nouveau build Web requis ici.

Les timestamps ont une précision d'une seconde : les ajouts de la même seconde
que l'activation sont volontairement omis, et une réimportation avec nouvelle
identité peut constituer un nouvel ajout. Les sources supprimées avant collecte
ou plus anciennes que sept jours restent hors fenêtre. Les événements
`node.unavailable` et `provider.error` restent indisponibles, sans état simulé.
La livraison reste au moins une fois, avec les quotas et retries déjà documentés.
Retour arrière : désactiver `content` ou les destinations, conserver les tables
additives et revenir au binaire précédent ; aucun média original n'est touché.
Prochaine priorité de notifications : des faits durables de transitions Node et
d'erreurs fournisseur avant d'activer ces deux autres catégories.

## 4 novembre — Budget TVDB et transport des chapitres

Le test opt-in `extension_runtime::provider_budget_tests` exécute le vrai module
TVDB optimisé (248 968 octets) via `extension_runtime::execute`, avec les limites
Wasmi de production. Recherche de 20 résultats renseignés, détails/traduction et
images : **1/1 en 9,98 s**. Réponses ordonnées synthétiques, aucun réseau, aucune
installation signée ni application Core/HTTP prétendue. Le détail et la commande
sont dans [le rapport TVDB](../../../../Aether-Plugins/docs/TVDB_2026_11_03.md).

`chapter_images_remote` démarre Core et Node sur deux ports TCP du même processus
de test : scan réel, extraction AV1 FFmpeg 9.0.2, lien signé direct puis canal
inverse imposé sans repli, GET/HEAD et Range : **1/1 en 13,66 s**. Le correctif
réutilise l'encodage HEAD des autres dérivés. Le premier essai forcé a expiré au
login en 27,93 s, sans atteindre ce chemin ; il est conservé et ne constitue pas
une reproduction du défaut HEAD. [Le rapport chapitres](../../CHAPTER_IMAGES.md) décrit
le harnais corrigé, les limites et les journaux. Pas de preuve de deux machines
ou de NAS. Aucun nouveau test global du lecteur n'était nécessaire pour ce
correctif de transport d'image.

Contrôles finaux : `cargo fmt --all -- --check` et
`cargo clippy --locked -p media-poc -p media-node --lib --tests -j 1 -- -D warnings`
réussissent, ce dernier en 1 min 18 s. Son premier passage avait refusé le type
de file HTTP de fixture jugé trop complexe ; un alias de type réservé aux tests
a corrigé cet avertissement. Aucun comportement n'a changé et les tests
fonctionnels n'ont pas été rejoués pour ce seul alias.
