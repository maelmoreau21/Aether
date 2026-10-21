# Vérification locale — 21 octobre 2026

Windows 10.0.26300, Chrome 155.0.8059.85 headless, Web construit en production.
Les données, comptes et médias des tests sont synthétiques et temporaires.
Aucun déploiement, push, publication ou changement des conteneurs utilisateur.

## Résultats

- Web : `npm --prefix apps/web run check` (0 erreur/0 avertissement), `run lint`,
  `test` (68 tests), `run build` réussis. Sortie de build : `../web-build.log`.
- Node : `cargo test --locked -p media-node --lib -j 1 -- --test-threads=1` :
  42 réussis, un helper ignoré lancé par son test parent.
- Miniatures en release : 2 réussis, mesures et limites dans
  [image-preview-release.md](../image-preview-release.md).
- Core : `cargo test --locked -p media-poc --lib extensions::repositories -j 1 -- --test-threads=1` :
  2 réussis, scénario navigateur ignoré puis exécuté séparément.
- `cargo test --locked -p media-poc --test posters_flow --test distributed_multi_node --test node_enrollment_and_relay -j 1 -- --test-threads=1` :
  3 réussis. Miniatures locales/différentes largeurs et autorisation, mêmes
  octets de miniature via Core et accès direct signé, deux Nodes et canal sortant.
  Le test du canal sortant ne contient pas d'image ; sa nouvelle option de largeur
  n'est donc pas directement couverte dans ce scénario.
- Depuis `apps/web`, `node scripts/test-pwa.mjs` : téléchargement navigateur
  natif avec nom/octets exacts, absence de nouvelle copie IndexedDB, copies
  anciennes et clé d'appareil, récupération des copies avec phrase, Opus décodé
  et signal PCM 440 Hz, pause/seek/Range, suppression durable et reconnexion.
  Serveur HTTP de fixture, sans preuve d'installation PWA sur appareil physique.
- Dépôts : scénario ignoré `browser_repository_catalogue_version_trust_and_install`
  exécuté avec le binaire lib compilé par Cargo (exécution directe pour éviter
  le verrou de compilation pendant les autres tests). Vrai Core/SQLite et
  navigateur, documents HTTPS injectés uniquement sous `cfg(test)` :
  [rapport](repositories/report.json), captures desktop/mobile. Le test a découvert
  puis fait corriger le nom accessible du sélecteur de versions. Aucun dépôt
  distant public ou compte fournisseur réel n'a été utilisé.
- `cargo test --locked -p media-poc --test browser_posters -j 1 -- --ignored --nocapture --test-threads=1` :
  initial/redémarrage/modification/suppression réussis. Les cartes demandent
  effectivement `width=320`, le fond décoratif de 2 Mo n'est pas demandé,
  Films est visible pendant l'attente de Séries. Première affiche : 510 ms
  desktop, 552 ms mobile ; minuscule fixture, observations uniques sans A/B.
- `cargo test --locked -p media-poc --test browser_series -j 1 -- --ignored --nocapture --test-threads=1` :
  [rapport](series/report.json), notes rapides persistées pendant réponse retenue,
  liste créée et média ajouté via +, mono-version/mono-audio masqués, multi-version
  conservée, 7 images vidéo décodées, seek/pause, chapitres, épisodes et sessions.
  Clic vers lecture observée : 1890 ms, incluant automatisation, préparation et
  mode debug du Core. Ce n'est pas un comparatif avec Jellyfin ni une mesure NAS.
- Syntaxe PowerShell des scripts Aether-Plugins et `node --check` des trois
  scripts navigateur : réussis. Générateur de manifeste essayé sur un paquet
  local signé ; aucun hébergement/publication créé.
- Contrôles finaux : `cargo fmt --all -- --check` et
  `cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings` réussis.

## Limites

Docker Desktop est indisponible (pipe Linux absent) : taille compressée finale,
RSS Linux et RAM après cinq minutes non mesurées. Le Dockerfile retire un
exécutable Core doublon et borne les arènes glibc ; aucun gain chiffré revendiqué.
Pas de PostgreSQL réel, NAS, WAN, comparatif Jellyfin ni appareil physique dans
ce lot. Les originaux restent en lecture seule. La mémoire du cache d'images
n'est pas un quota RSS ; le relais distant ne transmet pas l'ETag de réponse.
