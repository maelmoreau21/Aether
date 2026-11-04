# Lecteur : fiabilité et mesures locales du 29 octobre 2026

Les corrections de ce jalon portent sur l'annulation des attentes, les erreurs
audio périmées et la mutualisation des préparations. Les mesures ci-dessous
ne démontrent pas une accélération générale ni l'atteinte des objectifs extrêmes.

## Changements et vérifications

- Audio : annulation des attentes de disponibilité à pause/fermeture,
  attente MSE bornée à dix secondes et nettoyée à fermeture, arrêt du scheduler
  après erreur terminale, ignorance des erreurs provenant d'un seek dépassé.
  Près d'une frontière (dernière demi-seconde), la reprise précharge aussi la
  fenêtre suivante pour permettre au décodeur de terminer le seek ; ailleurs,
  démarrage sur la seule fenêtre courante, deux fenêtres maximum.
- Vidéo : fermeture pendant attente de progression et supersession avant
  premier fragment ne laissent plus leur promesse pendante. Après watchdog,
  libération de l'audio et invalidation de la source permettent une vraie relance.
- Node : mutualisation des composants froids vidéo/audio complet/sous-titres,
  avec worker unique et 64 participants maximum, écriture finale vidée avant
  publication du fichier. Aucun réencodage ni changement d'API/autorisation.

Régressions reproduites avant correction : deux attentes vidéo expiraient
dans les tests au lieu de s'annuler ; le watchdog ne détruisait pas l'audio ;
huit demandes vidéo identiques donnaient un succès et sept refus `busy`.
Après correction, les huit demandes obtiennent le même fichier sous lease,
également vérifié pour audio complet et sous-titre. Les tests audio vérifient
l'arrêt du retry périodique et la propagation des seules erreurs encore actuelles.

Commandes réellement exécutées depuis la racine :

```powershell
npm test --prefix apps/web
npm test --prefix apps/web -- src/lib/player/segmented-audio.test.ts
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
cargo fmt --all -- --check
cargo clippy --locked -p media-node --all-targets -- -D warnings
cargo test --locked -p media-node --lib packaging::tests -- --test-threads=1
cargo test --locked -p media-node --test preparation_admission -- --nocapture
cargo test --locked -p media-node --lib cache_dirs::tests -- --test-threads=1
cargo test --locked -p media-node --lib stream::tests -- --test-threads=1
cargo build --locked -p media-poc -p media-node --bins
cargo test --locked -p media-poc --test node_enrollment_and_relay -- --test-threads=1
```

Web : suite finale complète de 187 tests réussis, dont la reprise à
29,844173 secondes dans une chronologie de 7 200 secondes. Le test de cette
frontière échouait avant (fenêtre 2 seule), puis passe (fenêtres 2 et 3).
Check final : zéro erreur, un avertissement préexistant `CatalogGrid.svelte:90`.
Lint et build statique finaux réussis. Node : packaging 8 réussis/2 ignorés ;
admission 1 réussi ; cache 2 réussis/1 helper ignoré invoqué par le test de crash ;
stream/video_stream 9 réussis. Ces derniers couvrent HEAD/206/416, les leases,
l'annulation du processus enfant et les admissions 4/8/16. Clippy Node strict,
fmt et build debug Core/Node réussis. L'intégration Core/Node via TCP local
`node_enrollment_and_relay` réussit (1 test, 12,95 s) : appairage, autorisations,
rotation/révocation, relais sortant, HEAD/206/416 et vidéo progressive.
Ces commandes ont été exécutées sans `-j 1` ; pour les reproduire avec un pic
mémoire moindre sous Windows, ajouter `-j 1` aux commandes Cargo de compilation
et de test. L'intégration a déclenché plusieurs liens MSVC simultanés.
Pas de suite Rust générale revendiquée.

## Audit de l'historique

Les constats initiaux du 21 octobre dans `PLAYER_PLAYBACK_AUDIT.md` ne décrivent
plus tous le code actuel : commutation transactionnelle avec ancienne piste
conservée, sélection explicite de langue, surveillance du buffering audio et
limites du cache avaient déjà été corrigées. Le 22 octobre, le parcours MP4
avait supprimé la copie vidéo entière en utilisant l'original et des fenêtres
audio. Le 23 octobre, reprise directement au segment utile, maintien de la
vidéo pendant la préparation audio et ouverture du cache hors verrou étaient
déjà livrés. Le 24 octobre, Opus MP4 natif sans FFmpeg, vidéo progressive MSE,
annulation des seeks remplacés et diagnostics des erreurs 503 existaient.

`STREAMING_SYSTEM_BENCHMARK.md` conserve des échecs à quatre sessions dans
la matrice debug historique. Le smoke Release ultérieur décrit dans
`STREAMING_MISSION_3_2.md`/`MEMORY.md` avait réussi à quatre sessions,
avec des latences très supérieures aux objectifs. Aucun de ces résultats
historiques n'est remplacé par les petits bancs synthétiques de ce jalon.

## Méthode et provenance

`scripts/browser-streaming-performance.cjs` utilise les modules TypeScript de
production dans un navigateur réel headless, avec un serveur HTTP Range de
fixtures. Ce serveur prépare des fenêtres Opus par copie de paquets FFmpeg.
Il ne remplace pas un essai Core/Media Node Rust et ne mesure pas leurs ressources.
Les deux stratégies sont l'original multi-audio choisi par le navigateur et
l'original avec audio explicitement sélectionné par segments.

Les modules sont désormais figés avant le lancement du navigateur ;
`MEDIA_PERFORMANCE_PLAYER_REF=HEAD` charge tous les modules depuis Git et les
SHA-256 sont conservés dans chaque JSON. L'ancien sélecteur de baseline
remplaçant seulement `native.ts` reste disponible pour les expériences historiques.
Les dépendances du repli progressif sont également injectées, mais ces essais
réussissant avec l'original ne prouvent pas à eux seuls ce repli.

La première frame est un callback `requestVideoFrameCallback`, le PCM est
observé par FFT WebAudio avec un polling nominal de 8 ms. Ce ne sont ni les
photons de l'écran ni le début du son physique. L'oracle PCM de ce banc exige
également un alignement des horloges à 150 ms. Le polling et le graphe de mesure
limitent sa précision, notamment face à l'objectif de commutation de 50 ms.

Le premier essai démarre dans un contexte navigateur neuf et avec un cache de
préparation vide. Le cache disque Windows n'est pas purgé. Les suivants
réutilisent explicitement contexte et préparations. Le hash de chaque original
est calculé après les mesures. La machine est partagée avec des compilations
et certaines campagnes se chevauchent : les écarts ne sont pas attribuables
uniquement au patch. Les candidats intermédiaires et finaux sont distingués par
leurs hashes ; les corrections finales sont couvertes par la matrice de charge
et les tests ciblés, pas rétroactivement par tous les timings précédents.

Chrome 156.0.8078.48, EdgeCore 156.0.4314.50 et Firefox 159.0 ont réellement
été lancés. Firefox utilise le geckodriver déjà présent via l'adaptateur du dépôt,
avec un nouveau processus/profil par stratégie ; aucune installation de driver
n'a été nécessaire. EdgeCore est un runtime Edge, sans preuve du navigateur
Edge installé pour un usage interactif normal.

## Comparaisons exploratoires

Rapports bruts locaux : `output/playwright/player-reliability/*/report.json`.
Toutes les lignes ci-dessous concernent la stratégie audio segmenté. Valeurs
p95 empiriques avant → après, en ms ; les effectifs réduits ne constituent
pas un engagement de service. Tous les essais de ces cellules ont réussi.

| Source/navigateur | Frame chaude | Nouveau PCM après switch | Frame après seek | PCM après seek |
| --- | ---: | ---: | ---: | ---: |
| MKV 12 s / Chrome | 39,0 → 29,6 (n=4) | 123,2 → 107,4 (n=10) | 34,0 → 36,8 (n=5) | 35,6 → 46,2 (n=5) |
| MP4 12 s / EdgeCore | 38,3 → 29,0 (n=4) | 136,6 → 142,8 (n=10) | 28,4 → 28,9 (n=5) | 41,6 → 43,6 (n=5) |
| MP4 12 s / Firefox | 29 → 32 (n=1) | 152 → 120 (n=4) | 18 → 26 (n=2) | 22 → 68 (n=2) |
| MP4 2 h / Chrome | 180,4 → 180,3 (n=3) | 271,5 → 181,3 (n=8) | 62,3 → 74,2 (n=4) | 253,0 → 240,7 (n=4) |

Les résultats incluent les régressions numériques observées : aucun gain
général de vitesse n'est établi. L'objectif de frame chaude <100 ms échoue
sur le MP4 de deux heures et celui du switch <50 ms n'est pas atteint au p95.
Les premières ouvertures Firefox segmentées ont livré une frame à 333 puis
367 ms ; aucun p95 à froid ne peut être estimé à partir d'un seul essai.

La fixture de deux heures a été créée sans réencodage à partir de la petite
fixture légale : AV1 10 bits, Opus FR 5.1 et EN stéréo, durée 7 200,006 s,
457 240 160 octets. Les seeks 2, 3 600, 7 180 puis 2 s ont réellement décodé
des frames et du PCM. Il s'agit d'extraits d'un conteneur de deux heures,
pas d'une lecture continue de deux heures ni d'un film personnel à haut débit.

```powershell
ffmpeg -v error -nostdin -n -stream_loop 600 -i tests/fixtures/generated/representative/representative.mp4 -map 0:v -map 0:a -c copy -t 7200 target/player-reliability-fixtures/two-hours.mp4
$env:MEDIA_PERFORMANCE_PLAYER_REF='HEAD' # retirer pour le candidat du checkout
$env:MEDIA_PERFORMANCE_ROUNDS='4'
$env:MEDIA_PERFORMANCE_SEEK_TARGETS='2,3600,7180,2'
$env:MEDIA_TEST_FIXTURE='target/player-reliability-fixtures/two-hours.mp4'
$env:MEDIA_REPORT_DIR='output/playwright/player-reliability/baseline-two-hours'
node scripts/browser-streaming-performance.cjs
```

La première baseline Chrome MP4, exécutée avant l'ajout du gel complet,
est conservée mais exclue du tableau. Le candidat Chrome MP4 a également
réussi cinq ouvertures, dix switches et cinq seeks par stratégie applicable.

Une paire supplémentaire a ensuite été exécutée **séquentiellement**, après
la fin des compilations et de la matrice de charge de cette équipe, sur la
même fixture de deux heures et les mêmes positions. Les modules HEAD puis
les modules du checkout sont figés avant leur navigateur respectif. Cette paire
précède le correctif supplémentaire de reprise près d'une frontière audio,
identifié ensuite dans le vrai parcours système ci-dessous ; les hashes
archivés identifient précisément cette variante.
Les rapports sont `controlled-baseline` et `controlled-final`, sans erreur
sur les deux stratégies. Aucun cache disque OS n'a été purgé entre les runs.

| Indicateur segmenté | HEAD médiane / p95 (ms) | Final médiane / p95 (ms) | n |
| --- | ---: | ---: | ---: |
| Première frame chaude | 147,7 / 175,6 | 176,4 / 193,3 | 3 |
| Nouveau PCM après switch | 47,3 / 240,1 | 44,8 / 207,7 | 8 |
| Frame après seek | 66,2 / 89,2 | 58,2 / 107,1 | 4 |
| PCM après seek | 151,3 / 303,7 | 129,2 / 319,4 | 4 |

Première frame à contexte neuf et préparation vide : 160,3 puis 176,2 ms,
une seule observation chacune. Cette comparaison dans les mêmes conditions
ne démontre toujours aucun gain général : notamment la frame chaude et les
p95 de seek augmentent. Les changements de ce jalon visent les courses et
attentes invalides, pas une réduction démontrée du coût de décodage AV1.

## Scénarios et limites

`browser-dual-opus.cjs` avec `MEDIA_TEST_SEGMENTED=1` a réussi sur les deux
pistes Opus 5.1 de la fixture 1080p : cinq commutations sans nouveau chargement
vidéo, conservation du plein écran, seeks en pause, franchissement de fenêtre,
échec réel du candidat avec ancienne piste poursuivie, puis fermeture pendant
une requête audio retardée. Cette fermeture rejette le chargement en 7,1 ms
dans cet essai et laisse zéro élément audio. Le rapport est `candidate-51`.
Il ne s'agit pas d'une preuve de sortie physique 5.1/7.1.

La matrice Chrome finale a ouvert 1, 4, 8 puis 16 lecteurs audio segmenté
concurrents sur la petite fixture MP4, chaque lot dans de nouveaux contextes,
avec fenêtres serveur déjà chaudes. Tous ont produit frames et PCM sans erreur
signalée par le lecteur. Après environ une seconde d'observation :

| Sessions | Première frame min–max (ms) | Premier PCM min–max (ms) | Frames perdues rapportées, somme du lot |
| --- | ---: | ---: | ---: |
| 1 | 42,7 | 120,6 | 0 |
| 4 | 394,3–517,0 | 823,6–842,3 | 19 |
| 8 | 100,8–241,5 | 386,8–641,3 | 0 |
| 16 | 249,6–845,5 | 1 514,1–2 135,8 | 0 |

Un lot par taille, machine partagée avec compilations : ces chiffres ne
caractérisent pas la capacité serveur. Le lot à quatre rapporte bien des
frames perdues et ne doit pas être annoncé comme une lecture parfaite.
Le RSS archivé par ce banc est celui du processus HTTP JavaScript, pas
celui des navigateurs, de Rust ou d'un NAS. Aucun profil global des allocations
n'a été exécuté.

Les demandes de changement audio concurrentes restent refusées par la garde
`NativePlayer.busy` et l'interface sérialise la sélection. Le jalon ne livre
donc pas une politique générale « dernier changement de piste gagnant » ni
une refonte ultime achevée. Son gain principal est la stabilité des attentes
et des erreurs ; un démarrage ou un changement instantané supplémentaire
par rapport aux optimisations déjà présentes n'est pas démontré.

## Vérification avec les véritables Core et Media Node

Le banc `streaming-system-benchmark.cjs` a utilisé les binaires debug actuels,
les assets Svelte actuels et un manifeste SHA-256 immuable, depuis
`target/player-reliability-artifacts/current-debug`. Core/SQLite et Node sont
deux vrais processus sur des ports loopback distincts. Les droits, le scan et
les requêtes média sont réels. Ce n'est toujours pas un NAS ni deux machines.

Un premier lot de deux heures a révélé deux problèmes distincts. Le scénario
de seek du banc envoyait seulement `input`, alors que le contrôle tactile
actuel attend `change` pour commettre le seek : le banc envoyait donc une
intention sans effectuer le seek. Le script émet désormais les deux événements.
Le rapport partiel `system-current` reste marqué interrompu/échoué ; aucune
conclusion sur quatre sessions ou les pannes n'en est tirée.

Un contrôle avec le MP4 synthétique existant de 3 500 s a ensuite réussi :
deux ouvertures froide/chaude, quatre changements audio et huit seeks réels
2/1 800/3 480/2 s. Frame 942,7 puis 335,9 ms ; PCM brut 947,6 puis 355,4 ms.
Node : pic RSS observé 23,9 puis 24,1 Mio, CPU total du lot 515,6 puis
328,1 ms, zéro descendant FFmpeg et zéro fichier de préparation créé.
Ce sont des ressources du véritable Node, collectées par Job Object Windows,
distinctes des mesures JavaScript précédentes. CPU et RSS concernent le lot
avec ses actions, pas seulement l'ouverture ; les I/O sont logiques.
Rapport complet : `system-control-3500`, artefacts inchangés.

Le second problème est un vrai défaut reproductible à la reprise près d'une
frontière audio. Avant correction, `system-boundary-before` réussit l'ouverture
et le seek à 29,844 s, puis échoue lors de la reprise à 29,840897 s.
L'historique diagnostic, toutes les 500 ms, montre l'audio bloqué en
`seeking=true`, `readyState=1`, tampon `[20,30]` pendant dix secondes.
La fenêtre `opus-2-chunk-2` répond HTTP 200 mais la suivante n'est jamais
demandée : le décodeur attend des données permettant de terminer son seek,
tandis que le chargement attend ce même seek avant de précharger la suite.
Ce diagnostic n'est pas une supposition fondée sur la seule erreur finale.

Le correctif autorise la prochaine fenêtre pendant le chargement initial
seulement dans la dernière demi-seconde de la fenêtre courante. Les autres
démarrages continuent de demander uniquement leur fenêtre utile. Le test
déterministe correspondant échouait avant et passe après ; sa validation
réelle avec le même scénario a ensuite réussi dans `system-boundary-after` :
la reprise proche de 29,84 s livre une frame en 284,5 ms et du PCM en 302,2 ms,
au lieu de l'expiration après dix secondes. Les fenêtres 2 **et** 3 sont bien
demandées, puis l'audio passe à `readyState=4`, `seeking=false` et tampon
`[20,40]`. Les deux ouvertures et leurs actions réussissent. Un seul essai
avant/après, aucune garantie de percentile ; c'est une preuve de suppression
du blocage sur cette reproduction réelle. Les manifestes identifient les
mêmes binaires Rust et les deux builds Web distincts.

À la demande de l'utilisateur d'arrêter pour préserver son quota, les
campagnes supplémentaires de deux heures à quatre sessions et de pannes
ont été interrompues et leurs processus de test arrêtés. Les répertoires
`system-two-hours-final`, `system-crash-final` et `system-network-final`,
s'ils existent, ne constituent pas une validation complète. Aucune nouvelle
campagne de mesure n'a été lancée après cette demande.

Restent à établir : endurance de plusieurs heures, fichiers de dizaines de Go,
NAS et Node sur machine physique distante, restitution multicanal physique,
réseau réel lent/instable et percentiles robustes avec répétitions indépendantes.
Les corrections de fiabilité sont étayées séparément par des régressions
déterministes avant/après ; elles ne transforment pas ces limites en garanties.
