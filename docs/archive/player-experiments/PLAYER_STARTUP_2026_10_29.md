# Démarrage et piste audio externe — 29 octobre 2026

## Résultats observés

- `scripts/browser-external-progressive-poc.cjs` : Chrome 156.0.8078.48,
  moteur TypeScript de production, original vidéo intentionnellement indisponible,
  piste indépendante `external-0` Opus et repli vidéo AV1 fMP4 progressif.
  Une frame observée après `player.play()` et reprise à 2 s apparaît à 875 ms ; PCM
  décodé non nul ; seeks 20 → 2 → 31 s en 251 / 301 / 354 ms, vidéo/audio
  synchronisés et aucune erreur. Les requêtes vidéo demandent explicitement
  les positions correspondantes. Le serveur HTTP et FFmpeg sont des instruments
  synthétiques, sans Core, Media Node ou NAS ; le callback est enregistré après
  le retour de `play()` et ne mesure donc pas forcément la toute première frame.
  Ce n'est pas un p95.
  Rapport : `target/player-external-progressive/report.json`.
- `scripts/browser-direct-seek-budget-poc.cjs` : Chrome et fichier synthétique
  de 352 256 545 octets ; le serveur retarde de 3,5 s le Range direct au seek
  vers 1 800 s, après réception des métadonnées. Avant (`native.ts` de `HEAD`,
  autres modules communs au candidat), le lecteur
  abandonne le direct et utilise le relais après 2,5 s : position atteinte
  en 2 980 ms, avec chargements direct puis relais. Après (modules de travail
  figés, hashes dans le rapport), le lecteur conserve le direct et atteint la
  position en 3 986 ms, sans erreur. Un seul essai par cas. Ce résultat évite
  une bascule prématurée mais est **plus lent** dans ce scénario où le relais
  simulé répond immédiatement ; il ne prouve aucun gain de latence général.
  Un changement ultérieur du repli après expiration n'est pas dans ce snapshot.
  Rapport : `target/player-direct-seek-budget/report.json`.
- Parcours Core standalone + Chrome avec `browser_scanner` précompilé et Web
  existant : la première phase MP4 a réussi scan, piste externe `external-0`,
  PCM, seek en pause, progression et reprise. Son rapport est
  `target/player-external-baseline/report.json` ; les chiffres d'ouverture ne
  mesurent pas exclusivement la piste externe. Après redémarrage, le processus
  ne produisait que l'initialisation du Core, sans trace média, capture ni
  rapport MKV. La phase a été interrompue après plus de quatre minutes pour
  libérer `media-poc.exe` et permettre la compilation des correctifs. Le test
  Rust complet est donc **interrompu/échoué** ; aucune cause précise ni succès
  MKV ne peut être déduit. Aucun NAS ni Node sur une seconde machine.

Les sources des modules injectés sont figées au début de chaque POC ; leurs
hashes figurent dans les rapports. Les fichiers de test sont des fixtures
synthétiques, sans modification de médias personnels. Le repli de la piste
externe démontre le lecteur avec des URL fournies au banc, pas la construction
des descripteurs par Core ni les permissions Node : celles-ci nécessitent leurs
propres tests. Aucun résultat n'établit un démarrage instantané universel.

```powershell
node scripts/browser-external-progressive-poc.cjs
node scripts/browser-direct-seek-budget-poc.cjs
$env:MEDIA_REPORT_DIR='target/player-external-baseline'
& .\target\debug\deps\browser_scanner-009ac52d3ea5fe01.exe --ignored --nocapture --test-threads=1
```
