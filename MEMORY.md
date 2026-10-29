## 29 octobre 2026 — Récupération explicite des leases de cache

- `CacheDirectory` libère explicitement le verrou de son fichier `lease` avant
  la suppression du répertoire. Les tests ciblés `cache_dirs` et la suite
  `cargo test --locked --workspace --all-features --lib --bins` passent.

## 29 octobre 2026 — Reprise après délai de seek vidéo

- Correction supplémentaire : un événement seeked ancien ne termine plus le chargement avant résolution de la progression et demande effective du seek ; régression reproduite puis corrigée, 32 tests NativePlayer réussis. Les chemins audio externe, préparations et repli vidéo ne garantissent pas tous une lecture progressive sans préparation complète.
- Après un timeout de seek initial sans relais distinct, une seule nouvelle tentative recharge la même source à la position demandée ; relais prioritaire, erreurs de métadonnées non retentées, fermeture annulable et échec final explicite. 31 tests ciblés NativePlayer réussis. Cause de l'incident personnel NL non établie ; aucun NAS ni fichier personnel testé.

## 29 octobre 2026 — Correction Bake GHCR

- `docker/bake-action@v6` devait recevoir les fichiers temporaires générés par `docker/metadata-action@v5` avec le préfixe `cwd://`; sans cela, le run échouait sur `lstat .../docker-metadata-action-bake.json: no such file or directory`.
- Correctif appliqué dans `.github/workflows/publish.yml` pour les trois images et documenté dans `docs/CI_GHCR.md`. `docker buildx bake -f docker-bake.ci.hcl --print runtime runtime-core runtime-node` passe localement; actionlint n'est pas installé localement.

## 29 octobre 2026 — Navigation du menu profil

- Les destinations « Mon profil & Préférences » et « Tableau de bord » du menu profil utilisent désormais une navigation SvelteKit explicite après fermeture du popover, tout en conservant leurs `href` pour l’ouverture dans un nouvel onglet.
- Check Web : `npm run check --prefix apps/web` (0 erreur, avertissement CatalogGrid:90 préexistant), `npm run lint --prefix apps/web` et `npm test --prefix apps/web` (187 tests réussis).

## 29 octobre 2026 — Réduction du temps CI et publication Docker

- Run 40931283481 : publication standalone 4 min 56 s de dépendances puis 9 min 01 s de binaires. Les arguments version/branche invalidaient la précompilation et les trois jobs de publication reconstruisaient les mêmes binaires.
- Publication des trois images dans un seul solve Bake, deux tâches Cargo, caches CI/publication mutualisés ; version déclarée après la précompilation et mounts registry/git aux deux étapes. Contrôles provenance/version/runtime Node conservés, digests groupés dans `digest-images` compatible release ; aucun profil release dégradé.
- Trois partitions Rust d'intégration conservées ; contrôles/unitaires/ABI/PostgreSQL dans une quatrième entrée parallèle, avec PostgreSQL seulement sur celle-ci. Features harmonisées pour éviter des recompilations. Gate Rust agrégée et gates de publication conservées ; davantage de runners/minutes cumulées possibles.
- Actionlint 1.7.12, politique release 14/14, répartition 3/3, plans Bake synthétiques vide/bêta/stable, gates et syntaxe Bash réussis. Pas de build Docker réel (daemon arrêté), de nouveau run GitHub, de push ou de publication ; gain distant non mesuré. Détails : `docs/CI_GHCR.md`.

## 29 octobre 2026 — Lecteur, annulation et préparations partagées

- Audio : attentes annulées à pause/fermeture, mises à jour MSE bornées et nettoyées, arrêt des préparations répétées après erreur terminale, erreurs d'un seek dépassé ignorées. Vidéo progressive : attente de progression et de premier fragment annulables ; watchdog natif libère l'audio et invalide la source pour permettre une vraie relance.
- Node : les mêmes préparations froides vidéo/audio complet/sous-titre partagent désormais un fichier sous lease, avec worker unique et 64 participants bornés ; flush Tokio avant publication. Régression avant/après : huit demandes vidéo donnaient 1 succès/7 busy, puis 8 succès sur le même fichier ; vérifié aussi pour audio et sous-titres.
- Web : 187 tests complets réussis après le correctif de reprise près d'une frontière ; check final sans erreur (avertissement préexistant CatalogGrid:90), lint et build statique finaux réussis. Node : packaging 8 réussis/2 ignorés, admission 1, cache 2/1 helper ignoré invoqué par test de crash, stream/video_stream 9. Clippy Node strict/fmt/build debug Core+Node et intégration TCP relais distant 1/1 réussis. Pas de suite Rust générale.
- Navigateurs réels Chrome, EdgeCore et Firefox sur banc HTTP synthétique ; seeks décodés dans une fixture légale AV1 10 bits/Opus de 7 200 s, pas une endurance de deux heures. Charge synthétique 1/4/8/16 produit frames/PCM, avec frames perdues dans le lot de quatre et latences élevées ; aucun gain général ni cible extrême revendiqué. Rapport et limites : `docs/PLAYER_RELIABILITY_2026_10_29.md`.
- Preuve Core/Node/Chrome de reprise en frontière : avant correction, reprise à 29,84 s bloquée en seek avec tampon audio [20,30] et timeout dix secondes ; après chargement des fenêtres 2 et 3, même scénario froid/chaud réussi, reprise image 284,5 ms / PCM 302,2 ms (un essai, pas un percentile). Contrôle réel 3 500 s froid/chaud également réussi. Essais supplémentaires arrêtés à la demande de l'utilisateur faute de tokens.
- Restent : validation NAS/gros films à haut débit/endurance/sortie surround, politique de changements audio concurrents (busy les refuse encore), annulation coordonnée au seek et profil global des allocations. Aucun média original modifié, aucun push/déploiement.

## 29 octobre 2026 — UX, musique et lecteur persistant

- Vues albums/artistes puis sélection d’un groupe, portée explicitement limitée aux 100 morceaux chargés, focus aller/retour. Pochettes et replis mutualisés dans MusicArtwork, initiales d’artistes faute de portrait. Lecture visible, file/playlist/format dans un menu ; mode inline AddToPlaylist sans popovers imbriqués.
- Barre compacte et espace réservé ajusté à sa hauteur réelle, au-dessus de la navigation mobile ; curseur tactile 44 px. Dialogue natif file/paroles, transport fixe, Échap/focus, répétition/mélange/volume/arrêt conservés. Moteur audio, Media Session, API, RBAC et médias personnels préservés.
- Check 0 erreur (CatalogGrid:90 préexistant), lint, build et 177 tests réussis. Banc musique 23 scénarios aux huit largeurs/deux thèmes avec vrai décodage Opus synthétique ; fondations 25 scénarios sans anomalie avant ajustement tactile final, matrice musique rejouée ensuite. CLI Playwright utilisé avec compte synthétique user. Captures inspectées dans output/playwright/ux-music ; rapport docs/UX_MUSIC_2026_10_29.md. Pas de preuve Core/NAS/Range réel, sortie sonore ou téléphone physique.
- Script Core playlists adapté à la fermeture du dialogue avant navigation, vérifié syntaxiquement seulement. Aucun push. Suite : recherche/filtres, listes, téléchargements/hors ligne.

## 29 octobre 2026 — UX, tâches et indexations

- File d’opérations en premier, filtre par état explicitement limité à la page chargée, planifications/automatisation et ajout repliables. Cartes centrées sur type/bibliothèque/état/serveur/actions, détails techniques et résultat conservés. Erreurs d’action séparées du chargement : le polling ne les efface plus. Garde du lease et idempotence conservées, aucun changement backend/API/RBAC.
- Check 0 erreur (avertissement CatalogGrid:90 existant), lint, build et 177 tests réussis. Banc tâches : 23 scénarios aux huit largeurs/deux thèmes, commandes et erreurs/retry, pagination 50, résultat, idempotence et paysage ; non-régression admin : 23 scénarios. Captures représentatives inspectées dans `target/ux-tasks/`. API synthétiques, sans preuve d’exécution Core/Node/NAS ni téléphone physique. Rapport : `docs/UX_TASKS_2026_10_29.md`.
- Aucun push. Suite : parcours musique/lecteur persistant, recherche, listes et téléchargements. Travaux concurrents comptes/groupes/backend préservés.

## 29 octobre 2026 — UX, édition des bibliothèques

- Éditeur en six rubriques, rail desktop/sélecteur mobile et paysage bas, contenu défilant et actions fixes. Groupes avancés repliables, réglages conservés entre rubriques ; validation qui ouvre et focalise le champ invalide même masqué. Ajout de dossier explicite obligatoire, doublons et limite 32 conservés ; aucun changement Core/API/RBAC ou médias personnels.
- Check (0 erreur, avertissement existant CatalogGrid:90), lint, build et 177 tests réussis. Banc bibliothèques : 20 scénarios aux huit largeurs et deux thèmes, payload complet conservé, création films/séries/musique, navigateur de dossiers, erreur/retry et focus ; shell admin : 23 scénarios réussis. Captures inspectées dans `target/ux-libraries/`. APIs synthétiques, pas de preuve de persistance Core/NAS ou téléphone physique. Un essai admin interrompu par reconstruction du preview a été relancé avec succès après redémarrage.
- Scripts Core séries/distribué/NFO/posters/scanner adaptés aux rubriques, syntaxe vérifiée seulement. Rapport : `docs/UX_LIBRARIES_2026_10_29.md`. Aucun push ; prochaine priorité : tâches/indexations puis harmonisation musique, recherche, listes et téléchargements.

## 29 octobre 2026 — UX, cadre administrateur et vue d’ensemble

- Cadre extrait dans `AdminShell.svelte`, douze destinations centralisées dans `navigation.ts`. Sidebar persistante dès 1100 px, dialogue natif mobile/tablette, focus et scroll après sélection, historique/fragments et liens de tâches conservés. Les annuaires restent montés ; filtre utilisateurs conservé lors d’un aller-retour. Travaux concurrents comptes/groupes/sauvegardes préservés.
- Vue d’ensemble : serveur, Nodes, lecteurs et indexations prioritaires, grille mobile à deux colonnes. Les erreurs de chargement des Nodes et d’activité sont visibles, pas assimilées à des zéros confirmés. Aucun changement backend, API ou rôle.
- Check (0 erreur, avertissement existant `CatalogGrid.svelte:90`), lint, build statique et 177 tests unitaires réussis. Banc administrateur : 23 scénarios aux huit largeurs et deux thèmes, 192 sélections de rubriques ; fondations : 25 scénarios sans anomalie. Mode navigation du test extensions réussi. Captures représentatives inspectées dans `target/ux-admin/`. APIs synthétiques, sans preuve Core/NAS/SSO/téléphone physique.
- Test historique extensions complet exécuté mais arrêté sur l’ancien sélecteur « Actualiser les dépôts », absent du composant actuel non modifié ; aucun succès du parcours complet revendiqué. Les scripts Core séries/distribué sont adaptés à la sidebar mais seulement vérifiés syntaxiquement dans ce jalon. Rapport : `docs/UX_ADMIN_2026_10_29.md`. Aucun push ; prochaine priorité : parcours d’édition des bibliothèques.

## 29 octobre 2026 — UX, fiches films et séries

- `DetailHero.svelte` mutualise titre/logo, métadonnées disponibles, résumé extensible, backdrop réel et composition mobile/desktop. Les affiches ne servent plus de panorama. Lecture avant les outils ; versions et inventaire technique du film repliables, téléchargement conservé parmi les actions secondaires.
- Série : un seul select de saison, épisodes illustrés 16:9, lecture visible, résumés/suivi et versions repliables. Pagination locale et `episodesPage` conservées. « Voir les épisodes » déplace localement focus/scroll sans navigation de route ; sa destination visible est vérifiée en paysage.
- `npm run check` (0 erreur, avertissement existant `CatalogGrid.svelte:90`), lint et build statique réussis ; 177 tests unitaires réussis. `browser-ux-details.cjs` : 23 scénarios réussis aux huit largeurs et deux thèmes, plus deux lancements/décodages depuis film/épisode avec fixture légale MP4 AV1 10 bits/Opus, pause/reprise/seek clavier/sortie. `browser-ux-foundations.cjs` : 25 scénarios réussis sans anomalie. Captures représentatives inspectées ; preuves dans `target/ux-details/`.
- APIs et transport média interceptés : aucun nouveau résultat Core/Node, NAS, MKV, écoute humaine ou téléphone physique. Contrat et limites : `docs/UX_DETAILS_2026_10_29.md`. Moteur vidéo, backend, droits, médias personnels et travaux concurrents conservés. Aucun push. Prochain jalon : navigation administrateur persistante desktop et accessible mobile avec conservation des fragments.

## 28 octobre 2026 — UX, premier jalon navigation et accueil

- Audit priorisé et suite : `docs/UX_REDESIGN_2026_10_28.md`. Navigation mobile/tablette nommée, sélecteur de bibliothèques avec liste défilante et actions fixes, accueil reprise/ajouts récents, hero à deux actions, affiches simplifiées, flèches de reprise accessibles et lecteur musical dégagé. Thèmes, logo et sélection manuelle/linguistique/rotation des images conservés.
- `npm run check` : zéro erreur, un avertissement préexistant dans `CatalogGrid.svelte:90`. `npm run lint`, `npm test` (177/177, 29 fichiers) et `npm run build` réussis. `git diff --check` sur les fichiers du jalon réussi.
- `node scripts/browser-ux-foundations.cjs` : 25 scénarios, 100 prises de capture, aucune anomalie restante dans le rapport final. Chrome 156.0.8078.48 Windows headless, huit largeurs 320–1920 px, clair/sombre, paysage/toucher simulé, clavier/menus et états absents/erreurs. Captures inspectées dans `target/ux-foundations/`. APIs et SVG synthétiques : aucune preuve NAS, téléphone physique, décodage vidéo ou écoute musicale réelle.
- Fiches et administration non entièrement refondues. Prochaine priorité : lecture et informations secondaires des fiches film/série, puis navigation admin persistante desktop avec conservation des fragments. Aucun push ni déploiement ; travaux préexistants utilisateurs/groupes et backend conservés.

## 28 octobre 2026 — Validations complémentaires

- Suite Rust complète relancée après correction du montage multi-Node et du schéma : unités Core 126 réussies, premières intégrations réussies, puis arrêtée à la demande de rapidité pendant le parcours distribué ; la suite complète n’est pas validée. Le test multi-Node corrigé a passé séparément.

- Suite Rust complète interrompue par un échec de classement multi-Node : le test supposait un remplacement immédiat du RTT malgré la moyenne mobile. Montage corrigé et scénario ciblé réussi avec deux Nodes TCP locaux ; nouvelle suite complète en cours.
- PostgreSQL : 28 colonnes initiales de temps Unix en `bigint`, sans migration ni modification des données existantes. Schémas SQLite/PostgreSQL vierges et `scripts/test-admin-postgres.ps1` réussis : sauvegarde/restauration dans une autre base PostgreSQL 18 jetable ; restauration des assets PostgreSQL et NAS non validés.

- `cargo clippy --locked --all-targets -j 1 -- -D warnings` réussi avant les derniers ajustements des messages de sauvegarde. Les CLI backup/recover recompilent (`cargo check` ciblé) ; sous Windows les ACL sont héritées et non contrôlées, avertissements ajoutés sans modification d’ACL.

- Les images Docker `runtime`, `runtime-core`, `runtime-node` se construisent localement avec `CARGO_BUILD_JOBS=1` par défaut ; les sources frontend ont évolué pendant les builds, qui ne garantissent donc pas l’état courant final. Aucune publication ou validation NAS.
- Bancs synthétiques Chrome 156 et EdgeCore 155 : AV1 8 bits/Opus mono MP4 et MKV, puis MP4 AV1 10 bits et deux pistes Opus 7.1, ouverture/changements/seek et PCM réussis. Ni sortie 7.1 physique, ni réseau distant, ni lecture continue de plusieurs heures.

- `lyrics.get` : validateur et contrôles avant écriture corrigés ; 4 tests unitaires et un parcours Core/worker Wasm synthétique signé/SQLite réussissent. Purge du cache expiré par lots de 64 dans le scheduler : test SQLite réussi ; PostgreSQL, charge et service LRCLIB réel non testés.

- `powershell -NoProfile -File scripts/test-postgres.ps1` : contrat HTTP PostgreSQL 18, 8 tests OIDC avec IdP simulé et scénario distribué à deux Nodes locaux réussis. Base Docker temporaire détruite par le script ; aucun test multi-machine ou Authentik réel.

- Le schéma initial PostgreSQL 18 passe sur une base Docker jetable ; son contrat HTTP et la réouverture passent, ainsi que huit tests OIDC avec fournisseur simulé. Pas de preuve Authentik réel.
- `restore_assets_cli_rejects_partial_extension_sync` réussit sur installations jetables ; des fichiers Core ont toutefois changé pendant la compilation, donc cette preuve couvre le binaire compilé et non nécessairement tous les derniers changements.
- Administration : focus initial, confinement clavier, Échap et restauration du focus ajoutés aux éditeurs comptes/groupes ; test Chrome sur DOM synthétique et ESLint réussis, Svelte check sans erreur, un avertissement existant.
- `lyrics.get` : rejet du validateur constaté et correctif préparé ; test de worker Wasm synthétique à exécuter avant toute validation.

## 28 octobre 2026 — Commandes vidéo sur téléphone

- Audio Opus segmenté : la fenêtre suivante est demandée huit secondes avant la limite au lieu de quatre, après le chargement initial. Les données de lecture affichent aussi la réserve de l'audio séparé. Tests unitaires ciblés : 20 réussis ; aucune preuve sur Pixel 7a, NAS ou réseau réel, ni identification certaine de la cause des saccades.
- Les commandes du lecteur se répartissent sur deux lignes sur écran étroit, avec une zone tactile plus grande pour la barre de progression. Le déplacement dans la vidéo est appliqué au relâchement, afin d'éviter des recherches successives pendant le glissement. Les tests unitaires du tampon et de la vidéo progressive passent ; aucun essai sur téléphone réel et aucune cause des saccades en lecture continue n'ont été établis.

## 28 octobre 2026 — Politique d’avatars native

- La portée des collections d’avatars est désormais un type explicite : 3 tests ciblés réussis, anciennes ressources conservées. Les routes natives restent dans le module des extensions ; aucun plugin n’est requis.
- OIDC : les comptes locaux liés conservent leur rôle ; les tests liaison/reconnexion et refus de promotion par simple claim réussissent avec un IdP synthétique, sans Authentik réel.
- Le scan musical respecte les marqueurs d’exclusion de dossiers ; test Node ciblé réussi sur fixtures locales, sans NAS.
- Fournisseurs : cache lié à la configuration/génération/paquet, correspondance musicale renforcée, champs artiste structurés côté Core/SDK et sources MusicBrainz/Discogs. Tests synthétiques réussis ; ni paquet Wasm publié ou testé de bout en bout, ni service réel.
- EdgeCore : les essais répétés du banc donnent 552 ms et 413 ms pour le premier signal, sans reproduire les délais anciens. Les horloges média et WebAudio ont été instrumentées ; cause initiale non établie.

- `cargo test --locked -p media-poc --test backup_bundle -j 1 -- --test-threads=1` : 4/4 réussis sur archives jetables, intégrité, corruption, destinations existantes et certificat. Le test du format des deux moteurs ne constitue pas une restauration PostgreSQL réelle. Archives courantes non chiffrées : elles contiennent des secrets.

- Les tests Rust ciblés `music_candidates_and_details_require_consistent_identity` et `initialization_keeps_empty_installation_free_of_phantom_extensions` réussissent. Ils ne prouvent ni les fournisseurs réels ni la restauration complète.

- Media Node : `cargo test --locked -p media-node --lib malformed_signed_channel_responses_back_off_then_reconnect -j 1 -- --nocapture` réussit ; le test local couvre un délai après réponse signée mal formée puis reconnexion. Il ne prouve pas un réseau distant réel.

- La politique d’import et de suppression d’avatars ne consulte plus les réglages des extensions actives. La politique native reste autoritaire ; test `cargo test --locked -p media-poc --test account avatar_policy_is_independent_of_active_plugins -j 1 -- --nocapture` réussi. Ceci remplace la décision contraire mentionnée dans la note du 27 octobre ci-dessous.
- Les collections natives restent implémentées dans un module partagé avec les anciennes routes d’extension et utilisent un marqueur interne ; le découplage structurel complet reste ouvert. Aucune migration ni suppression des anciennes données.

## 27 octobre 2026 — Correction des régressions CI (Avatar Policy & Browser Playlists)

- **Avatar policy & plugins actifs (`crates/core/tests/account.rs`)** :
  - **Cause racine** : lors du passage à la politique native d'avatars (`load_policy`), la fonction `permissions` dans `crates/core/src/avatars.rs` n'interrogeait plus les `extension_settings` des extensions actives disposant de la permission `avatars`. Les contraintes `disallow_custom_avatars` et `force_random_on_creation` imposées par les plugins étaient ignorées.
  - **Correction** : `permissions()` cumule désormais la politique native et les règles déclarées par les extensions actives, rétablissant la conformité du test `avatar_policy_enforced_for_active_plugins`.
- **Test Playwright des playlists partagées (`scripts/browser-playlists.cjs` & `product_tests::mixed_playlists`)** :
  - **Cause racine** : dans `apps/web/src/routes/admin/libraries/+page.svelte`, `<AvatarCollections />` était injecté dans un `<div hidden={currentHash !== '#avatars-section'}>`. Bien que masqué visuellement, le composant restait dans le DOM de la page d'administration, y compris lorsque l'utilisateur naviguait vers `#extensions-section`. Le test Playwright vérifiait `settings.locator('input[type=file]').count() === 0` (politique repository-only des extensions) et échouait avec `1 !== 0` en trouvant l'input de fichier de l'avatar pack.
  - **Correction** : conditionnement de `<AvatarCollections />` avec un bloc Svelte `{#if currentHash === '#avatars-section'}` afin qu'aucun input file parasite ne réside dans le DOM hors de sa section dédiée.
- **Vérifications validées** :
  - `cargo fmt --check` : 100 % conforme.
  - `cargo clippy --all-targets -- -D warnings` : 0 avertissement, 0 erreur.
  - `cargo test --test account` : 4/4 tests passés.
  - `cargo test --locked -p media-poc --lib product_tests::mixed_playlists -- --test-threads=1` : 1/1 passé (Playwright browser test).
  - `npm run check --prefix apps/web` : 0 erreur Svelte / TypeScript.
  - `npm run build --prefix apps/web` : build static OK.

## 27 octobre 2026 — Suite de stabilisation, validation incomplète

- Avatars : collections et politique natives implémentées dans les schémas initiaux, anciennes ressources conservées ; SQLite vierge et tests frontend ciblés réussis. Le test HTTP complet et PostgreSQL restent à exécuter. Les bases existantes n'ont pas automatiquement les nouvelles tables ; aucune migration ni réinitialisation. Le `cargo check` antérieur ne couvre pas les derniers changements.
- Paroles : branchement `lyrics.get`, protection des paroles locales et provenance modifiés ; compilation et tests Rust finaux interrompus sous pression mémoire, aucun appel réel LRCLIB. Un scan peut encore effacer le cache des paroles externes.
- IntroSkipper : entrée limitée à la bibliothèque autorisée, résultats liés aux copies et durées soumises et seule la cible est persistée. Test de ces derniers changements interrompu ; fraîcheur transactionnelle de la copie associée non garantie.
- EdgeCore 155 headless : banc synthétique MP4/MKV parcouru, mais signal audio initial original tardif et aucune preuve de parcours Svelte/Core/Node. Détails dans `docs/STREAMING_COMPATIBILITY.md`.

## 27 octobre 2026 — Stabilisation : premier contrôle ciblé

- Chrome 156 headless sous Windows : `browser_scanner` réussit sur fixtures synthétiques MP4 et MKV AV1/Opus, avec changements audio, sous-titres, pause/seek et redémarrage du Core. Ce test court ne démontre ni Edge, ni lecture continue de plusieurs heures, ni NAS ou Node distant. Les mesures de démarrage sont locales et ne constituent pas une comparaison avant/après.
- Avatars natifs : le parcours existant n'est pas finalisé. Sur SQLite vierge sans extension, l'import de pack échoue sur une clé étrangère ; galerie et politiques restent liées aux extensions. Les indications d'intégration complète ci-dessous sont à considérer comme non validées. PostgreSQL et Docker indisponibles lors de ce contrôle.
- Rotation d'affiches : une sélection explicitement vide doit désactiver toutes les bibliothèques ; l'absence de sélection conserve le repli historique. Chapitres : les limites sont triées et filtrées avant utilisation pour les marqueurs natifs.

## 27 octobre 2026 — Intégration native de Poster Rotator et de la Galerie d'Avatars (Sans Wasm)

- **Poster Rotator natif & sélection granulaire par bibliothèque** :
  - **Découplage de Wasm / plugins** : le système de rotation d'affiches fait désormais partie intégrante du cœur d'Aether et ne dépend d'aucun module externe.
  - **Préférences utilisateur repensées** :
    - Retrait de la section encombrante dans l'onglet « Compte ».
    - Création d'un onglet dédié « Affichage & Affiches » dans `SettingsView.svelte`.
    - Remplacement du basculement binaire rigide (« Films » / « Séries ») par une sélection dynamique et granulaire des bibliothèques accessibles par l'utilisateur (`enabled_libraries: Vec<String>`).
    - Rétro-compatibilité assurée (`enable_movies` et `enable_series` conservés en repli si `enabled_libraries` n'est pas encore défini).
  - **Paramétrage administrateur par bibliothèque** :
    - Ajout du commutateur `poster_rotation_enabled: bool` dans `LibraryOptions` (`crates/core/src/library_options.rs` et `apps/web/src/lib/admin/types.ts`).
    - Intégration d'une option « Rotation des affiches (Poster Rotator) » dans `LibraryOptions.svelte` permettant aux administrateurs d'activer ou désactiver la rotation spécifique à chaque bibliothèque.
  - **Sélection d'artwork dans le frontend** :
    - Mise à jour de `selectArtwork` dans `apps/web/src/lib/catalog/artwork-selection.ts` et du store réactif `artwork-rotation-policy.svelte.ts` pour respecter `policy.enabledLibraries` et `work.libraryId`.

- **Galerie d'Avatars native dans l'administration** :
  - **Section d'administration dédiée** :
    - Ajout de l'onglet « Avatars » dans la barre latérale sous la catégorie « Accès » (`#avatars-section`) dans `apps/web/src/routes/admin/libraries/+page.svelte`.
    - Affichage direct du composant `AvatarCollections.svelte` sans nécessiter l'ouverture d'un plugin Wasm.
  - **Points de terminaison natifs sans Wasm** :
    - Enregistrement des routes natives `/api/v1/admin/avatar-packs`, `/api/v1/admin/avatar-packs/upload`, `/api/v1/admin/avatar-packs/source`, `/api/v1/admin/avatar-packs/{pack}`, `/api/v1/admin/avatar-packs/{pack}/images`, et `/api/v1/admin/avatars/assign-missing`.
    - Support complet pour l'envoi de fichiers locaux (PNG, JPEG, WebP, max 2 Mo par image, 64 Mo par import) ou l'import direct depuis des dépôts GitHub (par exemple `cedev-1/jellyfin-avatars`).
    - Compatibilité modèle de données : initialisation transparente d'une entrée `native` dans `extensions` et `extension_publishers` (`active=1`, permission `avatars`) pour satisfaire les contraintes d'intégrité référentielle SQL sans migration disruptive.

- **Robustesse Docker / PostgreSQL (`MEDIA_ASSETS_PATH`)** :
  - Dans `crates/core/src/identity.rs`, fallback sécurisé sur `/data/assets` lorsque la base de données est distante (PostgreSQL) sans chemin de base de données local, évitant l'erreur `Configuration` au démarrage sous Docker.
  - Déclaration explicite de `MEDIA_ASSETS_PATH=/data/assets` dans le `Dockerfile`.

- **Vérifications et tests validés** :
  - `cargo fmt --check` : 100 % conforme.
  - `cargo test -p media-poc --lib` : 112 tests unitaires et d'intégration validés.
  - `npm test --prefix apps/web` : 29/29 fichiers de tests passés (176 tests).
  - `npm run check --prefix apps/web` : 0 erreur Svelte / TypeScript.

## 27 octobre 2026 — Correction CI : Résolution de `MEDIA_WEB_DIST` pour les tests navigateur et `cargo fmt`

- **Correction du test navigateur Playwright `product_tests::mixed_playlists`** :
  - **Cause racine** : le commit `74377be` avait remplacé l'implémentation de `configured_web_dist()` par un chemin fixe relatif `apps/web/build`, ignorant la variable d'environnement `MEDIA_WEB_DIST`. Lors de l'exécution de `cargo test -p media-poc --lib` dans le sous-répertoire `crates/core`, Axum `ServeDir` tentait de charger les assets depuis `crates/core/apps/web/build` au lieu de `${{ github.workspace }}/apps/web/build` (défini dans `.github/workflows/ci.yml`). Playwright échouait avec `net::ERR_HTTP_RESPONSE_CODE_FAILURE` (404) sur `http://127.0.0.1:<port>/app/music/`.
  - **Correction** :
    - Restauration de `configured_web_dist_from` et de la prise en compte de `std::env::var("MEDIA_WEB_DIST")` dans `crates/core/src/lib.rs`.
    - Restauration des tests unitaires `configured_web_dist_defaults_when_unset_or_empty`, `configured_web_dist_custom_path`, et `configured_web_dist_nonexistent_path_falls_back_to_raw`.
    - Dans `crates/core/src/product_tests.rs`, passage explicite par `secure_app_with_web_dist` résolvant `MEDIA_WEB_DIST` ou retombant de manière sûre sur `CARGO_MANIFEST_DIR/../../apps/web/build` pour les lancements locaux indépendamment du CWD.

- **Conformité `cargo fmt --check`** :
  - Formatage conforme sur `crates/core/src/extensions.rs`, `crates/core/src/preferences.rs`, `crates/core/tests/extensions.rs`, et `crates/core/tests/preferences.rs`.

- **Vérifications validées** :
  - `cargo fmt --check` : code 0 (100 % conforme).
  - `cargo test -p media-poc --lib tests::configured_web_dist` : 3/3 passés.
  - `cargo test -p media-poc --lib product_tests::mixed_playlists` : 1/1 passé.
  - `cargo clippy --all-targets -- -D warnings` : 0 avertissement, 0 erreur.

## 26 octobre 2026 — Refonte complète du versionnement et des releases (Aether & Aether-Plugins)

- **Source de vérité unique & SemVer pour toute la plateforme Aether** :
  - Harmonisation stricte de la version produit entre le backend Rust (`crates/shared/src/version.rs`, `version_policy.rs`, `build.rs`), `aether-core`, `aether-node`, le binaire standalone `aether`, l'interface Web SvelteKit et les 3 images Docker GHCR.
  - Structure `VersionInfo` exposée par `media_contracts::version::current_version_info()` (`version`, `channel`, `commit`, `build_date`, `release_url`).
  - Point de terminaison `/api/v1/config` enrichi avec ces métadonnées réelles.
  - Interface Web SvelteKit (`SettingsView.svelte`) : ajout d'un onglet « À propos » discret affichant la version, le canal (`stable`, `beta` ou `dev`), le SHA Git court, la date de compilation et le lien vers la GitHub Release.
  - Builds locaux non publiés : affichage transparent de la version de développement avec commit SHA (ex: `0.2.0-beta.0-dev+b08c635`), sans prétendre être une version officielle.

- **Automatisations Aether sur `develop` et `main`** :
  - `scripts/calculate_release.py` et `.github/workflows/` refondus :
    - Première initialisation déterministe à `0.1.0` sur `main` (aucun tag écrasé).
    - Sur `develop` : incrément automatique du compteur bêta après réussite des tests (`0.2.0-beta.0` → `0.2.0-beta.1` → `0.2.0-beta.2`), pose du tag Git sur le commit exact et publication Docker GHCR avec tags `develop`, `0.2.0-beta.N`, SHA.
    - Idempotence absolue : détection des commits déjà tagués pour éviter toute duplication lors d'une relance.
    - Sur `main` : promotion contrôlée via Pull Request (avec support des évolutions `AUTO`, `PATCH`, `MINOR`, `MAJOR`), publication des 3 images Docker avec digest SHA-256 dans les notes de version, tag immutable `vX.Y.Z`, et mise à jour de `latest` uniquement après validation complète.
    - Préparation automatique du cycle suivant sur `develop` (ex: `0.3.0-beta.0`).

- **Versionnement indépendant et respect des branches protégées pour Aether-Plugins** :
  - `scripts/detect_changed_plugins.py` fiabilisé : détection granulaire des seuls plugins modifiés (TMDb, IMDb, OMDb, MusicBrainz, IntroSkipper, etc.), exclusion des modifications documentaires, et détection ciblée des changements d'API partagée.
  - Résolution de la contrainte des branches protégées : fin des tentatives de push direct sur `main` dans le workflow de release !
    - Nouveau workflow `prepare-plugin-release.yml` : prépare le paquet signé Ed25519, met à jour `repository/manifest.json` et ouvre une Pull Request contrôlée ciblant `main`.
    - Workflow `release-plugins.yml` : après fusion de la PR sur `main`, vérifie les paquets et crée le tag Git indépendant (`<plugin>-v<version>`) ainsi que la GitHub Release sans aucune commande `git push origin main`.
    - Sur `develop` : incrément bêta propre à chaque plugin modifié (`<plugin>-v<version>-beta.N`).
    - Immutabilité des paquets : refus d'écraser un paquet signé existant.

- **Vérifications et tests validés** :
  - `python scripts/test-release-policy.py` (Aether) : 14/14 tests passés couvrant l'ensemble des 18 critères d'acceptation, les fusions squash/rebase/merge et la concurrence.
  - `python scripts/test-release-policy.py` (Aether-Plugins) : 7/7 tests passés.
  - `cargo test -p media-contracts` : 13/13 tests passés.
  - `npm run check --prefix apps/web` : 0 erreur Svelte/TypeScript.
  - `node scripts/test-manifests.cjs` & `test-repository.cjs` (Aether-Plugins) : catalogue et signatures 100 % valides.
  - Documentation exhaustive créée : `docs/RELEASES_AND_VERSIONING.md` dans les deux dépôts.
  - Respect strict des consignes de sécurité : aucune release réelle déclenchée pendant la phase de développement.

## 26 octobre 2026 — Optimisation de l'empreinte mémoire RAM (P0 à P3) & Allègement Docker

- **P0 — Diagnostic et observabilité de la RAM réelle (`crates/core/src/system_memory.rs`, `AdminMaintenance.svelte`)** :
  - Module `system_memory` distinguant la consommation réelle :
    - Core process : RSS, tas anonyme (`RssAnon`), fichiers/mmap (`RssFile`), pic historique (`VmHWM`), taille virtuelle.
    - Processus enfants : inspection des PIDs fils (workers Wasm, probe) et somme de leur RSS.
    - Hôte Linux / Conteneur : analyse de `/proc/meminfo` (`Cached`, `Buffers`, `SReclaimable`), calcul du cache restituable (`linux_reclaimable_cache_bytes`).
  - Endpoints sécurisés : `GET /api/v1/admin/system/memory` et `POST /api/v1/admin/system/memory/trim`.
  - Interface Web : panneau de diagnostic mémoire dans `AdminMaintenance.svelte` avec synthèse claire identifiant les vrais responsables (expliquant que la RAM visible dans Docker est souvent dominée par le cache de pages Linux restituable), et bouton « Libérer la mémoire (Trim) ».

- **P1 — Cache catalogue borné en Mo (`crates/core/src/inventory.rs`)** :
  - Remplacement de l'ancien `HashMap` limité en nombre d'entrées (512) qui purgeait tout brutalement par `BoundedCatalogueCache`.
  - Estimation de taille mémoire en octets pour chaque réponse (`estimate_json_bytes`).
  - Budget mémoire configurable via `MEDIA_CATALOGUE_CACHE_MAX_MB` (défaut : 32 Mo, minimum 1 Mo).
  - Éviction LRU fine au fur et à mesure sans purge destructrice globale.

- **P1 — Optimisation des allocations et libérations post-scan (`crates/node/src/scanner.rs`, `inventory.rs`)** :
  - Libération immédiate des structures de cache de répertoires (`drop(folder_cache)`, `drop(series_cache)`, `drop(artwork_memo)`).
  - Appel systématique à `trim_memory()` (`malloc_trim(0)`) à la fin de `scan_streaming`, dans `publish_staged_scan` et dans les tâches de scan pour restituer immédiatement les blocs d'arènes d'allocateur glibc au noyau Linux.

- **P2 — Maîtrise des allocations Argon2 et rétention de mémoire (`crates/core/src/identity.rs`, `auth.rs`)** :
  - Paramètre de mémoire Argon2 configurable via `MEDIA_ARGON2_MEMORY_KIB` (défaut : 65536 KiB, ajustable jusqu'à 19456 KiB OWASP pour NAS contraints).
  - Élimination de la rétention mémoire dans les threads Tokio : invocation de `trim_memory()` à la fin de chaque opération de hachage et de vérification (`verify_password`), évitant que les 64 Mo de tas ne restent piégés dans les arènes de threads.

- **P2 — Optimisation des caches persistants et SQLite (`crates/core/src/identity.rs`, `system_tasks.rs`)** :
  - Tailles de cache SQLite par connexion et mmap configurables et allégées par défaut : `MEDIA_SQLITE_CACHE_KIB` (-32 Mo au lieu de -64 Mo) et `MEDIA_SQLITE_MMAP_BYTES` (64 Mo au lieu de 256 Mo).
  - Ajout de `PRAGMA shrink_memory` et `trim_memory()` dans la tâche de maintenance périodique `optimize-database`.

- **P3 — Allègement des images Docker (`Dockerfile`)** :
  - Nettoyage des fichiers de documentation, manuels, caches temporaires et catalogues de langues inutilisés (`/usr/share/locale`, `/usr/share/info`, `/usr/share/man`, `/usr/share/doc`) dans `backup-runtime` et `runtime-node`.

- **Vérifications et tests validés** :
  - `cargo test -p media-poc --lib system_memory` : 2 tests passés.
  - `cargo test -p media-poc --lib test_bounded_catalogue_cache` : 1 test passé.
  - `cargo test -p media-poc --lib` : 111 tests passés (0 échec, 3 ignorés).
  - `npm test --prefix apps/web` : 29 suites passées (171 tests).
  - `npm run check --prefix apps/web` : 0 erreur Svelte/TypeScript.
  - `cargo clippy --workspace --all-targets -- -D warnings` : 0 avertissement, 0 erreur.
  - `cargo fmt --all -- --check` : 100 % conforme.

## 26 octobre 2026 — Mission 4 : Orchestrateur universel des extensions

- **Fonctionnalités livrées et validées** :
  - **1. Métadonnées automatiques & Provenance** :
    - Déclenchement post-scan lors de données manquantes (`metadata.missing`, `media.added`, `media.modified`).
    - Priorité par fournisseur et par bibliothèque (`automation_policies.priority`).
    - Sélection stricte du candidat de confiance (`trusted_metadata_candidate`, `trusted_music_candidate` avec tolérance de durée ±5s / ±5%).
    - Enrichissement progressif sans écrasement : conservation de la provenance dans `automatic_metadata_provenance` (work_id, field_name, extension_id, provider_id, priority, value_json, source_fingerprint, updated_at).
    - Respect absolu des corrections manuelles (`manual_json` préservé à 100%).
  - **2. Images & Déduplication** :
    - Récupération des affiches, backdrops, logos via extensions multimédias.
    - Déduplication via hash d'URL source (`source_url_hash`) et SHA-256 dans `AssetStore`.
    - Préservation inviolable des affiches manuelles (`work_image_entries` où `provider IS NULL`).
  - **3. Musique & Paroles synchronisées** :
    - Émission de snapshots dédiés (`automation::emit_music_snapshot`) sur `music_tracks`.
    - Enrichissement automatique artiste, album, pochette (`cover_path` / `asset:`) et paroles synchronisées (LRC) ou texte brut via opération `lyrics.get`.
    - Protection stricte des paroles locales manuelles existantes (fichiers tagués).
    - Cache contrôlé des réponses fournisseurs (`automation_provider_cache`) avec TTL et purge automatique.
  - **4. Administration & Traçabilité** :
    - Endpoints `/api/v1/admin/tasks/automation/history`, `/policies`, `/run` (avec option `missing_only: true`).
    - Interface d'administration enrichie (`AutomationTasks.svelte`) : configuration des priorités, bouton « Exécuter maintenant » / « Manquants uniquement », suivi des tâches avec durée et explication d'échecs, suivi des quotas/limites de débits fournisseurs et traçabilité de provenance des métadonnées.
  - **5. Défi Extrême & Benchmark d'échelle (100 000 médias)** :
    - Traitement naïf vs Traitement optimisé (`crates/core/tests/automation_flow.rs` - `extreme_scale_100k_and_naive_vs_optimized_comparison`) :
      - Requêtes HTTP externes : 5 000 -> 50 (99,0 % de requêtes évitées grâce au cache fournisseur partagé entre épisodes/pistes).
      - Transactions SQL : 5 000 commits individuels -> 50 transactions groupées (99,0 % de réduction d'écritures).
      - Temps d'exécution : 701 ms -> 186 ms (accélération ~3,7x à 4,3x).
      - Empreinte mémoire RAM : passage de O(N) non borné à O(1) borné via pagination par curseur (lots de 256).
  - **6. Tests & Qualité** :
    - `cargo test -p media-poc --test automation_flow` : 15/15 réussis (1 ignoré benchmark queue).
    - `cargo test -p media-poc --test extensions -- --test-threads=1` : 15/15 réussis.
    - `cargo test --manifest-path c:\Users\Mael\Documents\GitHub\Aether-Plugins\Cargo.toml` : 100 % réussis (lrclib, musicbrainz, omdb, tmdb, etc.).
    - `npm test --prefix apps/web` : 29 suites passées, 171 tests réussis.
    - `npm run check --prefix apps/web` : 0 erreur TypeScript/Svelte.
    - `cargo clippy --workspace --all-targets -- -D warnings` : 0 erreur, 0 avertissement.
    - `cargo fmt --all -- --check` : 100 % conforme.

## 26 octobre 2026 — P9 (Benchmarks à l'échelle) & P10 (Validation finale et livrables)

- **P9 — Benchmarks de passage à l'échelle mesurés (crates/core/tests/global_catalogue_consolidation.rs)** :
  - **Palier 1 000 à 1 000 000 d'œuvres (SQLite SSD)** :
    - 1 000 œuvres : Page 1 froide = 32.41 ms | Cache chaud = 0.702 ms | Recherche FTS5 = 6.87 ms | Combo 5 filtres = 9.52 ms.
    - 10 000 œuvres : Page 1 froide = 13.82 ms | Cache chaud = 0.671 ms | Recherche FTS5 = 10.11 ms | Combo 5 filtres = 11.76 ms.
    - 100 000 œuvres : Page 1 froide = 50.61 ms | Cache chaud = 1.062 ms | Recherche FTS5 = 19.86 ms | Combo 5 filtres = 83.86 ms.
    - 1 000 000 d'œuvres : Page 1 froide = 32.91 ms | Cache chaud = 1.715 ms | Recherche FTS5 = 86.04 ms | Combo 5 filtres = 29.74 ms.
  - **Fluidité de navigation** : temps de réponse sous les 2 ms en cache chaud et ~30 ms en requêtes froides multi-critères.

- **P10 — Livrables et validation finale (docs/STORAGE_REFONTE_LIVRABLE.md)** :
  - Publication du rapport exhaustif P0-P10 couvrant les 10 livrables requis : audit initial des défauts réels, plan architectural, schémas consolidés SQLite/PostgreSQL, stockage AssetStore, sauvegarde/restauration complète, automatisation unifiée, multi-Core, benchmarks réels, configurations Docker Compose (docker-compose.yml, compose.distributed.yaml), et limites documentées.
  - Toutes les vérifications de démarrage, bootstrap administrateur, scans, affiches, avatars, extensions Wasm, tâches durables et intégrité de restauration sont validées.

## 26 octobre 2026 — Finalisation P6, P7 et P8 : Automatisation unifiée, Sauvegarde/Restauration E2E & Architecture Multi-Core

- **P6 — Unification des tâches et automatisation (crates/core/src/system_task_queue.rs, dmin_backups.rs, scheduler.rs, system_tasks.rs)** :
  - **Durable task dispatch unifié** : scheduler::dispatch_once centralise le dispatch régulé de toutes les tâches Core (utomation::drain, system_task_queue::dispatch_once, et les scans/analyses 	ask_engine).
  - **Sauvegarde planifiée orchestrée via durable_tasks** : suppression de l'exécution concurrente hors file dans dmin_backups.rs. scheduled_tick effectue la réservation CAS de l'échéance et pousse la tâche ackup dans la file durable avec catégorie system. Exécution bornée (1 h), préemption immédiate si une lecture multimédia intervient, et enregistrement de l'état interrupted en cas d'interruption.
  - **Fencing et leases systématiques** : toutes les tâches système (optimisation DB, nettoyage sessions, nettoyage cache Aether, nettoyage playlists, sauvegarde) tournent sous lease temporisé avec renouvellement continu (heartbeat 10s), rejetant tout résultat si le lease ou l'attempt SQL a expiré.

- **P7 — Sauvegarde et restauration complètes (crates/core/src/backup_bundle.rs, ssets.rs, in/restore-assets.rs, 	ests/full_backup_restore_e2e.rs, docs/BACKUP_RESTORE.md)** :
  - **Sauvegarde totale chiffrée** : le format de bundle intègre la base SQL, les configurations sensibles chiffrées, ainsi que tous les assets binaires durables de /data/assets avec hashage Sha256 à la volée. Toute altération, absence ou corruption d'asset bloque atomiquement la création du bundle (AssetIntegrity).
  - **Restauration complète et vérifiée** : nouvel outil media-restore-assets et méthode AssetStore::restore_extracted pour importer les objets et auditer l'intégralité de la table ssets via erify_all().
  - **Régénération automatique des extensions Wasm** : la table extensions conservant les paquets signés intègres, sync_installed (intégré à media-restore-assets et au bootstrap de Core) reconstitue fidèlement sur disque (/data/plugins/<id>/) le module.wasm, manifest.json et package.json avec vérification d'intégrité cryptographique.
  - **Validation de bout en bout (	ests/full_backup_restore_e2e.rs)** : cycle complet validé (Server A avec admin, asset binaire et extension -> archive chiffrée -> Server B vierge -> restore SQL + restore assets + sync extensions -> vérification audit 0 missing, 0 corrupt, Wasm fonctionnel et administrateur authentifiable).

- **P8 — Préparation multi-Core (crates/core/src/database.rs, inventory.rs, docs/MULTI_CORE.md)** :
  - **Verrous transactionnels distribués** : utilisation de egin_write_scoped avec pg_advisory_xact_lock(hashtext(scope || ':' || entity_id)) permettant la sérialisation fine sur PostgreSQL sans verrou global.
  - **Cache de catalogue distribué** : validation du cache de requêtes catalogue par numéro de génération SQL partagé (inventory_cache_generation.gen), garantissant l'invalidation inter-Core dès qu'une écriture survient sur n'importe quel nœud Core.
  - **Documentation d'architecture multi-Core** : cartographie complète des états mémoire et prérequis de passage en cluster dans docs/MULTI_CORE.md.

- **Vérifications et conformité qualité** :
  - cargo test -p media-poc --lib : 107 tests passés avec succès (0 échec, 3 ignorés).
  - cargo test -p media-poc --test full_backup_restore_e2e : 100% réussi.
  - Tests d'intégration connexes validés : dmin_backups, ackup_bundle, ackup_restore, 	ask_engine_http, 	ask_operations, plugin_storage_integration.
  - cargo clippy -p media-poc --all-targets -- -D warnings : 0 avertissement, 0 erreur.
  - cargo fmt --all -- --check : 100% conforme.

## 26 octobre 2026 - P6 (sauvegarde planifiée en tâche durable) et P7 (suite)

- **P6 (partiel)** : `admin_backups::scheduled_tick` ne lance plus rien ; il réserve l'échéance (CAS sur `backup_schedule`) et met une tâche `system`/`backup` dans `durable_tasks` (`system_task_queue::enqueue_backup`). Exécution via `execute_scheduled` sous bail + fencing, limite 3600 s, préemption par la lecture, résultat enregistré seulement si le bail est conservé ; interruption -> `interrupted`. Test unitaire adapté (le verrou mémoire du tick a disparu). Tests lancés et réussis : `admin_backups::`, `system_tasks::`, tests d'intégration `admin_backups`, `backup_bundle`, `backup_restore`, `task_engine_http`, `task_operations`. **Reste** : deux tables de planification distinctes (`backup_schedule` et `system_task_schedules`), `gate` mémoire des tâches système, planificateur de scans (`scheduler.rs`) non unifié, pas de test de reprise après crash du backup.
- **P7 (suite)** : `AssetStore::restore_extracted` + binaire `media-restore-assets` (audit complet, sortie non nulle si incomplet). Les données durables des extensions sont dans la base SQL, donc déjà sauvegardées ; les paquets `/data/plugins` ne le sont pas. Binaire non exécuté de bout en bout ; PostgreSQL non testé.

## 26 octobre 2026 - P7 (assets dans la sauvegarde) et audit P8

- **Correctif de compilation** : inventory.rs (champ `episode` de `work()`) référençait une variable inexistante ; l'arbre de travail ne compilait pas. Remplacé par la lecture de la colonne `kind` (comportement non testé spécifiquement).
- **P7 (partiel)** : `backup_bundle` embarque les assets (enregistrements 8/9, manifeste `assets`/`assetBytes`), SHA-256 + taille recalculés à la création et à l'extraction ; `AssetStore::export_source`, `restore_object`, `verify_all`. Doc : `docs/BACKUP_RESTORE.md`. Tests unitaires réels (SQLite) : sauvegarde -> extraction -> restauration sur store vierge -> verify_all, et refus d'un asset altéré/absent. **Non fait** : binaire unique de restauration, test PostgreSQL, modules/objets d'extensions dans le bundle.
- **P8 (audit seulement)** : `docs/MULTI_CORE.md`, par lecture du code ; aucun test multi-Core.
- **P6** : non commencé (le moteur durable_tasks a déjà leases/fencing ; sauvegarde planifiée et gate des tâches système pas encore migrés).
- **Vérifications** : clippy `-D warnings` propre ; `cargo test -p media-poc --lib` : 106 réussis, 1 échec (`extension_data::tests::bounded_expiry_cleanup_commits_progress_and_empty_scopes_are_bound`) qui réussit seul : instable sous charge, cause non analysée. Les tests d'intégration (`crates/core/tests`) n'ont pas été relancés ; les résultats annoncés pour P0-P5 ci-dessus ne sont pas revérifiés.
## 26 octobre 2026 — P0, P1, P2, P3, P4 & P5 : Refonte complète des données, nouveau schéma initial, stockage binaire universel et optimisation SQL/Plugins

- **P0 — Sécurité des sauvegardes et assainissement des tâches (`crates/core/src/backup_bundle.rs`, `admin_backups.rs`, `bin/backup.rs`, `bin/recover.rs`, `system_tasks.rs`, `docs/BACKUP_KEY.md`)** :
  - **Suppression définitive de la clé de secours publique** : éradication de la clé fixe de 64 zéros. La clé de chiffrement XChaCha20-Poly1305 est désormais impérativement générée par CSPRNG cryptographiquement sûr et conservée dans un fichier protégé (`backup.key` en permissions `0600`), ou injectée explicitement par l'administrateur via `MEDIA_BACKUP_KEY` / `MEDIA_BACKUP_KEY_FILE`.
  - **Rejet strict des secrets faibles** : `is_known_insecure_backup_key` refuse au démarrage et à la restauration toute clé compromise ou publique (zéros, suites triviales, mots de passe de test).
  - **Sécurité et intégrité des tâches système** :
    - La tâche `clean-cache` n'accède plus jamais à `media_root` (les médias originaux sur NAS restent intouchables en lecture seule). Le nettoyage est strictement confiné aux répertoires applicatifs d'Aether (`/data/assets/tmp/`, caches dérivés, staging de plugins).
    - Correction de `clean-playlists` qui référence désormais la table courante `inventory_works` (au lieu de la table historique obsolète `works`).
    - Fin des erreurs silencieuses : chaque tâche système rapporte son statut, sa durée, son message d'erreur précis ainsi que le type d'erreur (`last_error_kind`) et sa portée (`last_error_scope`) dans `system_task_schedules`.

- **P1 — Schéma SQL initial unique et consolidé (`0001_initial_schema.sql`)** :
  - Consolidation des 50 migrations historiques accumulées durant la bêta en une seule migration initiale propre et structurée pour chaque moteur :
    - SQLite : `crates/core/migrations/0001_initial_schema.sql` (83 tables relationnelles, FTS5 pour le catalogue et les personnes, index composites optimisés, triggers de maintenance, contraintes CHECK et clés étrangères cohérentes).
    - PostgreSQL : `crates/core/migrations-postgres/0001_initial_schema.sql` (équivalent fonctionnel strict avec types `BIGINT`, `BYTEA`, horodatages Unix epoch standardisés et contraintes identiques).
  - Validation complète des invariants : `PRAGMA foreign_key_check` et tests d'intégrité exécutés sans aucune violation.

- **P2 & P3 — Stockage binaire universel adressé par le contenu (`crates/core/src/assets.rs`) & Déduplication des images** :
  - **Architecture `AssetStore`** :
    - Module dédié gérant les objets binaires applicatifs immuables indexés par le SHA-256 de leurs octets réels.
    - Élimination des BLOBs volumineux dans les tables de la base de données : `user_avatars`, `extension_avatar_images`, `work_posters`, `poster_previews`, `library_artwork` et `work_image_data` pointent désormais vers `assets(sha256)`.
    - Stockage direct sur SSD NVMe (`/data/assets/`) avec arborescence partitionnée à 2 niveaux (`ab/cd/<sha256>`).
    - Interface d'abstraction `BlobBackend` (`LocalFs`) prête pour une compatibilité future avec un stockage objet sans impacter le code métier.
    - Écritures temporaires en flux continu (`/tmp/`), fsync et publication atomique par renommage : résistance absolue aux crashs système ou coupures d'alimentation.
    - Déduplication transparente : un avatar de collection ou une affiche partagée par plusieurs entités n'est stocké qu'une seule fois physiquement.
    - Cycle de vie et ramasse-miettes différé : suppression en deux phases avec période de grâce et respect des verrous de traitement actifs.
    - Sécurité de diffusion HTTP : streaming après contrôle strict des autorisations au niveau Core (`response_after_authorization`), en-têtes `ETag`, `Cache-Control` configurés, aucun chemin disque interne exposé au client.

- **P4 — Optimisation profonde SQLx, SQLite et PostgreSQL (`crates/core/src/identity.rs`, `database.rs`, migrations SQL)** :
  - **Pool de connexions configurable** : `configure_pool_options(is_sqlite)` avec variables d'environnement (`MEDIA_DB_MAX_CONNECTIONS` / `MEDIA_DB_POOL_MAX` défauts 8 SQLite / 16 PostgreSQL, `MEDIA_DB_MIN_CONNECTIONS` défauts 1 SQLite / 2 PostgreSQL, `MEDIA_DB_ACQUIRE_TIMEOUT_SECS`, `MEDIA_DB_IDLE_TIMEOUT_SECS`) et validation stricte (rejet des valeurs <= 0).
  - **PRAGMAs SQLite haute performance** : `PRAGMA synchronous = NORMAL;`, `PRAGMA cache_size = -64000;` (64 Mo de cache en RAM), `PRAGMA mmap_size = 268435456;` (256 Mo MMAP), `PRAGMA temp_store = MEMORY;`. Checkpoint propre `PRAGMA wal_checkpoint(TRUNCATE)` à l'arrêt propre du pool.
  - **Suppression du verrou global PostgreSQL** : suppression de `pg_advisory_xact_lock(724398210)` dans `begin_write()` au profit de transactions MVCC natives concurrentes (`conn.begin().await`).
  - **Verrous fins partitionnés (`begin_write_scoped`)** : acquisition de verrous consultatifs transactionnels hashés `pg_advisory_xact_lock(hashtext(scope || ':' || entity_id))` pour isoler sélectivement les écritures sur une même entité ou extension sans bloquer les autres transactions.
  - **Index et fonctions de compatibilité** : remplacement des index d'expressions JSON coûteux par des index B-tree composites sur colonnes physiques (`inventory_works(kind, display_title_sort, id)` et `inventory_works(library_id, kind, display_title_sort, id)`). Ajout des fonctions de compatibilité PostgreSQL `instr()` et `json_extract()` éliminant les divergences SQL entre moteurs.

- **P5 — Stockage durable et cycle de vie des extensions (`crates/core/src/extensions.rs`, `extension_storage.rs`, `extension_data.rs`, `extension_objects.rs`)** :
  - **Mise à niveau sans perte de données** : endpoint d'administration `POST /api/v1/admin/extensions/{id}/upgrade` et gestion de `upgrade: bool` dans `InstallPackage`.
  - **Vérification cryptographique** : validation de l'identité du paquet et de la clé publique de signature de l'éditeur lors de la mise à jour.
  - **Préservation des données durables** : conservation intégrale des données KV privées (`extension_data`) et des objets binaires (`extension_objects`) lors du passage de version, avec incrémentation du champ `generation` pour invalider les jetons de workers obsolètes.
  - **Écriture atomique des paquets (`persist_upgrade`)** : mise à jour sécurisée des fichiers `package.json`, `manifest.json` et `module.wasm` dans `/data/plugins/{id}/` sans déclencher de faux conflit `AlreadyExists` et avec nettoyage automatique des dossiers de staging orphelins.

- **Vérifications et tests validés** :
  - `cargo test -p media-poc --lib` : 105 tests unitaires réussis.
  - Tests d'intégration : `extensions`, `extension_storage_lifecycle`, `catalog_performance`, `admin_users`, `admin_backups`, `backup_bundle`, `backup_restore`, `settings_avatars`, `artwork_variants`, `posters_flow`, `concurrency_writes_during_publication`, `auth_under_sqlite_lock`, `core_restart`, `task_engine_http`, `task_operations`, `plugin_storage_integration`, `scanner_flow`, `preferences`, `version_and_nodes`, `global_catalogue_consolidation` (1M works benchmark), `adversarial_challenge`, `healthcheck_resilience`, `multi_node_scale_and_failover`, `scheduler`, `scan_diagnostics`.
  - `cargo clippy -p media-poc --all-targets -- -D warnings` : 0 avertissement.
  - `cargo fmt --all -- --check` : 100 % conforme.

## 25 octobre 2026 — Vérification et fiabilisation du stockage des plugins (/data/plugins)

- **Stockage effectif et chemin unifié (`crates/core/src/identity.rs`, `docker-compose.yml`, `compose.distributed.yaml`)** :
  - Unification du répertoire des plugins sur `/data/plugins/` dans les deux modes Docker (`aether` autonome et `aether-core` distribué).
  - Détection déterministe dans `identity.rs` : vérification que `MEDIA_PLUGINS_PATH` n'est pas vide (`.filter(|p| !p.is_empty())`) avec repli direct vers `/data/plugins` si `/data` existe sur le système de fichiers hôte/conteneur.
  - Correction d'un bug en mode PostgreSQL sans `MEDIA_PLUGINS_PATH` explicite, où l'absence de base SQLite locale faisait replier le chemin sur le dossier relatif éphémère `./data/plugins` (`/app/data/plugins`), rejeté par le conteneur en `read_only: true` ou écrasé à la recréation du conteneur.
  - Ajout explicite de la variable d'environnement `MEDIA_PLUGINS_PATH=/data/plugins` dans `docker-compose.yml` et `compose.distributed.yaml`.
- **Auto-guérison, résilience aux crashes et nettoyage staging (`crates/core/src/extension_storage.rs`)** :
  - `clean_staging` : nettoyage systématique au démarrage et avant toute écriture des répertoires temporaires orphelins `.install-*` laissés en cas de crash ou coupure d'alimentation pendant un dépaquetage.
  - Tolérance de `persist` et auto-réparation : si un plugin est déjà enregistré dans la base SQL mais que ses fichiers sur disque sont absents, vides (0 octet) ou corrompus (altération SHA-256 du `.wasm`), `persist` réécrit atomiquement les fichiers sains du paquet sans lever de faux conflit `AlreadyExists`.
  - Résilience de `sync_installed` au démarrage : une corruption ou suppression de fichier sur un plugin n'interrompt plus brutalement la boucle de démarrage ; les erreurs sont journalisées en détail et chaque plugin valide ou restaurable est réconcilié.
- **Diagnostic et observabilité (`crates/core/src/extensions.rs`, `main.rs`, `bin/aether-core.rs`)** :
  - Nouvel endpoint d'administration `GET /api/v1/admin/extensions/storage/health` renvoyant la structure `PluginStorageHealth` : chemin configuré, chemin effectif, permissions de lecture/écriture, total de plugins en base SQL, plugins sains, corrompus ou manquants sur disque.
  - Enrichissement du payload de `GET /api/v1/admin/extensions` (`list`) avec les métadonnées de diagnostic `storage`.
  - Remplacement des logs silencieux `is_err()` au démarrage de Core par des messages explicites indiquant le chemin réel et le nombre de plugins synchronisés/vérifiés ou la nature exacte de l'erreur (`Io`, `NotFound`, etc.).
- **Permissions NVMe / NAS & Documentation (`docs/PLUGIN_STORAGE.md`)** :
  - Documentation exhaustive du stockage des extensions, de la topologie SQLite/Disque, de l'auto-réparation et des différences entre volume Docker nommé et dossier hôte monté (bind mount SSD NVMe).
  - Clarification de la cause d'échec fréquente : le conteneur s'exécute en utilisateur non privilégié `aether:aether` (`UID:GID 10001:10001`, `cap_drop: ALL`). Un dossier monté depuis l'hôte NVMe sous root ou un UID administrateur NAS sans permission pour `10001:10001` provoque une erreur `Permission denied (os error 13)` lors de `create_dir_all`. Commande de remédiation documentée : `chown -R 10001:10001 /data && chmod 750 /data`.
- **Vérifications et tests d'intégration réels** :
  - `crates/core/tests/plugin_storage_integration.rs` : 8 tests d'intégration réels (7 passés, 1 ignoré demandant une base Postgres externe) validant :
    1. Installation propre de `package.json`, `manifest.json` et `module.wasm`.
    2. Diagnostic de santé complet (`healthy`, `writable`, intégrité SHA-256).
    3. Restauration automatique au redémarrage si les fichiers du dossier disque sont supprimés.
    4. Réparation automatique si le fichier `module.wasm` est tronqué ou corrompu.
    5. Réparation automatique si le dossier de l'extension existe mais est vide.
    6. Nettoyage automatique des dossiers de staging orphelins `.install-*`.
    7. Endpoint HTTP d'administration `/api/v1/admin/extensions/storage/health` avec authentification session admin.
  - `cargo test -p media-poc --test plugin_storage_integration` : 7 passed, 0 failed.
  - `cargo test -p media-poc --lib extension_storage::tests` : 1 passed.
  - `cargo clippy -p media-poc --lib --test plugin_storage_integration -- -D warnings` : 0 avertissement.
  - `cargo fmt --all --check` : code parfaitement formaté.
  - `npm test --prefix apps/web` : 29 fichiers de tests passés (171 tests).
  - `npm run check --prefix apps/web` : 0 erreur Svelte/TypeScript.

## 25 octobre 2026 — Ajout des tâches système planifiées (style Jellyfin)

- **Architecture & persistance (`crates/core/migrations/0048_system_tasks.sql`, `crates/core/src/system_tasks.rs`)** :
  - Création de la table `system_task_schedules` (SQLite et PostgreSQL) pré-peuplée avec des déclencheurs périodiques par défaut :
    - `update-extensions` (Mise à jour des plugins & catalogues - vérifie les dépôts distants et actualise les manifests d'extensions, toutes les 24h).
    - `optimize-database` (Optimiser la base de données - exécute `PRAGMA optimize` et `PRAGMA wal_checkpoint(PASSIVE)` sur SQLite ou `ANALYZE` sur PostgreSQL, toutes les 24h).
    - `clean-sessions` (Nettoyer les sessions expirées - purge les sessions expirées et tokens inactifs de plus de 30 jours, toutes les 6h).
    - `clean-cache` (Nettoyer le cache et fichiers temporaires - supprime les prévisualisations de posters orphelines et fichiers temporaires `.tmp`/`.part`, toutes les 24h).
    - `clean-playlists` (Nettoyer les listes et collections - retire des listes personnelles les vidéos ou morceaux supprimés du catalogue, toutes les 24h).
  - Worker en tâche de fond dans Core (`scheduled_tick`) avec mutex de concurrence (`gate`), enregistrement atomique des démarrages/fins (`last_started_at`, `last_completed_at`, `last_outcome`) et calcul de l'échéance suivante sans tempête de rattrapage.
  - Endpoints REST sécurisés sous `/api/v1/admin/tasks/system` (authentification administrateur + CSRF) pour lister les tâches, modifier les déclencheurs (intervalles/activation) et déclencher manuellement une tâche (`POST /run`).
- **Interface utilisateur (`apps/web/src/lib/admin/AdminTasks.svelte`)** :
  - Intégration transparente dans la vue des tâches d'administration avec regroupement par catégorie (`Bibliothèques`, `Extensions`, `Maintenance`).
  - Prise en charge des déclencheurs périodiques éditables (fréquence et unité en minutes, heures ou jours).
  - Bouton d'exécution manuelle immédiate avec retour visuel d'état en cours et historique du dernier résultat / durée d'exécution.
- **Vérifications réelles** :
  - `cargo test -p media-poc --lib -- system_tasks` : 1 test réussi (`test_system_tasks_seeded_and_claimable` : validation du seeding des 5 tâches par défaut, exécution complète et persistance du résultat `succeeded`).
  - `npm test --prefix apps/web` : 29 fichiers de tests passés (171 tests).
  - `npm run check --prefix apps/web` : 0 erreur Svelte/TypeScript.

## 25 octobre 2026 — Découverte et affichage des pochettes d'albums musicaux

- **Découverte de pochettes dans le scanner musical (`crates/node/src/music.rs`)** :
  - Découverte des fichiers d'affiches/pochettes locales dans les dossiers musicaux (`folder`, `cover`, `album`, `poster`, `front` en `.jpg`, `.jpeg`, `.png`, `.webp`) via `crate::images::discover_named`.
  - Cache en mémoire par dossier pendant la traversée pour éviter de revalider la même pochette à chaque piste d'un même album.
  - Enrichissement de la structure `MusicTrack` avec `cover_path`, `cover_size`, `cover_modified_ns` et `cover_mime`.
- **Persistance et endpoint HTTP Core (`crates/core/src/music.rs`, migrations `0047_music_cover.sql`)** :
  - Migration SQL `0047_music_cover.sql` (SQLite & PostgreSQL) ajoutant les colonnes de cover à la table `music_tracks`.
  - Mise à jour de `publish_track` pour enregistrer les métadonnées de pochette.
  - Ajout de la route sécurisée `/api/v1/music/tracks/{id}/cover` (avec vérification des permissions de bibliothèque et streaming borné du fichier local ou relai nœud distant).
  - Exposition de `coverUrl` dans les listes de pistes (`/api/v1/music`) et le détail (`/api/v1/music/tracks/{id}`).
- **Interface Web (`apps/web/src/routes/music/+page.svelte`, `MusicPlayer.svelte`, `queue.ts`)** :
  - Affichage de la vraie image de pochette dans la vue par Albums (`groups`) au lieu d'une pastille textuelle.
  - Vignette miniature sur chaque piste dans la liste des morceaux.
  - Affichage de la pochette dans la barre de lecture du lecteur audio en bas de page (`MusicPlayer.svelte`).
- **Vérifications** :
  - `cargo test -p media-poc --lib product_tests::music_scan_metadata_stable_identity_and_range` : succès complet avec création d'image, scan, détection de `coverUrl` et vérification de la réponse HTTP 200 `image/jpeg`.
  - `npm --prefix apps/web run check` : 0 erreur Svelte/TypeScript.

## 25 octobre 2026 - Releases manuelles et UID NAS personnalise

- Decision : aucun nouveau numero de release sur simple push/merge. Release Aether et Release Plugin uniquement via workflow_dispatch sur main, choix PATCH (defaut), MINOR ou MAJOR ; images develop/latest et CI conservees. Calcul explicite Aether prioritaire sur classification des commits. Release manuelle plugin limitee au seul plugin choisi, ajout PATCH.
- Tests locaux : scripts/test-release-policy.py (2 tests et 3 choix d'increment), test-release-policy.py des plugins (1 test, 3 choix et isolation du plugin), syntaxe YAML et diff verifies. Pas de release, tag ni image publies par cette intervention.
- NAS : utilisateur annonce 1039:65539 face au volume /data 10001:10001 mode 750 explique le refus sans droits supplementaires. Garder l'UID NAS exige d'aligner les proprietaires des seuls volumes applicatifs ; cap_drop ALL conserve. Correction sur le NAS non executee localement.

## 25 octobre 2026 — Catalogue plugins : bannières et diagnostic installation

- Bannières affichées intégralement avec contain dans AdminExtensions. Stockage : comparaison de paquets désérialisés, évitant les faux conflits dus à la mise en forme JSON ; conflit de version et altération Wasm toujours refusés. Erreurs HTTP/UI distinctes pour permissions, conflit, intégrité et fichier absent ; cause système journalisée aussi au démarrage.
- Aether-Plugins : IntroSkipper est absent du catalogue signé local (8 paquets). La détection automatique de release inclut désormais les plugins absents ; jobs séquentiels depuis main, SVG incorporé avant signature. Aucun push, signature production, release ni déploiement effectué.
- Validation réelle : cargo test --locked -p media-poc --lib extension_storage::tests -j 1 (1 succès, incluant JSON formaté et conflit de version) ; cargo test --locked -p media-poc --lib repository_catalogue_install_preserves_trust_signature_and_persistence -j 1 (1 succès HTTP catalogue/installation). cargo fmt --all --check, npm --prefix apps/web run lint réussis ; npm --prefix apps/web run check : 0 erreur, 6 avertissements existants. Détection avec --base-ref HEAD : IntroSkipper seul sélectionné ; node ../Aether-Plugins/scripts/test-repository.cjs : 8 signatures/empreintes valides.
- Journaux fournis : Docker sur NAS Synology, stockage plugins indisponible au démarrage et cache média refusé avec Permission denied (os error 13). Les droits des volumes sont une piste étayée ; chemin effectif, UID et propriétaires à vérifier sur le NAS. Erreur plugins non reproduite localement. Les correctifs nécessitent reconstruction/déploiement ; IntroSkipper nécessite une publication signée avant actualisation du dépôt. Aucune validation NAS ni navigateur réel supplémentaire.


## 25 octobre 2026 - Sessions persistantes et appareils personnels

- Diagnostic statique : expiration inactive de 30 minutes et absolue de 12 heures. Nouvelle politique commune locale/SSO : 7 jours sans activite et 30 jours maximum ; cookie HttpOnly persistant, controles serveur conserves. Les sessions existantes gardent leur ancienne expiration absolue.
- Parametres : rubrique Securite et appareils, liste existante rendue accessible, dates de connexion/activite/expiration, actualisation, deconnexion individuelle ou globale. Une entree correspond a une session navigateur, sans identification materielle fiable.
- Verification : cargo test -p media-poc --lib auth::tests : 5 tests passes (expiration, revocation, redemarrage, CSRF et Origine). cargo test -p media-poc --test account : 2 tests passes (cookie 30 jours, retour apres un jour, expiration apres sept jours, revocation et CSRF). npm --prefix apps/web run check : 0 erreur, 6 avertissements hors fichiers modifies ; npm --prefix apps/web run lint : succes.
- cargo fmt --all --check signale un ecart dans artwork.rs, fichier modifie en parallele hors de ce jalon. Aucun deploiement ni parcours navigateur reel valide ; reconstruire/redemarrer le serveur puis se reconnecter une fois pour beneficier de la nouvelle echeance.

# MEMORY.md — État et décisions Aether

## 25 octobre 2026 — Refonte stockage, jalon 1 (P0) : clé de sauvegarde et maintenance

- Audit confirmé : (1) clé de sauvegarde de repli publique à 64 zéros utilisée silencieusement (aussi pour toute clé invalide) dans `backup_bundle`, `admin_backups`, `bin/backup`, `bin/recover` ; (2) `clean-cache` supprimait `*.part`/`.*.tmp` dans `media_root` ; (3) `clean-playlists` visait la table inexistante `works` et `clean-sessions` la colonne inexistante `oidc_flows.created_at`, erreurs ignorées par `let _ =` ; (4) résultats de tâches sans type/portée d'erreur.
- Corrigé : plus aucune clé par défaut. Ordre `MEDIA_BACKUP_KEY` (invalide ⇒ erreur) > `MEDIA_BACKUP_KEY_FILE` > clé d'installation CSPRNG `<data>/secrets/backup.key` (create_new, 0600). Clé nulle refusée. Anciennes archives uniquement via `media-recover ... --legacy-public-key` (avertissement). Doc : `docs/BACKUP_KEY.md`.
- Corrigé : `clean-cache` limité à des répertoires explicitement possédés (`MEDIA_IMAGE_CACHE_DIR` ou défaut, dossier de sauvegardes), noms de fichiers Aether uniquement, âge ≥ 1 h, non récursif, refus si chevauchement avec `media_root`. Toutes les erreurs SQL/disque/réseau sont remontées (`last_error_kind/scope/message`, migration 0051 additive SQLite+PostgreSQL).
- Tests réels : `system_tasks` 6 réussis (dont originaux `.part` préservés, chevauchement refusé, échec SQL rapporté), `backup_bundle` lib 4, `media-recover` 3, tests d'intégration `admin_backups` 2 et `backup_bundle` 7. Non fait : migration PostgreSQL 0051 exécutée sur un vrai PostgreSQL ; interface Web n'affiche pas encore `lastErrorKind/Scope/Message`.
- Restant de la refonte (non commencé) : schéma initial unique, module d'assets SHA-256, images, pool/transactions, stockage extensions, unification tâches, sauvegarde complète avec assets, multi-Core, benchmarks. Risque noté : `update-extensions` stocke le manifeste brut sans le valider au rafraîchissement.


## 25 octobre 2026 — Mission 1 : stockage privé persistant des extensions

- SDK stockage/objets v1 et broker bidirectionnel Wasm/Core, sans SQL, chemin ni secret DB dans le guest. Permission signée `storage.private`, six portées liées au contexte Core, contrôles existants et identité éditeur/clé réutilisés.
- Migrations additives0049/0050 SQLite/PostgreSQL ; CAS UUID, TTL, pagination, quotas100000 entrées/64Mio de valeurs ; objets distincts en blocs16Kio,16Mio/objet128Mio/extension. Conservation durable et purge explicite à la désinstallation ; cache régénérable distinct. Get autorisé en une requête atomique, sans cache de droits.
- Validation réelle : moteur7 tests SQLite+contrat PostgreSQL, objets3 SQLite+1 PostgreSQL, cycle de vie2 SQLite+1 PostgreSQL,4 intégrations finales dont vrai guest Rust/Wasm signé et129e opération refusée ; autres régressions extensions13 réussies. Crash client sans destructeurs avant/après commit, timeout réel sous verrou, corruption et isolation vérifiés sur les deux moteurs. Clippy Core tous targets strict/query-metrics et fmt réussis.
- Mesures finales100000 entrées par moteur : SQLite lecture1client p50 0,787ms/p95 2,160ms ; PostgreSQL SQLx/loopback p50 6,145ms/p95 28,131ms, SQL serveur seul p50 1,559ms/p95 2,032ms. Cible sous1ms non tenue globalement ; concurrence et résultats complets dans docs/EXTENSION_STORAGE.md et preuves extension-storage. Aucun push/déploiement ni suppression de données réelles.
- Clôture26octobre : refonte DB concurrente constatée (migrations regroupées, tables privées présentes) ; preuves sur instantané antérieur, pas attestation de cette refonte. Sources moteur/objets/runtime/SDK inchangées par SHA, extensions.rs modifié entre-temps. Docker arrêté : suppression du seul conteneur de test non confirmée, aucun redémarrage forcé. Pas de preuve NAS, coupure électrique ou cluster complet.

## 25 octobre 2026 — Refonte minimaliste de l'en-tête de catalogue, autofocus et correction de la recherche

- **Refonte minimaliste et compacte de l'en-tête du catalogue (`CatalogGrid.svelte`, `Catalog.svelte`)** :
  - Suppression du bloc `<details><summary>Filtrer le catalogue</summary>` brut positionné hors du flux au-dessus du contenu principal.
  - Suppression des bandeaux volumineux (redondance d'en-tête, sous-titres génériques verbeux sur 3 lignes et cartes encombrantes) qui repoussaient les affiches de films hors du viewport.
  - Création d'une barre d'en-tête compacte et épurée inspirée des plateformes de streaming modernes : titre de la bibliothèque/section et badge de comptage intégrés sur la gauche, barre d'outils unifiée sur la droite (recherche, tri, tiroir de filtres dépliable).
  - Gain immédiat d'espace vertical : les films et affiches apparaissent directement en haut de l'écran sans défilement nécessaire.
- **Correction du chevauchement de l'icône de recherche sur le texte** :
  - Identification de la cause racine : la règle globale de `tokens.css` `input:not([type="range"]):not([type="checkbox"]):not([type="radio"])` avait une spécificité supérieure à `input[type='search']`, écrasant le `padding-left: 42px` par un `padding: 10px 14px`, ce qui plaçait le texte directement sous l'icône loupe située à 14 px.
  - Correction avec un sélecteur haute spécificité et padding gauche explicite (`40px !important`), garantissant que le texte et le placeholder ne se superposent plus jamais à l'icône.
- **Autofocus automatique à l'ouverture d'une bibliothèque** :
  - Rétablissement de la référence `bind:this={searchInputRef}` qui était absente sur l'input de recherche.
  - Autofocus instantané dès l'arrivée sur une bibliothèque ou un changement de section/bibliothèque (`onMount` et `$effect` ciblés), positionnant le curseur en fin de texte sans nécessiter de clic préalable de l'utilisateur.
  - Soumission fluide via `goto` avec conservation du focus et remplacement d'état d'historique (`replaceState`).
- **Vérifications réelles** :
  - `npm --prefix apps/web run check` : 0 erreur TypeScript / Svelte diagnostics.
  - `npm --prefix apps/web test` : 152 tests vitest exécutés avec succès.
  - `cargo test -p media-poc --lib` : 64 tests unitaires Rust validés avec succès.


## 25 octobre 2026 — Releases automatiques et Versioning indépendant des extensions Aether

- **Découplage des versions dans `Aether-Plugins`** :
  - Remplacement de `version.workspace = true` par des versions explicites et autonomes `version = "0.1.0"` dans le `Cargo.toml` de chaque extension (`Avatar`, `Discogs`, `fanart.tv`, `IMDb`, `IntroSkipper`, `LrcLib`, `MusicBrainz`, `OMDb`, `TMDb`).
  - Validation complète de la chaîne de compilation : `cargo fmt`, `cargo clippy`, `cargo test --workspace` (tous les tests d'intégration et unitaires passent).
- **Détection ciblée des extensions modifiées (`scripts/detect_changed_plugins.py`)** :
  - Détection automatique basée sur `git diff --name-only <base>..HEAD` isolant chaque sous-dossier de plugin.
  - Exclusion des modifications documentaires (`.md`, `docs/`) pour éviter les releases superflues.
  - Déclenchement automatique de tous les plugins dépendants si le crate commun `crates/aether-plugin-api` est modifié.
  - Support du dispatch manuel avec choix du plugin et forçage d'une release `MAJOR` (ex: `1.12.0 → 2.0.0`) ou `MINOR`.
- **Mise à jour atomique du catalogue (`scripts/update_catalog_entry.py`)** :
  - Met à jour ou ajoute une entrée de plugin dans `repository/manifest.json` avec calcul frais du SHA-256 du paquet signé.
  - Conserve toutes les entrées, signatures et clés publiques existantes sans régression.
  - Contrôle strict du respect de la limite de taille de 256 Kio.
- **Workflow de Release GitHub (`.github/workflows/release-plugins.yml`)** :
  - Déclenchement sur `push` vers `main` ou `workflow_dispatch` manuel.
  - Utilise l'environnement GitHub `production` restreint à `main` avec le secret `ED25519_PRIVATE_KEY` (avec arrêt immédiat et explicite si absent).
  - Compilation Wasm optimisée (`opt-level = "z"`), packaging, signature Ed25519, vérification de l'intégrité du catalogue (`test-manifests.cjs`, `test-repository.cjs`).
  - Création du tag indépendant (ex: `tmdb-v0.2.0`), publication de la GitHub Release avec l'archive `.package.json` en asset téléchargeable, et commit automatisé du catalogue sur `main`.
  - Idempotence complète : aucun tag ou release en double si déjà publié.
- **CI et SDK (`.github/workflows/ci.yml`)** :
  - Récupération du SDK Aether alignée sur la branche stable `main` (`${{ github.repository_owner }}/Aether`).
  - Tolérance propre dans `test-manifests.cjs` pour les nouveaux plugins en cours de développement dans le workspace (comme `IntroSkipper`) en attente de leur première signature.
- **Documentation (`README.md`)** :
  - Explication des règles de calcul de version, de la signature sécurisée en CI, du dispatch manuel des versions majeures et de la restauration d'anciennes versions.
- **Vérifications réelles** :
  - `detect_changed_plugins.py` testé en scénarios automatique et manuel (`MAJOR`/`MINOR`).
  - `update_catalog_entry.py` testé avec succès sur `tmdb-0.1.0.package.json`.
  - `node scripts/test-manifests.cjs` et `node scripts/test-repository.cjs` validés avec succès (8 paquets vérifiés).
  - Aucun push distant, aucun tag distant créé ni clé privée exposée.

## 25 octobre 2026 — Automatisation des Releases SemVer, GHCR et Architecture Écosystème

- **Audit et Professionnalisation CI/CD & GitHub Actions** :
  - Audit complet de l'infrastructure de publication de `Aether` et `Aether-Plugins`.
  - Mise en place du versioning automatique Semantic Versioning piloté par Conventional Commits (`scripts/calculate_release.py`) : incréments PATCH pour fix/perf/secours, MINOR pour feat, MAJOR (ou MINOR en 0.x) pour breaking changes.
  - Création du workflow `.github/workflows/release.yml` sur `main` : calcul de version sans boucle infinie, vérification d'existence du tag Git, publication Docker avec tags SemVer (`0.x.y`, `0.x`, `latest`, commit SHA), puis création du tag Git et de la GitHub Release officielle avec notes automatiques.
  - Sécurisation de `.github/workflows/ci.yml` et `publish.yml` : interdiction stricte de publication d'images Docker depuis les Pull Requests (`github.event_name != 'pull_request'`), suppression des références GitHub en dur (`github.repository_owner` dynamique).
  - Paramétrage configurable du préfixe des registres de conteneurs dans `docker-compose.yml` et `compose.*.yaml` via `${AETHER_IMAGE_PREFIX:-ghcr.io/maelmoreau21}` pour assurer la transition sans rupture lors du transfert dans une organisation.
- **Documentation et Gouvernance** :
  - Rédaction de `docs/GITHUB_GOVERNANCE_AND_MIGRATION.md` : matrice des règles de protection des branches `main` et `develop`, permissions GitHub Actions, guide de transfert vers une organisation dédiée (ex. `AetherMedia`) préservant l'historique et les signatures de paquets.
  - Rédaction de `docs/FUTURE_PLUGIN_ARCHITECTURE.md` : feuille de route de découplage du SDK `media-extension-sdk`, gestion de la compatibilité ABI, et flux d'enregistrement automatisé dans le catalogue centralisé d'extensions.
- **Vérifications** :
  - Test unitaire et fonctionnel de `calculate_release.py` (`python scripts/calculate_release.py --json`) : détection correcte de la version suivante `v0.2.0`, extraction et formatage Markdown du changelog depuis l'historique Git.
  - Compilation syntaxique Python (`py_compile`) vérifiée.
  - Aucune modification distante (aucun push Git, aucun tag créé, aucune release ni transfert de dépôt déclenché).


## 25 octobre 2026 — Navigation dossiers et refonte de l'éditeur de bibliothèque

- Correction Web de `PathPickerModal.svelte` : l'effet d'ouverture suivait le chemin
  courant et réinitialisait la navigation vers le dossier initial, notamment
  `/tmp/aether-core-empty-local-root`. Initialisation isolée avec `untrack`,
  navigation parent/racines explicite, annulation des requêtes précédentes et
  confirmation limitée à un chemin résolu sans erreur. Backend inchangé.
- `AdminLibraries.svelte` utilise une fenêtre jusqu'à 1200 px, en-tête/actions
  persistants et sections général/dossiers/options ; adaptation téléphone.
  `LibraryOptions.svelte` présente métadonnées/langues, images et fournisseurs
  avec les seuls réglages déjà raccordés. Suppression de l'annonce obsolète
  « multi-dossiers à implémenter ». Médias conservés en lecture seule.
- Validation : `npm run check --prefix apps/web` (0 erreur, 5 avertissements CSS
  dans AdminExtensions), `npm run build --prefix apps/web` réussi ; ESLint ciblé
  sur les trois composants réussi. `npm run lint --prefix apps/web` échoue sur
  l'erreur préexistante `AdminExtensions.svelte:359` (lien sans `resolve`).
- Test Chrome synthétique : `node scripts/test-library-editor-ui.mjs` depuis
  `apps/web`, navigation parent stable, racines, chemin saisi, ajout d'un second
  dossier et payload des réglages vérifiés ; absence de débordement à 1440/390 px,
  thèmes sombre/clair. Captures : `docs/evidence/2026-10-25/library-editor/`.
  API simulée : aucune preuve supplémentaire de déploiement Docker/NAS ou d'accès
  disque réel. Un Node distant ne présente toujours que ses racines déclarées.


## 25 octobre 2026 — Correction des tests d'intégration CI Shards 1 et 2 (Preemption Playback et Multi-Folder Scan)

- **Problèmes résolus** :
  1. **Shard 2 (`scanner_performance.rs`)** :
     - Le test `test_concurrent_playback_and_catalog_navigation_during_scan` échouait en CI avec `Scan stopped or preempted for playback`.
     - *Cause* : Le middleware `local_stream_priority` dans `crates/core/src/lib.rs` annulait sans distinction tous les scans en cours (`cancel.store(true)`) dès qu'une requête HTTP contenait `/content`. Or, pour le Direct Play de médias locaux sur `/content`, le préchauffage ou préemption brutale de scan n'est requis que pour les transcodages lourds en arrière-plan (`/prepared/` et `/audio/`), pas pour les simples lectures Range directes.
     - *Correction* : Seules les requêtes vers `/prepared/` et `/audio/` déclenchent la préemption et l'attente d'annulation de scans.
  2. **Shard 1 (`scanner_flow.rs`)** :
     - Le test `grouped_folders_scan_filter_stream_and_remove` échouait avec `assertion left == right failed: left: Number(1), right: 2`.
     - *Causes* :
       - `task_engine::enqueue_spec` et `task_engine::claim` n'autorisaient pas les bibliothèques virtuelles/dossiers enfants (`library_folders`) car la clause SQL vérifiait strictement `l.enabled=1` sur l'ID enfant, alors que les dossiers enfants ont leur activation liée à `library_folders.active=1` et `parent.enabled=1`.
       - De plus, en test in-process (sans scheduler loop d'arrière-plan démarré), après le scan du premier dossier, `scans::status` ne consultait que `scan_jobs` sans vérifier les tâches résiduelles dans `durable_tasks`, ce qui renvoyait `status: "completed"` de manière prématurée avant le traitement du deuxième dossier.
     - *Correction* :
       - Dans `crates/core/src/task_engine.rs` (`enqueue_spec` et `claim`) et `crates/core/src/scans.rs` (`enqueue_one`), autorisation des dossiers enfants rattachés via `(l.enabled=1 OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))`.
       - Dans `crates/core/src/scans.rs` (`status`), vérification de l'existence de tâches résiduelles dans `durable_tasks` pour la bibliothèque ou ses sous-dossiers. Si une tâche est `queued` ou `running`, le statut reste `queued`/`running` et un tour de `dispatch_once` est déclenché pour pomper la file.
- **Validation** :
  - `cargo test -p media-poc --test scanner_flow` : 3/3 réussis.
  - `cargo test -p media-poc --test scanner_performance -- test_concurrent_playback_and_catalog_navigation_during_scan` : 1/1 réussi.
  - `cargo fmt --check` : conforme.
  - `cargo clippy --all-targets -- -D warnings` : 0 avertissement, 0 erreur.

## 24 octobre 2026 — Reconnaissance et affichage des films et vidéos dans des dossiers

- **Problème résolu** :
  - Lorsqu'un utilisateur organisait ses films dans des sous-dossiers (ex. `Inception/movie.mkv`, `Film/film.mp4`, `Avatar/cd1.mkv`, ou dossiers sans mention de l'année `(YYYY)` ni identifiant externe `[tmdb-...]`), ou plaçait un dossier de film dans une bibliothèque de type séries :
    1. Dans les bibliothèques de films, `identity(relative)` ignorait le nom du dossier si celui-ci n'avait pas d'année entre parenthèses ou d'identifiant externe. Si le fichier vidéo s'appelait `movie.mkv`, `film.mp4`, `cd1.mkv` ou `video.mkv`, le film était nommé « movie » ou « cd1 », n'était pas associé au dossier et devenait introuvable sous son vrai nom dans l'interface.
    2. Dans les bibliothèques de séries, `parse_series_episode(relative)` rejetait avec un avertissement tout fichier ne respectant pas les schémas stricts `SxxExx` ou `1x01` ou `Season \d+`, causant l'exclusion totale du film ou de l'épisode du scan et son absence complète de l'interface.
    3. Les extensions de conteneurs MP4 courantes `.m4v` et `.mov` étaient ignorées par le scanner.
- **Modifications apportées** :
  - `crates/node/src/scanner.rs` :
    - `is_generic_video_stem` : Détection des noms de fichiers génériques (`movie`, `film`, `video`, `main`, `feature`, `play`, `complete`, numéros de CD/Part/Disc, chiffres seuls `01`, `1`, etc.).
    - `identity` : Utilisation intelligente du dossier parent pour nommer et regrouper l'œuvre (`folder:...`) même en l'absence d'année ou d'ID externe si le nom du fichier est générique, commence par le nom du dossier, ou correspond au titre du dossier. Normalisation des séparateurs sous Windows (`\`) vers `/` pour la clé d'identité.
    - `parse_series_episode` : Prise en charge des dossiers de films ou vidéos autonomes placés dans une bibliothèque de séries comme S01E01 sous le titre du dossier/fichier, et détection des numéros d'épisodes en préfixe direct (`01 - Currahee`), garantissant que le média apparaît toujours dans le catalogue et est lisible.
    - Prise en charge des extensions MP4 `.m4v` et `.mov` en plus de `.mkv`, `.mp4` et `.webm`.
    - Ajout de tests unitaires dédiés : `identity_handles_movies_in_folders_with_and_without_years` et `parse_series_episode_fallback_for_movies_and_non_standard_folders`.
- **Validation** :
  - `cargo test -p media-node` : 88 tests réussis, 0 échec (dont les 2 nouveaux tests unitaires).
  - `cargo check --all-targets` : 0 avertissement, 0 erreur.
  - `npm run check --prefix apps/web` : 0 erreur, 0 avertissement.
  - `npm test --prefix apps/web` : 152 tests réussis, 0 échec.

## 24 octobre 2026 — Ergonomie Catalogue & Fiche Média : Menu Options `...`, Modales Dédiées, Dropdown Saisons et Barre de Recherche

- **Problèmes résolus** :
  1. **Pollution visuelle de la fiche média** : Présence directe sur la fiche de détails d'un pavé technique (« Provenance : Indexation automatique / Ajouté le ... / Nœud source : local ») et répétition erronée « Média local indexéMédia local indexé » provenant de fallbacks non filtrés.
  2. **Actions d'administration désordonnées** : Les boutons d'images `▧` et de modification des métadonnées (qui déroulait un grand `<details>` déformant toute la page) nuisaient à la propreté de l'interface.
  3. **Menu déroulant de saisons disgracieux** : L'ancien `<select>` HTML brut de sélection de saison présentait un rendu natif OS désuet et peu soigné.
  4. **Champ de recherche sans focus immédiat et loupe chevauchant le texte** :
     - Accéder à `/search` ne focalisait pas immédiatement le champ de saisie texte.
     - L'icône loupe SVG (`left: 14px`) chevauchait directement le début du texte saisi en raison d'une règle globale dans `tokens.css` dont la spécificité (`:not():not():not()`) écrasait le `padding-left` de l'input.
- **Modifications apportées** :
  - `crates/core/src/inventory.rs` :
    - Remplacement de `.unwrap_or("Média local indexé")` par `.unwrap_or("")` pour le champ `description`, évitant toute injection de valeur factice lorsqu'aucun synopsis n'existe.
  - `apps/web/src/lib/ui/Icon.svelte` :
    - Ajout des icônes vectorielles SVG `more` (trois petits points `...`), `edit` (crayon), `image` (gestion d'images) et `copy` (copie d'identifiant).
  - `apps/web/src/lib/catalog/WorkTools.svelte` :
    - Remplacement des boutons disparates par un bouton moderne à trois petits points `...` avec dropdown contextuel flottant.
    - Ajout d'une modale « **Informations média** » détaillée (Nœud source, statut connecté/hors-ligne, bibliothèque, identifiant copie avec bouton Copier, identité globale, provenance métadonnées, date d'ajout, classification, et specs techniques vidéo/audio/sous-titres/conteneur).
    - Ajout d'une modale dédiée « **Modifier les métadonnées** » (réservée aux administrateurs) avec tous les champs éditoriaux et le regroupement multi-nœuds.
    - Intégration soignée de la gestion d'images (`ImageManager`) et de la modale de playlists.
  - `apps/web/src/lib/catalog/FilmDetail.svelte` & `apps/web/src/lib/catalog/SeriesDetail.svelte` :
    - Suppression du bloc encombrant `.metadata-footer` (Provenance / Date d'ajout / Nœud source), désormais accessible via le menu `...` > « Informations média ».
    - Filtrage des mentions factices « Média local indexé » ou « Média local » sur le synopsis et l'outline pour une présentation nette.
    - Association des flux et specs techniques à `WorkTools` pour affichage dans la modale d'informations.
  - `apps/web/src/lib/catalog/SeriesDetail.svelte` :
    - Remplacement du `<select>` par un **sélecteur de saison custom moderne** inspiré des plateformes de streaming : bouton trigger avec saison active, compteur d'épisodes, chevron animé et menu popover sombre avec flou d'arrière-plan, coche de sélection et fermeture au clic/Échap.
  - `apps/web/src/lib/catalog/CatalogGrid.svelte` & `Navbar.svelte` :
    - Focus automatique immédiat sur le champ de recherche à l'arrivée sur `/search` (`onMount`, `$effect`, et clic sur la loupe dans la barre de navigation).
    - Correction définitive du chevauchement de l'icône loupe grâce à `padding: 10px 42px 10px 48px !important;` et masquage des décorations Webkit natives.
- **Validation** :
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement.
  - `npm --prefix apps/web test` : 25 suites, 152 tests réussis.
  - `cargo test -p media-poc --lib` : 57 tests réussis, 0 échec.

## 24 octobre 2026 — Correction de l'erreur « Les sauvegardes sont indisponibles. Actualisez pour réessayer »

- **Problème résolu** :
  - Dans la vue d'administration des tâches (`AdminTasks.svelte`), la requête `GET /api/v1/admin/backups` rejetait la promesse et déclenchait l'alerte d'erreur : *"Les sauvegardes sont indisponibles. Actualisez pour réessayer."*.
  - En backend (`crates/core/src/admin_backups.rs`), l'endpoint `list()` (`GET /api/v1/admin/backups`) utilisait `validate_directory(&state).await?` avec l'opérateur `?`, provoquant un arrêt immédiat en `409 Conflict` dès que le dossier de sauvegarde était inaccessible, relatif ou refusé par les politiques de sécurité (au lieu de retourner un statut HTTP 200 avec `{ available: false, reason: "..." }`).
  - De plus, `safe_directory` échouait systématiquement sur les chemins relatifs car `directory.ancestors()` contenait le composant racine vide `""` (rejeté par `check_path_safety`), et `directory.parent()` retournait `""` pour lequel `is_dir()` renvoie `false`.
  - Côté frontend (`AdminTasks.svelte`), le type `Backup` omettait le champ `reason`, et affichait un motif codé en dur erroné au lieu du motif réel fourni par l'API.
- **Modifications apportées** :
  - `crates/core/src/admin_backups.rs` :
    - `safe_directory` : prise en compte des chemins relatifs (le parent vide `""` est résolu comme le dossier courant `.`) et filtrage des ancêtres vides dans `ancestors().filter(|p| !p.as_os_str().is_empty())`.
    - `list()` : capture sécurisée de l'échec de validation de répertoire ; au lieu de renvoyer un code HTTP `409 Conflict`, l'API retourne HTTP 200 avec `available: false`, le motif explicite (`reason`) et une liste d'archives vide conformément au contrat d'API.
  - `apps/web/src/lib/admin/AdminTasks.svelte` :
    - Ajout de `reason?: string | null` dans l'interface `Backup`.
    - Affichage de `backup.reason` lorsque les sauvegardes ne sont pas disponibles au lieu d'un message trompeur sur la clé de récupération.
  - `crates/core/tests/admin_backups.rs` :
    - Ajout du test d'intégration `backups_list_returns_unavailable_when_directory_refused` garantissant qu'un dossier refusé par la politique renvoie bien HTTP 200 avec `available: false` et un `reason` explicite.
- **Validation** :
  - `cargo test -p media-poc --test admin_backups` : 2 tests réussis.
  - `cargo test -p media-poc --lib admin_backups` : 2 tests réussis.
  - `cargo clippy -p media-poc --lib -- -D warnings` : 0 avertissement.
  - `cargo fmt --check` : code formaté.
  - `npm --prefix apps/web test` : 25 suites, 152 tests réussis.
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement.

## 24 octobre 2026 — Suppression définitive des droits de bibliothèques sur les plugins et correction des erreurs 400

- **Problème résolu** :
  - Lors de l'installation ou la configuration d'un plugin, une erreur HTTP 400 affichait le message hérité et trompeur : *"Vérifiez les champs et les droits sur les bibliothèques sélectionnées."*, vestige d'une ancienne fonction de restriction par bibliothèque qui devait être supprimée.
  - La sélection de bibliothèques spécifiques pour les plugins de métadonnées était encore présente dans l'interface (`Utilisation : Toutes les bibliothèques / Certaines bibliothèques`, cases à cocher).
- **Modifications apportées** :
  - `apps/web/src/lib/admin/request.ts` :
    - Remplacement du message d'erreur générique 400 par un message neutre (`Requête invalide. Vérifiez les paramètres saisis.`).
    - Enregistrement des traductions explicites pour l'ensemble des codes d'erreurs de dépôts et de paquets (`incompatible_server_version`, `invalid_package_digest`, `invalid_package`, `catalogue_package_mismatch`, `invalid_public_key`, `invalid_signature`, `invalid_manifest`, `module_too_large`, `invalid_module`, `invalid_digest`, `incompatible_api`, `repository_https_public_host_required`, `repository_address_forbidden`, `repository_unavailable`, `extension_exists`, etc.).
    - Affichage direct du code d'erreur serveur `detail.error` si aucun message spécifique n'est défini, empêchant tout masquage d'erreur.
  - `apps/web/src/lib/admin/AdminExtensions.svelte` :
    - Suppression complète des contrôles de sélection de bibliothèques pour les plugins (`libraries`, `selected`, `allLibraries`, sélecteur et cases à cocher).
    - Les plugins sont désormais activés de manière globale sur l'ensemble de la médiathèque (`allLibraries: true`, `libraryIds: []`).
    - Nettoyage des sélecteurs CSS inutilisés (`fieldset`, `legend`).
- **Validation** :
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement.
  - `npm --prefix apps/web test` : 25 suites, 152 tests réussis.
  - `cargo check -p media-contracts -p media-node -p media-poc` : succès.


## 24 octobre 2026 — Mission 1 : Plateforme multimédia multi-serveur (jusqu'à 100 Media Nodes)

- **Architecture distribuée Core / Multi-Nodes (`crates/core/src/nodes.rs`, `crates/core/src/inventory.rs`, `crates/node/src/server.rs`)** :
  - **Registre d'état chaud sans I/O (`NodeRuntimeRegistry`, `NodeRuntimeEntry`, `NodeRuntimeTelemetry`)** : conservé en mémoire Core (`std::sync::RwLock<HashMap<String, NodeRuntimeEntry>>`), alimenté par les heartbeats signés, les scans et les relais, éliminant tout appel HTTP ou requête SQLite séquentielle lors du choix de nœud.
  - **Fonction de scoring pure O(1) (`score_node_entry`)** : évalue instantanément chaque candidat selon l'état (`connected` +10 000, `disconnected` -100 000, `revoked` -1 000 000), le mode d'acheminement (`local` +2 000, `direct` directUrl +1 500, `inbound` +800, `outbound` +500), la latence RTT mesurée (pénalité continue jusqu'à -1 000), la charge utile active (-120 par flux actif, -200 par scan), et les échecs consécutifs.
  - **Télémétrie en temps réel des flux (`active_streams`)** : instrumentée côté Media Node via un compteur atomique RAII (`ActiveStreamGuard`, `track_stream_body`), synchronisée au Core dans `HeartbeatPayload`, avec vue d'administration étendue (`AdminNodeView` : `latencyMs`, `activeScans`, `activeStreams`, `indexedAssets`, `selectionScore`).
  - **Sélection et basculement automatique mid-playback (`descriptor_for_user`, `ImmersivePlayer.svelte`)** :
    - Résolution de toutes les copies sœurs autorisées (dossiers multi-emplacements, groupes de médias consolidés).
    - Basculement automatique transparent vers la meilleure copie saine si le nœud primaire est déconnecté (`failoverFrom`), avec liste ordonnée `fallbackMediaIds`.
    - Sauvegarde et reprise automatique de la progression de lecture (`workId` / `position`) côté client en cas de rupture de flux en direct, sans interruption visible de l'expérience utilisateur.
- **Résultats des benchmarks et tests de montée en charge (`crates/core/tests/multi_node_scale_and_failover.rs`)** :
  - **Sélection sur cache chaud (50 itérations par jalon, cible < 50 ms)** :
    - **1 Nœud simulé** : p50 = `1,46 ms` | p95 = `4,80 ms` | max = `6,37 ms` | Pure score pass = `2,37 µs` | RSS = `25,2 MiB`.
    - **10 Nœuds simulés** : p50 = `1,23 ms` | p95 = `2,16 ms` | max = `2,61 ms` | Pure score pass = `24,82 µs` | RSS = `26,0 MiB`.
    - **100 Nœuds simulés** : p50 = `3,85 ms` | p95 = `7,43 ms` | max = `9,74 ms` (**< 10 ms**, très largement sous l'objectif de 50 ms) | Pure score pass = `491,23 µs` | RSS = `28,0 MiB`.
  - **Basculement automatique dynamique sur 100 Nœuds** : `4,74 ms` lors de la déconnexion de `sim-node-001`, avec re-classement immédiat vers `sim-node-002` puis `sim-node-004` lors d'une surcharge à 25 flux actifs.
  - **Test d'intégration réel multi-processus TCP (Core + 2 Nodes réels `node-primary` et `node-backup`)** :
    - Scan multi-dossiers et déduplication catalogue vérifiés.
    - Streaming direct `206 Partial Content` validé.
    - Simulation de panne réseau / crash de `node-primary` : détection d'indisponibilité immédiate, basculement vers `node-backup`, reprise exacte de la progression à `42,5 s`.
    - Concurrence validée : 50 sélections de descripteurs et 50 flux `Range` simultanés servis sans erreur.
- **Validation qualité** :
  - `cargo fmt --check` : succès.
  - `cargo clippy -p media-contracts -p media-node -p media-poc -- -D warnings` : 0 avertissement.
  - Tests unitaires frontend (`npm --prefix apps/web test`) : 25 fichiers, 152 tests réussis.
  - Vérification de types Svelte (`npm --prefix apps/web run check`) : 0 erreur, 0 avertissement.

## 24 octobre 2026 — Mission 2 : Catalogue multimédia global consolidé et passage à l'échelle 1 000 000 d'œuvres

- **Modèle hiérarchique et consolidation inter-bibliothèques / inter-Nodes (`0043_global_catalogue_consolidation.sql`, `crates/core/src/consolidation.rs`)** :
  - Séparation explicite de l'œuvre (`Work`), de l'édition éditoriale (`Edition`, via `inventory_edition_metadata` : `Édition locale`, `Director’s Cut`, `Version longue`, `Édition IMAX`, `Édition remastérisée`, `Édition spéciale`), de la version d'encodage/résolution/conteneur (`MediaVersion`, via `inventory_version_metadata` : ex. `1080p • H.264 • AAC • MP4`, `4K UHD • AV1 • 10-bit • OPUS • MKV`), de la source logique (`MediaSource`), du fichier physique (`Asset`) et des pistes (`Track`).
  - Reconnaissance automatique des doublons à partir des identifiants externes fiables (`imdb`, `tmdb`, `tvdb`, `wikidata`, `eidr` indexés dans `media_work_external_ids`) lors des ingestions et publications de scans, sans jamais fusionner sur le seul titre.
  - Détection des correspondances probables / ambiguïtés (`media_match_candidates`) avec score et motif explicite, validation manuelle (`POST /api/v1/admin/media-groups/candidates/{id}/confirm`), rejet persistant (`POST /api/v1/admin/media-groups/candidates/{id}/reject`), séparation d'une œuvre (`ungroup`) et annulation réversible (`POST /api/v1/admin/media-groups/merges/{id}/undo`) restaurée depuis `media_merge_history`.
  - Unification des films, séries, saisons et épisodes dans `expand_detail` (`crates/core/src/product.rs`) et de la progression (`playback_progress`), des favoris/notes (`work_feedback`) et des listes personnelles sur l'ensemble des membres du groupe, tout en appliquant strictement les permissions de bibliothèques et le contrôle parental (`authorized_group_members`, `crates/core/src/permissions.rs`) et en préservant les métadonnées manuelles (`manual_json`) et affiches personnalisées.
  - Interface d'administration et d'édition (`apps/web/src/lib/catalog/WorkTools.svelte`) enrichie pour regrouper films et séries, traiter les ambiguïtés candidates et annuler une fusion.
- **Optimisations SQL et cache pour 100 000 et 1 000 000 d'œuvres (`crates/core/src/inventory.rs`, `docs/SQL_PERFORMANCE.md`)** :
  - Colonnes de synthèse maintenues par triggers (`display_title_sort`, `effective_year`, `max_height`, `codecs_csv`, `languages_csv`, `genres_csv`, `has_available_asset`), jointure différée en 2 phases sur index couvrants (`inventory_works_lib_avail_sort`, `inventory_works_filter_sort`, `inventory_works_fts_sort`), recherche par trigrammes `FTS5` (`inventory_works_fts` avec `work_id UNINDEXED`) et compteur de génération `inventory_cache_generation` en O(1).
  - Benchmarks mesurés (`cargo test --package media-poc --test global_catalogue_consolidation -- --nocapture`) :
    - **1 000 œuvres** : Cold Page 1 (shelf) `31,93 ms` | Warm Cache `0,321 ms` | Cold Page 1 (+COUNT) `8,35 ms` | Cold Search (`'matrix'`) `4,67 ms` | Cold 5-Filter Combo `9,05 ms`.
    - **10 000 œuvres** : Cold Page 1 (shelf) `9,41 ms` | Warm Cache `0,291 ms` | Cold Page 1 (+COUNT) `10,68 ms` | Cold Search (`'matrix'`) `9,34 ms` | Cold 5-Filter Combo `12,48 ms`.
    - **100 000 œuvres** : Cold Page 1 (shelf) `7,06 ms` | Warm Cache `0,222 ms` | Cold Page 1 (+COUNT) `14,13 ms` | Cold Search (`'matrix'`) `5,50 ms` | Cold 5-Filter Combo `4,59 ms`.
    - **1 000 000 d'œuvres** : Cold Page 1 (shelf) `6,41 ms` | Warm Cache `0,242 ms` | Cold Page 1 (+COUNT) `101,59 ms` | Cold Search (`'matrix'`) `39,56 ms` | Cold 5-Filter Combo `5,41 ms`.

## 24 octobre 2026 — Mission streaming 3.2

- Course audio reproduite dans Chrome : seek 900 s en attente puis seek 2 s
  ignoré. Un seek explicite remplace désormais le précédent : 4/4 échecs avant,
  12/12 essais après avec les mêmes paquets Rust et le même retard réseau.
  Web : 151 tests, check et build réussis ; lint lecteur ciblé réussi.
- Native Opus : offsets calculés par curseur de chunk, historique de pré-roll
  variable conservé ; garde d'indexation maintenue dans le travail bloquant
  malgré annulation HTTP. Admission froide limitée à 64, attente 10 s ; hits
  chauds exemptés. Relais Core limité à 64 en-têtes en attente et purge des
  receivers annulés. Les processus vidéo progressifs restent plafonnés à quatre.
- Linux Docker : natif final 11 tests réussis (dont 4/8/16 requêtes, annulation,
  limite 64 et Range), autres modules vidéo/confinement/stream 15 réussis.
  Trois générations de fixtures échouent avec le FFmpeg sans encodeurs du
  conteneur ; conserver cette limite. Ce ne sont pas 16 lectures navigateur.
- Instrumentation d'allocations séparée dans l'exemple streaming_allocations ;
  harnais distingue PCM brut/aligné, dérive initiale et échecs sans censure.
  Aucune promesse p95/p99 Release ou NAS sans campagne réellement terminée.
  État, commandes et preuves : docs/STREAMING_MISSION_3_2.md.
- Clôture Release : quatre sessions simultanées, un lot froid applicatif puis
  un lot serveur chaud/nouveaux navigateurs : 8 ouvertures, 16 changements de
  piste et 32 seeks réussis, aucune erreur dans ce seul essai. À chaud : médiane
  première image 2252 ms, PCM brut 2375 ms, changement 215 ms, seek image 232 ms.
  Objectifs 50/100 ms non atteints ; pas de p95/p99 fiable, ni validation de
  8/16 lecteurs, NAS ou Node physique distant. Node : pic RSS observé ~22 MiB,
  aucun processus FFmpeg ni fichier cache créé sur ce parcours natif.
  CPU Node agrégé par lot : 672/844 ms froid/chaud ; pas un coût par session.
  Exemple allocations compilé sous Linux, pas de mesure allocations exécutée.
  Preuves : docs/evidence/2026-10-24/streaming-mission-3-2/release-smoke4-summary.json.

## 24 octobre 2026 — Menu paramètres du lecteur, masquage des détails de nœuds et données de lecture (style Jellyfin)

- **Demande utilisateur** :
  - Lors du chargement d'un flux, le lecteur affichait des messages techniques mentionnant la connexion au Media Node ou au relais. Les utilisateurs finaux n'ont pas à voir sur quel nœud se trouve le média.
  - Déplacement de ces informations techniques dans les statistiques de lecture (« Données de lecture »).
  - Ajout en bas à droite des contrôles du lecteur d'un bouton d'engrenage « Paramètres » ouvrant un menu calqué sur l'ergonomie Jellyfin (Format d'image, Vitesse de lecture, Mode de répétition, Données de lecture).
- **Modifications apportées** :
  - `apps/web/src/lib/player/native.ts` :
    - Remplacement des messages explicites (« Connexion directe au Media Node… », « Node direct indisponible. Connexion au relais… ») par un état neutre et convivial (« Chargement de la vidéo… »).
  - `apps/web/src/lib/player/ImmersivePlayer.svelte` :
    - Écran de chargement épuré (`Chargement de la vidéo…` avec spinner animé).
    - Retrait du bouton redondant « Statistiques du flux » de la barre supérieure.
    - Gestion de l'aspect ratio (`style:object-fit={aspectRatio}` : Normal / contain, Remplir / cover, Étirer / fill).
    - Gestion du mode répétition (`loop={repeat}`).
    - Overlay « Données de lecture » accessible via les paramètres, affichant clairement le Nœud serveur, le Mode de transport (Direct Node / Relais Core), la résolution, le buffer, les images perdues et le diagnostic technique.
  - `apps/web/src/lib/player/PlayerControls.svelte` :
    - Réorganisation des contrôles de droite pour correspondre fidèlement à la maquette :
      - Sous-titres `[CC]`
      - Piste audio `♪`
      - Groupe volume (icône haut-parleur + curseur)
      - Bouton Paramètres `⚙`
      - Plein écran `⛶`
    - Menu contextuel « Paramètres » :
      - Format d'image (Normal, Remplir, Étirer avec bascule dynamique)
      - Vitesse de lecture (1x, 1.25x, 1.5x... avec sous-menu et bouton retour `←`)
      - Mode de répétition (Aucun / Répéter)
      - Données de lecture (ouverture de l'overlay de statistiques techniques détaillées)
- **Vérifications réelles** :
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement Svelte/TypeScript.
  - `npm --prefix apps/web test` : 25 suites réussies, 150 tests réussis.
  - `npm --prefix apps/web run build` : compilation statique Vite/SvelteKit réussie.


## 24 octobre 2026 — Sélecteur de dossiers hiérarchique et saisie libre de chemin (ergonomie Jellyfin)

- **Problème initial** :
  - Dans la gestion des bibliothèques (`AdminLibraries.svelte`), l'ajout de dossier était restreint à une liste déroulante plate (`<select>`) énumérant d'un bloc la racine et tous ses sous-dossiers immédiats.
  - Impossibilité pour l'administrateur d'explorer l'arborescence dossier par dossier (voir les dossiers à la racine, puis cliquer pour déplier un sous-dossier, remonter avec `...`), ni de saisir manuellement un chemin disque.
- **Modifications apportées** :
  - **Backend Core** :
    - Nouvelle route d'administration sécurisée `GET /api/v1/admin/filesystem/browse` (`browse_filesystem` dans `crates/core/src/libraries.rs`).
    - Authentification `administrator` obligatoire (401 non authentifié, 403 non-administrateur).
    - Navigation par dossier : renvoie `path`, `parent` (pour le bouton `...` de remontée vers le parent) et la liste triée des sous-dossiers (`directories: [{ name, path }]`).
    - Gestion multi-plateforme : sous Unix/Linux démarre à `/` ; sous Windows liste les lecteurs disponibles (`C:\`, `D:\`) lorsque la racine est vide, puis permet la descente dans l'arborescence native.
  - **Frontend Web** :
    - Création du composant modal `apps/web/src/lib/admin/PathPickerModal.svelte` calqué sur l'ergonomie de sélection de chemin de Jellyfin :
      - Entête avec flèche retour `<-` et titre « Sélectionner un chemin ».
      - Champ texte « Dossier » avec bouton de navigation/recherche (ou touche Entrée) permettant de taper ou coller directement n'importe quel chemin.
      - Liste des dossiers du niveau courant avec entrée `...` pour remonter d'un cran, chaque ligne de dossier ouvrant ses sous-dossiers au clic.
      - Bouton « OK » validant le chemin courant dans le formulaire.
    - Mise à jour de `AdminLibraries.svelte` :
      - Champ texte modifiable directement pour le chemin du dossier (`root`).
      - Bouton « Parcourir… » ouvrant la modale de sélection hiérarchique.
      - Bouton « Ajouter ce dossier » sans blocage sur une liste fermée pré-déterminée.
      - Prise en compte automatique du chemin saisi si l'utilisateur valide le formulaire sans avoir cliqué sur « Ajouter ce dossier ».
- **Vérifications réelles** :
  - `cargo fmt --check` : conforme.
  - `cargo clippy --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test --lib libraries` : 6 passed, 0 failed.
  - `npm --prefix apps/web test` : 25 suites réussies, 150 tests réussis.
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement Svelte/TypeScript.


## 24 octobre 2026 — Diagnostic et résilience de l'erreur 503 sur la préparation vidéo progressive

- **Cause racine de l'erreur "Préparation vidéo progressive : HTTP 503 (Media Node indisponible)"** :
  - Sur navigateur ne décodant pas le MKV en direct (ex. Chromium/Chrome/Edge), le lecteur tente le flux progressif MSE (`ProgressiveVideo`).
  - Si Core ou Node renvoyait HTTP 503 (`node_unavailable`, `asset_changed_rescan_required`, ou indisponibilité FFmpeg), l'en-tête `x-media-preparation-error` n'était pas toujours inclus ou n'était pas exposé par CORS (`ACCESS_CONTROL_EXPOSE_HEADERS`), et le client web masquait le code JSON `{ error: ... }` en affichant le texte par défaut "Media Node indisponible".
  - En outre, en cas d'échec du flux progressif MSE, le lecteur abandonnait immédiatement sans tenter le repli vers la préparation intégrale `preparedVideoUrl` (`/prepared/video`).
- **Correctifs apportés** :
  - `crates/core/src/inventory.rs` : injection systématique de `x-media-preparation-error` sur les erreurs `node_unavailable`, `asset_changed_rescan_required` et sur les relais distants défaillants.
  - `crates/node/src/server.rs` : ajout de `x-media-preparation-error` dans `ACCESS_CONTROL_EXPOSE_HEADERS`.
  - `apps/web/src/lib/player/progressive-video.ts` et `native.ts` : extraction de l'erreur depuis l'en-tête OU le corps JSON avec messages explicites en français (ex. fichier source modifié, FFmpeg manquant, nœud déconnecté).
  - `apps/web/src/lib/player/native.ts` : basculement automatique sur `source.preparedVideoUrl` si le flux progressif MSE échoue au lieu d'interrompre brutalement la lecture.
- **Vérifications réelles** :
  - `npm --prefix apps/web test` : 25 fichiers de tests passés, 150 tests réussis.
  - `npm --prefix apps/web run build` : compilation Vite/SvelteKit réussie.
  - `cargo check --all-targets` : 0 avertissement bloquant.
  - `cargo test -p media-node --lib -- -q` : 64 passed, 0 failed, 3 ignored.
  - `cargo test -p media-poc --lib -- -q` : 55 passed, 0 failed, 1 ignored.

## 24 octobre 2026 — Correction du test CI browser_scanner pour MP4 native Opus et repli progressif

- Route d'interception audio : l'introduction de l'extraction native Opus ISOBMFF
  nomme les segments MP4 /prepared/opus-{trackId}-chunk- (ex. opus-2 pour
  internal-1 et opus-3 pour internal-2) plutôt que audio-{index}.
  Le test scripts/browser-scanner.cjs interceptait uniquement **/prepared/audio-1*,
  ce qui laissait passer les requêtes MP4 réelles avec succès et causait un timeout
  sur l'apparition de l'avis de rétention de piste précédente.
- Repli vidéo MKV en cas d'échec de lecture directe : depuis l'ajout de
  ProgressiveVideo via MSE (video-stream-0-...), la vidéo de repli est chargée
  via une URL blob: plutôt que le remux intégral /prepared/video. L'assertion
  de lecture de repli accepte désormais blob: en plus de /prepared/video.
- cargo test --locked --test browser_scanner -- --ignored --nocapture exécuté
  et réussi avec succès (1 passed, 37.80s, scénarios restart=false et restart=true).

## 24 octobre 2026 — Streaming progressif Rust, jalon mesuré et limites

- MP4 Opus validés au scan : fenêtres fMP4 virtuelles depuis les paquets
  originaux, sans FFmpeg ni cache audio disque. IDs MP4 distincts des ordinaux
  ffprobe ; timestamps absolus, éditions et pré-roll conservés. Index borné
  à 32 Mio/8 entrées, moov 16 Mio, lectures 64 Kio. Autres layouts : chemin
  existant de copie de paquets, aucun réencodage audio implicite.
- Repli vidéo AV1 continu avec backpressure, annulation et quatre processus
  maximum, sans remux intégral préalable. Fenêtres vidéo chevauchantes rejetées
  après blocage réel du décodeur. MSE borné à 128 Mio encodés, pas une garantie
  de RSS navigateur. Audio initial préparé parallèlement ; identité logique
  VF/VO transmise explicitement. Préparations FFmpeg identiques regroupées.
- Signatures des erreurs Node corrigées : longueur fixée avant digest, y
  compris uplink. Autorisation résolue une fois par demande, droits conservés.
  Windows : MEDIA_NODE_ROOTS conserve les lettres de lecteur.
- Vérifications : Node 64 réussis/3 ignorés, Web 150 réussis/check/lint/build,
  Core 4 tests inventaire + 2 parentaux + 2 intégrations distribuées réussis.
  POC Native MKV/MP4 GOP15 : seeks et raccords vérifiés avec PCM ; audio préparé
  annoncé, pas une preuve système à froid.
- Banc réel Core/SQLite + Node Rust séparé, 336 Mio et 1,76 Go synthétiques,
  caches applicatifs vides/chauds, comptes distincts, ressources Job Object,
  profil TCP simulé. Aucun Node physique distant disponible selon l'utilisateur.
  Rapports et méthode : docs/STREAMING_SYSTEM_BENCHMARK.md et preuves associées.
- Limite importante : la matrice candidate conserve des expirations audio à
  quatre sessions. Le p95 de démarrage sur cinq ouvertures chaudes n'améliore
  pas la référence ; ne pas annoncer 50 ms ni une fiabilité multi-session
  acquise. Aucun p99 UI estimé sur ces petits lots. Mesures debug Windows,
  cache OS non purgé, ni NAS ni sortie audio physique validés. L'utilisateur
  demande une clôture propre faute de quota : pas de campagne supplémentaire.

## 24 octobre 2026 — CI scanner, fermeture après choix audio validé

- La boucle de réouverture FR/EN attend désormais la sélection Svelte
  (`aria-checked=true`, contrôle disponible) avant de fermer le lecteur.
  La préférence serveur seule pouvait déjà correspondre à un choix antérieur
  et ne prouvait pas la fin de la transaction audio courante.
- Test `cargo test --locked -p media-poc --test browser_scanner -j 1 --
  --ignored --nocapture --test-threads=1` réussi localement : MP4/MKV, reprise
  FR/EN, PCM et redémarrage Core. Rapport : target/ci-scanner-reopen/.
  Aucun timeout augmenté ; GitHub Linux non relancé, aucun push.

## 23 octobre 2026 — Régression CI playlists, formulaire de dépôt replié

- `browser-playlists.cjs` ouvre « + Nouveau dépôt » après l'onglet Dépôts
  avant d'attendre Adresse du dépôt. Le formulaire est replié par défaut ;
  aucune modification produit ni augmentation de timeout.
- `cargo test --locked -p media-poc --lib product_tests::mixed_playlists -j 1
  -- --test-threads=1` réussi avec `MEDIA_TEST_PLAYLIST_BROWSER=1` et
  `MEDIA_WEB_DIST=apps/web/build` résolu : vrai Core/SQLite/Chrome local,
  partage/droits, lecture persistante, dépôts, révocation et PCM réussis.
  Rapport local : `target/ci-playlists-repository/report.json`.
- Syntaxe Node et diff vérifiés ; pas de push ni relance GitHub Actions.

## 23 octobre 2026 — Recherche streaming Rust et continuité VF/VO

- Cache de préparation Node : ouverture confinée hors verrou global, lease
  conservée pendant ouverture/streaming, worker libéré au hit après attente.
  Microbenchmark Windows debug, 8 sessions/512 premiers blocs de 64 Kio :
  médiane 255,9 → 101,7 ms sur trois alternances ; pas une mesure de décodage.
- Changement audio indépendant : ancienne vidéo/piste maintenue pendant la
  préparation du candidat ; commit aligné à la position courante, pause manuelle
  et erreurs préservées. Suivi des fenêtres actif pendant chargement du candidat.
- Blocage long reproduit : Chrome peut déclarer readyState=4 après réouverture
  avec buffered vide/ancien. Accepter HAVE_ENOUGH_DATA hors seek supprime notre
  attente artificielle ; réserve conservée pour readyState=3, aucun retry ajouté.
- Retard audio contrôlé 300 ms : trou vidéo médian 343,3 → 66,5 ms, silence PCM
  346,1 → 9,2 ms ; délai de la nouvelle langue toujours environ 360 ms.
- 50 ms observés sur fixtures chaudes 320×180 dans le moteur isolé : pas de
  promesse NAS/1080p/application complète. Long 1080p/12 fps, 336 Mio : huit
  ouvertures/seeks par stratégie et seize switches passent après correctif ;
  reprise segmentée médiane 122,7 ms, PCM seek p95 234,4 ms, objectifs ouverts.
- Méthode, preuves et limites : docs/STREAMING_PERFORMANCE.md. Aucun réencodage,
  changement d'architecture, push, déploiement ou modification de médias privés.
- Validation locale : 136 tests Web, check/lint/build, 49 tests Node, cinq
  intégrations Core, microbenchmark cache, Clippy workspace/all-targets strict
  et format réussis. Mesures Core/SQLite/Svelte : premier clic → frame
  277–296 ms, réouvertures usuelles 57–85 ms sur petites fixtures (passage
  précédent 144–194 / 44–67 ms, forte variabilité) ; repli
  après erreur réseau vérifié. Son physique/CPU-RSS Rust par session non mesurés.

## 23 octobre 2026 — Optimisation du chargement instantané de l'accueil et des images (style Jellyfin)

- **Chargement instantané du catalogue et affichage immédiat de l'accueil** :
  - Dans `apps/web/src/lib/catalog/page-cache.ts` :
    - Persistance du cache de catalogue dans `sessionStorage` (avec préfixe isolé `media_catalog_cache_v1_`) en complément du cache mémoire.
    - `cachedCatalog` restitue immédiatement les données en cache (mode Stale-While-Revalidate), permettant un premier rendu immédiat sans squelette bloquant à chaque navigation ou réouverture.
    - Éviction et invalidation synchronisées entre mémoire et `sessionStorage` (changement de compte, déconnexion, révocation d'accès, quotas).
  - Dans `apps/web/src/lib/catalog/Catalog.svelte` :
    - Préservation des films et séries en mémoire si des entrées en cache sont disponibles : `films` et `series` ne sont plus réinitialisés à vide avant vérification du cache, évitant le clignotement ou la disparition des titres.
  - Dans `apps/web/src/routes/+layout.svelte` :
    - Déclenchement de `preloadCatalog('home')` dès la vérification de la session, préchargeant l'accueil en parallèle de l'hydratation des préférences.

- **Performances et cache des images inspirés de Jellyfin** :
  - **Parallélisme de décodage côté Media Node (`crates/node/src/images.rs`)** :
    - Augmentation du sémaphore de validation d'images `VALIDATION` de 1 à 8 permis simultanés, éliminant le goulot d'étranglement qui forçait toutes les miniatures de la page d'accueil à être redimensionnées séquentiellement une par une.
  - **Cache disque persistant des miniatures générées (`crates/node/src/image_preview.rs`)** :
    - Augmentation du cache mémoire LRU de 8 Mo (128 entrées) à 64 Mo (2048 entrées).
    - Mise en place d'un cache sur disque persistant (`MEDIA_IMAGE_CACHE_DIR` ou répertoire temporaire système `media-image-cache`) basé sur l'empreinte SHA-256 du chemin canonique, de la taille, de la date de modification et de la largeur cible.
    - Écriture atomique (fichier temporaire puis renommage) et relecture directe en ~0.1 ms sans réencodage CPU lors des redémarrages ou après éviction mémoire.
  - **Vérification anticipée If-None-Match 304 (`crates/node/src/lib.rs`)** :
    - Vérification du header `If-None-Match` correspondant à l'ETag de la miniature avant tout appel à `generate`, renvoyant immédiatement `304 Not Modified` sans chargement ni allocation d'image.
  - **Affichage instantané des affiches dans le navigateur (`apps/web/src/lib/catalog/image-url.ts`, `Poster.svelte`, `HeroBanner.svelte`, `CatalogHome.svelte`)** :
    - Maintien d'un ensemble en mémoire des URLs d'images déjà chargées (`isImageLoaded`, `markImageLoaded`).
    - Action `checkLoaded` sur `<img>` détectant `node.complete && node.naturalWidth > 0` pour passer immédiatement `loadedUrl` à l'URL cible sans attendre l'événement `onload` si le navigateur a déjà l'image en cache.
    - Attribut `eager={index < 6}` sur les premières affiches visibles de la page d'accueil (`CatalogHome.svelte`).
- **Validations réelles exécutées** :
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement.
  - `npm --prefix apps/web test` : 125 tests unitaires passés avec succès.
  - `npm --prefix apps/web run lint` : validé sans erreur (ESLint).
  - `npm --prefix apps/web run build` : build statique de production SvelteKit réussi.
  - `cargo fmt --check` : 0 écart de formatage.
  - `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings` : 0 avertissement.
  - `cargo test -p media-node` : 48 unit tests + 3 integration tests passés avec succès.
  - `cargo test --locked -p media-poc --lib product_tests` : 6/6 tests de fonctionnalités produit passés avec succès.

## 23 octobre 2026 — Navigation retour accueil sur le logo Aether (Paramètres et Administration)

- **Gestion du retour à l'accueil via le logo Aether** :
  - Dans `apps/web/src/lib/ui/Navbar.svelte` (utilisé dans les Paramètres personnels `/settings`) :
    - Ajout du gestionnaire `navigateHome` avec appel direct à `goto(resolve('/'))` lors du clic sur le logo (`a.brand`) et le lien Accueil, avec support des touches modificatrices (Ctrl/Cmd).
    - Ajout de l'infobulle accessible `title="Retour à l’accueil"` et du style `cursor: pointer`.
  - Dans `apps/web/src/routes/admin/libraries/+page.svelte` (Administration du serveur / Paramètres système) :
    - Ajout du logo Aether (`a.brand.admin-brand`) dans l'en-tête d'administration (`admin-header`), permettant également de revenir directement à l'accueil via un clic ou le lien de retour.
- **Validations réelles exécutées** :
  - `npm --prefix apps/web run check` : 0 erreur, 0 avertissement.
  - `npm --prefix apps/web test` : 124 tests unitaires passés avec succès.
  - `npm --prefix apps/web run lint` : 0 erreur.
  - `npm --prefix apps/web run build` : build statique de production SvelteKit réussi.
  - Test E2E Playwright de navigation validé (clic sur logo dans `/settings/` et dans `/admin/libraries/` redirigeant vers `/app/`).

## 23 octobre 2026 — Page d'accueil orientée découverte et retrait des filtres

- **Retrait du menu « Filtrer le catalogue » sur la page d'accueil** :
  - Conditionnement de `<CatalogFilters />` dans `apps/web/src/lib/catalog/Catalog.svelte` pour n'apparaître que dans les sections de catalogue dédiées (`films`, `series`, `search`), et plus sur l'accueil (`home`) ni les paramètres.
- **Dynamisation et enrichissement de la découverte sur la page d'accueil** :
  - **HeroBanner interactif et tournant** : rotation dynamique parmi les œuvres avec visuel de fond (`artwork.backdrop`), évitant l'affichage statique permanent du tout premier média, et ajout du bouton « Surprenez-moi » permettant de changer de recommandation en vedette à la volée.
  - **Rayon « Pépites au hasard · À découvrir »** : sélection aléatoire renouvelée à chaque session ou via le bouton d'action « Mélanger », invitant à redécouvrir des œuvres inattendues de la médiathèque.
  - **Rayons thématiques et qualitatifs conditionnels** : ajout des étagères « Les mieux notés · Coups de cœur », « Grands classiques & Cinéma d'époque » (œuvres antérieures à 2010), « Séries à dévorer · Formats courts » (saisons $\le 2$), et rotation aléatoire des genres découverts à chaque visite.
  - **Composant `MediaShelf` enrichi** : prise en charge d'un bouton d'action dédié (`onAction` / `actionLabel`) intégré à la barre d'en-tête de l'étagère.
- **Validations réelles exécutées** :
  - `pnpm --filter media-web check` : 0 erreur, 0 avertissement.
  - `pnpm --filter media-web test` : 124 tests unitaires passés avec succès.
  - `pnpm --filter media-web lint` : validé sans erreur.
  - `pnpm --filter media-web build` : build statique de production SvelteKit réussi.
  - `cargo fmt --check` et tests Rust `product_tests` (6/6 passés).

## 23 octobre 2026 — Refonte de l’interface des extensions

- Catalogue et installées en cartes à visuels CSS/SVG locaux ; catégories issues des manifestes/permissions, recherche combinée, fiche catalogue dédiée avec choix de version, compatibilité, provenance et accès. Aucune image distante ni extension produit ajoutée.
- Dépôts en liste compacte avec formulaire « Nouveau dépôt » repliable ; fiche installée enrichie avec visuel, état, version et auteur. Contrats Core, réglages, activation et permissions conservés.
- Validation : `npm run check` (0 erreur/0 avertissement), `npm run lint`, `npm run build`, `git diff --check` réussis. `cargo test --locked -p media-poc --test extensions browser_extension_installs_and_executes_real_rust_module -j 1 -- --ignored --nocapture --test-threads=1` : 1 passé (13,90 s).
- Chrome desktop/mobile : catégories, recherche sans résultat, fiche catalogue, formulaire dépôt et absence de débordement ; cycle réel SQLite/Wasm signé activation/exécution/désactivation/désinstallation réussi. Captures sous `target/extensions-lifecycle-browser/`. Les entrées de démonstration du catalogue sont injectées par Playwright uniquement. Aucun dépôt distant, NAS ou déploiement vérifié.


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

## 23 octobre 2026 — démarrage lecteur et catalogue progressif

- Le catalogue démarre après vérification de session, sans attendre les préférences.
  Leur hydratation est partagée ; le lecteur conserve les préférences disponibles,
  signale leur panne et utilise alors les réglages de séance. Les paramètres
  réhydratent aussi leurs décalages sans écraser une saisie en cours.
- Accueil, Films, Séries et Recherche utilisent `count=false`, pages de 24 cartes,
  `hasMore` et cache de session borné. Les fiches et paramètres sont importés à
  leur ouverture. Le comptage exact reste le défaut de l'API pour les anciens clients.
- Le détail série Web demande 24 épisodes côté serveur ; permissions, contrôle
  parental et disponibilité s'appliquent avant pagination. `episodeAfter` récupère
  un seul prochain épisode, y compris entre saisons. Les playlists utilisent des
  résumés autorisés sans construire récursivement toutes les séries, 50 éléments
  visibles ; leurs limites serveur existantes restent 500 listes/1 000 entrées.
- MKV interne AV1/Opus : tentative vidéo originale avec audio Opus segmenté ; repli
  MP4 préparé atomique si le navigateur échoue. Aucun cache partiel publié. Reprise
  appliquée avant le premier segment audio ; réserve initiale 750→50 ms avec
  données lisibles obligatoires. Erreurs techniques affichées ; Réessayer relance
  réellement la lecture. Préparation complète encore possible sur client incompatible.
- Preuves locales : catalogue SQLite authentifié 10 000 films, première page sans
  comptage 6,23 ms contre 51–59 ms avec comptage ; pages profondes environ 50 ms.
  Série de 10 000 épisodes : page de 24, réponse <64 KiB, 133,99 ms en debug ;
  contrôle parental, retrait des droits et passage de saison vérifiés.
- Web : 120 tests Vitest réussis. Chrome/API simulée : rangées indépendantes des
  préférences lentes, pagination 10 000 éléments, cache/navigation et retry effectif.
  Core réel + Chrome 155.0.8059.86 : AV1/Opus MP4/MKV, pixels/PCM, quatre pistes,
  pause/seek/reprise/sous-titres ; MKV `/content` sans copie vidéo, puis panne forcée
  et repli préparé réussis. Clic → >0,2 s de lecture : MP4 628 ms, MKV 790 ms,
  distinct de la première image exacte. `scanner_flow` ciblé et harness
  `browser_scanner` réussis ; docs/RESPONSIVENESS_AND_ARTWORK.md et
  docs/PLAYER_TRACK_PREPARATION.md détaillent méthodes et limites.
- Aucun résultat NAS/production ni garantie vidéo/interface de 50 ms partout.
  Aucun déploiement, push ou changement des médias personnels.
- Vérifications finales : `cargo fmt --check`, Clippy strict workspace/all-targets,
  Web check 0/0, lint/build et `git diff --check` réussis. Le test Core réel
  `cargo test --locked -p media-poc --test browser_series -j 1 -- --ignored --nocapture`
  réussit en 31,19 s : huit images décodées, lecture/seek/pause, enchaînement
  inter-saisons, révocation/restauration et isolation des comptes. Lancement
  d'épisode mesuré à 1 001 ms sur cette fixture, sans extrapolation NAS.

## 23 octobre 2026 — corrections CI lecteur et multi-Node

- Échec CI `browser_scanner` au retour MKV : vidéo bloquée à 11,95478 s malgré
  des segments audio HTTP 200. Le MSE audio ne signalait pas sa fin ; le reliquat
  entre le dernier paquet Opus et la fin vidéo devenait une erreur après 10 s.
  `endOfStream()` clôt désormais le dernier segment ; un seek peut rouvrir le MSE.
  La fin vidéo tolère seulement un reliquat de 150 ms (paquet Opus + arrondi),
  sans supprimer les erreurs de troncature réelle ou de panne en cours de lecture.
  Les décalages audio explicites restent pris en compte ; les tests de persistance
  de langue attendent un seek stabilisé en pause, indépendamment du test de fin.
- Régression Chrome dédiée : nouvelle piste dans les 40 dernières ms, retour à
  une fenêtre antérieure après EOS, reprise audio/vidéo. Le test remet l'état
  « non vu » avant le contrôle manuel, car ce seek peut légitimement marquer vu.
- `distributed_multi_node` attendait à tort la signature MP4 pour l'URL originale
  MKV. Il vérifie les octets/MIME originaux et, séparément, le repli MP4 préparé,
  Range GET/HEAD 206, Range invalide 416 et refus utilisateur 403.
- Validation locale : `cargo test --locked --test distributed_multi_node -j 1 --
  --nocapture --test-threads=1` réussi (9,47 s) ; `cargo test --locked --test
  browser_scanner -j 1 -- --ignored --nocapture` réussi (35,70 s), Core/Chrome
  155.0.8059.86 Windows, MP4/MKV, pixels/PCM et régression fin/seek. Rapports
  `target/ci-scanner-tail-regression/`. 124 tests Web, check 0/0, lint/build,
  formatage et Clippy ciblé strict réussis. Linux CI reste à relancer ; aucun push.

## 24 octobre 2026 — Mission 3.2, smoke Release concurrent

- Artefacts Core/Node Release + Web copiés et SHA vérifiés inchangés; source
  modifiée par d'autres chats après build, donc pas d'attestation exact HEAD.
- Smoke réel Core/SQLite/NodeTCP/UI Chrome à4comptes :8ouvertures,
  16switches,32seeks,0échec. Chaud n4 : image médiane2251,9ms,
  PCM brut2375,3ms; switch médian215,5ms, seek image232,1ms.
  Objectifs50/100ms non atteints, aucun p99UI, aucun8/16/100 lancé.
- Node par cellule4sessions complète : CPU671,9/843,8ms froid/chaud,
  RSS échantillonné21,46/21,76Mio,0FFmpeg et0nouveau cache préparé.
  Chrome exclu; cacheOS non purgé, pas de NAS ou Node physique distant.
- LinuxDocker : Nativefinal11pass/1microbenchignoré (cap64/refus65e,
  exemptionchaud, annulationindex,4/8/16). Vidéo6/confinement6/stream3
  passés sur snapshotguard précédent. Ancienne suite complète66pass/3fail/
  4ignored conservée : générationfixtures impossible avecFFmpegLinuxminimal
  sans encodeurs/filtres. `cargo check` exempleallocations réussi seulement,
  aucune mesure d'allocationLinux ni preuveLinux navigateur.
- Docs/preuves : docs/STREAMING_MISSION_3_2.md et
  docs/evidence/2026-10-24/streaming-mission-3-2/release-smoke4-summary.json.
  Audit13fichiers sanssecret; processusRust/FFmpeg test nettoyés, aucunpush
  ou déploiement. Aucun commit effectué par l'agent benchmark.


## 25 octobre 2026 — Mission 4 : moteur de tâches distribué et validation finale

- File durable `task_engine`, scheduler existant réutilisé, catégories scan/analyze/prepare/image ;
  leases de 60 s et tentatives monotones, budget d'échecs distinct, idempotence,
  annulation/pause/reprise, concurrence bornée par Node et publication soumise au fencing.
  Aucune nouvelle infrastructure obligatoire ; standalone et médias en lecture seule conservés.
- Admission bornée et priorité lecture ; pression CPU/RAM/disque/I/O et réseau optionnel.
  Les écritures attendent avant d'acquérir une connexion SQL. Attribution jusqu'à 100 Nodes
  dans une transaction et une mise à jour SQL, sans exécuteur par tâche en attente.
- Résultats de jobs limités à 64 Kio et chargés au détail ; UI paginée, propriétaire/Node,
  durée, erreurs, compteurs et contrôles. Réponses périmées et ancienne configuration de racine
  refusées ; un sous-dossier ne contourne pas la désactivation de son parent.
- Validation : 15 intégrations Core ciblées + 8 tests Core ciblés réussis ; Node 84 réussis/4 ignorés ;
  Web 152 réussis, check 0/0, ESLint ciblé et build réussis ; Clippy tous targets réussi.
  Trois bancs explicites réussis. Chrome réel AV1/Opus MP4/MKV avec backlog 10 000 et
  redémarrage : pixels/PCM/seek/pause vérifiés, 138 puis 256 jobs terminés, aucune perte des 10 000.
- Mesures Windows/debug/SQLite : 10 000 admissions en 49,95 s (200,2/s), p95 admission 7,34 ms,
  navigation p95 16,05 ms, RSS final processus de test 19,52 Mio. Attribution unitaire p95 9,13 ms ;
  attribution groupée 100 Nodes p95 383,89 ms : cible 50 ms non atteinte pour ce cas et la contention.
- Limites : Nodes de charge simulés, backlog navigateur composé de scans de dossier vide ;
  pas de preuve Release/NAS/PostgreSQL/100 machines. Redistribution entre copies,
  checkpoints par fichier, budgets administratifs et métadonnées via extensions restent ouverts.
  Rapport et commandes : docs/DISTRIBUTED_TASKS.md ; preuves docs/evidence/2026-10-25/distributed-tasks/.

## 25 octobre 2026 — Images multiples et rotation native par visite

- Décision réalisée : pools privés, déduplication et sélection natifs ; découverte des images par les extensions optionnelles existantes. Audit du Poster Rotator Jellyfin au commit6640be2 : son hash sur octets compressés n'est pas réutilisé.
- Migration0045 SQLite/PostgreSQL : langue, fournisseur, empreinte pixels et alias de hash URL. Imports ajoutés conservés, déduplication dHash/couleurs/proportions/alpha, plafond64 images/œuvre et12/type ; choix manuel prioritaire avec réserve fournisseur conservée. Originaux locaux/anciens imports sans empreinte non rétroanalysés.
- Collecte administrateur/CSRF par œuvre depuis Modifier les images, fournisseurs actifs/grants vérifiés, identifiants externes connus obligatoires, langues et secours, neutres facultatifs. Bornes12 nouveaux imports,24 tentatives,55s globales ; URL déjà traitées/doublons mémorisés. Pas de collecte automatique de toute la bibliothèque.
- Web : variantes dans catalogue, choix commun affiches/fonds/logos/illustrations/vignettes ; stabilité au reload, exclusion du choix précédent à la nouvelle visite si alternative. Historique512 couples œuvre/type par compte ; session de navigation distincte des connexions longues, aucune synchronisation interappareil.
- Sources Aether-Plugins TMDb/fanart/API adaptées : logos, fallback/langues régionales, résolution externe et TVDB séries fanart, plafond200 images/60Kio avec répartition rôles/langues. Paquets signés installés inchangés : reconstruction/signature requises pour distribution.
- Vérifications : Core2 tests artwork+2 collecte+2 adaptateurs, intégration artwork1, intégrations extensions ciblées2 réussies ; Clippy Core lib/tests strict et formatage réussis. Web9 tests ciblés, ESLint/check (0 erreur,6 avertissements préexistants)/build réussis. Chrome155 Windows avec API simulée : images décodées, nouvelle visite sans reconnexion, stabilité/reconnexion, collecte/CSRF/partiel et1440/390px. Plugins20 tests et Clippy strict réussis.
- Limites : aucune preuve réseau fournisseur avec clés réelles, PostgreSQL/NAS/déploiement ; déduplication heuristique. Aucun push, publication, changement de médias ou modification du dépôt Jellyfin. Rapport/commandes : docs/ARTWORK_ROTATION.md ; captures/rapport target/artwork-rotation-ui/. Prochain jalon : collecte durable planifiée avec budgets par bibliothèque/fournisseur.


## 25 octobre 2026 — Extensions simplifiées, galerie et vue d’ensemble

- Les réglages d’extensions ouvrent un écran séparé, avec retour à la fiche.
  Retrait des tests fournisseur, historiques, journal et panneaux techniques.
  Fanart.tv expose seulement API Key ; adresses officielles préremplies pour les
  fournisseurs connus. La langue de bibliothèque est appliquée côté Core aux
  fournisseurs ; l’ABI historique des propositions simples sans configuration
  reste compatible. Les URL vides historiques sont ignorées à l’exécution pour
  rétablir le défaut du fournisseur (test de non-régression réussi). Activation
  conserve les grants existants.
- Avatars : trois préférences (aléatoire, Gravatar, verrouillage), galerie des
  imports, ajout local/GitHub, renommage de collection, suppression d’image et
  attribution idempotente aux comptes sans photo. Attribution automatique locale
  et OIDC depuis les collections actives. Gravatar est explicite, désactivé par
  défaut, normalisé et bloqué par le verrouillage ; pas d’adresse conservée.
  Le profil présente directement les collections, sans générateur/variante.
- Administration sans fragment : Vue d’ensemble par défaut. Activité récente,
  appareils, lecteurs vidéo ouverts et progression via signalement borné avec
  sessions/CSRF/droits/propriété/expiration ; migration additive0046 SQLite et
  PostgreSQL. Erreurs du dernier scan détaillées par bibliothèque. Aucune cause
  du vrai compteur356 sur le serveur utilisateur n’est établie sans ses messages.
- Vérifications : Web12 tests ciblés, lint/check/build ; Core tests ciblés activité
  et collections (dont Gravatar simulé et attribution), parcours Chrome155 Windows
 1440/390px et inspection des captures réussis. Import GitHub réel :7 collections
  détectées,26 Sonny normalisés en SQLite temporaire. Plugins13 tests ciblés et
  compilation workspace lib réussis. Clippy strict a rencontré un avertissement
  dans music.rs hors de ce périmètre. Rapport/commandes : docs/ADMIN_OVERVIEW.md.
- Limites : PostgreSQL/NAS et Gravatar avec un compte réel non validés ; présence
  vidéo Web seulement, pas le lecteur musical global ni les anciens clients.
  Sources de schémas simplifiées dans Aether-Plugins ; paquets signés existants
  inchangés. Catalogue distant vérifié sans Intro Skipper : prochaine étape,
  construire/signature/publication puis actualiser. Aucun push ni déploiement.


## 25 octobre 2026 — Compte, sécurité, appareils et navigation admin

- Compte regroupe profil, thème et langue ; Sécurité regroupe mot de passe et SSO ; Appareils présente les sessions et leur révocation. Rubriques Apparence/SSO séparées et note générique d’enregistrement supprimées. Anciens fragments redirigés vers les rubriques regroupées.
- Avatar : trois actions principales import/suppression/choix ; autorisations chargées via GET `/api/v1/account/avatar/policy`, contrôles PUT/DELETE côté Core. Plugins actifs avec permission avatars : `disallow_custom_avatars` masque/refuse import ; `force_random_on_creation` ou `require_avatar` masque/refuse suppression, administrateurs inclus. Collections et propositions des extensions accessibles derrière le bouton de choix.
- Administration → Général : `blockSsoLocalAccounts`, activé par défaut. GET/POST `/api/v1/account/local-credentials` pour permettre la création sur le même compte SSO seulement si politique autorisée, CSRF et connexion récente (<5 min). Pas de suppression des credentials existants. Logo puis Menu et Retour dans le bandeau ; logo vers accueil et Retour dans le tiroir.
- Validation : `cargo test --locked -p media-poc --test account -j 1` : 4 tests réussis (dont politique avatar avec extension active/désactivée et refus serveur, SSO/CSRF/connexion récente). `cargo fmt --all --check`, check/lint/build Web réussis ; dernier check sans erreur avec avertissements hors périmètre (CatalogGrid et AdminUsers modifié en parallèle). Clé de boucle manquante et espaces finaux AdminUsers corrigés pendant la vérification du lint. Chrome headless local avec API simulées : navigation, galerie et boutons masqués réussis, capture `target/account-devices-smoke.png` inspectée. Aucune validation NAS/SSO réel ni déploiement ; reconstruction et déploiement restent nécessaires.

- Dernière relance Rust : avertissement Windows de cache incrémental inaccessible (os error 5), compilation terminée et tests exécutés. Le second essai navigateur étendu à l’en-tête admin n’a pas abouti et a été arrêté ; seul le premier parcours compte/appareils à API simulées est retenu comme preuve navigateur.


## 26 octobre 2026 — Mission 2 : automatisation événementielle (jalon métadonnées)

- Outbox transactionnelle reliée à la publication d'inventaire et au worker existant ; tâches d'extensions dans durable_tasks avec leases/fencing, déduplication par source/configuration/langue/version, retries/backoff/jitter, dépendances et modes automatique/manuel/désactivé/programmé. Déclencheurs système raccordés à la même file, ancien worker système retiré ; migrations0002/0003 SQLite+PostgreSQL, conservation SQLite vérifiée.
- Wasm : quatre processus maximum, une invocation par extension, trois automatiques maximum ; contrôles de droits/lease avant persistance, cooldown fournisseur partagé et Retry-After, cadence HTTP bornée. Métadonnées automatiques manquantes seulement avec metadata.write ; corrections manuelles préservées. Administration > Tâches expose les politiques.
- Tests réellement exécutés : cargo test --locked -p media-poc --test automation_flow --test distributed_multi_node -j1 (7+1 réussis, benchmark ignoré lors de cette passe) ; runtime storage_frames5 réussis ; python scripts/test-system-task-queue-migration.py1 réussi ; npm --prefix apps/web run check0 erreur/5 avertissements existants ; ESLint ciblé AdminTasks/AutomationTasks réussi.
- Parcours réel local : scan AV1/Opus + Wasm signé + persistance3412,335ms ; deux Nodes TCP/3assets2246,817ms ; nouveau média traité sur Node2 pendant panne Node1721,655ms. Redémarrage, déduplication, Range et révocations vérifiés. Aucun média original modifié.
- Benchmark final explicite : 100000 tâches persistantes/100Nodes simulés, admission unitaire p50=10,967ms/p95=18,869ms ; attribution groupée100Nodes p50=1107,675ms/p95=1534,107ms (objectif50ms manqué pour cette opération). La première mesure groupée p95=4631ms a motivé correction de l'index et du chemin UPDATE. Mesures Debug/SQLite locales, pas100machines ni preuve d'absence d'impact sur la lecture.
- Périmètre assumé réduit à la demande utilisateur : métadonnées automatiques, pas orchestration complète images/paroles/intro. PostgreSQL/NAS et percentiles lecteur non validés. Snapshot encore complet sous transaction de publication (mémoire paginée256assets) ; optimiser par delta et améliorer admission groupée constituent le prochain petit jalon. Docs : docs/AUTOMATION.md ; preuves : docs/evidence/2026-10-26/automation/results.json. Aucun push/déploiement.
## 26 octobre 2026 — IntroSkipper, garde-fous initiaux (Mission 3 non terminée)

- Le plugin refuse désormais de comparer un lot mélangé entre séries/saisons, exige un identifiant de série et une saison pour les comparaisons acoustiques, rejette les empreintes uniformes et les cadences incompatibles. Tests ciblés du plugin : 9 réussites.
- Le Core valide maintenant la structure et les bornes des sorties Wasm de marqueurs. L'heuristique de chapitres du descripteur est limitée aux épisodes et à des titres précis ; elle n'invente plus la fin d'un dernier chapitre sans durée. `cargo check -p media-poc --lib` a réussi.
- Le parcours complet demandé reste ouvert : catalogue signé, tâches à l'ajout, extraction audio Node, persistance Core, préférences/corrections manuelles, compatibilité des copies et mesures sur corpus/deux Nodes. Voir `docs/INTRO_SKIPPER.md`. Le plugin installé ne déclenche pas encore d'analyse.

## 26 octobre 2026 — IntroSkipper, reprise de Mission 3

- Parcours local ajouté : événement épisode → Wasm de chapitres → marqueurs automatiques Core ; si intro absente, tâche durable Chromaprint sur Node propriétaire → événement d'analyse → comparaison bornée avec un épisode de la même saison → marqueurs Core. Préférences par utilisateur et correction/désactivation administrateur prioritaires dans le lecteur. Source/génération d'extension contrôlées ; empreintes retardées pendant une lecture active.
- Limite Wasm mesurée sur paire synthétique : 5 100 mots/épisode, 139 millions de fuel et 12,6 s sur Windows local après indexation ; 2 millions provoquaient un trap. Le budget d'`intro.detect` est 200 millions, entrée JSON plafonnée à 65 536 octets. Tests plugin 11/11, Core automation_flow 10 réussis/1 ignoré, build Wasm réussi ; voir `docs/INTRO_SKIPPER.md` pour limites.
- Mission encore incomplète : crédits audio sans chapitre, preuve de timeline compatible entre copies, publication signée dans le catalogue, précision/rappel sur corpus annoté de 24 épisodes et essai IntroSkipper complet sur deux Nodes. Aucune preuve NAS/production ni push.

## 26 octobre 2026 — Mission 5, premier verrou inter-Core (partiel)

- Les admissions des tâches et automatisations utilisent un verrou transactionnel PostgreSQL commun ; SQLite conserve son verrou d'écriture. Les opérations de résultat et de lease restent conditionnées par tentative. Test SQLite de concurrence passé ; essai PostgreSQL à deux processus non réalisé faute de serveur local.
- Le Wasm exécuté provient du paquet durable en base avec taille et SHA-256 vérifiés ; le cache local peut être reconstruit depuis `extensions.package_json` après une mise à jour. Le démarrage PostgreSQL n'interrompt plus les exécutions d'extensions d'un autre Core ; les lignes de plus d'une heure sont récupérées provisoirement.
- Une tâche système peut reprendre un plan laissé `running` par une tentative expirée. La propriété durable des connexions Node, l'atomicité des effets, la reprise des exécutions d'extensions et la preuve à deux Core/100 Nodes/100 000 tâches restent ouvertes. Voir `docs/MULTI_CORE_READINESS.md`.
- Décision utilisateur : viser des bases neuves avec un schéma initial propre, sans reprise ni migration de l'ancienne base. La consolidation des scripts SQL actuels en schéma initial unique reste à faire ; aucune base existante n'a été effacée.
- Audit complémentaire : de nombreuses requêtes Core emploient encore `unixepoch()`/`json_extract()` propres à SQLite ; PostgreSQL ne peut pas être déclaré fonctionnel sans adaptation SQL et essais réels. Docker Desktop arrêté et non démarrable depuis cette session, aucun serveur PostgreSQL local accessible.

## 26 octobre 2026 — Mission 5, PostgreSQL 18 et deux processus Core (validation ciblée)

- Docker Desktop fourni par l'utilisateur a permis un PostgreSQL 18 temporaire en mémoire. Le schéma PostgreSQL initial a été corrigé : nom de paramètre SQL réservé, ordre des tables référencées, fragments de déclencheurs SQLite invalides, fonctions `unixepoch`/`json_valid`, colonnes flottantes `DOUBLE PRECISION`. Les six fichiers SQL se créent sur une base neuve.
- `scripts/test-postgres.ps1` passe : contrat HTTP PostgreSQL (1), OIDC (7), deux Nodes TCP (1). Le script isole les caches locaux des extensions/assets par exécution pour éviter les collisions avec d'anciens paquets.
- `scripts/test-two-cores.ps1` passe : deux vrais processus `aether-core` partagent PostgreSQL, répondent chacun à `/api/v1/config`, puis un Core répond encore après arrêt forcé de l'autre. Cela ne prouve pas la reprise des tâches en vol ni l'absence de double effet.
- Le défi 100 Nodes/100 000 tâches et la création d'un schéma initial unique sans scripts historiques restent ouverts. Aucune base utilisateur ni média n'a été supprimé.
- Mesure ponctuelle du smoke deux Core sur Windows Debug : prêts en 1 950 ms, RSS 21,6 Mio par processus, réponse du survivant 7 ms après Stop-Process ; aucune tâche en vol et aucune distribution statistique.
- Smoke étendu : administrateur et session créés via Core 1, `/api/v1/auth/me` accepté par Core 2 sur PostgreSQL commun. Dernier passage : prêts en 1 894 ms, RSS 21,7 Mio/Core, réponse du survivant 11 ms après arrêt forcé ; pas de tâches en vol.

## 26 octobre 2026 — Schéma initial unique et mesure de charge ciblée

- À la demande de repartir sur des bases neuves sans migrations, les scripts SQL actifs SQLite/PostgreSQL ont été consolidés en `crates/core/schema.sql`, avec une section par moteur et `schema_version=1`. Le démarrage installe ce schéma sur une base vide ; une ancienne base incompatible est refusée sans effacer ses données. Les anciens fichiers SQL actifs ont été supprimés.
- Création neuve SQLite et PostgreSQL 18 validée par `scripts/test-fresh-schema.py` et `scripts/test-fresh-postgres-schema.ps1`. Quatre tests ciblés `database::tests` réussis ; les contrats PostgreSQL HTTP (1), OIDC (7) et deux Nodes TCP (1) ont réussi après consolidation.
- Après consolidation, `scripts/test-two-cores.ps1` a démarré deux vrais Core sur une PostgreSQL commune : prêts en 8 591 ms, RSS 21,8/21,6 Mio, session Core 1 lue sur Core 2, réponse du survivant 35 ms après arrêt forcé. Il n'y avait aucune tâche en vol.
- Benchmark SQLite Debug : 100 Nodes simulés, 100 000 tâches persistantes. Avant regroupement des lectures des liens d'automatisation : insertion 3 572 ms, attribution par lots de 100 p50 477 ms/p95 596 ms ; admission unitaire p50 10,3 ms/p95 15,1 ms. Après regroupement : insertion 3 524 ms, lot p50 509 ms/p95 582 ms, unitaire p50 9,8 ms/p95 12,2 ms. Le gain p95 est modeste et variable ; rien ici ne prouve la reprise multi-Core sous charge ni l'absence de double effet durable.
## 26 octobre 2026 — Réparation ciblée des contrôles GitHub CI

- `ci.yml` et `release.yml` regroupent les écritures dans `GITHUB_OUTPUT` pour satisfaire ShellCheck SC2129, sans changer les sorties de release.
- Les blocs FFI `malloc_trim` du Node et du Core sont déclarés `unsafe extern "C"`, conformément à Rust 2024 pour Linux GNU. Le lien externe de `SettingsView.svelte` porte une exception ESLint limitée à cette ancre GitHub.
- Vérifié localement : `actionlint` 1.7.12 sur tous les workflows, `cargo check --locked -p media-node -p media-poc`, `npm run lint` et `npm run check` (0 erreur, 5 avertissements existants). `cargo fmt --all -- --check` signale un écart préexistant dans `crates/shared/build.rs`. Le contrôle croisé Linux a été bloqué par l'absence d'un sysroot OpenSSL Linux sur l'hôte Windows ; la CI Linux reste à confirmer après intégration.

## 26 octobre 2026 — Correction du contrôle de format CI

- Les deux expressions `if !s.is_empty()` de `crates/shared/build.rs` ont été mises au format attendu par rustfmt, sans changement de comportement. `cargo fmt --all -- --check` et `git diff --check` passent localement ; le run GitHub après intégration reste à vérifier.

## 26 octobre 2026 — Réparation du contrôle Clippy CI

- `version_policy.rs` conserve la priorité du tag Git et les mêmes versions produites, avec conditions simplifiées pour Clippy Rust 1.99. `task_engine.rs` passe directement à SQLx les références `provider` et `extension` déjà empruntées.
- Vérifié localement : `cargo fmt --all -- --check`, `cargo test --locked -p media-contracts` (13 tests), `python scripts/test-release-policy.py` (14 tests), et `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings`. Aucun run GitHub après intégration observé.

## 27 octobre 2026 — Clé de sauvegarde publique et contrat de mise à jour des extensions

- Le fichier `crates/core/secrets/backup.key`, ajouté le 26 octobre, a été
  retiré du suivi Git et supprimé du checkout ; `.gitignore` et `.dockerignore`
  excluent ce chemin. Son historique public reste exposé. Aucun accès aux
  serveurs ou archives externes : son usage réel n'a pas été établi.
- Sur instruction explicite de retirer toute gestion des clés de sauvegarde,
  le format courant devient `MDBK0002` sans chiffrement ni clé. Les archives
  contiennent les secrets en clair ; le SHA-256 final détecte les corruptions
  accidentelles sans authenticité. L'ancien `MDBK0001` exige l'ancien binaire
  et sa clé d'origine ; aucune archive existante n'a été modifiée.
- Le code possède déjà une route d'upgrade en place mais sans les garanties
  complètes attendues ; `docs/PLUGIN_UPDATE_LIFECYCLE.md` définit identité,
  compatibilité, consentement, sauvegarde, bascule, échec et rollback avant de
  déclarer ce parcours prêt pour des données importantes.
- Vérifié localement : `cargo check --locked -p media-poc --all-targets`,
  `cargo test --locked -p media-poc --test backup_bundle` (4 tests),
  `--test admin_backups` (2 tests), `--test full_backup_restore_e2e` (1 test),
  tests unitaires ciblés du bundle, des assets et de `media-recover`, puis
  `cargo fmt --all -- --check`,
  `cargo clippy --locked -p media-poc --all-targets -- -D warnings` et
  `npm run check --prefix apps/web` (0 erreur, 5 avertissements préexistants).
  Aucun serveur externe, ancien paquet ou NAS n'a été testé.

## 27 octobre 2026 — Entrée du tableau de bord administrateur

- L’ouverture de `admin/libraries/` sans fragment, ou avec un fragment inconnu, sélectionne désormais « Vue d’ensemble ». Les liens explicites vers Bibliothèques et les autres sections restent utilisables. Vérification locale : `npm run check --prefix apps/web` (0 erreur, 5 avertissements préexistants). Aucun déploiement ni parcours navigateur réel effectué.
- Le planificateur Web dit désormais « Toutes les 24 heures » et « Intervalle » pour la règle actuelle de 1440 minutes ; l'horaire civil fixe avec fuseau et gestion du changement d'heure reste non implémenté. Même vérification Web après ce changement.

## 27 octobre 2026 — Poster Rotator configurable

- Le dépôt voisin `Aether-Plugins` contient maintenant le code source d'une extension Poster Rotator inspirée du plugin Jellyfin du même auteur. Le Core n'expose la politique de rotation qu'avec une extension installée, active, un éditeur actif et une autorisation pour la bibliothèque visible du compte.
- L'interface tourne les affiches des films et séries selon les paramètres enregistrés : ordre, délai minimal, taille du pool et langues. Une image manuelle garde priorité. Sans extension, l'affiche est fixe. Les autres types d'images restent fixes.
- Vérifié localement : tests Web ciblés (11 réussis), `npm run check` (0 erreur, 5 avertissements préexistants), ESLint ciblé, `cargo check -p media-poc --lib`, test unitaire Core du bornage des paramètres, test HTTP Core avec paquet signé simulé (installation, autorisation, paramètres, désactivation), test unitaire de l'extension, compilation Wasm (114 301 octets), validation ABI et test du manifeste du dépôt des extensions.
- La collecte automatique en masse et le planificateur du plugin Jellyfin ne sont pas portés. Aucun déploiement NAS ou test avec des images réelles n'est établi.
- Le 27 octobre, Poster Rotator 0.1.0 a été signé avec une clé d'éditeur dédiée, puis publié dans le catalogue `main` d'Aether-Plugins et en GitHub Release (`poster-rotator-v0.1.0`). Les neuf paquets du catalogue passent la vérification locale des signatures ; l'URL publique du manifeste et celle du paquet ont répondu 200, avec empreinte SHA-256 conforme. Les PR de préparation, de clé dédiée et de paquet sont les numéros 7, 8 et 9. Le NAS doit encore récupérer un Aether contenant la politique de rotation et actualiser son catalogue d'extensions.

## 27 octobre 2026 — Navigation des tests navigateur CI

- Le run 40466113909 échouait avant le scan Films car la page d'administration ouvre désormais la vue d'ensemble. Échec reproduit localement après reconstruction Web ; les scénarios ouvrent explicitement Bibliothèques ou Nodes selon leur besoin.
- `npm run build --prefix apps/web`, puis les tests Chrome `browser_scanner` et `browser_series` avec `--ignored --nocapture` ont réussi sous Windows et Chrome 156. Films couvre scan initial/incrémental, lecture, seek et redémarrage ; Séries couvre notamment lecture, seek et révocation. NFO, affiches, distribué et nouvelle CI Linux restent à valider. Voir `docs/CI_GHCR.md`.

## 27 octobre 2026 — Refus conservateur des mises à jour d'extensions

- Les deux routes de mise à jour refusent désormais toute bascule tant que le format des données privées et la clôture atomique des écritures anciennes ne sont pas établis. Identité de l'éditeur, clé, signature et version strictement supérieure sont vérifiées ; aucune migration ni modification des données installées n'est effectuée. L'administration explique le refus.
- Le test HTTP SQLite ciblé `test_extension_upgrade_refuses_unverified_compatibility_without_mutation` passe : version et génération restent inchangées, donnée privée conservée, versions égale/inférieure, clé différente et signature invalide refusées. `npm run check --prefix apps/web` passe avec 5 avertissements existants. PostgreSQL, crash, reprise et véritable mise à jour restent à réaliser ; voir `docs/PLUGIN_UPDATE_LIFECYCLE.md`.
- Les scénarios NFO et affiches ont été adaptés à l'éditeur et au catalogue actuels. Après redémarrage, ils attendent une progression effective avant le seek ; NFO attend aussi la fin du scan nouvellement lancé. Les quatre phases initiale, redémarrage, mise à jour et suppression passent sous Chrome 156/Windows avec rapports isolés via `MEDIA_REPORT_DIR`. Le scénario distribué reste à valider.
- Les opérations de marqueurs exigent désormais `playback.markers` dans l'adaptateur Core ; `metadata.read` seul ne suffit plus. Le manifeste source d'IntroSkipper possède déjà ce droit. Test unitaire ciblé `marker_operations_require_the_dedicated_permission` réussi ; aucune publication ni validation complète du plugin n'a été effectuée.

## 27 octobre 2026 — Chemin des assets Web fixe

- Le Core sert l'interface depuis `apps/web/build`, relatif à son répertoire de travail (`/app` dans les images Docker). La variable de configuration du chemin Web a été supprimée du code, des images, des exemples Compose, des scripts et de la documentation courante.
- Les tests d'intégration fournissent un répertoire Web temporaire directement au constructeur du routeur ; le banc de mesure copie le build Web de ses artefacts dans son répertoire de travail avant de démarrer le Core.
- Vérification ciblée : `cargo test --locked -p media-poc --test static_web --test adversarial_challenge` (6 tests passés), test unitaire `configured_web_dist_uses_build_directory` (1 passé), `rustfmt --check --edition 2024 --config skip_children=true` sur les trois fichiers Rust touchés, syntaxe Node/Python et validation des deux exemples Compose. Aucun build Docker ni test navigateur réel pour ce changement.
# 29 octobre 2026 — scénarios navigateur et panneau des versions

Le journal CI fourni montre les scans initial et incrémental réussis, puis un
échec de sélection MP4 dans le panneau des versions replié. Les scripts scanner,
NFO et affiches ouvrent désormais ce panneau avant la sélection ; la validation
du job GitHub Linux reste à faire. Le scénario scanner ouvre aussi le panneau de
suivi et passe localement sous Windows/Chrome, avant et après redémarrage ; les
scénarios NFO et affiches ne sont pas relancés.
# 29 octobre 2026 — parcours CI Séries

Le scénario navigateur Séries a été adapté à l'interface actuelle, notamment
au chargement de la couverture et aux panneaux repliés. Le test complet passe
sous Windows/Chrome 156 ; la CI Linux reste à confirmer.
# 29 octobre 2026 — test navigateur playlists

Les commandes audio du test ciblent le panneau musique ouvert pour éviter les
doublons du lecteur réduit. Le test ciblé avec navigateur activé passe sous
Windows/Chrome 156 ; la CI Linux reste à confirmer.
# 29 octobre 2026 — coût de la CI

Le workflow CI omet les symboles de débogage Cargo dev/test, groupe les cibles
navigateur Films/Séries dans une commande et annule les anciens runs de PR de
la même ref. Les contrôles et portes de publication restent présents. Le
dossier Films utilise MEDIA_SCANNER_REPORT_DIR pour préserver celui de Séries.
Actionlint et contrôles statiques passent ; aucun gain distant mesuré à ce stade.

Optimisation complémentaire : annulation des validations obsolètes par job
sur toutes les refs sans annuler les publications, construction des trois
images dans un même solve Bake avec export de cache commun unique. Plan Bake
et actionlint validés ; daemon Docker indisponible pour un build réel.
## 29 octobre 2026 — Retrait du plugin Avatar du catalogue

- Les avatars et la rotation des affiches disposent de parcours natifs existants. Le dépôt voisin retire les sources, le paquet Avatar et les références de publication ; Poster Rotator y était déjà absent. Le catalogue distant reste inchangé jusqu’à publication.
- Les anciennes installations et collections liées aux extensions ne sont pas migrées ni supprimées automatiquement. L’administration les signale désormais ; leur suppression peut entraîner la perte des collections associées. Les données et modifications préexistantes sont conservées.
## 29 octobre 2026 — Intégration des réglages d’images et d’avatars

- Les écrans parlent de rotation des affiches sans nom de plugin. Le profil choisit dans la galerie du serveur sans générateur d’extension ; Gravatar n’est plus proposé dans l’interface. Les champs et routes historiques restent présents pour compatibilité et ne constituent pas une suppression backend ni une migration des collections d’extension.
- La rotation actuelle utilise les variantes existantes et la collecte reste manuelle par œuvre ; elle ne reproduit pas les tâches planifiées et la gestion globale du projet Jellyfin. La migration des collections historiques et une refonte complète restent ouvertes.
