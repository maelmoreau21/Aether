# Vue d’ensemble de l’administration

Le tableau de bord ouvre Vue d’ensemble sans fragment, avec retour conservé aux
liens explicites de bibliothèque, tâches, extensions et autres sections.
Il rassemble l’état du Core, les bibliothèques, les nœuds, l’activité récente et
les lecteurs vidéo ouverts. Les scans en erreur sont détaillés par bibliothèque,
avec leurs messages et compteurs du dernier scan disponible. Le compteur ne
prouve ni une panne actuelle du serveur ni l’illisibilité de tout le catalogue.
La cause des « 356 erreurs » d’un serveur distant nécessite ses vrais messages ;
la fixture navigateur de 356 erreurs sert uniquement à vérifier la présentation.

## Activité

GET `/api/v1/admin/activity` est réservé au rôle administrator. Réponse
`{sessions:[{userId,name,device,lastSeenAt,playbackId,title,position,duration,paused}],activeWindowSeconds:120}`,
au plus 200 lignes, sans cookie, token ou empreinte de session. Seules les sessions
non expirées des comptes actifs, vues depuis moins de 120 secondes, apparaissent.
C’est une présence récente, pas une preuve de connexion réseau continue.

PUT `/api/v1/account/playback/{uuid}` reçoit
`{workId,position,duration,paused}`. Le Core vérifie session/CSRF, droits sur
l’œuvre, limites numériques, propriété de l’identifiant et au plus 16 lecteurs
par session. DELETE retire uniquement le lecteur de la session courante.
Migrations additives 0046 SQLite et PostgreSQL : les présences dépendent de la
session avec ON DELETE CASCADE. Les entrées de plus de 120 secondes sont nettoyées
lors des signalements. Une entrée de plus de 45 secondes n’est plus affichée.

Le lecteur vidéo Web signale son état toutes les 15 secondes et retire son entrée
à sa fermeture ; la suppression attend le dernier signalement en vol. Un client
coupé disparaît par expiration. La vue actualise toutes les 10 secondes uniquement
lorsqu’elle est ouverte. Les changements de compte cessent les signalements.
Aucun état de lecture n’est déduit de l’historique de progression, qui peut aussi
être modifié manuellement. Les clients anciens et le lecteur musical global ne
signalent pas encore leurs lectures dans ce contrat.

Validation ciblée : test Rust des droits, CSRF, propriété de session et expiration ;
tests Web de fermeture pendant un envoi et de changement de compte ; parcours
Chrome desktop/mobile sur serveur local et SQLite temporaire. PostgreSQL et NAS
nécessitent validation dans leur environnement réel.

## Vérifications exécutées le 25 octobre 2026

- `cargo test -p media-poc --lib historical_empty_endpoint_keeps_provider_default_and_credentials -- --nocapture` : réussi ; les URL vides historiques ne masquent plus les défauts des fournisseurs, les URL personnalisées sont conservées.
- `cargo test -p media-poc --lib activity_is_authenticated_scoped_expires_and_stops -- --nocapture` : réussi.
- `cargo test -p media-poc --lib imports_normalized_collections_without_approval_and_enforces_activation -- --nocapture` : réussi, y compris galerie désactivée, CSRF, renommage, suppression, attribution idempotente et automatique, import Gravatar JPEG simulé, verrouillage.
- `cargo test -p media-poc --test extensions browser_extension_installs_and_executes_real_rust_module -- --ignored --nocapture` : réussi. Exécutable du même test relancé après les derniers changements Web : réussi.
- Test collections avec `MEDIA_TEST_AVATAR_LIVE=1` : réussi contre GitHub, 7 dossiers détectés et 26 avatars Sonny normalisés en SQLite temporaire.
- `npm --prefix apps/web test -- src/lib/admin/extension-settings.test.ts src/lib/player/presence.test.ts src/lib/player/progress.test.ts` : 12 tests réussis.
- `npm --prefix apps/web run lint`, `run check`, `run build` : réussis ; check conserve un avertissement préexistant dans CatalogGrid.svelte.
- `cargo check -p media-poc --lib` et `cargo fmt --all --check` : réussis.
- Clippy strict Core lib rencontré : avertissement collapsible_if dans music.rs:209, hors de cette refonte. Contrôle complémentaire `cargo clippy -p media-poc --lib -- -D warnings -A clippy::collapsible_if` réussi.
- Aether-Plugins : `cargo check --workspace --lib` et `cargo test -p aether-plugin-avatar -p aether-plugin-fanart -p aether-plugin-tmdb --lib --tests` réussis, 13 tests d’intégration.

Captures inspectées : `target/extensions-lifecycle-browser/avatar-gallery-desktop.png`,
`avatar-gallery-mobile.png`, `overview-desktop.png`, `overview-mobile.png`.
Les images et données de ce parcours sont synthétiques. Le test GitHub distinct
ne publie pas les avatars téléchargés.

Le catalogue distant a également été consulté :
https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json
ne contient pas Intro Skipper à la date de cette vérification. La publication
signée est le prochain petit jalon pour rendre ce plugin installable depuis le catalogue.
