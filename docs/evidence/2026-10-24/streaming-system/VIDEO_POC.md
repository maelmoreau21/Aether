Ces essais vérifient le décodage et la continuité du lecteur de production avec
une fixture synthétique AV1 10 bits 320 × 180, 12 images/s, GOP de 15 secondes,
et Opus. Ils utilisent Chrome headless et un serveur HTTP JavaScript local qui
lance FFmpeg en copie vidéo. Ce serveur ne remplace pas une preuve du Node Rust,
des permissions Core, de deux machines physiques, du NAS ou du réseau réel.

Les fenêtres audio sont explicitement préparées avant lecture. Le cache disque
de l’OS est incontrôlé. Les durées de seek du rapport sont des diagnostics ;
ces essais ne constituent pas une comparaison de vitesse ni un démarrage froid.
Le PCM du graphe WebAudio du test et les horloges HTML5 ne prouvent pas une sortie
physique multicanal ni la synchronisation perceptuelle de tout fichier arbitraire.

Les trois rapports `poc-video-windows-*-failed.json` conservent les échecs réels
du prototype à fenêtres vidéo indépendantes et chevauchantes :

- `overlap-no-offset` : première implémentation, sans correction des éditions MP4.
- `parser-reset-no-offset` : réinitialiser le parseur MSE ne corrige pas la timeline.
- `correct-offset-overlap` : la plage MSE atteint bien 30 s, mais le décodage
  s’arrête vers 19,9 s après remplacement d’un GOP déjà en lecture.

Ces rapports historiques précèdent l’instrumentation des SHA des modules.
Leurs sources exactes ne sont donc pas attestées par le hachage du code actuel.
Ils documentent l’abandon du prototype ; ils ne prouvent aucune livraison de ce
chemin dans le produit.

`poc-native-progressive-final-gop15-passed.json` exerce NativePlayer et son helper
ProgressiveVideo avec le flux FFmpeg continu, les lectures réseau par blocs,
la limite des données encodées, le préchargement audio et l’identité explicite.
Un original HTTP 404 provoque réellement le repli, puis les seeks 31 → 2 s
interrompent et remplacent le flux sans remplacer la source MSE. Les frontières
10/20/30 s vérifient les frames, les événements de pause et le PCM.
Le JSON contient les SHA-256 du TypeScript et du JavaScript effectivement
transpilés, les versions des outils, les probes et les SHA des fixtures calculés
après les mesures. Les identifiants et médias sont synthétiques ; aucun jeton
ou identifiant utilisateur réel n’est archivé.

Reproduction depuis la racine du dépôt, avec Chrome installé, FFmpeg/ffprobe
et les dépendances Web disponibles :

```powershell
node scripts/browser-progressive-player-poc.cjs
```

Si les fixtures manquent, le script prépare les fichiers synthétiques par
`browser-video-window-poc.cjs` en mode `MEDIA_VIDEO_POC_PREPARE_ONLY=1`, puis
lance le lecteur. L’encodage AV1/Opus sert uniquement à créer cette fixture.
Le repli vidéo testé copie les paquets avec `-c:v copy` et n’écrit aucun remux
vidéo complet avant la lecture.

Les fichiers détaillés et les sorties de travail se trouvent sous
`target/streaming-video-window-poc/`. L’exécution demande environ une minute de
lecture réelle pour les deux conteneurs ; le temps dépend de la génération
initiale et de la charge de la machine.
