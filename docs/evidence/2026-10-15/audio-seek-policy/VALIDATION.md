# Validation locale, 2026-10-15

Windows ; Shaka 5.2.13. Versions navigateur dans chaque JSONL.
Les fichiers v1/v2 précèdent le watchdog ; final-* incluent watchdog et opt-in.
La dernière distinction audioId/marge0 a été ajoutée ensuite et testée unitairement,
sans nouvelle matrice navigateur (le chemin positif .5 ne change pas).
Pilote invalide conservé : choix manuel sous-titres omis ; ne pas exploiter ses résultats.
Chrome-v2 conserve les avertissements de préparation sans effacer le résultat strict.
Les 2 échecs Firefox concernent POST /session, AbortSignal.timeout(45000),
ports4444 puis4455 ; 0 test média. Le champ end n'implique pas une session réussie.
Les tentatives Edge à chemin binaire implicite ont échoué avant lancement et ne sont
pas des mesures de lecture ; relance avec le binaire EdgeCore installé.

Commandes réellement exécutées :
- cargo fmt --check
- cargo clippy --all-targets -- -D warnings
- cargo test
- node --test scripts/audio-seek-policy.test.cjs scripts/player-comparison-stats.test.cjs scripts/safe-margin-stats.test.cjs
- node scripts/browser-seek-policy.cjs, variantes MEDIA_BROWSER_CHANNEL, MEDIA_REPORT_DIR,
  MEDIA_BROWSER_EXECUTABLE, MEDIA_WEBDRIVER_PORT, filtres et préparation décrits dans le rapport.

Anciennes suites présentes, pas de nouvelle matrice exhaustive MSE/native lors de ce jalon.
Aucune mesure CPU/RAM, écoute physique multicanal ou synchronisation physique.
37 hashes avant/après par série ; end.unchanged=true pour les séries complètes.
summary.json ne mélange pas les essais finaux, dont les protocoles diffèrent.
Les silences et marqueurs ne constituent pas une preuve de continuité.
