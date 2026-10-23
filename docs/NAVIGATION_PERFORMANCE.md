# Navigation et lancement du lecteur — 23 octobre 2026

## Changement livré

Le catalogue conserve ses pages en mémoire de la session : clé complète incluant
section, bibliothèque, recherche, cinq filtres, tri, offset et limite. Fraîcheur
30 s, LRU de 48 pages et plafond de 4 Mio de JSON UTF-8. Ce plafond mesure le
contenu sérialisé, pas le heap JavaScript exact ; objets, chaînes et proxies ont
un surcoût. Les requêtes en vol sont également limitées à 48.

Une page fraîche est affichée sans requête catalogue. Une page périmée est affichée
avant son actualisation. Une erreur réseau conserve les données et affiche un
avis ; 401/403 effacent les caches concernés. Les requêtes communes sont partagées,
les réponses d'une navigation abandonnée ne modifient pas sa remplaçante. Quitter
une page laisse son fetch partagé finir, sous le timeout existant ; une invalidation
l'annule. Aucun cache privé persistant ni service worker média ajouté.

Les changements de session (y compris reconnexion du même compte) purgent pages,
descripteurs et positions. Les mutations administratives invalident les pages de
la bibliothèque et les agrégats ; les changements de droits/nœuds invalident tout.
La fin d'un scan observée dans l'administration invalide sa bibliothèque. Les
changements d'affiches invalident aussi les pages ; la liste des bibliothèques
rechargée détecte retraits/renommages. Les scans externes/planifiés ne sont pas
poussés au navigateur : ils apparaissent à la prochaine consultation périmée
(après 30 s de fraîcheur), sans nouveau protocole de notifications serveur.

La navigation des actions de lecture et des filtres utilise SvelteKit. Les positions
de défilement des catalogues sont conservées, avec contrôle de navigation/session
avant restauration. L'historique de reprise utilise son cache existant au retour ;
les écritures de progression continuent de le rafraîchir.

Le dialogue lecteur s'ouvre pendant la résolution des sources. Le descripteur est
toujours revalidé avant lecture, les jetons précédents ne sont jamais réutilisés
pour contourner cette étape. Les requêtes simultanées de descripteur sont partagées,
avec annulation indépendante des consommateurs. Les métadonnées préchargées servent
à sélectionner une version ; le module lecteur reste préchargé sur la fiche.
Pour un épisode multiversion froid, les descripteurs frais sont demandés en parallèle
et la meilleure version est utilisée sans seconde revalidation séquentielle.
Fermer le dialogue empêche une réponse tardive de démarrer la lecture.

Le moteur native.ts conserve ses contrôles d'expiration, son chargement initial
sur la bonne piste, la vidéo commune aux changements audio, et les caches/index
existants du Node. Aucun transcodage ni timeout augmenté. Le statut de préparation
audio est maintenant affiché dans le dialogue au lieu du texte générique constant.

## Mesures observées

Chrome **155.0.8059.86**, Windows, headless. [Rapport UI](evidence/2026-10-23/navigation/chrome.json).
UI Svelte réelle via Vite ; API interceptée : catalogue 180 ms, descripteur 300 ms ;
MP4 synthétique légal. Avant = Catalog/api/ImmersivePlayer de HEAD ; après = fichiers
modifiés. Les autres corrections de navigation restent communes à cette comparaison.
Un seul essai comparatif, pas une garantie de latence ; CPU partagé avec des tests.

| Mesure | Avant | Après |
| --- | --- | --- |
| 10 retours accueil, durée automatisée clic → catalogue | 331–359 ms | 62–74 ms |
| Requêtes catalogue sur ces retours | 20 | 0 |
| Clic → dialogue, horloge navigateur, ouverture / réouverture | 322 / 314 ms | 8 / 3 ms |
| Clic → première image, horloge navigateur | 514 / 421 ms | 416 / 402 ms |
| Clic → événement playing | 514 / 415 ms | 417 / 403 ms |
| Descripteurs frais demandés par lancement | 1 | 1 |
| Heap JS global à la fin (parcours différents, sans GC forcé) | 20,37 Mo | 16,50 Mo |

Le heap global ne mesure pas isolément le cache : le parcours après comprend aussi
série/épisode et deux bibliothèques. Aucune conclusion de baisse de RAM. Les tests
vérifient séparément éviction, 48 pages, rejet d'une entrée dépassant 4 Mio et purge.
Le [dernier contrôle du code final](evidence/2026-10-23/navigation/chrome-final-regression.json)
confirme aussi le scroll, l'épisode multiversion et l'annulation avant réponse sans
réouverture tardive. Ses retours sont à 68–87 ms et son dialogue à 10/4 ms ; première
image à 650/423 ms. La variabilité du premier lancement interdit de promettre un
gain systématique de première image sur ces seuls essais.
Le délai catalogue inclut l'automatisation ; aucune attente HTTP catalogue ne reste
sur le retour frais. L'absence de loader bloquant est aussi observée par MutationObserver
sur le retour Films connu.

Décodage séparé avec le moteur de production, HTTP local et FFmpeg copie sans
réencodage : [rapport](evidence/2026-10-23/navigation/decoding.json). AV1 Main 10 bits
1080p / deux Opus, première image 106 ms, lecture prête 612 ms, réouvertures prêtes
16 / 46 ms. PCM FR/EN distinct, pause, seeks, restauration après 404 et annulation
validés. Ce rapport ne compare pas avant/après, ne mesure pas le premier son physique,
et playReady/playing ne sont pas une mesure du premier échantillon audible.
Le temps audio initial exact et sa décomposition réseau/préparation/décodage restent
à instrumenter sur l'installation réelle.

Vrai Core/SQLite/Node local, MP4 puis MKV après redémarrage :
[avant redémarrage](evidence/2026-10-23/navigation/core-scanner.json),
[après](evidence/2026-10-23/navigation/core-scanner-restart.json).
Lecture, PCM, langues, sous-titres, pause/seek, progression et réouvertures réussis.
Une seconde exécution sur le build suivant réussit également :
[rapport](evidence/2026-10-23/navigation/core-scanner-final.json),
[redémarrage](evidence/2026-10-23/navigation/core-scanner-final-restart.json).
Logs de cette exécution : MP4 audio première fenêtre 258 ms de préparation,
HTTP prêt 260 ms ; MKV/sous-titres déjà préparés : cache 2–5 ms. Ce n'est pas
une preuve du débit disque NAS ni du délai de bout en bout sur sa bibliothèque.

Benchmark SQLite existant, debug Windows : 1 000 œuvres, première page 10,55 ms,
page profonde 19,78 ms, recherche 6,50 ms ; 10 000 œuvres : 86,38 / 173,53 / 24,79 ms.
SQL, accès disque, sérialisation JSON et résolution distribuée ne sont pas chronométrés
séparément dans cette demande. PostgreSQL réel non mesuré. Aucun changement Rust/SQL
proposé sur cette base ; les chemins serveur restent inchangés.

## Validation et limites

- Web : 104 tests, check Svelte/TypeScript, lint et build.
- Catalogue : dix allers-retours, films/séries/films, série/épisode/accueil,
  ouverture/fermeture/réouverture, absence de loader au retour frais : Chrome UI.
- Changement de compte, déconnexion/reconnexion, réponses tardives, annulations,
  erreur réseau, cache périmé et éviction : tests Vitest dédiés et tests session/API.
- Bibliothèque modifiée/supprimée, permissions vivantes, deux Nodes et streaming :
  tests Rust catalog_performance (5), scanner_flow (3), distributed_multi_node (1).
  Les transitions de cache sont aussi testées avec invalidation ciblée.
- Indisponibilité/révocation Node : couverte côté serveur par distributed_multi_node ;
  pas de coupure d'un NAS réel dans Chrome.
- Scroll : retour à 200 px validé dans Chrome sur catalogue synthétique ; grand
  catalogue de production non mesuré.
- Docker : `docker info` échoue, moteur Docker Desktop absent. Aucun test Docker
  exécuté ici. NAS, PostgreSQL, première sortie sonore physique et charge non validés.

## Reproduction locale

```powershell
npm test --prefix apps/web
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
cargo test --locked -p media-poc --test catalog_performance --test scanner_flow --test distributed_multi_node -j 1 -- --test-threads=1
$env:MEDIA_REPORT_DIR='target/navigation-performance/core-scanner'
cargo test --locked -p media-poc --test browser_scanner -j 1 -- --ignored --nocapture
# Terminal séparé :
npm run dev --prefix apps/web -- --port 5179
# Autre terminal :
node scripts/browser-navigation-performance.cjs
```

Le script teste par défaut les sources courantes. `MEDIA_COMPARE_BASELINE=1` active
la comparaison à HEAD : il remplace temporairement les trois fichiers annoncés dans
le script, puis les restaure dans finally. À exécuter sans édition concurrente ;
ne pas interrompre brutalement le processus durant cette comparaison.

Décodage / Docker jetable, une fois Docker disponible, sans remplacer les services :

```powershell
$env:MEDIA_TEST_SEGMENTED='1'
$env:MEDIA_REPORT_DIR='target/navigation-performance/decoding'
node scripts/browser-dual-opus.cjs
$env:MEDIA_TEST_DOCKER='1'
$env:MEDIA_REPORT_DIR='target/navigation-performance/docker'
node scripts/browser-dual-opus.cjs
```

## Mesurer sur le NAS

Après déploiement explicite par l'administrateur, ouvrir Chrome DevTools → Network,
conserver le journal et laisser le cache navigateur actif. Noter Chrome/OS, format,
langues, nœud, mode direct/relais et état froid/chaud. Faire les neuf scénarios de la
demande, dont dix retours espacés de moins de 30 s, puis un retour après 30 s.
Filtrer `/api/v1/catalog?` : zéro nouvelle requête sur le retour frais ; actualisation
discrète après expiration. Tester également Offline après expiration : les cartes
connues doivent rester avec un avis. Réactiver le réseau avant connexion/déconnexion.

Enregistrer une trace Performance par lancement : clic, ouverture du dialogue,
première image (requestVideoFrameCallback), événement playing, source audio et PCM
via Web Audio si permis. Mesurer le premier son séparément ; ne pas assimiler
loadedmetadata ou canPlayType à du décodage. Répéter au moins dix fois par média,
à froid puis à chaud, et rapporter médiane/p95 sans exporter cookies ou URLs signées.
DevTools Memory : snapshots après GC au catalogue initial puis après 50 recherches,
tri/pagination et retours ; vérifier stabilisation et purge après déconnexion.

Sur l'hôte Docker (adapter les noms, commandes de lecture uniquement) :

```sh
docker stats --no-stream aether-core
docker logs --since 10m aether-core 2>&1
```

Faire aussi ces lectures pour les Nodes distants. Corréler les événements
`preparation_requested`, `preparation_cache_hit`, `preparation_ready`,
`media_http_response`, `prepared_remote_response` et `node_prepared_response`.
Comparer TTFB catalogue/descripteur, temps Core/Node, préparation audio, nombre de
préparations et CPU/RSS. Les champs `elapsed_ms` HTTP décrivent la création de la
réponse, pas la fin du transfert. Ne modifier SQL/index/packaging qu'après ce profilage.
Voir aussi [diagnostics existants](NAS_PLAYBACK_DIAGNOSTICS.md).


## Complément du 23 octobre — préchargement et images

Les liens Accueil et bibliothèques Films/Séries anticipent la première page au
survol/focus. Le cache utilise maintenant l'URL complète sans préfixe de section :
une réponse identique est partagée entre navigation anticipée et affichage. Les
filtres, le tri, la pagination et la bibliothèque restent séparés ; les purges de
session/droits et les plafonds existants restent actifs. Le lecteur optionnel est
importé après chargement du catalogue. Un clic Lecture anticipe la progression
pendant la revalidation obligatoire du descripteur, y compris sans survol tactile.

Le lecteur charge l'audio Ogg indépendant déjà préparé pendant les métadonnées
vidéo ; il ne l'active qu'après réussite des deux. La préparation serveur reste
séquentielle ; Opus segmenté garde son initialisation après l'index vidéo. Aucun
transcodage ajouté. Les affiches deviennent visibles au chargement sans le fondu
supplémentaire de 300 ms. La requête catalogue ne résout plus trois fois les images
avant la résolution effective groupée (qui applique masquage/ordre/permissions).

Mesures locales, Windows/Chrome 155, une exécution, charge machine variable :

- UI réelle/API simulée avec catalogue 180 ms et descripteur 300 ms : dix retours
  89–134 ms, zéro requête catalogue ; dialogue 12/7 ms. Première image 752/488 ms,
  playing 663/491 ms. Le scénario ne satisfait pas 200 ms de bout en bout.
- Moteur réel/HTTP isolé/fixture AV1 10 bits et deux Opus 5.1 : Ogg indépendant,
  première image 88 ms, lecture prête 188 ms, réouvertures 53/67 ms ; Opus segmenté,
  première image 263 ms, lecture prête 585 ms, réouvertures 78/53 ms. Décodage PCM,
  langues, pause/seek, restauration après erreur et annulation segmentée réussis.
  Pas de mesure du premier son physique, du NAS ou du parcours complet Core/Node.
- SQLite debug, 10 000 œuvres : première page 269 → 345 ms, page profonde
  599 → 486 ms, recherche 91 → 73 ms ; 1 000 œuvres : 63 → 29 ms, profonde
  87 → 31 ms. Exécutions uniques non isolées : aucun gain général ni objectif
  serveur sous 200 ms démontré. Le contrôle EXPLAIN historique utilise une requête
  simplifiée ; il ne prouve pas le plan exact de catalogue_filtered.

Preuves : [navigation](evidence/2026-10-23/catalog-startup/navigation.json),
[décodage Ogg](evidence/2026-10-23/catalog-startup/decoding.json),
[décodage segmenté](evidence/2026-10-23/catalog-startup/segmented.json).

Validation : 106 tests Web, check/lint/build ; 5 tests catalogue, 5 images Node,
7 scanner Node ; navigateur navigation et les deux modes audio ci-dessus.
Les nouveaux tests couvrent le partage préchargement/affichage, le chargement
audio concurrent sans activation prématurée, les alias d'affiches et les fonds
préfixés sans image générique. Pas de déploiement ni modification de médias.
La cible 200 ms reste ouverte pour le premier chargement et l'installation réelle.

Le [contrôle ciblé de préchargement](evidence/2026-10-23/catalog-startup/navigation-prefetch.json)
visite une bibliothèque encore absente du cache : après survol et réception de
sa page, clic → cartes 111 ms, zéro requête supplémentaire et aucun loader.
Ce temps exclut volontairement l'anticipation avant clic ; il ne mesure pas un
premier chargement sans cache ni intention préalable. Les dix retours de cette
exécution sont à 76–125 ms.

Commandes exécutées pour ce complément :

```powershell
npm test --prefix apps/web
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
cargo test --locked -p media-poc --test catalog_performance -j 1 -- --nocapture --test-threads=1
cargo test --locked -p media-node --lib images:: -j 1 -- --test-threads=1
cargo test --locked -p media-node --lib scanner::tests:: -j 1 -- --test-threads=1
cargo test --locked -p media-poc --test posters_flow -j 1 -- --test-threads=1
cargo fmt --check
cargo clippy --locked --all-targets -j 1 -- -D warnings
node scripts/browser-navigation-performance.cjs
$env:MEDIA_REPORT_DIR='target/catalog-startup/decoding'
node scripts/browser-dual-opus.cjs
$env:MEDIA_TEST_SEGMENTED='1'
$env:MEDIA_REPORT_DIR='target/catalog-startup/segmented'
node scripts/browser-dual-opus.cjs
git diff --check
```

Les contrôles ont également révélé des 404 de logos/indicateurs sur les routes
Films/Séries : construire un chemin d'image à partir de `resolve('/')` produisait
une URL relative dépendant du document. Les six composants concernés utilisent
désormais `asset('images/...')` de SvelteKit (y compris connexion/administration).
La configuration paths.relative=false évite aussi de conserver un chemin relatif
SSR après navigation cliente depuis l'accueil. Le préfixe /app reste configurable
dans Vite. Le test navigateur signale toute réponse HTTP d'image en erreur.
Le test HTTP posters_flow réussit : images catalogue, GET/HEAD/Range, miniatures,
ETag et permissions/révocation. Format Rust et Clippy all-targets strict réussis.

Contrôle final sur **build de production** servi par `vite preview`, après correction
des chemins : [rapport](evidence/2026-10-23/catalog-startup/production-final.json).
Zéro erreur JS ou image HTTP ; dix retours catalogue 72–104 ms, bibliothèque
préchargée 122 ms après clic sans nouvelle requête. Dialogue 15/3 ms ; première
image 644/384 ms avec le descripteur artificiellement retardé de 300 ms. Ces
mesures finales ne changent pas les limites des essais média isolés ci-dessus.
