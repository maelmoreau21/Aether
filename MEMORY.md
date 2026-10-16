# MEMORY.md — État et décisions Aether

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


> **Etat courant : 16 octobre 2026. SQLite/bootstrap, connexion Web, sessions persistantes et permissions backend des fixtures sont presents. Bibliotheques et droits fins sur fixtures livres ; scanner et bibliotheques personnelles absents, P1 autonome incomplet.** Les entrees datees conservent l'historique ; verifier code/tests avant toute annonce.

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
- [ ] **P1** — Mode autonome utilisable : scan read-only, catalogue minimal, lecture P0, admin local, SQLite, déploiement simple.
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
- [ ] Sécurité bootstrap administrateur vérifiée.
- [ ] Standalone déployé.
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
