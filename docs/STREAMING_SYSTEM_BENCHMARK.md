# Banc système streaming — protocole de travail

État du 24 octobre 2026 : protocole, launcher système et collecteur Windows
livrés ; premières mesures exploratoires obtenues, lot de référence en cours.
Les mesures historiques du moteur isolé restent dans
[STREAMING_PERFORMANCE.md](STREAMING_PERFORMANCE.md).

## Chemin réellement exercé

Deux processus Rust distincts, Core/SQLite et Media Node, écoutent chacun sur un
port loopback différent. Chrome ouvre les véritables assets Svelte servis par
Core. Bootstrap administrateur, appairage, bibliothèque distante et scan passent
par les API de production. Les réponses `/api`, les médias et les préparations
ne sont pas remplacés par Playwright ou par un serveur JavaScript.

Le launcher JavaScript ne sert aucun fichier : il lance les processus, organise
les essais et archive les résultats. Il ne transpile pas le lecteur de test.
La vidéo doit provenir du Node via `/stream/v1/assets/.../content` et un véritable
jeton limité ; le journal réseau garde méthode, statut, Range et chemin sans la
query contenant le jeton. Les préparations audio passent par le vrai cache Rust.
Un échantillon relayé Core est séparé, jamais mélangé au chemin direct.

Ce déploiement représente un Node distant **logique** sur la même machine. Il
ne mesure ni deux disques physiques, ni un LAN, ni un NAS. Le test existant
`distributed_multi_node` utilise de vrais sockets mais héberge les routeurs
dans un même processus : il est réutilisable pour les contrats, pas pour séparer
le coût CPU/RSS de Core, Node et FFmpeg.

Les données sont jetables et hors des médias : SQLite, identité Node, inventaire,
cache de préparation et logs ont chacun un dossier propre au run. Le média
synthétique est exposé en lecture seule par convention d'inventaire et vérifié
par hash après les mesures ; sous Windows un hardlink évite une copie intégrale
de fixture avant chaque essai. Le serveur n'écrit jamais dans ce dossier.

## États de cache et compteurs

| Phase | Processus | Navigateur | Cache de préparation | Cache disque OS |
| --- | --- | --- | --- | --- |
| Configuration | démarrés, bootstrap/scan | fermé | vide vérifié | lecture de scan explicite |
| Froid application | redémarrés, DB/inventaire conservés | contexte neuf | nouvelle racine vide | non purgé, état inconnu |
| Chaud serveur | conservés | contexte neuf par essai | fenêtres précédentes conservées | non purgé |
| Reprise chaude | conservés | même contexte, dialogue fermé/réouvert | conservé | non purgé |

Chaque phase mesure 1 session puis 4 simultanées. Le départ simultané est une
barrière après navigation/authentification, suivie des vrais clics Lecture ; il
ne s'agit pas de quatre lectures lancées séquentiellement puis observées ensemble.
Les scans, probes, hashes et la configuration sont hors du chronomètre du clic,
avec leurs propres durées. Le hash de fixture est calculé après les essais pour
ne pas primer discrètement tout son contenu.

Le fichier existant 1080p AV1 10 bits / deux Opus 5.1 fait 352 256 545 octets,
3 500 s à environ 12 fps. Il sert à la première validation. Une fixture MKV et
une variante 24 fps ne seront pas remplacées par une simple affirmation de
codec ; leur génération, taille, index, disposition des pistes et hachages des
paquets doivent être conservés. Aucun fichier personnel n'est utilisé.

Le clic est horodaté par un listener DOM capture de `.hero-cta`, avant le handler
Svelte. Le dialogue ouvert, `loadstart`, métadonnées, premier callback vidéo,
lecture effective et premier PCM sont distincts. rVFC observe une soumission
au compositeur, pas le photon à l'écran. PCM utilise exclusivement une sonde
WebAudio de test, avec durée et espacement réel des polls ; les graphes retirés
sont déconnectés. Le début physique du son et la sortie physique 5.1 ne sont pas
déduits du PCM.

Les seeks visent des positions éloignées et reviennent sur des fenêtres déjà
utilisées. Les switches vérifient la nouvelle fréquence connue, la position,
la continuité vidéo, le nombre de `loadstart`, le silence de toutes les pistes
et la restauration de l'ancienne piste lors d'un vrai échec du candidat. Les
erreurs réseau injectées sont des scénarios distincts des timings naturels.

Les échantillons bruts incluent tous les échecs et délais dépassés. Médiane,
p95, maximum et nombre d'échantillons sont obligatoires. **p99 n'est calculé
qu'avec au moins 100 échantillons indépendants dans la même cellule** ; sinon
le rapport indique « p99 non estimé » et garde le maximum exploratoire.
Un échantillon comprenant quatre sessions fournit quatre latences mais une
seule observation de ressources concurrentes, sans prétendre quatre runs
indépendants de la machine.

## Ressources Windows et limites Linux

Le collecteur Windows associe chaque processus Rust à un Job Object avant toute
préparation. Les enfants FFmpeg restent dans le job du Node. Les compteurs
cumulatifs du job permettent de conserver CPU et I/O des enfants déjà terminés,
contrairement à un simple polling de la liste des processus.

- CPU user/kernel : différences des compteurs Job Object par phase.
- RSS : somme échantillonnée des working sets des PID actifs du job, avec
  intervalle réel, pic observé et avertissement pour les enfants trop courts.
- Octets I/O : compteurs Windows read/write transfer ; **ce sont des I/O
  logiques, incluant éventuellement les pipes, pas des écritures physiques du NAS**.
- Cache : octets de fichiers effectivement créés/modifiés dans la racine de
  préparation, distincts des compteurs système ; SQLite Core séparée.
- Enfants FFmpeg : nombre total créé, temps CPU cumulatif du job et logs de
  préparation bornés. Le RSS instantané d'un enfant déjà terminé reste inconnu.

L'assignation Job Object doit réussir ou le run est déclaré sans mesure de
ressources complète. Le collecteur ne doit pas silencieusement remplacer ces
compteurs par ceux du launcher JavaScript.

Vérification locale en lecture seule : Docker installé mais daemon Linux
injoignable ; WSL liste uniquement `docker-desktop`. Aucun résultat Linux,
conteneur ou NAS n'est disponible dans cet environnement à ce stade. Aucun
daemon, conteneur ou distribution n'a été démarré pour changer cet état.

## Provenance et commande prévue

Chaque run archive le commit de base résolu, le diff binaire du checkout et son
SHA-256 s'il est modifié, les SHA-256 des exécutables et de tous les assets Web,
les versions de navigateur/FFmpeg, le profil de build et la configuration
sanitisée. Un manifeste scellé avant le départ est vérifié après les essais.
Un benchmark avant/après utilise deux ensembles d'artefacts immuables ; il ne
recompile pas une variante au milieu d'un lot ni ne réécrit le source à chaud.

Commande cible du futur launcher, après compilation sérialisée autorisée par
l'agent principal (aucune compilation lancée pour ce protocole) :

```powershell
node scripts/streaming-system-benchmark.cjs --core target/release/media-poc.exe --node target/release/aether-node.exe --fixture tests/fixtures/generated/dual-opus/dual-opus-long.mp4 --sessions 1,4 --rounds 20 --out target/streaming-system-baseline
```

Le launcher est `scripts/streaming-system-benchmark.cjs`, son observation DOM
dans `streaming-system-observer.cjs` et son collecteur dans
`streaming-process-metrics.ps1`. Dépendances nécessaires : binaires Core/Node
du même snapshot, build Web correspondant,
Chrome/Playwright, FFmpeg/ffprobe, PowerShell et droit d'assigner les enfants
créés aux Job Objects. Aucun compte, média ou service externe requis.

Le binaire Node de référence découpe `MEDIA_NODE_ROOTS` sur `:` et `;`, ce qui casse
les chemins absolus Windows à lettre de lecteur. Pour le banc, une racine
relative autorisée (`media`) depuis son cwd jetable est résolue canoniquement
par le code existant. Ce choix est archivé et ne représente pas une correction
de ce problème CLI. Le candidat corrige ce découpage : sous Windows, seul `;`
sépare les racines ; les lettres de lecteur sont conservées.

## Premier parcours système et correction du protocole

Le snapshot immuable `target/streaming-system-artifacts/baseline-bd08e05` contient
les deux exécutables debug existants et les assets Web copiés avant les nouveaux
changements. Son manifeste donne les SHA-256 de chaque fichier. L'association
au commit `bd08e05839963afa8c84dbcd5aca067dea7863ad` est fournie par le lancement,
pas une attestation de compilation embarquée dans ces exécutables.

Un smoke test réel a terminé : une session, une ouverture froide application/
préparation puis une chaude. Frame 338,3 / 225,0 ms ; PCM 537,7 / 247,7 ms.
Ce sont deux observations exploratoires, pas des percentiles fiables.
Sur l'ouverture froide, le Node a créé un FFmpeg : 187,5 ms CPU descendant,
78,1 ms CPU Rust, 398 047 octets de cache créés ; ouverture chaude sans nouveau
processus FFmpeg et sans nouveau fichier de préparation. Ces compteurs viennent
du Job Object du véritable Node, pas du launcher JavaScript.

Une première tentative concurrente réutilisait le compte administrateur pour
les quatre pages. Elle a révélé une réponse Node 503 transformée en Core 502,
une interruption audio et une conservation de piste ; les conflits de
progression entre les quatre pages perturbaient aussi les essais. Ce lot a été
arrêté après trois cellules concurrentes, sans présenter un run incomplet comme
référence réussie. Les chiffres et erreurs restent archivés.

Le protocole courant crée un compte `user` distinct par session via la véritable
API administrateur, lui attribue la bibliothèque et authentifie chaque contexte
séparément. Les erreurs candidates conservant l'ancienne piste sont capturées
avant qu'une notification ultérieure les masque. Aucun rejet n'est supprimé ou
relancé silencieusement. `--http-samples 100` ajoute des requêtes Range autorisées
sur les URLs vraiment utilisées par le lecteur, avec p99 HTTP distinct du clic,
du décodage et du premier PCM.
