# Expériences lecteur (P0C)

Pages et scripts des expériences de lecture archivées dans `docs/archive/player-experiments/`.
Ils ne font pas partie de l'interface produit, mais restent compilés dans le Core par `include_str!`
(`src/lib.rs`) :

| Fichier | Route |
|---|---|
| `mse.html`, `mse.js` | `/mse`, `/mse.js` |
| `shaka.html`, `shaka.js` | `/shaka`, `/shaka.js` |
| `bench.html`, `bench.js` | `/` en mode fixture-bench, sinon `/bench` ; `/bench.js` |
| `audio-seek-policy.js` | `/audio-seek-policy.js` |

`audio-seek-policy.js` est couvert par `scripts/audio-seek-policy.test.cjs` (CI) et `tests/p0.rs`.
Supprimer un fichier implique de retirer sa route et ces tests.
