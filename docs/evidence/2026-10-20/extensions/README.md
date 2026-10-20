# Extensions — preuves locales du 20 octobre 2026

Parcours réellement exercé sur Core/SQLite temporaire, Chrome headless et
Playwright, avec une fixture Rust compilée en `wasm32-unknown-unknown`.
Aucune interception d'API, aucun média personnel, aucun éditeur approuvé par défaut.

## Commandes et résultats

- `cargo build -p media-extension-sdk-fixture --target wasm32-unknown-unknown --release -j 1` : réussi, fixture de 131 137 octets après généralisation.
- `cargo test --locked -p media-poc --test extensions -j 1 -- --include-ignored --test-threads=1 --nocapture` : 6 tests extensions après généralisation de l’ABI passés, aucun ignoré. Le test scheduler a passé dans la validation précédente.
- `cargo test --locked -p media-poc --test admin_access -j 1 -- --test-threads=1 --nocapture` : les deux tests ont passé lors de l'exécution groupée initiale avec extensions/scheduler; le groupe initial avait échoué ensuite dans le navigateur, corrigé et relancé avec succès par la commande précédente.
- `cargo clippy --all-targets -j 1 -- -D warnings`, `cargo fmt --all --check`, `git diff --check` : réussis.
- `npm --prefix apps/web run check` : zéro erreur et avertissement. Lint et build : réussis. `npm --prefix apps/web test` : 57 tests passés.

Le test navigateur prouve approbation de clé, import signé, consentement sur
une bibliothèque, activation, recherche/sélection d'œuvre, exécution du module
Rust, proposition persistée après rechargement, désactivation, refus 403 et
désinstallation avec purge. Aucun débordement horizontal à 390 px, aucune erreur
JavaScript. Captures desktop 1440 px et mobile 390 px inspectées visuellement.

Les tests backend couvrent signature/digest altérés, manifeste incompatible,
identifiant de traversée, capacité inconnue, refus sans consentement et sur une
autre bibliothèque, rôle user refusé, CSRF, révocation d'éditeur et
désactivation/réactivation pendant invocation. Les tests runtime couvrent boucle
infinie, mémoire initiale excessive, croissance mémoire excessive, pointeur hors
limites, sortie excessive, imports et fonction start interdits.

Les premiers passages navigateur ont permis de corriger des réponses 201 vides
incompatibles avec le client JSON, un libellé de select et un sélecteur de test
ambigu. Le dernier parcours passe sur le build Web corrigé.

## Mesures et limites

`worker-measurements.json` contient trois invocations du binaire Windows debug,
mesurées avec `GetProcessMemoryInfo` à intervalle de 2 ms : pic de working set
observé 10,29–10,36 Mio, premier lancement 1 319 ms puis environ 100 ms.
Ce sont des observations locales, pas un quota RSS dur, un benchmark release,
une mesure NAS ou un test de charge. La mémoire linéaire invitée est bornée
séparément à 16 Mio par le runtime.

Docker n'était pas disponible : aucune validation d'image ni de PostgreSQL sur
ce changement. Aucun déploiement, publication ou nouvelle preuve codec.
Les fonctionnalités avancées non livrées sont listées dans `EXTENSIONS_API.md`.

La clarification utilisateur exclut toute extension produit : le module Rust est
une fixture sous `tests/fixtures/extension-sdk`, non installée par défaut.
Les mesures de `worker-measurements.json` concernent l’ABI initiale avant sa
généralisation; elles ne constituent pas un nouveau benchmark de cette révision.
