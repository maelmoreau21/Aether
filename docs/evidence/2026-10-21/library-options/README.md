# Validation des réglages locaux et permissions

21 octobre 2026, Windows, Chrome headless. Ce lot réalise une **partie** de la
demande : voir [contrat et fonctionnalités restantes](../../../LIBRARY_OPTIONS.md).

## Résultats

| Vérification | Résultat |
| --- | --- |
| Contrats Rust | 10 réussis |
| Node Rust | 42 réussis, 1 helper ignoré |
| Core Rust | 43 réussis, 1 scénario extensions ignoré |
| Permissions HTTP | 2 réussis |
| Scheduler | 2 réussis, dont nouvelle actualisation périodique persistante |
| Core et deux Nodes distants | 1 réussi |
| Appairage et relais sortant | 1 réussi |
| Administration navigateur existante | Réussie, vrai Core/SQLite temporaire |
| Nouveau parcours bibliothèques/permissions | Réussi, vrai Core/SQLite temporaire |
| PWA/coffre/téléchargement natif | Réussi, API de fixture |
| Svelte/TypeScript | 0 erreur, 0 avertissement |
| ESLint | Réussi |
| Vitest | 70 tests réussis |
| Build statique | Réussi |
| Rust fmt, Clippy workspace/all-targets -D warnings | Réussis |
| git diff --check | Réussi |

`report.json` contient la version Chrome, les points vérifiés et les erreurs JS.
Les captures images/mobile/bureau et contrôle parental ont été inspectées.
Le rapport `admin-regression/report.json` couvre le scénario existant.

## Commandes exécutées

Depuis la racine :

```powershell
cargo test -p media-contracts -p media-node -p media-poc --lib -j 1 -- --test-threads=1
cargo test -p media-poc --test scheduler --test permissions --test distributed_multi_node --test node_enrollment_and_relay -j 1 -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings
git diff --check
```

Pour les scénarios admin, une clé de sauvegarde **synthétique**, limitée au
processus de test, est nécessaire au parcours existant. Les archives sont dans
le dossier temporaire du harness, voisin de sa base, hors des médias :

```powershell
$env:MEDIA_BACKUP_KEY = ('c' * 64)
$env:MEDIA_REPORT_DIR = '../../docs/evidence/2026-10-21/library-options/admin-regression'
cargo test -p media-poc --test browser_admin -j 1 -- --ignored --test-threads=1
```

Le nouveau parcours a aussi été exécuté isolément :

```powershell
cargo test -p media-poc --test browser_admin browser_library_options_and_media_policy -j 1 -- --ignored --nocapture --test-threads=1
```

Depuis `apps/web` :

```powershell
npm run check
npm run lint
npm test
npm run build
$env:MEDIA_REPORT_DIR = '../../docs/evidence/2026-10-21/library-options/pwa'
node scripts/test-pwa.mjs
```

Les premières exécutions ont révélé un motif HTML incorrect pour le pays et
un chemin historique d'import qui ne filtrait pas les NFO ; ils ont été corrigés
et les vérifications concernées rejouées. Le scénario navigateur attend désormais
le chargement des permissions avant de modifier une case et actualise la liste
après avoir créé son compte par API. Le scénario de sauvegarde a été relancé
avec la clé synthétique requise par son harness.

## Ce que ces preuves établissent

Les tests HTTP vérifient les droits admin/CSRF, leur refus pour un utilisateur,
le refus des téléchargements GET/HEAD, la conservation de lecture/Range exacte,
les tags bloqués avant pagination et sur les ressources dérivées, les horaires
UTC et leurs bornes, la récupération administrateur et les images normalisées.
Les tests de migration/reprise préservent les options lors d'un ancien PUT et
après réouverture. Le scheduler conserve l'échéance et réutilise le runner.

Le nouveau navigateur valide quatre affiches décodées dans la mosaïque,
l'enregistrement/rechargement des options, les imports d'affiche et de fond,
les formulaires utilisateur et les mises en page 390/1440 px. Les couleurs des
affiches proviennent de données d'inventaire et d'images synthétiques injectées
par le harness ; aucune lecture vidéo n'est déduite de ces cartes.

Le scénario PWA utilise un serveur API simulé et un Opus légalement généré.
Il vérifie le fichier natif exact, son nom, l'absence de copie IndexedDB ajoutée
par ce téléchargement, les anciennes copies chiffrées, le décodage Opus,
pause/seek, Range, suppression et reconnexion. Le refus d'autorisation de
téléchargement réel est établi séparément par les tests HTTP Core.

Pas de validation PostgreSQL réelle de la migration 0037, de fournisseur réel,
de NAS ou de mobile physique dans ce lot. Pas de nouvelle preuve AV1 navigateur,
de mesure de charge ou de déploiement. Les traitements avancés listés dans le
contrat restent absents ; les deux rôles et les bibliothèques ro sont conservés.
