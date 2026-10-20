# Validation locale — administration et affiches

Chrome 155.0.8059.82, Windows NT 10.0.26300, headless. Médias synthétiques,
Core réel et bases SQLite temporaires ; mobile simulé à 390 px.

- `report.json` : comptes, droits, avatar réellement décodé, groupes, clé API sans
  expiration et révocation, libellé SSO persistant, tâches et sauvegardes chiffrées.
- `posters/report-{initial,restart,update,delete}.json` : catalogue, affiche,
  repli, sessions/droits, texte échappé, lecture/pause/seek et absence d’erreurs JS/CSP.
- `posters/progressive-*.png` : Film visible avant libération de la réponse Séries.
- `extensions/` : paquet signé, consentement bibliothèque, exécution d’un vrai
  module Rust de test, désactivation et désinstallation ; ce module synthétique
  ne représente pas un fournisseur distant réel.

Première affiche de la minuscule fixture : 568 ms à 1440 px, 173 ms à 390 px.
Ce sont deux observations locales, sans comparaison avant/après ni mesure des
cinq secondes signalées sur l’installation personnelle. Les originaux ne sont
pas redimensionnés. Les captures utilisateurs et chargement mobile ont été
inspectées visuellement.

Commandes depuis la racine, après le build Web et la génération des fixtures :

```powershell
cargo test --locked -p media-poc --test admin_access --test admin_users --test settings_avatars --test posters_flow --test extensions -j 1 -- --test-threads=1
cargo test --locked -p media-poc --lib oidc -j 1 -- --test-threads=1
cargo test --locked -p media-poc --lib configuration_tests -j 1 -- --test-threads=1
# Clé synthétique uniquement pour les sauvegardes du test temporaire.
$env:MEDIA_BACKUP_KEY = 'ab' * 32
$env:MEDIA_REPORT_DIR = Join-Path (Get-Location) 'docs/evidence/2026-10-20/admin-local'
cargo test --locked -p media-poc --test browser_admin -j 1 -- --ignored --nocapture --test-threads=1
$env:MEDIA_REPORT_DIR = Join-Path (Get-Location) 'docs/evidence/2026-10-20/admin-local/posters'
cargo test --locked -p media-poc --test browser_posters -j 1 -- --ignored --nocapture --test-threads=1
$env:MEDIA_REPORT_DIR = Join-Path (Get-Location) 'docs/evidence/2026-10-20/admin-local/extensions'
cargo test --locked -p media-poc --test extensions -j 1 -- --ignored --nocapture --test-threads=1
```

Les tests ciblés HTTP ont passé ; la commande groupée a initialement révélé le
trap TMDb, corrigé et retesté. `posters_flow` et `settings_avatars` ont aussi été
exécutés directement via leurs exécutables de test reconstruits. Les anciens
sélecteurs du test d’affiches ont été adaptés aux fiches actuelles. Le scénario
de chargement différé et d’erreur partielle utilise une interception explicitement
limitée au test ; les contrôles de lecture et d’autorisations utilisent le Core.

Aucun fournisseur réel, callback SSO distant, PostgreSQL réel, NAS ou appareil
mobile physique validé. Aucun déploiement ni publication.

Contrôles finaux réussis : Web `check` (0 erreur, 1 avertissement préexistant dans
la page des téléchargements), `lint`, 60 tests et `build` ;
`cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings`,
et `git diff --check` dans les deux dépôts.
