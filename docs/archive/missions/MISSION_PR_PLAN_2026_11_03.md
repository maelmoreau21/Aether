# Lots de revue — mission du 3 novembre 2026

Les commits utilisateur `5249506` / `525b61f` et `7e5b13c` / `d07160d`
contiennent plusieurs lots préparés pendant la mission. Ils ne sont pas
réécrits. Le worktree contient également une refonte visuelle concurrente :
ses changements ne doivent pas être ajoutés aux lots ci-dessous par un ajout
global des fichiers Web. Aucun push ni PR distante n'a été créé ici.

Les descriptions suivantes préparent la revue ; les fichiers partagés tels que
`lib.rs`, `database.rs`, les manifestes et les workflows demandent une sélection
des seuls hunks du domaine. Les contrats et résultats détaillés restent dans
les rapports liés, avec les échecs intermédiaires et limites conservés.

| Ordre et titre proposé | Changement cohérent à revoir | Validation et retour arrière |
|---|---|---|
| 0 — `fix(ci): align collection SDK and verify ASS canvas` | Réparations préexistantes SDK/collections, détachement natif, scénario ASS et contexte Bake/SHA, identifiées séparément | [Baseline CI](MISSION_CI_BASELINE_2026_11_03.md), [réparations](CI_P0_REPAIRS_2026_11_03.md). Ne pas présenter les CI distantes comme vertes pour les commits locaux. |
| 1 — `feat(subtitles): add explicit OpenSubtitles acquisition` | Crate fournisseur Plugins + ABI JSON `subtitles.*` + permission signée ; Core routes, schéma additif, dérivés et descripteur ; dialogue fiche et option bibliothèque | [Contrat](OPENSUBTITLES.md), [tests Core](MISSION_CORE_VALIDATION_2026_11_03.md), [Web](MISSION_WEB_VALIDATION_2026_11_03.md). Désactiver fournisseur/téléchargements ; conserver ou supprimer explicitement les seuls dérivés. |
| 2 — `feat(metadata): add optional TheTVDB v4 provider` | Nouveau crate et tests guest, manifestes/release/CI Plugins. Réutilise le pipeline Core et les identités existants | [Rapport TVDB](../../Aether-Plugins/docs/TVDB_2026_11_03.md). Désactiver l'extension ; aucune nouvelle migration catalogue. Compte/licence réelle non validé. |
| 3 — `feat(chapters): generate previews on the owning Media Node` | Moteur Node et cache partagés, routes signées directes/inverses, droits et références Core, contrôle à la demande, capacités FFmpeg | [Chapitres](CHAPTER_IMAGES.md). Tests Node/FFmpeg réel et droits ciblés ; transport distant/NAS encore à éprouver. Désactiver l'option et conserver le cache évictable ; aucune modification média. |
| 4 — `feat(recommendations): rank the permitted local catalogue per account` | Calcul explicable, préférences additives, API privée sans cache, composants accueil/réglages et tests d'isolation | [Contrat](RECOMMENDATIONS.md), [Core](MISSION_CORE_VALIDATION_2026_11_03.md). Opt-out immédiat au rafraîchissement ; aucune copie durable du profil calculé à migrer. |
| 5 — `feat(notifications): persist events and deliver through four adapters` | Tables/file, sources terminales des tâches, worker distinct, email/ntfy/Gotify/webhook, préférences et configuration masquée, récepteurs locaux de test | [Notifications](NOTIFICATIONS.md). Désactiver les canaux et arrêter le worker avant retour de version ; conserver historique/file. Livraison au moins une fois, sources non raccordées indisponibles. |
| 5a — `feat(notifications): notify opted-in accounts of permitted new content` | Souscription additive, faits d'ajout catalogue, déduplication et payload minimal ; contrôle parental et droits relus avant envoi | [Validation du 4 novembre](MISSION_CORE_VALIDATION_2026_11_03.md) : six tests locaux, PostgreSQL et consentement Chrome. Désactiver `content` ; aucune donnée média modifiée. |
| 6 — `build(docker): validate both architectures before publishing manifests` | Trois images sur deux architectures, caches séparés, smoke local/natif CI, digests et gates de publication | [Rapport Docker](DOCKER_MULTIARCH_2026_11_03.md). Conserver les images précédemment déployées ; aucune publication locale. Émulation et matériel physique distingués. |
| 7 — `fix(extensions): fence manual run history across Core processes` | Ownership/heartbeat/lease additifs, finalisation conditionnelle et récupération de l'historique ; test de panne avec deux vrais Core | [Readiness](MULTI_CORE_READINESS.md), [preuve Core](MISSION_CORE_VALIDATION_2026_11_03.md). Retour de version avec un Core unique, toutes instances arrêtées ; aucune promesse de fencing de tous les effets. |

Le mode multi-Core reste expérimental après ces lots. La prochaine priorité de
ce domaine est la propriété durable et le routage des connexions Node, puis le
fencing transactionnel des effets et les fichiers/configurations partagés.
Les comptes fournisseurs, les essais NAS et le matériel ARM nécessitent leurs
validations distinctes ; ils ne sont pas clos par les fixtures locales.
