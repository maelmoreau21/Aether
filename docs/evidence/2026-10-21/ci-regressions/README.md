# Corrections des tests GitHub — develop

Périmètre final : tests de version, environnement FFmpeg 9.0.2 de la CI et cohérence des contrôles Docker. Les essais de mise à jour générale des dépendances ont été retirés à la demande de l’utilisateur. Aucun push, publication ou remplacement de service déployé.

## Changements

- Le test partagé ne suppose plus que tout build est exécuté sur develop. La politique réellement utilisée par build.rs est testée avec des branches explicites, une version de base arbitraire, main, refs/heads/develop, origin/develop, HEAD et les noms contenant seulement le mot develop. Les overrides restent autorisés ; GITHUB_HEAD_REF couvre les PR et les variables vides sont ignorées.
- CI Rust et navigateur : FFmpeg/ffprobe 9.0.2 depuis l’archive officielle, SHA-256 vérifié ; cache par système/distribution/architecture et script. Le profil de génération fournit libaom/Opus et les encodeurs H.264/HEVC utilisés par les tests existants. Le cache des fixtures anciennes est invalidé.
- Docker : même source 9.0.2, profil sans encodeur vidéo ; les contrôles attendent les deux outils au lieu d’interdire FFmpeg alors que le Dockerfile l’embarquait déjà. La règle Docker qui excluait également les sources TypeScript .ts est corrigée.
- Le diagnostic du scénario navigateur conserve les requêtes de préparation et leurs codes HTTP en cas d’échec. Aucun contrôle de décodage, langue, seek, pause ou redémarrage n’est supprimé.

## Vérifications

- Politique du build : sept simulations (main, develop, PR develop, ref complète, branche contenant develop, HEAD et override explicite) réussies.
- 11 tests media-contracts réussis, dont règles de version main/develop.
- Build de FFmpeg/ffprobe 9.0.2 sous Debian Linux réussi ; versions et liaison libc/libm vérifiées. Aucun encodeur vidéo dans le profil runtime.
- Test Linux de copie AV1/Opus réussi ; vérifie les paquets vidéo/audio et le cache avec contrôle de la source.
- Tests Linux des leases de cache : 2 réussis, helper ignoré.
- actionlint 1.7.12 : workflows valides après les modifications ciblées.

Les rapports scanner et leurs captures sont séparés en Windows et Linux. Les validations locales ne constituent pas un nouveau run GitHub. Le symptôme de préparation du runner doit être évalué avec le parcours navigateur complet, pas avec le seul succès de ffprobe ou d’une commande vidéo.
