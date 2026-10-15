# Plan du jalon média 10 bits/5.1 — suivi

Le jalon est maintenant exécuté sur AV1 10 bits SDR, FR Opus 5.1 et EN stéréo,
avec trois sous-titres intégrés : voir `P0C_REPRESENTATIVE_MEDIA.md` et la
matrice. Ce plan initial ne vaut pas preuve de tout son périmètre : EN 5.1,
FR SDH, restitution physique et rendu des sous-titres restent non testés.
La suite proposée, non exécutée, est `PLAYER_COMPARISON_EXPERIMENT.md`.

## Fixture synthétique proposée

- Une vidéo AV1 Main 10 bits, `yuv420p10le`, SDR explicitement balisée BT.709,
  mire animée avec compteur temporel, points clés connus et durée permettant
  plusieurs changements de langue et seeks. HDR et 4K restent des essais distincts.
- Deux audios Opus 48 kHz 5.1, FR/EN, avec tonalités et séquences identifiables
  par canal, marqueurs synchronisés à l'image et fréquences différentes par langue.
  Déclarer précisément le layout, channel mapping family, ordre des canaux,
  `pre-skip`, gain, timestamps et éventuel décalage : `channels=6` seul ne suffit pas.
- Sous-titres synthétiques FR forcés, FR SDH et EN complets, avec textes et
  horodatages distincts. Matroska peut porter SubRip/ASS ; choisir ensuite un
  format MP4 réellement accepté par le muxeur et conserver les limites observées.
  WebVTT externe constitue un autre scénario, pas une preuve du support intégré.
- Générer une seule source légale, conserver les originaux et remuxer avec
  `-c copy` lorsque possible. Une conversion de sous-titres doit être identifiée
  séparément et ne doit jamais être décrite comme une copie bit à bit.

## Critères de preuve avant toute conclusion

1. Vérifier les versions et options des outils disponibles avant génération.
   Conserver les arguments, probes complets, hashes des fichiers et tous les
   paquets encodés ; comparer vidéo et audios après chaque remux.
2. Décoder chaque piste hors navigateur, vérifier réellement 10 bits, les six
   canaux et leurs signaux/ordres. Un downmix stéréo navigateur peut confirmer
   la présence du son, jamais une restitution physique 5.1.
3. Rejouer sur Chrome, Edge et Firefox : URL native séparément du packaging/MSE,
   image décodée, signal audio, pause, seek avant/arrière, début/fin et erreurs.
   Consigner OS, navigateur, GPU/décodage logiciel ou matériel et format exact.
4. Tester FR → EN → FR à position connue, y compris après pause et seek.
   Mesurer la restauration de position, la reprise vidéo et la reprise audio,
   vérifier la fréquence de la langue et rechercher les désynchronisations
   par marqueurs. Comparer source remplacée et vraie sélection dans un même asset.
5. Vérifier séparément l'inventaire et le rendu des sous-titres : langue,
   forced/SDH, activation/désactivation, seek et synchronisation. Une piste
   listée par ffprobe n'est pas une piste rendue par le navigateur.
6. Réaliser une écoute humaine avec sortie 5.1 identifiée et mire de canaux ;
   documenter la procédure et les résultats. Sans matériel approprié, marquer
   la restitution 5.1 non testée, même si décodage et downmix passent.

## Périmètre et limites

La préparation n'ajoute ni catalogue, ni base, ni interface complète, ni
transcodage pendant lecture. Ne pas activer de fonctions expérimentales sans
les distinguer des réglages par défaut. Les fichiers courts et le réseau local
ne prouvent ni le comportement d'un film long ni le coût sur NAS. Sous-titres
images, HDR, objets audio, plusieurs vidéos et formats raw restent hors de cet
essai ; chaque limite de muxeur, décodeur ou navigateur doit être consignée.
