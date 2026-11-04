# Mission fonctionnalités — constat initial du 3 novembre 2026

Audit statique ciblé : les sources indiquent les parcours présents, sans prouver leur
fonctionnement courant. Aucun test, déploiement ou appel fournisseur n'a été effectué
pour établir cette matrice. Les validations ultérieures doivent être consignées par lot.

## Suivi actuel des lots

Le tableau initial ci-dessous décrit le départ de la mission. Les commits
utilisateur `525b61f` (Aether) et `d07160d` (Plugins) contiennent les premiers lots.
Les validations suivantes portent aussi sur les corrections locales ; le travail
visuel concurrent est conservé et n'est pas attribué à cette mission.

| Domaine | État vérifié localement | Limite restante |
|---|---|---|
| OpenSubtitles | Partielle : manuel, worker signé, droits, quotas, dérivés SQLite/PostgreSQL et piste rendue après redémarrage Core | Compte/service réel, hash et automatisation non validés ou indisponibles |
| TVDB | Partielle : fournisseur optionnel, cinq tests natifs, vrai guest hors ligne et recherche/détails/images validés sous les limites Wasmi du Core | Licence/compte réel, application Core et fuel sur réponses réelles non vérifiés |
| Chapitres | Partielle : vrais chapitres à la demande, extraction FFmpeg, cache/reprise et droits Core vérifiés | Direct/canal inverse TCP local validés ; machine distante physique et pannes non éprouvées, génération pendant scan indisponible |
| Recommandations | Partielle : moteur local facultatif, trois tests Core et parcours Web réussis | Pertinence utilisateur et performance NAS non mesurées ; pool borné à 1 000 titres |
| Notifications | Partielle : six tests SQLite/receveurs locaux et contrat PostgreSQL réussis, quatre adaptateurs ; tâches/scan/nouveau contenu raccordés | Node indisponible et erreur fournisseur non raccordés ; pas d'envoi externe réel ni livraison exactement une fois garantie |
| Docker | Partielle : trois images AMD64 construites et smoke réussis ; CI native et publication conditionnelle sur deux architectures préparées | Construction ARM64 interrompue par arrêt de Docker ; trois images finales non validées et aucun matériel ARM physique attesté |
| Multi-Core | Partielle, expérimental : ownership/heartbeat/fencing de l'historique testé SQLite et PostgreSQL, panne d'une invocation sur deux vrais Core vérifiée | Routage Nodes, partitions et fencing de tous les effets restent ouverts ; pas de readiness production |

Preuves : [Core](MISSION_CORE_VALIDATION_2026_11_03.md),
[Web](MISSION_WEB_VALIDATION_2026_11_03.md),
[Docker](DOCKER_MULTIARCH_2026_11_03.md),
[TVDB](../../Aether-Plugins/docs/TVDB_2026_11_03.md).
Seize tests Core ciblés ont réussi ; les tests PostgreSQL notifications et
ownership ont chacun réussi sur une base jetable avec deux pools indépendants.
Ces deux pools ne constituent pas une preuve de deux processus Core.
Un essai distinct avec deux vrais processus Core et PostgreSQL a ensuite réussi
en 43,48 s : écriture Wasm privée bloquée, arrêt brutal du propriétaire,
historique interrompu après expiration, zéro effet de la tentative tuée, puis
un seul effet après nouvelle invocation explicite sur le survivant. Ce scénario
ne prouve ni le rejeu automatique des tâches ni le routage des Nodes.
Le lecteur Chrome/Core réel MP4 puis MKV après redémarrage a réussi en 68,20 s,
avec vidéo décodée, PCM, seek/synchronisation, ASS et nouveau dérivé applicatif.
Il ne prouve ni le service OpenSubtitles, ni une sortie audio physique.

Les nouvelles CI ne sont pas déclarées validées : après fetch, les deux dépôts
ont encore deux commits locaux d'avance sur `origin/develop`. Aucun push,
paquet signé publié, manifest publié ou déploiement réel n'a été réalisé ici.

Complément du 4 novembre : notifications **6/6 en 10,26 s**, contrat PostgreSQL
18 enrichi **1/1 en 2,39 s** avec deux pools. L'abonnement aux nouveaux contenus
est séparé, désactivé par défaut et n'annonce pas l'ancien catalogue. Le test
couvre aussi contrôle parental, révocation des droits et payload sans titre.
Le module TVDB optimisé réel passe recherche (20 résultats), détails et images
dans Wasmi avec les limites du Core : **1/1 en 9,98 s**, réponses synthétiques
sans réseau. Ce passage ne prouve pas l'accès au fournisseur réel.

| Domaine | État initial | Interface → API/validation → données → exécution → tests existants |
|---|---|---|
| OpenSubtitles | Fournisseur absent ; options déclaratives | [Options](../apps/web/src/lib/admin/AdvancedLibraryOptions.svelte) → [options validées/persistées](../crates/core/src/library_options.rs), sans recherche/téléchargement. Aucun package dans [workspace plugins](../../Aether-Plugins/Cargo.toml). Le pipeline des extensions et le lecteur de sous-titres existants sont à réutiliser. Validation du service réel non effectuée. |
| TVDB | Fournisseur absent ; identités déjà prises en charge | [Identification](../apps/web/src/lib/catalog/IdentifyModal.svelte), badges → [adaptateur](../crates/core/src/extension_adapters.rs), [normalisation](../crates/core/src/consolidation.rs) → identités catalogue/NFO → [scanner](../crates/node/src/scanner.rs), [NFO](../crates/node/src/nfo.rs). Tests d'identité dans [series_flow](../crates/core/tests/series_flow.rs) et [consolidation](../crates/core/tests/global_catalogue_consolidation.rs), pas de preuve API TVDB. |
| Images de chapitres | Déclaratives seulement ; chapitres et Trickplay distincts existants | Options « moteur à venir » → booléens bibliothèque persistés ; pas de route/tâche/image dédiée trouvée. [Probe](../crates/node/src/probe.rs) extrait déjà les chapitres ; [lecteur](../apps/web/src/lib/player/PlayerControls.svelte) affiche titres/repères/sauts. [Trickplay Core](../crates/core/src/trickplay.rs) autorise et route au [moteur Node](../crates/node/src/trickplay.rs) : cache applicatif, fingerprint, génération/reprise/quota. Tests unitaires de grille, fingerprint, quota et [Web Trickplay](../apps/web/src/lib/player/trickplay.test.ts) présents, non rejoués ici. |
| Recommandations personnelles | Absentes ; contenus similaires partiels présents | [SimilarWorks](../apps/web/src/lib/catalog/SimilarWorks.svelte) → catalogue autorisé → classement frontend genres/personnes/année, exclusion œuvre courante/doublons. Aucun historique/favoris/notes ni explication personnelle/opt-out intégré à ce classement. `similarItemsProviders` reste préparatoire. Pas de test personnel identifié. |
| Notifications externes | Absentes pour le parcours demandé | Aucun réglage/route/file/adaptateur email, ntfy, Gotify, webhook trouvé. [Tâches système](../crates/core/src/system_tasks.rs) et [file durable](../crates/core/src/task_engine.rs) sont des sources d'événements réutilisables. Les toasts WatchParty ne sont pas ce socle. Aucun test d'envoi revendiqué. |
| Docker AMD64/ARM64 | Trois cibles existantes ; ARM64 non livré par le plan | [Bake](../docker-bake.ci.hcl) fixe `linux/amd64` → [Dockerfile](../Dockerfile) construit standalone/Core/Node, UID 10001, healthchecks, FFmpeg source et outils PostgreSQL. ARM64, exécution QEMU et matériel ARM réel non vérifiés ici. CI distante contrôlée séparément. |
| Multi-Core | Partiel, expérimental ; production non garantie | [Administration Nodes](../apps/web/src/lib/admin/AdminNodes.svelte) → [Nodes Core](../crates/core/src/nodes/mod.rs) → PostgreSQL/tâches durables, admission transactionnelle → canaux et registre Nodes encore en RAM ; `EFFECT_GATE` local au processus. [extension_runs](../crates/core/src/extensions.rs) sans propriétaire/heartbeat ; interruption PG par ancienneté. [Deux Core](../scripts/test-two-cores.ps1) couvre un smoke/session, pas les effets de tâches en vol. [Readiness](MULTI_CORE_READINESS.md) reste ouverte. |

## Défaut connexe confirmé statiquement

Le profil `runtime` de [build-ffmpeg.sh](../scripts/build-ffmpeg.sh) désactive
`swscale`, les filtres et les encodeurs sauf WebVTT. Or le moteur Trickplay exige
`fps,scale,format,tile` et des JPEG. Les images Node/standalone embarquent ce profil.
Cette incompatibilité doit être corrigée au lot images/FFmpeg puis vérifiée en
générant une vraie image avec le binaire embarqué. Ce constat n'est pas une
reproduction Docker et ne justifie pas de réécrire le moteur existant.

## Travail préexistant à préserver

Les deux dépôts étaient sur `develop`, avec des modifications locales. Aether :
workflows CI/publication, lecteur natif et tests, scénario navigateur ASS,
politique de publication et documentation d'audit. Plugins : SDK et lockfile,
compatibilité collection, adaptateurs fournisseurs, scripts de paquetage/tests,
workflow et documentation. Leur présence ne signifie ni commit, ni publication,
ni déploiement. Les SHA et CI distantes sont relevés dans l'audit CI séparé.

## Prochain lot

Vérifier les blocages CI courants, puis réaliser le parcours manuel OpenSubtitles :
configuration protégée → recherche explicite par média autorisé → téléchargement
explicite → dérivé applicatif durable → sélection lecteur → suppression contrôlée.
Documenter séparément tests hors ligne et validation du service avec un compte
autorisé. Aucun accès fournisseur ou envoi externe de notification n'est présumé.

## Lot interface manuelle OpenSubtitles — validation locale

Ajouts : [dialogue](../apps/web/src/lib/catalog/SubtitleDownloads.svelte),
[client HTTP](../apps/web/src/lib/catalog/subtitle-download-api.ts), accès administrateur
via [WorkTools](../apps/web/src/lib/catalog/WorkTools.svelte) sur film et épisode,
et clarification des [réglages bibliothèque](../apps/web/src/lib/admin/AdvancedLibraryOptions.svelte).
La langue, le fournisseur, les filtres forcés/SDH et la pagination sont explicites.
Le quota restant, sa réinitialisation et le prochain délai autorisé sont affichés
lorsqu'ils sont connus du Core. Les tentatives interrompues et les dérivés périmés
ont des messages de suppression/recherche explicites ; aucune reprise automatique.
Téléchargement et suppression nécessitent chacun une action. Les erreurs sont traduites
sans afficher le message brut du fournisseur. Fermeture/annulation interrompt l'attente
HTTP ; une opération déjà acceptée par le serveur peut avoir abouti, ce que l'interface
explique. Le cache des descripteurs est invalidé ; les pistes sont actualisées au prochain
lancement du lecteur. Le moteur de lecture n'a pas été modifié pour ce lot.

Commandes exécutées :

- `npm run check` : 0 erreur, 0 avertissement.
- ESLint ciblé sur les composants et le client modifiés : réussi.
- `npx vitest run src/lib/catalog/subtitle-download-api.test.ts` : 5/5 réussis
  (CSRF, propagation annulation, quota sans retry, erreurs sans fuite, suppression 204,
  honnêteté des correspondances).
- `npm run build` : réussi.
- `node scripts/browser-subtitle-downloads.cjs` : réussi sur Chrome 156.0.8078.80,
  build Svelte de production avec API synthétiques : recherche sans téléchargement
  implicite, pagination, en-tête CSRF, téléchargement, bouton doublon désactivé,
  suppression, actualisation du quota, fermeture Échap, action absente pour le rôle `user`.

Limites : ce scénario navigateur n'est une preuve ni du Core, ni des permissions
backend, ni du service OpenSubtitles, ni d'une lecture/synchronisation réelle.
L'automatisation reste indisponible. Le hash indisponible bloque explicitement le
parcours si la bibliothèque l'exige. Les validations backend, stockage après redémarrage
et fournisseur doivent compléter ce lot avant de classer le parcours opérationnel.

## Lot Core OpenSubtitles — compléments de validation du 3 novembre

Le parcours manuel est **partiel, validé localement** : fournisseur optionnel,
API administrateur, données persistantes et raccordement des pistes existent.
Le [contrat complet](OPENSUBTITLES.md) décrit permissions, limites et retour arrière.
L'accès réel au service et la synchronisation d'un sous-titre téléchargé réel
restent non vérifiés. Aucun paquet n'a été signé ou publié par ces tests.

- `cargo test --locked -p media-poc --lib signed_wasm_http_download_survives_reopen_and_requires_admin_csrf -- --test-threads=1` : **1/1 réussi**, 13,21 s hors compilation. Installation HTTP d'un Wasm synthétique signé, activation/grant, recherche puis acquisition via vrai worker, récupération CDN remplacée uniquement dans le code de test, doublon refusé, lecture protégée, persistance après réouverture SQLite, invalidation et suppression admin/CSRF.
- `pwsh -NoProfile -File scripts/test-postgres.ps1 -OnlySubtitles` : **1/1 réussi**, 1,75 s hors compilation. PostgreSQL 18 réel dans un conteneur jetable tmpfs/boucle locale, deux pools indépendants, initialisation idempotente, réservation concurrente unique, quota/dérivé après réouverture, lecture GET/HEAD, refus d'un autre asset et invalidation après changement du média. Conteneur supprimé par le runner.

La revue a corrigé l'attente obsolète `200` du test d'activation (contrat existant
`204`) et la mémorisation d'un refus explicite de quota fournisseur sans échéance
(repli prudent 24 heures). Aucun délai de lecture n'a été augmenté. Les durées
sont celles de runs uniques locaux, sans comparaison avant/après ni p95 revendiqué.
Le test PostgreSQL ne prouve pas le multi-Core, une panne de Core en vol ou le NAS.
