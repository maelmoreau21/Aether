# Mission fonctionnalités — constat initial du 3 novembre 2026

Audit statique ciblé : les sources indiquent les parcours présents, sans prouver leur
fonctionnement courant. Aucun test, déploiement ou appel fournisseur n'a été effectué
pour établir cette matrice. Les validations ultérieures doivent être consignées par lot.

| Domaine | État initial | Interface → API/validation → données → exécution → tests existants |
|---|---|---|
| OpenSubtitles | Fournisseur absent ; options déclaratives | [Options](../apps/web/src/lib/admin/AdvancedLibraryOptions.svelte) → [options validées/persistées](../crates/core/src/library_options.rs), sans recherche/téléchargement. Aucun package dans [workspace plugins](../../Aether-Plugins/Cargo.toml). Le pipeline des extensions et le lecteur de sous-titres existants sont à réutiliser. Validation du service réel non effectuée. |
| TVDB | Fournisseur absent ; identités déjà prises en charge | [Identification](../apps/web/src/lib/catalog/IdentifyModal.svelte), badges → [adaptateur](../crates/core/src/extension_adapters.rs), [normalisation](../crates/core/src/consolidation.rs) → identités catalogue/NFO → [scanner](../crates/node/src/scanner.rs), [NFO](../crates/node/src/nfo.rs). Tests d'identité dans [series_flow](../crates/core/tests/series_flow.rs) et [consolidation](../crates/core/tests/global_catalogue_consolidation.rs), pas de preuve API TVDB. |
| Images de chapitres | Déclaratives seulement ; chapitres et Trickplay distincts existants | Options « moteur à venir » → booléens bibliothèque persistés ; pas de route/tâche/image dédiée trouvée. [Probe](../crates/node/src/probe.rs) extrait déjà les chapitres ; [lecteur](../apps/web/src/lib/player/PlayerControls.svelte) affiche titres/repères/sauts. [Trickplay Core](../crates/core/src/trickplay.rs) autorise et route au [moteur Node](../crates/node/src/trickplay.rs) : cache applicatif, fingerprint, génération/reprise/quota. Tests unitaires de grille, fingerprint, quota et [Web Trickplay](../apps/web/src/lib/player/trickplay.test.ts) présents, non rejoués ici. |
| Recommandations personnelles | Absentes ; contenus similaires partiels présents | [SimilarWorks](../apps/web/src/lib/catalog/SimilarWorks.svelte) → catalogue autorisé → classement frontend genres/personnes/année, exclusion œuvre courante/doublons. Aucun historique/favoris/notes ni explication personnelle/opt-out intégré à ce classement. `similarItemsProviders` reste préparatoire. Pas de test personnel identifié. |
| Notifications externes | Absentes pour le parcours demandé | Aucun réglage/route/file/adaptateur email, ntfy, Gotify, webhook trouvé. [Tâches système](../crates/core/src/system_tasks.rs) et [file durable](../crates/core/src/task_engine.rs) sont des sources d'événements réutilisables. Les toasts WatchParty ne sont pas ce socle. Aucun test d'envoi revendiqué. |
| Docker AMD64/ARM64 | Trois cibles existantes ; ARM64 non livré par le plan | [Bake](../docker-bake.ci.hcl) fixe `linux/amd64` → [Dockerfile](../Dockerfile) construit standalone/Core/Node, UID 10001, healthchecks, FFmpeg source et outils PostgreSQL. ARM64, exécution QEMU et matériel ARM réel non vérifiés ici. CI distante contrôlée séparément. |
| Multi-Core | Partiel, expérimental ; production non garantie | [Administration Nodes](../apps/web/src/lib/admin/AdminNodes.svelte) → [Nodes Core](../crates/core/src/nodes.rs) → PostgreSQL/tâches durables, admission transactionnelle → canaux et registre Nodes encore en RAM ; `EFFECT_GATE` local au processus. [extension_runs](../crates/core/src/extensions.rs) sans propriétaire/heartbeat ; interruption PG par ancienneté. [Deux Core](../scripts/test-two-cores.ps1) couvre un smoke/session, pas les effets de tâches en vol. [Readiness](MULTI_CORE_READINESS.md) reste ouverte. |

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
