# P3/P4 — préparation des pistes (18 octobre 2026)

## Reprise d'une piste audio externe — 30 octobre 2026

Le chargement des métadonnées audio commence en parallèle de la récupération de
la progression, puis la piste est positionnée directement sur la reprise au
lieu de valider d'abord la position zéro. Les attentes sont bornées et annulées
à la fermeture. La courte fin naturelle d'une piste Opus reste acceptée ; si
la durée vidéo n'est pas encore connue, sa vérification est différée.
Un délai de seek vidéo affiche les positions demandée et atteinte ainsi que
l'état de seek, sans URL ou jeton. Ces indications ne suffisent pas toujours
à identifier la cause d'un incident sur un fichier personnel.

Validation locale : Core/Node autonome Rust reconstruit, Chrome et fichier MP4
AV1/Opus synthétique de 5 400 s avec audio externe ; reprise et six seeks
éloignés réussis, puis 10,5 s de lecture sans erreur. La réouverture observée
à 356 ms et la série de seeks stabilisée à 782 ms sont deux observations
isolées, pas des percentiles. Rapport brut :
`target/player-long-external-current-20261030/report.json`. Le premier essai
avec un Core précompilé plus ancien s'était arrêté à la création de bibliothèque,
avant la lecture. Aucun NAS, film personnel ou endurance de 90 minutes testé.

## Seeks répétés — 30 octobre 2026

Le watchdog vidéo repart de zéro à chaque nouveau seek et se désarme quand le
navigateur a terminé le seek et dispose de données. Une attente réellement
bloquée reste signalée. Une fenêtre audio MSE demandée après un seek n'attend
plus la fin d'une opération devenue obsolète pour commencer son téléchargement ;
le nouvel append attend toujours la fin de l'opération MSE. Les plages déjà
présentes sont réutilisées. Tests et limites : [mesures locales](PLAYER_SEEK_2026_10_30.md).

## Sous-titres au démarrage — 30 octobre 2026

La préparation du sous-titre initial démarre en parallèle : la lecture vidéo et
audio peut commencer avant la fin d'une extraction intégrée. Exception : les
fenêtres audio préparées par FFmpeg partagent son worker avec les sous-titres ;
leur ordre initial reste conservé pour éviter d'affamer l'audio. Le sous-titre
apparaît lorsqu'il est prêt ; en mode parallèle, un échec est signalé sans
arrêter la lecture. Une
sélection manuelle ultérieure et la fermeture invalident les résultats périmés.
Ce changement ne prouve pas que les sous-titres expliquent un incident donné,
ni que le démarrage est instantané sur un NAS ou un navigateur particulier.
En cas d'échec initial, la console indique désormais la position demandée, la
position effective et l'état de seek, sans inclure l'URL ou les jetons média.

## Reprise après délai vidéo — 29 octobre 2026

Les requêtes de progression et de préférences démarrent en parallèle ; le choix
de piste attend toujours les préférences et le seek attend la progression.
Le délai de connexion directe (2,5 s pour les métadonnées lorsqu'un relais est
disponible) ne s'applique plus au seek : la position et le seek disposent chacun
de leur propre délai borné. Une progression expirée est distinguée d'un seek
expiré ; seul ce dernier peut être retenté sur la même source sans relais.

Le lecteur ignore aussi les événements `seeked` antérieurs à la résolution de
la progression et au seek effectivement demandé : ils ne peuvent plus valider
prématurément la reprise. Un test reproduit la course avec progression retardée.

Lorsqu'un seek initial expire et qu'aucun relais distinct ni repli utilisable
ne sont proposés, le lecteur recharge une seule fois la même source à la
position demandée. Lorsque le repli progressif est disponible, ou qu'une vidéo
préparée existe, il est essayé après l'échec sans refaire le seek sur la même URL. Les
erreurs de métadonnées ne déclenchent pas cette nouvelle tentative ; un relais
disponible reste prioritaire. Chaque attente est bornée à dix secondes et la
fermeture l'annule. Ce comportement ne démontre pas la cause d'un incident
réseau ou d'un fichier particulier ; une seconde expiration reste explicite.

## Annulation et préparations partagées — 29 octobre 2026

Les demandes froides d'un même composant vidéo, audio complet ou sous-titre
partagent désormais le verrou par clé déjà utilisé pour les fenêtres audio.
Les requêtes attendantes ne lancent pas de FFmpeg supplémentaire ; après
publication elles ouvrent le même résultat sous lease. La concurrence reste
bornée à 64 participants et un worker FFmpeg. Une demande différente peut
toujours recevoir `preparation_busy` : il ne s'agit pas d'une file globale
illimitée. L'attente d'un composant complet expire après la limite de préparation
configurée plus cinq secondes. L'annulation du demandeur libère ses gardes ;
un autre demandeur intéressé peut reprendre. Les écritures Tokio sont vidées
avant publication du fichier et les erreurs de cette dernière écriture remontent.

Côté lecteur, pause/fermeture annulent les attentes de disponibilité audio ;
une reprise dans la dernière demi-seconde d'une fenêtre charge également la
suivante pour permettre au décodeur de terminer son seek, sans attendre la fin
du chargement initial. Ailleurs, le démarrage garde la seule fenêtre courante.
La préparation reste limitée à deux fenêtres, sans copie intégrale du média.
Une mise à jour MSE Opus sans événement de fin expire après dix secondes et se
nettoie à fermeture. Une erreur terminale de fenêtre active arrête les demandes
périodiques plutôt que de relancer la même préparation toutes les 250 ms.
Une préparation vidéo progressive supersédée rejette son attente de premier
fragment ; sa fermeture rejette aussi une attente de progression sauvegardée.
Le watchdog vidéo invalide la source et libère l'audio avant de signaler son
erreur, pour qu'une nouvelle sélection de la même piste recharge réellement.

Au seek, l'attente de disponibilité audio suit la nouvelle position avec son
timeout initial ; les époques empêchent un ancien démarrage de relancer l'audio.
L'annulation explicite de cette attente au seek reste à coordonner avec la
reprise dans NativePlayer. Les erreurs MSE d'une ancienne époque sont ignorées,
alors que celles de la position courante restent fatales. La maintenance du
cache conserve des opérations disque synchrones ; son impact sur stockage lent
reste à mesurer avant modification du protocole de récupération.

Les correctifs ne changent ni les codecs, ni les canaux, ni les autorisations.
Mesures, scénarios exécutés et limites :
[rapport du 29 octobre](PLAYER_RELIABILITY_2026_10_29.md).

## Démarrage progressif et reprise — 23 octobre 2026

La progression sauvegardée se charge en parallèle des métadonnées vidéo. Le
lecteur atteint cette position avant de demander le premier segment audio : une
reprise au milieu du film ne prépare plus inutilement le segment zéro avant le
segment utile. Une progression reçue après fermeture ne modifie pas le lecteur.

Le démarrage exige toujours des données décodables (`HAVE_FUTURE_DATA`) et une
réserve contiguë de 50 ms, au lieu de 750 ms. Ce seuil est une quantité de média
en tampon, pas une promesse de démarrage en 50 ms. Le transport, l'index du
conteneur, le décodage et une préparation de repli peuvent encore demander
plus de temps. Pour les MKV AV1, le lecteur essaie maintenant la vidéo originale
avec l'audio Opus sélectionné par segments. La présence de métadonnées ne suffit
pas : il attend aussi des données décodables à la position demandée. En cas de
refus du navigateur, il prépare explicitement la vidéo MP4 sans réencodage puis
conserve le même audio sélectionné. La copie vidéo complète reste un repli,
dont la première préparation peut être longue.

Les bornes d'erreur, la pause, l'annulation et la synchronisation
audio/vidéo restent actives. Le panneau d'erreur affiche désormais la cause
technique remontée par le moteur au lieu de masquer toutes les causes derrière
le même message. Les préférences du compte sont disponibles avant de choisir
la piste, même lorsque l'interface générale se charge progressivement.

Validation unitaire ciblée : 35 tests réussis dans `native.test.ts`,
`startup-buffer.test.ts`, `segmented-audio.test.ts` et `external-audio.test.ts`.
Ces tests ne certifient ni le décodage sur un NAS ni une durée de démarrage réelle.

## MP4 sans copie vidéo complète — 22 octobre 2026

Pour les MP4 AV1, la vidéo est maintenant l'original servi avec Range. Chrome/Edge
acceptant MSE Opus chargent seulement l'audio sélectionné en fenêtres de 10 s,
copiées vers fMP4. La préparation vidéo complète n'est plus un préalable. La
fenêtre courante et la suivante sont seules préchargées ; les seeks demandent
les fenêtres visées. Les anciens parcours MKV/Ogg restent disponibles.

Preroll, alignement temporel, cache avec leases, logs FFmpeg bornés, mesures et
limites de la validation NAS : [NAS_PLAYBACK_DIAGNOSTICS.md](NAS_PLAYBACK_DIAGNOSTICS.md).
Cette section remplace, pour ce parcours MP4, les mentions de copie complète
et de préparation uniquement locale dans l'historique ci-dessous.

## Corrections du 21 octobre 2026

Le lecteur prépare vidéo et audio en série, conformément au worker unique du
Node. Un changement entre pistes partageant la même vidéo charge et positionne
le nouvel audio en pause avant de remplacer l'ancien. Un échec détruit le candidat
et laisse l'ancienne piste disponible ; la fermeture annule aussi le candidat.
Les erreurs terminales invalident la source pour permettre une vraie nouvelle
tentative. Aucun réencodage vidéo ou audio n'est introduit.

La sélection automatique privilégie une piste précise dans la langue préférée,
puis le français ; le choix manuel garde priorité. La langue d'un original avec
plusieurs audios vaut `und`, car le navigateur peut choisir une autre piste que
la première. Les codes `fra/fre/fr-FR` sont reconnus comme français. L'interface
identifie cet original comme « piste choisie par le navigateur ».

Le chargement audio attend une position exploitable et des données décodables.
Le seek et le décalage recalent explicitement l'audio ; un blocage audio suspend aussi
la vidéo, avec reprise sur `canplay` et erreur après dix secondes. Une pause
manuelle annule cette reprise. La correction périodique intervient au-delà de
80 ms, sans modification de vitesse pour rattrapage. Les deux horloges restent
indépendantes : aucun engagement de précision physique à l'échantillon.

Les limites configurables sont documentées ci-dessous. L'augmentation des bornes
ne rend pas le remux instantané : la première préparation lit et écrit toujours
le composant complet avant sa lecture. Une piste non sélectionnée n'est pas
extraite préventivement. Le Direct Play mono-audio approprié reste prioritaire.
Le cache reste temporaire, borné et sans éviction de composants pendant lecture.
Les chemins préparés distants utilisent le même contrat via Core/Node.

Les paquets AV1/Opus sont copiés, sans `-ac`, filtre, réencodage ou graphe WebAudio
de production. Les tests couvrent la conservation de six et huit canaux ; la
sortie matérielle 5.1/7.1 dépend du navigateur, de l'OS et de l'équipement et
n'est pas certifiée par ces tests. Les sondes de type MIME/codec guident le choix,
elles ne remplacent pas une preuve de décodage.

Résultats et limites : [audit et corrections](PLAYER_PLAYBACK_AUDIT.md).

Le texte historique ci-dessous décrit le premier jalon ; les points ci-dessus
et les nouvelles limites remplacent ses anciennes restrictions.

Le lecteur conserve le parcours direct original et ajoute, pour les assets locaux
AV1, un choix par piste Opus interne. Le Node prépare une vidéo MP4 fragmentée
sans audio et un fichier Ogg par piste sélectionnée, uniquement par copie de
paquets. FFmpeg est **facultatif**, requis sur le PATH du processus pour ce
parcours ; son absence donne une erreur explicite. Les images Docker existantes
ne sont pas modifiées : aucune disponibilité de cette commande en conteneur
n'est annoncée. Le chemin direct et les pistes externes existants sont conservés.

Le premier passage à une piste préparée change la source vidéo en conservant la
position et la pause. Les passages entre pistes préparées partagent la même
vidéo : aucun rechargement vidéo, plein écran conservé. Chaque nouvel audio est
chargé séparément ; une courte pause de commutation reste possible. Le mécanisme
à deux éléments média corrige une dérive de plus de 80 ms toutes les 100 ms ;
il ne garantit pas une synchronisation à l'échantillon.

Les sous-titres intégrés textuels SubRip, WebVTT, mov_text, ASS/SSA sont extraits
vers WebVTT. Les sidecars ASS/SSA UTF-8 ont un **repli texte**, avec chronologie,
virgules et sauts de ligne conservés et balises échappées. Ni polices, placements,
animations, karaoké, dessins, ni styles ASS avancés ne sont reproduits. Ce parcours
n'est pas un moteur libass. Les sous-titres bitmap restent non sélectionnables.

Les offsets acceptent -30 à +30 secondes (positif = retarder). L'audio doit être
préparé ou externe ; le contrôle est désactivé pour la source originale. Les
choix précis par asset, la langue et les offsets sont mémorisés par compte dans
ce navigateur. Pas de synchronisation des préférences entre appareils. Les
sous-titres forcés FR conservent leur sélection automatique, après choix manuel.

## Frontières et limites

Chaque GET/HEAD/Range passe par les droits Core actuels. Seuls l'ID d'asset et
les indices de pistes présents dans le probe sont acceptés. L'entrée FFmpeg est
un handle confiné, avec protocoles fd/pipe et formats MKV/MP4 autorisés ; aucun
chemin ou URL fourni par le navigateur, aucune écriture dans les bibliothèques.
Cette restriction de protocoles n'est pas un sandbox système.

Une préparation à la fois, refus immédiat si occupé, copie des sorties par blocs,
64 entrées maximum. Bornes par processus standalone/Node, chargées au démarrage :

| Variable | Défaut | Plage |
| --- | --- | --- |
| `MEDIA_PREPARATION_SECONDS` | 900 secondes | 30–3600 |
| `MEDIA_PREPARATION_CACHE_GIB` | 96 Gio | 1–2048 |
| `MEDIA_PREPARATION_VIDEO_GIB` | 64 Gio | 1–1024 |
| `MEDIA_PREPARATION_AUDIO_MIB` | 4096 Mio | 64–16384 |

Les composants doivent tenir dans la borne du cache ; une configuration invalide
refuse la préparation. Les sous-titres restent bornés à 4 Mio. Ces plafonds ne
réservent pas le disque ; dimensionner le stockage applicatif séparé en conséquence.
`MEDIA_PREPARATION_CACHE_DIR` accepte un chemin absolu avec parent existant,
hors bibliothèques. Les images Docker placent ce cache dans `/data/preparation-cache`
plutôt que dans le tmpfs `/tmp` limité à 64 Mio par les Compose : les fichiers
préparés ne sont donc pas plafonnés involontairement à 64 Mio. Aucun volume média
n'est modifié. Sans cette variable, un lancement natif utilise le dossier temporaire
privé existant. Les entrées restent sous lease et sont nettoyées comme auparavant ;
un stockage sur `/data` n'en fait pas un cache réutilisable après redémarrage.
Les réponses déjà en cache restent
accessibles pendant une autre préparation. Cache plein = erreur explicite ;
pas d'éviction d'un fichier en cours de lecture. Les répertoires temporaires
sont nettoyés à la destruction normale du service. Un arrêt brutal peut laisser
des fichiers temporaires orphelins : la borne n'est pas une garantie globale
sur ces orphelins entre redémarrages. Le nettoyage persistant reste à améliorer.

Le packaging distant n'est pas exposé dans ce jalon. Les chemins multi-Node
existants restent en place. Autres codecs audio, vidéo hors AV1, HDR, sortie
physique 5.1, autres navigateurs et rendu ASS complet ne sont pas validés.
Le raw reste expérimental et ne bloque pas ce parcours.

## Catalogue

Migration additive 0016 SQLite/PostgreSQL : `inventory_works.added_at`, timestamp
Unix de première insertion, stable aux rescans et redémarrages. Les œuvres
antérieures conservent NULL (date historique inconnue). Tris `title-asc/desc`,
`year-asc/desc`, `added-asc/desc`, inconnus en dernier et ordre déterministe par
identifiant. L'accueil utilise `added-desc`. Aucune date inventée par migration.

## Validation

Commandes et résultats effectivement exécutés dans MEMORY.md ; preuves Chrome
sous `evidence/2026-10-18/player-finalization/` et essais longs séparés sous
`player-long/`. Un fichier de 90 minutes et une minute observée ne constituent
pas une preuve de lecture continue de 90 minutes ni une mesure de fuite RSS.

Commandes exécutées :

- `cargo test --locked --workspace -j 1 -- --test-threads=1` : 127 réussis, 8 ignorés.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo fmt --check`.
- Web : `npm run check`, `npm run lint`, `npm test` (48), `npm run build`.
- `cargo test --locked --test browser_scanner -- --ignored --nocapture`, puis même
  commande avec `$env:MEDIA_TEST_LONG='1'` : courts 40,41 s / longs 249,71 s, réussis.
- `cargo test --locked --test browser_series -- --ignored --nocapture` avec
  `MEDIA_REPORT_DIR` isolé : réussi en 38,66 s.

Les tests longs ont laissé des caches lors des premiers arrêts forcés : 31 dossiers,
1 703 262 315 octets dans le répertoire temporaire de l'hôte. La commande de
nettoyage a été refusée par le contrôle automatique (`blocked by policy`) et
n'a supprimé aucun fichier. Les essais finaux isolent les temporaires dans leur
propre dossier jetable ; le nombre et le volume des anciens orphelins restent
inchangés après les validations finales. Ce résidu et la purge après crash restent
une limite explicite, sans annoncer le cache de production définitivement fiabilisé.


## Validation locale du 23 octobre 2026 : démarrage natif et repli MKV

Les pistes internes AV1/Opus MKV utilisent désormais la vidéo originale et des
fenêtres audio Opus de dix secondes. Une erreur réelle de chargement/décodage
vidéo déclenche le repli `preparedVideoUrl`, dont le fichier MP4 reste entièrement
validé avant publication. Une lecture native réussie évite donc les copies
complètes vidéo et audio au démarrage ; le repli peut encore être long sur un
gros fichier. Aucun cache incomplet n'est exposé.

Validation Chrome 155.0.8059.86 headless, Windows 10.0.26300, fixtures synthétiques
représentatives AV1 10 bits/Opus : MP4 et MKV ont fourni des images décodées
(542 à 861 couleurs distinctes sur les échantillons), des signaux PCM FR/EN
identifiables, des changements de piste, pauses/seeks, sous-titres et reprise de
progression fonctionnels. Le MKV conserve son URL `/content` pendant les quatre
changements de piste, sans requête `/prepared/video`. Une panne forcée de cette
URL confirme ensuite le repli vidéo préparé avec l'audio sélectionné actif.

La mesure clic → `currentTime > 0.2 s` est de 628 ms pour MP4 et 790 ms pour MKV
sur ce passage. Elle inclut plus de 200 ms de lecture et ne mesure pas exactement
la première image. Elle ne démontre ni un démarrage en 50 ms, ni les performances
d'un NAS, d'un réseau distant, d'un autre navigateur ou d'un film arbitraire.

Commandes : test `scanner_flow` ciblé réussi (16,25 s), puis exécution directe du
harness `target/debug/deps/browser_scanner-f7f850305e003846.exe --ignored --nocapture`
réussie (49,79 s), avec le binaire Core recompilé par le test scanner et les assets
Web finaux. Le harness Rust était inchangé ; cette exécution ne valide pas le
nouvel endpoint de pagination des épisodes ajouté pendant ce passage.
Rapports : `target/browser-scanner/report.json`, `report-restart.json` et
`target/browser-scanner-current.log`. Les vérifications pixels échantillonnent
plusieurs images pour traverser les flashs blancs de synchronisation de la fixture.

## Fin MSE audio et régression CI — 23 octobre 2026

Le journal Linux signalait une attente audio de 10 secondes avec la vidéo à
11,95478 s, après des segments reçus HTTP 200. Le lecteur signale désormais
`MediaSource.endOfStream()` après le dernier segment audio. Il autorise toujours
l'ajout d'un segment après un seek en arrière depuis l'état MSE `ended`.
Un écart final audio/vidéo d'au plus 150 ms peut terminer naturellement ; une
troncature significative et une panne au milieu du média restent des erreurs.

Le parcours scanner ouvre explicitement une nouvelle piste dans les 40 dernières
millisecondes puis revient à une fenêtre audio antérieure et reprend la lecture.
L'état « vu » éventuellement produit par ce seek est réinitialisé avant le test
distinct du bouton manuel. Les tests unitaires vérifient aussi l'arrêt court
normal, la reprise après EOS et le refus d'une vraie troncature.
Le reliquat est comparé aux durées encodées, afin de préserver un décalage négatif
choisi par l'utilisateur. Le test de persistance de langue attend son seek en
pause pour ne pas dépendre du temps restant sur la fixture de douze secondes.

Validation locale : scanner Core/Chrome 155.0.8059.86 Windows réussi en 35,70 s,
MP4/MKV, images et PCM, réouvertures FR/EN et repli préparé. Le test distribué
réussit en 9,47 s et distingue désormais la vidéo originale MKV du repli MP4,
avec GET/HEAD/Range et refus utilisateur conservés. 124 tests Web, check, lint,
build et Clippy ciblé réussis. Rapports dans `target/ci-scanner-tail-regression/`.
Ces résultats locaux ne constituent pas une réexécution du workflow Linux distant.
