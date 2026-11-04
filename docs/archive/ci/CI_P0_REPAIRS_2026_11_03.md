# Trois blocages P0 — bilan local du 3 novembre 2026

Bases : Aether `699345fc17fff56b8528b3d836fdab67359f019b`, plugins
`0943099a6f0586a92c2f1d348f83e13c33df769f`, sur `develop`. Changements
préexistants conservés. Aucun commit, push, PR distante ou déploiement effectué.
Les trois lots restent indépendants.

## 1. Plugins — contrat MetadataCollection incompatible

Le SDK épinglé à `8f337380…` ne fournit pas les types utilisés par TMDb.
L'alignement sur `133896830c609a177e4c4c611084154d7e4aab5d`, déjà préparé dans
`Cargo.toml` et `Cargo.lock`, a été conservé et validé sans substitution locale.
Nouveaux tests : réexportation des types, aller-retour JSON avec/sans collection,
et invocation du vrai module TMDb Wasm via broker HTTP déterministe hors ligne.

**72 tests natifs workspace réussis**, Clippy strict, format et build Wasm
réussis ; **9 modules valides** sous 262 144 octets, smoke ABI et scénarios
metadata Wasm réussis. Plus gros module : IMDb 237 886 octets ; TMDb 231 012.
[Fichiers, commandes et limites du lot SDK](../../Aether-Plugins/docs/SDK_COLLECTION_CI_2026_11_03.md).

PR proposée : `fix(plugins): align the pinned SDK with the collection contract`.
Les autres changements préexistants des fournisseurs/stockage ne sont pas
attribués à cette correction ; les tests portent sur le worktree courant.

## 2. Chrome — attente ASS obsolète et sous-titre résiduel réel

L'échec a été reproduit exactement à l'ancienne ligne 255 du scanner : attente
de `ASS Hello, world` dans `video.textTracks`, après décodage vidéo et observation
des fréquences FR/EN. Or le chemin ASS utilise désormais libass/JASSUB et un
canvas. [Échec de référence](evidence/2026-11-03/p0-ci/baseline-failure.json).

Le renderer de production a aussi dessiné du texte stylé et appliqué le décalage
sur une vidéo AV1 en pause sous la même CSP, puis supprimé son canvas :
[preuve indépendante](evidence/2026-11-03/p0-ci/renderer.json).

L'inspection des captures a révélé un vrai défaut supplémentaire : l'ancien SRT
français pouvait rester visible sous l'ASS sur une image en pause. Les éléments
étaient retirés sans désactivation préalable de leur TextTrack.
[Capture avant correction](evidence/2026-11-03/p0-ci/before-native-caption-cleanup.png).

Changements :

- `apps/web/src/lib/player/native.ts` désactive les pistes natives avant leur
  détachement, lors du remplacement de source ou de sous-titre. Aucun changement
  de moteur, de politique linguistique ou de pipeline audio.
- `native.test.ts` vérifie la désactivation **avant** détachement ; test rouge
  avant correction (`showing` au lieu de `disabled`).
- `scripts/browser-scanner.cjs` attend la fin de sélection et le dimensionnement
  du canvas ; vérifie des pixels visibles, leur disparition avec un décalage
  de 30 s en pause, leur retour après remise à zéro et le retrait du canvas sur Off.
  Les anciennes pistes doivent être désactivées.
- Le repli WebVTT est ensuite testé en désactivant explicitement OffscreenCanvas
  dans le contexte isolé, puis en restaurant cette capacité. Cette simulation
  ne remplace pas les vérifications réelles du canvas.

Les captures masquent temporairement les commandes par CSSOM, sans contourner
la CSP. Deux essais intermédiaires d'instrumentation ont échoué : injection de
style refusée par CSP, puis capture du placeholder 300×150 avant initialisation.
Rapports conservés dans `target/p0-ass-fixed-20261103` et
`target/p0-ass-fixed-cssom-20261103`. Aucun délai de production n'a été augmenté.

Tests réellement exécutés :

- `npm test --prefix apps/web -- src/lib/player/native.test.ts src/lib/player/subtitle-preference.test.ts src/lib/player/subtitle-style.test.ts` : **52 réussis**.
- `npm run check --prefix apps/web` : **0 erreur, 0 avertissement**.
- ESLint ciblé `native.ts`/`native.test.ts` et build Web final : réussis.
- `node scripts/browser-ass-renderer.cjs` : rendu stylé, décalage en pause et nettoyage réussis.
- Banc Core/test construit avec `cargo test --locked -p media-poc --test browser_scanner -- --ignored --nocapture --test-threads=1`, puis rejoué après changements JS/Web avec le même exécutable fraîchement compilé, Rust inchangé :

```powershell
$env:MEDIA_REPORT_DIR='target/p0-ass-final-20261103'
& .\target\debug\deps\browser_scanner-ddeb6cdb3d3aae5a.exe --ignored --nocapture --test-threads=1
```

**Réussi en 63,54 s** : MP4, redémarrage du Core, puis MKV. Chrome 156.0.8078.80,
Windows 10.0.26300, Core/SQLite réels, fixtures synthétiques AV1/Opus et originaux
en lecture seule. Vidéo décodée, fréquences FR/EN, PCM externe non nul (pic ≈0,0484),
sync/seek en pause, pixels ASS/décalages/Off, repli WebVTT, SRT forcés, préférences,
progression et repli vidéo réussis ; zéro erreur CSP dans les deux phases.

[MP4](evidence/2026-11-03/p0-ci/report.json),
[MKV après redémarrage](evidence/2026-11-03/p0-ci/report-restart.json),
[synthèse](evidence/2026-11-03/p0-ci/summary.json),
[ASS final sans texte français résiduel](evidence/2026-11-03/p0-ci/ass-rendered.png).

PR proposée : `fix(player): clear paused native captions and verify ASS rendering`.
Risque principal : retrait des anciennes pistes pendant les changements, couvert
par les tests de remplacement et le vrai parcours MP4/MKV. Pas de réencodage,
écriture média, changement d'API ou de schéma.

## 3. Docker — contexte local et SHA du checkout

Le correctif A01 est conservé : `source: .` dans CI/publication, checkout Images
explicite de `github.sha` (déjà explicite en publication). Les cibles, overlays
et gates restent en place. Politique de release : **15 tests réussis** ;
Actionlint 1.7.12 et `git diff --check` dans les deux dépôts : réussis.

Les trois images `linux/amd64` ont été reconstruites avec le **frontend final** :

```powershell
docker buildx bake -f docker-bake.ci.hcl --load --set '*.cache-to=' --set '*.cache-from=' --progress plain
```

Smoke sans réseau : versions et utilisateur non-root des trois images validés.
Node : ffmpeg/ffprobe 9.0.2 exécutés, pas de `libav*.so` partagée, **177 356 775
octets**, sous le seuil CI de 200 000 000. Standalone : 343 370 975 octets ;
Core : 300 796 170. [Images finales et historique BuildKit](evidence/2026-11-03/p0-ci/current-images.json).

Reconstruction finale ≈16,7 s avec caches Rust/dépendances chauds et frontend
rebâti. Le précédent build de 13 min 26 s compilait les dépendances : conditions
non comparables, **aucun gain de performance n'est déduit**. Les durées du test
échoué et du parcours final à deux phases ne sont pas comparables non plus.
Aucun benchmark avant/après comparable de lecture, CPU ou RSS n'a été réalisé.

PR proposée : `fix(ci): build Docker images from the checked-out SHA`.
[Contrat et risque de résolution locale des fichiers](CI_DOCKER_A01_2026_11_03.md).

## Sortie et prochaine priorité

Les trois corrections sont validées **localement** ; les CI distantes ne sont
pas déclarées vertes. Restent : nouvelle CI Linux après publication, événements
PR/push/dispatch et release/tag réels, lecture et métadonnées sur le NAS mis à
jour. Pas de preuve de sortie audio physique 5.1/7.1, de deux machines physiques,
d'endurance de film long, d'Edge/Safari ou de réseau fournisseur réel. Le PCM
navigateur ne prouve pas le câblage des enceintes.

Prochaine tâche : intégrer les trois petits lots séparément, observer les CI,
puis reprendre A03/A04 avec le diagnostic NAS et des mesures froid/chaud
comparables. Les captures, erreurs intermédiaires et limites restent conservées.
