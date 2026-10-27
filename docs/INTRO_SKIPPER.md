# IntroSkipper — état vérifié le 26 octobre 2026

IntroSkipper est un plugin Wasm de classification de chapitres et de comparaison
d'empreintes fournies par l'hôte. Une extension installée, activée et autorisée
pour la bibliothèque est désormais admise par l'outbox à chaque ajout ou
modification d'épisode. Le Core appelle `intro.parse_chapters` dans le worker
Wasm, valide les marqueurs et persiste le résultat dans `automatic_timeline_markers`.
Les opérations de détection et de lecture des marqueurs exigent la permission
`playback.markers` ; `metadata.read` seul ne les autorise pas. Le manifeste
source d'IntroSkipper déclare déjà cette permission.
Le descripteur de la copie exacte réutilise uniquement un résultat de la
révision courante, de la génération d'extension active et d'une bibliothèque
autorisée. Les boutons du lecteur utilisent ce résultat ; sans résultat,
l'ancienne heuristique de chapitres reste un repli temporaire.

Les préférences `skipIntroEnabled` et `skipCreditsEnabled` sont enregistrées
par compte via `/api/v1/preferences`. L'administrateur peut corriger ou
désactiver chaque type de marqueur pour une copie dans le lecteur :
`PUT /api/v1/admin/assets/{asset}/markers/{intro|credits}` avec
`{"enabled":true,"start":10,"end":60}` ou `{"enabled":false}` ; `DELETE`
retire la correction. Session administrateur, CSRF, épisode et bornes de la
timeline sont vérifiés côté Core. La correction en base prime sur la détection.

Le catalogue signé local ne contient toujours pas IntroSkipper. Sa publication
avec la clé de release reste nécessaire pour une installation publique.
Le Node expose une opération `analyze` avec composant `fingerprint` sur les
assets inventoriés. Elle décode seulement l'audio avec le muxer Chromaprint
de FFmpeg, via le descripteur confiné, sur au plus 600 s, avec sortie plafonnée
à 32 KiB et délai de 90 s. L'annulation et la préemption utilisent le worker
de tâches existant. FFmpeg doit disposer du muxer Chromaprint ; son absence
produit une erreur explicite. L'opération passe sur la fixture AV1/Opus.
Une introduction absente des chapitres déclenche une tâche d'empreinte à faible
priorité. Après achèvement, le Core compare au plus deux épisodes distincts de
la même saison via `intro.detect` avec entrée base64 bornée. Le résultat est
persisté par copie. Le moteur de tâches reprend les calculs interrompus et
ignore les empreintes d'une source modifiée. L'admission d'une empreinte attend
l'absence de lecture active non pausée.

Les crédits sans chapitre ne sont pas encore analysés : le Node ne décode que
les 600 premières secondes et le plugin exige une empreinte de fin explicite.
Les différentes copies gardent leurs marqueurs séparés faute de preuve de
compatibilité de leurs timelines.

## Limites et suite

Le plafond d'entrée Wasm reste 65 536 octets, avec deux épisodes par appel.
Le fuel de `intro.detect` est porté à 200 millions ; les autres opérations
restent à 2 millions. Une paire synthétique de 5 100 mots par épisode échouait
à 2 millions. Après indexation par quatre bandes, elle a consommé environ
139 millions de fuel et 12,6 s sur Wasmi Windows local. Cette mesure ne prouve
ni précision ni performance sur un corpus réel. Le partage entre copies exige
une vérification de leur timeline, notamment près de la fin pour les crédits.

Pour traiter les crédits audio et 24 épisodes sans gonfler l'entrée Wasm, le jalon suivant est un cache binaire Node avec checksum et une comparaison paginée. La saison, la source et la version d'algorithme devront être vérifiées à chaque page. Le Core conservera les corrections manuelles prioritaires.

## Preuve encore nécessaire

Le corpus annoté doit couvrir 24 épisodes, plusieurs saisons, cold opens,
récapitulatifs, intros variables, silence et crédits. Rapporter précision, rappel,
faux positifs et cas ambigus par méthode, ainsi que temps CPU, RSS, volume audio
lu et nombre d'empreintes. Les tests synthétiques actuels ne prouvent pas ces
mesures ni le fonctionnement complet sur deux Nodes. Le résultat de chapitre
signé a été testé après redémarrage de la base locale.
