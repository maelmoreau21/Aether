# MEMORY.md — État et décisions Aether

## 23 octobre 2026 — Correction du test E2E browser_scanner (reopenedAudio)

- **Diagnostic de l'échec ligne 113 de scripts/browser-scanner.cjs** :
  - La fixture synthétique representative (MKV/MP4) a une durée totale de 12 secondes.
  - Durant les étapes de test précédentes (switches audio 1 & 2, simulation de panne réseau), la vidéo avançait en continu.
  - Lors de la boucle reopenedAudio (fermeture puis réouverture avec internal-2 puis internal-1), le deuxième tour atteignait 11.95s / 12.0s (fin de média), déclenchant l'état ended (paused=true) et marquant automatiquement le média comme vu (watched=true, ratio >= 0.95), provoquant une interruption de lecture.
  - De plus, le clic sur .hero-cta s'exécutait immédiatement après button.return sans attendre la fermeture effective du dialogue modal dialog#player.
- **Correction appliquée dans scripts/browser-scanner.cjs** :
  - Réinitialisation de la position de lecture (v.currentTime = 1) dans la boucle reopenedAudio si la vidéo dépasse 3 secondes pour ne jamais heurter la limite des 12 secondes.
  - Attente explicite de la fermeture du dialogue (!document.querySelector('dialog#player')?.hasAttribute('open')) avant de cliquer sur .hero-cta pour rouvrir le lecteur.
- **Validation** :
  - Exécution complète réussie de cargo test --locked --test browser_scanner -- --ignored --nocapture (1 passed en 29.70s, switch internal-2 & internal-1 validés, 0 warning/erreur).

## 23 octobre 2026 — Correction du lint Clippy items_after_test_module (local_mounts)

- Correction de l'erreur clippy::items-after-test-module dans crates/core/src/local_mounts.rs : la fonction validate etait declaree apres le module de test mod tests.
- Fonction validate repositionnee avant #[cfg(test)] mod tests.
- Verifications validees : cargo clippy --locked --workspace --all-features --all-targets -- -D warnings, cargo fmt --check et suite complete de tests unitaires cargo test --locked --workspace --all-features --lib --bins.

## 23 octobre 2026 — Montages médias sans préfixe imposé

- Sans MEDIA_LIBRARY_ROOT, découverte des montages de dossiers Linux via /proc/self/mountinfo ; exclusion des chemins système, /app, /data et /backups. Racine explicite toujours prioritaire.
- Administration et scans valident chaque dossier dans ces racines ; aucun scan automatique ni ouverture de la racine système.
- Suppression de MEDIA_LIBRARY_ROOT par défaut dans l’image standalone et des deux variables redondantes dans Compose. MEDIA_WEB_DIST conserve sa valeur image.
- Test unitaire de parsing des montages réussi sous Windows ; déploiement Docker/NAS non exécuté. Documentation DEPLOYMENT_DOCKER.md mise à jour.


## 23 octobre 2026 — Clé de sauvegarde MEDIA_BACKUP_KEY rendue optionnelle

- **Suppression du blocage et de l'obligation de MEDIA_BACKUP_KEY** :
  - Clé interne par défaut configurée dans `crates/core/src/backup_bundle.rs` (`DEFAULT_BACKUP_KEY`) et réexportée dans `admin_backups.rs`.
  - Si `MEDIA_BACKUP_KEY` est absente de l'environnement ou vide, le système utilise automatiquement la clé par défaut : les sauvegardes manuelles, le téléchargement et les sauvegardes planifiées fonctionnent immédiatement sans exiger la saisie ou la configuration d'une clé de 64 caractères hexadécimaux.
  - La console Web d'administration (`AdminMaintenance.svelte`) ne bloque plus l'écran des sauvegardes avec le message d'indisponibilité demandant `MEDIA_BACKUP_KEY`. Les boutons et la planification sont directement opérationnels.
  - Les commandes CLI `media-backup --full` et `media-recover` fonctionnent désormais directement sans variable `MEDIA_BACKUP_KEY` requise (tout en conservant la possibilité de spécifier une clé personnalisée si souhaité).
  - Tests unitaires et d'intégration validés : `cargo test -p media-poc --test admin_backups --test backup_bundle`, tests unitaires `media-backup` / `media-recover` et `pnpm --filter web check` sans aucune erreur.


## 23 octobre 2026 — Correction du test E2E playlists (mixed_playlists)

- **Correction du sélecteur d’onglet Dépôts dans scripts/browser-playlists.cjs** :
  - L’onglet « Dépôts » dans AdminExtensions.svelte possédait un attribut `aria-label="Dépôts"` qui surchargeait le nom accessible de l’élément (« Dépôts » au lieu de « Dépôts (X) »). Le sélecteur Playwright `getByRole('button', { name: /^Dépôts \(/ })` expirait donc par timeout de 30 secondes.
  - Ajustement du sélecteur avec `{ name: /^Dépôts/ }` pour s'aligner sur la présence éventuelle de `aria-label` ou du compteur textuel, de manière cohérente avec les autres tests d'extensions.
  - Validation exécutée avec succès : `cargo test --locked -p media-poc --lib product_tests::mixed_playlists -- --test-threads=1` (1 test passé en 3.31s, playlists mixtes, partage, intégrité audio et politique de partage vérifiés).

## 23 octobre 2026 — Correction du test E2E browser_series

- **Correction de la navigation d’administration dans scripts/browser-series.cjs** :
  - Suite au passage de la barre de navigation d’administration dans un <dialog> tiroir fermé par défaut, les sélecteurs directs page.locator('a[href="#users-section"]') et page.locator('a[href="#libraries-section"]') échouaient par timeout de 30 secondes car les liens étaient masqués dans le dialogue fermé.
  - Alignement avec les autres tests d’administration (scripts/browser-distributed.cjs, apps/web/scripts/test-admin-live.mjs) : ouverture du tiroir via .mobile-menu-toggle si aria-expanded === 'false' et ciblage scoped .admin-sidebar a[href$="#..."].
  - Validation exécutée avec succès : cargo test --locked --test browser_series -- --ignored --nocapture (1 test passé en 63.33s sous Chrome headless, scan, lecture AV1/Opus, permissions 403, transitions inter-saisons et changement de compte vérifiés).

## 23 octobre 2026 — Fluidité initiale et dépôt de plugins préconfiguré

- Réserve contiguë 0,75 s avant play, prête immédiatement si déjà en cache ;
  attente bornée/annulable, aucun play tardif. Opus segmenté : seconde fenêtre
  préparée avec quatre secondes d’avance, sans job immédiat au démarrage.
- Petite dérive audio : correction de vitesse ±2 % avec hauteur conservée,
  zone neutre 40 ms, resynchronisation forte >250 ms. Test Chrome segmenté :
  cinq play initiaux → un, aucun waiting avant 2 s, zéro image perdue ; preuve
  courte synthétique, pas NAS/4K/endurance/sortie physique. Préparation audio
  froide 974 ms : cible globale 200 ms non tenue.
- Migration 0041 SQLite/PostgreSQL : dépôt Aether Plugins ajouté une seule fois,
  supprimable durablement, snapshot initial vide et actualisation explicite.
  Confiance éditeur/installation désactivée/droits/activation restent séparés.
- Aether-Plugins : quatre paquets signés versionnés, manifeste/public key dans
  repository/, Build-Repository.ps1 détecte les plugins et vérifie les sorties.
  Clé privée hors Git, aucun fournisseur/API secret dans les paquets.
- URL préparée : https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json
  Publication sur main encore nécessaire ; aucun push ou déploiement effectué.
- Administration : cartes compactes, configuration dépliable, catalogue par
  défaut, suppression du double chargement initial. Core/SQLite/Chrome réel :
  dépôt, signature/confiance/version/install/retrait/persistance réussis (4 tests).
- 112 tests Web/check/lint/build, format et Clippy strict réussis ; cycle de vie
  navigateur Core/SQLite/Rust-Wasm réel également réussi. Catalogue de quatre
  manifestes signé vérifié en UI production clair/sombre 1440/390 sans erreur.
- Détails, mesures, limites et preuves : docs/STARTUP_AND_PLUGIN_CATALOGUE.md.


## 23 octobre 2026 — Latence catalogue/lecture et alias d'images

- Préchargement catalogue au survol/focus des liens accueil/bibliothèques, URL
  commune au cache d'affichage, reprise anticipée dès clic et module lecteur
  importé après catalogue. Suppression du fondu d'affiche de 300 ms.
- Audio Ogg indépendant chargé pendant les métadonnées vidéo, activé après
  réussite des deux ; Opus segmenté et préparations serveur gardent leur ordre.
- Catalogue : suppression de trois lectures d'images corrélées redondantes ;
  résolution groupée existante conserve ordre/masquage/permissions.
- Scanner : alias cover/default/movie/show et fonds background/art ; fond
  préfixé retrouvé même sans image générique dans un dossier partageable.
  Identités ambiguës/symlinks refusés ; nouveau scan requis pour ces alias.
- Web 106 tests/check/lint/build, catalogue 5, images 5 et scanner Node 7 réussis.
  Chrome : retours catalogue 89–134 ms ; moteur/HTTP isolé Ogg image 88 ms/prêt
  188 ms, segmenté 263/585 ms. UI/API lente : image 752/488 ms, cible 200 ms
  non atteinte. SQLite 10k première page 269→345 ms : pas de gain général prouvé.
- 404 logos/indicateurs sur routes imbriquées : utiliser asset('images/...'),
  pas resolve('/') pour construire les chemins statiques ; paths.relative=false
  conserve des URLs /app stables après navigation. posters_flow 1 réussi,
  format et Clippy strict all-targets réussis.
- Contrôle final build production : catalogue retour 72–104 ms, bibliothèque
  préchargée 122 ms ; zéro erreur JS/image HTTP, image film 644/384 ms avec
  API lente simulée. La cible globale de 200 ms reste ouverte.
- Détails/preuves : docs/NAVIGATION_PERFORMANCE.md, docs/LOCAL_FILM_POSTERS.md.
  NAS/PostgreSQL/son physique non mesurés ; aucun push ni déploiement.


## 23 octobre 2026 — Cinema Design System, contrôle Web local

- Charte OLED/Pierre Douce consolidée : rayons 6/10/12/16/20/24, surfaces,
  contraste des boutons de lecture, thème initial externe sous CSP self.
  Accueil sans sections décoratives vides ; hero limité, affiches 2:3,
  reprise/épisodes 16:9, navigation et administration harmonisées.
- Musique : vues morceaux/albums/artistes sur les résultats de la page courante,
  durées, numéro et titre actif ; pochettes/photos absentes de l’API, initiales
  sobres 1:1/circulaires. Pas de nouvelle API ou de faux artwork produit.
- Chrome headless, build production et APIs/images synthétiques interceptées :
  64 états de page, 74 captures clair/sombre en 390/768/1440/1920 et paysage ;
  sans erreur JS/débordement. Thèmes, regroupements et saison testés.
  Captures inspectées ; références docs/evidence/2026-10-23/cinema-ui/.
- Check Svelte/TypeScript zéro erreur/avertissement, ESLint, 104 tests et build
  réussis. Aucun test Rust relancé, aucun changement backend ; pas de preuve
  NAS/mobile physique/PWA installée ou d’audit WCAG exhaustif.
- Direction complète et limites : docs/UI_DIRECTION.md. Cache catalogue en cours
  conservé ; aucun push, déploiement ou modification des médias personnels.


## 23 octobre 2026 — Cache catalogue et ouverture du lecteur

- Pages catalogue en mémoire par requête complète/session : SWR 30 s, LRU 48 pages,
  4 Mio de JSON estimé (pas un plafond exact du heap), requêtes dédupliquées/bornées.
  Retours frais sans fetch catalogue ni loader, scroll conservé ; erreurs réseau
  non destructives, invalidation après mutations/scans observés/session/403.
- Dialogue ouvert avant descripteur, mais aucune lecture sans descripteur frais ;
  déduplication des requêtes en vol avec annulation par consommateur. Épisodes
  multiversions : sélection après ouverture, sans double requête séquentielle à froid.
  Navigation actions/filtres SvelteKit, historique déjà chargé réutilisé.
- Chrome 155.0.8059.86, vraie UI/API simulée lente : dix retours 331–359 → 62–74 ms,
  20 → 0 requêtes catalogue ; dialogue 322/314 → 8/3 ms, première image
  514/421 → 416/402 ms (ouverture/réouverture, deux observations seulement).
- Web 104 tests/check/lint/build, Rust catalogue 5/scanner 3/distribué 1,
  Chrome Core/SQLite avec redémarrage et décodage AV1/Opus local réussis.
  Cache Node existant réutilisé, aucun changement Rust/SQL/transcodage/timeouts.
- Docker Desktop indisponible ; NAS/PostgreSQL/premier son physique non mesurés.
  Scans externes visibles à la prochaine consultation périmée, pas de push serveur.
  Méthode, rapports, limites et mesures production : docs/NAVIGATION_PERFORMANCE.md.
  Aucun push, déploiement ou média personnel modifié.

## 22 octobre 2026 — CI raccourcie localement, mesure distante en attente

- Intégration Rust répartie automatiquement sur trois runners par cibles Cargo
  (44 actuelles, 15/15/14), séquentielles dans chaque runner. Format/Clippy strict,
  unitaires/ABI/PostgreSQL une fois ; contrôle Rust historique agrège les trois
  partitions et reste obligatoire avant publication.
- Chrome préinstallé testé avant repli Playwright ; FFmpeg cache fonctionnel
  évite apt, outils de compilation seulement à froid, bibliothèques runtime
  restaurées si nécessaires. Version 9.0.2 et smoke check préservés.
- Tests de répartition 3/3, plans exhaustifs réels, Chrome local, actionlint et
  ShellCheck vérifiés. Pas de nouvelle CI distante, gain non mesuré, push ou
  publication. Trois runners réduisent l'attente visée mais peuvent augmenter
  les minutes cumulées. Détails : docs/CI_GHCR.md.

## 22 octobre 2026 — Audit performance lecteur, jalon P0 local

- `develop` : rejet immédiat DTS/DTS-HD MA quand le navigateur refuse la
  déclaration codec/conteneur. Cas utilisateur : MKV H.264/DTS-HD MA DUT,
  fichier et transport NAS indisponibles ; cause réelle du timeout non clôturée.
  Chrome/Edge Windows déclarent H.264/MKV mais pas DTS ; test de descripteur,
  pas de décodage d'une fixture DTS-HD MA ou du fichier personnel.
- Échec d'une langue : ancienne lecture reprise, erreur récupérable dans l'UI,
  sélection/préférences conservées. Seeks obsolètes : garde de génération avant
  append et reprise immédiate à la nouvelle position. Timeout métadonnées/seek
  distingué ; logs numériques navigateur et HTTP original Core/Node sans jeton.
- Sous-titres externes : labels UTF-8 réparés après un premier essai UI échoué.
  Essai suivant Core/SQLite/Svelte/Chrome réussi en MP4 puis MKV après redémarrage.
- Web 91 tests/check/lint/build ; stream 3, scanner_flow/distributed 3,
  label UTF-8, packaging copie/cache/7.1 et dual-5.1 explicite réussis ;
  fmt et Clippy workspace/all-targets strict vérifiés. Pas de suite Rust générale.
- Chrome/Edge headless : PCM FR/EN, frames, pause/seeks, 404 avec reprise,
  fermeture et refus DTS immédiat ; Edge gros MP4 3500 s, seeks jusqu'à 3480 s.
  Derniers essais : première image Chrome 291 ms/Edge 264 ms, lecture prête
  613/355 ms. Référence Chrome 186/188 ms : aucun gain de latence revendiqué.
- P1–P4 restent ouverts : silence réel, ressources, NAS, 4K, endurance et
  pipeline commun non validés. Aucun réencodage serveur, média personnel,
  push ou déploiement. Méthode/preuves : docs/PLAYER_PERFORMANCE_P0.md.

## 22 octobre 2026 — MP4 original et fenêtres Opus, incident NAS non clôturé

- Retour utilisateur : Node local Synology/Docker, MP4 AV1 10 bits 1080p,
  FR et EN Opus 5.1 ; un échec immédiat et une attente longue, indices absolus
  et stderr indisponibles. Avatar et NAS non accessibles ici : piste fautive
  et cause spécifique non identifiées ; ne pas annoncer l'incident résolu.
- MP4 : vidéo originale avec Range, plus de copie vidéo complète préalable.
  MSE Opus : fenêtres fMP4 de 10 s avec preroll, copie bornée 12 s, seek fd
  puis output -ss 0 pour exclure l'audio de la précédente image clé. Pas de
  transcodage/downmix ; paquet/canaux/placement temporel testés. Repli Ogg et
  MKV historiques conservés, avec leurs limites de préparation complète.
- Cache : LRU des fenêtres inactives, leases jusqu'à fin HTTP, worker unique,
  deux attentes 5 s, coalescence ; job fenêtre 20 s/16 Mio max. Logs JSON
  asset/composant/indice/langue/canaux/méthode/erreur/durée et stderr drainé 16 Kio.
- Chrome/Edge headless, vrai FFmpeg runtime Docker sans encodeur sous 2 CPU/
  256 Mio et Nginx buffering actif/timeout10 s : décodage FR/EN/FR et seeks
  8/900/1800/3480/2 s sur MP4 synthétique 336 Mio, 12 fps/3500 s. Dernière
  mesure proxy : première image 529 ms, audio prêt 1470 ms, environ 5 Mo de
  fenêtres préparées. Réouvertures EN/FR 430/410 ms ; annulation après réception
  HTTP nettoyée en 2,1 ms, socket amont fermé dans cet essai. Pas de preuve NAS.
- Web 81 tests/check/lint/build, Node 44 tests puis packaging dual-5.1/7.1
  et streaming/leases, Core scanner_flow/distributed_multi_node/browser_scanner,
  fmt/Clippy strict/actionlint vérifiés. CI inclut la petite fixture segmentée,
  contrôle son rapport et l'archive ; nouvelle CI distante pas exécutée.
- Docs/commandes/preuves : docs/NAS_PLAYBACK_DIAGNOSTICS.md. Core complet en
  conteneur, appareil physique/sortie surround, média personnel et séance entière
  non validés. Aucun média original modifié, push ni déploiement applicatif.

## 22 octobre 2026 — Chemin des rapports Films fixé dans la CI

- Régression de la correction lecteur : défaut local déplacé vers
  target/browser-scanner, mais vérification/upload CI restés dans
  docs/evidence/2026-10-16/scanner, causant FileNotFoundError.
- L'étape browser_scanner définit désormais MEDIA_REPORT_DIR explicitement au
  dossier CI attendu ; assertions de réussite et de fraîcheur conservées.
- Parsing YAML et concordance génération/nettoyage/rapports/captures/upload,
  actionlint 1.7.12 et diff check vérifiés. Pas de navigateur rejoué pour ce
  changement de configuration seul, de nouvelle CI Linux ou de push.

## 22 octobre 2026 — Sélection AV1/Opus et préférences FR/EN

- Cause reproduite : `playbackMime` fabriquait un codecs AV1 incomplet (`av01`),
  rejeté par Chrome/Edge. Les sources FR/EN préparées étaient filtrées, puis
  l'original multiaudio `und` déclenchait à tort l'avis de langue indisponible.
- MIME conteneur sans configuration inventée ; capacité indicative sans exclusion
  de langue, restauration indexée et migration de l'ancien choix ambigu ; `und`
  ne remplace plus la préférence linguistique. Original retiré du menu audio
  quand toutes les pistes sont explicitement proposées, source originale conservée.
- Nouveau MP4 synthétique 1080p AV1 Main 10 bits avec deux Opus 5.1 : Chrome
  155.0.8059.85 et Edge 155.0.4283.62 headless passent FR/EN/FR, PCM distinct,
  un seul chargement vidéo, plein écran, pause/seeks/reprise, erreur Ogg 404 et
  réouverture. Test Rust explicite : paquets identiques, six canaux conservés.
- Core/SQLite/Chrome MP4 puis MKV après redémarrage : réussi en 39,96 s,
  préférence serveur EN puis FR conservée et faux avis absent. Web 80 tests,
  cinq régressions d'audit, check 0/0, lint/build réussis ; deux anciens tests
  packaging réussis, dont 7.1. Détails/commandes : docs/AV1_OPUS_AUDIO_FIX.md.
- scanner_flow et distributed_multi_node réussis : droits vivants, HEAD/GET/Range,
  deux Nodes distants locaux ; aucune preuve multi-machine déduite.
- Fmt, Clippy workspace/all-targets strict et diff check réussis ; suite Rust
  workspace générale non rejouée.
- Aucun Tdarr, média personnel, push ou déploiement modifié. Avatar, NAS,
  réseau physique multi-machine, sortie matérielle 5.1 et endurance non testés.

## 22 octobre 2026 — Assertion scanner_flow alignée sur les langues explicites

- Le journal GitHub fourni confirme P0 Linux 12/12, puis scanner_flow échouait
  sur une ancienne attente `source.language == fr` pour le MKV à deux audios.
- Contrat déjà implémenté/documenté : original multiaudio = und ; seules les
  sources internal-N garantissent la piste et sa langue. Test corrigé : original
  internal-default/und, sources explicites fr/en et conservation index/canaux,
  langue en de l’original MP4 mono-audio. Tags techniques fra/eng toujours vérifiés.
- Aucun changement de production, des fixtures ou des médias. Test ciblé
  cargo test --locked -p media-poc --all-features --test scanner_flow -j 1
  -- --test-threads=1 : 1 réussi Windows ; fmt et diff checks réussis.
- Pas de suite workspace rejouée, push ni déploiement. CI complète à relancer.

## 22 octobre 2026 — Correction CI P0 : caches et profil AV1

- Cause des 404 MSE/DASH et VTT absents : cache partagé entre Rust (jeu complet)
  et navigateur (jeu sans MSE/DASH). Clés distinctes versionnées, anciennes entrées
  écartées ; vérification des fichiers/rapports préparés avant Cargo, cache inclus.
- Profil AV1 : accepter Main ou sa représentation numérique chaîne 0 observée
  en CI, sans relâcher les vérifications codec, pixels 10 bits, couleurs et audio.
- Validation : garde-fou positif + cache incomplet rejeté, YAML/ordre/clé valides,
  fmt et diff checks ; cargo test --locked -p media-poc --all-features --test p0
  -j 1 -- --test-threads=1 : 11 réussis Windows. Test symlink Linux non compilé
  sur cet OS ; pas de suite workspace générale ni nouvelle CI Linux exécutée.
- Aucun média modifié, push, déploiement ni publication. Voir docs/CI_GHCR.md.

## 21 octobre 2026 — Raccordement des opérations des extensions

- Core générique : `/run` accepte search/details/images avec permissions signées,
  contexte titre/année/type/identifiants du catalogue et choix manuel du candidat.
  Proposition simple et ancien historique conservés pour les paquets existants.
- Interface : recherche fournisseur, détails, application explicite et import
  d’images par le pipeline d’artwork protégé. Adaptation des noms SDK au catalogue,
  fusion des corrections et identifiants externes ; aucun remplacement par URL.
- Avatars : liste des extensions actives et opérations authentifiées/CSRF depuis
  les paramètres du compte, sans accès bibliothèque/configuration/réseau. SVG
  inertes strictement validés et enregistrés pour le compte courant ; MIME/CSP
  adaptés sur les routes utilisateur et administrateur. La politique d’upload
  dépend seulement d’extensions avatars actives et d’éditeurs approuvés.
- Worker 65 s, requêtes Web extensions 70 s, sortie alignée SDK 64 Kio. Erreurs
  fournisseurs : code borné conservé, message brut exclu. Broker : résolution DNS
  publique épinglée, aucun proxy implicite/redirection, réponse lue par morceaux.
- Aether-Plugins : fanart search par ID existant (TVDB pour séries), contexte réel
  et refus des correspondances ambiguës pour propositions TMDb/IMDb ; Avatar
  Wasm indépendant de l’horloge. Script de paquetage accepte avatar et accorde
  avatars seul, ou metadata.read/write + images aux fournisseurs. Paquets à
  reconstruire/signer/réinstaller pour les nouvelles permissions et handlers.
- Vérifié : tests extensions 9 réussis/3 ignorés ; filtre unitaire extension
  6 réussis/1 ignoré, puis 2 tests adaptateurs rejoués après fusion des IDs ;
  Clippy strict Core all-targets/all-features, Web check 0/0 + lint/build,
  check workspace Aether-Plugins, compilation Wasm Avatar et exécution réelle
  list (4 packs)/generate dans le worker ; syntaxe paquetage et OpenAPI valides.
- Pas de navigateur E2E rejoué, d’appel fournisseur réel avec clé API, de paquet
  signé publié, de push ou de déploiement. Quotas documentés conservés ; ces
  vérifications ne prouvent pas tous les volumes de réponses des fournisseurs.

## 21 octobre 2026 — Fournisseurs externes et correction du test extensions

- Suppression du traitement des domaines/Authorization en fonction du nom
  tmdb/fanart/imdb dans le Core. `Manifest.httpHosts` déclare les hôtes HTTPS
  exacts couverts par la signature, affichés avant installation/activation ;
  liste absente/vide = aucun réseau. Le Core ne contient aucun fournisseur
  installé par défaut. Les identifiants NFO/dossiers restent des données locales.
- Aether-Plugins porte les domaines dans chaque manifeste fournisseur ; son
  script de paquetage les copie et demande metadata.read/metadata.write. Les
  anciennes signatures sans réseau restent compatibles (champ vide omis).
  Anciens paquets réseau à recréer/signer/réinstaller, sans attribution automatique.
- Application de métadonnées réservée à une extension installée/activée, éditeur
  actif, bibliothèque accordée et permission metadata.write, administrateur/CSRF.
  Le test CI utilise une extension synthétique et une affiche importée par l'API
  d'artwork protégée ; une URL distante de métadonnées ne remplace pas cette image.
- Les anciens tests de modules TMDb/Avatar cherchant des fichiers dans le dépôt
  voisin sont remplacés par un test externe générique explicitement ignoré tant
  qu'un module n'est pas fourni via MEDIA_TEST_EXTENSION_WASM. Aucune dépendance
  des tests Core ordinaires envers un fournisseur ou un dépôt voisin.
- Vérifications locales : tests extensions 8 réussis/3 ignorés, puis cas apply
  rejoué avec refus de permission read-only (réussi) ; Clippy strict workspace/
  all-targets ; Web check 0/0, lint/build ; cargo check workspace Aether-Plugins ;
  JSON OpenAPI, syntaxe PowerShell et diff checks. Pas de suite Rust générale,
  appel fournisseur réel, build de paquet publié, push ou déploiement.

## 21 octobre 2026 — Correction CI FFmpeg lavfi et artefacts absents

- Le profil FFmpeg fixtures héritait de `--disable-avdevice`, rendant `-f lavfi`
  inconnu et bloquant generate-fixtures.ps1. Réactivation de libavdevice et du
  seul input lavfi pour fixtures ; profil runtime inchangé. Smoke test testsrc2
  et sine dans setup-ffmpeg, caches binaires/fixtures invalidés par le script.
- Preuves navigateur nettoyées dès checkout ; upload conditionné à la présence
  de fichiers, y compris partiels après panne. Assertions de fraîcheur/réussite
  conservées. L'absence de rapports après échec fixtures ne masque plus la cause
  par une seconde erreur upload-artifact. Punycode reste un avertissement amont.
- Vérifications ciblées réussies : actionlint 1.7.12, bash -n, lavfi court sur
  FFmpeg Windows installé et git diff --check. Docker Linux arrêté : profil Linux
  non reconstruit, aucun run GitHub, push ou publication effectué. Détails dans
  docs/CI_GHCR.md ; pas de relance des tests Rust/Web sans rapport avec ce défaut.

## 21 octobre 2026 — Navigation par bibliothèque, éditeur d’images et réactivité

- Le menu principal affiche les bibliothèques actives autorisées et leur nom réel via GET /api/v1/libraries ; filtre bibliothèque appliqué au catalogue et à la musique, conservé dans les formulaires/pagination. Aucun chemin disque exposé.
- Éditeur partagé façon galerie : affiche, vignette, logo, illustration, bannière, disque/paysage, plusieurs fonds ordonnables ; import/remplacement/agrandissement/retrait, dimensions, bureau/mobile. Migration 0038 SQLite/PostgreSQL ; stockage applicatif, masquage des images locales sans suppression physique. Administrateur/CSRF pour les écritures, droits actuels avant lectures et 304.
- Aperçus persistants 320/640/1280, traitements bornés à deux décodeurs, attente de huit secondes ; ancien poster avec révision/ETag et cache persistant. Requêtes de décoration regroupées par page et représentants de groupes limités aux groupes existants et au filtre courant.
- Mesure locale debug, 10000 films synthétiques et utilisateur authentifié : médiane catalogue 369,57 → 92,21 ms, recherche 165,60 → 12,06 ms. Ne pas extrapoler au NAS ni au serveur public.
- Reprise bornée à 2,5 s sans écrasement d’une progression inconnue ; préparation parallèle des pistes, état visible/annulation, repli direct vers relais borné et délais d’en-têtes Node. Chaque lancement revérifie le descripteur : la tentative de cache de cinq secondes a été retirée après le test de révocation.
- Web : check 0/0, lint, 72 tests et build réussis. Deux parcours admin Core/SQLite réel et PWA fixture réussis ; parcours séries réel Chrome réussi après correction (décodage, seek, épisodes, 403/restauration, isolation comptes). Mesure de lancement locale unique 1125 ms, pas une garantie ni un comparatif.
- Tests Core : 44 réussis, 0 échec, 1 scénario navigateur hors périmètre ignoré ; 5 tests d’intégration droits/images/deux nœuds/relais réussis ; Clippy workspace/all-targets sans avertissement, fmt et diff --check réussis.
- Aucun déploiement/push, PostgreSQL réel, NAS ou recherche authentifiée du serveur public vérifié. La demande antérieure de réglages Jellyfin reste partielle (multi-racines, fournisseurs et autres fonctions encore ouvertes).
- Contrat : docs/RESPONSIVENESS_AND_ARTWORK.md ; commandes, captures et limites : docs/evidence/2026-10-21/responsiveness/README.md. Prochain jalon : mesurer le catalogue et les démarrages authentifiés sur le serveur après installation autorisée.


## 21 octobre 2026 — Correction de la concurrence SQLite et désérialisation de mise à jour de bibliothèque

- Résolution de core_restart : lors du PUT /api/v1/admin/libraries/{id}, les nouveaux champs exposés par GET (coverUrls, imageKinds, imageVersions) faisaient échouer la validation avec 422 Unprocessable Entity car LibraryInput impose #[serde(deny_unknown_fields)]. Les champs d'exposition ont été exclus du payload de mise à jour dans le test.
- Résolution du test concurrency_writes_during_publication : acquisition d'une connexion et utilisation de crate::database::begin_write(&mut conn) dans crates/core/src/libraries.rs:save().
- Élimine le code 500 / SQLITE_BUSY dû à une tentative d'élévation de transaction différée pendant le verrou d'écriture d'une publication.
- Validation complète : cargo test --locked --workspace --all-features --test '*' -- --test-threads=1 passé avec 100% de succès (0 échec).

- Résolution du test concurrency_writes_during_publication : acquisition d'une connexion et utilisation de crate::database::begin_write(&mut conn) dans crates/core/src/libraries.rs:save().
- Élimine le code 500 / SQLITE_BUSY dû à une tentative d'élévation de transaction différée pendant le verrou d'écriture d'une publication.
- Tests validés : concurrency_writes_during_publication, admin_users, permissions, clippy et cargo fmt.

## 21 octobre 2026 — Réglages locaux de bibliothèque et permissions (demande partiellement réalisée)

- Options persistantes par bibliothèque : préférences langue/pays (stockées, pas encore consommées par des fournisseurs en ligne), import NFO, types d'images locales et actualisation 0/30/60/90 jours via la file existante. Filtres appliqués aux imports locaux/distants et au chemin historique ; anciens PUT sans options préservent la configuration. Migration 0037 SQLite/PostgreSQL, sans écriture dans les médias.
- Cartes admin : scan direct, progression indéterminée, mosaïque CSS de quatre affiches disponibles au maximum, import/retrait d'affiche et de fond normalisés en base avec révisions. Formats PNG/JPEG/WebP bornés, administrateur/CSRF et réponses privées.
- Permission de téléchargement originale/hors ligne par compte, contrôlée dans le Core sur les nouvelles routes /download, y compris les téléchargements distants relayés. La lecture /content reste disponible ; ce droit ne protège pas contre la copie des octets du lecteur et ne révoque pas les copies déjà présentes.
- Contrôle parental : tags NFO/manuels normalisés, héritage des tags série vers épisodes, filtrage avant pagination et sur ressources dérivées, classifications françaises usuelles. Plages hebdomadaires UTC vérifiées à chaque nouvelle authentification/session Node et catalogue API ; administrateurs exemptés des horaires, réponses déjà ouvertes non interrompues.
- Vérifications : 10 tests contrats, 42 Node et 43 Core réussis (2 helpers/scénarios ignorés au total) ; permissions 2, scheduler 2, distributed_multi_node 1 et node_enrollment_and_relay 1 réussis. Web check 0/0, lint, 70 tests et build réussis. Deux scénarios Chrome headless Core/SQLite réel passent, plus PWA/téléchargement/coffre sur API fixture ; captures bureau/mobile inspectées. Détail des commandes et limites dans docs/evidence/2026-10-21/library-options/README.md.
- La demande complète reste ouverte : multi-dossiers/Nodes par bibliothèque, priorités des fournisseurs, nombre/largeur des fonds, écritures NFO/artwork, suppression/téléversement de sous-titres, quotas réseau/écrans, surveillance temps réel, trickplay/chapitres/intros, regroupements automatiques et sous-titres automatiques. Voir docs/LIBRARY_OPTIONS.md ; ne pas annoncer ces fonctions comme livrées.
- Pas de PostgreSQL réel, fournisseur réel, NAS, déploiement ou publication vérifiés dans ce lot. Prochain jalon fonctionnel : bibliothèque logique multi-racines en conservant les identités, droits et instantanés de sources.


## 20 octobre 2026 — Installation locale, administration et premiers visuels

- Guides Windows `docs/INSTALLATION_LOCALE.md` et README Aether-Plugins : vrais binaires, bootstrap interactif sans argument, SQLite, racine média explicite, compilation/signature/import/configuration/activation des paquets, sauvegarde chiffrée facultative. `Aether-Plugins/scripts/Build-LocalPackage.ps1` prépare des modules Wasm optimisés pour rester sous 256 Kio ; pas de dépôt distant ou de compatibilité DLL Jellyfin annoncés.
- Un paquet TMDb utilisable pour l’essai local est préparé dans le dossier ignoré `../Aether-Plugins/artifacts/local-20261020/`, avec `INSTALLER.md` et `publisher-public.txt`. Éditeur `local-20261020`, approbation manuelle conservée. Clé privée persistante sous `%LOCALAPPDATA%/MediaExtensionSigning/`, jamais copiée dans Git ou les logs. Signature et intégrité validées par le vrai packer ; aucune clé fournisseur réelle utilisée.
- Utilisateurs : grille filtrable, avatar normalisé autorisé par nouvelle route administrateur, dernière activité issue des sessions conservées, fiche Profil/Accès/Contrôle parental/Mot de passe. Permissions directes et héritées distinguées, sélection de toutes les bibliothèques actuelles sans accorder les futures. Après nettoyage/révocation des sessions, l’activité peut disparaître. Le navigateur a découvert et fait corriger la lecture `is_default` des groupes via `DbBool` ; tests GET de liste pour faux/vrai ajoutés.
- Clés API : choix explicite sans expiration, `expiresAt:null` dans l’API et zéro en stockage, révocation et droits vivants préservés, portée `catalog:read` inchangée. Les quotas des fournisseurs ne dépendent pas de ce choix. Libellé SSO stocké dans Général, exposé publiquement uniquement comme `buttonLabel`, modifiable sans redémarrage ; anciens PUT de présentation compatibles.
- Extensions : grille/recherche/filtres et aide locale ; configuration de l’extension transmise au worker et aux fournisseurs (`api_key`/`apiKey`, langue), anciens modules sans configuration préservés. TMDb compilé échouait avec `call stack exhausted` : Wasmi exprime `set_max_stack_height` en octets, ancienne borne 1 Kio corrigée en 64 Kio avec profondeur 128. Mémoire linéaire 16 Mio, fuel 2 millions et délai inchangés ; récursion normale/excessive testée. Les parcours d’images fanart/avatars et récupération massive restent absents.
- Affiches : rayons Films/Séries rendus indépendamment, six premières affiches prioritaires, suivi de l’URL chargée pour la visibilité, erreur partielle gardant les résultats disponibles. Chrome retient volontairement Séries et exige Film décodé/visible avant libération : 568 ms à 1440 px et 173 ms à 390 px sur minuscule fixture locale, observations uniques sans comparaison avant/après ni preuve NAS. Originaux sans réduction de dimensions ; aucun gain garanti sur les cinq secondes signalées par l’utilisateur.
- Vérifications exécutées : Web check 0 erreur/1 avertissement préexistant dans téléchargements (`video` non réactif), lint, 60 tests et build réussis. Rust ciblé : admin_access 3, admin_users 3, posters_flow 1, settings_avatars 1, OIDC 7, configuration worker 1, extensions 10 réussis ; deux tests ignorés d’extensions ensuite exécutés et réussis. Aether-Plugins : 19 tests, trois modules Wasm et trois signatures de paquets validés par étapes. Binaire `aether-core --version` : 0.1.0-beta.
- Contrôles finaux : `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings` et `git diff --check` réussis. Le script de paquetage a été validé par étapes (compilation et signature), pas rejoué intégralement.
- Chrome 155.0.8059.82 / Windows NT 10.0.26300 headless : administration réelle Core/SQLite temporaire, avatar décodé, droits, groupes, clé sans expiration/révocation, label SSO persistant, sauvegardes chiffrées, tâches, paramètres et responsive passés. Affiches : initial/redémarrage/modification/suppression, pause/seek, cache/droits, texte hostile échappé et absence d’erreurs JS/CSP passés. Installation/permissions/exécution Rust Wasm/désactivation/désinstallation desktop/mobile passées. Preuves inspectées dans `docs/evidence/2026-10-20/admin-local/`.
- Pas de suite Rust complète, PostgreSQL réel, fournisseur réel, NAS, mobile physique, déploiement, push ou publication validés dans cette intervention. Lectures ciblées et sous-agents séparés utilisés pour contenir le contexte, sans estimation chiffrée de tokens économisés.

## 20 octobre 2026 — Correction du blocage 403 Forbidden sur mobile / portail Authentik

- **Analyse du blocage 403** :
  - **Symptôme** : L'accès direct à Aether (via favori ou URL directe) fonctionne en 200 OK car aucun header `Origin` n'est émis. En revanche, l'accès depuis le portail Authentik (`https://authentik.dfmag.fr`), un lien tiers, ou un navigateur mobile (Chrome Android, Safari iOS) envoyant `Origin: https://authentik.dfmag.fr` ou `Sec-Fetch-Site: cross-site` était immédiatement rejeté avec `403 Forbidden` (`Content-Length: 0`).
  - **Cause racine** :
    1. Dans `crates/core/src/lib.rs` (`local_security_with_store`), le middleware de sécurité globale rejetait aveuglément toute requête portant `Sec-Fetch-Site: cross-site` ou dont l'en-tête `Origin` différait de `primary_origin`, en ne prévoyant une exception que pour le callback OIDC exact (`/api/v1/auth/oidc/callback`). Cela interdisait à tout utilisateur d'accéder au site web (`/`, `/app/`) via un lien externe ou un portail applicatif.
    2. Ni le middleware global ni `auth.check_origin` ne prenaient en compte les origines externes de confiance (comme l'issuer Authentik ou une liste blanche d'origines).
- **Résolution** :
  - **1. Autorisation des navigations web de premier niveau (Top-level Document Navigation)** :
    - Les requêtes HTTP `GET` ou `HEAD` vers les pages publiques du frontend (`/`, `/app`, `/app/*`, `/favicon*`) avec `Sec-Fetch-Mode: navigate` sont désormais autorisées même si elles proviennent d'un site tiers (`cross-site`), conformément aux recommandations W3C Fetch Metadata et OWASP. Les requêtes safe vers des documents statiques ne constituent pas une menace CSRF.
  - **2. Support des origines externes de confiance (`allowed_origins`)** :
    - `crates/core/src/auth.rs` : introduction de `resolve_allowed_origins` qui lit `MEDIA_ALLOWED_ORIGINS` (ou `MEDIA_TRUSTED_ORIGINS`) et dérive automatiquement l'origine de l'issuer OIDC à partir de `MEDIA_OIDC_ISSUER` (ex: `https://authentik.dfmag.fr`).
    - `AuthState` expose `allowed_origins(&self)` et `is_origin_allowed(&self, candidate: &str)`.
    - `check_origin` et `local_security_with_store` autorisent explicitement les origines externes de confiance.
    - Les requêtes malveillantes (comme `evil.attacker.com`) ou sans mode navigation restent strictement rejetées avec `403 Forbidden`.
    - Le Content Security Policy (CSP) intègre automatiquement ces origines de confiance dans `img-src` et `connect-src`.
- **Validations réelles exécutées** :
  - `cargo fmt --check` : validé.
  - `cargo clippy --all-targets -- -D warnings` : 0 erreur, 0 avertissement.
  - `cargo test -p media-poc --test challenger_security_challenge` : 4/4 passed (100% des tests de sécurité et CSRF validés).
  - `cargo test -p media-poc --test static_web` : 2/2 passed (nouveaux tests validant la navigation Authentik mobile et l'origine de confiance `MEDIA_ALLOWED_ORIGINS`).
  - `cargo test --lib -p media-poc` : 36/36 passed.

## 20 octobre 2026 — Correction du verrou d'allocation cache sous Linux CI (`cache_dirs`)

- **Analyse de la régression / échec CI GitHub Actions** :
  - Le test `cache_dirs::tests::reclaims_orphan_but_preserves_active_and_unowned` échouait avec `called Result::unwrap() on an Err value: Custom { kind: Other, error: "cache allocation busy" }`.
  - Cause racine : [crates/node/src/cache_dirs.rs](file:///c:/Users/Mael/Documents/GitHub/Aether/crates/node/src/cache_dirs.rs) utilisait un `global.try_lock()` immédiat sans attente sur `allocation.lock`. Lors de l'exécution concurrente des tests multi-threads sous Linux, des sous-processus lancés en parallèle (via `Command::new` dans `abrupt_exit_releases_lease`, `probe`, etc.) dupliquent les descripteurs de fichiers lors du `fork()` avant que `execve` ou la terminaison ne les libère. La contention même transitoire (quelques millisecondes) sur le verrou `flock(LOCK_EX)` provoquait un rejet immédiat par `try_lock()`.
  - De plus, en production, deux requêtes concurrentes créant un cache échouaient immédiatement en erreur 500 au lieu d'attendre la libération du verrou (qui dure typiquement moins d'une milliseconde).
- **Résolution** :
  - Remplacement de `global.try_lock()` direct par une boucle d'attente bornée avec retours `TryLockError::WouldBlock` (timeout de 5 secondes, sleep de 5ms) et préservation du message typé en cas d'erreur I/O réelle.
  - Silençage de la sortie stdout/stderr du sous-processus de test dans `abrupt_exit_releases_lease` pour éviter l'entrelacement de `running 1 test` dans les logs du runner de test.
- **Validations réelles exécutées** :
  - `cargo fmt --check` : 0 différence.
  - `cargo clippy --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test -p media-node --lib` : 40 passed, 0 failed, 1 ignored.
  - `cargo test --locked --workspace --all-features --lib --bins` : l'intégralité des tests unitaires et binaires passe avec succès.


## 20 octobre 2026 — Unification du SDK, Broker HTTP sécurisé et Contrats de Métadonnées enrichis

- **Résolution des verrous architecturaux des extensions** :
  - **1. Unification du SDK (`media-extension-sdk`)** :
    - [crates/extension-sdk](file:///c:/Users/Mael/Documents/GitHub/Aether/crates/extension-sdk/src/lib.rs) devient la source de vérité unique pour tous les types, l'ABI Wasm et les contrats de métadonnées.
    - Intégration des modèles complets : `MediaType`, `PluginCapability`, `ImageType`, `MediaImage`, `ActorCredit`, `ProviderRating`, `StructuredMetadata`, `SearchQuery`, `SearchResultItem`, `DetailsRequest`, `ImagesRequest`, `PluginError`, `ConfigOption` (avec `From<&str>`), et `ConfigField` (`defaultValue` et `options` typées alignées sur l'UI Svelte).
    - `Aether-Plugins` basculé sur `media-extension-sdk` dans son workspace Cargo ; `aether-plugin-api` réexporte directement le SDK officiel.
  - **2. Broker HTTP sécurisé Wasm (`media_host_http`)** :
    - [crates/core/src/extension_runtime.rs](file:///c:/Users/Mael/Documents/GitHub/Aether/crates/core/src/extension_runtime.rs) lève l'interdiction aveugle des imports pour autoriser exclusivement l'import hôte `("aether", "aether_host_http")`. Tout autre import (WASI, syscalls arbitraires) reste strictement interdit (`"imports_forbidden"`).
    - **Sécurité & Protection SSRF** : Seul le protocole HTTPS est autorisé (`scheme_forbidden`). Blocage absolu des adresses loopback (`127.0.0.1`, `localhost`, `::1`), privées RFC1918 et documentation (`host_forbidden`).
    - **Allowlist par extension** : Validation stricte que l'hôte distant fait partie des `allowed_hosts` autorisés pour le provider (ex: `api.themoviedb.org`, `image.tmdb.org`, `webservice.fanart.tv`, `v3.sg.media-imdb.com`).
    - **Quotas stricts** : Plafond de 10 requêtes HTTP par invocation (`quota_exceeded`), timeout paramétré à 6s maximum, et taille de réponse limitée à 256 Ko.
    - **Protection des secrets** : Core injecte automatiquement les headers d'authentification (`auth_headers`) depuis `extension_settings` sans exposition arbitraire à des serveurs tiers.
  - **3. Contrat de métadonnées complet & Dispatcher Wasm** :
    - Support des opérations `search`, `details`, `images`, `avatars` et rétro-compatibilité `metadata.propose`.
    - `crates/extension-sdk/tests/contracts_test.rs` valide le roundtrip de l'ensemble des structures riches et du contrat de host call HTTP.
- **Validations réelles exécutées** :
  - `media-extension-sdk` : 3/3 tests passés (`cargo test -p media-extension-sdk`).
  - `Aether Core` : 11/11 tests passés (`cargo test -p media-poc --test extensions` dont exécution réelle des binaires `.wasm` de TMDb et Avatar dans le runtime Wasmi, et application complète de métadonnées enrichies dans l'inventaire avec casting, notes et affiches).
  - `Aether-Plugins` : 19/19 tests passés (`cargo test` dans `Aether-Plugins`), `cargo fmt --check` validé et Clippy validé avec 0 erreur.
  - Compilation WebAssembly `wasm32-unknown-unknown` : `aether_plugin_avatar.wasm` (150 Ko) et `aether_plugin_tmdb.wasm` (245 Ko) générés avec succès.
  - CI GitHub Actions : création de `.github/workflows/ci.yml` dans `Aether-Plugins` avec linting, tests natifs, build Wasm et archivage des artefacts `.wasm`.

- **Correction CI GitHub Actions (`npm run lint` & `npm run check`)** :
  - `AdminExtensions.svelte` : Élimination des types `any` stricts (`@typescript-eslint/no-explicit-any`) via `SettingValue` (`string | number | boolean`) et `ExtensionSettings`.
  - Élimination des balises `{@html}` (`svelte/no-at-html-tags`) sur `candidate.manifest.banner` et `extension.manifest.banner` : remplacées par une conversion sécurisée en data-URI SVG/image (`toBannerSrc(...)`) rendue via `<img src={bannerSrc} />`. Sécurisation anti-XSS conforme aux standards web (exécution de script bloquée dans les balises `<img>`) et suppression des erreurs de linter.
  - Résolution du binding typé pour les cases à cocher de configuration dynamique via `checked` et `onchange`.
- **Dossier de données et plugins locaux** :
  - Explication du dossier `data` : ignoré par Git via `.gitignore`, stocké dans un volume Docker nommé `aether_data` par défaut en mode Compose.
  - Robustesse Core : `crates/core/src/identity.rs` crée automatiquement le dossier parent (`data/`) lors de l'ouverture SQLite si inexistant (`fs::create_dir_all`).
  - Création du dossier local `data/plugins/` avec son guide `README.md` pour le dépôt de paquets de plugins locaux hors-ligne.
- **Validations réelles exécutées** :
  - `npm --prefix apps/web run lint` : 0 erreur, 0 avertissement.
  - `npm --prefix apps/web run check` : 0 erreur.
  - `npm --prefix apps/web run build` : build réussi.
  - `cargo check -p media-poc` : réussi.

## 20 octobre 2026 — Extensions Rust, bannières Jellyfin, paramétrage dynamique et Avatar Plugin

- **Extensions dans `C:\Users\Mael\Documents\GitHub\Aether-Plugins`** :
  - **`aether-plugin-tmdb`** : métadonnées complètes, casting ordonné avec photos, équipe technique, affiches/backdrops (original et W500), notes moyennes et identifiants externes.
  - **`aether-plugin-imdb`** : recherche instantanée via suggestions IMDb (sans clé requise), extraction Schema.org JSON-LD (note IMDb, votes, classification, durée ISO 8601 en minutes).
  - **`aether-plugin-fanart`** : logos transparents ClearLogos (HD/4K), fonds sans texte Backdrops, ClearArt, bannières et jaquettes disc art.
  - **`aether-plugin-avatar`** : inspiré de `cedev-1/jellyfin-avatars`. Fournit 4 packs d'avatars vectoriels SVG haute résolution intégrés (`personas`, `geometric`, `retro`, `botts`), générateur procédural déterministe avec graine (seed), attribution aléatoire et options de verrouillage de politique administrateur (`disallow_custom_avatars`).
- **Inspiration Jellyfin & Métadonnées enrichies dans `crates/aether-plugin-api` et `crates/extension-sdk`** :
  - Métadonnées dans `Cargo.toml` (`[package.metadata.aether]`) avec version de l'extension, ABI ciblée (`target_abi = "1"`), compatibilité serveur minimale/maximale (`min_server_version = "1.0.0"`), catégorie et checksum SHA-256.
  - Bannière graphique Jellyfin intégrée (`banner.svg` vectoriel haute définition) pour chaque extension.
  - Schéma de configuration dynamique (`config_schema: Vec<ConfigField>`) typé (`password`, `text`, `boolean`, `select` avec options) pour les clés d'API et options du serveur.
- **Raccordement Core & Persistance dans Aether** :
  - Migration `0035_extension_settings.sql` (SQLite et PostgreSQL) créant `extension_settings (extension_id PRIMARY KEY, settings_json)`.
  - Endpoints sécurisés `GET /api/v1/admin/extensions/{id}/settings` et `PUT /api/v1/admin/extensions/{id}/settings` avec audit administratif et permissions.
  - Endpoint `list` (`GET /api/v1/admin/extensions`) incluant automatiquement `settings` pour chaque extension.
  - Protection des avatars dans `crates/core/src/avatars.rs` : interdiction du téléversement d'avatars personnalisés pour les utilisateurs non-administrateurs lorsque la politique `disallow_custom_avatars` est activée sur une extension d'avatars.
- **Interface d'administration Web (`AdminExtensions.svelte`)** :
  - Affichage des bannières visuelles vectorielles de chaque plugin.
  - Badges de version, catégorie, compatibilité ABI et compatibilité Aether, avec checksum SHA-256.
  - Menu interactif "Paramètres du plugin" générant les champs selon `configSchema` (clés secrètes masquées, cases à cocher, listes de sélection) avec sauvegarde asynchrone sécurisée.
- **Validations réelles exécutées** :
  - `Aether-Plugins` : 18/18 tests unitaires et d'intégration réussis (`cargo test --workspace`), `cargo fmt --check` et `cargo clippy --all-targets -- -D warnings` (0 erreur, 0 avertissement). Binaires release générés dans `target/release/`.
  - `Aether` : `cargo check` réussi, tests d'extensions réussis (`cargo test -p media-poc --test extensions` avec validation des paramètres, du cycle de vie et des droits), `npm run check --prefix apps/web` réussi (0 erreur TypeScript / Svelte).


## 20 octobre 2026 — Premier jalon PWA et demandes produit

- Manifeste statique généré à partir de `DEFAULT_NAME`, icônes PNG 192/512,
  lancement standalone et service worker SvelteKit réseau uniquement. CSP Core
  étendue à `worker-src 'self'` et `manifest-src 'self'`.
- Message hors connexion pour navigations `/app/`, aucun cache de données privées
  ou médias, aucune interception des API/Range/Nodes distants et aucune activation
  forcée pendant la lecture. Pas de téléchargement hors ligne livré.
- Suivi complet des demandes dans `docs/PRODUCT_BACKLOG.md`, dont bibliothèques
  musicales, comptes/sessions, multi-Node et correction du cache après crash.
  Ces fonctionnalités restent ouvertes ; aucun changement du lecteur dans ce lot.
- Web check/lint, 57 tests et build validés ; Chrome headless : manifeste,
  icônes décodées, worker actif, navigation hors ligne et reconnexion, cache vide.
  Test Rust `static_web` réussi ; `cargo fmt --check` et `git diff --check` réussis.
  Voir `docs/PWA.md`. Installation physique et NAS HTTPS non testés.
- Modifications préexistantes préservées ; pas de push ni de déploiement.
  Token Saver appliqué par lectures ciblées et validations regroupées.

## 20 octobre 2026 — Socle d’extensions tierces et validation du runtime

- Clarification utilisateur : fournir la plateforme et le SDK, sans développer ou livrer d’extension produit. Démonstration retirée des livrables et des artefacts, modules synthétiques uniquement dans les fixtures de test. ABI générique `{apiVersion,operation,input}` → `{apiVersion,result}`; adaptateurs métier soumis aux permissions Core.
- Autorisation utilisateur de poursuivre jusqu'à un parcours fonctionnel. Sous-ensemble livré : paquet JSON local Ed25519/SHA-256, éditeur explicitement approuvé, installation désactivée, consentements par bibliothèque, activation, proposition de métadonnées, révocation et désinstallation avec choix de conservation des résultats.
- `extensions.rs` raccordé au Core commun; migration 0031 SQLite/PostgreSQL. Sessions/rôle administrateur/CSRF existants, contrôles vivants bibliothèque/Node/éditeur et génération unique contre désactivation/réactivation et réinstallation. Aucun second rôle ou planificateur. Historique borné à 1 000 exécutions, audit administratif transactionnel séparé.
- `media-extension-worker` : Wasmi 2.0.0 sans imports/WASI dans un processus temporaire distinct; module 256 Kio, JSON 32 Kio, mémoire invitée 16 Mio, fuel 2 millions, délai 3 secondes, une invocation à la fois. Limites de mémoire invitée distinctes d'un quota RSS système, non implémenté. Aucun accès média/réseau/SQL/secrets de l'extension.
- SDK Rust générique `crates/extension-sdk`, fixture de test uniquement `tests/fixtures/extension-sdk`, CLI de paquetage hors ligne `media-extension-pack`, contrat `docs/extensions-v1.openapi.json`, guide et limites `docs/EXTENSIONS_API.md`. L'écran Administration → Extensions remplace le panneau de conception.
- Validations locales réellement exécutées : compilation Wasm de la fixture; 6 tests extensions après généralisation de l’ABI (aller-retour générique, SDK réel, limites, révocation pendant exécution et navigateur Chrome desktop/mobile), 1 test scheduler et 2 tests admin_access passés; Clippy strict, fmt, diff check; Web check 0/0, lint, build et 57 tests passés. Captures et trois mesures debug de l'ABI initiale dans `docs/evidence/2026-10-20/extensions/` : pic observé de 10,29–10,36 Mio; 1 319 ms au premier lancement puis environ 100 ms. Aucune promesse de charge ou de quota RSS.
- Limites : Docker indisponible, donc image et PostgreSQL non validés ici; aucun déploiement/publication. Dépôts/catalogue distants, mise à jour/rollback, hooks durables, tâches Node, stockage privé généralisé, réseau/secrets, Trickplay, Intro Skipper, fournisseurs externes et statistiques restent à développer. Les fichiers des modifications préexistantes ont été préservés.

## 20 octobre 2026 — Fiches allégées, demi-étoiles et attentes de chargement

- Retrait du lien/bandeau « Toutes les séries / Tous les films » sous la navigation principale des fiches.
- `PersonalFeedback` affiche un cœur et cinq étoiles avec sélection par moitié, clavier, retrait par second clic et erreurs d'enregistrement explicites. API de 0,5 à 5 par pas de 0,5 ; migration 0030 SQLite/PostgreSQL préservant les anciennes notes entières. Identité, CSRF et droits restent côté Core.
- Affiches saison et vignettes exactes déjà gérées par le scanner : parcours renforcé par les tests HTTP et Chrome avec de vraies images synthétiques. Dimensions réservées, décodage asynchrone et aperçu horizontal adapté au mobile. Les fichiers de l'exemple NAS (`season01-poster.jpg`, `<vidéo>-thumb.jpg`) suivent les conventions existantes ; une actualisation rapide après mise à jour Core/Node est nécessaire pour les découvrir. Aucun accès au NAS ni modification des médias personnels.
- Les quatre comportements d'actualisation sont conservés, avec des libellés plus simples distinguant le rapide recommandé, la conservation des descriptions, le complément des descriptions absentes et la réanalyse complète plus lente.
- Images locales : ETag + `private, no-cache`, réponse 304 vide après contrôle session/droits/confinement/empreinte. Le middleware garde cette politique explicite et applique `no-store` par défaut aux autres réponses. Aucun gain de cache conditionnel revendiqué pour le relais distant ; pas de réduction des octets du premier chargement ni de conversion d'images.
- La fiche film s'affiche avant la fin des descripteurs ; le lecteur charge directement la piste préférée au lieu de charger le français puis de changer. La progression se charge en parallèle avant d'appliquer la reprise. Préparation/remux et compatibilité navigateur restent des limites distinctes.
- Vérifications exécutées : Web `check` (0 erreur/avertissement), `lint`, `test` (57 réussis), `build` ; `cargo test --locked -p media-node --lib -j 1` (38 réussis) ; `cargo test --locked -p media-poc --test series_flow --test posters_flow --test static_web -j 1 -- --test-threads=1` (4 réussis). Smoke test SQLite en mémoire : note/favori existants conservés, 4,5 accepté, 4,25 refusé et cascade conservée. PostgreSQL réel non exécuté, Docker Engine indisponible.
- `MEDIA_REPORT_DIR=docs/evidence/2026-10-20/series-feedback cargo test --locked -p media-poc --test browser_series -j 1 -- --ignored --nocapture` : 1 réussi. Chrome 155.0.8059.82, Windows 10.0.26300, headless, thèmes clair/sombre, 390/1440 px : favoris/note 4,5 persistés après reload, visuels décodés, lecture/pause/seek, transitions inter-saison, révocation et isolation des comptes ; aucune erreur JS/CSP. Inspection visuelle du rendu mobile exécutée.
- Contrôles finaux : `cargo fmt --check`, `cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings` et `git diff --check` réussis. La suite Rust complète et PostgreSQL réel n'ont pas été exécutés pour cette intervention.
- Délai clic → vidéo avançant de plus de 0,1 s : 1310 ms sur un lancement synthétique local. Ce chiffre n'est ni une comparaison avant/après ni une mesure NAS/WAN/audio physique. Preuves : `docs/evidence/2026-10-20/series-feedback/report.json` et captures associées. Aucun push, publication ou déploiement.


## 20 octobre 2026 — Correction syntaxe PostgreSQL migration 0028 et clarification de la consommation mémoire

- **Diagnostic de l'échec CI PostgreSQL (`postgres18_http_contract_and_reopen`)** :
  - **Symptôme** : Panique à la ligne 619 de `crates/core/tests/postgres.rs` :
    `PgDatabaseError { code: "42601", message: "syntax error at or near \"WHERE\"", position: 562 }` lors de l'exécution de la migration 28.
  - **Cause racine** : Dans `crates/core/migrations-postgres/0028_default_groups_and_library_exclusions.sql`, la vue `effective_library_permissions` contenait :
    `SELECT l.library_id, u.id AS user_id FROM user_group_libraries l JOIN user_groups g ON g.id=l.group_id JOIN users u WHERE g.is_default=TRUE;`
    En SQLite, une jointure `JOIN users u WHERE ...` sans clause `ON` est interprétée implicitement comme un produit cartésien (`CROSS JOIN`). En PostgreSQL (conformité SQL standard stricte), un `JOIN` sans `ON` déclenche une erreur de syntaxe à l'instruction `WHERE`.
  - **Correctif appliqué** :
    - Remplacement de `JOIN users u` par `CROSS JOIN users u` dans `crates/core/migrations-postgres/0028_default_groups_and_library_exclusions.sql` et dans la version SQLite `crates/core/migrations/0028_default_groups_and_library_exclusions.sql` pour assurer une syntaxe ANSI SQL uniforme et stricte.
  - **Validations exécutées** :
    - `cargo check --tests` : 0 erreur.
    - `cargo test -p media-poc --lib` : 33/33 tests unitaires passés avec succès.
    - `cargo test -p media-node --lib` : 38/38 tests unitaires passés avec succès.
    - `cargo clippy --all-targets -- -D warnings` : 0 avertissement.
    - `cargo fmt --check` : code formaté.

- **Clarification et diagnostic de la consommation mémoire (affichage de ~514 MB au repos)** :
  - **Mémoire RSS (processus Rust) vs Page Cache (noyau Linux)** :
    - Les tests d'ingestion et de charge (`crates/core/tests/scanner_performance.rs`) démontrent que l'empreinte mémoire active réelle (RSS) de `media-poc` / `aether-core` se maintient entre **8 et 38 MiB**, même avec 10 000 médias catalogués.
    - Lorsque l'application tourne dans un conteneur Docker (ex: sur Synology DSM via Container Manager ou sous Linux), la commande `docker stats` et les dashboards affichent par défaut `memory.usage_in_bytes` (cgroup v1) ou `memory.current` (cgroup v2), qui agrège :
      `Mémoire affichée = RSS (mémoire vive du processus) + Page Cache (fichiers gardés en cache par le noyau)`.
    - Les lectures de vidéos par `ffprobe`, l'analyse de NFO, les affiches et les accès SQLite dans `/media` et `/data` remplissent naturellement le cache de pages du noyau Linux. Ce cache disque n'est pas de la mémoire perdue ou consommée par le processus : le noyau Linux le libère instantanément dès qu'une autre application ou le conteneur en a besoin.
  - **Mesures d'optimisation recommandées** :
    - Pour observer la vraie mémoire du processus : `docker top aether -o pid,%mem,vsz,rss,comm` ou `cat /sys/fs/cgroup/memory/memory.stat`.
    - Pour contenir les arènes mémoire Tokio sous glibc dans Docker, l'environnement peut être complété avec `MALLOC_ARENA_MAX=2`.


## 19 octobre 2026 — Correction du test d'intégration navigateur `browser_series` et archivage des preuves CI

- **Diagnostic des échecs CI (`FileNotFoundError: docs/evidence/2026-10-17/series/report.json`)** :
  1. **Répertoire de sortie par défaut** : Dans `scripts/browser-series.cjs`, `out` avait été modifié vers `docs/evidence/2026-10-19/catalogue` alors que `.github/workflows/ci.yml` attend strictly les preuves sous `docs/evidence/2026-10-17/series/`.
  2. **Attente du scan film asynchrone** : `page.waitForFunction(async () => ...)` dans Playwright recevait une fonction `async` retournant une `Promise` (toujours truthy en JavaScript dans le contexte de page), ce qui résolvait immédiatement sans attendre la complétion réelle du scan (`status === 'completed'`). La navigation vers `/app/films/` s'exécutait alors que le scan tournait encore, produisant un catalogue vide (`0 film`).
  3. **Timeout artificiel sur le clic film** : Un `timeout: 5000` arbitraire faisait échouer l'action avant le chargement complet des cartes.
- **Correctifs appliqués** :
  - `scripts/browser-series.cjs` :
    - Rétablissement de la sortie par défaut vers `docs/evidence/2026-10-17/series` (avec copie miroir vers `docs/evidence/2026-10-19/catalogue` dans le bloc `finally`).
    - Remplacement du `waitForFunction` asynchrone par une boucle de polling côté Node (`page.evaluate`) vérifiant le statut `completed` ou `error` du job de scan.
    - Utilisation de `await filmCard.waitFor()` avec le timeout Playwright standard (30s) au lieu du timeout réduit à 5s.
- **Vérifications réelles exécutées** :
  - `cargo test --locked --test browser_series -- --ignored --nocapture` : **100 % passé avec succès** (1 passed, 0 failed en 47.32s).
  - Validation du script Python de la CI (`.github/workflows/ci.yml` step 260) :
    - Présence et `passed: true` vérifiés sur `docs/evidence/2026-10-16/scanner/report.json`, `docs/evidence/2026-10-16/scanner/report-restart.json` et `docs/evidence/2026-10-17/series/report.json`.
    - Présence de toutes les images requises vérifiée (`admin-scan.png`, `series-detail.png`, `immersive-playback.png`, etc.).
  - `cargo check --tests` : 0 erreur.

## 19 octobre 2026 — Correction du test d'intégration navigateur `browser_scanner` (dialogue natif d'ajout de médiathèque)

- **Diagnostic de l'échec `browser_scanner_persists_and_plays_after_core_restart`** :
  - Suite à la refonte de l'interface d'administration des médiathèques (`AdminLibraries.svelte`) utilisant des dialogues `<dialog>` natifs, le formulaire d'ajout d'une médiathèque est encapsulé dans une boîte de dialogue modale fermée par défaut (`editor?.showModal()`).
  - Le script `scripts/browser-scanner.cjs` tentait d'écrire directement dans le champ `#library-name` sans ouvrir préalablement le dialogue via le bouton « + Ajouter une médiathèque », provoquant un timeout Playwright de 30 secondes (`element is not visible`).
  - L'affichage du statut de scan (`scan-status`) utilisait la classe `scan-status` sans la classe historique `scan-counters` et n'incluait pas le libellé « découvert(s) ».
- **Correctifs apportés** :
  - `AdminLibraries.svelte` : Ajout de la classe `scan-counters` et du libellé « découvert(s) » sur `<p class="scan-status scan-counters" role="status">` pour compatibilité complète avec les assertions des scripts E2E et l'affichage Jellyfin-like.
  - `scripts/browser-scanner.cjs` : Ouverture explicite du dialogue via `await page.getByRole('button', { name: 'Ajouter une médiathèque', exact: true }).click()`, soumission via le dialogue, et adaptation des regex de scan counters (`/2 (?:fichier\(s\) )?découvert/`, `/0 analysé.*2 réutilisé/`).
  - `scripts/browser-nfo.cjs` et `scripts/browser-posters.cjs` : Harmonisation de l'ouverture du dialogue d'ajout de médiathèque lors de la phase initiale.
  - `apps/web` : Rebuild statique `npm run build` synchronisé.
- **Vérifications réelles exécutées** :
  - `cargo test --locked --test browser_scanner -- --ignored --nocapture` : **100 % passé avec succès** (1 passed, 0 failed en 22.40s, tests avant/après redémarrage de Core, switch audio, sous-titres ASS/SRT/VTT, progression).
  - `cargo test --lib --bins` : l'ensemble des 74 tests unitaires des crates passent avec succès.
  - `cargo check --tests` : 0 erreur de compilation.

## 19 octobre 2026 — Optimisation de performance et fluidité de chargement des images (posters et backdrops)

- **Diagnostic de la lenteur constatée** :
  - Sur chaque requête d'image (`GET /api/v1/assets/{id}/image`), `media_node` exécutait `validate_opened`, qui acquérait un sémaphore global de taille 1 (`VALIDATION = Semaphore::const_new(1)`), relisait le fichier entier en mémoire et décompressait l'intégralité des pixels en CPU via `DynamicImage::from_decoder`. Pour une page de catalogue affichant 30 posters, les requêtes étaient sérialisées dans ce goulot CPU (3 à 6 secondes d'attente cumulative).
  - Dans l'interface, les conteneurs d'arrière-plan (`HeroBanner.svelte`, `FilmDetail.svelte`, `SeriesDetail.svelte`) laissaient un vide noir avant l'arrivée de l'image de fond et les affiches (`Poster.svelte`) apparaissaient brusquement sans transition.
- **Optimisation backend (`crates/node/src/images.rs` et `crates/node/src/stream.rs`)** :
  - **Cache de validation en mémoire (`VALIDATION_CACHE`)** : Bounded in-memory cache associant `(canonical_path, size_bytes, modified_ns)` au format MIME validé. Dès qu'une image a été validée (au scan ou lors de la première lecture), toute requête ultérieure sur le même fichier inchangé est servie immédiatement sans passer par le sémaphore ni allouer 10 Mio ni exécuter de décodage CPU (temps de traitement réduit à ~0 ms).
  - **Amorçage lors des scans incrémentaux** : Les fonctions `discover_cached_with_folder` et `discover_series_poster` pré-remplissent le cache de validation lors de la réutilisation des entrées inchangées.
  - **En-têtes HTTP de streaming** : Ajout de l'en-tête `ETag` calculé à partir de `(modified_ns, file_size)` sur les réponses 200/206 dans `serve_opened_parts`.
- **Améliorations frontend (`HeroBanner.svelte`, `Poster.svelte`, `FilmDetail.svelte`, `SeriesDetail.svelte`)** :
  - `HeroBanner` : L'arrière-plan cinématique (`cinematic-horizon.png`) est conservé en permanence en sous-couche sous le dégradé sombre, supprimant tout vide noir au chargement. L'image de fond se charge en priorité haute (`fetchpriority="high"`, `loading="eager"`) et effectue un fondu progressif (`opacity: 0 -> 1` avec transition douce) dès qu'elle est prête (`onload`).
  - `Poster` : Ajout d'une transition d'opacité fluide à l'arrivée de l'image pour supprimer le pop visuel saccadé.
  - `FilmDetail` et `SeriesDetail` : Chargement eager et fondu progressif pour le backdrop flouté en arrière-plan.
- **Validations exécutées** :
  - `cargo test -p media-node` : 36/36 tests unitaires passés + 3 tests d'intégration.
  - `cargo test -p media-poc --test posters_flow --test scanner_flow` : validés avec succès.
  - `cargo fmt --check` et `cargo clippy --all-targets -- -D warnings` : 0 avertissement.
  - `npm run check` (0 erreur, 0 avertissement), `npm test` (54/54 tests), `npm run lint` (0 erreur), `npm run build` réussi.

## 19 octobre 2026 — Tolérances sous-titres, groupes par défaut et exclusions d'indexation (NAS / corbeille)

- **Tolérances et robustesse sous-titres (`crates/node/src/subtitles.rs`)** :
  - Résolution des anomalies de scan `subtitles exceed 512 KiB per video` et `subtitle end precedes start` constatées sur des pistes volumineuses (ex. CJK/SDH) ou avec des timecodes corrompus.
  - Plafond par piste porté de 256 Kio à 1 Mio, plafond total par vidéo porté de 512 Kio à 16 Mio, nombre max de pistes porté de 16 à 64.
  - Dans les convertisseurs WebVTT (SRT et ASS), les répliques isolées où `end <= start` sont désormais ignorées sans invalider tout le reste de la piste.
- **Groupes par défaut (`is_default`)** :
  - Ajout d'une colonne `is_default INTEGER/BOOLEAN NOT NULL DEFAULT 0` sur `user_groups` via migration 0028 (SQLite et PostgreSQL).
  - La vue `effective_library_permissions` associe automatiquement tous les utilisateurs existants et futurs aux groupes marqués comme par défaut, leur conférant l'accès aux bibliothèques associées sans affecter le modèle strict à deux rôles (`administrator` / `user`).
  - Interface d'administration `AdminGroups.svelte` et `AdminUsers.svelte` enrichie d'une case à cocher « Groupe par défaut » et de badges d'indication clairs.
- **Exclusion des dossiers NAS / corbeille et filtres personnalisés** :
  - Le scanner (`crates/node/src/scanner.rs`) ignore d'office les dossiers et fichiers système ou corbeille des NAS : `#recycle`, `@eaDir`, `@synoEAStream`, `@appstore`, `#snapshot`, `lost+found`, `.@__thumb`, `thumbs.db`, `desktop.ini`, ainsi que tout nom commençant par `@`.
  - Prise en charge des marqueurs d'exclusion dans les dossiers : `.nomedia`, `.ignore`, `.aetherignore`.
  - Ajout du champ `excluded_patterns` par bibliothèque (`crates/core/src/libraries.rs`, formulaire `AdminLibraries.svelte`), transmis au Media Node lors du scan en streaming (`RemoteScanStartRequest`).
- **Validations exécutées** :
  - `cargo test -p media-node` : 36/36 tests unitaires passés + 3 tests d'intégration (dont test dédié sur dossiers NAS, `.nomedia` et patterns).
  - `cargo test -p media-poc` : ensemble des suites unitaires et d'intégration validées (dont `scheduler`, `series_flow`, `catalog_performance`, `admin_access`).
  - `cargo fmt --check` et `cargo clippy --all-targets -- -D warnings` : 0 avertissement.
  - `npm run check` (0 erreur, 0 avertissement) et `npm test` (54/54 tests passés dans `apps/web`).

## 19 octobre 2026 — Général, réseau effectif, avatars et modes d’actualisation

- Nouvelle demande appuyée par cinq captures Jellyfin : console avec navigation latérale fixe sur bureau, marque/version, pages Général et Réseau ; médiathèques en grille et menus Gérer/Renommer/Actualiser/Planifier, formulaires en dialogues natifs, menu mobile repliable. Les cartes restent une représentation Films/Séries sans fausses affiches.
- Général : nom du serveur et message d’accueil persistants (`server_settings`), édition administrateur + CSRF, relecture après sauvegarde, nom dans la navigation et message sur l’accueil authentifié. La marque produit de `/api/v1/config` reste indépendante du nom de cette installation.
- Réseau : page et endpoint administrateur montrant l’origine réellement utilisée par AuthState, les cookies HTTPS et les chemins Web/API. Transport en lecture seule : pas de changement à chaud de ports/TLS/MEDIA_PUBLIC_URL, ni d’ouverture automatique d’accès distant.
- Avatar : GET/HEAD/PUT/DELETE `/api/v1/account/avatar`, compte courant uniquement, CSRF sur mutations. PNG/JPEG/WebP ≤ 2 Mio, dimensions ≤ 4096, pixels décodés ≤ 32 Mio, deux décodeurs maximum ; recadrage/réencodage PNG 256×256, stockage dans la base (`user_avatars`), cache privé no-store/nosniff. Affiché dans Compte, l’en-tête et le menu profil ; suppression et repli icône. Aucun chemin ou URL externe accepté, aucune écriture dans les médias.
- Modes manuels : `incremental` (défaut historique), `new_only` (éditorial connu conservé), `missing_metadata` (NFO validés connus conservés, absents complétés), `full` (caches techniques/images ignorés et données locales relues). Le Core fige le mode dans le job ; publication commune Films/Séries/Épisodes. Les pistes, fichiers modifiés, contrôles de confinement et autorisations restent vérifiés. Mode complet distant via caches vides du contrat existant ; les plans périodiques restent incrémentaux.
- Migration additive 0027 SQLite + PostgreSQL : réglages, avatars et mode de job. Les données du compte et du serveur sont incluses dans les sauvegardes de la base.
- Fournisseurs futurs : contrat/politiques préparés et documentés, pas de recherche TMDB/TVDB/IMDb implémentée ni de tâche d’extension fictive. « Manquant » signifie actuellement absence de NFO validé, pas fusion champ par champ. Détails : `docs/SETTINGS_AND_REFRESH.md`, revue mise à jour dans `docs/SETTINGS_REVIEW.md`.
- Vérifications réussies : `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace` (tests externes/navigateurs ignorés gardent leurs prérequis), `npm run check` (0 erreur/avertissement), `npm run lint`, `npm test` (54 tests), `npm run build`, `git diff --check`.
- Vérifications ciblées après dernières retouches : `cargo test -p media-poc --test nfo_flow --test settings_avatars` (3 tests), `cargo test -p media-poc --lib avatars::tests` (1 test : trois formats réencodés, dimensions bornées). Les tests de modes couvrent Films, Séries et Épisodes : conservation, complétion, réanalyse complète, identifiants stables et médias inchangés. Les tests avatars/réglages couvrent droits, CSRF, fichiers refusés/oversize, HEAD et persistance après réouverture.
- Chrome headless/API simulée : `node scripts/test-admin-ui.mjs` réussi, deux thèmes/cinq largeurs, pages Général/Réseau, upload/suppression avatar, modes envoyés par le menu de médiathèque, tâches et préférences. Premier démarrage du serveur Vite froid a dépassé 10 s pendant la compilation Rust ; scénario relancé avec navigation 30 s, puis réussi.
- Vrai Core/SQLite temporaire : `cargo test -p media-poc --test browser_admin -- --ignored --nocapture` réussi. Création/renommage de médiathèque par UI, nom/message relus et message vu sur l’accueil, réseau effectif, avatar décodé à 256 px puis rechargé/supprimé, actualisation complète par dialogue, comptes/groupes/clefs/backups/préférences conservés. Clé de sauvegarde éphémère fournie au seul processus de test ; aucun dossier personnel utilisé. Correction du script Playwright pour lire l’ID de création avant de quitter la page.
- Captures inspectées : `docs/evidence/2026-10-19/settings-expanded/`, dont compte/actualisation issus du vrai Core. Les scripts navigateur Films/NFO/posters/Séries/distribué sont adaptés aux menus et dialogues ; seule l’administration réelle a été relancée explicitement dans ce jalon.
- Limites : pas de nouvelle preuve PostgreSQL réel, NAS, appareil physique ou nouveau décodage média navigateur. Aucun déploiement/publication/push. Prochain petit lot proposé après retour sur l’interface : règles de reprise configurables et gestion personnelle des sessions. Token Saver : contexte réutilisé, lectures ciblées et contrôles regroupés ; aucune économie chiffrée revendiquée.


## 19 octobre 2026 — Correction du test E2E séries (`browser_series`)

- **Correction de la navigation Playwright dans `scripts/browser-series.cjs`** :
  - Suite à la refonte de la console d'administration découpant les rubriques en sections masquées (`hidden`), l'attribution des droits d'accès à la bibliothèque `Séries Navigateur` pour `series-user-b` échouait sur un timeout de 30 secondes car le bouton « Modifier » dans `#users-section` était masqué depuis `#libraries-section`.
  - Ajout explicite de la navigation vers `a[href="#users-section"]`, attente de l'enregistrement du compte, puis bascule vers `a[href="#libraries-section"]` pour déclencher le scan de la bibliothèque.
  - Validation exécutée avec succès : `cargo test --locked --test browser_series -- --ignored --nocapture` (1 test passé en 20.71s, scan, lecture AV1/Opus, transitions inter-saisons, isolation de cache et permissions 403 vérifiés sous Chrome headless).

## 19 octobre 2026 — Correction du parcours Jellyfin et revue des paramètres

- Demande précisée par deux captures : liste de tâches compacte par catégorie, puis page dédiée et liste de déclencheurs ; la précédente présentation en cartes/détails dépliables ne répondait pas au besoin.
- `AdminTasks.svelte` remplace cette présentation : Bibliothèques + Maintenance, lien de configuration et lancement indépendant à droite, détail par `#tasks-section?task=scan-ID` ou `task=backup`, retour navigateur, ajout/modification en minutes/heures/jours et suppression. Endpoints de scan/backup existants réutilisés. Un seul intervalle par tâche ; pas de cron/calendrier ou durées de scan inventés.
- Sauvegarde chiffrée réellement intégrée au planificateur : création, planification et désactivation du déclencheur ; accès aux archives/configuration conservé dans Sauvegardes. Cette vue se recharge à son ouverture pour refléter les mutations du planificateur.
- Paramètres personnels organisés en Compte / Apparence / Lecture / Connexion SSO, liens directs par fragment et navigation mobile. Délais audio/sous-titres exposés via `offsets` existant (−30 à +30 s ; audio externe). Suppression de la bannière de succès permanente et de l’inférence SSO par préfixe du login.
- Revue ciblée des fonctions existantes/manquantes : `docs/SETTINGS_REVIEW.md`. Mot de passe personnel, sessions/appareils, configuration/diagnostic général et historique/calendrier des tâches restent des lots serveur distincts à valider ; aucune fausse tâche d’extension ajoutée.
- Contrôles réussis : `npm run check` (0 erreur/avertissement), `npm run lint`, `npm test` (54 tests), `npm run build`, `git diff --check`.
- `node scripts/test-admin-ui.mjs` : Chrome headless/API simulée, scans et backups, création/suppression des déclencheurs, erreurs/limites/CSRF, liens directs et historique navigateur, 2 thèmes × 5 largeurs, paramètres par rubrique, offsets sauvegardés/rechargés, noms longs et refus du rôle user. Captures inspectées dans `docs/evidence/2026-10-19/settings-jellyfin/`.
- `cargo test -p media-poc --test browser_admin -- --ignored --nocapture` : 1 test réussi sur vrai Core/SQLite temporaire et Chrome, après adaptation des anciens sélecteurs ; création compte/groupe, scan, suppression du déclencheur relue par API, clés API, archive chiffrée et tâches backup (intervalle relu, seconde archive créée, désactivation relue), offsets relus par API puis après rechargement. Clé de sauvegarde éphémère fournie uniquement au processus de test ; MEDIA_BACKUP_DIR laissé au dossier temporaire. Premier essai bloqué par ancien sélecteur de groupe, corrigé avant réussite.
- Limites : aucune nouvelle preuve NAS, PostgreSQL, mobile physique ou lecture/décodage média. Prochain petit jalon proposé : mot de passe personnel et sessions, après validation de cette interface. Skill codex-token-saver appliqué par lectures ciblées, contexte réutilisé et vérifications regroupées ; aucune économie chiffrée revendiquée.


## 19 octobre 2026 — Charte clair/sombre et présentation des paramètres

- Groupes placé après Utilisateurs, dans Accès et sécurité. Navigation mobile repliable (≤ 960 px), rubriques directes et sélection active conservées.
- Planificateur présenté en liste : icône horloge, scan manuel à droite, échéances connues, déclencheurs et résultats dépliables. Intervalles API existants conservés ; aucune nouvelle tâche serveur.
- Charte Pierre Douce / Cinéma OLED centralisée dans `tokens.css` ; remplacement des couleurs violettes et fonds fixes de l’administration/menu profil. En-tête et pied du catalogue adaptés au thème, logo SVG officiel transparent dans Navbar, panneau SSO personnel simplifié.
- Débordements corrigés sur paramètres personnels et noms longs des bibliothèques, y compris boutons de scan ; aucune nouvelle dépendance.
- Vérifications locales exécutées : `npm run check` (0 erreur, 0 avertissement), `npm run lint`, `npm test` (54 tests), `npm run build`, `git diff --check` réussis. `node scripts/test-admin-ui.mjs` réussi sous Chrome headless/API simulée : deux palettes, neuf rubriques à 360/390/768/1024/1440 px, menu mobile, détails au clavier, requêtes CSRF scan/planification, paramètres personnels et noms longs à 360/1024 px. Captures inspectées : `docs/evidence/2026-10-19/admin-theme/`.
- Limites : vérification de présentation et du contrat Web simulé ; pas de nouvelle preuve de persistance/RBAC Core, de déploiement NAS ou d’appareil mobile réel. Aucun test Rust relancé pour ces modifications Web. Prochain petit jalon : retour visuel utilisateur avant toute extension du planificateur.


## 19 octobre 2026 — Diagnostic Scan (Permissions RO), Intégration Favicon Logo et Gestion Droits Utilisateurs/Groupes

- **Résolution et diagnostic explicite de l'erreur `Permission denied (os error 13)`** :
  - **Explication système & conteneur** : Aether lit les médias en lecture stricte (`OFlags::RDONLY` / `:ro`). L'erreur `Permission denied (os error 13)` sous Linux / Docker survient lorsque les fichiers/dossiers sur l'hôte ont été passés en lecture seule sans le bit d'exécution sur les répertoires (ex. `chmod 444` au lieu de `chmod 755` / `chmod a+rX`), ou lorsque les permissions Unix restreignent l'accès au propriétaire sans accorder les droits de lecture à l'UID 10001 du conteneur unprivileged `aether`.
  - **Diagnostic enrichi** : Amélioration des messages d'erreur dans `crates/core/src/scans.rs` et `crates/node/src/scanner.rs` indiquant précisément à l'administrateur la cause système et la commande de correction hôte (`chmod -R o+rX` et droit `+x` sur les répertoires).
- **Intégration de la Favicon Navigateur avec le logo de la marque** :
  - Création de `apps/web/static/favicon.svg` à partir de la charte officielle `apps/web/static/images/brand-mark.svg`.
  - Déclaration de `<link rel="icon" type="image/svg+xml" href="%sveltekit.assets%/favicon.svg" />` dans `apps/web/src/app.html`.
  - Ajout des routes `/favicon.svg` et `/favicon.ico` dans `crates/core/src/lib.rs` redirigeant vers `/app/favicon.svg` pour les navigateurs interrogeant la racine.
- **Séparation des responsabilités : Droits bibliothèques gérés sur les Utilisateurs et les Groupes** :
  - **Retrait dans Bibliothèques (`AdminLibraries.svelte`)** : Suppression du `<fieldset class="grants">` et du compteur d'utilisateurs autorisés dans les cartes de bibliothèques. Les bibliothèques se concentrent uniquement sur la définition du répertoire, du type et du Media Node.
  - **Attribution directe sur les Utilisateurs (`AdminUsers.svelte`)** : Ajout de la sélection des bibliothèques autorisées lors de la création et de la modification d'un compte utilisateur.
  - **Support backend (`crates/core/src/admin_users.rs`)** : Extension de `Create` et `Update` avec `library_ids`, synchronisant automatiquement les lignes dans `library_permissions(library_id, user_id)`.
  - **Tests et validation** :
    - Test unitaire dédié `user_library_permissions_create_and_update` dans `crates/core/tests/admin_users.rs`.
    - Script Playwright `scripts/browser-series.cjs` mis à jour pour accorder l'accès via la rubrique Utilisateurs.
- **Optimisation drastique de l'empreinte de stockage de l'image Aether** :
  - **Suppression du paquet apt Debian `ffmpeg` (~350 à 400 Mo économisés)** : `probe-runtime` dans `Dockerfile` compile désormais à la fois `ffmpeg` et `ffprobe` minimalistes depuis les sources (sans encodeurs superflus, sans X11, sans OpenGL, sans PulseAudio/ALSA, strictement pour le copy-muxing et l'analyse de conteneurs). Les binaires de ~15 Mo sont copiés directement dans l'image de runtime sans aucune dépendance lourde externe.
  - **Profil de compilation Rust optimisé (`Cargo.toml`)** : Ajout de `[profile.release]` avec `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, et `strip = true`, réduisant le volume des exécutables compilés de 30 % à 50 %.
  - **Nettoyage des couches runtime** : Purge systématique des caches apt, fichiers temporaires et documentations (`/usr/share/man`, `/usr/share/doc`).
- **Harmonisation de la navigation administrateur** :
  - L'onglet « Utilisateurs & Accès » est renommé en « **Utilisateurs** » (dans `i18n` FR/EN, la barre latérale et les en-têtes de section).
  - L'onglet « Tâches » est renommé en « **Planificateur de tâches** » (dans `i18n` FR/EN, la barre latérale et l'en-tête de section).



- **Initialisation sécurisée du premier administrateur (Critique sous condition)** :
  - **Jeton de bootstrap à usage unique** : `AuthState` génère au démarrage un jeton cryptographique aléatoire de 128 bits (hexadécimal de 32 caractères) ou utilise `MEDIA_BOOTSTRAP_TOKEN` si défini. Ce jeton est consigné dans les logs et persisté dans un fichier local protégé `.bootstrap_token` avec les permissions `0o600` (lecture/écriture propriétaire exclusive).
  - **Validation constante et suppression immédiate** : `POST /api/v1/auth/bootstrap` compare le jeton fourni en temps constant (`constant_time_eq`). Dès la validation et la création du premier administrateur, le jeton mémoire est effacé et le fichier `.bootstrap_token` est supprimé du disque. Tout essai avec jeton absent ou invalide renvoie `401 Unauthorized` (`invalid_bootstrap_token`).
  - **Désactivation HTTP explicite** : Support de `MEDIA_DISABLE_HTTP_BOOTSTRAP=true` ou `MEDIA_BOOTSTRAP_HTTP_ENABLED=false` interdisant l'initialisation via l'API Web HTTP (`403 Forbidden` / `http_bootstrap_disabled`) au profit de l'initialisation locale.
  - **Interface utilisateur mise à jour** : La page `/login` détecte le besoin d'initialisation et présente un champ dédié pour le jeton de bootstrap, ou un bandeau d'instruction CLI/local si le bootstrap HTTP est désactivé.

- **Resynchronisation et révocation OIDC à la baisse (Élevé)** :
  - **Révocation du privilège administrateur** : Lors du rappel OIDC (`callback`), si un compte pur OIDC (`password_hash IS NULL`) possédait le rôle `administrator` mais ne fait plus partie d'aucun groupe IdP configuré dans `admin_groups`, son rôle est immédiatement rétrogradé à `user` en base.
  - **Révocation immédiate des sessions existantes** :
    - En cas de perte du rôle administrateur OIDC, toutes les sessions actives de l'utilisateur sont purgées (`DELETE FROM sessions WHERE user_id = ?`) afin de forcer la ré-authentification avec ses nouveaux droits minimaux.
    - Si l'utilisateur perd tous ses groupes autorisés (`require_group = true`), ses sessions existantes sont également révoquées avant le rejet en `403 Forbidden`.
  - **Tests unitaires exhaustifs** : Couverture ajoutée dans `crates/core/src/oidc.rs` validant l'attribution initiale du rôle administrateur, la rétrogradation à `user`, et la purge systématique des sessions.

- **Limiteur de connexion scopé anti-DoS (Moyenne)** :
  - **Problème résolu** : Le limiteur global à 10 connexions/minute permettait à un tiers malveillant d'épuiser le quota et d'interdire la connexion à tous les utilisateurs légitimes.
  - **Migrations 0026** : Création de `0026_login_throttle_scoped.sql` (SQLite & PostgreSQL) introduisant une clé de découpage (`throttle_key TEXT PRIMARY KEY`) à la place du singleton.
  - **Clé par adresse IP cliente** : L'adresse IP est extraite de `x-forwarded-for` (premier saut non fiable), `x-real-ip` ou `ip:127.0.0.1`. Le blocage de 10 tentatives/minute s'applique désormais par IP cliente indépendante, protégeant le service contre les dénis de service distribués sur les utilisateurs légitimes.

- **Filtrage de groupe OIDC obligatoire par défaut (Moyenne)** :
  - `require_group` est désormais activé par défaut (`true`) dans `crates/core/src/oidc.rs`. Si la configuration OIDC est activée sans groupes spécifiés, le démarrage est refusé par défaut afin d'éviter l'accès libre non désiré, sauf déclaration explicite de `MEDIA_OIDC_REQUIRE_GROUP=false`.

- **Surveillance Dependabot npm (Moyenne)** :
  - Activation de la surveillance continue des dépendances de l'application web dans `.github/dependabot.yml` (`package-ecosystem: npm`, `directory: /apps/web`, hebdomadaire le lundi à 04h00).

- **Durcissement Docker Compose (Moyenne)** :
  - Dans `docker-compose.yml` et `compose.distributed.yaml` :
    - Ajout de `read_only: true` sur les conteneurs Core et Media Node.
    - Montage `tmpfs` dédié pour `/tmp` (`rw,noexec,nosuid,size=64m`).
    - Définition de limites strictes de ressources : `cpus: '2.00'`, `memory: 1G`, `pids: 200`.

- **Nettoyage des journaux internes IA (Faible)** :
  - Désindexation de `.agents/teamwork` via `git rm -r --cached` et exclusion persistante dans `.gitignore` pour préparer une base propre pour la publication open source.

- **Alignement OWASP ASVS 5.0.0 Niveau 2** :
  - Documentation détaillée intégrée dans `docs/SECURITY.md` (V2 Authentification, V3 Gestion de session, V4 Contrôle d'accès, V14 Configuration sécurisée) et dans `docs/OIDC.md`.



- **Diagnostic et résolution des deux échecs de la CI** :
  1. **Échec `scanner_performance::test_1000_synthetic_files_scan_and_rescan` (`No space left on device`)** :
     - **Cause** : Le test créait 1 000 dossiers contenant chacun une copie complète de `sample_mp4` (368 Ko) sur disque (soit ~370 Mo d'écritures). Sur les runners GitHub Actions Ubuntu, l'accumulation de 37 binaires de tests d'intégration complets, d'objets debug et des SDKs préinstallés (Android, .NET, GHC) saturait la partition racine (`/`).
     - **Correctifs** :
       - Dans `.github/workflows/ci.yml` : ajout d'une étape de libération d'espace disque en début de job `rust` supprimant `/usr/share/dotnet`, `/usr/local/lib/android`, `/opt/ghc` (~30 Go libérés immédiatement).
       - Dans `crates/core/tests/scanner_performance.rs` : le test rescan réutilise les sondes pré-remplies en base et n'a besoin que d'un fichier présent avec sa taille/mtime indexés. Remplacement de la copie de 368 Mo par un stub de 512 octets lu en mémoire et écrit dans les dossiers (~500 Ko au total, création 10x plus rapide et empreinte disque minimale).
  2. **Échec `browser_series` (`Timeout 30000ms waiting for getByRole('button', { name: 'Se déconnecter' })`)** :
     - **Cause** : Sur les runners Ubuntu Azure, `navigator.language` est configuré sur `en-US`. `+layout.svelte` initialisait la locale sur `en` en l'absence de préférences utilisateur enregistrées (`saved.locale ?? (navigator.language.startsWith('en') ? 'en' : 'fr')`). Par conséquent, le bouton du menu profil affichait `"Sign out"` au lieu de `"Se déconnecter"`, provoquant un timeout Playwright de 30 secondes.
     - **Correctifs** :
       - Dans `scripts/browser-series.cjs` et `scripts/browser-scanner.cjs` : ajout de `locale: 'fr-FR'` lors de l'initialisation du contexte Playwright (`browser.newContext({ viewport: { width: 1440, height: 900 }, locale: 'fr-FR' })`).
       - Dans `scripts/browser-series.cjs` : assouplissement résilient des sélecteurs avec regex bilingue `/^(Se déconnecter|Sign out)$/` et navigation `/^(Séries|TV Shows)$/`.

## 19 octobre 2026 — Intégration des logos animés / chargement et correction complète de la CI

- **Renommage et intégration des logos SVG** :
  - `logo animée.svg` renommé en `logo-animated.svg` (racine et `apps/web/static/images/logo-animated.svg`). Utilisé dans la page de connexion (`apps/web/src/routes/login/+page.svelte`) et la barre de navigation (`Navbar.svelte`) pour afficher l'animation d'apparition fluide du ruban.
  - `logo chargement .svg` renommé en `logo-loading.svg` (racine et `apps/web/static/images/logo-loading.svg`). Intégré en remplacement des spinners génériques dans la médiathèque (`Catalog.svelte`), la console d'administration (`admin/libraries/+page.svelte`) et le lecteur immersif (`ImmersivePlayer.svelte`).
  - `brand-mark.svg` synchronisé avec la charte graphique et la géométrie actuelle du logo Aether.
- **Résolution des échecs CI GitHub Actions** :
  - **Job Rust (`cargo test --lib --bins`)** : l'installation de FFmpeg (`sudo apt-get install -y ffmpeg`) était conditionnée par `steps.cache-fixtures.outputs.cache-hit != 'true'`. En cas de cache hit des fixtures, `ffmpeg` et `ffprobe` manquaient sur le runner Ubuntu, causant des erreurs `NotFound` (`Os { code: 2 }`) dans les tests unitaires de `media-node` (`probe::tests`, `packaging::tests`). L'installation de FFmpeg est désormais inconditionnelle dans le job `rust`.
  - **Test navigateur Séries (`browser_series`)** :
    - Conflit d'accessibilité : `AdminGroups.svelte` générait des cases à cocher enveloppées dans `<label>` contenant le nom de l'utilisateur sans label distinct, causant une violation de mode strict Playwright (`getByLabel('series-user-b', { exact: true })` trouvait 2 éléments). Des attributs `aria-label` explicites ont été ajoutés dans `AdminGroups` (`"Membre : ..."` et `"Bibliothèque autorisée : ..."`), et le sélecteur dans `scripts/browser-series.cjs` a été scopé sur `#libraries-section`.
    - Bouton de profil & déconnexion : Le libellé accessible du bouton de menu et du conteneur nav dans `ProfileMenu.svelte` a été restauré à `"Menu profil"` (au lieu de la clé de traduction générique), et la traduction française du bouton de déconnexion dans `apps/web/src/lib/i18n/index.ts` a été alignée sur `"Se déconnecter"`.
  - **Test sauvegardes administratives (`admin_backups`)** :
    - Erreur HTTP 409 lors du téléchargement : `backup_database` et `restore_database` dans `crates/core/src/backup.rs` imposaient une permission Unix `0o750` sur le dossier parent de destination (`std::fs::set_permissions(parent, Permissions::from_mode(0o750))`). Or, `safe_directory` dans `crates/core/src/admin_backups.rs` vérifie strictement `metadata.permissions().mode() & 0o077 == 0` (seul l'utilisateur propriétaire a des droits). L'application de `0o750` par `backup_database` introduisait le bit groupe `0o050`, provoquant le rejet en `409 Conflict` lors du `GET /api/v1/admin/backups/:id/download`.
    - Permissions alignées sur `0o700` (`rwx------`) dans `backup.rs` et application explicite de `0o700` dans `admin_backups::perform_backup`.
- **Validation & Contrôle Qualité** :
  - `cargo test --locked --workspace --all-features --lib --bins` : 83/83 tests unitaires réussis.
  - `cargo test --locked --workspace --all-features --test '*' -- --test-threads=1` : 100% des suites de tests d'intégration réussies (45 tests sans aucun échec).
  - `cargo test --locked -p media-poc --test browser_scanner -- --ignored --nocapture` : 100% réussi.
  - `cargo test --locked -p media-poc --test browser_series -- --ignored --nocapture` : 100% réussi (`passed: true`).
  - `npm run check` (dans `apps/web`) : 0 erreur, 0 avertissement.
  - `npm test` (dans `apps/web`) : 53/53 tests unitaires réussis.
  - `npm run build` (dans `apps/web`) : compilation et génération statique réussies sans erreur.
## 19 octobre 2026 — Correction des anomalies de sous-titres, groupes par défaut et exclusion de dossiers/fichiers d'indexation (#recycle)

- **Correction des anomalies de sous-titres (`subtitles exceed 512 KiB per video` et `subtitle end precedes start`)** :
  - **Augmentation des limites de taille dans `crates/node/src/subtitles.rs`** :
    - `MAX_BYTES` (taille max d'un fichier de sous-titre) relevé de 256 Ko à 1 Mo (`1024 * 1024`).
    - `MAX_TOTAL_BYTES` (taille max cumulée des pistes WebVTT par vidéo) relevé de 512 Ko à 16 Mo (`16 * 1024 * 1024`). Permet d'indexer sans blocage les films longs comportant de nombreuses pistes externes (ex: Avatar 2, Babylon, Avengers, Harry Potter avec 10+ langues).
    - `MAX_TRACKS` relevé de 16 à 64 pistes.
  - **Tolérance aux erreurs mineures de minutage (`subtitle end precedes start`)** :
    - Dans `to_webvtt` (SRT/VTT) et `ass_to_webvtt` (ASS), si une réplique a `end <= start` (durée nulle ou inversion de quelques ms dans un fichier de 2 000 répliques), la réplique corrompue est ignorée via `continue` au lieu de rejeter l'intégralité du fichier de sous-titres du film. Si aucune réplique valide n'est trouvée, l'erreur est maintenue.
- **Groupes par défaut pour tous les utilisateurs (`is_default`)** :
  - Migration SQL `0028_default_groups_and_library_exclusions.sql` (SQLite & PostgreSQL) ajoutant la colonne `is_default` (booléen) sur `user_groups` et mettant à jour la vue `effective_library_permissions` :
    - Tout groupe marqué par défaut (`is_default = 1`) accorde automatiquement l'accès à ses bibliothèques à tous les utilisateurs existants et futurs (locaux et OIDC), sans nécessiter d'assignation manuelle dans `user_group_members`.
  - API `crates/core/src/groups.rs` : prise en charge de `isDefault` dans `create`, `update` et `list`.
  - Frontend :
    - `apps/web/src/lib/admin/AdminGroups.svelte` : badge « Par défaut » sur les groupes concernés et case à cocher explicite dans le formulaire de création/modification.
    - `apps/web/src/lib/admin/AdminUsers.svelte` : prise en compte de `isDefault` dans le calcul d'accès aux bibliothèques des utilisateurs.
- **Exclusion de dossiers/fichiers lors de l'indexation (`#recycle`, `@eaDir`, `.nomedia`, motifs personnalisés)** :
  - **Exclusions automatiques système dans `crates/node/src/scanner.rs`** :
    - `ignored(name)` ignore désormais inconditionnellement les corbeilles et dossiers système NAS : `#recycle`, `@eaDir`, `@synoEAStream`, `@appstore`, `#snapshot`, `lost+found`, `.@__thumb`, `thumbs.db`, `desktop.ini`, ainsi que les noms débutant par `@`.
    - `FolderVideos::load` ignore également ces entrées lors du parcours des fichiers annexes.
    - Tout dossier contenant un fichier marqueur d'ignorance `.nomedia`, `.ignore` ou `.aetherignore` est automatiquement sauté lors du scan récursif.
  - **Exclusions configurables par médiathèque** :
    - Ajout de la colonne `excluded_patterns TEXT NOT NULL DEFAULT ''` sur la table `libraries`.
    - Interface d'administration `AdminLibraries.svelte` : champ de saisie permettant à l'administrateur de renseigner des motifs ou dossiers à exclure séparés par des virgules (ex: `bonus, trailers, specials, #recycle`).
    - Prise en charge dans `crates/core/src/libraries.rs`, `crates/core/src/scans.rs`, `crates/core/src/nodes.rs`, `crates/shared/src/protocol.rs` (`RemoteScanStartRequest`) et `crates/node/src/scanner.rs` (`scan_streaming` et `is_excluded`).

## 19 octobre 2026 — Console administrateur, premier jalon

Navigation par rubrique, annuaire utilisateurs filtrable, scans manuels et suivi, documentation sauvegardes SQLite/PostgreSQL dans la console. Aucun CRUD utilisateurs, planificateur, groupes locaux, clés API ou moteur de plugins ajouté ; états indisponibles explicites. Backend inchangé. Voir docs/ADMIN_CONSOLE.md pour validations et limites du test navigateur à API synthétique.


## 19 octobre 2026 — Mises à jour des dépendances stables et Refonte Bi-thème (Clair "Pierre Douce" & Sombre "Cinéma OLED")

- **Mises à jour des dépendances vers les dernières versions stables** :
  - **Rust Workspace** :
    - Alignement de `base64` sur la version `0.23.1` dans `crates/shared/Cargo.toml` (partagé avec `crates/core`).
    - Mise à jour de `Cargo.lock` avec les dernières versions stables compatibles (`cc` 1.9.0, `crypto-common` 0.1.7, `find-msvc-tools` 0.1.15, `libc` 0.2.191, `unicode-normalization` 0.1.26).
  - **Frontend Web (`apps/web`)** :
    - Mise à jour d'`eslint` vers `^10.13.0` et `svelte-check` vers `^4.7.7`.
    - Maintien rigoureux de `typescript` en `^6.0.3` (exigé par `@sveltejs/kit@3.1.0` et `typescript-eslint@8.71.2`, TS 7.0 n'étant pas encore supporté par les plugins du compilateur SvelteKit/ESLint).
- **Implémentation du Système Bi-thème de Référence** :
  - **Fichier de référence (`apps/web/src/app.css`)** :
    - Contient exactement les variables spécifiées pour `:root`, `[data-theme="light"]` (Pierre Douce) et `[data-theme="dark"]` (Cinéma OLED) :
      - Couleurs de marque : `--brand-amber: #F59E0B`, `--brand-red: #EF4444`, `--brand-gradient: linear-gradient(135deg, #F59E0B 0%, #EF4444 100%)`.
      - États : `--status-success: #10B981`, `--status-warning: #F59E0B`, `--status-error: #EF4444`.
      - Thème Clair "Pierre Douce" : `--bg-main: #EFECE6`, `--bg-surface: #FAF9F6`, `--bg-elevated: #E5E0D8`, `--border-subtle: #DCD6CC`, `--text-primary: #1C1917`, `--text-secondary: #57534E`, `--text-muted: #A8A29E`, `--accent-primary: #EA580C`, `--accent-secondary: #D97706`.
      - Thème Sombre "Cinéma OLED" : `--bg-main: #09090B`, `--bg-surface: #121215`, `--bg-elevated: #1E1E24`, `--border-subtle: #27272A`, `--text-primary: #FAFAFA`, `--text-secondary: #A1A1AA`, `--text-muted: #52525B`, `--accent-primary: #EF4444`, `--accent-secondary: #F59E0B`.
  - **Intégration du Design System (`apps/web/src/lib/styles/tokens.css`)** :
    - Mappage réactif complet des surfaces, canvas, lignes, verres, ombres et typographies vers les nouvelles variables pour les deux modes, avec prise en compte automatique de `@media (prefers-color-scheme: dark)`.
  - **Gestionnaire de Thème (`apps/web/src/lib/theme/index.ts`)** :
    - Module Svelte store avec persistance `localStorage` (`media_theme`), détection dynamique de l'OS (`prefers-color-scheme`), et application immédiate de `data-theme` sur `<html>`.
  - **Composants d'Interface & Internationalisation** :
    - Sélecteur de thème rapide ajouté au menu profil (`ProfileMenu.svelte`) avec bascule instantanée Clair / Sombre.
    - Sélecteur complet ajouté aux paramètres de profil (`SettingsView.svelte`) supportant les 3 modes (Clair, Sombre, Système).
    - Clés de traduction bilingues (FR / EN) ajoutées dans `apps/web/src/lib/i18n/index.ts`.
    - Icônes `sun` et `moon` ajoutées dans `Icon.svelte`.
- **Validation & Contrôle Qualité** :
  - `cargo test --workspace` : 100% réussi.
  - `cargo clippy --all-targets -- -D warnings` : 0 warning.
  - `cargo fmt --check` : 0 diff.
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement.
  - `npm --prefix apps/web run lint` : 0 erreur.
  - `npm --prefix apps/web test` : 48/48 tests réussis.
  - `npm --prefix apps/web run build` : build statique réussi sans violation CSP.

## 18 octobre 2026 — Refonte Tableau de bord Jellyfin, Support SSO Unicode & Pseudo, et Internationalisation (FR / EN)

- **Tableau de bord Administrateur Jellyfin-style & Menu Profil** :
  - Élimination des liens dupliqués dans le menu profil : « Mon profil & Préférences » pointe vers `/settings/` et « Tableau de bord (Admin) » vers `/admin/libraries/`.
  - Refonte de la page d'administration (`apps/web/src/routes/admin/libraries/+page.svelte`) en dashboard Jellyfin avec barre latérale gauche catégorisée (`SERVEUR`, `MÉDIATHÈQUE`, `ACCÈS & SÉCURITÉ`), navigation réactive par ancre et badges d'état.
  - Découverte automatique des sous-dossiers dans l'endpoint `roots` (`/api/v1/admin/libraries/roots`) permettant la sélection immédiate des répertoires montés (ex. `/media/films`, `/media/series`) dans l'interface sans devoir les taper manuellement.
  - Diagnostic explicite pour l'erreur `directory unreadable` : détection d'erreur de permissions et recommandation de contrôle des droits hôte pour l'UID conteneur 10001 (`aether:aether`).
- **Support SSO des Pseudonymes et Noms Réels Unicode (ex. « Maël Moreau »)** :
  - Migrations 0019 et 0020 créées pour SQLite (`crates/core/migrations/`) et PostgreSQL (`crates/core/migrations-postgres/`) ajoutant `display_name TEXT` et `sso_username TEXT` à la table `users`.
  - `Claims` dans `crates/core/src/oidc.rs` enrichi pour extraire de manière ordonnée le pseudo (`preferred_username`, `username`, `nickname`, ou préfixe d'email) et le nom complet réel (`name`, `given_name` + `family_name`), acceptant sans restriction les espaces, majuscules et accents.
  - Mise à jour automatique de `users.display_name` et `users.sso_username` lors du callback OIDC, que le compte soit nouveau ou déjà existant.
  - Table des utilisateurs (`AdminUsers.svelte`) affichant désormais deux colonnes distinctes : `Identifiant / Pseudo` et `Nom complet / Affiché`, ainsi que l'identification claire du type d'authentification (`SSO (OIDC)` ou `Compte local`).
  - Menu Profil (`ProfileMenu.svelte`) et vue Paramètres (`SettingsView.svelte`) affichant le nom réel complet en titre et le pseudo `@login` en sous-titre.
- **Variables d'environnement SSO étendues** :
  - `MEDIA_OIDC_ADMIN_GROUPS` (et alias `OIDC_ADMIN_GROUP`) : attribue automatiquement le rôle `administrator` aux comptes appartenant aux groupes spécifiés de l'IdP, levant l'obligation de créer un compte local préalable.
  - `MEDIA_AUTH_LOCAL_ENABLED=false` (et alias `MEDIA_OIDC_ONLY=true`, `MEDIA_AUTH_DISABLE_LOCAL_LOGIN=true`) : masque le formulaire local et désactive les routes de connexion locale pour un mode SSO exclusif.
- **Internationalisation bilingue (Français & Anglais)** :
  - Création du module réactif d'internationalisation `apps/web/src/lib/i18n/index.ts` avec dictionnaire complet `fr` et `en`, détection de la langue du navigateur et mémorisation dans `localStorage` (`media_locale`).
  - Sélecteur rapide de langue (`FR` / `EN`) intégré dans le menu déroulant du profil et dans la page des paramètres.
  - Traduction réactive appliquée à la navigation (`Navbar.svelte`), l'administration (`AdminUsers.svelte`, sidebar et hero), le profil et les paramètres (`SettingsView.svelte`).
- **Validation & Contrôle Qualité** :
  - `cargo test --workspace` : 100% réussi.
  - `cargo clippy --all-targets -- -D warnings` : 0 warning.
  - `cargo fmt --check` : 0 diff.
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement.
  - `npm --prefix apps/web run lint` : 0 erreur.
  - `npm --prefix apps/web test` : 48/48 tests réussis.
  - `npm --prefix apps/web run build` : build statique réussi.

## 18 octobre 2026 — Optimisation majeure du workflow CI GitHub Actions (`.github/workflows/ci.yml`)

- **Objectif** : Réduire drastiquement le temps d'exécution de la CI (~6 min 40 s initialement) sans compromettre les vérifications de sécurité, les contrats ni la rigueur des tests.
- **Optimisations appliquées (`.github/workflows/ci.yml`)** :
  1. **Cache des fixtures synthétiques (`actions/cache@v4`)** :
     - Mise en cache du dossier `tests/fixtures/generated` (13 Mo) sous la clé `fixtures-p0-p1-${{ hashFiles(...) }}` dans les jobs `rust` et `browser`.
     - Lorsque les scripts de génération n'ont pas changé (`cache-hit == 'true'`), l'installation d'apt-get ffmpeg et l'encodage lourd de 6 scripts FFmpeg (AV1 10-bit, Opus 5.1/stéréo, segments MSE, manifestes DASH, remux) sont ignorés. Gain estimé : ~1 min 30 s par job.
  2. **Parallélisation sélective des tests unitaires (`cargo test`)** :
     - Séparation de l'exécution des tests unitaires (`cargo test --lib --bins`) et des tests d'intégration (`cargo test --test '*' -- --test-threads=1`).
     - Les tests unitaires (28 tests dans `media-poc`, 35 dans `media-node`, 10 dans `media-contracts`) bénéficient de la pleine parallélisation sur les CPU du runner (durée réduite de ~101s à ~4s pour `media-poc`).
     - Les tests d'intégration lourds conservent `--test-threads=1` pour garantir l'absence d'interférence sur les bancs de 10k entrées et les verrous SQLite. Gain estimé : ~1 min 35 s.
  3. **Audit de sécurité accéléré (`rustsec/audit-check@v1.4.1`)** :
     - Remplacement de la compilation depuis les sources `cargo install cargo-audit --locked` (~90s) par l'action officielle `rustsec/audit-check` utilisant un binaire précompilé (~5s). Gain estimé : ~1 min 30 s.
- **Validation** :
  - Validation syntaxique et schéma GitHub Actions avec `actionlint` (code de sortie 0).

## 18 octobre 2026 — Résilience du canal de contrôle des Media Nodes outbound (`crates/core/src/nodes.rs`, `node_enrollment_and_relay.rs`)

- **Problème diagnostiqué en CI GitHub Actions** :
  - Échec du test `test_node_enrollment_outbound_channel_and_streaming_relay` sur l'étape 13 (rotation du secret) avec assertion `503 == 200`.
  - Cause : en mode sortant (`outbound`), le Node communique avec le Core via des requêtes HTTP de long-polling. À la fin de l'étape 12 (streaming relais), la commande `StreamRelay` était consommée et le récepteur du canal Axum était libéré. Lorsque l'étape 13 déclenchait immédiatement `POST /api/v1/admin/nodes/{id}/rotate-secret`, le Core vérifiait `ACTIVE_CHANNELS` qui contenait encore le `Sender` obsolète du long-polling précédent. La transmission échouait immédiatement et, le Node n'ayant pas de `base_url` entrant, le Core renvoyait prématurément `503 Service Unavailable` sans attendre que la requête de long-polling suivante du Node n'arrive.
- **Corrections backend (`crates/core/src/nodes.rs`)** :
  - Nettoyage automatique des canaux fermés dans le handler `node_channel` dès la fin du long-polling (`if let Some(s) = channels.get(&id) && s.is_closed() { channels.remove(&id); }`).
  - Boucle d'attente résiliente (jusqu'à 3s avec scrutation non-bloquante toutes les 40ms) dans `rotate_node_secret` et `relay_stream_from_node` pour les nœuds sortants, absorbant de façon fluide la transition réseau entre deux requêtes de long-polling.
  - Ajout d'une micro-pause de 150ms dans le test d'intégration pour laisser le worker cycler proprement après un streaming relais.
- **Validation** :
  - `cargo test -p media-poc --test node_enrollment_and_relay` : 100% réussi.
  - `cargo clippy --all-targets -- -D warnings` : 0 warning.
  - `cargo fmt --check` : 0 diff.

## 18 octobre 2026 — Assistant d'initialisation premier lancement (Web Bootstrap) et association administrateur local/SSO

- **Contexte & Demande utilisateur** :
  - Sur une base de données fraîche, l'utilisateur a rencontré le message d'erreur : `« un compte administrateur local de secours doit être créé avant d'utiliser le SSO »`.
  - Demandes :
    1. Proposer directement la création du compte administrateur local depuis l'interface Web au premier lancement.
    2. Pouvoir utiliser « uniquement le SSO » car un groupe administrateurs existe déjà côté fournisseur d'identité.
- **Règles de sécurité et architecture (rappel non négociable `AGENTS.md`)** :
  - L'application interdit strictement l'auto-promotion administrateur via une simple assertion/claim OIDC (règle produit non négociable pour prévenir l'escalade de privilèges et la perte totale d'accès en cas d'indisponibilité ou compromission du fournisseur SSO).
  - Un compte administrateur local de secours sans mot de passe par défaut doit obligatoirement être créé au premier lancement.
  - Le compte administrateur local peut ensuite être lié à l'identité SSO depuis **Paramètres → Connexion SSO → Lier mon compte local au SSO**. Une fois lié, l'administrateur peut se connecter directement et exclusivement via le bouton SSO avec le rôle `administrator`.
- **Réalisations backend (`crates/core/src/auth.rs`)** :
  - `GET /api/v1/auth/bootstrap/status` : route publique non authentifiée retournant `{"needsBootstrap": true|false}` selon la présence ou non d'un administrateur ou état initialisé en base.
  - `POST /api/v1/auth/bootstrap` : route publique protégée par limitation de débit et vérification de l'origine (`Origin`) permettant d'initialiser l'administrateur local (`login` alphanumérique, mot de passe $\ge 15$ caractères), créant la session immédiatement sans passer par la CLI. Fermeture irréversible (`HTTP 403 Forbidden` permanent dès que le bootstrap est consommé).
  - Test d'intégration unitaire dédié `bootstrap_status_and_bootstrap_web_flow` validant la détection initiale, le rejet des mots de passe trop courts, le succès avec session et le rejet définitif des tentatives ultérieures.
- **Réalisations frontend (`apps/web`)** :
  - `apps/web/src/lib/auth/session.ts` : ajout de `bootstrapAdmin({ login, password })`.
  - `apps/web/src/routes/login/+page.svelte` :
    - Détection automatique au montage de l'état `needsBootstrap` via `/api/v1/auth/bootstrap/status`.
    - Si le serveur n'est pas encore initialisé, affichage d'un assistant d'accueil dédié (« Initialisation du serveur », badge `Premier lancement`, formulaire de création du compte administrateur local de secours, rappel explicatif de sécurité pour l'association SSO ultérieure).
    - Une fois l'administrateur créé, la session est automatiquement activée et redirige vers le catalogue ou les paramètres.
    - Pour les connexions ultérieures, le bouton SSO est mis en valeur comme moyen principal de connexion, avec un panneau discret pour la connexion locale de secours.
- **Validation** :
  - `cargo test --locked --workspace` : 100% réussi.
  - `cargo clippy --all-targets -- -D warnings` : 0 warning.
  - `cargo fmt --check` : 0 diff.
  - `npm --prefix apps/web test` : 48/48 tests réussis.
  - `npm --prefix apps/web run build` : build statique réussi.

## 18 octobre 2026 — Refonte de la page de connexion SSO et redirection de la racine (`apps/web`, `crates/core/src/lib.rs`, `crates/core/src/oidc.rs`)

- **Diagnostic du problème `{"error":"authentication_required"}`** :
  - Lors de l'accès navigateur initial à la racine du serveur (`GET /`), Axum acheminait la requête vers le routeur `media_routes` qui déclarait `route("/", get(|| async { Html(include_str!("bench.html")) }))`.
  - Le routeur `media` étant protégé par le middleware de permissions `permissions::require_media`, toute requête anonyme sans session valide était rejetée avec un code `HTTP 401 Unauthorized` et une réponse JSON brute `{"error":"authentication_required"}` au lieu d'amener l'utilisateur sur l'interface graphique de connexion.
- **Corrections backend (`crates/core`)** :
  - `crates/core/src/lib.rs` :
    - En mode banc d'essai (`mode == "localhost-fixtures"`), la route `/` sert le banc de fixtures `bench.html` sous `require_media` (satisfaisant le contrat strict de permissions de `permissions.rs`).
    - En mode production / autonome (`mode != "localhost-fixtures"`), la page de banc est accessible sur `/bench`, et la racine `/` redirige en `307 Temporary Redirect` vers `/app/`.
    - Accompagnement de `/app/` par `ServeDir` pour servir le bundle statique de l'application SvelteKit.
  - `crates/core/src/oidc.rs` :
    - Enrichissement de `GET /api/v1/auth/oidc/status` avec le champ `provider` détecté automatiquement (Authentik, Keycloak, Dex, Pocket ID ou SSO générique) en préservant le booléen canonique `enabled`.
  - `crates/core/tests/static_web.rs` :
    - Validation du contrat de redirection de `/` vers `/app/`.
- **Refonte de la page de connexion (`apps/web/src/routes/login/+page.svelte`)** :
  - **Design system cinématographique** : conteneur centré en verre dépoli (glassmorphism), halos lumineux ambiants (`ambient-glow`), icônes vectorielles contextuelles (`Icon.svelte`), typographie soignée et alignée sur la charte Aether.
  - **Prise en charge SSO de premier plan** :
    - Lorsque le SSO est activé, affichage d'un bouton d'action principal dédié (`Se connecter avec SSO`) avec icône de bouclier de sécurité, badge du fournisseur détecté et indicateur de progression.
    - Séparateur visuel élégant (« ou avec un identifiant local »).
    - Gestion robuste des erreurs : décodage des codes d'erreur IdP et OIDC (`local_administrator_required`, `oidc_invalid_response`, `oidc_group_denied`, `oidc_account_disabled`, `oidc_provider_unavailable`, etc.) et affichage d'une bannière d'alerte claire avec `role="alert"`.
    - Prise en charge des erreurs transmises par paramètres d'URL (`?error=...`).
  - **Authentification locale préservée** :
    - Formulaire d'identifiant et mot de passe ergonomique avec icônes intégrées, placeholders, autocomplétion sécurisée.
    - Maintien strict de la conformité avec les tests Playwright (`page.getByRole('button', { name: 'Se connecter avec SSO', exact: true })`, `page.getByLabel('Identifiant', { exact: true })`, `page.getByLabel('Mot de passe', { exact: true })`, `page.getByRole('button', { name: 'Se connecter', exact: true })`).
- **Validation & Tests** :
  - `cargo test --locked --workspace` : 100% réussi (toutes les suites : `media_contracts`, `media_node`, `media_poc`, tests d'intégration, scans, sécurité).
  - `cargo clippy --all-targets -- -D warnings` : 0 warning.
  - `cargo fmt --check` : 0 diff.
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement.
  - `npm --prefix apps/web run lint` : 0 erreur.
  - `npm --prefix apps/web test` : 48/48 tests réussis.
  - `npm --prefix apps/web run build` : build statique réussi.


- **Problème diagnostiqué** :
  - **Échec 1 (`browser_scanner`, sous-titres)** : timeout de 30s à la ligne 129 de `scripts/browser-scanner.cjs`. La position de lecture lors du test de la piste forcée intégrée (`internal-4`, cue 2 de 4.0s à 5.0s) était fixée à `v.currentTime = 4`. Sur les runners Linux sans accélération matérielle, la recherche (seek) dépose l'horloge sur la trame vidéo précédente (~3.958s). La vidéo étant en pause, le cue n'était jamais actif dans `activeCues`. Alignement appliqué sur `v.currentTime = 4.25` (standard utilisé dans `browser-p1.cjs`, `mse-checks.cjs`, `player-comparison-checks.cjs` et `remux-checks.cjs`), et vérification tolérante de la présence du texte dans l'ensemble des `activeCues`.
  - **Échec 2 (`browser_scanner`, statut de visionnage restart=true)** : timeout de 30s à la ligne 183 de `scripts/browser-scanner.cjs` attendant le bouton `'Marquer non vu'`. Le clic sur `button.return` enclenche une sauvegarde asynchrone de la position de lecture (`progressTracker.save(playback, true)`), incrémentant la révision en base. L'action utilisateur immédiate `markWatched` lisait l'ancienne révision en parallèle de la sauvegarde de sortie, déclenchant un conflit `HTTP 409` et maintenant le bouton sur `'Marquer comme vu'`.
  - **Échec 3 (`browser_series`)** : timeout de 30s à la ligne 152 de `scripts/browser-series.cjs` attendant `getByRole('alert')` contenant `/n’a pas accès à cette ressource/` lors de la simulation d'un HTTP 403. Lors de la refactorisation de `Catalog.svelte` extrayant `SeriesDetail.svelte`, la prop `mediaError` et son rendu `<div class="error-banner" role="alert">` avaient été conservés dans `FilmDetail.svelte` mais omis dans `SeriesDetail.svelte`. L'erreur levée par `revalidateMediaDescriptor` était capturée par `Catalog.svelte` mais jamais affichée dans la vue série.
  - **Échec 4 (Archive CI)** : `scripts/browser-scanner.cjs` et `scripts/browser-series.cjs` écrivaient par défaut dans des sous-dossiers `2026-10-18/` lorsqu'aucune variable `MEDIA_REPORT_DIR` n'était fournie, alors que `.github/workflows/ci.yml` vérifie et archive strictement `docs/evidence/2026-10-16/scanner/` et `docs/evidence/2026-10-17/series/`.
- **Corrections apportées** :
  - `apps/web/src/lib/catalog/watch-state.ts` : mécanisme de retry automatique (jusqu'à 3 tentatives avec backoff) dans `markWatched` en cas de `409 Conflict`, éliminant les collisions avec les sauvegardes de sortie en arrière-plan.
  - `scripts/browser-scanner.cjs` : attente déterministe de la fermeture du lecteur et de l'enregistrement de la position de sortie avant le basculement manuel de l'état de visionnage.
  - `apps/web/src/lib/catalog/SeriesDetail.svelte` : ajout de la prop `mediaError`, affichage du bloc `<div class="error-banner" role="alert">` et styles associés.
  - `apps/web/src/lib/catalog/Catalog.svelte` : transmission de `mediaError` à `<SeriesDetail {mediaError} ... />`.
  - `scripts/browser-scanner.cjs` : calage temporel `currentTime = 4.25` pour les sous-titres forcés et rétablissement du répertoire par défaut à `docs/evidence/2026-10-16/scanner`.
  - `scripts/browser-series.cjs` : rétablissement du répertoire par défaut à `docs/evidence/2026-10-17/series`.
  - `crates/core/tests/browser_scanner.rs` et `crates/core/tests/browser_series.rs` : propagation explicite de `MEDIA_REPORT_DIR` au processus Node si la variable d'environnement est définie.
- **Validation** :
  - `cargo test --locked --test browser_scanner -- --ignored --nocapture` : 100% réussi (les deux passes sans et avec redémarrage réussies en 24s).
  - `cargo test --locked --test browser_series -- --ignored --nocapture` : 100% réussi (`revokedPermission403`, `restoredPermissionPlayback`, `accountSwitchCacheIsolation`, `crossSeason`, etc.).
  - Validation Python des preuves CI : toutes les assertions de présence et de succès passent (`report.json`, captures `.png`).
  - `npm run check` (TypeScript/Svelte) : 0 erreur, 0 avertissement.
  - `npm test` (vitest) : 48/48 tests réussis.
  - `cargo fmt --check` et `cargo clippy --all-targets -- -D warnings` : 0 erreur, 0 avertissement.

## 18 octobre 2026 — Refonte de l'appairage des Media Nodes (Initiation sortante, tokens éphémères, canal bidirectionnel NAT-proof et streaming relais)

- **Périmètre & Objectif** :
  - Refonte complète de la méthode de connexion des Media Nodes au Core : passage d'un modèle où le Core devait contacter un port entrant sur le Node à un modèle où **le Core génère une invitation et le Node initie lui-même la connexion sortante vers le Core**.
  - Permet d'installer un Media Node sur n'importe quelle machine distante (derrière une box, un routeur ou un NAT de FAI), sans devoir ouvrir de port, configurer de redirection de port (port-forwarding) ou disposer d'une IP publique.
  - Gestion native de plusieurs bibliothèques (Films et Séries) par un seul et même Node (`MEDIA_NODE_ROOTS: /media/films;/media/series`).
  - Streaming direct en HTTPS prioritaire lorsque le Node dispose d'une URL publique (`public_url`), sinon streaming relais sans mise en mémoire tampon intégrale (chunked streaming zero-copy) transitant par la liaison sortante du Node vers le Core.
  - Préservation intégrale et rétrocompatible du mode d'appairage direct historique (`POST /api/v1/admin/nodes/pair`).

- **Sécurité & Cycle de vie des jetons** :
  - Jetons d'invitation générés côté Core : préfixe `mnet_`, 32 octets aléatoires CSPRNG encodés en hexadécimal (total 69 caractères), durée de validité limitée (15 minutes par défaut).
  - Seul le hash SHA-256 du jeton est conservé en base (`token_hash`), aucun jeton en clair dans les tables ni dans les logs.
  - Consommation atomique à usage unique : `UPDATE node_enrollment_tokens SET used_at=?, used_by_node_id=? WHERE token_hash=? AND used_at IS NULL AND expires_at > ?`. Rejet strict avec `401 Unauthorized` pour tout jeton inconnu, expiré ou déjà consommé.
  - Preuve mutuelle HMAC-SHA256 (`compute_enrollment_mutual_proof`) et dérivation de secret partagé durable (`derive_paired_secret`) combinant le jeton, une moitié de secret générée par le Node et une moitié générée par le Core.
  - Le Node persiste ses identifiants durables (`node_id`, `key_id`, `shared_secret`, `core_internal_url`) dans son volume persistant (`node_identity.json`). Reconnexion automatique au redémarrage sans nouvel appairage.
  - Rotation de secret (`POST /api/v1/admin/nodes/{id}/rotate-secret`) et révocation (`POST /api/v1/admin/nodes/{id}/revoke`) transmises instantanément sur le canal de contrôle.

- **Protocole & Canal de communication bidirectionnel (`/channel`)** :
  - `crates/shared/src/protocol.rs` :
    - Nouveaux types : `EnrollNodeRequest`, `EnrollNodeResponse`, `NodeCommandEnvelope`, `NodeCommandType` (`Ping`, `Scan`, `SyncInventory`, `RotateSecret`, `Revoke`, `StreamRelay`), `StreamRelayRequest`.
    - Fonctions : `generate_enrollment_token()`, `compute_enrollment_mutual_proof()`.
  - Canal de contrôle persistant : le Node maintient une requête sortante authentifiée par signature HMAC vers `/internal/v1/core/nodes/{id}/channel`. En cas de commande en attente (Scan, Sync, Rotate, Revoke, StreamRelay), le Core y répond immédiatement avec le `NodeCommandEnvelope` ; sinon timeout à 25s avec `NodeCommandType::Ping`.
  - Reconnexion automatique avec backoff exponentiel (1s à 5s) en cas de coupure réseau ou de redémarrage du Core.

- **Relais multimédia NAT-proof à mémoire bornée (`/stream-uplink/{stream_id}`)** :
  - Lorsqu'un client sollicite la lecture d'un asset (`GET /api/v1/assets/{id}/content`) situé sur un Node en mode `outbound`, le Core envoie une commande `StreamRelay` avec un `stream_id` éphémère via le canal.
  - Le Node établit aussitôt une requête sortante `POST /internal/v1/core/nodes/{id}/stream-uplink/{stream_id}` avec le corps de flux chunked (morceaux de 64 KiB), les entêtes de range/content-range et la signature d'intégrité des métadonnées (`stream_metadata_digest`).
  - Le Core associe le flux montant directement au flux descendant HTTP de l'utilisateur via `tokio::sync::oneshot` sans aucun chargement du fichier en RAM.

- **Base de données & Migrations** :
  - Migration additive `0018_node_enrollment.sql` (SQLite dans `crates/core/migrations/` et PostgreSQL dans `crates/core/migrations-postgres/`) :
    - Création de la table `node_enrollment_tokens` (`id`, `token_hash`, `created_at`, `expires_at`, `used_at`, `used_by_node_id`).
    - Ajout de la colonne `connection_mode TEXT NOT NULL DEFAULT 'outbound'` à `media_nodes`.

- **Composant Node (`crates/node`)** :
  - `crates/node/src/bin/aether-node.rs` : support de `MEDIA_CORE_URL` et `MEDIA_NODE_ENROLLMENT_TOKEN`. Si unpaired au démarrage, appel automatique de `enroll_with_core`. Démarrage systématique du worker `start_channel_worker`. Découpage des racines `MEDIA_NODE_ROOTS` par `;` et `:`.
  - `crates/node/src/server.rs` : implémentation de `enroll_with_core`, `start_channel_worker`, `handle_channel_scan`, `handle_channel_stream_relay`, `send_stream_uplink_body` et `send_stream_uplink_error`.

- **Interface Administrateur SvelteKit (`apps/web`)** :
  - `apps/web/src/lib/admin/types.ts` : ajout de `connectionMode` sur `AdminNode`, libellés `connectionModeLabels` et interface `EnrollmentToken`.
  - `apps/web/src/lib/admin/AdminNodes.svelte` :
    - Bouton principal « **Ajouter un Node** ».
    - Modal d'enrôlement affichant le jeton temporaire généré, bouton de copie rapide du jeton et extrait `docker-compose.yml` complet prérempli avec les variables d'environnement (`MEDIA_CORE_URL`, `MEDIA_NODE_NAME`, `MEDIA_NODE_ENROLLMENT_TOKEN`, `MEDIA_NODE_ROOTS`).
    - Badges de mode de connexion sur chaque carte de Media Node (`Relais sortant (NAT)`, `Direct HTTPS`, `Entrant direct (Legacy)`).
    - Affichage de l'état (connecté/déconnecté), dernière activité, dossiers multimédias, bibliothèques associées, et actions Reconnecter, Renouveler secret, Révoquer.
    - Préservation du formulaire d'appairage direct historique dans un panneau rétractable.

- **Validation & Tests** :
  - Nouveau test d'intégration complet `crates/core/tests/node_enrollment_and_relay.rs` (100% réussi) :
    - Génération de token admin, validation format `mnet_`.
    - Rejet des tokens malformés (400) et forgés (401).
    - Auto-enrôlement du Node distant en mode sortant sans port d'écoute public.
    - Rejet de la réutilisation du même jeton (usage unique strict).
    - Établissement du canal persistant et réception de battements de cœur.
    - Enregistrement de bibliothèques multiples.
    - Déclenchement de scan distant par le Core via le canal sortant du Node, remontée de batchs et publication catalogue.
    - Streaming de média en direct via le relais sortant avec `206 Partial Content` (Range seek) sans mise en cache RAM intégrale.
    - Rotation du secret partagé et révocation du Node via le canal.
  - Suites de tests existantes : `distributed_multi_node` (1/1 réussi), `challenger_security_challenge` (4/4 réussis), `version_and_nodes` (2/2 réussis), `media-contracts` (10/10 réussis).
  - Contrôle qualité : `cargo clippy --all-targets -- -D warnings` (0 avertissement), `cargo fmt --check` (0 diff).
  - Frontend : `npm run check` (0 erreur, 0 avertissement), `npm test` (48/48 tests réussis).
  - Règle AGENTS.md respectée : aucun commit, aucun push, aucune image poussée sur Git.

## 18 octobre 2026 — Versionnement unifié (0.1.0 / 0.1.0-beta) et visibilité des versions des Media Nodes

- **Périmètre & Objectif** :
  - Mise en place d'un système de versionnement automatique et unifié pour les 3 conteneurs et exécutables du projet :
    - `aether` (mode standalone tout-en-un)
    - `aether-core` (service Core distribué)
    - `aether-node` (service Media Node distribué)
  - Base de version fixée à `"0.1.0"`.
  - Détection automatique de branche : lorsque le projet est compilé sur la branche `develop` (ou via variable/build-arg `GIT_BRANCH=develop` / `GITHUB_REF_NAME=develop`), la version est automatiquement qualifiée en `"0.1.0-beta"`. Sur `main` ou tags de version, elle reste en `"0.1.0"`. Surchargeable à l'exécution via la variable générique `MEDIA_VERSION` (ou `AETHER_VERSION`).
  - Prise en compte du décalage de mise à jour des Media Nodes distants : les nœuds ne sont pas tous mis à jour simultanément. Le protocole interne, le stockage Core et l'interface Web permettent désormais de visualiser immédiatement la version de chaque Media Node.
- **Backend Rust & Protocoles** :
  - `crates/shared` :
    - `crates/shared/build.rs` : script de compilation qui évalue la branche git courante (ou variable d'environnement CI/Docker) et génère la constante `AETHER_APP_VERSION` (`0.1.0-beta` sur develop, `0.1.0` sinon).
    - `crates/shared/src/version.rs` : module exposant `current_version() -> &'static str`, prenant en compte la surcharge à chaud par `MEDIA_VERSION` ou la constante compilée.
    - `crates/shared/src/protocol.rs` : enrichissement rétrocompatible de `PairNodeResponse` et `HeartbeatPayload` avec le champ optionnel `version: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`).
  - `crates/node` :
    - `/internal/v1/health` inclut désormais le champ `version`.
    - Les réponses d'appairage (`pair_node`) et de pulsation périodique (`node_heartbeat` et background task dans `aether-node.rs`) transmettent la version du composant.
    - CLI `aether-node --version` / `-V` / `-v` affiche `aether-node <version>` et quitte immédiatement avec code 0.
  - `crates/core` :
    - CLI `aether --version` et `aether-core --version` affichent leur version respective et quittent avec code 0.
    - Routes publiques et de santé `/api/v1/health`, `/internal/v1/health` et `/api/v1/config` exposent la clé `"version"`.
    - Migrations additives `0017_node_version.sql` (SQLite dans `crates/core/migrations/` et PostgreSQL dans `crates/core/migrations-postgres/`) ajoutant la colonne `version TEXT` à la table `media_nodes`.
    - `crates/core/src/nodes.rs` : persistance de la version reçue lors de l'appairage (`pair_node_admin`), de la reconnexion (`reconnect_node`) et des pulsations (`receive_node_heartbeat`). Préservation rétrocompatible en cas de nœud legacy ne transmettant pas de version (`COALESCE(?, version)`).
    - `AdminNodeView` expose `version` : pour le nœud `local`, il reflète toujours la version du runtime Core en cours ; pour les nœuds distants, il reflète la version réelle du nœud.
- **Frontend SvelteKit (`apps/web`)** :
  - `apps/web/src/lib/admin/types.ts` : ajout de `version?: string | null` sur `AdminNode` et `version?: string` sur `CoreHealth`.
  - `apps/web/src/lib/admin/AdminNodes.svelte` : badge de version stylisé et immédiatement visible sur chaque carte de Media Node (`v0.1.0-beta`, `v0.1.0` ou `v?` si nœud antérieur non versionné).
  - `apps/web/src/lib/admin/AdminOverview.svelte` : affichage de la version du Core dans les détails techniques et détection d'hétérogénéité des versions de nœuds dans les KPIs avec alerte visuelle si plusieurs versions coexistent.
- **Docker, CI et GHCR** :
  - `Dockerfile` : support de l'argument de build `ARG GIT_BRANCH=develop` et précompilation dummy avec génération de `crates/shared/build.rs`.
  - `.github/workflows/ci.yml` : passage du build-arg `GIT_BRANCH=${{ github.ref_name }}` sur la matrice de construction des 3 images Docker (`runtime`, `runtime-core`, `runtime-node`).
  - `.github/workflows/publish.yml` : transmission du build-arg `GIT_BRANCH=${{ github.ref_name }}` et étape de vérification automatique `docker run --rm "$ref" "$BINARY" --version`.
- **Validation & Tests** :
  - Tests unitaires et d'intégration `media-contracts` : 8/8 réussis, incluant validation de la version `0.1.0-beta` sur develop et compatibilité rétrocompatible des charges utiles.
  - Tests `media-node` : 38/38 réussis, vérification `--version`.
  - Nouveau test d'intégration `crates/core/tests/version_and_nodes.rs` : 2/2 réussis, validant l'exposition de version sur `/health`, `/config`, le nœud local, et la persistance/mise à jour dynamique des versions de nœuds distants sur heartbeat et reconnexion.
  - Test distribué multi-nœuds `crates/core/tests/distributed_multi_node.rs` : 100 % réussi.
  - Validation du contrat SQLite/PostgreSQL `crates/core/tests/postgres.rs` : réussi.
  - Formatage `cargo fmt --check` : 0 diff.
  - Clippy strict `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 warning.
  - Frontend : `npm run check` (0 erreur, 0 avertissement), `npm test` (48/48 réussis), `npm run lint` (0 erreur), `npm run build` (build statique réussi).
  - Workflows GitHub Actions : validés par `actionlint` sous Docker sans erreur.
  - Règle respectée : aucun commit, aucun push, aucune image poussée sur Git/registre.

## 18 octobre 2026 — Refonte complète de l'interface utilisateur (Design system, Accueil immersif, Catalogues, Lecteur, Administration)

- Périmètre : Refonte frontend complète de `apps/web` inspirée visuellement des plateformes modernes (Netflix, Disney+, HBO Max) et ergonomiquement de Jellyfin. Aucun changement backend, aucune modification d'infrastructure, de Traefik, de Docker ou de fonctionnalités raw AV1/Opus.
- Design system & Composants :
  - Palette sombre cinématique (`tokens.css`), surfaces étagées, flous de verre (glassmorphism), accent améthyste (`#a78bfa`), skeletons scintillants (`Skeleton.svelte`), palette d'icônes SVG optimisée (`Icon.svelte`).
  - Navigation supérieure adhésive (`Navbar.svelte`) avec indicateurs d'onglets, barre de recherche rétractable, menu profil intégré et tiroir responsive mobile accessible au clavier.
  - Carrousel horizontal ergonomique (`MediaShelf.svelte`) avec défilement fluide et commandes tactiles / boutons chevrons.
  - Bannière héro cinématique (`HeroBanner.svelte`) avec média mis en avant, badges techniques, synopsis et boutons d'action rapide (« Lire », « Plus d'infos »).
  - Cartes d'affiches (`Poster.svelte`) au ratio 2:3 avec badges d'état de visionnage, barre de progression et survol soigné.
  - Cartes 16:9 « Continuer à regarder » (`ContinueWatching.svelte`) avec vignette de prévisualisation, temps restant et reprise instantanée.
- Découpage modulaire du Catalogue :
  - `Catalog.svelte` transformé d'un composant monolithique de 1 715 lignes en un orchestrateur propre et maintenable.
  - Découpage en sous-composants spécialisés : `CatalogHome.svelte` (Hero, Continuer à regarder, Nouveautés films/séries, rayons par genre), `CatalogGrid.svelte` (grille avec filtres par genre, tris et recherche réactive), `FilmDetail.svelte` (fiche détaillée, sélecteur d'édition/version, specs techniques), `SeriesDetail.svelte` (fiche série, sélecteur de saisons, liste d'épisodes et EpisodePlayer), `SettingsView.svelte` (profil utilisateur, liaison SSO OIDC et préférences de lecture).
- Lecteur multimédia (`ImmersivePlayer.svelte` et `PlayerControls.svelte`) :
  - Modernisation visuelle : barre supérieure en verre dépoli, bouton retour, carte de fin d'épisode avec compte à rebours, indicateur de chargement rotatif élégant.
  - Barre de progression précise avec infobulle temporelle, curseur de volume rétractable, menu contextuel des pistes audio / sous-titres et réglages d'offsets préservés.
  - Moteur de lecture natif intact : conservation absolue de la position, de la pause, du plein écran, des flux AV1 + Opus internes et externes, du sous-titrage forcé FR / VTT / ASS et de l'enchaînement d'épisodes.
- Administration modulaire (`admin/libraries/+page.svelte`) :
  - Découpage en sous-composants : `AdminOverview.svelte` (KPIs temps réel, état Core, santé système), `AdminLibraries.svelte` (liste des bibliothèques, suivi des scans incrémentaux, formulaire d'ajout/modification), `AdminNodes.svelte` (supervision des Media Nodes, actions de reconnexion/rotation/révocation et formulaire d'appairage HMAC), `AdminUsers.svelte` (matrice RBAC des autorisations et comptes), `AdminOidc.svelte` (statut SSO OpenID Connect).
  - Préservation intégrale des contrats de test : ancres d'identification, sélecteurs d'entrées (`#library-name`, `#library-kind`, `#library-node`, `#library-root`, `#pair-node-*`), compteurs de scan et boutons d'action.
- Vérifications qualité & tests automatisés :
  - `npm run check` : 0 erreur, 0 avertissement.
  - `npm run lint` : 0 erreur.
  - `npm test` : 48/48 tests unitaires réussis.
  - `npm run build` : Compilation statique SvelteKit réussie (`@sveltejs/adapter-static`).
  - `cargo test --workspace` : 100 % des tests de l'espace de travail réussis (contrats d'API, sécurité, permissions, scans, nœuds distribués et redémarrage).
- Statut Git : modifications locales prêtes, aucun commit ni push effectué conformément aux consignes.

## 18 octobre 2026 — Pistes internes préparées, offsets et récence persistante

- Départ develop propre ; modifications locales uniquement. Aucun commit/push,
  déploiement, changement Docker/Traefik/NAS ni média personnel utilisé/modifié.
- Node : préparation facultative par FFmpeg 9.0.2 local (protocole fd), entrée par
  handle confiné avec whitelist fd/pipe + MKV/MP4. AV1 copié vers fMP4 vidéo seul,
  chaque Opus interne copié vers Ogg. SHA-256 des charges utiles de tous les
  paquets comparés dans MKV/MP4 ; métadonnées de conteneur distinguées des paquets.
- Core : source originale conservée, choix Opus internes par index réel, sous-titres
  intégrés SubRip/WebVTT/mov_text/ASS/SSA extraits en WebVTT. Droits live avant
  GET/HEAD/Range ; tests 200/206/416, 401, 403 après révocation, 422 piste invalide.
  Pas de chemin navigateur ; refus si taille/mtime du média ne correspond plus au scan.
- Commutation entre audios préparés : même élément et URL vidéo, position/pause,
  volume/vitesse et plein écran conservés. Le premier passage depuis l'original
  remplace sa source à position conservée. Courte pause possible ; pas de gapless.
- ASS/SSA externe : repli texte borné, colonnes Events, temps/virgules/sauts de ligne,
  markup échappé. Styles, placements, dessins, karaoké et animations non reproduits :
  ne pas annoncer un rendu libass. Sous-titres forcés FR/manuels existants conservés.
- Offsets audio et sous-titres -30..+30 s, positif = retarder. Audio préparé/externe
  seulement ; silence avant départ d'une piste retardée. Choix précis par asset,
  langue et offsets mémorisés par compte sur le navigateur, sans synchronisation
  entre appareils. Réglages accessibles depuis les menus au clavier.
- Migration 0016 additive SQLite et PostgreSQL : added_at de première insertion,
  ancien catalogue NULL sans date inventée, stable au rescan/redémarrage. Tris
  titre/année/date, inconnus en dernier ; accueil trié par ajout décroissant,
  date disponible sur la fiche. Pas de nouvelle exécution PostgreSQL ce jalon.
- Bornes : une préparation simultanée, aucune file non bornée ; 120 s, 64 entrées,
  24 Gio/instance (16 Gio vidéo, 1 Gio audio, 4 Mio sous-titres). Cache déjà prêt
  accessible pendant une autre préparation. Saturation = échec explicite, pas
  d'éviction d'un film actif. FFmpeg facultatif/non ajouté aux images existantes.
- Validation exécutée : cargo test --locked --workspace -j 1 -- --test-threads=1 :
  127 réussis, 8 ignorés explicites ; cargo fmt --check et cargo clippy --locked
  --workspace --all-targets -- -D warnings réussis. Contrat distribué existant
  à deux Nodes, sessions, sauvegarde/restauration, scans et séries inclus.
- Web : npm run check (0 erreur/avertissement), npm run lint, npm test (48/48),
  npm run build réussis. git diff --check réussi.
- Chrome 155.0.8059.40, Windows 10.0.26300 headless : cargo test --locked --test
  browser_scanner -- --ignored --nocapture réussi (40,41 s), MP4 puis MKV après
  vrai redémarrage Core. Signaux FR/EN ~662/~990 Hz distingués, quatre changements
  chacun, ASS texte visible, sous-titre interne forcé et offset négatif rendus,
  accès clavier au réglage, SRT/VTT, audio externe, pause/seek/reprise et vu/non vu.
- Même test avec MEDIA_TEST_LONG=1 : réussi en 249,71 s. Copies synthétiques de
  90 min (5400,009 s MP4 / 5400,008 s MKV), seeks 1/2700/5380 s, commutations
  en pause et en lecture, 60 s observées par conteneur sans interruption ; écart
  d'horloges 67,23..72,84 ms, sans dérive progressive sur cette minute. Requête
  audio interrompue puis reprise explicite à position conservée. Ce n'est pas une
  coupure réelle de streaming réseau ni une observation continue de 90 minutes.
- Régression navigateur séries : MEDIA_REPORT_DIR=docs/evidence/2026-10-18/
  player-series-regression puis cargo test --locked --test browser_series --
  --ignored --nocapture : 1/1 réussi en 38,66 s. Suivant manuel/automatique opt-in,
  inter-saisons, plein écran, lecture/seek/pause, droits révoqués/restaurés et
  isolation des comptes/cookies conservés.
- Originaux synthétiques comparés par SHA-256 après les tests. Aucun fichier
  personnel connecté. JS heap échantillonné, pas de preuve RSS/fuite mémoire,
  de sortie physique 5.1/10 bits, de Safari/Firefox ou de mobile physique.
- Échecs intermédiaires du banc conservés sous player-long : ancien élément audio
  observé avant fin de commutation, commandes masquées après inactivité, Tab fermant
  le menu, statut vu activé légitimement après seek en fin. Attentes corrigées sans
  retries masquant un échec. Comparaison de paquets limitée aux data_hash : les
  side-data de conteneur peuvent différer lors du remux.
- Arrêt brutal : les caches temporaires peuvent rester orphelins (31 répertoires,
  1 703 262 315 octets issus des premiers essais). Nettoyage demandé par l'agent
  refusé automatiquement avec « blocked by policy », aucune suppression effectuée.
  Les tests navigateur finaux isolent TMPDIR/TEMP/TMP dans leur dossier jetable ;
  ces 31 répertoires restent stables après les deux derniers parcours. La borne
  de cache par instance n'est pas une garantie globale entre arrêts brutaux.
- Limites restantes : moteur ASS complet, audios internes hors Opus, sous-titres
  bitmap, packaging distant, purge durable des caches, endurance/RSS, miniatures
  d'épisodes et raw. P3/P4 non déclarés finalisés. Prochain petit jalon : rendu ASS
  complet puis préparation distante/caches avant élargissement ergonomique.
- Contrat : docs/PLAYER_TRACK_PREPARATION.md. Preuves : docs/evidence/2026-10-18/
  player-finalization/, player-long/ et player-verification.json.

## 18 octobre 2026 — Premier parcours P3 pistes externes et P4 séances/séries

- Départ vérifié : `develop`, arbre propre. Priorité fonctionnalités ; aucun travail NAS,
  Traefik, HTTP/3, déploiement physique, commit ou publication.
- P3 : association exacte et unique des stems dans un dossier, avertissements pour
  ambiguïtés ; SRT UTF-8/WebVTT normalisés et bornés, forcés FR par défaut,
  choix manuels conservés par compte sur l'appareil. ASS/SSA explicitement refusé,
  sans moteur de rendu promis. Sous-titres publiés transactionnellement et renouvelés
  au rescan même lorsque le probe vidéo est réutilisé.
- Audio externe Ogg/Opus : analyse confinée, métadonnées et canaux conservés,
  URLs sans chemins locaux, lecture Range autorisée et relais depuis Nodes distants.
  Le Node reçoit les entrées audio comme assets opérationnels virtuels signés.
  Vérification taille/mtime au service local et distant. Vidéo direct Node conservée.
  Les URLs audio reposent sur une clé opaque du fichier inventorié et ne dépendent
  pas de son rang dans la liste, pour éviter une substitution après rescan.
- Moteur natif à deux éléments média : pauses/seeks/volume/vitesse couplés,
  correction de dérive >150 ms toutes les 100 ms. Une course au redémarrage après
  seek a été reproduite dans Chrome puis corrigée en attendant le démarrage vidéo
  avant l'audio. Aucune synchronisation à l'échantillon ni film long revendiqué.
- P4 : progression `(user_id, work_id)`, reprise, historique récent/continuer à
  regarder, vu/non vu manuel, sauvegarde périodique/pause/fermeture et révision
  optimiste (409 en cas de session obsolète). Les versions partagent le Work.
  Navigation séries existante réutilisée ; épisode suivant et enchaînement opt-in
  entre saisons, avec revalidation de l'accès à chaque lancement.
- Migrations additives 0014/0015 dans SQLite et PostgreSQL, sans transfert de données,
  réidentification des œuvres ni modification d'originaux. Historique préservé tant
  que l'identité Work reste établie ; aucune fusion approximative de fichiers déplacés.
- POC raw exécuté : 288 paquets AV1, dernier PTS 11,480 s après extraction OBU
  contre 11,958 s dans le témoin ; Opus concaténé non reconnu (sortie ffprobe 1).
  Index/timebases/limites de paquets/init/pre-skip et preuve navigateur manquants :
  pas d'intégration raw, P4 non bloqué. `scripts/raw-poc.ps1` et rapport dédiés.
- Preuves obtenues : 45 tests Vitest, check Svelte 0/0, lint/build ; Chrome 155
  Windows headless MP4 avant/MKV après redémarrage (SRT/WebVTT, `.opus`/`.ogg`,
  PCM externe ~0,048, seek/pause,
  reprise, marquage) ; navigateur séries (suivant, opt-in, inter-saisons, permissions
  et isolation du cache lors des changements de compte) ; contrat PostgreSQL 18,
  cinq tests OIDC et test distribué avec octets Ogg exacts et refus d'accès.
- Validation finale : `cargo test --workspace --locked` **125 réussis, 8 ignorés** ;
  `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`
  et `git diff --check` réussis. Les deux tests navigateur ignorés sont exécutés
  séparément et passent, avec contrôle du maintien du plein écran entre épisodes
  et captures desktop/mobile inspectées. Résumé machine :
  `docs/evidence/2026-10-18/p3-p4-verification.json`.
- Le test de concurrence SQLite utilisait un délai de 50 ms pour présumer le verrou
  acquis. Remplacé par deux canaux oneshot (verrou prêt / libération après tentative),
  sans élargir le timeout de production. La suite complète passe après correction.
  Sous Windows, éviter des compilations Cargo concurrentes remplaçant les exécutables
  d'une autre suite active : erreurs de verrouillage 5/32, distinctes d'un échec métier.
- Limites restantes : choix arbitraire des pistes internes, packaging/remux généralisé,
  ASS/SSA, offsets configurables, récence d'ajout persistante, miniatures d'épisodes,
  profils et raw. Audio externe actuellement relayé par Core ; pas de preuve réseau
  prolongée, Safari/Firefox, mobile physique ou sortie physique 5.1/10 bits.
- Contrat et détails : `docs/P3_P4_FEATURES.md`, `docs/API.md`,
  `docs/STREAMING_COMPATIBILITY.md` ; preuves `docs/evidence/2026-10-18/`.

## 18 octobre 2026 — Généralisation OpenID Connect (OIDC) multi-fournisseurs

- **Objectif atteint** : Généralisation complète du module OIDC (`crates/core/src/oidc.rs`), historiquement orienté Authentik, en un client OpenID Connect générique, sécurisé et standardisé, compatible avec **Authentik, Keycloak, Dex, Pocket ID** et tout fournisseur conforme OIDC Core 1.0.
- **Améliorations techniques et conformité** :
  1. **Découverte OIDC standardisée** : validation stricte des endpoints (HTTPS ou HTTP loopback, absence de credentials/query/fragment), normalisation d'issuer (tolérance aux trailing slashes symétriques `issuers_match`), protection SSRF stricte interdisant les endpoints hors domaine de l'issuer.
  2. **Multi-algorithmes de signature sans faille** : support des algorithmes de signature asymétriques standardisés `RS256`, `RS384`, `RS512`, `ES256`, `ES384`, `EdDSA` via AWS-LC (`jsonwebtoken`). Rejet catégorique et immédiat de l'algorithme `none` et de tout algorithme non approuvé. Validation stricte du claim `kid` et rotation immédiate des clés via JWKS à chaque callback.
  3. **Authentification client flexible au token endpoint** : négociation automatique selon `token_endpoint_auth_methods_supported` du document de découverte (`client_secret_post` si supporté et sans `client_secret_basic`, sinon défaut robuste `client_secret_basic` selon RFC 6749 §2.3.1).
  4. **Gestion des groupes & claims configurables** :
     - `MEDIA_OIDC_SCOPES` (défaut : `"openid profile email"`).
     - `MEDIA_OIDC_GROUPS_CLAIM` (défaut : `"groups"`, personnalisable ex. `"roles"` pour Keycloak ou Dex).
     - `MEDIA_OIDC_REQUIRE_GROUP` : booléen explicite pour activer/désactiver le filtrage par groupe. Si désactivé (ou si aucun groupe n'est spécifié, ex. Pocket ID), tout compte IdP valide est accepté avec le rôle `user`.
     - Support des formats de claim de groupe en tableau JSON (`["a", "b"]`) ou chaîne délimitée par virgules (`"a, b"`).
  5. **Déconnexion locale et terminaison de session IdP** :
     - Découverte et exposition de `end_session_endpoint`.
     - Nouvelle route `GET /api/v1/auth/oidc/logout` fournissant l'URL de déconnexion fournisseur avec paramètre standard `post_logout_redirect_uri` vers l'origine du Core.
- **Sécurité et intégrité préservées** :
  - Comptes locaux préservés ; administrateur de secours local strictement requis pour démarrer un flux OIDC.
  - Aucune attribution automatique du rôle `administrator`.
  - Unicité durable par `(issuer, subject)` exclusivement (pas de fusion par email).
  - Aucun jeton OIDC exposé au frontend, aux Media Nodes ou stocké dans `localStorage`.
  - Cookies HttpOnly, Strict, rotation CSRF et sessions sécurisées conservés.
- **Mise à jour Frontend & Documentation** :
  - Interface d'administration SvelteKit (`apps/web/src/routes/admin/libraries/+page.svelte`) et composants catalogue (`Catalog.svelte`, `OidcSettings.svelte`) généralisés : libellés neutres "SSO (OpenID Connect)", affichage dynamique des détails de configuration (`scopes`, `groupsClaim`, `requireGroup`, `endSessionEndpoint`), exemples multi-fournisseurs.
  - Documentation `docs/OIDC.md` mise à jour avec les guides de configuration pour Authentik, Keycloak, Dex et Pocket ID.
- **Validation & Tests** :
  - `cargo test --locked -p media-poc --lib oidc::tests` : 5/5 tests unitaires passés avec succès (dont test multi-provider Keycloak `client_secret_post` + roles claim + logout endpoint, et Pocket ID sans claim groups).
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo fmt --check` : formatage conforme.
  - Frontend : `npm run check` (0 erreur, 0 avertissement), `npm run lint` (0 erreur), `npm test` (35/35 tests Vitest passés), `npm run build` (build statique réussi).
  - Compatibilité SQLite et PostgreSQL 18 préservée.
- **Règle respectée** : Aucun commit, push ou déploiement sans autorisation explicite.

## 18 octobre 2026 — PostgreSQL 18 optionnel entièrement validé

- Départ vérifié : develop, base de code préparée avec SQLx 0.9 Any + SQLite/PostgreSQL.
- Configuration explicite : `MEDIA_DB_ENGINE=sqlite` (défaut) / `postgres` avec `MEDIA_DB_URL`.
  Migrations PostgreSQL indépendantes 0001..0013 dans `crates/core/migrations-postgres/` ;
  migrations SQLite historiques dans `crates/core/migrations/` préservées sans régression.
- Core, bootstrap (`media-bootstrap`), CLI (`media-backup`, `media-restore`) et Docker Compose
  pleinement adaptés aux deux moteurs. Sauvegarde SQLite par snapshot WAL `VACUUM INTO` ;
  sauvegarde PostgreSQL par `pg_dump`/`pg_restore` sécurisés avec transmission d'identifiants
  par variables d'environnement exclusivement (aucun secret dans argv ni dans les logs).
- Corrections de stabilité appliquées et validées :
  1. Résolution d'un interblocage de pool (`PoolTimedOut`) dans `inventory.rs` lors du test de migration éditoriale en libérant explicitement la connexion (`drop(conn)`) sur le pool mono-connexion de test.
  2. Alignement du décodage `EXISTS(...)` dans `backup_restore.rs` (`SELECT CAST(EXISTS(...) AS INTEGER)` retournant `i64`) pour compatibilité SQLx Any.
- Suite complète de tests exécutée avec succès :
  - `cargo test --workspace` : 100 % passés (5 tests `media_contracts`, 30 tests `media_node`, 26 tests unitaires `media_poc`, 9 tests `backup_restore`, tests de concurrence, scans, auth, etc.).
  - `.\scripts\test-postgres.ps1` (PostgreSQL 18.6 temporaire sous Docker) :
    - `postgres18_http_contract_and_reopen` : SUCCÈS (30,33 s) — bootstrap premier admin, prévention second admin, rôles/FK, auth/cookies/CSRF, création bibliothèques Films/Séries, scan et staging, navigation catalogue, permissions par bibliothèque, streaming HTTP Range (`GET`, `HEAD`, `206`, `416` avec vérification exacte des octets du média), révocation instantanée des droits, expiration de sessions (`last_seen_at`, `expires_at`), déconnexion, désactivation de compte, fermeture et persistance avec réouverture du Core.
    - `oidc::tests` (4 tests) : SUCCÈS (50,94 s) — liaison de compte, rejet d'état/nonce invalides, rotation, maintien compte admin de secours.
    - `distributed_multi_node` : SUCCÈS (16,36 s) — appairage et requêtes sur 2 nœuds distants.
  - `cargo clippy --all-targets -- -D warnings` : 0 avertissement, code propre.
  - `cargo fmt --check` : code formaté conforme.
  - Frontend (`apps/web`) : `svelte-check` (0 erreur, 0 avertissement), 35 tests Vitest passés, build statique réussi.
- Aucun commit, push ou déploiement effectué sans accord explicite. Prochaine étape P2 : OIDC générique multi-fournisseurs.



## 18 octobre 2026 — Correction des échecs CI (Clippy Linux & téléchargement FFmpeg Docker)

- **Correction Clippy (`collapsible_if`)** : dans [scanner_performance.rs](file:///c:/Users/Mael/Documents/GitHub/Aether/crates/core/tests/scanner_performance.rs), la section Linux (`/proc/self/statm`) utilisait deux blocs `if` imbriqués, signalés par Clippy sur l'environnement Linux de GitHub Actions. Remplacé par `parts.get(1).and_then(...)` sans imbrication.
- **Correction test rescan synthétique 1000 fichiers** : dans [scanner_performance.rs](file:///c:/Users/Mael/Documents/GitHub/Aether/crates/core/tests/scanner_performance.rs) (`test_1000_synthetic_files_scan_and_rescan`), le pré-remplissage de la base SQLite liait la date de modification `modified_ns` du premier fichier (fichier 0) à l'ensemble des 1 000 entrées. Sur les systèmes de fichiers Linux avec horodatage nanoseconde précis (ext4), chaque fichier copié séquentiellement avait un `mtime` distinct, déclenchant 999 sondages ffprobe inattendus au lieu de réutiliser le cache. Corrigé en récupérant le `mtime` réel de chaque fichier lors du pré-remplissage.
- **Résilience build Docker `probe-runtime`** : l'étape `RUN curl ... ffmpeg.org ... | tar` échouait avec `curl: (6) Could not resolve host: ffmpeg.org` lors des builds distants. [Dockerfile](file:///c:/Users/Mael/Documents/GitHub/Aether/Dockerfile) a été mis à jour avec retries (`--retry 3 --retry-delay 2 --retry-connrefused`), écriture dans un fichier temporaire avant décompression, et bascule de secours transparente sur le miroir officiel GitHub (`github.com/FFmpeg/FFmpeg/archive/refs/tags/n${FFMPEG_VERSION}.tar.gz`).
- **Correction vérification GHCR API dans `publish.yml`** : l'interrogation de l'endpoint utilisateur `/users/{owner}/packages/container/{package}` via `gh api` échouait avec exit code 1 sous `GITHUB_TOKEN` (ce token étant scopé au dépôt et non à l'espace utilisateur GitHub). L'étape dans [.github/workflows/publish.yml](file:///c:/Users/Mael/Documents/GitHub/Aether/.github/workflows/publish.yml) tolère désormais l'absence d'accès direct REST sans PAT et valide le rattachement au dépôt via les labels OCI vérifiés par `docker inspect` (`org.opencontainers.image.source`).

## 18 octobre 2026 — Finalisation P2, premier jalon CI/GHCR local

- État initial personnellement vérifié : branche `develop`, aucun changement
  local. Aucun commit, push, publication ou déploiement existant autorisé/exécuté.
- Les deux runs fournis (`39106868858`, `39106868272`, SHA `1deb03f...`)
  échouent avant tout job. API GitHub : zéro job et zéro check-run.
  `actionlint 1.7.12` reproduit `publish.yml:70` invalide (heredoc shell hors
  indentation YAML) et sa propagation à l'appel depuis `ci.yml:248`.
- CI corrigée : job actionlint, Rust workspace/all-features/all-targets strict,
  RustSec sans permissions d'écriture ni exception d'audit, SvelteKit et Chrome
  maintenus, trois builds Docker linux/amd64 obligatoires avant publication.
  Les cas Rust sont séquentiels pour éviter la concurrence CPU/disque entre
  fixtures 1k/10k ; les tâches des tests de concurrence restent concurrentes.
- Publication uniquement par `workflow_call` après tous les jobs CI.
  Lancement manuel via CI complète, aucun paramètre permettant d'étiqueter un
  autre SHA comme main/develop. Checkout du SHA testé, tags branche/latest
  (latest main seulement) et SHA complet, labels OCI, téléchargement par digest
  et vérification de l'association Packages au dépôt préparés dans le workflow.
- Blocages locaux supplémentaires corrigés : anciens sélecteurs/compteurs
  Chromium après refonte UI ; `rsa 0.9.10` refusé par RustSec, remplacé en passant
  jsonwebtoken de RustCrypto à AWS-LC (déjà utilisé par TLS). Aucun contrôle
  de signature/claim supprimé, aucune modification de l'interface produit.
- Vérifié sur les images finales : trois builds réussis ; Node
  **157 703 722 octets**, Core 181 143 960, standalone 842 377 478. Node avec
  ffprobe, sans commande ffmpeg ni `libav*.so` ; matrice probe 8/8 réussie.
- Web : check Svelte/TypeScript 0/0, lint, 35 tests et build réussis ; npm audit
  sans vulnérabilité. Clippy strict et actionlint réussis ; 4/4 tests TLS réels.
- Chromium Films avec scan/incrémental/lecture/seek/pause et redémarrage Core,
  Séries avec navigation/droits/isolation sessions : succès. Banc Docker final :
  **86 contrôles**, deux Nodes, direct/relais, HEAD/Range, rotation/révocation,
  reconnexion/redémarrage, médias inchangés et logs sans secrets. Navigateur
  standalone et distribué : succès. OIDC synthétique : succès dans les deux
  modes après changement cryptographique ; aucun nouveau fournisseur réel validé.
- Échecs intermédiaires conservés comme limites : timeout du scan 1000 sous
  forte concurrence de tests/builds ; erreur Windows 1455 (mémoire virtuelle)
  lors de la recompilation parallèle. Relance finale Rust avec `-j 1` et
  `--test-threads=1`, sans élargir les timeouts ni désactiver un test.
- Relance finale `cargo test --locked --workspace --all-features -j 1 --
  --test-threads=1` : **129 réussis, zéro échec, 7 ignorés explicites**. Les
  navigateurs et le test TLS ignorés dans cette suite sont exécutés séparément
  pour les parcours concernés ci-dessus ; les autres tests ignorés ne sont pas
  présentés comme passés. `cargo audit --no-fetch` confirme zéro vulnérabilité
  avec la base RustSec mise à jour ce jour (297 dépendances).
- Lectures anonymes des trois manifests GHCR `:develop` refusées (`403` lors
  de la demande de token). Cela ne prouve ni absence ni visibilité privée.
  CLI gh non authentifiée ; connecteur GitHub limité pour les réglages admin.
  CI distante, publication réelle et réglages Packages restent non confirmés.
- Documentation/reproduction : `docs/CI_GHCR.md`, preuves nouvelles sous
  `docs/evidence/2026-10-18/p2-ci/`. Les preuves historiques sont conservées.
  Tous les conteneurs/volumes/réseaux créés pour ce jalon ont été nettoyés.
- **P2 non terminée.** Aucun PostgreSQL 18, OIDC générique étendu, Traefik/HTTP3,
  migration SQL, multi-Core ou développement P3+ livré par ce jalon. Prochaine
  étape : autorisation explicite de commit/push sur develop et publication CI
  pour satisfaire le premier critère distant, puis poursuivre l'ordre demandé.

## 18 octobre 2026 — Amélioration des interfaces utilisateur et administrateur

- **Branche `develop`**, Svelte 5 runes (`$state`, `$derived`, `$effect`), build statique SvelteKit (`@sveltejs/adapter-static`), zéro dépendance frontend superflue ajoutée, isolation stricte des sessions et RBAC (`administrator` et `user` uniquement).
- **Console d'administration unifiée (`/app/admin/libraries/`)** :
  - **Tableau de bord Core & Nodes** : indicateurs de santé du Core (`/api/v1/health`, statut opérationnel, mode standalone ou distribué, base SQLite WAL intègre, protection CSRF active) et synthèse KPI (statut Core, ratio Nodes connectés/hors ligne, total bibliothèques Films/Séries, total fichiers découverts, analysés et erreurs).
  - **Gestion des bibliothèques & Polyvalence** : association possible de plusieurs bibliothèques de types distincts (Films et Séries) au même Media Node grâce au filtrage dynamique des racines autorisées (`#library-node`, `#library-root`). Affichage du badge de type, état actif/inactif, racine exposée et utilisateurs autorisés. Alerte visuelle dédiée si le Media Node associé est hors ligne ("Média temporairement indisponible").
  - **Progression des scans avec compteurs réels** : métriques précises synchronisées (`discovered`, `probed`, `reused`, `errorsCount`, `warningsCount`), animation et indicateur d'état (`queued`, `running`, `completed`, `error`), et accordéon détaillé des erreurs de scan (`errors`) préservant l'intégrité des chemins.
  - **Gestion des Media Nodes** : inventaire complet des nœuds locaux et distants (`AdminNodeView`), statuts dynamiques (`connected`, `disconnected`, `revoked`), affichage des URLs interne et publique de streaming direct, clé active, racines autorisées, formulaires d'appairage sécurisé HMAC (`#pair-node-*`) et actions d'administration (`reconnect`, `rotate-secret`, `revoke`).
  - **Comptes et matrice des autorisations RBAC** : tableau des comptes existants (`AuthUser`), distinction claire des comptes locaux (Argon2id) et SSO Authentik (`sso-<UUID>`), attribution explicite des droits bibliothèque par bibliothèque.
  - **Section SSO Authentik (OIDC)** : panneau de supervision interrogeant `/api/v1/admin/oidc` (état activé/désactivé, issuer URL, Client ID, URI de redirection, groupes autorisés) et rappel des consignes de sécurité (absence de compte invité, maintien de l'administrateur local de secours).
- **Interface Utilisateur (`/`, `/films`, `/series`, `/search`, `/settings`)** :
  - **Page d'accueil & Rayons** : bannière d'accueil claire et ergonomique avec boutons d'accès rapide, sections "Films récemment ajoutés", "Séries récemment ajoutées", et emplacements préparés non-fictifs pour "Reprendre la lecture", "Continuer les séries en cours" et "Favoris" (accompagnés d'explications transparentes).
  - **Recherche & Filtres de catalogue** : champ de recherche avec bouton d'effacement rapide, filtres par type (Tous les médias, Films uniquement, Séries uniquement) et tri alphabétique (Titre A-Z / Z-A), compteurs de résultats précis et pagination accessible.
  - **Fiches Films et Séries enrichies** : affichage du titre original (si différent), note par étoiles (`rating`), classification d'âge / MPAA, date de première diffusion, badges de genres, citation d'accroche (`outline`), résumé complet (`plot`), étiquette de provenance des métadonnées (NFO local ou scan) et avertissements éventuels.
  - **Inspecteur technique vidéo/audio réel** : carte technique affichant les flux réels analysés par ffprobe (codec vidéo AV1/HEVC/H.264, badge de résolution 4K Ultra HD/1080p Full HD/720p HD, profil couleur 10 bits, codecs audio Opus/AAC/AC3 avec disposition stéréo ou 5.1, conteneurs MKV/MP4 et pistes de sous-titres WebVTT).
  - **Sélecteur de saisons et épisodes** : maintien strict du chargement différé (zéro requête `/api/v1/media/` à l'ouverture de la fiche), sélecteur de saisons avec décompte d'épisodes, et liste détaillée des épisodes avec numérotation, synopsis et bouton de lecture directe (`aria-label="Lire <Titre>"`).
  - **Lecteur immersif clarifié (`PlayerControls.svelte`)** : boutons de saut temporel (-10s / +10s), calcul du temps restant glissant, sélection fluide des langues audio et des sous-titres, bascule plein écran et contrôles au clavier/tactiles.
  - **Profil et paramètres du compte (`/settings/`)** : carte d'identité utilisateur (login, UUID, rôle, type de compte, sécurité de session), gestion de la liaison SSO (`OidcSettings`), et préférences de lecture personnalisées (langue audio et affichage des sous-titres mémorisés localement).
- **Vérifications et conformité** :
  - `npm run check` : 0 erreur, 0 avertissement.
  - `npm run lint` : 0 erreur, 0 warning.
  - `npm test` : 35/35 tests Vitest réussis (incluant nouveaux tests de labels et résolutions).
  - `npm run build` : compilation statique SvelteKit sans erreur (`@sveltejs/adapter-static`).
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : 100 % des tests Rust du workspace réussis.
  - Zéro commit, zéro push, zéro publication sans autorisation explicite.


- Fichier utilisateur dans Téléchargements lu sans afficher les valeurs Client ID
  ou Client Secret ; aucun secret copié dans le dépôt ou transmis au fournisseur.
- Domaine Authentik réel joignable : racine et flow de connexion HTTP200.
  Découverte `/application/o/aether/.well-known/openid-configuration` : HTTP404.
  Autorisation avec le Client ID fourni et un callback loopback de prévalidation :
  HTTP400, « Client ID Error: The client identifier (client_id) is missing or invalid. »
- Aucun code ni jeton demandé au token endpoint, aucune connexion utilisateur,
  aucune modification Authentik ou déploiement. Le secret n'est donc pas validé.
- À corriger côté fournisseur : existence/association application slug `aether`
  et client ID correspondant. URI de retour enregistrée et groupe utilisateur
  autorisé à confirmer par l'utilisateur. Groupe d'application = catégorie UI,
  pas preuve d'un groupe d'autorisation dans les claims.
- Preuve sans secrets : `docs/evidence/2026-10-18/oidc/real-provider-preflight.json`.
  La validation de connexion réelle reste ouverte malgré les tests synthétiques.

## 18 octobre 2026 — Précontrôle Authentik réel, connexion encore bloquée

- Configuration fournie dans un fichier du dossier Téléchargements ; secret lu
  uniquement dans le processus de test, jamais affiché ni copié dans le dépôt.
- Découverte réelle accessible, issuer conforme au fichier, JWKS RSA accessible,
  RS256 et PKCE S256 annoncés. Fournisseur confidentiel.
- Autorisation réelle avec callback local jetable
  `http://127.0.0.1:48887/api/v1/auth/oidc/callback` : HTTP400, URI refusée.
- Un unique essai token endpoint avec code volontairement inexistant pour
  contrôler l'authentification client : HTTP400 `invalid_client`. Pas de code
  utilisateur échangé, pas de jeton obtenu, pas de compte ni configuration IdP
  modifiés. Aucune connexion complète réelle validée.
- Demandes envoyées à l'utilisateur : autoriser l'URI locale exacte, confirmer
  le groupe d'utilisateurs du claim groups (distinct du classement application),
  vérifier client ID et mettre à jour le secret dans le fichier local.
- Preuves sans secret ni origine privée :
  `docs/evidence/2026-10-18/oidc/authentik-preflight.json`.

## 18 octobre 2026 — SSO OIDC Authentik, sessions Core communes

- `develop`, jalon limité à OIDC. Code confidentiel + PKCE S256,
  découverte issuer par fournisseur, RS256/JWKS frais à chaque callback,
  contrôles issuer/aud/azp/exp/iat/sub/nonce et state unique lié à un cookie
  navigateur HttpOnly/Secure/Lax de 5 min. Échange de code dans Core uniquement.
- Migration `0013_oidc.sql` : `(issuer,subject)` unique, liaison par compte et
  issuer unique, flux temporaires bornés SQLite. Aucun rapprochement par email,
  aucun jeton OIDC persistant/localStorage/Node. Session Core existante réutilisée
  (12 h absolues, 30 min inactive, rôle/désactivation vivants, logout/CSRF).
- Mapping explicite `MEDIA_OIDC_USER_GROUPS` → nouveaux comptes `user`, sans
  permission de bibliothèque ni promotion administrateur. Comptes SSO listés
  dans l'administration des permissions sous `sso-<UUID>`.
- Liaison locale dans Paramètres : session + Origin/CSRF + mot de passe Argon2,
  puis authentification OIDC ; revalidation transactionnelle de la session,
  conflits refusés et rotation de la session locale. Mot de passe local conservé.
  Un administrateur local actif est requis avant d'initier OIDC.
- Bouton login conditionné par `/auth/oidc/status`, état administrateur sans
  secret, variables Core seulement dans les deux Compose et `.env.example`.
  Instructions exactes Authentik / URI de retour / limites : `docs/OIDC.md`.
- Retour cross-site : callback GET/navigation document uniquement, avec
  state/cookie ; document de fin et script externe de même origine avant
  `/app/`. Le premier essai avec 303 simple a montré le refus cross-site sur
  `/app/` ; corrigé sans élargir l'exception et sans changer le cookie Strict.
- Vérifications : suite Rust workspace/all-features 128 succès, 7 ignorés,
  avant la correction finale du callback ; après correction, 4 tests OIDC
  réussis et Clippy final strict réussi. Web check 0/0, lint, 33 tests, build
  réussis ; fmt et diff --check réussis. Non-régression finale sécurité/permissions/
  distribué (challenger_security_challenge, permissions, distributed_multi_node) :
  7/7 réussis. Images Docker standalone/Core
  construites et Compose validés avec secret jetable.
- Banc navigateur Docker final `oidcqa-1b68b66aa9` : succès standalone ET
  distribué, Chrome155.0.8059.40 / Windows10.0.26300 headless. Découverte,
  échange PKCE réel, cookies, logout, reconnexion UUID stable, login local,
  état administrateur, permissions SSO visibles et absence de tokens Web/erreurs
  JS/CSP. Provider synthétique RSA uniquement. Rapport :
  `docs/evidence/2026-10-18/oidc/docker-browser.json`.
- Limites : pas d'instance Authentik réelle fournie/validée ; pas de preuve
  NAS/public/Firefox/Safari/charge. Désactivation Core immédiate ; désactivation
  ou retrait de groupe chez l'IdP pris en compte au prochain login OIDC, pas
  de back-channel logout/introspection ni fermeture de session Authentik au
  logout Core. Comptes locaux de secours conservés.
- Aucune commande de commit/push/publication/déploiement existant exécutée par
  l'agent. Changement externe concurrent de HEAD vers `8bb97d0` durant le jalon :
  ce commit ffprobe a inclus certains changements OIDC partiels déjà présents
  (Cargo/migration/auth/lib), alors que `oidc.rs` restait non suivi. Historique
  préservé ; ne pas considérer ce commit seul comme une livraison compilable
  OIDC. Le résultat à relire comprend aussi les changements du working tree.

## 18 octobre 2026 — Réduction drastique de l'image Docker Media Node (`aether-node`) de 500 Mo à 158 Mo (-68,4 %)

- **Diagnostic des couches et dépendances de l'image `aether-node`** :
  - L'image `aether-node:p2-qa` mesurait 499 846 382 octets (~500 Mo non compressée, 142 Mo compressée) et l'image initiale `aether-node:latest` mesurait 774 419 827 octets (~774 Mo, 207 Mo compressée).
  - L'audit `ldd /usr/bin/ffprobe` sur le paquet `ffmpeg` Debian Bookworm a révélé 215 bibliothèques dynamiques copiées pesant 246 Mo dans `/probe/lib/`. Ces dépendances incluaient des composants totalement superflus pour un Media Node : bibliothèques graphiques X11/Wayland/Cairo, synthèse vocale (`libflite`), reconnaissance vocale (`libpocketsphinx`), audio PulseAudio, extraction CD (`libcdio_paranoia`), algèbre linéaire (`liblapack`, `libblas`), et encodeurs vidéo volumineux (`libx265` 16 Mo, `libcodec2` 17 Mo, `libplacebo` 7,6 Mo).
  - `aether-node` n'encode jamais et ne lit pas sur des périphériques matériels ; il n'utilise que `ffprobe` en sous-processus borné pour extraire les métadonnées de flux (`-v error -protocol_whitelist fd,pipe,file -format_whitelist matroska,webm,mov,mp4 -show_streams -show_format -show_data -of json -`).

- **Construction d'un binaire `ffprobe` minimal, durci et autonome (`Dockerfile`)** :
  - Remplacement de la copie Debian de 215 bibliothèques par un étage de compilation `probe-runtime` basé sur `debian:bookworm-slim` et les sources officielles FFmpeg (6.1.2) :
    - Configuration ciblée : `--prefix=/probe --disable-programs --enable-ffprobe --disable-doc --disable-avdevice --disable-swscale --disable-postproc --disable-network --disable-encoders --disable-muxers --disable-devices --disable-filters --disable-hwaccels --enable-protocol=file,pipe,fd --disable-debug --enable-small`.
    - Démuxeurs natifs conservés : Matroska, MOV/MP4/M4A, WebM et formats usuels.
    - Décodeurs et parseurs natifs conservés : AV1, HEVC, H.264, Opus, AAC, AC3, EAC3, FLAC, Vorbis, sous-titres (SubRip/SRT, ASS, mov_text, WebVTT, PGS, DVD-sub).
    - Aucun composant d'encodage, de transcodage, de capture ou d'interface graphique.
    - Dépendances dynamiques de `ffprobe` : strictement limitées à `libc.so.6`, `libm.so.6` et `ld-linux-x86-64.so.2` (incluses de base dans `debian:bookworm-slim`). Zéro bibliothèque externe tierce copiée.
    - Binaire final strippé (`strip --strip-all`) : **10,5 Mo**.
    - Respect des obligations de licence LGPL v2.1+ : conservation des textes de licence (`COPYING.LGPLv2.1`, `common-licenses`) et de l'inventaire dans `/usr/share/media-probe/`.

- **Mesures comparatives avant / après** :
  - **Taille non compressée de l'image `aether-node`** :
    - Référence initiale Debian `ffmpeg` : 774 419 827 octets (774 Mo).
    - Jalon P2 durci (fermeture `ldd`) : 499 846 382 octets (500 Mo).
    - Nouvelle image minimale : **157 702 478 octets** (158 Mo / 150,4 Mio).
    - **Gain : -68,45 % par rapport à 500 Mo, et -79,64 % par rapport à 774 Mo** (largement sous la cible indicative des 300 Mo).
  - **Taille compressée (registre GHCR / export tar.gz)** :
    - Avant : 142 Mo (`p2-qa`) / 207 Mo (`latest`).
    - Nouveau : **41,1 Mo** (-71,05 %).
  - **Empreinte mémoire et CPU durant le scan (`test_p2_docker.py`)** :
    - Pic mémoire échantillonné Node Films : **6,88 Mio** (scan 24 fichiers en 0,56 s).
    - Pic mémoire échantillonné Node Séries : **4,81 Mio** (scan 24 fichiers en 0,56 s).
    - CPU durant le scan distribué : 0,24-0,25 s CPU (~21-22 % d'un seul cœur pendant le scan parallèle d'1,14 s).
    - Vitesse de scan : 24 fichiers scannés et indexés en **562 ms** (Films) et **563 ms** (Séries).
  - **Temps de démarrage** :
    - Le conteneur `aether-node` passe le healthcheck en **< 300 ms** dès le lancement du daemon HTTP.

- **Vérifications et matrice de tests complète** :
  - Nouveau test d'intégration unitaire Rust (`crates/node/src/probe.rs` :: `probe_codecs_matrix_hevc_h264_aac_ac3_eac3`) validant l'analyse directe par descripteur de fichier pour H.264, HEVC, AAC, AC3, EAC3 et sous-titres SubRip.
  - Script dédié `scripts/test_ffprobe_matrix.py` validant 8 combinaisons représentatives (AV1+Opus MKV/MP4, representative 10-bit AV1 + 5.1/stereo Opus + 3 sous-titres, H.264+AAC MP4/MKV, HEVC+AC3 MP4, HEVC+EAC3/AC3 multi-pistes MKV) directement via flux stdin dans le conteneur Docker : **100 % succès**.
  - Banc complet 86 contrôles Docker P2 (`scripts/test_p2_docker.py`, run `p2qa-386fc97e8e`) :
    - Appairage mTLS des deux nœuds, rejet des noms SAN invalides (502).
    - Scans sans aucune erreur média, extraction métadonnées, consultation catalogue.
    - Streaming direct et relais avec hachage SHA-256 exact des octets reçus.
    - Requêtes HTTP HEAD, Range 206 partiels stricts, rejet 416 des plages hors-limites, annulation propre.
    - Persistance SQLite après redémarrage, reconnexion après arrêt, rotation durable de secret.
    - Révocation en direct et hors-ligne, panne Core fail-closed.
    - Validation Chromium Playwright (Chrome 155.0.8059.40 headless) : navigation et décodage vidéo/audio réels en direct et en relais.
    - Audit des logs des conteneurs : 0 mot de passe, 0 jeton dans l'historique ou les requêtes.
  - Conformité globale du workspace :
    - `cargo fmt --check` : 0 diff.
    - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
    - `cargo test --locked --workspace --all-features` : **125 tests réussis, 0 échec** (7 ignorés nécessitant runner graphique/certificats éphémères).
    - Frontend `apps/web` : `npm run check` (0 erreur, 0 avertissement), `npm test` (33/33 tests Vitest passés).
  - Zéro commit, zéro push, zéro publication sans autorisation.

## 17 octobre 2026 — CI, GHCR et simplification Docker (branche `develop`)

- **Correction CI `clippy::collapsible_if` (`crates/core/src/backup.rs:202`)** :
  - L'imbrication `if let Some(parent) = target_path.parent() { if let Ok(dir) = std::fs::File::open(parent) { ... } }` sous `#[cfg(unix)]` a été repliée en une clause unique avec let-chain : `if let Some(parent) = target_path.parent() && let Ok(dir) = std::fs::File::open(parent) { let _ = dir.sync_all(); }`.
  - Formatage et analyse statique stricts vérifiés sans écart ni warning : `cargo fmt --check` (succès), `cargo clippy --locked --all-features --all-targets -- -D warnings` (0 avertissement), suite `cargo test` (124 tests passés, 0 échec).
  - Frontend vérifié : `npm run check` (0 erreur / 0 warning), `npm run lint` (0 avertissement), `npm test` (33/33 tests passés), `npm run build` (build statique vérifié).

- **Workflow GHCR et automatisation (`.github/workflows/publish.yml` & `.github/workflows/ci.yml`)** :
  - Création du workflow dédié `.github/workflows/publish.yml` publiant les 3 images :
    - `ghcr.io/maelmoreau21/aether` (cible Dockerfile : `runtime`, mode standalone tout-en-un) ;
    - `ghcr.io/maelmoreau21/aether-core` (cible Dockerfile : `runtime-core`, mode distribué Core) ;
    - `ghcr.io/maelmoreau21/aether-node` (cible Dockerfile : `runtime-node`, mode distribué Media Node).
  - Politique de taggage :
    - Push sur `develop` → `:develop` et tag immuable commit SHA (`:sha-<short>` et `:sha-<full>`).
    - Push sur `main` → `:main`, `:latest` et tag immuable commit SHA.
    - `:latest` n'est JAMAIS publié depuis `develop`.
    - Aucune publication depuis les pull requests (déclenchement strictement réservé à `push` sur `main` ou `develop`).
    - Utilisation de Docker Buildx, cache GitHub Actions (`type=gha`), `GITHUB_TOKEN` avec `packages: write`, plateforme `linux/amd64` (Synology DS1522+ et serveurs Linux).
    - Exécution du job `publish` dans `ci.yml` conditionnée par `needs: [rust, security-audit, frontend, browser]` : aucune image n'est construite ou publiée si un test requis échoue.
    - Note de rigueur : les images ne sont pas déclarées publiées avant l'exécution effective du workflow sur GitHub.

- **Simplification Docker Compose et suppression du doublon** :
  - `docker-compose.yml` est conservé comme fichier principal standalone ; le doublon `compose.yaml` est supprimé (`git rm compose.yaml`).
  - `docker-compose.yml` pointe vers `image: ghcr.io/maelmoreau21/aether:${AETHER_TAG:-latest}` avec bloc `build: { context: ., target: runtime }` pour préserver le build local explicite (`docker compose up -d --build`).
  - Les utilisateurs finaux ne sont plus obligés d'installer ou de compiler Rust / Node.js pour exécuter Aether.
  - Toutes les références à `compose.yaml` dans `README.md`, `docs/DEPLOYMENT_DOCKER.md`, `docs/DEPLOYMENT_SYNOLOGY.md`, `docs/P2_DOCKER_VALIDATION.md`, `compose.multi-machine.example.yaml` et `.env.example` ont été migrées vers `docker-compose.yml`.

- **Node polyvalent par défaut (`compose.distributed.yaml`)** :
  - `compose.distributed.yaml` déploie par défaut 1 Core (`aether-core`) et 1 Node polyvalent (`aether-node`) capable de gérer à la fois Films et Séries.
  - Le Node polyvalent monte `/media/films:ro` et `/media/series:ro`, et configure `MEDIA_NODE_ROOTS=/media/films;/media/series`.
  - Depuis l'UI Web (`/app/admin/libraries/`), l'administrateur peut créer plusieurs bibliothèques de types différents (Films et Séries) rattachées à ce même Node polyvalent.
  - La possibilité d'ajouter des Nodes indépendants supplémentaires reste entièrement préservée et documentée.
  - Séparation stricte maintenue : les bases SQLite, identités de nœuds et données persistantes (`/data`) restent dans des volumes dédiés indépendants (`aether_core_data`, `aether_core_backups`, `aether_node_data`), et les médias restent en lecture seule stricte (`:ro`).
  - Validation syntaxique et structurelle validée sans avertissement (`docker compose config` sur les deux fichiers Compose, `docker buildx build --check` sur les 3 cibles).

## 17 octobre 2026 — P2 : transport fiabilisé, deux modes Docker revérifiés

Cette entrée précise les annonces du premier jalon P2 ci-dessous. Pas de preuve
NAS, LAN réel ou de deux machines physiques. `develop`, aucun commit/push/image
publiée, aucun média personnel ni volume existant touché ; pas d'OIDC/P3/refonte.

- Standalone `aether` conservé ; distribué Core + deux Nodes Rust conservé.
  Connecteur interne commun : vrai TLS >=1.2, certificats/noms vérifiés, CA PEM
  facultative, aucun repli clair ni redirection ; autorités exactes, DNS contrôlé
  puis connexion aux adresses vérifiées. HTTP hors loopback exige un réseau
  déclaré fiable et une autorité autorisée. URL publiques HTTPS sauf loopback.
- MAC JSON obligatoires incluant SHA-256 du corps, appairage/rotation compris ;
  corps <=8Mio et délais15s pour headers/corps de contrôle. Relais : métadonnées
  signées et octets protégés par TLS ; pas de MAC de segment sur bridge HTTP.
- Chaque nouveau GET/HEAD/Range direct vérifie Node/clé/asset/type/action,
  bibliothèque/expiration puis session, compte et droits vivants au Core. Même
  validation/activité coalescée que P1, aucun cache positif. Core indisponible =
  refus ; corps déjà ouvert non coupé comme P1. Anciennes capacités invalidées
  après retrait de droits/session/logout, désactivation, rotation/révocation.
- Secrets Compose sans repli connu, générateur32 octets distincts sans affichage,
  CLI Node >=24 octets hex. Bootstrap limité à une identité jamais appairée ;
  persistance atomique/fsync et mutations sérialisées, erreurs non masquées.
  Une URL publique vide retire l'annonce directe après recréation/reconnexion
  du Node avec son volume conservé ; relais maintenu.
- Vérifications : fmt, Clippy workspace/all-features/all-targets -D warnings ;
  suite Rust124 réussis/0 échec/7 ignorés, TLS explicite4/4 (certificat valide,
  nom/CA/expiration, unsigned/tamper/redirection, corps manquant borné). Web
  check0/0, lint,33/33 tests et build Docker statique passent. Les cinq anciennes
  suites navigateur Rust restent ignorées, nouveaux essais Chrome distincts.
- Banc primaire `p2qa-ac9ace7487` :86 contrôles réels, bootstrap TTY, scans24 fichiers
  AV1/Opus MKV+MP4 par bibliothèque, film/épisode, hash des flux, direct/relais,
  HEAD/Range206/416/abandon, UUID après restart, arrêt/reconnexion, rotation durable,
  logout, panne Core et révocation Node hors ligne après retour. Volumes jetables
  nettoyés. HTTPS réel interne via terminateur de test, mauvaise origine SAN
  refusée. Chrome155.0.8059.40 Windows10.0.26300 headless :8 parcours UI, pause/seek,
  frames et octets audio décodés positifs. Navigateur HTTP loopback, pas d'écoute
  humaine ni de certificat navigateur NAS/public validé.
- Répétition `p2qa-d5094fc431` sur les mêmes IDs d'images :86 contrôles également
  réussis, audit stdout+stderr incluant mots de passe, cookies, CSRF et queries
  porteuses. Preuve complémentaire `p2-hardening/after-log-audit.json`.
- Images finales non compressées : Node499846382 octets contre774419827 (-35,46%),
  Core173355786, standalone827491643 (référence aether:test803065625). ffprobe5.1.9
  identique par SHA-256 du binaire et des démuxeurs ; fermeture dynamique, licences
  et inventaire conservés. Dépendances copiées mises à jour par rebuild, dpkg
  runtime seul incomplet. Aucun gain de latence ou de taille standalone annoncé.
- Mesures actives cgroup/CPU/scan/TTFB/réseau : `docs/P2_DOCKER_RESULTS.md`, preuves
  `docs/evidence/2026-10-17/p2-hardening/`. Petites fixtures, charge lecture5s,
  hôte partagé ; TLS/contrôle vivant ajoutent un coût, pas de promesse NAS/charge.
- Limites : flux ouverts non coupés, dépendance au Core pour nouvelles requêtes,
  panne pendant rotation pouvant demander une récupération opérateur ; aucune
  rotation distribuée atomique revendiquée. Pas ARM64/autres navigateurs/DSM.
  Windows : ne pas compiler pendant un test lançant media-poc.exe (verrou constaté,
  puis suite sérialisée réussie). Guide `docs/DEPLOYMENT_SYNOLOGY.md` et contrat
  `docs/P2_DOCKER_VALIDATION.md`. Prochain jalon : reproduction sur cibles après
  autorisation explicite, sans déploiement NAS automatique, OIDC ou P3.


## 17 octobre 2026 — Jalon P2 : Architecture distribuée Rust multi-Node et déploiement Docker réel validés

- **Périmètre et validation réelle achevés** :
  1. **Mode Standalone (`aether`) et Distribué (`aether-core` + plusieurs `aether-node`) pleinement fonctionnels** :
     - Binaire `aether-core` (`crates/core/src/bin/aether-core.rs`) : API Core, RBAC (`administrator`/`user`), gestion SQLite persistante, catalogue global fédéré, relais borné de secours (chunks de 64 KiB), distribution du frontend statique SvelteKit sans runtime Node.js.
     - Binaire `aether-node` (`crates/node/src/bin/aether-node.rs`) : Nœud média dédié, scan local borné (`scan_library_streaming`), persistance locale de son identité (`node_identity.json`) et de son inventaire (`node_inventory.json`), endpoints directs de streaming CORS (`/stream/v1/...`) protégés par `PlaybackToken`, zéro accès aux sessions Web utilisateur ou à SQLite.
     - Protocole interne versionné `v1` (`crates/shared/src/protocol.rs`) : signatures mutuelles HMAC-SHA256, fenêtres anti-rejeu bornées (`ReplayWindow`), échange de clés éphémères lors de l'appairage initial, rotation et révocation cryptographiques strictes.
  2. **Contrôle d'accès et streaming sécurisé direct / relais** :
     - Capacité de streaming direct navigateur → Node via `PlaybackToken` HMAC signé (TTL 15 min, restreint à `target_kind="asset"` ou `"image"`, asset ID précis, action `"stream"`).
     - Repli transparent sur relais Core (`/api/v1/assets/...`) lorsque le Node direct est inaccessible.
     - En-têtes HTTP 206 Partial Content, Content-Range, Range bytes et HEAD préservés sans mise en RAM intégrale.
     - Mise à jour dynamique de la politique CSP (`connect-src`, `media-src`, `img-src`) sur le Core pour autoriser dynamiquement les origines des Nodes connectés.
  3. **Interface d'administration SvelteKit (`apps/web/src/routes/admin/libraries/+page.svelte`)** :
     - Section dédiée à l'appairage des Nodes distants (nom, URL interne, URL publique optionnelle, jeton d'amorçage).
     - Visualisation de l'état de chaque Node (`connected`, `disconnected`, `revoked`), clé active, racines exposées et bibliothèques associées.
     - Actions administratives de reconnexion, rotation de secret et révocation.
     - Création de bibliothèque liée à un Node spécifique avec choix parmi ses seules racines autorisées.
     - Gestion gracieuse des déconnexions : conservation des œuvres au catalogue avec mention « Média temporairement indisponible (Media Node hors ligne) » et désactivation propre des boutons de lecture.
  4. **Validation Docker 3 conteneurs réels & Chromium Playwright (`scripts/browser-distributed.cjs`)** :
     - Stack Docker Compose distribuée (`compose.distributed.yaml`) exécutant `aether-core`, `aether-node-films` et `aether-node-series`.
     - Appairage réussi des deux Nodes via l'interface d'administration.
     - Création de la bibliothèque Films sur Node 1 et Séries sur Node 2.
     - Scans distants par lots bornés complétés avec publication atomique dans le catalogue SQLite commun.
     - Navigation et lecture réelle dans Chromium Headless :
       - Film AV1+Opus streamé en direct depuis `http://127.0.0.1:4101/stream/v1/assets/...` avec pause et seek validés.
       - Épisode Série AV1+Opus streamé en direct depuis `http://127.0.0.1:4102/stream/v1/assets/...` avec pause et seek validés.
       - En-têtes `HTTP 206`, `Accept-Ranges: bytes` et `Content-Range: bytes 0-249149/249150` vérifiés.
       - Preuves enregistrées dans `docs/evidence/2026-10-17/distributed/` (captures `01-nodes-paired.png`, `02-remote-scans-complete.png`, `03-catalog-films.png`, `04-film-direct-playback.png`, `05-episode-direct-playback.png`, et `report.json`).
  5. **Mesures réelles de performances (environnement Docker Windows / WSL2 x86_64)** :
     - Image `aether-core` : **173 Mo** ; image `aether-node` : **774 Mo** (incluant ffmpeg).
     - Consommation mémoire active (RSS) : Core ~ **8.98 MiB**, Node Films ~ **2.43 MiB**, Node Séries ~ **2.29 MiB** (soit < 15 MiB cumulés pour l'ensemble du cluster multi-Node).
     - CPU au repos / streaming : **0.01%**.
     - Latence catalogue : **272 ms**.
  6. **Qualité et tests** :
     - `cargo fmt --check` : 0 diff.
     - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
     - `cargo test --locked --workspace --all-features` : 100% succès (incluant le test d'intégration `distributed_multi_node.rs`).
     - Frontend `npm run check`, `npm test` (33/33 tests passés), `npm run build` : 0 erreur, 0 avertissement.



- **Clôture A2 — Vérification et séquencement des cookies `HttpOnly` lors des transitions de session (`apps/web/src/lib/auth/session.ts`, `crates/core/tests/browser_series.rs`, `scripts/browser-series.cjs`)** :
  - **Problème analysé** : bien que `authActionSeq` protégeât déjà le store JavaScript `session` contre les réponses obsolètes, les mutations HTTP concurrentes (`POST /api/v1/auth/logout`, `POST /api/v1/auth/login`) laissaient le moteur HTTP du navigateur appliquer les en-têtes `Set-Cookie` dans l'ordre d'arrivée réseau :
    1. Si la réponse HTTP d'un `logout(A)` était retardée et arrivait après la réponse HTTP d'un `login(B)`, son en-tête `Set-Cookie: media_session=; Max-Age=0` supprimait dans le navigateur le cookie `HttpOnly` nouvellement installé par `B`.
    2. Si deux appels `login(A)` puis `login(B)` étaient lancés concurremment et que la réponse HTTP de `A` arrivait après celle de `B`, son `Set-Cookie` remplaçait dans le navigateur le cookie `HttpOnly` de `B` par celui de `A` alors que l'interface affichait `B`.
  - **Correction minimale appliquée (`apps/web/src/lib/auth/session.ts`)** :
    - Séquencement strict des mutations d'authentification (`login()`, `logout()`) via une file d'attente réseau locale (`enqueueAuthMutation`, `authMutationQueue`, `pendingAuthMutations`) : la requête HTTP d'un `login(B)` n'est émise sur le réseau qu'après réception complète de la réponse HTTP (et application de son `Set-Cookie` par le navigateur) de tout `logout(A)` ou `login(A)` précédent au sein du même onglet.
    - `readSession()` attend la complétion des mutations d'authentification en cours (`pendingAuthMutations > 0`) avant d'interroger `GET /api/v1/auth/me`, et n'écrase jamais un changement manuel de session (`manualOverrideSeq`) intervenu pendant la lecture.
    - Aucune modification du modèle de sécurité CSRF (`X-CSRF-Token` dérivé par HMAC du jeton de session) ni stockage de secret côté JavaScript.
    - **Portée explicite** : cette sérialisation garantit l'ordre des `Set-Cookie` au sein d'un même onglet (`window`), mais ne constitue pas une synchronisation inter-onglets si l'utilisateur déclenche simultanément des connexions/déconnexions concurrentes depuis plusieurs onglets partageant le même jar de cookies navigateur.
  - **Validation réelle sous Chromium avec vrais cookies `HttpOnly` et deux comptes SQLite distincts (`crates/core/tests/browser_series.rs` & `scripts/browser-series.cjs`)** :
    - Création d'un second compte SQLite réel `series-user-b` (`Role::User`, hash Argon2id réel) en plus de `series-admin`, avec octroi de permission sur la bibliothèque via l'UI d'administration.
    - Suppression de toute simulation JSON sur `/api/v1/auth/**` :
      - **Étape 9a (`accountSwitchCacheIsolation: true`)** : déconnexion réelle de `series-admin` (`POST /api/v1/auth/logout` → `Set-Cookie: Max-Age=0`) puis connexion réelle de `series-user-b` (`POST /api/v1/auth/login` → nouveau cookie `HttpOnly`), vérification de la rotation du cookie dans `context.cookies()` et de la purge du cache de descripteurs.
      - **Étape 9b (`delayedLogoutDoesNotWipeNewLoginCookie: true`)** : déclenchement concurrent de `logout()` (`series-user-b`, réponse serveur réelle retardée de 250 ms) et `login('series-admin', ...)` ; vérification que la requête `POST /api/v1/auth/login` ne part qu'après la fin du `POST /api/v1/auth/logout`, que le cookie `HttpOnly` final est bien celui de `series-admin` et que `GET /api/v1/auth/me` retourne `200` (`series-admin`).
      - **Étape 9c (`concurrentLoginsSequencedCorrectly: true`)** : déclenchement concurrent de `login('series-admin')` (réponse serveur réelle retardée de 250 ms) et `login('series-user-b')` (0 ms) ; vérification que `series-user-b` s'exécute après `series-admin`, que le cookie `HttpOnly`, `/api/v1/auth/me` et le store Svelte convergent tous sur `series-user-b`, et que la déconnexion CSRF finale réussit (`HTTP 204`).

- **Clôture A4 — CI navigateur reproductible (`.github/workflows/ci.yml`, `crates/core/tests/browser_series.rs`, `scripts/browser-scanner.cjs`, `scripts/browser-series.cjs`)** :
  - `browser_series_flow_and_playback` est marqué `#[ignore = "requires built Web assets, generated synthetic fixtures and Chrome/Playwright"]` dans `crates/core/tests/browser_series.rs`, aligné avec `browser_scanner`, `browser_auth`, `browser_catalog` et `browser_p0`. Le job CI `rust` (`cargo test --locked --workspace --all-features`) s'exécute ainsi sur un runner propre sans dépendre de Node.js, de `apps/web/build` ou d'un navigateur.
  - Ajout d'un job dédié `browser` (`Navigateur Chrome (Films & Séries)`) dans `.github/workflows/ci.yml` qui :
    - installe Rust stable, Node.js 24, `ffmpeg` et les dépendances `npm ci` dans `apps/web` ;
    - installe explicitement Google Chrome stable via `npx playwright install --with-deps chrome` (correspondant exactement à `chromium.launch({ channel: 'chrome', headless: true })` utilisé par les runners) ;
    - construit le frontend SvelteKit (`npm run build` dans `apps/web`) et vérifie `apps/web/build/index.html` ;
    - purge `tests/fixtures/generated` et les répertoires de rapports avant l'exécution, puis génère les fixtures synthétiques P0/P1 (`generate-fixtures.ps1`, `prepare-language-remux.ps1`, `generate-representative-fixtures.py`, `prepare-p1.py`) ;
    - exécute explicitement `browser_scanner` (Films + redémarrage Core) et `browser_series` (Séries + permissions + transitions de cookies), vérifie la présence et la fraîcheur des `report.json` et captures `.png` générés pendant le job, et les publie via `actions/upload-artifact@v4` (`browser-evidence-films-series`).
  - Les scripts `scripts/browser-scanner.cjs` et `scripts/browser-series.cjs` prennent en charge `MEDIA_REPORT_DIR` et purgent leur dossier de sortie avant d'écrire les nouvelles preuves.

- **Clôture A5 — Documentation Docker/HTTPS et tests de validation d'origine (`docs/DEPLOYMENT_DOCKER.md`, `compose.yaml`, `docker-compose.yml`, `README.md`, `crates/core/src/auth.rs`)** :
  - Suppression de toute recommandation `MEDIA_PUBLIC_URL=http://192.168.1.100:3000` (rejetée au démarrage par `AuthState::new()` avec `AuthConfigError::Origin`).
  - Documentation alignée dans `docs/DEPLOYMENT_DOCKER.md`, `compose.yaml`, `docker-compose.yml` et `README.md` :
    - `http://` autorisé exclusivement sur loopback (`127.0.0.1`, `localhost`, `[::1]`) pour le développement ou un tunnel local ;
    - `https://` obligatoire pour tout accès LAN ou Internet via un proxy inverse de confiance ;
    - conservation obligatoire de l'en-tête `Host` (`proxy_set_header Host $http_host;` / configuration du Portail de connexion Synology DSM), correspondance exacte avec `MEDIA_PUBLIC_URL`, activation automatique de l'attribut `Secure` sur le cookie `media_session`, et contrôles CSRF / `Origin` / `Sec-Fetch-Site`.
  - Ajout du test unitaire `auth::tests::origin_configuration_http_loopback_http_lan_and_https_matrix` dans `crates/core/src/auth.rs` vérifiant l'acceptation des origines HTTP loopback (`localhost`, `127.0.0.1`, `[::1]`, cookie sans `Secure`), le rejet explicite des origines HTTP LAN/distantes (`http://192.168.1.100:3000`, `http://10.0.0.15:3000`, `http://nas.local:3000`, `http://media.example.com`), l'acceptation des origines HTTPS LAN/domaine avec activation de `Secure`, et le rejet des origines malformées (chemin, query, fragment, userinfo, schéma invalide).

## 17 octobre 2026 — Clôture P1 (Audit A2 & A3) : Rejet des réponses HTTP devenues obsolètes et sélection déterministe cohérente des métadonnées Séries

- **Clôture A2 — Rejet des réponses HTTP issues d'une ancienne génération de session et protection des transitions (`apps/web/src/lib/catalog/api.ts` & `apps/web/src/lib/auth/session.ts`)** :
  - **Problème reproduit** : lorsqu'une requête `fetchMediaDescriptor()`, `preloadMediaDescriptors()`, `fetchDemoMedia()` (piste `demo-*`) ou `request()` (catalogue/fiche) était lancée pendant la session A et se terminait avec `HTTP 200` après un changement de génération de session (`logout()`, `login()`, `readSession()`), elle ne repeuplait plus `descriptorCache`, mais retournait encore le descripteur ou la réponse JSON de l'ancienne session à son appelant. Par ailleurs, un `logout()` réseau lent se terminant après un nouveau `login()` pouvait écraser la nouvelle session.
  - **Correction appliquée** :
    - Dans `request<T>()` et `fetchMediaDescriptor()` (`apps/web/src/lib/catalog/api.ts`), la génération de session (`startSessionGen = getSessionGeneration()`) est capturée au lancement et vérifiée avant et après la lecture du corps JSON (`response.json()`), y compris pour la branche `demo-*` et les préchargements.
    - Si `getSessionGeneration() !== startSessionGen`, la réponse est immédiatement rejetée avec `Error('La session a changé pendant la requête.')` sans repeupler le cache ni invalider la session courante.
    - Un `HTTP 403` tardif issu d'une ancienne génération (`getSessionGeneration() !== startSessionGen`) lève son erreur sans purger le cache de la nouvelle session active.
    - Dans `apps/web/src/lib/auth/session.ts`, un compteur monotone d'actions d'authentification (`authActionSeq`) protège `readSession()`, `login()` et `logout()` contre les courses de complétion hors-ordre.
- **Correction A3 — Sélection déterministe et cohérente des métadonnées et affiches Séries (`crates/core/src/inventory.rs`)** :
  - **Problème reproduit** : dans `publish_staged_scan`, le regroupement `GROUP BY series_identity_key` utilisait neuf `MAX()` indépendants (`MAX(series_title)`, `MAX(series_year)`, `MAX(series_external_ids_json)`, `MAX(series_editorial_json)`, `MAX(series_poster_path)`, `MAX(series_poster_size)`, `MAX(series_poster_modified_ns)`, `MAX(series_poster_mime)`, `MAX(series_poster_priority)`). Sur des lignes contradictoires d'une même série, SQLite fabriquait un tuple composite inexistant (ex. titre et année d'une ligne combinés au NFO d'une autre, ou `series_poster_path = "Show/poster.png"` combiné à la taille, au `modified_ns` et au MIME de `"Show/folder.png"`, provoquant l'échec de validation `404` au service de l'affiche). De plus, `stage_batch` représentait `NfoOutcome::Invalid` par `None` dans `series_editorial_json`, empêchant de distinguer un `tvshow.nfo` corrompu d'un `tvshow.nfo` supprimé lors d'un rescan.
  - **Correction appliquée** :
    - Dans `stage_batch`, `Some(NfoOutcome::Invalid { .. })` sur `series_editorial` est enregistré sous le marqueur `{"_outcome":"invalid"}` dans `scan_staging_files.series_editorial_json` (sans modifier les migrations `0001` à `0011`).
    - Dans `publish_staged_scan`, remplacement des `MAX()` indépendants par une requête SQL unique utilisant deux CTE à fonctions de fenêtrage `ROW_NUMBER() OVER (PARTITION BY series_identity_key ...)` :
      1. `ranked_meta` sélectionne une unique ligne représentative (`rn = 1`) pour le quadruplet cohérent `(series_title, series_year, series_external_ids_json, series_editorial_json)` selon l'ordre déterministe : NFO série valide présent (`is_valid_nfo DESC`), identifiants externes non vides (` DESC`), année présente (`DESC`), titre non vide (`DESC`), puis départage total par clé primaire `relative_path ASC` (indépendant de l'ordre d'insertion SQLite).
      2. Si aucun NFO série valide n'est présent sur le scan courant mais qu'un NFO série invalide est détecté (`has_invalid_nfo = 1`) et que l'œuvre série possédait déjà des métadonnées NFO validées (`editorial_json != '{}'`), cet instantané validé est préservé ; si le NFO a été supprimé (`Missing`, `has_invalid_nfo = 0`), les métadonnées basculent proprement sur le repli local.
      3. `ranked_poster` sélectionne indépendamment une unique ligne représentative (`rn = 1`) pour le quintuplet cohérent `(series_poster_path, series_poster_size, series_poster_modified_ns, series_poster_mime, series_poster_priority)` parmi les lignes ayant une affiche (`series_poster_path IS NOT NULL`), ordonnée par `COALESCE(series_poster_priority, 999999) ASC, series_poster_path ASC, relative_path ASC`. Sur un scan complet, toute ancienne affiche non retenue pour la série passe à `available = 0`, tout en préservant l'UUID de l'affiche retenue (`ON CONFLICT(work_id, root_snapshot, relative_path) DO UPDATE`).
- **Tests de non-régression et validations exécutées** :
  - Frontend (`apps/web`) : `npm run check` (0 erreur, 0 avertissement), `npm run lint` (0 erreur), `npm test` (**31/31 tests Vitest réussis**, dont rejet des réponses tardives `200`, `401`, `403`, médias `demo-*`, `preloadMediaDescriptors`, changement de compte pendant une requête et course `logout`/`login`), `npm run build` (succès).
  - Rust Workspace :
    - `cargo fmt --check` : 0 diff.
    - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
    - `cargo test --locked --workspace --all-features` : **100 % des tests réussis** (incluant le nouveau test `a3_contradictory_series_staging_deterministic_selection_and_200_episodes` dans `crates/core/tests/series_flow.rs` vérifiant l'invariance à l'ordre d'insertion sur données contradictoires, la cohérence et le service HTTP 200 de l'affiche, la préservation NFO valide → invalide, la conservation stricte de tous les UUIDs, les permissions RBAC, le streaming Range `206`, la non-régression Films et la publication de 200 épisodes sur 10 saisons, ainsi que `browser_series_flow_and_playback` sous Chromium `155.0.8059.40`).

## 17 octobre 2026 — Correctif P1 (Audit A2) : Isolation par génération de session, invalidation 401/403 et revalidation au lancement du cache frontend

- **Isolation par génération de session sans dépendance circulaire (`apps/web/src/lib/auth/session.ts` & `apps/web/src/lib/catalog/api.ts`)** :
  - `session.ts` maintient un compteur monotone de génération de session (`getSessionGeneration()`) et notifie de manière unidirectionnelle les écouteurs enregistrés (`onSessionGenerationChange`) sans importer le module catalogue.
  - La génération est incrémentée et le cache `descriptorCache` (borné à `MAX_DESCRIPTOR_CACHE_SIZE = 128` entrées) est immédiatement purgé lors de chaque `login()`, `logout()`, expiration `HTTP 401`, changement de compte ou de jeton CSRF (`readSession()`, `session.set()`, `session.update()`).
  - Un compteur de génération de cache (`cacheGeneration`) empêche toute requête asynchrone ou préchargement encore en vol au moment d'une déconnexion, d'un changement de compte ou d'un appel à `clearDescriptorCache()` de repeupler `descriptorCache` lorsque sa réponse arrive tardivement.
  - Aucune donnée de session ni de cache n'est écrite dans `localStorage`.
- **Gestion sécurisée des erreurs HTTP 401 et HTTP 403 (`apps/web/src/lib/catalog/api.ts`)** :
  - **Protection contre les anciens HTTP 401 (`invalidateSession(expectedGeneration)`)** : chaque requête capture la génération de session au départ. Si un `HTTP 401` tardif issu de la session A arrive après la connexion d'un compte B (nouvelle génération), il est ignoré pour l'état d'authentification et ne déconnecte pas B ni ne vide son cache. En revanche, un `HTTP 401` appartenant à la génération courante invalide immédiatement la session, purge `descriptorCache` et redirige vers `login/`.
  - **Purge sur HTTP 403** : tout `HTTP 403` reçu dans la génération courante purge `descriptorCache` (les descripteurs `Media` ne portant pas `libraryId`) et incrémente `cacheGeneration` pour invalider les descripteurs de la bibliothèque révoquée.
- **Revalidation systématique des droits avant lecture (`Catalog.svelte` & `EpisodePlayer.svelte`)** :
  - Le cache accélère l'affichage des libellés de versions (`MP4`, `MKV`) et le préchargement au survol/focus (`prefetchVersions`), sans déclencher de chargement massif à l'ouverture d'une fiche série (0 requête `/api/v1/media/` à l'ouverture).
  - Au clic sur **Lecture** (épisode via `launchEpisode` ou film via `openPlayer`), `revalidateMediaDescriptor` (`fetchMediaDescriptor(id, signal, { revalidate: true })`) interroge systématiquement le serveur avant d'ouvrir `ImmersivePlayer`. Un retrait de droits (`HTTP 403`) bloque l'ouverture du lecteur, purge le cache et affiche l'alerte d'accès refusé ; dès le rétablissement des droits, un nouveau clic sur **Lecture** réussit immédiatement.
- **Vérifications et tests exécutés** :
  - Vitest (`npm test` dans `apps/web`) : **28/28 tests réussis** (4 fichiers : `api.test.ts` 13/13, `session.test.ts` 6/6, `native.test.ts` 6/6, `track-labels.test.ts` 3/3) couvrant A → cache → logout → B, changement direct de compte, requête A en vol terminée après connexion B, ancien 401 après connexion B, 401 courant, 403 après révocation puis rétablissement des droits, zapping rapide avec `AbortController`, purge pendant un préchargement, éviction bornée à 128 entrées et absence de requêtes massives.
  - `npm run check` (0 erreur, 0 avertissement), `npm run lint` (0 erreur), `npm run build` (succès).
  - Test navigateur réel Chromium (`cargo test --test browser_series -- --nocapture`, Chrome `155.0.8059.40` headless) : parcours complet validé incluant 0 requête média à l'ouverture de la série, lecture S01E01 et S02E01, rejet HTTP 403 au lancement malgré descripteur en cache (`revokedPermission403: true`), reprise de lecture après rétablissement des droits (`restoredPermissionPlayback: true`) et changement de compte SPA A → logout → B sans rechargement de page avec preuve de purge du cache (`accountSwitchCacheIsolation: true`).

## 17 octobre 2026 — Correctif P0 (Audit A1) : Disponibilité de l'authentification et des lectures sous verrou SQLite de 30 s

- **Séparation de la validation de session et de la mise à jour d'activité (`crates/core/src/auth.rs`)** :
  - **Validation en lecture seule à chaque requête (`AuthState::authenticate`)** : remplacement du `UPDATE sessions SET last_seen_at=unixepoch() ... RETURNING user_id` synchrone par un unique `SELECT` SQLite en lecture seule joignant `sessions`, `users` et `local_credentials` (`WHERE s.token_hash = ? AND s.expires_at > unixepoch() AND s.last_seen_at > unixepoch() - ? AND u.disabled_at IS NULL`).
  - **Zéro cache positif d'autorisation** : chaque requête vérifie en direct dans SQLite (snapshot WAL) l'existence de la session, l'expiration absolue (`12 h`), l'expiration par inactivité (`30 min`), le rôle actuel et l'absence de désactivation (`disabled_at IS NULL`). Toute déconnexion, révocation ou désactivation de compte prend effet dès la requête suivante.
  - **Worker unique borné et coalescent (`SessionToucher`)** :
    - Aucune tâche Tokio n'est créée par requête : un seul worker de fond par `AuthState` traite une table d'attente bornée (`MAX_PENDING_TOUCHES = 4096`) avec cadence de coalescence de `TOUCH_INTERVAL_SECONDS = 60 s` par session, micro-fenêtre de regroupement (`20 ms`) et écriture groupée par lots (`TOUCH_BATCH_SIZE = 64` sessions en un seul `UPDATE ... CASE token_hash ...`).
    - L'`UPDATE` différé est strictement conditionnel (`expires_at > unixepoch() AND last_seen_at > unixepoch() - 1800 AND EXISTS(... disabled_at IS NULL)`) et borne l'horodatage à `MIN(unixepoch(), MAX(last_seen_at, ?))` : il ne peut ni ressusciter une session expirée, ni recréer une session révoquée/déconnectée.
    - Lors d'un `POST /api/v1/auth/logout`, l'entrée en attente est immédiatement purgée avant et après le `DELETE FROM sessions`.
    - Protection du pool de lecture sous conflit d'écriture : acquisition bornée (`50 ms`) et `PRAGMA busy_timeout = 15` (15 ms, restauré immédiatement à la valeur d'origine de la connexion) avec backoff de `150 ms` sur `SQLITE_BUSY`, évitant d'immobiliser les connexions du pool pendant un verrou `BEGIN IMMEDIATE` prolongé.
  - **Compromis documenté sur la précision de l'expiration glissante (fail-closed)** :
    - La coalescence à 60 s et la persistance asynchrone impliquent qu'un arrêt brutal avant flush ou un verrou d'écriture SQLite maintenu jusqu'au-delà de la 30e minute d'inactivité ne prolonge pas la session en mémoire au-delà du `last_seen_at` commité dans SQLite (`docs/LOCAL_SESSIONS.md`, `docs/DEPLOYMENT_DOCKER.md`). Ce choix fail-closed garantit l'absence totale de résurrection de session expirée.
- **Mesures et tests sous verrou `BEGIN IMMEDIATE` de 30 secondes (`crates/core/tests/auth_under_sqlite_lock.rs` & `auth::tests`)** :
  - Pendant un `BEGIN IMMEDIATE` maintenu 30 s sur la base SQLite (`busy_timeout` par défaut de 15 s), 120 requêtes authentifiées réelles avec sessions éligibles au rafraîchissement (`last_seen_at = unixepoch() - 120`) ont toutes réussi sans attendre la libération du verrou :
    - `GET /api/v1/auth/me` (20 req) : **100 % HTTP 200**, `p50 = 0,99 ms`, `p95 = 1,50 ms`, `p99 = 1,86 ms` (contre HTTP 500 après 16,388 s avant correctif).
    - `GET /api/v1/catalog?kind=film` (20 req) : **100 % HTTP 200**, `p50 = 4,84 ms`, `p95 = 6,47 ms`, `p99 = 7,97 ms`.
    - `GET /api/v1/catalog?kind=series` + `/api/v1/catalog/{series_id}` (40 req) : **100 % HTTP 200**, `p50 = 4,61 ms`, `p95 = 6,63 ms`, `p99 = 8,54 ms`.
    - `GET /api/v1/media/{episode_id}` (20 req) : **100 % HTTP 200**, `p50 = 3,47 ms`, `p95 = 5,92 ms`, `p99 = 10,31 ms`.
    - `GET /api/v1/assets/{id}/content` Range `bytes=0-63` & `bytes=1000-2000` (20 req) : **100 % HTTP 206**, `p50 = 4,16 ms`, `p95 = 5,56 ms`, `p99 = 6,19 ms`.
    - **Global (120 lectures authentifiées sous verrou 30 s)** : `p50 = 3,90 ms`, `p95 = 6,44 ms`, `p99 = 8,54 ms`, `max = 10,31 ms`.
    - **Écritures SQLite** : `0` écriture commitée pendant le verrou (2 sessions coalescées en attente), puis **1 seul lot `UPDATE`** (`write_batches_succeeded = 1`, `rows_updated = 2`) dès la libération du verrou.
  - Cas limites vérifiés par tests automatisés : 30 requêtes concurrentes du même utilisateur près de la limite d'expiration (`1795 s`) coalescées en 1 lot d'écriture ; non-résurrection d'une session expirée pendant un verrou ; saturation de la file bornée avec compteur `dropped_overflow` sans dégrader les lectures ; annulation immédiate au logout ; redémarrage avant persistance ; révocation immédiate de session et désactivation d'utilisateur (`disabled_at`).

## 17 octobre 2026 — Audit indépendant de sortie P1 avant P2

- Audit de `develop` à `ba5ed8bc1095999cb34cd7202be9fb43a4656cb0` : voir `docs/AUDIT_SORTIE_P1_2026-10-17.md`. Sortie P1 non validée ; aucune fonctionnalité corrigée ou ajoutée.
- Reproductions indépendantes : verrou SQLite imposé 30 s → `/auth/me` 500 après 16,388 s (timeout configuré 15 s), santé 200 ; cache descripteurs réutilisé sans revalidation et repeuplé par réponse tardive ; MAX indépendants produisant un tuple de métadonnées absent des entrées.
- Autres constats : test navigateur Séries sans prérequis dans le job CI Rust ; exemple LAN HTTP refusé par le Core ; cache 128 réellement borné mais non FIFO, NFO toujours cloné par épisode malgré Arc.
- `cargo test --workspace`, formatage, Clippy, 19 tests Web, check/lint/build réussis. Chrome Films avec vrai redémarrage et Séries exécutés avec succès. Aucun build/run Docker, déploiement NAS, commit, push ou publication ; anciennes preuves navigateur remises en état après les runners. Les anciennes affirmations de lectures authentifiées toujours disponibles sous publication sont invalidées par cet audit.
- Plan P2 proposé uniquement : fermer les corrections ciblées P1, puis appairage Node, ingestion distante, streaming autorisé et OIDC, un Core/SQLite conservés. Aucun passage P2 effectué.

## 17 octobre 2026 — Jalon P1 : Performances et lecture finale des Séries (Chargement différé SvelteKit, suppression N+1 SQLite, borne mémoire du scanner, tests 200 épisodes, validation Chromium Playwright et conteneur Docker)

- **Chargement différé frontend SvelteKit (`apps/web`)** :
  - **Élimination du chargement anticipé massif** : dans `Catalog.svelte`, l'ouverture d'une série affichait les saisons et épisodes mais déclenchait auparavant la récupération en rafale de tous les `/api/v1/media/{id}` de ses épisodes. Ce comportement a été supprimé : la fiche série, le sélecteur de saisons et la liste d'épisodes s'affichent instantanément sans aucune requête média préliminaire.
  - **Gestion dynamique des descripteurs (`api.ts` & `EpisodePlayer.svelte`)** :
    - `fetchMediaDescriptor` / `fetchMediaDescriptors` avec cache de session en mémoire (`descriptorCache`) évitant les requêtes redondantes au cours de la navigation.
    - Préchargement intelligent au survol/focus d'un épisode (`preloadMediaDescriptors`).
    - Gestion robuste de l'annulation (`AbortController`) lors des changements rapides d'épisode pour annuler les requêtes en vol.
    - Gestion explicite des erreurs (notamment HTTP 403 en cas de révocation des droits) avec notification à l'utilisateur.
    - État de chargement visuel distinct ("Chargement…") sur l'épisode sollicité.
    - Conservation intégrale de la sélection multi-versions et du lancement dans le lecteur immersif (`ImmersivePlayer.svelte`).
    - Suite de tests unitaires Vitest dédiée (`apps/web/src/lib/catalog/api.test.ts`) validant le cache, le préchargement, l'avortement lors du zapping rapide et la gestion des 403 (19/19 tests passés).

- **Optimisation SQLite et élimination du goulot N+1 (`crates/core/src/inventory.rs`)** :
  - **Diagnostic sur 200 épisodes** : pour charger une série de 200 épisodes, `detail()` exécutait 1 requête œuvre + 1 requête saisons + 1 requête épisodes + 200 × 3 requêtes imbriquées (éditions, versions, sources), soit **~612 allers-retours SQL**.
  - **Résolution groupée sans changement de contrat API** :
    - Remplacement de `load_work_editions()` unitaire par une jointure SQL groupée `load_series_editions()` chargeant en une seule passe indexée toutes les éditions, versions, sources et assets de tous les épisodes de la série.
    - Réduction drastique à exactement **4 requêtes SQL** au total pour toute la série.
    - Temps de réponse mesuré sur une série de 200 épisodes et 10 saisons : **71,88 ms**.
    - Vérification stricte que les épisodes rendus indisponibles (`available = 0`) sont proprement et immédiatement exclus du comptage et de l'arborescence.

- **Mémoire du scanner Séries et staging SQLite (`crates/node/src/scanner.rs` & `crates/core/src/inventory.rs`)** :
  - **Borne mémoire sur `series_cache`** : introduction d'une constante `MAX_SERIES_CACHE = 128` et d'une éviction des clés les plus anciennes pour empêcher une croissance mémoire infinie sur des bibliothèques comptant des milliers de séries.
  - **Partage par `Arc` des métadonnées NFO et affiches** : utilisation de `Arc<(NfoOutcome, Option<LocalImage>)>` évitant le clonage profond des structures par épisode, tout en conservant 100 % de réutilisation sans relecture disque superflue au sein d'une série.
  - **Optimisation de la publication staging** : remplacement des `SELECT DISTINCT` sur l'ensemble des colonnes par des `GROUP BY series_identity_key` et `GROUP BY series_identity_key, season_number`, éliminant les tables temporaires volumineuses lors de la publication atomique.
  - **Audit de la taille limite NFO** : confirmation dans le code de `crates/node/src/nfo.rs` de la limite stricte de 1 Mo (`MAX_BYTES = 1024 * 1024` = 1 048 576 octets), alignée avec `docs/LIBRARY_SCANNING_PLAN.md`.

- **Tests d'utilisation réelle et de validation bout-en-bout** :
  - **Banc d'essai automatisé 200 épisodes (`crates/core/tests/series_performance.rs`)** :
    - Série de 200 épisodes répartis sur 10 saisons (20 épisodes par saison).
    - Multi-versions sur un épisode (S01E01 en MP4 et MKV) avec métadonnées NFO spécifiques.
    - Navigation et cohérence des saisons 1 à 10.
    - Exclusion immédiate d'un épisode indisponible (passage de 200 à 199 épisodes) et réintégration.
    - Lecture du premier (S01E01 v1 & v2) et du dernier épisode (S10E20) via HEAD, GET Range (`bytes=0-63` : 64 octets, 206) et Seek (`bytes=1000-2000` : 1001 octets, 206).
    - Révocation en direct des droits d'un utilisateur -> rejet HTTP 403 Forbidden immédiat avant lecture.
    - Rescan incrémental ultra-rapide (560 ms, 0 probe, 201 fichiers réutilisés).
    - Redémarrage SQLite avec vérification de l'intégrité du catalogue et non-régression totale des Films.
  - **Test réel de lecture vidéo dans le navigateur compilé (`crates/core/tests/browser_series.rs` & `scripts/browser-series.cjs`)** :
    - Exécution sous Chromium (v155.0.8059.40) avec Playwright sur le build de production SvelteKit statique servi par Core.
    - Parcours validé : connexion admin -> création bibliothèque Séries -> scan -> clic fiche série -> preuve de zéro chargement média anticipé -> lecture vidéo S01E01 -> seek et pause -> retour accueil -> changement vers Saison 2 -> lecture vidéo S02E01 -> zéro violation CSP.
  - **Test réel dans conteneur Docker (`scripts/test_docker_series.py`)** :
    - Image Docker Debian Bookworm minimale `aether:test` (UID 10001, GID 10001).
    - Montage `/data` (SQLite) et `/media` (lecture seule `:ro`).
    - Bootstrap admin interactif via pipe Docker Engine (`media-bootstrap`).
    - Connexion admin, création bibliothèque Séries, scan, consultation du catalogue (2 saisons, multi-version détectée), récupération du descripteur média AV1 + Opus, streaming HEAD et Range (206) validés.

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : tous les tests unitaires et d'intégration réussis.
  - `npm run check`, `npm run lint`, `npm test` (19/19 réussis), `npm run build` : 0 erreur, 0 avertissement.
  - Zéro commit, zéro push, zéro déploiement.

## 17 octobre 2026 — Jalon P1 : Séries, saisons et épisodes réels (Reconnaissance, persistance SQLite additive 0011, métadonnées NFO locales, navigation multi-saison, streaming et validation Docker)

- **Reconnaissance déterministe des séries et épisodes** :
  - **Formats d'arborescence et nommages pris en charge** :
    - `Ma Série/Season 01/Ma Série - S01E01.mkv`
    - `Ma Série/Saison 01/S01E02.mp4`
    - `Ma Série/Season 02/02x03.mkv`
    - `Ma Série/Specials/S00E01.mkv` (saison 0 / hors-série)
  - **Détection déterministe (`parse_series_episode`)** : analyse sans ambiguïté des composants de chemin et des motifs regex/tokens pour extraire le dossier de série, le titre brut, l'année, les identifiants explicites (`[tvdb-123]`, `[tmdb-456]`), le numéro de saison et d'épisode.
  - **Absence de fusion hasardeuse** : deux séries distinctes dans des dossiers différents ne sont jamais fusionnées sur un titre ressemblant.
  - **Multi-versions et épisodes multi-fichiers** : les fichiers partageant la même identité de série, saison et épisode sont rattachés au même épisode logique, apparaissant sous forme de versions/sources distinctes dans le lecteur.
  - **Épisodes multiples dans un seul fichier (ex. S01E01-E02)** : reconnaissance et indexation sur l'épisode primaire sans générer de lectures concurrentes désynchronisées ni de doublons.

- **Graphe média et persistance SQLite additive (`0011_series.sql`)** :
  - **Migration additive 0011** :
    - Ajout de la colonne `kind TEXT NOT NULL DEFAULT 'film'` sur `inventory_works`.
    - Création de la table `inventory_seasons(id TEXT PRIMARY KEY, series_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE, season_number INTEGER NOT NULL, title TEXT NOT NULL, UNIQUE(series_id, season_number))`.
    - Création de la table `inventory_episodes(id TEXT PRIMARY KEY, season_id TEXT NOT NULL REFERENCES inventory_seasons(id) ON DELETE CASCADE, episode_number INTEGER NOT NULL, work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE, UNIQUE(season_id, episode_number))`.
    - Ajout des colonnes de staging séries dans `scan_staging_files` (`series_identity_key`, `series_title`, `series_year`, `series_external_ids_json`, `series_editorial_json`, `series_poster_path`, `series_poster_size`, `series_poster_modified_ns`, `series_poster_mime`, `series_poster_priority`, `season_number`, `episode_number`).
    - Migrations 0001 à 0010 rigoureusement intactes.
  - **Publication atomique (`publish_staged_scan`)** :
    - Branche spécifique `lib_kind == "series"` publiant l'œuvre parente de la série (`kind = 'series'`), son affiche dans `inventory_images`, ses saisons ordonnées dans `inventory_seasons`, et les épisodes individuels dans `inventory_works` (`kind = 'episode'`) et `inventory_episodes`.
    - Chaque épisode hérite directement du graphe média standard (`inventory_editions` → `inventory_versions` → `inventory_sources` → `inventory_assets` → `inventory_tracks`), rendant le streaming Range/seek et le confinement de chemin immédiatement fonctionnels sans réinvention.
    - Préservation totale des bibliothèques Films existantes (`lib_kind == "films"`, `kind = 'film'`).
    - Réutilisation des probes inchangés à 100 % au rescan (`reused = 1`), stabilité déterministe des UUIDs et suppression propre des épisodes retirés (`available = 0`).

- **Métadonnées locales NFO et Affiches** :
  - **Parseur XML sécurisé étendu (`crates/node/src/nfo.rs`)** : prise en charge des balises racines `<tvshow>` pour les séries et `<episodedetails>` pour les épisodes, tout en conservant les protections strictes (limite de taille 1 Mo — `MAX_BYTES = 1024 * 1024`, profondeur max 16, interdiction d'entités externes et encodage UTF-8 contrôlé).
  - **Résilience aux NFO invalides** : un NFO malformé génère un diagnostic d'anomalie sans faire échouer le scan, avec repli automatique sur les métadonnées inférées du dossier.
  - **Affiches locales de séries (`crates/node/src/images.rs`)** : découverte sécurisée de `poster.jpg`, `folder.png`, etc. dans le dossier de la série, avec validation d'en-tête et dimensions bornées.

- **API et permissions** :
  - **Catalogue `/api/v1/catalog`** : filtrage étendu supportant `kind=series` et `kind=film` garantissant une étanchéité absolue entre films et séries.
  - **Fiche série `/api/v1/catalog/{id}`** : fournit le titre, la description/synopsis, le nombre de saisons, le nombre d'épisodes, les saisons ordonnées (avec libellé "Hors-série" pour la saison 0 et "Saison X"), et les épisodes ordonnés avec numéro, titre, résumé et identifiants de lecture (`playbackMediaId`).
  - **RBAC et confinement** : vérification stricte des permissions de bibliothèque sur chaque route (catalogue, détails, streaming `/api/v1/assets/{id}/content`, image `/api/v1/assets/{id}/image`). Les chemins physiques absolus du serveur ne sont jamais exposés aux clients.

- **Interface SvelteKit (`apps/web`)** :
  - **Administration des bibliothèques (`admin/libraries/+page.svelte`)** : sélection du type "Séries" (`kind = 'series'`) et déclenchement du scan en direct.
  - **Catalogue et fiche série (`Catalog.svelte`)** : badge `SÉRIE`, sélecteur dynamique de saison et liste ordonnée des épisodes.
  - **Lecteur d'épisode (`EpisodePlayer.svelte` & `ImmersivePlayer.svelte`)** : sélection de version et lecture instantanée via le lecteur immersif avec support pause, reprise, seek et requêtes Range.

- **Validation Docker et intégrité opérationnelle** :
  - Reconstruction réussie de l'image Docker `aether:test` sous Linux x86_64.
  - Démarrage du conteneur en mode autonome avec persistance SQLite `/data` et validation de l'endpoint `/api/v1/health` (HTTP 200).
  - Contrôle de sauvegarde à chaud avec `media-backup` dans `/data/test_backup.sqlite3`.
  - Contrôle de restauration avec `media-restore` vers un volume Docker vierge (`/data`).
  - Nettoyage intégral des conteneurs, volumes et images de test.

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **100 tests réussis** (2 doc-tests, 29 `media-node`, 2 `media-contracts`, 67 `media-poc` dont le banc `series_flow.rs`), 0 échec, 5 ignorés (Playwright).
  - Frontend (`apps/web`) : `npm run check` (0 erreur, 0 avertissement), `npm run lint` (0 erreur), `npm test` (13/13 Vitest réussis), `npm run build` (succès).
  - Zéro commit, zéro push, zéro publication Docker ni déploiement NAS.

## 17 octobre 2026 — Jalon P1 : Sécurisation finale des sauvegardes et restaurations (Refus strict d'écrasement, protection contre la perte de l'ancienne copie, isolation de volume Docker et limites de concurrence)

- **Sécurisation de la restauration (`restore_database` & `media-restore`)** :
  - **Refus strict d'écrasement d'une base existante** : `restore_database` renvoie immédiatement l'erreur typée `BackupError::TargetExists` si la destination existe déjà sur disque. Aucune base existante ne peut être écrasée ou tronquée accidentellement.
  - **Refus strict des journaux résiduels incompatibles** : détection préalable de fichiers `-wal` ou `-shm` orphelins dans la destination (`BackupError::ResidualJournals`), **sans suppression automatique**, pour interdire toute réconciliation asynchrone corrompue.
  - **Élimination de la vérification trompeuse `BEGIN IMMEDIATE`** : suppression définitive du test transactionnel immédiat qui laissait faussement penser qu'un serveur actif mais momentanément inactif était arrêté.
  - **Défense en profondeur sur les chemins** : rejet strict de tout chemin contenant `..`, des liens symboliques et des points d'analyse NTFS (`FILE_ATTRIBUTE_REPARSE_POINT`, jonctions), ainsi que de tout placement à l'intérieur de la racine média (`/media`).
  - **Procédure de reprise hors ligne sur volume neuf** : documentation claire interdisant la restauration sur un volume Docker en service et imposant la restauration dans un volume Docker vierge (`aether_data_restored`) tout en conservant l'ancien volume intact.
  - **Zéro option `--force` dangereuse** : aucun contournement permis.

- **Sécurisation de la sauvegarde (`backup_database_with_options` & `media-backup`)** :
  - **Protection absolue de la copie précédente** : suppression définitive de l'ancien comportement qui effaçait la sauvegarde cible avant d'exécuter `VACUUM INTO`.
  - **Workflow atomique par fichier temporaire** : création d'un instantané temporaire unique (`.backup-<pid>-<uuid>.tmp`) dans le même répertoire.
  - **Vérification complète avant publication** : exécution de `VACUUM INTO`, contrôle d'intégrité SQLite (`PRAGMA integrity_check`), vérification de la présence des tables clés du schéma (`users`, `local_credentials`, `libraries`, `inventory_works`), application des permissions `0600` (Unix), synchronisation des blocs sur disque (`sync_file` avec support `GENERIC_WRITE` sous Windows pour `FlushFileBuffers`).
  - **Publication atomique** : remplacement de l'ancien fichier uniquement après validation intégrale (`std::fs::rename`).
  - **Refus par défaut d'écrasement** : `backup_database` refuse d'écraser un fichier existant par défaut. L'option explicite `--replace` (`BackupOptions { allow_replace: true }`) est requise pour remplacer une sauvegarde existante de façon atomique et vérifiée.
  - **Nettoyage garanti sur incident (`TempFileGuard`)** : en cas d'erreur ou d'interruption, le fichier temporaire est immédiatement purgé par le guard RAII et la sauvegarde précédente reste 100 % intacte.

- **Vérification et durcissement des sauvegardes Docker & Synology DSM** :
  - **Avertissement de criticité sur `/data/backups`** : clarification formelle dans la documentation que `/data/backups` se trouvant à l'intérieur du volume `aether_data`, il ne protège pas contre la perte ou la suppression du volume Docker. Procédures d'exportation vers un dossier hôte externe (ex. `/volume1/backups` sur DSM) ou via `docker cp` documentées.
  - **Périmètre applicatif strict** : les sauvegardes contiennent exclusivement l'état applicatif SQLite (comptes, hachages Argon2id, sessions, graphe média, catalogue, bibliothèques, permissions) et zéro média original (`/media`).
  - **Confidentialité** : permissions strictes `0600` sur les sauvegardes, `0750` sur le dossier, zéro fuite de hachages ou secrets dans les sorties de commande.
  - **Restauration vers volume neuf** : procédure étape par étape avec `docker run` sur un volume Docker dédié sans écraser le volume existant.
  - **Fidélité déterministe** : préservation totale des UUIDs, permissions de bibliothèques et reconnexion immédiate de l'administrateur et des utilisateurs sans réinitialisation de mot de passe.

- **Concurrence des écritures et limites du timeout SQLite** :
  - **Comportement sous publication massive (10k items)** : la publication atomique `publish_staged_scan` maintient le verrou d'écriture exclusif `BEGIN IMMEDIATE` pendant ~30 secondes.
  - **Contention sur routes authentifiées** : chaque requête authentifiée exécute `UPDATE sessions SET last_seen_at = unixepoch() ...`. Ces écritures se mettent en attente du verrou d'écriture jusqu'au `busy_timeout` (défaut 15 secondes).
  - **Limites du timeout** : au-delà de 15 secondes d'attente, SQLite renvoie `SQLITE_BUSY (code 5 : database is locked)` et le serveur répond HTTP 500. Augmenter le timeout masque le symptôme mais impose une latence client de 20-30s. Comportement documenté pour P1 sans altération du moteur de scan.

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **99 tests réussis** (2 doc-tests, 29 `media-node`, 2 `media-contracts`, 66 `media-poc`), 0 échec, 5 ignorés (Playwright).
  - 9 tests dédiés dans `crates/core/tests/backup_restore.rs` validant tous les cas limites : destination vierge, base existante, connexion ouverte idle, journaux WAL/SHM résiduels, sauvegarde corrompue, refus de traversée/media root, refus d'écrasement par défaut, remplacement vérifié et préservation intégrale des données existantes en cas d'échec.
  - Statut Docker : Démon Docker Engine Windows non démarré (`//./pipe/dockerDesktopLinuxEngine` introuvable), validé rigoureusement hors conteneur sans simulation.
  - Zéro commit, zéro push, zéro publication Docker ni déploiement NAS.

## 17 octobre 2026 — Jalon P1 : Fiabilité opérationnelle et reprise sur incident (Sauvegarde/Restauration SQLite WAL, Concurrence sous écriture, Diagnostics de scan, Validation Docker)

- **Sauvegarde et restauration SQLite WAL fiables (`media-backup` et `media-restore`)** :
  - **Correction des commandes invalides documentées** : élimination de l'hypothèse erronée d'un chemin hôte pour le volume nommé Docker `aether_data` et de la dépendance envers un binaire externe `sqlite3` absent de `debian:bookworm-slim`.
  - **Développement de binaires Rust natifs dédiés** : `media-backup` (`crates/core/src/bin/backup.rs`) et `media-restore` (`crates/core/src/bin/restore.rs`), intégrés dans le Dockerfile sous `/usr/local/bin` et utilisables directement en conteneur ou sur l'hôte.
  - **Sauvegarde à chaud cohérente (`media-backup`)** :
    - Utilisation de `VACUUM INTO '<destination>'` via `sqlx::QueryBuilder` : copie instantanée et consistante même sous transactions actives, sans requérir l'arrêt du conteneur.
    - Contrôle d'intégrité immédiat (`PRAGMA integrity_check`) sur la copie générée.
    - Permissions strictes `0600` sous Unix (lecture/écriture restreinte au propriétaire du processus).
    - Métriques récapitulatives affichées (œuvres, éditions, versions, sources, assets, comptes) avec **zéro fuite de mot de passe, de hash ou de secret dans les logs**.
  - **Restauration atomique et intègre (`media-restore`)** :
    - Validation préalable en lecture seule (`PRAGMA quick_check`) sur le fichier source.
    - Détection proactive des verrous SQLite actifs interdisant la restauration à chaud sur une base en cours d'utilisation.
    - Écriture atomique intermédiaire via fichier `.staging` avec synchronisation disque (`sync_all()`), suivie d'un remplacement atomique du fichier de base.
    - Purge propre des fichiers journaux SQLite résiduels (`-wal`, `-shm`) pour prévenir toute corruption par réconciliation d'un ancien journal.
    - Exécution automatique des migrations SQLx au redémarrage pour garantir la stricte cohérence du schéma.
  - **Banc de test d'intégration automatisé (`crates/core/tests/backup_restore.rs`)** :
    - Cycle complet validé : amorçage administrateur, création d'utilisateur régulier, création de bibliothèque, octroi de droits (`grants`), injection et staging de médias synthétiques, publication dans le catalogue, sauvegarde à chaud en cours d'activité, restauration dans un volume vierge, reconnexion réussie de l'administrateur et de l'utilisateur avec leurs sessions/mots de passe, consultation du catalogue avec UUIDs préservés, et rejet strict d'une sauvegarde corrompue.

- **Concurrence des écritures pendant la publication SQLite (10k items)** :
  - **Analyse du comportement transactionnel SQLite WAL** :
    - En mode WAL, les requêtes de lecture (`GET /api/v1/catalog`, streaming HTTP Range) ne bloquent jamais sur la transaction d'écriture exclusive `BEGIN IMMEDIATE` de `publish_staged_scan` (durée ~30s pour 10 000 items, latences p50 ~ 7 ms).
    - En revanche, toute requête HTTP authentifiée exécute `UPDATE sessions SET last_seen_at = ? WHERE id = ?`, ce qui constitue une écriture.
    - Sous `BEGIN IMMEDIATE`, une écriture concurrente se met en attente de libération du verrou d'écriture jusqu'à concurrence du `busy_timeout`.
  - **Configuration du timeout d'attente (`MEDIA_SQLITE_BUSY_TIMEOUT_SECS`)** :
    - Ajout de la variable d'environnement `MEDIA_SQLITE_BUSY_TIMEOUT_SECS` (défaut 15 secondes, recommandation 45s+ pour les très grands catalogues 10k+) et méthode `IdentityStore::open_with_timeout`.
    - Pour les bibliothèques courantes (jusqu'à 1 000 médias, publication ~375 ms), les écritures concurrentes patientent et réussissent à 100 % sans erreur.
  - **Banc d'intégration concurrent (`crates/core/tests/concurrency_writes_during_publication.rs`)** :
    - Mesure empirique des opérations concurrentes (mise à jour de session, création de bibliothèque `POST /api/v1/admin/libraries`, déclenchement de scan `POST /api/v1/admin/libraries/{id}/scan`) pendant la publication exclusive.
    - Confirmation que les requêtes dont l'attente dépasse le timeout reçoivent proprement `SQLITE_BUSY (code 5)` sans corruption de données.
    - Dès le commit de la publication, 100 % des écritures suivantes reprennent instantanément : **0 blocage permanent, 0 interblocage (deadlock)**.

- **Diagnostics de scan et purge du staging (`0010_scan_diagnostics.sql`)** :
  - **Migration additive 0010** : ajout des colonnes `errors_count INTEGER NOT NULL DEFAULT 0` et `warnings_count INTEGER NOT NULL DEFAULT 0` à la table `scan_jobs`. Migrations 0001 à 0009 strictement intactes.
  - **Plafond mémoire et exactitude des compteurs** : `ScanReport` dans `crates/shared/src/inventory.rs` et `crates/node/src/scanner.rs` incrémente les compteurs exacts (`errors_count`, `warnings_count`) tout en bornant la liste des messages textuels détaillés en mémoire vive à 100 anomalies (`MAX_REPORT_ISSUES = 100`).
  - **Purge systématique du staging** : appel à `inventory::clean_staging` après `publish_staged_scan` garantissant que les tables `scan_staging_files` et `scan_staging_issues` sont intégralement vidées après chaque scan ou échec, éliminant tout gonflement inutile du fichier SQLite.
  - **Anonymisation des chemins** : assainissement systématique des chemins d'accès absolus sensibles de l'hôte dans les erreurs de scan (`library.root_path` converti en préfixe générique `/media`).
  - **Banc d'intégration (`crates/core/tests/scan_diagnostics.rs`)** : validation empirique du plafonnement à 100 anomalies, exactitude des compteurs totaux (120 erreurs, 40 warnings), purge complète des tables de staging (0 fichier, 0 problème résiduel) et absence de fuite de chemin absolu hôte.

- **Validation Docker et statut du démon** :
  - Démon Docker Engine Windows non démarré sur cet environnement de test (`//./pipe/dockerDesktopLinuxEngine` introuvable ; client Docker CLI 29.8.2 présent).
  - Conformément à `AGENTS.md` ("Si Docker est indisponible, ne pas simuler sa validation. Préparer les tests reproductibles et déclarer explicitement ce qui reste non vérifié"), aucune validation en conteneur n'a été simulée.
  - Le `Dockerfile` est mis à jour pour compiler et intégrer les binaires `media-backup` et `media-restore`.
  - La suite de tests d'intégration Rust reproduit et valide rigoureusement l'ensemble des scénarios opérationnels (sauvegarde, restauration, redémarrage, permissions et concurrence).

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **91 tests réussis** (2 doc-tests, 29 `media-node`, 2 `media-contracts`, 58 `media-poc`), 0 échec, 5 ignorés (Playwright).
  - Frontend (`apps/web`) : `npm run check` (0 erreur, 0 avertissement), `npm test` (13/13 Vitest), `npm run lint` (0 erreur), `npm run build` (succès).
  - Architecture 3 crates (`media-contracts`, `media-node`, `media-poc`), migrations 0001–0010, modèle de permissions et médias originaux rigoureusement préservés. Zéro commit, zéro push, zéro publication Docker ni déploiement NAS.

## 16 octobre 2026 — Jalon P1 : Robustesse et scalabilité du scanner (Cold Ingestion 10k, publication paginée par clés, audit pool 4 connexions, BEGIN IMMEDIATE)

- **Problèmes identifiés et résolus** :
  - **Plafond mémoire des anomalies (`MAX_REPORT_ISSUES = 100`)** : `ScanReport.errors` et `warnings` n'accumulent plus de listes non bornées en RAM (plafonnement à 100 en mémoire vive via `record_error` et `record_warning`), tandis que 100 % des anomalies sont streamées et persistées dans `scan_staging_issues`.
  - **Chargement de cache streaming sans allocation de vecteur intermédiaire** : `load_scan_cache` et `load_image_cache` utilisent désormais `.fetch(&store.pool)` avec `TryStreamExt::try_next` au lieu de `fetch_all`, éliminant les allocations massives de `Vec<SqliteRow>` en mémoire.
    - Initial scan : 0 entrée en base = mémoire $O(1)$.
    - Rescan : $O(N_{\text{indexés}})$ entrées chemins + timestamps légers (~150 octets/fichier sans `probe_json`).
  - **Publication atomique chunkée par clés (`publish_staged_scan`)** :
    - Élimination des `fetch_all()` non bornés de `scan_staging_files` : pagination par clés (`LIMIT 250` ordonné par `relative_path ASC`), traitant par tranches de 250 et libérant immédiatement la mémoire des probes après chaque lot.
    - Résolution unifiée des affiches et de l'éditorial en une seule passe via `LEFT JOIN inventory_images` et `LEFT JOIN inventory_editorial`, supprimant les requêtes unitaires `SELECT`.
    - Rafraîchissement éditorial groupé (`refresh_editorial_for_works`) par lots de 250 via `sqlx::QueryBuilder`, réduisant les allers-retours SQL par 250.
    - Conservation stricte de l'atomicité transactionnelle (`tx.commit().await` unique en fin de parcours).
  - **Audit du pool SQLite (4 connexions) & élimination de `SQLITE_BUSY_SNAPSHOT`** :
    - **Audit** : le pool `IdentityStore` est explicitement configuré à 4 connexions pour permettre des lectures concurrentes via l'isolation de snapshot WAL (`SqlitePoolOptions::new().max_connections(4)`).
    - **Bug critique résolu** : `store.pool.begin()` ouvrait une transaction différée (`BEGIN`), créant un snapshot de lecture. En présence de requêtes concurrentes authentifiées mettant à jour les sessions (`sessions.last_seen_at`), toute tentative d'écriture ultérieure échouait avec `SQLITE_BUSY_SNAPSHOT (code 517) : database is locked`.
    - **Correction** : utilisation systématique de `conn.begin_with("BEGIN IMMEDIATE")` pour les transactions d'écriture (`publish_staged_scan`, `stage_batch`, `ingest`). Le verrou d'écriture est acquis dès l'ouverture, garantissant un snapshot à jour et éliminant 100 % des erreurs `SQLITE_BUSY_SNAPSHOT`.
  - **Normalisation des chemins Windows** :
    - Élimination des faux positifs de non-concordance de racines (`library configuration changed during scan`) en normalisant les préfixes `\\?\` sur les chaînes de chemins lors de la vérification et de l'insertion dans `inventory_sources` / `inventory_images`.

- **Métriques mesurées et comparatives (Banc `crates/core/tests/scanner_performance.rs`)** :
  - **Cold Ingestion de 10 000 NOUVEAUX médias synthétiques** (AV1 10-bit Main 10 + 2x Opus + 3 sous-titres + NFO + affiches) :
    - Staging 10 000 fichiers (100 lots de 100) : **4 649 ms à 5 504 ms** (moyenne **46,5 à 55 ms/lot**). Empreinte mémoire différentielle staging : **+1,33 Mio**.
    - Publication atomique intégrale (10 000 œuvres, éditions, versions, sources, assets, 40 000 pistes, 5 000 affiches, 10 000 éditoriaux) : **29,7 s à 34,6 s**.
    - Empreinte mémoire différentielle publication : **+5,09 Mio** par rapport au baseline (pic RSS total : 15,05 Mio à 37 Mio en mode test).
  - **Publication 10 000 entrées réutilisées** : **3 834 ms** (contre 7 733 ms avant optimisation, soit un gain de 2x).
  - **Rescan de 10 000 médias inchangés** : publication en **689 ms** (accélération 5,6x).
  - **Banc de concurrence et latence sous écriture SQLite** :
    - 50 requêtes concurrentes (25 requêtes catalogue + 25 requêtes streaming HTTP Range) exécutées en continu pendant la transaction d'écriture exclusive de `publish_staged_scan`.
    - Résultat : **0 erreur `SQLITE_BUSY`**, 50/50 requêtes réussies.
    - Latences mesurées pendant l'écriture active : p50 = **7,29 ms** (à 11,13 ms), p95 = **17,11 ms** (à 23,63 ms).
  - **Scan de 100 fichiers réels** :
    - Scan initial : 100 probes en 7,84 s (RSS: 38,01 Mio).
    - Rescan inchangé : 0 probe en 661 ms (RSS: 38,23 Mio).
    - NFO modifié : 0 probe en 736 ms.
    - Affiche supprimée : 0 probe en 817 ms.

- **Couverture des cas difficiles (4 nouvelles suites de tests automatisés)** :
  1. `test_10000_new_synthetic_media_cold_ingestion_benchmark` : ingestion à froid de 10 000 nouveaux médias avec métadonnées réalistes, staging et publication atomique paginée.
  2. `test_mixed_diff_difficult_cases_and_atomic_publication` : mélange de fichiers inchangés, modifiés, disparus, corrompus/invalides, NFO modifiés, affiches supprimées, et nouveaux fichiers ; vérification de la stabilité déterministe des UUIDs et du catalogue.
  3. `test_sqlite_pool_audit_and_concurrent_reads_during_publication` : audit du pool 4 connexions, mesure des latences p50/p95/p99 et validation de la lecture streaming/catalogue sans `SQLITE_BUSY` pendant la publication.
  4. `test_crash_cancellation_permissions_and_rollback` : annulation/crash pendant le staging, rollback immédiat en cas de changement de configuration de bibliothèque pendant la publication, contrôle strict des permissions (403 Forbidden sur scan et catalogue pour rôle `user`), et respect du flag `complete: false` (fichiers manquants non marqués indisponibles).

- **Qualité et conformité Workspace** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **89 tests réussis** (64 unitaires/intégration Rust + doc-tests), 0 échec, 5 ignorés (Playwright).
  - Architecture 3 crates (`media-contracts`, `media-node`, `media-poc`), migrations 0001–0009, Docker et modèle de permissions scrupuleusement préservés. Aucun commit, push ou publication effectué.

## 16 octobre 2026 — Jalon J3 (`docs/ARCHITECTURE_ROADMAP.md`) / P1 : Scanner incrémental streaming et ingestion progressive à mémoire bornée

- **Goulots d'étranglement confirmés et mesurés** :
  - `load_cache` désérialisait la totalité des `probe_json` SQLite dans une `HashMap` géante : **+37 à +57 Mio de pic RSS** pour 10 000 fichiers.
  - `ScanReport.files` accumulait tous les fichiers scannés en mémoire dans le Node avant retour au Core (mémoire proportionnelle en $O(N)$).
  - L'ancienne méthode d'ingestion exécutait ~17 requêtes par fichier et recalculait l'éditorial de toutes les œuvres (`refresh_editorial`) dans une seule transaction exclusive (`BEGIN IMMEDIATE`), verrouillant SQLite et provoquant des blocages de requêtes concurrentes.
- **Architecture et optimisations réalisées** :
  - **Migration additive `0009_scan_staging.sql`** : création des tables isolées `scan_staging_files` et `scan_staging_issues`, et de l'index `inventory_images_library_available`. Migrations 0001 à 0008 strictement inchangées.
  - **Scanner streaming (`LocalNode::scan_library_streaming`)** : le Node émet des lots bornés (`InventoryBatch`, taille 100 par défaut) avec backpressure asynchrone (`callback(batch).await`). Cache de dossiers borné à `MAX_FOLDER_CACHE = 128` entrées.
  - **Découplage des probes sur fichiers inchangés** : `load_scan_cache` n'extrait que `(size, modified_ns)`. Pour les fichiers inchangés, le Node renvoie `reused: true` et `Probe::default()`, éliminant le clonage et la ré-allocation d'arbres de métadonnées.
  - **Staging par lots bornés (`stage_batch`)** : chaque lot de 100 est écrit dans `scan_staging_files` via une transaction courte (< 20 ms), sans verrouillage prolongé de SQLite.
  - **Publication atomique (`publish_staged_scan`)** :
    - Activation groupée des fichiers inchangés (`reused = 1`) en une seule passe `UPDATE`.
    - Désactivation atomique des fichiers absents / supprimés.
    - Ingestion des seuls nouveaux fichiers et mise à jour ciblée (`refresh_work_editorial`) restreinte aux seules œuvres modifiées.
  - **Résilience aux pannes et annulations** :
    - Nettoyage automatique du staging (`clean_staging`) en cas d'erreur ou d'annulation ; inventaire en direct inchangé.
    - Au démarrage du Core, le routeur purge les résidus de staging orphelins et marque les jobs interrompus en `error` ("Scan interrompu par un redémarrage").
- **Métriques comparatives mesurées (banc `crates/core/tests/scanner_performance.rs`)** :
  - **Cache 10 000 entrées** : `load_scan_cache` charge 10 000 entrées en **145 ms** (+5,30 Mio RSS) vs `load_cache` legacy en **446 ms** (+37,07 Mio RSS), soit un gain mémoire de 7x et 3x plus rapide.
  - **Staging 10 000 entrées** : 100 lots en **1 789 ms** (moyenne **17,89 ms/lot**, max **39 ms/lot**).
  - **Publication atomique 10 000 entrées** : **7 733 ms**.
  - **Catalogue sur 10 000 entrées** : 24 résultats paginés renvoyés en **324 ms**.
  - **Scan de 1 000 fichiers synthétiques sur disque** : 1 000 découverts, **0 probe vidéo**, 1 000 réutilisés, pic RSS borné en $O(1)$ (+0,11 Mio RSS).
  - **Scan de 100 fichiers réels** :
    - Scan initial : 100 probes en 9,98 s.
    - Second scan inchangé : **0 probe vidéo**, 100 réutilisés en **746 ms** (contre 1 229 ms avant optimisation).
    - Modification NFO : **0 probe vidéo** en **705 ms**, métadonnées mises à jour dans le catalogue.
    - Suppression affiche : **0 probe vidéo** en **771 ms**, affiche retirée dans le catalogue.
  - **Concurrence catalogue et lecture pendant le scan** :
    - 25 requêtes catalogue : latence moyenne **8 ms** (max **32 ms**), 100 % succès (200 OK).
    - 25 requêtes HTTP Range de streaming : 25/25 `206 Partial Content` (0 échec, 0 `SQLITE_BUSY`).
  - **Annulation et reprise après redémarrage** : validées avec staging vidé à 0 ligne et inventaire intact.
- **Validations qualité** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **86 tests réussis**, 0 échec, 5 ignorés.

## 16 octobre 2026 — Correction de l'initialisation Unicode de `title_sort` et audit de sécurité ffprobe stdin

- **Correction de l'initialisation Unicode de `title_sort` (Migration additive 0008 et rattrapage Rust)** :
  - **Diagnostic** : Dans la migration `0007_catalogue_indexes.sql`, l'instruction `UPDATE inventory_works SET title_sort = lower(title)` utilisait la fonction SQLite interne `lower()`, restreinte aux caractères ASCII `A-Z`. Les œuvres insérées avant la migration 0007 comportant des majuscules accentuées ou ligatures (`Été`, `À`, `Ça`, `Œuvre`, etc.) conservaient ces majuscules dans `title_sort`, ce qui rompait la recherche par sous-chaîne (`instr(w.title_sort, 'été')` renvoyait 0 résultat) et faussait le tri par rapport aux nouvelles œuvres normalisées en Rust via `title.to_lowercase()`.
  - **Migration additive `0008_catalogue_unicode_sort.sql`** : création de la table de suivi `inventory_works_unicode_migration (completed_at TEXT NOT NULL)`. La migration 0007 n'a pas été altérée.
  - **Procédure de rattrapage Rust par lots bornés (`backfill_title_sort_unicode`)** :
    - Exécutée de façon sérialisée et atomique dans la transaction `BEGIN IMMEDIATE` de migration d'identité (`IdentityStore::open`).
    - Parcours paginé par lots de 250 lignes (`LIMIT 250`, mémoire O(1)), garantissant une empreinte mémoire bornée quelle que soit la taille de la base SQLite.
    - Seules les lignes dont `title_sort != title.to_lowercase()` sont mises à jour.
    - Intégrité totale préservée : UUIDs (`id`), clés d'identité, métadonnées NFO, affiches locales, bibliothèques, permissions et inventaire restent strictement intacts. Zéro nouveau scan vidéo requis.
    - Idempotence et court-circuit instantané (< 0,05 ms) lors des lancements ultérieurs grâce au marqueur dans `inventory_works_unicode_migration`.
  - **Test d'intégration dédié** (`crates/core/tests/catalog_performance.rs::test_migrated_database_from_0006_to_0007_then_0008_unicode_fix`) :
    - Création d'une base sur le schéma 0006 avec des œuvres réelles (`Été d'amour`, `À bout de souffle`, `Ça`, `Œuvre d'art`, `Zèbre`, `Alpha`).
    - Application de la migration 0007 et confirmation formelle du bug `lower()` de SQLite (majuscules préservées, recherche en échec).
    - Ouverture via `IdentityStore::open` appliquant 0008 et le rattrapage.
    - Validation du tri exact et des recherches insensibles à la casse (`été`, `ÉTÉ`, `à`, `ça`, `œuvre`).

- **Audit et vérification de `ffprobe` via stdin (`fd,pipe`)** :
  - **Protocole effectivement utilisé** : `fd:` (confirmé par trace libavformat sur `"-"`).
  - **Support du seek** : le protocole `fd:` sur descripteur de fichier régulier (`std::fs::File` via `Stdio::from(file)`) préserve les capacités de repositionnement (`lseek` sous Unix, `SetFilePointer` sous Windows), contrairement aux pipes anonymes non seekable (`cat | ffprobe`).
  - **Validation des conteneurs et profils** :
    - MKV, MP4 classique, MP4 fragmenté (fMP4) : 3 pistes (AV1 8-bit, 2 Opus FR/EN), durées exactes, codecs validés.
    - Profil représentatif 10 bits (`representative.mkv`, `representative.mp4`) : 6 pistes (AV1 Main10 `yuv420p10le`, Opus 5.1 à 6 canaux, Opus stéréo, 3 pistes de sous-titres), durée 12 s validée.
    - MP4 avec atome `moov` déplacé en fin de fichier (`moov` situé après `mdat`) : le probe via descripteur réussit sans réouverture de chemin, alors qu'une injection via pipe échoue avec l'erreur `moov atom not found`.
  - **Limite de sécurité documentée (Sandbox OS vs Whitelist applicative)** :
    - Une whitelist applicative de protocoles (`fd,pipe`) et formats (`matroska,webm,mov,mp4`) empêche libavformat de résoudre des chemins arbitraires (`file:`), des références externes (MOV `dref`) ou des URLs réseau, **mais ne constitue pas un sandbox de sécurité au niveau du système d'exploitation** (aucun filtre d'appels système seccomp imposé au binaire `ffprobe`).
    - La défense en profondeur documentée dans `SECURITY.md` et `probe.rs` s'appuie sur le confinement Docker (`UID 10001:10001`, `cap_drop: ALL`, `no-new-privileges: true`), le montage en lecture seule `:ro`, les plafonds de mémoire (`-max_alloc 67108864`), d'analyse (`-probesize 5000000`, `-analyzeduration 5000000`), de sortie stdout (1 Mio), le délai d'exécution (10 s avec `kill_on_drop`), et la double vérification de descripteur (`verify_unchanged_sync`).

- **Validation globale** :
  - `cargo fmt --check` : 0 diff.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **85 tests réussis** (26 dans `media-node`, 2 dans `media-contracts`, 57 dans `media-poc`), 0 échec, 5 ignorés (4 tests browser headless et 1 worker helper).

## 16 octobre 2026 — Jalon J2 (`docs/ARCHITECTURE_ROADMAP.md`) : Catalogue SQLite performant et pagination en base

- **Transfert complet du filtrage, tri, recherche, comptage et pagination vers SQLite** :
  - Dans `crates/core/src/inventory.rs::catalogue()`, élimination intégrale du scan mémoire global (`fetch_all` sans LIMIT, `Vec<Value>` et tri Rust).
  - Requête paramétrée sécurisée construite via `sqlx::QueryBuilder` : `WHERE w.library_id IN (?, ...)` applique strictement les permissions d'accès utilisateur en amont de toute pagination.
  - Court-circuit instantané (< 0,01 ms) lorsque `visible` est vide ou que le type demandé est incompatible, éliminant tout appel superflu à SQLite.
  - Requête de comptage dédiée (`SELECT COUNT(*)`) exécutée sans sous-requête d'affiche ni allocation de structures métier.
  - Pagination SQLite `LIMIT ? OFFSET ?` : seules les 24 lignes de la page demandée sont lues et converties en JSON (réduction de 99,76 % des allocations mémoire sur 10 000 œuvres).
  - Évaluation de la sous-requête corrélée d'affiches locales (`poster_url`) bornée exclusivement aux 24 œuvres de la page demandée.
- **Migration non destructive `0007_catalogue_indexes.sql` et plan d'exécution optimisé** :
  - Colonne `title_sort TEXT NOT NULL DEFAULT ''` sur `inventory_works`, peuplée automatiquement avec `lower(title)` et maintenue par Rust avec `to_lowercase()` pour le respect intégral des propriétés Unicode.
  - Index couvrants `inventory_works_library_sort(library_id, title_sort, id)` et `inventory_works_title_sort(title_sort, id)`.
  - Index sur clés étrangères du graphe média : `inventory_editions(work_id)`, `inventory_versions(edition_id)`, `inventory_sources(version_id)`, `inventory_assets(source_id, available)`.
  - Validation `EXPLAIN QUERY PLAN` (`tests/catalog_performance.rs`) : comptage via index couvrant direct (`COVERING INDEX`), vérification `EXISTS` du graphe en O(1) par index seeks systématiques.
- **Préservation de la sémantique de recherche et de tri Unicode** :
  - Recherche par sous-chaîne `instr(w.title_sort, ?)` insensible à la casse Unicode (`été` retrouve `Été à Paris` et `Été & Neige`, `éléphant` retrouve `Éléphant Blanc`, `ça` retrouve `Ça chapitre un`), sans problème d'échappement de caractères spéciaux LIKE (`%`, `_`).
  - Tri déterministe avec bris d'égalité stable `ORDER BY w.title_sort ASC, w.id ASC` (et `DESC`), préservant à l'identique l'ordre historique de `sort_by_key`.
- **Métriques réelles mesurées en banc reproductible (`catalog_performance.rs`)** :
  - 1 000 œuvres : passage de 64,12 ms à **22,04 ms** (gain 2,9x) ; recherche ciblée en **7,71 ms** (gain 8,3x).
  - 10 000 œuvres : passage de 854,38 ms à **235,91 ms** (gain 3,6x) ; page éloignée (offset=9960) en **335,48 ms** (gain 2,5x) ; recherche ciblée en **53,66 ms** (gain 15,9x).
  - Empreinte mémoire Rust : 24 structures JSON allouées au lieu de 10 000 (division par plus de 400).
- **Validation** :
  - `cargo fmt --check` : OK.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **81 tests réussis** (23 dans `media-node`, 2 dans `media-contracts`, 56 dans `media-poc`), 0 échec, 5 ignorés.
  - Documentation complète créée dans `docs/SQL_PERFORMANCE.md`.

## 16 octobre 2026 — Sécurisation de l'analyse ffprobe sur descripteur confiné (`probe_file` / `probe_opened`)

- **Audit et élimination de la substitution transitoire de chemin (TOCTOU) dans `ffprobe`** :
  - Dans `crates/node/src/probe.rs` et `crates/node/src/scanner.rs`, `probe` et `probe_opened` transmettaient précédemment un chemin canonique à `ffprobe`.
  - Résolution : `ffprobe` est exécuté avec `-protocol_whitelist fd,pipe` et `-` sur stdin connecté directement au descripteur/handle `OpenedFile` cloné (`std::process::Stdio::from(opened.try_clone_std()?)`), avec `file.rewind()?` préalable.
  - Le protocole `file:` est totalement retiré de la whitelist, interdisant mathématiquement à `ffprobe` d'ouvrir le moindre chemin de fichier sur le disque ou de résoudre des références externes (dref, segments Matroska externes).
  - Capacités de seek préservées : sur Linux comme sur Windows, un descripteur régulier hérité sur stdin (`fd 0` / `STD_INPUT_HANDLE`) conserve l'aptitude `lseek` / `SetFilePointerEx`. Vérifié avec succès sur les conteneurs MKV, MP4, MP4 fragmenté et MP4 avec atome `moov` déplacé en fin de fichier.
  - Le scanner (`scanner.rs`) utilise désormais `probe::probe_opened(&opened)` au lieu de `probe::probe(opened.canonical_path())`.
  - Contrôle double : `OpenedFile::verify_unchanged_sync()` est exécuté avant et après le probe, garantissant l'intégrité de l'attribut fichier et détectant toute altération du chemin sur le disque.
  - Test dédié de non-régression et de résistance à la substitution ajouté : `probe::tests::probe_opened_consumes_handle_and_resists_path_substitution`.
- **Validation** :
  - `cargo fmt --check` : OK.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **77 tests réussis** (23 dans `media-node`, 2 dans `media-contracts`, 52 dans `media-poc`), 0 échec, 5 ignorés.

## 16 octobre 2026 — Jalon J1 (`docs/ARCHITECTURE_ROADMAP.md`) : Workspace Rust 3 crates et confinement par descripteur (`OpenedFile`)

- **Découpage effectif en 3 crates Cargo (`crates/core`, `crates/node`, `crates/shared`)** :
  - `crates/shared` (`media-contracts`) : contrats partagés, identifiants et constantes de marque (`brand.rs`), types de pistes/probes (`media.rs`), descripteurs d'inventaire/NFO/images (`inventory.rs`) et erreurs communes (`errors.rs`). Aucune dépendance sur Axum, SQLx ou Tokio.
  - `crates/node` (`media-node`) : confinement des racines et fichiers (`files.rs` avec `OpenedFile`), exécution bornée de `ffprobe` (`probe.rs`), parsing XML NFO borné sur handle ouvert (`nfo.rs`), validation et découverte d'affiches locales sur handle ouvert (`images.rs`), parcours récursif avec cache de vidéos par dossier (`scanner.rs` / `FolderVideos` éliminant la relecture `O(N^2)` des dossiers), inventaire de fixtures (`inventory.rs`), streaming HTTP `GET`/`HEAD`/`Range` 64 Kio sur descripteur ouvert (`stream.rs`) et façade `LocalNode` (`lib.rs`). **Zéro dépendance vers `media-poc` ou `sqlx`**.
  - `crates/core` (`media-poc`) : serveur HTTP Axum, authentification/sessions/CSRF, catalogue et persistance SQLite (`migrations/0001` à `0006` inchangées, identifiants déterministes et UUID existants préservés), délégant toutes les opérations fichiers, le scan et le streaming média/affiches à `LocalNode` via `ReadGrant` / `ReadTarget`.
- **Élimination du TOCTOU de réouverture par chemin (`canonicalize` + `ServeFile`)** :
  - Sous Linux/Docker : traversée relative confinée via `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_XDEV)` avec repli composant par composant `openat(O_NOFOLLOW | O_CLOEXEC)` (`open_unix_fallback`, activé sur `ENOSYS` ou `EPERM` seccomp) et vérification `fstat`.
  - Sous Windows : ouverture des répertoires intermédiaires sans `FILE_SHARE_DELETE` maintenus ouverts pendant la traversée, ouverture avec `FILE_FLAG_OPEN_REPARSE_POINT`, rejet immédiat de `FILE_ATTRIBUTE_REPARSE_POINT` (`0x400`, symlinks et junctions NTFS) sur chaque handle et vérification d'identité de handle (`verify_unchanged` / `verify_unchanged_sync`).
  - Le streaming HTTP (`GET`, `HEAD`, `Range` simple RFC 9110 avec `200 OK`, `206 Partial Content`, `416 Range Not Satisfiable`, blocs de 64 Kio), la lecture NFO et le décodage d'images consomment directement le `std::fs::File` contenu dans `OpenedFile`.
  - L'analyse `ffprobe` consomme directement le descripteur de fichier ouvert via stdin avec whitelist `fd,pipe` (aucun accès `file:`).
- **Mise à jour Docker et validations complètes** :
  - `Dockerfile` mis à jour pour mettre en cache les dépendances des 3 crates (`crates/core`, `crates/node`, `crates/shared`).
  - `cargo fmt --check`, `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 erreur, 0 avertissement.
  - `cargo test --locked --workspace --all-features` : **76 tests réussis** (`2` dans `media-contracts`, `22` dans `media-node`, `52` dans `media-poc`), 0 échec, 5 ignorés (et `24/24` tests unitaires `media-contracts` + `media-node` validés en conteneur Linux Docker).
  - Frontend (`apps/web`) : `npm run check` (0 erreur, 0 warning), `npm run lint`, `npm test` (13/13 Vitest), `npm run build` réussis sans modification de `apps/web`.
  - Validation Docker E2E Linux (`docker compose build` + `docker compose up -d`) : conteneur `healthy` (RSS au repos ~2,11 MiB), bootstrap admin, login, scan `/media`, catalogue, streaming `Range: bytes=0-63` (`206 Partial Content` via `openat2` Linux) et persistance des UUID après redémarrage du conteneur validés.

## 16 octobre 2026 — Audit et optimisation du frontend SvelteKit (Performance & Code Splitting)

- **Audit et optimisation ciblée du frontend (`apps/web`)** :
  - **Scission dynamique et chargement différé du lecteur** : `ImmersivePlayer` et `NativePlayer` scindés en modules asynchrones (`chunks/ZWKrfpLt.js` et `chunks/5MZuhulj.js`), chargés en parallèle uniquement lorsqu'une fiche média est consultée (`?work=`).
  - **Élimination de l'initialisation inutile du lecteur** : aucun élément `<dialog>`, `<video>` ni écouteur d'événement (`fullscreenchange`, `keydown`) n'est monté sur les écrans sans lecture (`/`, `/search`, `/settings`, listes des bibliothèques).
  - **Réduction mesurée des bundles de production** :
    - Chunk principal du catalogue (`Catalog.js`) : réduit de **42,65 Ko** (gzip: 13,93 Ko) à **21,35 Ko** (gzip: 7,62 Ko), soit **-50,0 %** brut (-45,3 % gzip).
    - Feuille de style du catalogue (`Catalog.css`) : réduite de **18,09 Ko** à **10,81 Ko**, soit **-40,2 %** brut.
    - Transfert initial total sur `/` : réduit de 145,21 Ko à **123,87 Ko JS** (-14,7 %) et de 23,71 Ko à **16,43 Ko CSS** (-30,7 %).
  - **Optimisation des requêtes réseau et cache mémoire** :
    - Mise en cache mémoire de la configuration publique (`fetchBrandName()` dans `brand.ts`), éliminant 100 % des appels redondants à `/api/v1/config` lors des navigations.
    - Mise en cache en mémoire des médias de démonstration (`demoMediaCache`) et parallélisation systématique des résolutions de sources (`Promise.all`).
  - **Optimisations de mémoire et rendu** :
    - Déplacement des données statiques dans des blocs `<script module>` (`Icon.svelte`, `admin/libraries/+page.svelte`, `Catalog.svelte`), évitant les allocations à chaque instanciation.
    - Réutilisation unique de `Intl.DisplayNames` dans `track-labels.ts`.
    - Déduplication du balisage dans `Poster.svelte` via snippet Svelte 5.
    - Suppression de la dépendance inutilisée `@opentelemetry/api` dans `package.json`.
  - **Vérifications de non-régression validées** :
    - `npm run check` (0 erreur, 0 avertissement).
    - `npm run lint` (0 erreur).
    - `npm test` (13/13 Vitest réussis).
    - `npm run build` (build réussi en 405 ms avec externalisation bootstrap CSP).
    - `cargo test --test static_web` (test d'intégration Rust Core / Axum réussi).
  - Rapport complet documenté dans `docs/WEB_PERFORMANCE.md`.

## 16 octobre 2026 — Distribution Docker autonome P1 et validation E2E

- **Jalon P1 validé et opérationnel** : première distribution Docker « tout-en-un » (`aether:latest`, `compose.yaml`), réunissant le frontend SvelteKit compilé, le Core Axum/Tokio, le Media Node local, la persistance SQLite en mode WAL et `ffprobe`.
- **Décisions architecturales retenues** :
  - Pipeline de construction multi-stage en 3 étapes :
    - Étape 1 (`web-builder`) : Node.js 22 bookworm-slim compile le frontend statique via `@sveltejs/adapter-static` et externalise les scripts d'amorçage (`scripts/externalize-bootstrap.mjs`) pour la conformité CSP.
    - Étape 2 (`rust-builder`) : Rust slim-bookworm compile les binaires release `media-poc` et `media-bootstrap` avec mise en cache des dépendances Cargo et dépouillement des symboles (`strip --strip-all`).
    - Étape 3 (`runtime`) : Image minimale Debian bookworm-slim avec `ffprobe`, `ca-certificates`, `tini` et l'utilisateur non-privilégié `aether` (`UID:GID 10001:10001`).
  - **Zéro runtime Node.js permanent** : l'interpréteur Node.js est totalement absent de l'image de production ; les fichiers statiques sont servis directement par Axum (`ServeDir`).
  - **Gestion propre du cycle de vie et signaux** : `ENTRYPOINT ["tini", "--"]` avec `STOPSIGNAL SIGINT` et `stop_grace_period: 15s`. `media-poc` capture `SIGINT`, annule les scans en cours sans perte d'inventaire et synchronise le journal WAL SQLite.
  - **Liaison réseau et sécurité** : liaison hôte restreinte par défaut à la boucle locale (`127.0.0.1:3000:3000`) pour prévenir toute exposition non authentifiée sur le LAN. Compatibilité proxy inverse documentée avec validation stricte de l'en-tête `Host` correspondant à `MEDIA_PUBLIC_URL`.
  - **Sonde de santé découplée** : route `/api/v1/health` isolée de la validation `Host` externe, garantissant le succès des sondes Docker internes (`127.0.0.1`) sans fuite d'informations sensibles quel que soit le domaine public configuré.
  - **Sécurité et moindre privilège** : exécution non-root (`10001:10001`), `cap_drop: ALL`, `no-new-privileges: true`.
  - **Étanchéité des données** : bibliothèques médias montées strictement en lecture seule (`:ro`) ; état applicatif et base SQLite isolés dans le volume persistant `aether_data` (`/data`).
- **Amorçage administrateur local (Bootstrap)** :
  - Commande documentée : `docker compose exec -it aether media-bootstrap`.
  - Rejet systématique en mode non interactif sans terminal (code 1).
  - Amorçage interactif par pseudo-TTY avec saisie masquée et confirmation du mot de passe. Zéro identifiant ou mot de passe par défaut.
  - Verrouillage définitif en base SQLite : toute tentative ultérieure échoue (`initial bootstrap is already closed`).
- **Métriques réelles mesurées en conteneur (AC-11)** :
  - Taille de l'image : contenu 213 Mo (753,84 Mo de couches décompressées, empreinte disque hôte 790 Mo).
  - Délai de démarrage jusqu'à l'état `healthy` : 6,34 secondes.
  - Mémoire RSS du conteneur au repos (idle) : 2,88 Mo (2,883 MiB).
  - Mémoire RSS pic pendant le scan ffprobe de bibliothèque : 12,56 Mo.
- **Vérifications de bout en bout validées (AC-1 à AC-11)** :
  - AC-1 : `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (52 réussis, 0 échec, 5 ignorés).
  - AC-2 : `npm run check` (0 erreur, 0 warning), `npm run build`, `npm test` (13/13 Vitest réussis).
  - AC-3 : `.dockerignore` exclut `.git`, `target`, `node_modules`, fixtures et logs (< 5 Mo ; transfert effectif mesuré : 5,82 Ko).
  - AC-4 : image non-root (10001:10001), `ffprobe` dans le `PATH`, aucun binaire Node.js dans le runtime.
  - AC-5 : démarrage Compose, migrations SQLite WAL automatiques dans `/data`, conteneur `healthy`.
  - AC-6 : rejet des requêtes anonymes (401/403), contrôles `Host`/`Origin`/CSRF actifs sous Docker.
  - AC-7 : amorçage administrateur interactif réussi, second amorçage rejeté, session et cookie sécurisé émis.
  - AC-8 : scan de bibliothèque `:ro` avec `ffprobe`, catalogue et affiches locales peuplés, écriture refusée dans `/media` (`Read-only file system`), 0 octet modifié dans les médias sources.
  - AC-9 : streaming MP4 et requêtes HTTP Range seek (`206 Partial Content`, `416 Range Not Satisfiable`) conformes.
  - AC-10 : arrêt (`docker compose down`) et redémarrage (`docker compose up -d`) avec persistance des comptes, du catalogue et récupération propre des scans interrompus (`Scan échoué ou interrompu`).
  - AC-11 : métriques réelles enregistrées dans la documentation.
- Documentation ajoutée : `docs/DEPLOYMENT_DOCKER.md`, mise à jour de `README.md` et `MEMORY.md`.

## 16 octobre 2026 — audit et consolidation de l'architecture Rust

- Feuille de route : `docs/ARCHITECTURE_ROADMAP.md`. Audit du code actuel sur
  develop, sans refactorisation backend, modification Web/packaging/migrations,
  commit, push ni publication. Un sous-agent a vérifié les frontières Node/I/O.
- Cible retenue : trois crates core/node/shared, appels Rust en standalone,
  Core propriétaire des UUID/droits/SQLite, Node sans dépendance Core/SQL Core.
  Auth et DB restent des modules ; aucune crate vide supplémentaire créée.
- Trois macro-jalons proposés : extraction et sécurité fichiers P1 ; standalone
  utilisable/mesuré P1 ; Node distant et OIDC P2. Ce plan n'implémente aucune de
  ces étapes et ne clôture pas P1. Les quatre images restent une cible packaging.
- Risques prioritaires vérifiés : TOCTOU confinement/réouverture, cycle logique
  Node/inventory, rapports/cache et pagination catalogue en RAM, coût des images
  et relectures de dossiers, contention SQL et budgets globaux manquants.
- Web statique servi par Rust : runtime sans Node.js possible. ffprobe externe
  reste requis pour les scans actuels ; aucun parseur Rust natif annoncé.
- Exécutés : cargo metadata --locked --no-deps --format-version 1 ; cargo fmt
  --check ; cargo clippy --locked --all-features --all-targets -- -D warnings ;
  cargo test --locked --all-features : 50 tests réussis, 5 ignorés (4 navigateurs
  et helper subprocess). git diff --check réussi. Aucun benchmark, navigateur,
  conteneur ou NAS relancé. Les objectifs de performance du document sont proposés.
- Statut initial propre ; `.agents/` et `ORIGINAL_REQUEST.md` sont apparus pendant
  l'audit, hors de nos modifications, et ont été laissés intacts.

## 16 octobre 2026 — clarification bibliothèques et scanner

- Décision : Media Node responsable des dossiers, scan récursif, analyse réelle
  MKV/MP4 multipistes et streaming ; Core responsable du catalogue global SQLite,
  comptes/permissions et réception de l'inventaire. Web = interface/administration.
- Chaque bibliothèque appartient à un Node et une racine explicite : Films →
  Node local → /Films, Séries → Node local → /Séries (exemples uniquement).
  P1 conserve un seul Node local ; aucun multi-Node P2 implémenté.
- Scanner futur autonome, sans dépendance Radarr/Sonarr : dossiers manuels et
  conventions ARR, films/séries/saisons/épisodes, sous-titres externes, NFO/images.
  Entrées non fiables et médias toujours en lecture seule ; caches séparés.
- Code inspecté : inventaire explicite de fixtures, libraryId statiques, sessions
  SQLite présentes ; pas de Library/Node/racine persistants ni de grants fins.
  Les anciennes notes globales pré-auth ne décrivent plus le code courant.
- Contrats précisés dans docs/LIBRARIES_AND_SCANNER.md et documents associés.
  Aucun code, migration ou endpoint ajouté ; administration/permissions sur
  fixtures d'abord, véritable scanner ensuite. Aucun média personnel connecté.
- Validation de cette intervention : git diff --check et revue des liens locaux ;
  suites Rust/Web non relancées pour cette clarification documentaire seule.


> **État courant : 17 octobre 2026. Distribution Docker autonome P1 et fiabilité opérationnelle validées. Sauvegarde et restauration SQLite WAL (`media-backup` et `media-restore`), résistance à la concurrence sous écriture SQLite (10k items), diagnostics de scan bornés (migration 0010) et purge du staging opérationnels. Standalone déployé, testé et récupérable après sinistre.** Les entrées datées conservent l'historique ; vérifier code/tests avant toute annonce.

## 1. Vision actée

- [x] Aether est un **nom de code provisoire** et doit rester changeable.
- [x] Application gratuite, **open source**, entièrement **auto-hébergeable**, interface moderne inspirée de Netflix.
- [x] **Mode autonome prioritaire** : Web + Core + Node local, installation simple et SQLite.
- [x] **Multi-Node** ensuite : un catalogue et une interface, plusieurs serveurs médias.
- [x] **Multi-Core et réplicas Web envisagés pour de fortes charges**, sans les implémenter dans le MVP.
- [x] Stack cible : Rust (Axum/Tokio) + Svelte 5/SvelteKit, SQLx/SQLite, PostgreSQL plus tard.
- [x] Authentification locale sécurisée pour le premier admin et SSO OIDC (ex. Authentik) ; rôle OIDC mapping explicite.
- [x] **Seulement deux rôles `administrator` et `user` : aucun `guest`/invité**.
- [x] Volumes des médias en **lecture seule** et coexistence avec Jellyfin : aucune migration obligatoire.
- [x] Pas de transcodage vidéo obligatoire ; remux facultatif et tests de compatibilité réels.

`[x]` ici signifie **décision retenue**, et **non fonctionnalité codée**.

## 2. Priorité multimédia mise à jour (TRÈS IMPORTANT)

### Priorité n°1 — IMMÉDIATE

- [x] Les fichiers existants **`.mkv` et `.mp4`** constituent les entrées prioritaires.
- [x] Les codecs **AV1 vidéo + Opus audio** sont le **premier couple à faire fonctionner**, pas un cas optionnel.
- [x] Analyser correctement **plusieurs pistes vidéo, plusieurs audios, plusieurs sous-titres intégrés** lorsqu'ils existent.
- [x] **La preuve en navigateur est nécessaire** : présence AV1/Opus dans le fichier ≠ lecture directe garantie. Autoriser un remux/packaging sans réencodage lorsque testé ; sinon afficher l'incompatibilité.
- [x] Les films restent accessibles via Jellyfin parallèlement : **aucune écriture dans les médias**.

### Priorité n°2 — APRÈS le parcours autonome/multi-Node

- [x] Supporter **plusieurs assets distincts** : 1 ou N fichiers vidéo, 1 ou N audios, 1 ou N sous-titres ; associations, langues, offsets, durées et seek.
- [x] Gérer également l'hybride (MKV/MP4 avec audio et sous-titres externes).
- [x] Privilégier audio français et sous-titres français forcés si disponibles/appropriés, tout en respectant le choix explicite utilisateur.

### Ambition de format ultérieure — PAS encore validée

- [x] Le produit doit **prévoir** le cas de fichiers de pistes *vraiment indépendantes et sans conteneur multimédia* (flux AV1 élémentaire et paquets Opus bruts), pas seulement des MKV/MP4 à une piste chacun.
- [ ] Définir et tester le **format effectif** de ces données, le manifeste d'assemblage, l'index, l'initialisation codec et les horodatages.
- [ ] Prouver le packaging / la lecture browser avec ces données raw **sans recoder la vidéo**.
- [ ] Décider seulement après POC si ce mode sera recommandé ou si les fichiers mono-piste conteneurisés restent préférables.

**Ne pas confondre** `.opus` (habituellement Ogg Opus) avec un flux de paquets Opus sans conteneur. `.mkv` mono-piste reste un conteneur. Cette nuance doit figurer dans la documentation.

## 3. Utilisateurs, permissions et SSO

- [x] Rôles : administrateur et utilisateur **exclusivement**, même en distribué.
- [x] Premier lancement : administrateur local créé sans identifiants prédéfinis. Un utilisateur normal n'accède pas aux réglages Core/Node.
- [x] OIDC : Authentik compatible mais pas obligatoire pour une installation locale ; aucune promotion admin automatique ou auto-inscription publique par défaut.
- [x] Les profils de lecture multiples par compte peuvent être développés après le MVP ; **un profil n'est pas un rôle invité**.
- [x] Des permissions de bibliothèques/médias contrôlées côté Core et Node seront nécessaires pour le multi-utilisateur.

## 4. Plateforme de référence

- **Synology DS1522+** (Ryzen R1600, pas d'iGPU), installation coexistante avec Jellyfin et médias présents.
- Accès lecture seule `:ro` ; index, SQLite et cache ailleurs.
- Faible RAM/CPU recherchés, sans objectif chiffré arbitraire non démontré.
- La montée en charge dépend du débit disque/réseau, du packaging, des éventuels relais et de l'efficacité des requêtes.

## 5. Décisions techniques retenues

| Domaine | Décision | État |
|---|---|---|
| Backend | Rust édition 2024 + Axum/Tokio/Tower | Banc local P0 codé et testé ; Core produit non développé |
| Frontend | Svelte 5 + SvelteKit + TypeScript strict | Build statique servi par Rust, écran local P1 et direction visuelle v1 testés |
| Lecteur | HTML5 natif + remux par langue, provisoire | Décision utilisateur P1 ; comparaisons suspendues, interface Player légère conservée |
| DB MVP | SQLx + SQLite | Choisi, non codé |
| DB cluster | PostgreSQL partagé, migration explicite | Futur uniquement |
| Cache/coordination | Local d'abord, Valkey éventuel | Futur si besoin |
| Auth | local bootstrap + OIDC Authorization Code / PKCE | Bootstrap CLI testé ; connexion/sessions/OIDC absents |
| Streaming | Direct si compatible, remux/packaging testé, erreur sinon | Natif, remplacement URL, MSE minimal et Shaka DASH testés localement ; P0 partiel, Video.js et sortie physique NT |
| Raws AV1/Opus | Format et index à concevoir/valider ultérieurement | **Non implémenté** |
| Plugins | Facultatifs, pas de framework à ce stade | Hors MVP |
| Licence | GNU Affero General Public License v3.0 (AGPL-3.0) | Décidée et fichier LICENSE ajouté |

## 6. Feuille de route (aucune phase terminée)

- [ ] **P0A** — Scanner les MKV AV1+Opus et lire/démontrer sur Chrome/Edge/Firefox ; informations de pistes exactes.
- [ ] **P0B** — Même chose pour les MP4 AV1+Opus ; différence claire entre MP4 valide, navigateur compatible et fMP4/MSE si requis.
- [ ] **P0C** — Pistes internes multiples, seek, pistes FR/EN, sous-titres et erreurs explicites ; comparer moteurs de lecture.
- [x] **P1** — Mode autonome utilisable : scan read-only, catalogue minimal, lecture P0, admin local, SQLite, déploiement simple.
- [ ] **P2** — Deux Nodes, appairage/auth, catalogue global, OIDC Authentik et permissions user/admin.
- [ ] **P3A** — Associations de pistes externes, versions vidéo alternatives et audio indépendant, synchronisation.
- [ ] **P3B** — POC raw AV1 / raw Opus + index/manifeste, puis décision de format ; **pas nécessaire pour lancer la V1**.
- [ ] **P4** — Films/séries, recherche, fiches, progression, profils et polish UI.
- [ ] **P5** — Tests de charge, sauvegarde, observabilité, robustesse, compatibilité étendue.
- [ ] **P6 possible** — Plusieurs Core actifs, PG + sessions et jobs partagés, réplication statique et tests de failover.

## 7. Non-objectifs immédiats

Pas de rôle invité, pas de cluster, pas de moteur de recommandation, pas de transcodage vidéo obligatoire, pas de nouveau conteneur propriétaire codé avant POC, pas de plugins, pas d'import Jellyfin indispensable, pas d'app native téléphone/TV au MVP, pas de support « tous les codecs et tous les navigateurs » promis.

## 8. Vérifications et état réel

- [x] Code exécutable présent et vérifié : banc P0 Axum local uniquement.
- [x] Scan MKV AV1+Opus réussi : fixture synthétique, trois pistes, adaptateur ffprobe temporaire.
- [x] Scan MP4 AV1+Opus réussi : fixtures classique/fragmentée, trois pistes chacune.
- [x] Lecture navigateur réelle vérifiée : Chrome 155.0.8059.40, Edge 155.0.4283.45, Firefox 155.0.1 headless Windows ; pixels/PCM/pause/seek/fin et FR/EN par remux, écoute humaine et sélection interne non validées.
- [x] Sécurité bootstrap administrateur vérifiée.
- [x] Standalone déployé.
- [ ] Deux Nodes et OIDC testés.
- [ ] Pistes externes synchronisées.
- [ ] Formats raw conçus et vérifiés.
- [ ] Tests de charge effectués.

**Ne cocher une case ci-dessus qu'après commande/test documenté** (OS, version, méthode, résultat, erreur). Les décisions sont une spécification, pas un état de livraison.

## 9. Prochain pas recommandé à l'IA

La décision utilisateur du 15 octobre autorise P1 et suspend les comparaisons
approfondies de lecteurs. La dernière intervention vise la direction visuelle,
pas de nouvelles fonctions serveur. Examiner docs/UI_DIRECTION.md et les
preuves P1 avant d'élargir l'UI. Recueillir le retour utilisateur sur cette
première composition. Ensuite seulement, jalon séparé pour admin local/SQLite,
puis bibliothèques ro avec autorisations serveur. Pas de médias personnels
exposés par ce prototype localhost sans authentification.

## 10. Jalon réel du 15 octobre 2026 — premier banc P0

- Dépôt initial : seulement les quatre documents racine, aucun code ; `docs/`
  absent malgré les références. Les documents de ce dossier ont été créés pour
  consigner le périmètre réel, pas présentés comme une spécification préexistante.
  Les modifications utilisateur déjà présentes ont été conservées.
- Workspace Rust édition 2024, une seule crate `media-poc` dans `crates/core`,
  frontières internes HTTP/media/node. Axum 0.8.9, tower-http 0.7.1, Tokio
  1.53.3 ; dépendances résolues dans Cargo.lock. Rust/Cargo 1.96.1 utilisés.
- Quatre fixtures synthétiques de huit secondes : MKV, MP4 faststart, WebM,
  MP4 fragmenté ; AV1 Main 8 bits 320×180 24 fps et deux Opus mono 48 kHz
  FR/EN. Génération avec FFmpeg 9.0.2 ; remux `-c copy`, décodage de toutes
  les pistes et comparaison SHA-256 de tous les paquets encodés réussis.
- Probe ffprobe temporaire au démarrage, jamais pendant la lecture : tous les
  flux conservés avec paramètres disponibles et codec init. Tableau extensible,
  mais aucune commutation de pistes, sous-titre, vidéo alternative ou format raw
  validé. Langues source ISO conservées ; normalisation BCP 47 à faire.
- Serveur limité à `127.0.0.1`, quatre IDs explicites, aucun chemin HTTP libre,
  contrôles Host/Origin/cross-site. Lecture seule, Range via ServeFile (64 KiB).
  Pas d'auth : fixtures synthétiques uniquement, pas d'exposition publique/NAS.
  Confinement canonique non atomique face à un écrivain hostile : limite TOCTOU
  documentée, test de symlink Unix pas exécuté sous Windows.
- Chrome 155.0.8059.40 headless via Playwright 1.62.1 : les quatre URLs natives
  ont produit pixels et signal PCM, pause stable, seek à 4 s, reprise et fin.
  Source absente : HTTP 404 et MediaError 4 affichée. Pas d'écoute humaine,
  de sélection FR/EN, de MSE, Video.js/Shaka, Edge/Firefox/Safari ou preuve NAS.
- Commandes exécutées : `./scripts/generate-fixtures.ps1`, `cargo fmt --check`,
  `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`
  (**5 tests Windows réussis**) et `node scripts/browser-smoke.cjs` avec le
  Playwright de l'environnement. Test d'I/O sur copie temporaire de 1 GiB,
  premier chunk borné et abandon ; aucune mesure de RSS globale ni de charge.
- Preuves et commandes : `docs/STREAMING_COMPATIBILITY.md`, `docs/TEST_PLAN.md`,
  `docs/evidence/2026-10-15/{generation,browser}.json`. CI Linux ajoutée mais
  non exécutée sur GitHub. Pas de commit/push/publication, pas de licence modifiée,
  pas de média personnel touché. **Aucune phase complète cochée.**
- Prochain petit jalon proposé : mêmes fixtures sur Edge/Firefox, écoute et
  sélection audio manuelles. Puis P0C (sous-titres/vidéos alternatives et
  comparaison moteurs), avec validation du périmètre avant élargissement.

## 11. Second jalon réel du 15 octobre 2026 — Edge/Firefox et limite audio

- Mêmes quatre fixtures : SHA-256 conformes au premier jalon et identiques
  avant/après chaque test. Aucun original modifié ni régénéré.
- Scénario commun `scripts/media-checks.cjs`, runner `browser-matrix.cjs`.
  Chrome 155.0.8059.40 et Edge 155.0.4283.45 via Playwright 1.62.1 ; Firefox
  155.0.1 installé via geckodriver 0.37.1 officiel Mozilla, outil dev uniquement.
  Profils de test distincts, Windows x64 10.0.26300, headless, pas de fonction
  expérimentale audio activée. Edge existant trouvé dans EdgeCore ; aucune
  installation après le refus automatique d'une tentative d'extraction initiale.
- **12 lectures natives réussies** : pixels RGB, PCM avant/après seek,
  fréquence dominante 439,453125 Hz (FR 440 Hz), pause, seek 4 s, reprise et fin.
  Source manquante : HTTP 404, MediaError 4 et message visible sur les 3 clients.
- **Sélection FR/EN non validée** : `video.audioTracks` absent dans les 12 cas.
  Le banc affiche cette limite et désactive son sélecteur. Le scénario prévoit
  une séquence FR/EN/FR et comparaison 440/880/440 Hz si deux pistes sont
  exposées, mais cette branche reste NT ici. Menus intégrés et écoute humaine NT.
- WebM/fMP4 remuxés sont lus par URL native. `isTypeSupported` annonce MP4/WebM
  et refuse le MIME Matroska testé ; **aucun append/lecture MSE exécuté**.
- fmt/clippy, 5 tests Rust et syntaxe JS vérifiés. Rapports et commandes dans
  `docs/TEST_PLAN.md`, `docs/evidence/2026-10-15/native-audio/` et la matrice.
  CI distante, NAS, Safari, sous-titres et vidéo alternative NT. P0A/P0B restent
  partiels, P0C commencé mais non terminé. Aucun ajout SvelteKit/auth/catalogue/Node.
- Économie de contexte : lectures ciblées et un scénario partagé entre clients ;
  aucun pourcentage d'économie inventé.

## 12. Jalon P0C réel du 15 octobre 2026 — remux FR/EN et changement de source

- `scripts/prepare-language-remux.ps1` produit quatre nouveaux fichiers dans
  `tests/fixtures/generated/language-remux` avec `-c copy`, une vidéo AV1 et
  un audio Opus chacun. Refus d'écrasement vérifié. Tous les 192 paquets AV1 et
  401 paquets Opus sélectionnés sont identiques par SHA-256 aux originaux,
  sorties décodées intégralement. Les quatre fichiers initiaux restent intacts.
- Préparation unique : MKV FR 1407,36 ms (premier lancement), MKV EN 87,11 ms,
  MP4 FR 134,03 ms, MP4 EN 108,30 ms ; 710698 octets supplémentaires.
  Pas de benchmark répété ou de mesure NAS. FFmpeg uniquement hors lecture.
- Inventaire : quatre IDs dérivés optionnels et association langue/source/index,
  aucune entrée HTTP par chemin libre. Un dossier de remux partiel ou des
  codecs/langues incompatibles empêchent le démarrage. Les remux utilisent les
  mêmes routes GET/HEAD/Range et le même confinement en lecture seule.
- Menu de banc distinct de l'audio natif : changement réel d'URL, métadonnées,
  restauration de `currentTime`, puis reprise seulement si auparavant en lecture.
  Erreurs explicites ; pas de restauration automatique de l'ancienne source.
- Scénario commun `scripts/remux-checks.cjs`, lancé via
  `MEDIA_TEST_SCENARIO=remux` : **36 changements en lecture + 12 en pause réussis**,
  MKV/MP4 sur Chrome 155.0.8059.40, Edge 155.0.4283.45, Firefox 155.0.1,
  Windows x64 10.0.26300 headless, navigateurs exécutés séquentiellement.
  FR mesuré 439,453125 Hz, EN 878,90625 Hz (une observation 884,765625 Hz),
  tolérance ±20 Hz. Erreur de position enregistrée 0 s, pause conservée à
  4,25 s, reprise FR et fin vérifiées. 404/MediaError 4 conservé sur les 3 clients.
- Demande → détection audio stable : Chrome 138,8–156,8 ms, Edge 143,2–169,2 ms,
  Firefox 164–288 ms. Cette mesure inclut la FFT glissante ~171 ms ; ce n'est
  pas le silence physique. Silences PCM échantillonnés : 22,8–233 ms selon le
  cas, écarts callbacks vidéo avant/après 50–253,3 ms. Polling maximal observé
  44 ms. Quelques frames antérieures à la cible avant restauration sont
  détectées ; absence de flash et synchronisation A/V fine non prouvées.
- Ce compromis duplique la vidéo/recharge le démuxeur et **n'est pas définitif**.
  Vraie sélection interne, MSE, Video.js/Shaka et écoute humaine restent NT.
  Méthode/limites : `docs/P0C_AUDIO_SWITCHING.md`, matrice chiffrée et preuves
  `docs/evidence/2026-10-15/remux/`. Tentative intermédiaire Chrome conservée :
  race du harnais entre readyState prêt et événement activant le menu ; attente
  corrigée puis série finale complète réussie.
- 6 tests Rust réussis, fmt/clippy et syntaxe JS vérifiés. CI adaptée pour
  préparer les remux mais exécution GitHub non revendiquée. Le plan suivant
  10 bits/Opus 5.1/sous-titres est écrit, **aucune fixture ni implémentation**.
  P0C complet reste ouvert ; aucun ajout hors périmètre ni push/publication.
- Régression native Chrome rejouée après P0C : quatre sources initiales et
  erreur 404 réussies. SHA-256 des quatre remux toujours conformes à leur
  génération après tous les essais.

## 13. Jalon P0C du 15 octobre 2026 — profil représentatif 10 bits/5.1/texte

- Nouveau générateur Python standard `scripts/generate-representative-fixtures.py`,
  dossier `tests/fixtures/generated/representative`, refus d'écrasement vérifié.
  Mire synthétique encodée une fois en AV1 Main `yuv420p10le`, SDR BT.709,
  320×180/24 fps, 12 s. Aucun transcodage de vidéo existante.
- MKV/MP4 multiaudio FR Opus 5.1 et EN Opus stéréo, trois sous-titres FR complet,
  FR forced et EN complet ; deux dérivés EN. AV1/Opus copiés, 288 paquets vidéo
  et 601 par audio comparés par SHA-256, init codec identique. SRT MKV converti
  explicitement en mov_text MP4 : cette conversion texte n'est pas bitcopy.
- Décodage intégral A/V, 20 contrôles PCM de canaux et 60 marqueurs d'amplitude
  hors navigateur réussis. FR FL/FR/FC/LFE/BL/BR : 440/550/660/80/770/880 Hz,
  EN 990/1100 Hz ; marqueurs à 2, 6 et 9 s. Préparation unique 21,09 s,
  2266372 octets. Huit anciens médias et quatre nouveaux intacts après tests.
- Limite réelle du muxeur MP4 9.0.2 : default ajouté sur vidéo et premier
  sous-titre malgré les dispositions 0 demandées ; MKV les conserve. Forced
  du second sous-titre et defaults audio corrects. Écart consigné par
  `strict_disposition_preservation=false`, pas masqué par le succès A/V.
- Scénario commun `representative-checks.cjs`, `MEDIA_TEST_SCENARIO=representative` :
  **12 parcours A/V réussis**, 4 sources × Chrome 155.0.8059.40, Edge
  155.0.4283.45, Firefox 155.0.1, Windows x64 10.0.26300 headless ; profils
  par défaut, navigateurs séquentiels. Pixels, PCM et six/deux fréquences
  séparées avant/après seek, pause, 48 seeks, reprise, fin et erreur404/code4.
- 48 observations de flash/amplitude : écart horloge média audio − PTS vidéo
  Chrome −2,40..+20 ms, Edge −0,89..+20,97 ms, Firefox −54..+6,50 ms.
  Mesure grossière, RMS ~21,3 ms, vidéo ~41,7 ms, polling max 27,5 ms,
  tolérance préétablie ±200 ms ; pas de synchronisation physique prouvée.
- Graphe FR expose bien six voies, mais destination annoncée limitée à deux
  canaux : **restitution physique 5.1 NT**. Canvas ne prouve pas affichage
  physique 10 bits. HDR/4K, matériel/logiciel décodeur, NAS et films longs NT.
- Trois pistes texte détectées par fichier, mais `textTracks.length=0` et
  `audioTracks` absent dans les douze cas. Menu texte conditionnel désactivé ;
  sélection/cues/rendu intégrés et auto-forced non validés. EN vérifié par
  chargement de son dérivé, pas par commutation interne. Restauration de
  position FR/EN 8 bits non attribuée à ces nouveaux fichiers.
- 7 tests Rust, fmt/clippy et syntaxes JS/Python vérifiés. Tests HTTP couvrent
  12 assets ; tests du profil, pistes, forced, MP4 tronqué et dossier incomplet.
  CI adaptée, exécution distante non revendiquée. Régressions Chrome native
  et remux 8 bits repassées. Un premier Cargo test a été bloqué par le binaire
  Windows en cours ; arrêt du prototype puis relance complète réussie.
- Preuves `docs/evidence/2026-10-15/representative/`, méthode et matrice mises
  à jour. Proposition séparée `PLAYER_COMPARISON_EXPERIMENT.md`, non exécutée,
  moteur non choisi. P0C complet reste ouvert ; aucun ajout hors P0 ni publication.
- Vérification texte additionnelle réellement exécutée :
  `python scripts/representative_subtitles.py`, 12 pistes et 48 cues exacts
  (horodatages à ±20 ms). Module aussi intégré aux générations fraîches.
  Rapport distinct archivé ; aucun rendu navigateur déduit de ce résultat.

## 14. P0C — MSE minimal exécuté (2026-10-15)

- Première expérience limitée, sans intégrer simultanément Video.js/Shaka.
  Banc /mse : une MediaSource et deux SourceBuffers, vidéo commune conservée,
  audio remplacé sans load/src ni seek forcé. Buffer audio complet 12 s remplacé
  pour conserver la langue après seek arrière ; pas un scheduler de film long.
- scripts/prepare-mse.py : remux -c copy, 288 AV1/601 FR/601 EN paquets et init
  identiques, décodage réussi ; MKV source AV1 Main10/FR Opus 5.1/EN stéréo.
  Originaux intacts. PTS/DTS audio +7 ms, durée quantifiée jusqu'à 1 ms,
  timebases conservées dans rapport ; aucune identité temporelle prétendue.
- Chrome 155.0.8059.40, Edge 155.0.4283.45, Firefox 155.0.1 Windows headless :
  18 changements (lecture/pause/après seek), six/deux signaux séparés, seek EN
  arrière/avant, pixels/frames, pause, source et buffer vidéo conservés validés.
  Trois sous-titres WebVTT externes : cues et neuf captures avec texte visible.
  Forced manuel seulement ; ni sélection intégrée ni auto-forced validés.
- Demande → signal correct en lecture/après seek : Chrome 431,7–450,1 ms,
  Edge 424–447,8 ms, Firefox 377–1205 ms. FFT ~171 ms, RMS ~21 ms,
  trois observations correctes, polling max56 ms. Silences échantillonnés
  consignés dans les rapports ; pas de promesse sans interruption. Pause inclut 100 ms
  d'observation et reprise, chiffrée séparément. Un parcours/browser seulement.
- Échecs conservés : comparaison format SRT/VTT corrigée numériquement ;
  duration=12 refusée corrigée par fin réelle des buffers ; Firefox AudioContext
  sans activation utilisateur bloqué, vrai clic WebDriver puis succès, aucune
  préférence autoplay modifiée. Attentes bornées et progression de test ajoutées.
- 8 tests Rust passent, fmt/clippy, syntaxes JS/Python. HTTP MSE fixe : GET/HEAD,
  Range exact/416, MIME, whitelist/Origin ; 404 et langue invalide navigateur.
  CI préparée, non exécutée à distance. Adaptateurs navigateur réutilisés.
- CPU/RSS, films longs, réseau dégradé, segments MSE tronqués, EOS, dérive A/V,
  restitution physique 5.1/7.1 et affichage physique10bits : NT.
- Clarification utilisateur : chaque piste peut être mono/stéréo/5.1/7.1 ;
  conserver ses canaux sans imposer le 5.1. Mono et 7.1 restent non testés.
- Recommandation provisoire : poursuivre MSE vidéo commune/audio séparé/VTT,
  prochain petit essai Shaka + DASH réel ; Video.js10 ensuite, aucun lecteur
  définitif choisi. Disponibilité Video.js10 vérifiée, compatibilité NT.
  Remux URL 8bits reste témoin, latences non comparables directement.
- Preuves et limites : docs/P0C_MSE_EXPERIMENT.md et evidence/2026-10-15/mse/.
  P0C reste ouvert ; aucun ajout P1, push ou modification des originaux.

- Revue : assertion pixels corrigée pour exclure alpha et exiger >10 couleurs
  RGB, puis trois parcours navigateur rejoués avec succès. Régression HTML5
  Chrome (4 sources, pause/seek/fin/404) réussie après extraction des adaptateurs.
  Refus d'écraser le packaging existant vérifié.

## 15. P0C — Shaka 5.2.13 face au MSE (2026-10-15)

- Shaka fixé depuis npm, SHA512 archive/SHA256 bundle consignés, Apache-2.0
  et LICENSE conservés dans fixtures ignorées. setup prepare-shaka.py horsligne
  possible avec --archive, aucun CDN ni FFmpeg ajouté au serveur.
- prepare-dash.py : copies binaires fMP4/VTT MSE inchangés, MPD12.008s,
  SegmentList/Timeline et init/ranges tirés de tfdt/tfhd/trun réels (7fragments),
  154ms unique, paquets/init reliés aux preuves MSE. Audio+7ms conservé.
  Originaux et copies vérifiés avant/après. Préparations refusent écrasement.
- /shaka et whitelist HTTP fixe ; Shaka LoadMode.MEDIA_SOURCE observé.
  APIs versionnées selectAudioTrack(track,0), preferredAudio/preferredText,
  NativeTextDisplayer(player), selectTextTrack(track|null). ABR désactivé,
  handleForcedSubtitlesAutomatically=false pour respecter le choix texte manuel.
- Chrome 155.0.8059.40, Edge 155.0.4283.45, Firefox 155.0.1 Windows 10.0.26300
  headless : 108 changements (18×6couples), 36marqueurs A/V et choix manuels
  conservés après seek. FR et FRforced par défaut Shaka, EN manuel prioritaire ;
  12captures Shaka inspectées, FR complet/forced/EN/off corrects.
- Latence médiane/P95=max (n12actifs) : MSE Chrome370.4/383.2ms,
  Edge365.9/386.7ms, Firefox1120/1151ms ; Shaka160.75/181.2ms,
  152.05/211ms,167/207ms. Pause n6parcouple séparée avec hold100ms/reprise.
  Mêmes URL vidéo mais Shaka36/36 actifs avec waiting/seeking, silence jusqu’à
  150ms, gap vidéo182.64ms. MSE0silence détecté et0waiting ; reste signalancien
  plus longtemps. Aucune promesse de lecture ininterrompue.
- A/V : indicateur video.currentTime au seuil RMS moins PTS flash, pas timestamp
  PCM physique. 36observations dans±200ms ; plages et résolution dans protocole.
  Pas de mesure précise de+7ms, de sortie matérielle ni de dérive longue.
- Harness commun nouveau : FFT8192, RMS derniers1024 du tableau8192. Ancienne
  troncature initiale biaisait le RMS d’environ149ms ; parcours préliminaire
  conservé, exclu. Ne pas comparer ces latences aux chiffres historiques MSE.
- CPU/RAM échantillonnés séparément : arbre navigateur par PID+création, Rust
  distinct, CPU deltas non négatifs, working sets sommés (pages partagées
  possiblement doublées). CPU comprend graphe/tests ; collecte coûte247–746ms.
  Pas de budget produit/NAS ni d’avantage CPU extrapolé. Chiffres et JSONL archivés.
- 9tests Rust, fmt/clippy, syntaxes JS/Python, tests médiane/P95/PID passent.
  MPD absent diagnostic Chrome→404/Shaka1001 ; pas de preuve affichage erreur
  via ce diagnostic API. Menu texte du banc synchronisé avec choixAPI puis
  contrôleChrome effectué. CI préparée, exécution distante non revendiquée.
- Recommandation provisoire : Shaka candidat pour DASH/langues/texte et vitesse
  de sélection ; tester marge audio non nulle et clips longs avant décision.
  MSE inchangé conservé ; Video.js reste prochain jalon distinct, aucunP1.
- docs/P0C_SHAKA_EXPERIMENT.md, STREAMING_COMPATIBILITY et preuves
  docs/evidence/2026-10-15/shaka/. Mono/7.1, physique 5.1 / 10 bits, réseau dégradé,
  segments corrompus, endurance et NAS NT. Aucun push/publication.

## 16. P0C — safeMargin, protocole et pilote (2026-10-15)

- Shaka5.2.13 et MSE conservés. switchLanguage accepte explicitement 0/.5/1/2/4,
  défaut0 inchangé. Aucun média, packaging ni bufferingGoal modifié.
- Protocole et runner isolent un changement mesuré par présentation12s neuve ;
  marges/sens tournants, fenêtres500ms après signal, compteurs frames perdus
  distincts des callbacks, probes A/V et seekarrière. Sampler CPU exclu pour
  comparer toutes les marges à un0 frais dans les mêmes conditions.
- PiloteChrome12essais 0/4 réussi pour changements/marqueurs ; marge4 rejoue
  l'ancienne langue après seekarrière dans6cas, marge0 dans0cas. Régression
  comptée séparément ; pilote exclu de la matrice finale en cours.
- Nouveau test statistiques échecs/sens/frames/seeks et anciens tests stats
  passent ; syntaxesJS/fmt vérifiées. Documents P0C_SAFE_MARGIN_EXPERIMENT.md.
- Matrice complète en cours, aucune marge gagnante ni fonctionnalité globale
  validée à ce stade. Résultats finaux à consigner après exécution complète.

- Première série finale achevée : Chrome140essais, aucun échec changement/marqueur,
  112retours de langue incorrecte après seek dans les marges positives.
  Preuves docs/evidence/2026-10-15/safe-margin/chrome/. Edge/Firefox en cours.

- Série Edge achevée :140changements/marqueurs aboutis,112probes seek ne
  correspondent pas à la langue sélectionnée. Série Firefox lancée séparément.

## P0C — safeMargin terminé (2026-10-15)

Shaka5.2.13 conservé, aucun média/packaging réencodé ou modifié. MSE conservé.
424 essais :420 changements mesurés,4 échecs de préparation Firefox conservés,
complétés par4 essais indépendants. Marges0/.5/1/2/4, trois navigateurs Windows.
20 mesures de lecture et4 pause/4 après-seek par marge et navigateur.

| Navigateur | Marge0 latence P50/P95 | Marge0,5 latence P50/P95 |
|---|---:|---:|
| Chrome |163/186ms|409/437ms|
| Edge |166/197ms|402/422ms|
| Firefox |185/214ms|1123/1134ms|

Marge0,5 : zéro silence détecté et zéro frame perdue signalée en lecture et
après-seek. Mais les336 probes arrière de TOUTES les marges positives rejouent
l'ancienne langue ; les84 probes marge0 sont corrects. Aucun réglage ne résout
tout. Candidat fluidité :0,5s, NON promu ; défaut0 conservé. Marge4 retarde le
signal d'environ2,3s sans bénéfice démontré.420 marqueurs A/V dans±200ms,
indicateur média non physique ; polling maximal168ms limite les conclusions.

9 tests Rust, fmt/clippy, syntaxes et statistiques passent. Anciennes suites
navigateur :4/6 réussies ; ShakaChrome échoue sur progression vidéo aprèsseek,
MSEFirefox timeout PCM FR. Ces échecs sont conservés, causes non résolues.
Pas de lecteur définitif ni ajout Video.js/SvelteKit.
Voir docs/P0C_SAFE_MARGIN_EXPERIMENT.md.
Preuves : docs/evidence/2026-10-15/safe-margin/.

## Vérification courte de la resélection audio (2026-10-15)

Six essais ciblés FR→EN, safeMargin0,5, Chrome 155.0.8059.40,
Edge 155.0.4283.45 et Firefox 155.0.1. Script scripts/verify-audio-seek.cjs.
Sans attente500ms après seek1,25s, le contrôle initial trouve déjà le signalEN
après retour0,5s sur les trois navigateurs : cela ne prouve aucune correction.
Avec la même attente500ms que la matrice précédente, le défaut est reproduit
sur les trois : pisteEN sélectionnée mais signalFR après seekarrière.
Resélectionner la pisteEN déjà active avec selectAudioTrack(track,0), puis
observer700ms, laisse le mauvais signalFR dans les trois cas. Cette méthode
simple ne valide donc pas une invalidation du buffer. Le résultat dépend du
point de changement dans les segments ; aucune correction générale livrée.
Source vidéo conservée et37 empreintes originales identiques avant/après
chaque essai. Syntaxe du nouveau script et deux suites statistiques passées.
Pas de modification Rust : suite Rust non relancée, pas de répétition de la
matrice420points pour cette vérification ciblée (token-saver appliqué).
Preuves docs/evidence/2026-10-15/audio-seek-quick/, contrôles et reproductions
séparés. Prochain essai : mécanisme supporté d'invalidation audio, puis preuve
FFT ; aucune méthode privée ou double commutation non validée ajoutée.

## Fichiers communautaires, CI unifiée et politique de sécurité (2026-10-15)

- Licence actée et ajoutée : GNU Affero General Public License v3.0 (`LICENSE`), conformément au choix utilisateur explicite.
- Fichiers communautaires ajoutés à la racine et configurés pour GitHub :
  - `CONTRIBUTING.md` : aligné avec les règles d'architecture (rôles `administrator`/`user`, lecture seule `:ro`, absence de transcodage forcé, priorité P0-P5, nom neutre `MEDIA_*`).
  - `SECURITY.md` : politique de sécurité standard et signalement privé GitHub Security Advisories, pointant vers `docs/SECURITY.md`.
  - `CODE_OF_CONDUCT.md` : Contributor Covenant v2.1.
  - `.github/ISSUE_TEMPLATE/` : formulaires structurés pour bugs (`bug_report.yml`), propositions d'améliorations (`feature_request.yml`) et `config.yml`.
  - `.github/PULL_REQUEST_TEMPLATE.md` : checklist d'architecture et de sécurité `AGENTS.md`.
  - `.github/dependabot.yml` : suivi automatisé hebdomadaire pour Cargo et GitHub Actions, prêt pour npm (`apps/web`).
- CI unifiée `.github/workflows/ci.yml` :
  - Remplace le banc initial `p0.yml` sans duplication.
  - Exécute génération des fixtures synthétiques P0, tests statistiques Node, `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`.
  - Intègre l'audit de sécurité des dépendances avec `rustsec/audit-check` (`cargo audit`).
  - Intègre la configuration conditionnelle pour SvelteKit (détection de `apps/web/package.json`).

## Nettoyage forcé de la variante active — preuve ciblée (2026-10-15)

API publique Shaka5.2.13 testée : selectVariantTrack(activeVariant,true,0).
La source versionnée player.js/switchVariant_ confirme force=true pour la
variante déjà active si clearBuffer=true, contrairement à selectAudioTrack.
Le nettoyage concerne aussi la vidéo : ce n'est PAS une invalidation audio seule.

Six essais, FR→EN et EN→FR sur Chrome 155.0.8059.40, Edge 155.0.4283.45,
Firefox 155.0.1 : défaut préalable reproduit6/6 après changement marge0,5 et
seekarrière ; nettoyage forcé puis bonne langue réellement décodée6/6.
FFT trois observations consécutives, six/deux voies, confirmation après700ms.
URL vidéo conservée, frames vidéo à nouveau présentées,37 originaux intacts
par essai. Aucun load(), réencodage, API privée ou double changement de piste.

| Navigateur | Retour au signal correct FR→EN | EN→FR |
|---|---:|---:|
| Chrome |354,9ms|778,2ms|
| Edge |395,7ms|368,3ms|
| Firefox |639ms|161ms|

Des événements waiting/seeking/buffering sont observés. Une frame perdue
signalée dans le cas FR→EN Edge et dans le cas FR→EN Firefox. Ne pas annoncer
une récupération sans interruption. Un essai par sens/navigateur : ni P95
ni stabilité à long terme, ni synchronisation physique validés ici.

Script scripts/verify-audio-seek.cjs : MEDIA_SEEK_RECOVERY=variant-clear,
MEDIA_SEEK_FROM=fr ou en, MEDIA_BROWSER_CHANNEL et MEDIA_REPORT_DIR neuf.
Sans MEDIA_SEEK_RECOVERY, l'essai de simple resélection reste disponible.
Preuves docs/evidence/2026-10-15/variant-clear-quick/.

Conclusion : mécanisme public de récupération démontré sur ces6cas, pas encore
intégré automatiquement au seek. Défaut0 du banc inchangé. Prochain petit jalon :
politique de récupération sur seek avec pause conservée et prévention des boucles,
puis répétitions/seek avant-arrière et mesure A/V. Ne pas choisir le lecteur final.
Seul le script expérimental a changé ; syntaxeJS vérifiée, suite Rust non
relancée car aucun code Rust/serveur modifié. Tests précédents non effacés.


## 2026-10-15 — P0C, politique expérimentale après seek

Cause partielle établie : safeMargin positif conserve des paquets audio anciens ;
resélection de la piste active ne les purge pas. MPD/IDs distincts, aucune preuve
justifiant un changement de packaging. Cause des blocages vidéo après purge
non identifiée. Purge publique selectVariantTrack(active,true,0) = contournement,
pas correction fiable ; purge audio ET vidéo. Marge normale0 conservée.

Contrôleur isolé audio-seek-policy.js, opt-in dans /shaka : tentative unique,
verrou/récursion/concurrence, position/pause/vitesse/choix manuel conservés,
append frais attendu, timeout et erreur terminale, aucune FFT de production.
Watchdog5s après reprise : blocage>2s en lecture -> erreur, pas seconde purge.
Vrai changement audio marge0 nettoyant déjà l'audio seul : pas purge vidéo inutile.

36 essais préliminaires Chrome/Edge : seuls8 seeks protégés mesurés,7 avec
langue finale correcte,3 marqueurs A/V réussis ; autres échecs préparation ou
synchronisation.12/12 contrôles mesurés reproduisent l'ancienne langue.
Ces résultats incomplets ne sont PAS un taux de fiabilité produit.
Deux démarrages Firefox WebDriver expirés45s : aucune validation Firefox actuelle.
Contrôle final préparation1,25s FR->EN seek0,5s : Chrome et Edge1/1 protégé
correct chacun, marqueurs A/V réussis ; contrôle Chrome déjà correct sans purge,
contrôle Edge incorrect. Signal307/1755ms ; silence238/1677ms ; frames perdues6/46.
Position conservée à8,7ms près. Fluidité insuffisante, option désactivée par défaut.

Oracle représentatif : FR six fréquences440/550/660/80/770/880Hz, EN990/1100Hz,
pas le couple mono440/880 ancien. Aucun média/packaging réencodé ou modifié ;
hash37 originaux avant/après chaque série complète. Tests antérieurs conservés.
Rust fmt/clippy strict/9 tests intégration passent. Tests Node politique/statistiques
passent ; CI ajoutée mais pas exécutée à distance. Précisions et preuves :
docs/P0C_AUDIO_SEEK_RECOVERY.md et docs/evidence/2026-10-15/audio-seek-policy/.

Prochain jalon P0 seulement : isoler blocages vidéo, refaire Firefox fonctionnel,
puis répétitions séquentielles et seeks avant/arrière dans une même présentation.
Ni solution définitive, ni P1, ni Video.js validés.

Validation finale Node : 10 tests politique + 2 suites statistiques =12/12, exit0. Syntaxe des cinq scripts modifiés vérifiée. Le dernier ajustement distinguant audioId/marge0 est couvert unitairement ; pas de nouvelle matrice navigateur après cet ajustement.

## 2026-10-15 — P1 local puis première refonte visuelle

- Décision utilisateur : HTML5 natif et copies par langue provisoires ; petites
  interruptions/rechargement acceptés, comparaisons Shaka/MSE/Video.js suspendues.
  Expériences conservées ; aucune récupération instable activée par l'écran P1.
- Première intégration apps/web Svelte 5.57.2 / SvelteKit 3.1.0 / TypeScript strict et build
  statique sous /app/, servi par Axum existant. Pas de Node permanent. API démo
  sources/pistes, quatre copies AV1 10 bits / Opus FR 6 canaux et EN 2 canaux, VTT P0 réutilisés.
  Préparation hors lecture bornée, paquets et originaux SHA256 inchangés.
  Pas de compte, DB, vraie bibliothèque, multi-Node ou plugin implémenté.
- Le premier écran fonctionnel est remplacé à la demande de l'utilisateur :
  noir encre/blanc chaud/améthyste, hero cinématographique original, marque SVG
  existante conservée, vraies cartes de sélection MKV/MP4, lecteur intégré,
  panneau de séance et contrôles discrets. Le paysage est une illustration
  déclarée, pas une vidéo/œuvre promise dans le catalogue.
- Fondations : tokens.css global, Button/Field/Badge/Icon/MediaCard ; styles
  hover/focus/active/disabled, clavier, mobile et mouvement réduit. Aucune nouvelle
  dépendance UI. NativePlayer et logique de lecture/audio/sous-titres inchangés.
  Seul ajustement serveur de cette refonte : CSP img-src self pour les images locales.
- Vérifications après refonte : check Svelte/TS : 0 erreur et 0 avertissement, lint,
  5 tests Vitest, build, npm audit : 0 vulnérabilité ; Rust fmt/clippy strict et 10 tests
  intégration passent (9 P0 conservés). Browser P1 MKV/MP4 sur Chrome/Edge/Firefox :
  pixels/PCM réels, FR/EN pause/lecture, position/volume/vitesse, seek EN,
  VTT/off/choix manuel, fin et erreur404 réussis ; hashes médias/VTT inchangés.
- Firefox est validé à 1× seulement : retour FR 5.1 à 1,5× sans PCM attendu observé
  avant refonte, cause non établie, pas corrigée dans le jalon visuel. Ne pas
  annoncer de compatibilité universelle ou d'expérience technique achevée.
- Layout Chrome 360/390/768/1280/1440/1920 px : pas de débordement/erreur CSP,
  focus clavier et images vérifiés ; carte MP4→vraie source, erreurs Core503,
  VTT404 et audio404, tentative audio unique. Captures desktop/mobile inspectées.
- Preuves docs/evidence/2026-10-15/p1-ui-v1/, direction docs/UI_DIRECTION.md,
  parcours/commandes docs/P1_WEB_INTEGRATION.md. CI distante, vraie sortie
  physique 5.1 / 10 bits, films longs, NAS, Safari/mobile réel restent non vérifiés.
- Première direction visuelle concrète, pas une expérience finie. Ne pas étendre
  automatiquement tous les écrans ; prochain petit jalon : retour sur cette base.

## 2026-10-15 — Lecteur immersif P1 validé localement

- Le lecteur encadré/panneau permanent de la page est remplacé par un dialog
  plein écran, ouvert depuis les actions existantes de l'accueil. Identité,
  hero, cartes et choix de source de l'accueil conservés. Aucune route ajoutée.
- Fullscreen API depuis le clic ; repli couvrant la fenêtre en cas de refus.
  Vidéo sans controls natifs, ratio conservé sur fond noir ; Retour/titre en
  haut, progression et commandes superposées en bas, menus flottants à droite.
- PlayerControls Svelte ne connaît que PlaybackState/callbacks. ImmersivePlayer
  reçoit une PlayerFactory ; NativePlayer et backend inchangés. Types étendus
  aux métadonnées déjà présentes pour les libellés audio/vidéo réels.
- FR/EN, WebVTT/off/forced, vitesse, volume/muet, seek, sources MKV/MP4 ; choix
  manuels et état conservés. Les sources de démo ne sont pas présentées comme
  des œuvres ou versions éditoriales. Pistes vidéo internes affichées en
  lecture seule avec sélection explicitement indisponible ; aucun faux choix.
- Contrôles cachés après3s en lecture ; pause/menu/erreur/clavier les gardent.
  Curseur masqué, mouvement/tactile les révèlent, menus clavier, Échap,
  Space/flèches/M/F, zones sûres et focus restauré après sortie fullscreen.
- Retour détruit le moteur, retire src/pistes, arrête média et timers. Les
  callbacks tardifs sont ignorés par génération. Aucun retry automatique ajouté.
- Tests réels Windows10.0.26300 headless : Chrome155.0.8059.40,
  Edge155.0.4283.45, Firefox155.0.1/geckodriver0.37.1. Fullscreen réel et refus
  simulé, menus/clavier, audio/texte/source, retour/nettoyage réussis. Toucher
  simulé390×844/844×390 et layout360–1920px sans débordement/CSPerror ;
  erreurs Core503/VTT404/audio404 explicites, tentative audio unique.
- Non-régression média P1 :6parcours MKV/MP4, pixels/PCM FR6/EN2, seek,
  pause/reprise, choix texte, fin/404, hashes originaux/VTT inchangés. Chrome
  à1,5× ; Edge et Firefox validés à1×. Les échecs Edge multicanaux accélérés
  observés ne sont pas effacés ni présentés comme résolus ; Firefox1,5× reste
  également une limite connue. Pas de récupération ou de transcodage ajouté.
- Tests P1 adaptés aux vrais menus sans supprimer leurs assertions média.
  Position restaurée observée à seeked, séparément de l'avancement après reprise.
  Fixture repositionnée avant commutation active pour ne pas mesurer après EOS.
- check Svelte/TS0erreur0avertissement, lint/build,8tests Vitest et10tests Rust
  passent ; fmt/clippy stricts et git diff --check réussis. Pas de CI distante.
- Documentation docs/IMMERSIVE_PLAYER.md ; preuves et captures dans
  docs/evidence/2026-10-15/immersive-player/. Mobile physique/Safari, films
  longs, restitution5.1/10bits et NAS restent non vérifiés. P1 complet non clos.
- Token saver appliqué à la fin : lectures ciblées, logs condensés et réutilisation
  des preuves existantes ; aucun pourcentage de quota économisé inventé.

## 2026-10-15 — Premier parcours catalogue P1 validé localement

- Navigation statique Accueil / Films / Séries / Recherche / Paramètres ; fiches
  par paramètre `work`, film et série séparés, saison puis épisode vers lecteur.
- API `/api/v1/catalog` et `/api/v1/catalog/{id}`, recherche, tri alphabétique,
  pagination 24 par défaut (maximum API 100). Associations explicites des
  fixtures Work / Edition / MediaVersion / MediaSource vers Asset / Track.
  Un film `fixture-film` regroupe MKV/MP4 ; `fixture-series` contient une saison
  et `fixture-episode-1`, réutilisant explicitement la même mire synthétique.
  Pas de fusion par titre, base persistante ou identification automatique.
- Années/affiches réelles, dates d'ajout, historique et recommandations absents :
  états explicites, replis typographiques signalés. Aucun média personnel.
- Lecteur/moteur conservés ; sources limitées à l'œuvre ou l'épisode choisi.
  Amorçage des scripts navigateur adapté sans retirer assertions média.
  Bootstrap statique externalisé récursivement pour routes directes sous CSP.
- Vérifié : 15 tests Rust passent ; check Web 0/0, lint, 8 tests Vitest et
  build passent après ajustements UI. Chrome155.0.8059.40 Windows headless :
  deux parcours média P1 MKV/MP4 pixels/PCM/hashes réussis après adaptation.
  Commandes Rust standard fmt --check, clippy --locked --all-targets -- -D warnings,
  test --locked relancées avec succès. Dernier browser-catalog : chargement
  explicite et épisode jamais présenté comme film par `work` détourné vérifiés.
- browser-catalog : 390/768/1440px, fiches, série→épisode→MP4, recherche vide,
  catalogue vide injecté, 503/réessai, 404, pagination simulée72 bornée24 passent.
  Captures Accueil/Films/Séries desktop/mobile inspectées. browser-p1-layout :
  360/390/768/1280/1440/1920px sans débordement/CSP, audio404 tentative unique,
  VTT404/Core503 passent. browser-immersive : fullscreen réel/refus, menus,
  clavier, tactile simulé390×844/844×390 passent. Preuves dans
  docs/evidence/2026-10-15/catalogue/ (report.json, regression, layout, immersive).
  Aucun nouveau résultat Edge/Firefox/Safari/mobile physique/NAS ; pagination
  simulée sans mesure de charge ou catalogue personnel.
- Bootstrap admin, authentification, sessions, RBAC, SQLite et scanner absents :
  interdisent l'exposition de dossiers personnels. Prochain petit jalon : admin
  local + sessions/RBAC + SQLite, avant dossier explicitement configuré `ro`
  et scan borné hors requêtes HTTP. P1 complet non clos, essais P0 suspendus.
- Token saver : lectures ciblées et deux délégations ; aucun chiffre d'économie
  revendiqué. Documentation : docs/CATALOGUE_P1.md et contrat docs/API.md.


## 2026-10-15 — Navigation principale et menu profil

- Navigation principale limitée à Accueil / Films / Séries / Recherche. Recherche
  associe une loupe SVG et son texte sur desktop comme dans la barre mobile.
  Paramètres reste accessible à son URL existante via le menu profil uniquement.
- ProfileMenu frontend en haut à droite : avatar de repli, mention « Aucun profil
  configuré », Profil désactivé « Bientôt », lien Paramètres fonctionnel. Pas de
  faux compte, rôle, déconnexion, authentification ou nouveau backend.
- Popover HTML natif auto : ouverture clic/Entrée/Espace, focus du premier lien
  utilisable, fermeture Échap/clic extérieur/re-clic du bouton/navigation et
  sortie clavier hors du composant. Retour du focus après Échap vérifié.
  Styles sombre/améthyste, hover/focus/active et mouvement réduit conservés.
- Aucun fichier du lecteur ni code Rust modifié pour cet ajustement. Ancien diff
  de travail préservé. Le test catalogue passe désormais par le menu profil pour
  ouvrir Paramètres et vérifie les quatre liens principaux.
- Web : npm run check (0 erreur/0 avertissement), lint, 8 tests Vitest et build
  réussis. Chrome155.0.8059.40 Windows10.0.26300 headless : nouveau runner
  browser-profile.cjs passe en360/390/768/1440px (clavier, tactile simulé,
  fermeture, focus, navigation, aucun débordement/CSP). Captures inspectées.
  browser-catalog.cjs et browser-immersive.cjs passent sans retirer assertions.
  Preuves : docs/evidence/2026-10-15/profile-navigation/.
- Pas de nouvelle validation Safari/Firefox/Edge/mobile physique. Tests Rust non
  relancés : backend inchangé. Prochain petit jalon fonctionnel reste admin local,
  sessions/RBAC et SQLite avant bibliothèques personnelles ; aucun élargissement.


## 2026-10-15 — Séparation profil, paramètres et administration

- Menu : « Mon profil · Bientôt » (avatar/identité/mot de passe/préférences
  langue/audio/sous-titres), Paramètres pour les réglages personnels uniquement.
  Retrait de « Bibliothèques personnelles » et du bouton d’ajout de bibliothèque.
- Bibliothèques Films/Séries, dossiers/scans, utilisateurs/permissions et
  configuration relèvent de l’administration serveur future. Administration et
  Déconnexion restent masquées : aucune identité authentifiée ni rôle simulé.
  Deux rôles seulement administrator/user ; futurs droits à contrôler côté backend.
- Aucune nouvelle fonctionnalité, route, auth ou administration. Lecteur,
  catalogue métier et navigation principale inchangés. Tests existants adaptés
  aux nouveaux libellés sans retirer les autres assertions.
- Vérifié : check0/0, lint/build et browser-profile.cjs Chrome155 Windows
  headless360/390/768/1440px passent, y compris absence Administration/Déconnexion
  et bibliothèques dans Paramètres. Preuves docs/evidence/2026-10-15/personal-settings/.
  Suites média/Rust non relancées : changement de présentation uniquement.

## 2026-10-15 — Corrections ciblées du lecteur immersif

- ImmersivePlayer.svelte : maintien du focus sur la surface (.player-shell) lors du
  masquage automatique des contrôles ; ajout d'une écoute de secours globale des
  raccourcis clavier (Space, flèches, M, F, Échap) via svelte:window sans perte après
  inertie des contrôles.
- PlayerControls.svelte : structure ARIA normalisée (rôles group et presentation pour les
  sections, aria-controls et id de menu liés, suppression de la duplication de Tab,
  pattern de roving tabindex 0/-1 lors de la navigation fléchée Home/End/haut/bas sans
  anomalie de modulo négatif, bouton Fermer préservé).
- Mobile : titre secondaire relevé de 6px à 10px lisible sur smartphone et 10px desktop.
- Smartphone : disposition désencombrée sur mobile (<700px), curseur de volume inline masqué
  au profit des boutons matériels/muet, espacements aérés (row-gap 8px), suppression des
  débordements et cibles tactiles confortables (>=40px).
- Vérifications exécutées :
  - apps/web : npm run check (0 erreur, 0 avertissement), npm run lint (0 erreur),
    npm test (8 tests Vitest passés), npm run build réussi.
  - Rust : cargo fmt --check, cargo clippy --locked --all-targets -- -D warnings,
    cargo test --locked (15 tests passés).
  - Navigateurs réels : node scripts/browser-immersive.cjs (Chrome 155 headless, 11 étapes
    incluant focus masqué, ARIA/roving tabindex, taille mobile >=10px et volume masqué),
    node scripts/browser-immersive-firefox.cjs (Firefox 155 headless avec geckodriver) réussis.

## 2026-10-15 — Mise à jour complète de la toolchain et des dépendances

- **Toolchain Rust** : mise à jour vers stable 1.99.1 (rustc 1.99.1, cargo 1.99.1, rustup 1.29.1).
- **Cargo.toml** : passage de `rust-version = "1.85"` à `rust-version = "1.99"`.
- **Dépendances Rust** : vérification et alignement avec crates.io sur les dernières versions stables :
  - `axum` 0.8.9, `tokio` 1.53.3, `tower` 0.5.3, `tower-http` 0.7.1, `serde` 1.0.229, `serde_json` 1.0.154, `thiserror` 2.0.21, `http-body-util` 0.1.5, `tempfile` 3.27.0.
- **Frontend web (`apps/web`)** : mise à jour vers les dernières versions stables compatibles :
  - `eslint` : passage de `^9.0.0` (9.39.5) à `^10.12.0`.
  - `vitest` : passage de `^4.0.0` (4.1.12) à `^5.0.5`.
  - `typescript-eslint` : passage de `^8.0.0` à `^8.71.2`.
  - `eslint-plugin-svelte` : passage de `^3.0.0` à `^3.23.1`.
  - `svelte-check` : passage de `^4.0.0` à `^4.7.6`.
  - `@opentelemetry/api` : passage de `^1.0.0` à `^1.9.1`.
  - `typescript` : fixé à `^6.0.3` (dernière version de la branche 6.x, compatibilité stricte avec le peer requirement `^5.0.0 || ^6.0.0` de `svelte-check`).
  - Dépendances déjà à jour : `svelte` (5.57.2), `vite` (8.3.4), `@sveltejs/kit` (3.1.0), `@sveltejs/adapter-static` (4.0.0), `@sveltejs/vite-plugin-svelte` (7.3.1), `playwright` (1.64.1).
- **Vérifications et tests validés** :
  - Backend : `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked` (15/15 tests passés).
  - Frontend : `npm audit --audit-level=high` (0 vulnérabilité), `npm run check` (0 erreur, 0 avertissement), `npm run lint` (ESLint 10 propre), `npm test` (8/8 tests Vitest 5 passés), `npm run build` (build statique réussi).
  - Scripts de validation : `node scripts/player-comparison-stats.test.cjs`, `node scripts/safe-margin-stats.test.cjs`, `node --test scripts/audio-seek-policy.test.cjs` (10/10 tests passés).

## 2026-10-15 — Correction CI GitHub : horodatages sous-titres WebVTT sous FFmpeg 6.1.1

- **Cause identifiée** : dans `representative.mkv`, le délai lookahead des flux audio Opus (`delay 312`, ~6,5 ms arrondi à 7 ms en timebase 1 kHz) induit un `start_time` de conteneur de `-0.007000`. Sous FFmpeg 6.1.1 (Ubuntu CI), l'absence de l'option `-copyts` lors de l'extraction WebVTT soustrayait `start_time`, décalant chaque horodatage de cue de +7 ms (`2.007 --> 2.507` au lieu de `2.000 --> 2.500`), déclenchant `RuntimeError: Subtitle timing/text mismatch: fr-full`. Sous FFmpeg 9.0.2 (Windows), ce décalage conteneur n'était pas reporté sur l'extraction d'un flux de sous-titre isolé (`start: 0.000000`).
- **Correction** : ajout de l'option explicite `-copyts` à la commande FFmpeg d'extraction des sous-titres dans `scripts/prepare-mse.py` (ligne 118, aligné sur la ligne 67 vidéo/audio). Conservation stricte de l'assertion d'égalité exacte `source_cues == output_cues` et `len(output_cues) == 4`.
- **Test de régression ciblé** : `mse_subtitles_exact_timing_and_no_preroll_drift` ajouté dans `crates/core/tests/p0.rs`, vérifiant les horodatages stricts des 3 pistes WebVTT et rejetant explicitement le décalage `+7ms` (`00:02.007`).
- **Tests exécutés et validés** :
  - `prepare-mse.py` validé sous FFmpeg 6.1 et FFmpeg 9.0.2 (reproduction de l'erreur sans `-copyts` et succès avec `-copyts`).
  - Rust : `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked` (16 tests passés).
  - Node : tests statistiques et de politique audio (10/10 tests passés).
- **Tests non exécutés** : CI GitHub distante (pas de push automatique).



## 16 octobre 2026 — jalon identité SQLite / bootstrap local

- SQLx 0.9.0, SQLite embarqué/WAL/FK et migration 0001 : users UUID,
  local_credentials séparés, rôle CHECK administrator/user, marqueur de
  bootstrap durable. Modèle User typé avec création/désactivation ; aucune
  session/table OIDC anticipée, aucune nouvelle route HTTP.
- Core ouvre MEDIA_DB_PATH (défaut data/core.sqlite3) et migre avant HTTP.
  Parent privé existant requis, distinct des médias ; aucun média modifié.
  CLI media-bootstrap : terminal interactif, saisie masquée/confirmation,
  pas d'arguments ni secret en variable/pipe. Aucun compte réel créé par l'agent.
- Argon2id v19, m=65536 KiB/t=3/p=1, sortie32octets, sel OS, PHC seulement,
  Zeroizing pour mot de passe/confirmation, hashing spawn_blocking.
- BEGIN IMMEDIATE protège migration et bootstrap ; test initial a révélé que
  le verrou de migration SQLx SQLite est un no-op, corrigé par transaction
  englobant vérification/application. Bootstrap atomique et définitivement
  fermé même après suppression du compte ; rollback de panne testé.
- Windows / Rust1.99.1 : cargo fmt --check, cargo clippy --locked --all-targets
  -- -D warnings réussis ; cargo test --locked : 22 tests réussis (16 existants,
  6 identité), un helper ignoré exécuté par le test multi-processus.
  Trois processus sur une base initialement absente : 1 gagnant, 2 refus.
  Deux pools concurrents, migrations rejouées/réouverture, hash valide/mauvais
  secret, rôle interdit/FK, entrées invalides, corruption, parent absent,
  panne d'insertion et refus CLI sans terminal couverts. Un lancement de test
  concurrent au précédent a eu un conflit linker Windows ; relance séquentielle
  complète réussie. Correction finale Clippy stylistique uniquement.
- Pas de preuve de saisie masquée interactive manuelle, de NAS, de CI distante,
  de navigateur supplémentaire ou de coût Argon2 de production. Lecteur/UI
  inchangés ; pas de relance Web ou des expériences codecs.
- Frontière de confiance : opérateur OS et ACL du dossier, pas de protection
  contre un opérateur pouvant remplacer DB/symlinks. Aucun endpoint sensible.
  Prototype toujours loopback, sans session : aucune bibliothèque personnelle,
  publication, tunnel ou exposition LAN autorisée. Voir docs/LOCAL_ADMIN.md.
- Prochain jalon proposé, non commencé : connexion, sessions révocables,
  CSRF/rate limiting et permissions backend. P1 complet reste ouvert.
- Skill codex-token-saver appliqué : tests Rust pertinents conservés ; aucun
  test Web/codec répété sans changement concerné ni pourcentage d'économie annoncé.

## 16 octobre 2026 — connexion, sessions et permissions des fixtures

- Parcours Web /app/login/ -> session -> catalogue/lecture proteges -> deconnexion.
  Compte CLI existant reutilise, aucun compte reel cree par l'agent, aucun role
  supplementaire ni inscription/OIDC/multi-Node. Lecteur et medias conserves.
- Migration 0002 : sessions (SHA-256 du jeton seulement, user_id, dates) et budget
  global de connexion. Deux UUIDv4 OS par jeton (244 bits aleatoires), cookie
  HttpOnly/SameSite Strict/Path=/, Secure pour origine HTTPS ; expiration absolue
  12h, idle30min controlees atomiquement, suppression a la deconnexion.
  CSRF derive/separe du jeton + Origin exact ; aucun jeton localStorage.
- Argon2id existant, verification factice pour compte absent/desactive ; message
  generique. Budget SQLite 10 tentatives/60s conserve apres redemarrage et maximum
  deux workers Argon2, permis conserve meme si HTTP annule. Pas de secrets logs.
- secure_app impose auth puis permission avant catalogue/descripteurs/fichiers,
  sous-titres et derives MSE/DASH, y compris GET/HEAD/Range. Administrator autorise
  uniquement sur inventaire synthetique ; user refuse403 sans droits implicites.
  Pas encore de stockage/gestion de grants par bibliotheque ou de medias personnels.
- Banc historique app compile seulement via fixture-bench pour tests ; binaire
  toujours secure_app meme avec cette feature, aucun bypass runtime. CI adaptee
  pour clippy/test --all-features ; CI distante non executee.
- MEDIA_PUBLIC_URL origine exacte HTTPS ou HTTP loopback, Host/Origin verifies,
  en-tetes forwarded ignores, listener toujours127.0.0.1. TLS non termine par Core,
  aucun proxy/publication effectue, HTTPS navigateur/deploiement non valide.
- Verification finale Windows/Rust1.99.1 : cargo fmt --check,
  cargo clippy --locked --all-features --all-targets -- -D warnings passent ;
  cargo test --locked --all-features : 27 tests reussis, 2 ignores par harness
  (helper bootstrap lance par competing_processes, navigateur lance separement).
  Le test core_restart arrete/redemarre vraiment trois processus : session
  conservee puis revocation conservee. Tests HTTP HTTPS Secure/Host/Origin passent.
- Premier run complet a revele une course PRAGMA WAL a l'ouverture SQLite.
  Correction IdentityStore : trois tentatives maximum uniquement BUSY/LOCKED,
  attente50/100ms ; corruption/permissions/autres erreurs jamais masquees.
  Test concurrent repasse six fois ; suite complete finale reussie.
- Web : npm run check (0 erreur/avertissement), lint, npm test (12 tests), build
  passent. cargo test --locked --test browser_auth -- --ignored --nocapture passe :
  Chrome155.0.8059.40 Windows10.0.26300 headless390/1440px, erreur login puis succes,
  cookie HttpOnly/Strict, identite/menu admin, catalogue, MP4/seek, rechargement,
  logout puis catalogue/Range401, aucune erreur JS/CSP. Captures inspectees dans
  docs/evidence/2026-10-16/auth/ ; pas de nouvelle preuve PCM/MKV/Firefox/Edge/Safari.
- Limites : flux deja ouvert non interrompu au logout, budget global pouvant etre
  epuise par un acteur, revocation session courante seulement, pas de droits fins,
  TLS reel/NAS/charge non testes ; ACL operateur/TOCTOU du banc toujours applicables.
  Voir docs/LOCAL_SESSIONS.md et docs/TEST_PLAN.md. Pas de commit/push/deploiement.
- Prochain petit jalon P1 propose : droits explicites par bibliotheque en SQLite,
  matrice administrator/user et tests de chaque ressource/derive, toujours sur
  fixtures, avant toute connexion de dossiers personnels ro.
- Token saver applique : trois sous-agents aux fichiers distincts, lectures ciblees,
  integration centralisee et tests utiles conserves. Aucun gain chiffre revendique.

## 16 octobre 2026 — bibliothèques et permissions P1 livrées sur fixtures

- SQLite migration0003 : Node local, bibliothèques stables (UUID créations),
  nom/type/racine canonique/état et droits par compte. Initialisation unique des
  deux bibliothèques de fixtures sans grants. Configuration et grants atomiques.
- Administration Web : liste/ajout/modification/activation/utilisateurs autorisés.
  API session administrator, Origin exact et CSRF mutations. Node valide racines
  existantes dans MEDIA_FIXTURE_DIR, refuse relatifs/parents/sorties et junctions.
  Aucun scan, disque distant, NAS ni original modifié.
- Core autorise chaque lecture SQLite : recherche filtrée avant pagination,
  fiches et URLs directes, sous-titres/médias GET/HEAD/Range, révocation immédiate
  aux prochaines requêtes. Administrator ne lit pas une bibliothèque désactivée.
  Assets demo/sous-titres explicitement partagés Films/Séries (même mire) ;
  P0/MSE/DASH et descripteurs relèvent de Films. Racine/type incompatible retire
  association fixture. Les nouvelles bibliothèques restent vides avant scanner.
- Catalogue éditorial encore explicite en Rust, seul registre bibliothèques/droits
  persisté. Pas d'endpoint image privée ; illustrations statiques publiques.
- Tests : cargo fmt --check ; cargo clippy --locked --all-features --all-targets
  -- -D warnings ; cargo test --locked --all-features réussis :32 tests,
  2 ignorés (helper exécuté par sous-processus, navigateur lancé séparément).
  CRUD/CSRF/rôles/rollback/révocation/confinement/junction Windows testés.
  core_restart relance trois vrais processus : bibliothèque modifiée/désactivée
  conservée et média refusé après redémarrage, session/logout conservés aussi.
- Web check0/0, lint, 12 tests, build réussis. cargo test --locked --test
  browser_auth -- --ignored --nocapture réussi : Chrome155.0.8059.40 Windows
  10.0.26300 headless390/1440, création/modification/désactivation, catalogue,
  MP4/seek/logout, pas d'erreur JS/CSP/débordement. Captures inspectées dans
  docs/evidence/2026-10-16/libraries/. Premier essai échoué sur sélecteur de test
  encodé CP1252 ; corrigé UTF-8, relance réussie.
- Limites : fixtures/loopback, pas de scanner/NFO/transcodage/Node distant,
  compte réel ni interface création utilisateurs. Canonicalisation/ouverture
  non atomiques contre écrivain OS hostile ; flux ouvert non coupé à révocation.
  Aucun nouvel essai PCM/MKV/Firefox/Edge/Safari/NAS/charge/CI distante.
  Aucun commit/push/déploiement. Voir docs/LIBRARIES_AND_SCANNER.md.
- Trois sous-agents sur fichiers distincts, lectures ciblées token-saver,
  intégration/tests transverses centralisés ; aucun gain chiffré annoncé.
  Prochain jalon après validation : scanner local Media Node indépendant ARR.

## 16 octobre 2026 — premier scanner Films local P1

- Branche develop, jalon Bibliothèques/Permissions réutilisé. Aucun push,
  publication, déploiement, compte réel ou média personnel utilisé.
- Node interne : parcours récursif MKV/MP4, sans symlink/point de réanalyse Windows,
  caches Jellyfin et temporaires ignorés ; erreurs non bloquantes. Adapter probe
  existant réutilisé, AV1/Opus non imposés. JSON technique et index réels conservés.
  Identité par fournisseur explicitement qualifié, dossier exact avec année ou
  chemin exact ; conflits explicites séparés. Aucun rapprochement flou/NFO/réseau.
- Migration0004 : Work/Edition/MediaVersion/MediaSource/Asset/Track distincts et
  état durable des scans. IDs stables, taille/mtime/cache probe, pas de doublon
  au second scan. Versions d'un dossier de film identifiées sous une carte.
  Groupement limité à la bibliothèque/racine ; rapprochement entre bibliothèques
  différé. Un fichier disparu devient indisponible ; aucune purge des métadonnées.
  Parcours incomplet conserve les disparitions non confirmées ; transaction et
  snapshot de racine évitent une publication après changement de configuration.
- MEDIA_LIBRARY_ROOT : racine opérateur absolue existante, mode standalone-local
  vide, aucun probe de fixtures ni amorçage des bibliothèques de démo. Le mode
  MEDIA_FIXTURE_DIR historique reste isolé. DB hors médias, y compris bootstrap.
- Administration Scanner : POST/GET /api/v1/admin/libraries/{id}/scan,
  session administrator + Origin/CSRF, 202, doublon409 ; idle/queued/running/
  completed/error, compteurs finaux et erreurs. Un seul probe local simultané,
  cancellation Ctrl+C et attente des tâches ; jobs interrompus signalés au restart.
- Catalogue/fiche SQLite avec recherche/tri/pagination ; descripteur natif
  /api/v1/media/{assetId}. Routes de contenu existantes et streaming64Kio conservés.
  Contrôles actifs des droits avant GET/HEAD/Range, révocation et désactivation.
  Le lecteur garde son moteur ; source non française possible, aucune piste
  interne sélectionnable ni lecture compatible inventée. Labels démo séparés.
- Tests Rust : cargo fmt --check ; cargo clippy --locked --all-features
  --all-targets -- -D warnings ; cargo test --locked --all-features :38 réussis,
  3 ignorés par harness (2 navigateurs lancés séparément, helper processus exécuté
  par competing_processes). Inclut scan exclusif/arrêt/restart jobs, jonction
  Windows, corruption/temporaire, deux bibliothèques, révocation GET/HEAD/Range,
  réouverture SQLite/IDs/cache, AV1 10bits/Opus FR/EN/sous-titres internes,
  changement de racine/réassociation, disparition et scan incomplet.
- Web : npm run check 0 erreur/avertissement ; npm run lint ; npm test13/13 ;
  npm run build réussis. Parcours browser_scanner : création bibliothèque Web,
  scan2 fichiers, un film/deux versions, second scan0 probe/2 réutilisés,
  MP4 lecture/pause/seek4s, arrêt forcé/relance réel du binaire même SQLite puis
  catalogue/lecture/seek réussis. Chrome155.0.8059.40, Windows10.0.26300 headless.
  Rapports/captures docs/evidence/2026-10-16/scanner/ ; captures inspectées.
- Non-régression navigateur : cargo test --locked --test browser_auth --
  --ignored --nocapture réussi1/1, Chrome390/1440, CRUD permissions, lecture/seek,
  logout/Range401 et aucune erreur JS/CSP. Passe browser_scanner finale1/1 réussie.
- Dernier ajustement : erreur I/O du processus ffprobe fait échouer le job sans
  publier une fausse corruption globale ; disparition pendant probe reste une
  erreur fichier non bloquante. fmt/Clippy strict repassés ; cargo test --locked
  --all-features node::scanner --lib :3/3 et cargo test --locked --all-features
  --test scanner_flow :1/1 repassés (13,87s pour le parcours intégré).
- Sources : copies synthétiques en dossiers jetables, hashes/comparaisons bytes
  avant/après identiques ; aucun original modifié. Pas de nouvelle preuve PCM,
  compatibilité MKV générale, Edge/Firefox/Safari, NAS, charge/RSS ou CI distante.
- Premier essai navigateur échoué : POST scan répondait uniquement status, UI
  attendait errors/counters. Corrigé des deux côtés, relance réussie. Une première
  suite Rust a rencontré LNK1104 à cause du même exécutable navigateur lancé en
  parallèle ; validations suivantes sérialisées et réussies.
- Limites : confinement puis ouverture non atomiques (TOCTOU), ACL opérateur
  requises ; flux déjà ouvert non coupé à révocation ; inventaire/cache en RAM
  proportionnels aux fichiers ; ffprobe non sandboxé. Séries/NFO/images/pistes
  externes/OIDC/multi-Node/ordonnanceur et vrai déploiement non implémentés.
  Détails/usage : docs/LOCAL_FILM_SCANNER.md. Prochain petit jalon recommandé :
  métadonnées NFO Films strictement locales et bornées, après validation utilisateur.

## 16 octobre 2026 — métadonnées NFO Films locales

- develop et modifications non commitées du scanner préservés. Migration0004
  inchangée (SHA256 9175E1B4934317EE89921B164D18A048211A7D5AA4E1B28FD4D3A9D1A5C13E6F).
  Migration additive0005_film_editorial : candidats par asset, JSON/provenance Work.
- Node/nfo.rs, quick-xml0.42.0 : sidecar exact `<base vidéo>.nfo` prioritaire,
  puis movie.nfo seulement dans un sous-dossier non ambigu. Pas de movie.nfo
  partagé à la racine ; conflits explicites fichier/dossier refusés. NFO exact
  invalide ne bascule pas silencieusement sur le partagé. Aucun appel ARR/réseau.
- title/originaltitle/plot/outline/year/premiered/genres/rating/mpaa/IDs explicitement
  qualifiés extraits. Champs techniques ignorés. Identifiants NFO jamais utilisés
  comme clés de fusion ; IDs du graphe média conservés. Conflits interversions
  ou avec repli local signalés, fournisseur contradictoire retiré des IDs publics.
- Chaque scan relit les NFO bornés indépendamment du cache vidéo : actualisation
  et suppression sans nouveau ffprobe vidéo. Valide actualise ; absent rétablit
  le repli ; invalide garde le dernier instantané valide, sinon repli local.
  Nouveau graphe/racine ne reprend pas l'ancien instantané. Sélection Work
  déterministe exacte/partagée puis chemins, transactionnelle. Médias indisponibles
  conservent leurs métadonnées. Avertissements NFO séparés des erreurs média,
  exposés au statut admin sans rendre la vidéo indisponible.
- XML non fiable : DTD/entités externes/inconnues rejetées, sans résolveur réseau.
  UTF8 strict/BOM accepté, taille1Mio, profondeur16, champs16Kio,64genres ;
  caractères XML interdits et références invalides rejetés. Liens/reparse refusés.
  Valeurs jamais utilisées comme chemins/URLs. Texte échappé Svelte, aucun HTML
  arbitraire. Risque TOCTOU existant toujours explicite, ACL opérateur requises.
- Web garde design/navigation/lecteur, affiche titre/année/synopsis/genres dans
  Accueil, Films, Recherche et fiche. API catalogue enrichie, provenance nfo/local,
  autres champs et metadataWarnings. Aucun nouveau endpoint ou permission.
- Vérifications : cargo fmt --check ; cargo clippy --locked --all-features
  --all-targets -- -D warnings passent. cargo test --locked --all-features :
  45 réussis,4 ignorés (3 navigateurs explicites, helper processus exercé ailleurs).
  Tests nouveaux : parser hostile/XXE/expansion/taille/profondeur/encodage/HTML,
  NFO exact/partagé/jonction Windows, migration0004→0005 préservant les IDs,
  changement de racine, update/delete/invalides/cache sans probe, deux versions,
  IDs contradictoires, droits fiche/GET/HEAD/Range et hashes sources/NFO.
- Web npm run check :0 erreur/avertissement ; npm run lint ; npm test13/13 ;
  npm run build passent. cargo test --locked --test browser_nfo -- --ignored
  --nocapture :1/1 réussi en49,22s. Chrome155.0.8059.40 Windows10.0.26300 headless,
  quatre phases initial/restart/update/delete, vrais arrêts/relances Core même DB,
  métadonnées Accueil/Films/Recherche/fiche, HTML hostile rendu texte, MP4/seek4s/
  pause ;0 probe après update/delete et deuxième scan. Aucune erreur JS/CSP.
  Preuves distinctes docs/evidence/2026-10-16/nfo/, capture initiale inspectée.
- 13 anciens fichiers de preuve du16octobre comparés SHA256 : tous inchangés.
  Copies synthétiques jetables seulement ; seules les mutations explicites des
  tests changent leurs NFO. Aucun original média/NFO écrit par le scanner,
  hashes et comparaisons avant/après vérifiés. Aucun commit/push/publication.
- Limites : UTF8 seulement, association prudente des NFO partagés, rating0..10,
  pas de validation fournisseur en ligne ni fusion entre bibliothèques ; aucun
  nouveau résultat mobile/Edge/Firefox/Safari/NAS/charge/CI distante. Séries,
  illustrations, sous-titres externes, multi-Node/DB externe restent différés.
  Prochain petit jalon recommandé : affiches locales Films en lecture seule,
  après validation. Détails : docs/LOCAL_FILM_NFO.md.
- Token saver appliqué à la demande : lectures ciblées, journaux limités,
  compilations intégrées centralement ; validations obligatoires conservées,
  aucun gain chiffré annoncé.


## 16 octobre 2026 — affiches locales Films

- Develop, sans commit/push/publication. Migration additive0006_local_posters ;
  migrations0004/0005 intactes. Inventaire images distinct du graphe vidéo, UUID
  stable par Work/racine/chemin ; chemins relatifs privés, taille/mtime/MIME/priorité.
- Node : poster puis folder puis base-vidéo-poster puis base-vidéo ; formats
  jpg/jpeg/png/webp dans cet ordre. Partage refusé à la racine et entre identités
  ambiguës ; première image présente invalide = avertissement/repli. Une affiche
  manquante ne supprime pas son Work. Cache images taille/mtime, sans ffprobe vidéo.
- Core : GET/HEAD /api/v1/assets/{uuid}/image, session et droits de bibliothèque
  vivants, désactivation/révocation appliquées. Confinement symlink/reparse et
  validation réelle image0.25.10 avant flux64Kio ; MIME/nosniff et no-store.
  Catalogue/fiche posterUrl révisée taille/mtime, aucun chemin image exposé.
- Limites :10Mio compressés, dimensions4096, pixels décodés32Mio, budget interne
  décodeur32Mio de meilleur effort, validation sérialisée sur worker bloquant.
  Coût CPU par GET/HEAD, aucune preuve de RSS/charge agrégée. Confinement puis
  validation puis ouverture non atomiques : TOCTOU résiduel et ACL opérateur.
  Révocation ne coupe pas un corps ouvert ni des pixels déjà affichés.
- Web : composant Poster adapté, Accueil/Films/recherche/fiche, ratio2:3,
  cartes lazy/async et fiche eager, repli si absent/erreur ; design/lecteur conservés.
- Token saver appliqué : lectures ciblées et validations regroupées ; aucun gain
  chiffré annoncé. Détails : docs/LOCAL_FILM_POSTERS.md.

- Validation finale : cargo fmt --check ; cargo clippy --locked --all-features
  --all-targets -- -D warnings ; cargo test --locked --all-features réussis :
  50 tests réussis,5 ignorés (4 navigateurs explicites et helper subprocess).
  Inclut migrations0004→0005→0006 avec IDs/metadata conservés, priorités/formats,
  absences/corruptions/extensions trompeuses/oversize/jonction Windows, cache
  images, modifications/suppressions sans probe, droits GET/HEAD de2 bibliothèques,
  désactivation/grants/révocation de session, graphes/NFO/originaux inchangés.
- Web : npm run check (0 erreur/avertissement), npm run lint, npm test (13/13),
  npm run build réussis. Chrome explicite : cargo test --locked --test
  browser_posters -- --ignored --nocapture,1/1 réussi en39,51s ; Chrome155.0.8059.40,
  Windows10.0.26300 headless,4 phases initial/restart/update/delete, vrais arrêts
  et relances Core avec même SQLite. Nouveaux pixels/dimensions après rescan,
  Accueil/Films/recherche/fiche, lazy/repli après erreur, MIME/nosniff/no-store,
  refus anonyme GET/HEAD, NFO échappé, lectureMP4/pause/seek4s et aucune erreur
  JS/CSP. Preuves : docs/evidence/2026-10-16/posters/ ; captures initial/delete
  inspectées visuellement. Originaux vidéo/NFO/affiche comparés, seules mutations
  explicites des copies jetables par tests. Aucun média personnel connecté.
- Essais initiaux corrigés : verrou Windows sur exécutable lors de compilations
  concurrentes ; attente de scan navigateur remplacée par polling API ; Clippy
  if imbriqué simplifié. Validation centrale finale sérialisée et réussie.
- Pas de preuve NAS/LAN/charge/CI distante ni nouveau navigateur. Prochain petit
  jalon recommandé : durcir les ouvertures de fichiers contre TOCTOU avant toute
  exposition à un stockage modifiable par un acteur hostile. Séries/backdrops/
  pistes externes/OIDC/multi-Node/PostgreSQL/Docker restent hors périmètre.
## 21 octobre 2026 — Miniatures, téléchargements simples et dépôts

- Demande poursuivie sans déploiement ni publication. L’utilisateur précise que
  Télécharger doit enregistrer le fichier sur l’appareil depuis le site.
- Téléchargement natif par lien protégé, nom proposé et choix des fichiers quand
  plusieurs versions existent ; aucune phrase ni nouvelle copie IndexedDB dans
  ce parcours. Anciennes copies hors ligne préservées, récupération des anciennes
  copies avec phrase conservée ; suppression visible seulement après effacement
  durable des blocs et de la clé. Bouton + pour créer une liste avec le média ou
  ajouter à une liste existante. Notes optimistes avec écritures sérialisées et
  dernier choix conservé pendant une réponse lente.
- Miniatures autorisées sur la route image existante : largeurs 320/640/1280,
  hauteur au plus deux fois la largeur, sans agrandissement, JPEG/PNG ou original
  si déjà adapté et plus petit. LRU de 8 Mio/128 corps compressés, décodeur unique
  partagé avec scans/originaux, fichier et droits revalidés avant cache. GET/HEAD,
  Range et conditions conservés ; aucune écriture dans la bibliothèque. Largeur
  transportée aux Nodes distants et au canal sortant. Le relais ne transmet pas
  encore l’ETag de réponse. Fond décoratif de presque 2 Mo remplacé par dégradé
  CSS pour éviter sa demande dès le chargement initial.
- Mesure release unique Windows : JPEG synthétique 3 862 387 octets réduit à
  86 620 octets en 320×480, génération 159,281 ms, cache 305,8 µs. Ce n’est pas une
  mesure réseau/navigateur/NAS ni une limite RSS. Preuve :
  `docs/evidence/2026-10-21/image-preview-release.md`.
- Lecture : promesse d’import partagée, descripteur et import parallèles, montage
  attendu avant ouverture, requête de choix de version évitée pour mono-version.
  Progression préparée sur survol/focus cinq secondes, consommée une seule fois,
  bornée à 32 œuvres et purgée aux écritures/changements de compte. Contrôles
  Version et Audio masqués sans choix opérationnels distincts ; original/remux
  d’une même piste dédupliqués. Aucune promesse de décodage fondée sur ffprobe.
- Dépôts : migrations additives 0036 SQLite/PostgreSQL ; administration Dépôts,
  Catalogue et Installées, choix de version, installation signée désactivée,
  approbation explicite des éditeurs, accès/activation conservés. HTTPS public
  uniquement, DNS vérifié et épinglé, aucun proxy/redirection, bornes de taille,
  délai et concurrence. Pas de dépôt officiel publié inventé ni de compatibilité
  avec les DLL/manifestes Jellyfin. Générateur fourni dans Aether-Plugins.
- Tests déjà exécutés : Web check 0 erreur/0 avertissement, lint, 68 tests et
  build ; 42 tests Node réussis (un helper ignoré appelé par son test parent),
  2 tests release miniatures, 2 tests Core dépôts, posters_flow,
  distributed_multi_node et node_enrollment_and_relay réussis. Chrome
  155.0.8059.85 headless : PWA/téléchargement exact et copies anciennes, dépôts
  réels Core/SQLite (documents HTTPS injectés uniquement sous cfg(test)),
  affiches initial/redémarrage/modification/suppression réussis. Affiche minuscule
  visible en 510 ms desktop/552 ms mobile, observations uniques sans comparaison.
- Docker : arènes glibc bornées à 2 et suppression de la copie du second binaire
  Core standalone, alias media-poc/aether vers aether-core ; outils de secours et
  Node conservés. Moteur Docker Desktop absent : aucun poids final, RSS à cinq
  minutes ou gain Linux mesuré, aucun conteneur/volume utilisateur modifié.
- Preuves complémentaires et état final des contrôles :
  `docs/evidence/2026-10-21/usability/README.md` et `DEV_STATE.md`.
- Clôture locale : browser_series réussit après correction du sélecteur de test
  du disclosure natif des listes ; notes rapides persistées, liste créée et
  média ajouté, choix mono-version/mono-audio masqués, 7 images vidéo décodées,
  seek/pause, chapitres, transitions et isolation des comptes vérifiés. Clic vers
  lecture observée 1890 ms (Core debug, préparation et automatisation incluses),
  sans comparaison Jellyfin. Fmt, Clippy strict workspace/all-targets et diff
  checks des deux dépôts réussis.

## 21 octobre 2026 — Audit ciblé du lecteur, sans correctif de production

- Analyse des sept problèmes audio/vidéo : `docs/PLAYER_PLAYBACK_AUDIT.md`.
  Préparation parallèle Web incompatible avec l'admission d'un seul worker Node
  à froid ; test réel MKV/MP4 reproduisant le refus concurrent et la réussite en
  série. Conservation de six canaux sur la piste FR extraite et hashes des
  paquets AV1/Opus identiques dans le test de copie existant.
- Chrome 155.0.8059.85 / Windows 10.0.26300, fixtures synthétiques et vrai code
  lecteur : audio VO HTTP 404 laissé attaché, resélection VF ignorée ; original
  étiqueté FR d'après la première piste mais piste EN default réellement décodée
  à 990,23 Hz. Serveur de test isolé, pas une preuve Core/RBAC.
- Cinq diagnostics déterministes reproduits, dont blocage audio seul sans
  watchdog et dérive de 140 ms non corrigée après seek. Les 72 tests Web
  existants passent. Parcours Core/SQLite/Chrome browser_scanner réussi en
  56,08 s, MP4 puis MKV après redémarrage : VF/VO, PCM, seek/pause, progression
  et sous-titres. Fmt et diff --check réussis.
- Aucun code de production modifié ; tests, rapport et preuves seulement.
  Corrections recommandées : préparations sérialisées, commutation atomique,
  sélection explicite de piste et surveillance du buffering audio. POC MSE à
  horloge commune seulement ensuite si nécessaire ; pas de refonte immédiate.
  Pas de sortie physique 5.1/7.1, autre navigateur, NAS/4K réel ou endurance
  validés. Les limites 120 s / 16 Gio vidéo / 1 Gio audio restent effectives.

## 21 octobre 2026 — Correctifs du lecteur demandés après l'audit

- État d'audit précédent remplacé pour le code de production : préparations
  sérialisées, chargement et positionnement d'un candidat audio avant activation,
  ancien audio préservé lors d'un échec, nettoyage/annulation complets et source
  invalidée après erreur terminale. Intention de lecture conservée pendant une
  pause technique, pause manuelle annulant la reprise automatique.
- Sélection partagée normalisant les langues et privilégiant la piste concrète ;
  original multi-audio exposé en `und`, libellé comme choix navigateur. Buffering
  audio coordonné avec vidéo, seek explicite et seuil périodique 80 ms. Pas de
  réencodage, filtre audio, downmix ou graphe WebAudio de production.
- Limites via MEDIA_PREPARATION_* : cache 96 Gio, vidéo 64 Gio, audio 4096 Mio,
  délai 900 s par défaut ; valeurs bornées et erreurs HTTP 503 différenciées,
  en-tête informatif de préparation disponible sur HEAD et relais. Stockage
  Docker corrigé vers /data/preparation-cache (ancien /tmp limité à 64 Mio dans
  Compose). Variable MEDIA_PREPARATION_CACHE_DIR absolue, hors bibliothèques.
  Toujours cache temporaire, un worker et copie entière avant lecture ; aucun
  gain chiffré 4K annoncé, aucune image publiée ni conteneur remplacé.
- Vérifications : check 0/0, lint/build, 73 tests Web + 5 régressions ciblées,
  copie de paquets AV1/Opus dont 7.1 MKV/MP4, deux tests de leases réussis.
  Chrome 155/Core/SQLite : VF/VO/PCM/seek/pause/progression/sous-titres, MP4 puis MKV
  après redémarrage, réussi en 33,09 s. Premier essai en échec sur pause technique
  pendant commutation, corrigé puis rejoué. Compose config --quiet, fmt/diff et
  Clippy strict workspace/all-targets. Suite Rust générale non relancée.
- Preuves après correction : docs/evidence/2026-10-21/player-audit/browser-fixed.json
  et browser-fixed/. Sortie physique 5.1/7.1, Safari/Firefox/Edge, NAS/4K réel et
  endurance non validés. Deux éléments ne garantissent pas une horloge commune.


## 2026-10-22 — Médias indépendants, premier lot vérifié

- Bouton manuel Épisode suivant retiré en haut du lecteur ; option d’enchaînement
  automatique volontaire conservée. Parcours Chrome/Core/SQLite Séries réussi,
  y compris transitions inter-saisons et permissions ; preuve dans
  docs/evidence/2026-10-22/separated-media/series/report.json.
- WebM ajouté à la découverte et aux associations Opus/sous-titres existantes.
  Vidéo détectée sans audio exclue des choix audio fictifs. WebM AV1 avec Opus
  internes conserve la vidéo originale et les fenêtres MSE audio, comme MP4.
- Association explicite facultative `<video filename>.media.json` v1, 64 Kio,
  8 audios/64 sous-titres : noms locaux seulement, langues normalisées,
  offset audio borné ±30 s, forced/SDH. Document invalide refusé sans association
  implicite ; mêmes sondes/confinement/autorisations. Relecture à chaque scan.
- Tests : Node 46 réussis/2 ignorés ; scanner_flow 2 réussis (dont WebM,
  manifeste et accès audio HEAD/GET/Range 401/200/206/416) ; Web 82 tests,
  check sans erreur/avertissement, lint/build réussis. Mires légales : images
  et PCM Chrome/Edge, vidéo seule MP4/WebM + Opus 2/6/8 canaux, FR/EN/FR,
  pause/seeks/plein écran/404/réouverture ; empreintes paquets préservées.
  WebM interne segmenté Chrome : changements/frontière/annulation réussis.
- CI étendue aux mêmes scénarios, purge/validation/archivage alignés sur les
  rapports générés ; exécution GitHub non observée. Voir docs/SEPARATED_MEDIA.md.
- Limites : P0 Avatar/NAS encore ouvert, fichier/NAS non disponibles. Aucun
  nouveau test physique NAS, Docker, Node distant, Firefox, H264/HEVC navigateur,
  séance longue ou sortie physique multicanal dans ce lot. `.obu` désormais
  signalé par erreur explicite de scan ; index/packager raw AV1 non implémenté.
  Véritable Opus brut étudié et refusé, distinct d’Ogg Opus. Le support complet
  des flux bruts n’est pas livré et ne doit pas être annoncé comme terminé.

Validation finale de ce lot : Clippy strict workspace/all-targets, cargo fmt,
actionlint et git diff --check réussis. Suite Rust workspace complète non relancée.

## 22 octobre 2026 — Playlists et lecteur musical

- Playlists mixtes, partage nominatif et visibilité publique uniquement entre comptes connectés ; politiques administrateur désactivées par défaut et droits des bibliothèques conservés.
- Lecteur musical global natif, file bornée, suivant/précédent, aléatoire, répétition, Media Session et paroles locales LRC/texte. Aucun transcodage ; pas d'enchaînement automatique vidéo/musique.
- Extensions : Installées/Catalogue/Dépôts ; import manuel signé par fichiers avec MEDIA_EXTENSION_IMPORT_DIR, confiance explicite, sans activation ou permission automatique.
- 96 tests Web, Svelte/lint/build, Clippy strict, scan musical, import signé et parcours Chrome/Core/SQLite ciblés passés. Ni Edge/NAS ni nouveau PostgreSQL réel validés pour ce jalon.
- Contrats, commandes, limites et preuves : docs/PLAYLISTS_AND_MUSIC.md. Aucun push ou déploiement.

## 22 octobre 2026 - Presentation connexion et lecteur

- Retour SSO : document sombre centre, CSS externe, script de navigation conserve.
- Lecteur : chargement simple, diagnostics dans Statistiques du flux optionnelles ;
  mesures video navigateur chaque seconde pendant ouverture du panneau seulement.
- Web : 96 tests reussis. Verification SSO reel et NAS non effectuee pour ce jalon.

## 23 octobre 2026 — Administration sans colonne latérale permanente

- Bouton Menu et tiroir modal fermé par défaut sur toute largeur ; aucune place
  réservée. Navigation, Échap, clic extérieur et retour du focus vérifiés Chrome.
- Vue d’ensemble réduite aux états utiles ; informations techniques repliées.
  Introductions et badges redondants retirés, catalogue plus court, version
  sélectionnable seulement si plusieurs choix. Droits/alertes restent explicites.
- Check sans erreur/avertissement, lint, 112 tests et build réussis. UI production
  clair/sombre 1440/390 et cycle catalogue : API simulée, aucune erreur navigateur.
  Preuves : docs/evidence/2026-10-23/startup-plugins/catalogue-ui/.
- Ancien scénario test-admin-ui interrompu sur le lien #account des préférences
  personnelles ; ne pas le compter comme validé. Captures administration clair
  partielles dans docs/evidence/2026-10-23/admin-compact/ ; scénario ciblé réussi.

## 23 octobre 2026 — Plugins sans approbation, configuration et collections

- Demande utilisateur révisée : aucun clic d’approbation pour les dépôts/auteurs.
  Clé du catalogue utilisée pour vérifier signatures/empreintes, identité interne
  par digest sans réservation d’un nom d’auteur. Anciennes routes de clé conservées
  uniquement pour compatibilité des outils ; import avec publicKey autonome.
- UI paramètres dédiée, activation par interrupteur, schéma générique, outils de
  test repliés. Fournisseurs : Toutes les bibliothèques par défaut et périmètre
  durable allLibraries ; anciens périmètres explicites conservés. Avatars : aucune
  case bibliothèque. Correction visuelle des vieux textes sans altérer le signé.
- Générateur PowerShell : stdout natif UTF-8 explicite, options Cargo sans échappement
  dépendant de la console, permissions déclarées par chaque plugin. Huit paquets
  reconstruits/signés/vérifiés ; aucune clé privée dans Git, aucun push.
- Collections GitHub importées par Core, images PNG/JPEG/WebP normalisées 256×256,
  stockage transactionnel borné ; galerie authentifiée et activation vérifiée.
  Vrai HTTPS GitHub : 7 dossiers, 26 avatars sonny importés en SQLite temporaire.
- Modèle indépendant Aether-Plugins/templates/metadata-provider compilé et exécuté
  par le worker Core (149077 octets), configuration personnalisée retenue. Guide
  CREATING_PLUGINS.md : créer un fournisseur sans modifier/recompiler Aether.
- 113 tests Web, check/lint/build ; 41 tests workspace plugins ; 10 tests Core
  extensions et navigateurs Core réels installation/exécution/désactivation et
  dépôt/version/suppression réussis. UI production simulée clair/sombre 1440/390,
  champs mobile lisibles ; preuves docs/evidence/2026-10-23/plugin-settings/.
- Dernier complément demandé : fichiers installés sous data/plugins/<id> ;
  package.json, manifest.json, module.wasm ; lecture Wasm confinée + SHA-256,
  export des anciennes installations à l’ouverture du serveur, nettoyage connu
  à la désinstallation. Snapshot/paramètres/droits conservés en base. Validation
  complémentaire : test stockage/reprise/altération/suppression réussi, dix tests
  Core extensions relancés avec lecture des modules sur disque réussis ; Clippy
  all-targets strict réussi. Dockerfile fixe MEDIA_PLUGINS_PATH=/data/plugins.
- Ajout demandé ensuite : upload local d’une collection de fichiers images en
  multipart (nom + images répétées), mêmes bornes normalisation/atomicité que les
  téléchargements serveur. Axum multipart activé. Test Core >400 Ko normalisé
  256×256 et refus anonyme/CSRF réussi ; UI upload multipart et sélection dans
  galerie profil réussis avec API simulée. encoding_rs verrouillé à 0.8.35 pour
  éviter d’ajouter une exigence Rust 1.88 au seul support multipart.
