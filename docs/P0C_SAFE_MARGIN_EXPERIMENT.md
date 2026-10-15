# P0C — marges audio Shaka 5.2.13

## Protocole fixé avant la matrice finale

Même Shaka5.2.13, bundle et MPD/AV1 Main10/Opus FR5.1 et ENstéréo que le
[jalon précédent](P0C_SHAKA_EXPERIMENT.md). Aucun muxeur, réencodage ou changement
des originaux. MSE conservé. Seul argument varié :
`selectAudioTrack(track, safeMargin)`, marges 0 / 0,5 / 1 / 2 / 4 secondes.
La valeur0 reste le comportement normal du banc ; aucune marge retenue par défaut
sur la base d'un plan. Les valeurs hors de cette liste sont rejetées explicitement.

La [source Shaka versionnée](https://github.com/shaka-project/shaka-player/blob/v5.2.13/lib/media/streaming_engine.js)
montre deux branches : marge0 efface/flush tout le buffer audio ; marge positive
supprime seulement le futur après currentTime+marge. Les sept fragments du clip
ont des frontières réelles, majoritairement proches de2s ; conserver4s n'implique
pas un délai de changement exactement égal à4s, notamment quand des fragments
entiers chevauchent la partie conservée. Ce comportement doit être mesuré.

La vidéo et l'audio ont12s. Pour éviter de dépasser la fin en cumulant deux
marges de4s, **un seul changement mesuré par présentation neuve**. Rechargement
entre essais indépendants, jamais pendant la mesure du changement. Tous les
essais partent du même lecteur FR ; pour EN→FR, un préchauffage FR→EN à marge0
est exclu des statistiques. Départ vers1,25s, stabilisation500ms en lecture,
ou seek supplémentaire vers3,25s pour la catégorie après-seek.

Matrice visée : Chrome, Edge, Firefox Windows headless, profils par défaut,
20 changements en lecture par marge/navigateur (10 dans chaque sens), quatre
en pause et quatre après seek (deux par sens dans chaque catégorie). Cela donne
420 changements mesurés. Une session navigateur neuve par tour de10 essais ;
ordre des marges tournant, sens alterné à chaque tour. Pas de tests concurrents.
Statistiques par navigateur, marge, mode et sens ; échecs conservés avec leurs
mesures partielles. P95 par rang supérieur, descriptif sur cet effectif local.

Mesure commune issue du banc précédent : FFT8192, six canaux séparés, RMS
sur les derniers1024 échantillons, trois observations correctes consécutives.
Signal FR440/550/660/80/770/880Hz, EN990/1100Hz et quatre voies silencieuses.
Polling cible10ms et période réelle conservée. État pause observé100ms puis
reprise : demande→signal et reprise→signal séparés. Les silences de pause
volontaire ne sont pas comparés aux silences en lecture.

L'observation continue500ms **après** confirmation du nouveau signal, sur toutes
les marges, y compris0. Silence total et plus longue séquence échantillonnée,
gaps callbacks vidéo, frames/PTS en progression, événements waiting/seeking,
différences des compteurs `getVideoPlaybackQuality()` dropped/total sont
conservés. Les gaps callbacks et sauts du compteur presentedFrames ne sont
pas assimilés à des frames perdues : seul le compteur du navigateur est ainsi
nommé, sans prétendre compter toutes les répétitions d'image sur un écran.

Buffer audio/vidéo avant/après conservé via getBufferedInfo ; aucune modification
de bufferingGoal/rebufferingGoal/bufferBehind. Pas de sampler CPU/RSS concurrent
dans cette matrice pour éviter le coût CIM constaté précédemment. Les résultats
ne sont comparés qu'à la nouvelle marge0, avec le même protocole et sans sampler.

Après chaque changement : flash vidéo/amplitude audio à9s, même tolérance
préétablie±200ms. L'horloge audio est video.currentTime lors du seuil RMS,
pas un horodatage intrinsèque de sortie PCM. Cela vérifie une cohérence grossière,
pas une synchronisation physique précise ni la dérive d'un film long.

Enfin seek arrière vers0,5s, observation400ms et comparaison FFT de la langue
réellement décodée avec celle sélectionnée. Une API annonçant EN avec un signal
FR est une **régression fonctionnelle**, même si la commutation initiale aboutit.
Elle est comptée séparément des erreurs d'exécution et ne doit pas être masquée
par une moyenne de latence ou par un statut de piste.

## Pilote exclu des statistiques finales

Douze essais Chrome, marges0/4, dans les deux sens et trois modes : changements
et marqueurs observés, sans erreur. Six seeks arrière rejouent l'ancienne langue
avec marge4, contre zéro avec marge0. Ce pilote a validé la faisabilité sur12s ;
il n'est pas mélangé aux répétitions finales. Preuves conservées séparément.

## Reproduction

Le serveur Rust doit être actif avec les fixtures DASH et le bundle déjà préparés.

```powershell
$env:MEDIA_BROWSER_CHANNEL='chrome'
Remove-Item Env:MEDIA_BROWSER_EXECUTABLE -ErrorAction SilentlyContinue
$env:MEDIA_REPORT_DIR='artifacts/safe-margin-final/chrome'
node scripts/browser-safe-margin.cjs
```

Rejouer avec channel=msedge/firefox et leur chemin MEDIA_BROWSER_EXECUTABLE
documenté dans P0C_MSE_EXPERIMENT.md ; un dossier neuf est obligatoire.
Les variables MEDIA_SAFE_ACTIVE_ROUNDS, MEDIA_SAFE_OTHER_ROUNDS et
MEDIA_SAFE_MARGINS permettent un pilote explicite ; valeurs finales par défaut
10/2 et 0,0.5,1,2,4. Ne pas confondre pilote et matrice complète.

```powershell
node scripts/summarize-safe-margin.cjs artifacts/safe-margin-final
node scripts/safe-margin-stats.test.cjs
```

Les JSONL conservent tous les essais, leurs marqueurs, buffers, compteurs vidéo,
erreurs et empreintes avant/après. Les ressources et timings de préparation
du jalon précédent ne sont pas remesurés ni extrapolés.

## Résultats finaux

Matrice achevée sur Windows x64 10.0.26300 : Chrome155.0.8059.40,
Edge155.0.4283.45, Firefox155.0.1 ; Playwright1.62.1/geckodriver0.37.1.
**424 essais, 420 commutations et marqueurs mesurés**, avec quatre échecs
de préparation conservés. Les quatre points manquants ont été complétés dans
des présentations indépendantes, fichiers supplement séparés ; aucun effacement
ni reclassification des échecs d'origine. Cela donne20 mesures de lecture
et4 par autre catégorie pour chaque couple navigateur/marge.

**Aucune marge testée ne satisfait tous les critères fonctionnels.**
Les336 probes arrière avec marge positive rejouent tous l'ancienne langue,
malgré une piste sélectionnée différente. Aucun cas seulement silencieux ou
inclassable dans ces336 observations. Les84 probes à marge0 correspondent
à la langue sélectionnée. L'observation se limite aux400ms prévues, sans
garantie sur une session longue. Les420 snapshots après500ms de lecture
confirment le signal demandé avant ce seek.

[Résumé complet et par sens](evidence/2026-10-15/safe-margin/summary.json),
[signaux après observation](evidence/2026-10-15/safe-margin/post-observation-signals.json),
[preuves brutes et régressions](evidence/2026-10-15/safe-margin/).

Dans les JSONL, passed désigne l’aboutissement de la commutation et du marqueur,
avec invariants de source/pause. Le probe arrière a son propre statut :
backwardSeek.matchesSelectedLanguage. Un passed accompagné de false à ce probe
ne constitue donc pas une validation fonctionnelle globale. Les résumés
conservent les nombres de régressions et les erreurs ; une moyenne ne les masque pas.

### Lecture continue : vitesse, silences et vidéo

n20 par ligne, P95 par rang supérieur (19e valeur triée), dix mesures par sens.
Silence = durée totale échantillonnée pendant demande→nouveau signal+500ms.
Un zéro est une absence de silence détecté avec ce graphe, pas la preuve
d’une absence de microcoupure acoustique. Gaps et frames perdues restent distincts.

| Navigateur | Marge s | Latence P50 / P95 / max ms | Silence P50 / P95 / max ms | Frames perdues / total navigateur | Gap callback max ms | Waiting |
|---|---:|---:|---:|---:|---:|---:|
| Chrome | 0 | 163,1 / 186 / 208,4 | 43,9 / 74,1 / 96,8 | 12/393 | 132 | 20/20 |
| Chrome | 0,5 | 409,4 / 437,4 / 457,2 | 0 / 0 / 0 | 0/443 | 75,1 | 0/20 |
| Chrome | 1 | 405,6 / 417,7 / 420,4 | 0 / 0 / 0 | 0/435 | 74,6 | 0/20 |
| Chrome | 2 | 405,6 / 433,9 / 443,9 | 0 / 0 / 0 | 0/440 | 75 | 0/20 |
| Chrome | 4 | 2315,7 / 2331,5 / 2333,9 | 0 / 0 / 0 | 0/1355 | 83,3 | 0/20 |
| Edge | 0 | 166,1 / 197,1 / 331,5 | 40,1 / 117,5 / 250,6 | 9/390 | 283,3 | 20/20 |
| Edge | 0,5 | 402,3 / 421,9 / 422,1 | 0 / 0 / 0 | 0/436 | 67,3 | 0/20 |
| Edge | 1 | 405,6 / 422,9 / 475 | 0 / 0 / 0 | 0/438 | 67,1 | 0/20 |
| Edge | 2 | 401,3 / 415,9 / 427,1 | 0 / 0 / 0 | 0/435 | 66,9 | 0/20 |
| Edge | 4 | 2312,4 / 2321,2 / 2328,6 | 0 / 0 / 0 | 0/1350 | 83,4 | 0/20 |
| Firefox | 0 | 185 / 214 / 243 | 88 / 119 / 176 | 1/283 | 261,4 | 20/20 |
| Firefox | 0,5 | 1122,5 / 1134 / 1142 | 0 / 0 / 0 | 0/781 | 86,3 | 0/20 |
| Firefox | 1 | 1125,5 / 1136 / 1141 | 0 / 0 / 0 | 1/784 | 182,8 | 0/20 |
| Firefox | 2 | 1152 / 1163 / 1170 | 0 / 0 / 0 | 0/796 | 82,1 | 0/20 |
| Firefox | 4 | 2324 / 2340 / 2370 | 0 / 0 / 0 | 0/1364 | 91,4 | 0/20 |

Les marges positives n’ont aucun silence détecté dans les240 changements en
lecture, ni dans les48 changements après-seek. Toutefois marge1 signale une
frame perdue Firefox en lecture, trois Firefox et une Edge après-seek ; ne pas
les masquer derrière zéro silence. Les compteurs du navigateur ne prouvent
pas une restitution physique sans gel. Des gaps de callbacks subsistent.
La marge4 coûte environ2,3s avant le signal et n’apporte pas d’amélioration
mesurable du silence par rapport aux petites marges sur ce jeu.

### Comparaison des deux sens en lecture

Dix observations par sens : leur P95 est donc le maximum observé.

| Navigateur | Marge s | FR→EN P50 / P95 ms | EN→FR P50 / P95 ms |
|---|---:|---:|---:|
| Chrome | 0 | 159,8 / 182,4 | 168 / 208,4 |
| Chrome | 0,5 | 399,5 / 437,4 | 416,8 / 457,2 |
| Chrome | 1 | 394,7 / 411,4 | 407 / 420,4 |
| Chrome | 2 | 399,5 / 425,4 | 414,6 / 443,9 |
| Chrome | 4 | 2313,3 / 2331,5 | 2315,7 / 2333,9 |
| Edge | 0 | 167,6 / 194,1 | 163,3 / 331,5 |
| Edge | 0,5 | 393,2 / 410,6 | 408,5 / 422,1 |
| Edge | 1 | 401,3 / 413,4 | 410 / 475 |
| Edge | 2 | 393,1 / 414 | 405,7 / 427,1 |
| Edge | 4 | 2311,7 / 2319,8 | 2314,8 / 2328,6 |
| Firefox | 0 | 182,5 / 214 | 190 / 243 |
| Firefox | 0,5 | 1120 / 1142 | 1125,5 / 1134 |
| Firefox | 1 | 1124,5 / 1141 | 1126,5 / 1136 |
| Firefox | 2 | 1155 / 1170 | 1147,5 / 1161 |
| Firefox | 4 | 2316,5 / 2333 | 2326,5 / 2370 |

### Pause, après-seek et cohérence A/V

Quatre observations par catégorie/ligne, deux par sens : effectif exploratoire,
pas une estimation robuste des événements rares. Pause conservée dans tous
les changements mesurés. Ci-dessous reprise→signal, sans le hold volontaire100ms.
Les délais demande→signal, qui l’incluent, restent dans le JSON.

| Navigateur | Marge s | Reprise→signal P50 / max ms | Après-seek P50 / max ms | A/V toutes catégories min..max ms |
|---|---:|---:|---:|---:|
| Chrome | 0 | 78,5 / 82,2 | 164,2 / 171,4 | 8..29,3 |
| Chrome | 0,5 | 382,8 / 392,8 | 385,5 / 398,2 | 8..30 |
| Chrome | 1 | 370,2 / 396,7 | 828,8 / 860,3 | -31,3..32 |
| Chrome | 2 | 370,5 / 389,4 | 825,4 / 835,7 | 8..32 |
| Chrome | 4 | 2306 / 2318,7 | 2772,5 / 2786,6 | 10,7..29,3 |
| Edge | 0 | 78,6 / 81,4 | 156,4 / 165,2 | 8..49,8 |
| Edge | 0,5 | 377,1 / 394,7 | 384,8 / 389,3 | 8..29,3 |
| Edge | 1 | 375,3 / 382,5 | 810,6 / 813,4 | 8..40 |
| Edge | 2 | 374,3 / 383,2 | 823,3 / 829,9 | -34..40 |
| Edge | 4 | 2299,5 / 2345 | 2766,8 / 2782,1 | 8..31,5 |
| Firefox | 0 | 76 / 85 | 167 / 174 | -60,7..18,7 |
| Firefox | 0,5 | 1118 / 1126 | 1122,5 / 1130 | -63,3..10,7 |
| Firefox | 1 | 1120 / 1128 | 1127 / 1194 | -101,7..21,3 |
| Firefox | 2 | 1121 / 1129 | 1155 / 1192 | -60,7..21,3 |
| Firefox | 4 | 2317 / 2335 | 2807 / 2837 | -60,7..18,7 |

Les420 marqueurs sont dans la tolérance±200ms : plage globale−101,7..+49,8ms.
Cela ne prouve ni une synchronisation physique précise ni la dérive sur un film.
Polling maximal observé168ms : petites interruptions pouvant être manquées.
Les longues marges prolongent aussi le délai après reprise ; la nouvelle langue
n’est pas déclarée audible pendant la pause. Source vidéo inchangée pendant
tous les changements mesurés ; des interruptions peuvent néanmoins exister.

### Buffers, erreurs et anciennes suites

Même configuration streaming Shaka et mêmes octets. Buffer audio observé
jusqu’à12,001s avant /12,008s après, sur toutes les marges : aucune augmentation
de bufferingGoal pour obtenir ces résultats. Une marge conserve de l’ancien
audio ; ce n’est pas une promesse d’un volume supplémentaire égal à cette marge.
Pas de conclusion RAM/CPU ni de comportement sur un film long à partir du clip.

Quatre échecs Firefox dans la matrice initiale :

- index13 : préchauffage FR→EN à marge0, PTS vidéo en recul (−333ms) ;
  la cible marge1/lecture n’a pas été mesurée.
- index110 : timeout WebDriver45s après un préchauffage abouti, avant la cible
  marge4/pause. Étape exacte/cause non déterminées ; pas une erreur codec prouvée.
- index122 : préchauffage marge0 avec recul PTS (−125ms), avant la cible
  marge0/après-seek.
- index126 : préchauffage marge0 sans progression PTS dans la fenêtre, avant
  la cible marge1/après-seek.

Compléments ciblés, un essai chacun, tous mesurés : dossiers firefox-supplement.
Leurs métadonnées indiquent les filtres MEDIA_SAFE_ONLY_MODE/ONLY_FROM et la
marge. Ils complètent les effectifs sans supprimer le taux d’échec de préparation.
Aucun échec cible de commutation/marqueur dans les420 observations obtenues,
mais336 régressions fonctionnelles de langue au seek arrière.

Anciennes suites communes MSE/Shaka rejouées avec arguments par défaut :
**4/6 parcours réussis**, pas une validation globale verte.

- MSE Chrome/Edge et Shaka Edge/Firefox :18 changements et contrôles passés.
- Shaka Chrome : échec au6e changement, FR après seek ; PCM correct mais
  un seul callback vidéo, progression PTS nulle dans la fenêtre, une frame
  perdue signalée, gap116,4ms. Échec conservé, aucun seuil assoupli.
- MSE Firefox : timeout PCM FR après15 changements aboutis, contrôle suivant
  non achevé. Cause non résolue ; le prototype MSE reste inchangé.

Ces deux parcours avaient réussi lors du jalon précédent. La présente
réexécution ne permet pas d’attribuer leur échec au paramètre safeMargin ;
elle confirme que la fluidité et la répétabilité ne sont pas acquises.
Preuves dans le sous-dossier regression ; pas de relance destinée à les masquer.
Neuf tests Rust, fmt/clippy, syntaxesJS et les deux suites de statistiques
passent. CI adaptée aux statistiques uniquement, exécution distante non revendiquée.
Empreintes des médias, MPD et bundle inchangées avant/après toutes les séries.

## Configuration retenue pour poursuivre, sans choix de lecteur définitif

**0,5s est le candidat minimal pour la fluidité en lecture seule** : zéro silence
détecté, zéro frame perdue signalée dans ses60 observations de lecture et12
après-seek, délai proche des marges1/2 sur Chrome/Edge et environ1,1s sur
Firefox. Les petites différences entre0,5/1/2 ne justifient pas un classement
statistique fin ;4s impose un délai plus lourd sans gain démontré ici.

**Ne pas promouvoir0,5s en réglage général** : sa langue après seek est incorrecte,
comme pour les autres marges positives. Le banc garde0 par défaut, plus rapide
et cohérent au seek dans les observations mesurées, mais avec des coupures
et des limites de répétabilité documentées. Aucune marge testée ne résout tout.

Prochain petit essai : trouver et tester un mécanisme supporté par Shaka pour
invalider l’ancien audio lors d’un seek, avec preuve FFT de la langue après
seek, sans reload vidéo. Re-sélectionner simplement la même piste n’est pas
présumé forcer ce nettoyage. Ensuite : clips plus longs et durées de segments
variées, rapidité des choix successifs/annulation, réseau lent, endurance et
écoute physique multicanal. Mono/7.1 et affichage physique10bits restent NT.
Video.js et SvelteKit ne sont pas développés dans ce jalon.

## Vérification courte de la resélection audio (2026-10-15)

Six essais ciblés FR→EN, safeMargin0,5, Chrome155.0.8059.40,
Edge155.0.4283.45 et Firefox155.0.1. Script scripts/verify-audio-seek.cjs.
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

## Nettoyage forcé de la variante active — preuve ciblée (2026-10-15)

API publique Shaka5.2.13 testée : selectVariantTrack(activeVariant,true,0).
La source versionnée player.js/switchVariant_ confirme force=true pour la
variante déjà active si clearBuffer=true, contrairement à selectAudioTrack.
Le nettoyage concerne aussi la vidéo : ce n'est PAS une invalidation audio seule.

Six essais, FR→EN et EN→FR sur Chrome155.0.8059.40, Edge155.0.4283.45,
Firefox155.0.1 : défaut préalable reproduit6/6 après changement marge0,5 et
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


Suite : voir P0C_AUDIO_SEEK_RECOVERY.md. Politique opt-in intégrée, mais interruptions et échecs persistants ; récupération non fiable et désactivée par défaut. Les conclusions antérieures ponctuelles ne constituent pas une validation du contrôleur.
