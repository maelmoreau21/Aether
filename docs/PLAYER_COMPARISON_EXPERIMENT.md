# Proposition distincte — comparaison des moteurs P0C

**MSE et Shaka exécutés ; Video.js non testé. Aucun moteur définitif choisi.**
Voir [MSE initial](P0C_MSE_EXPERIMENT.md) et [comparaison Shaka](P0C_SHAKA_EXPERIMENT.md). Le protocole ci-dessous reste
la référence pour les expériences suivantes. Le périmètre reste un banc de lecture,
sans interface produit, catalogue ou authentification.

## Protocole commun

1. Geler les empreintes des fixtures `representative.mkv` et
   `representative.mp4` ainsi que des dérivés EN : AV1 Main 10 bits SDR BT.709,
   FR Opus 5.1 et EN Opus stéréo, sous-titres FR complet/FR forced/EN complet.
   Relever les versions réellement installées des navigateurs, outils et moteurs.
   Vérifier à ce moment la disponibilité de **Video.js 10** et ses contraintes ;
   documenter toute autre version effectivement utilisée, sans l'assimiler à 10.
2. Conserver un témoin HTML5 natif avec les fichiers actuels et le remplacement
   de source FR/EN. Une URL MP4 native, même fragmentée, ne prouve pas MSE.
   Pour les nouveaux fichiers, les dérivés EN se chargent actuellement comme
   sources distinctes ; ne pas leur attribuer la restauration FR/EN 8 bits
   précédemment mesurée sans la rejouer sur ces fixtures.
3. Préparer séparément un packaging AV1/Opus accepté par les outils et MSE :
   segments réels, initialisations et manifeste si nécessaire. Ne jamais remplacer
   AV1/Opus par AVC/AAC. Préserver les paquets encodés vidéo/audio par copie,
   comparer **chaque** empreinte de paquet par piste aux sources, ainsi que
   codec init, timestamps, timebases, offsets, pre-skip et mapping canaux.
   Documenter les éventuelles différences de représentation et leurs effets.
   Si aucun packaging commun ne fonctionne, consigner le blocage sans réencoder.
4. Utiliser les mêmes segments, conditions réseau et séquences d'actions pour
   trois essais : MSE minimal (`SourceBuffer`, append, erreurs et seek explicites),
   Video.js et Shaka Player. Pour Video.js, préciser la tech active et chaque
   plugin/gestionnaire de source, son rôle, sa version et ses paramètres : le nom
   du lecteur seul ne prouve pas le chemin de décodage. Pour Shaka, préciser
   également version, configuration et chemin natif ou MSE effectivement utilisé.
5. Traiter les sous-titres séparément : SRT source, mov_text existant, éventuelle
   conversion WebVTT/TTML ou extraction externe. Conserver les transformations,
   langues, forced, timings et texte. Une conversion de texte n'est pas bitcopy ;
   une extraction externe ne prouve pas le rendu d'une piste intégrée.
6. Répéter sur Chrome, Edge et Firefox disponibles avec profils par défaut.
   Éventuels réglages expérimentaux : profils et rapports séparés. Conserver
   versions, OS, paramètres, matériel et hypothèses de décodage sans extrapolation.

## Matrice à remplir avec preuves

Une ligne par navigateur × moteur × packaging × profil ; **NT** pour non testé,
échec accompagné d'une erreur reproductible. Capacité annoncée et lecture
observée restent des colonnes distinctes.

| Domaine | Mesure ou preuve demandée |
|---|---|
| Chemin réel | URL native ou MSE ; manifeste, MIME, codecs, segments et tech/plugin effectifs |
| Capacité annoncée | `canPlayType`, `isTypeSupported`, capacités du moteur ; aucune conclusion de lecture à partir de ces seules réponses |
| Copie et décodage | Hashes des paquets AV1/Opus et init ; décodage hors navigateur ; frames présentées et PCM navigateur, erreurs append/décodeur |
| Audio | Inventaire FR 5.1/EN stéréo, mapping et signal réel ; sélection via moteur, API native ou remplacement de source explicitement distingués |
| Sous-titres | Inventaire, rendu réel, langue/forced, activation/désactivation, cue après seek ; intégrés ou externes précisés |
| Transport et seek | Range, annulation, segments incomplets/absents, seek avant/arrière et limites de buffer, pause/reprise/fin |
| Changement de langue | FR → EN → FR en lecture et en pause ; position demandée/restaurée, fréquence réelle, état conservé et erreurs |
| Latence et interruptions | Préparation à froid séparée de la lecture ; demande → PCM correct et frame restaurée ; silence échantillonné et gaps vidéo, répétitions et dispersion |
| Synchronisation A/V | Flash vidéo et marqueur d'amplitude audio aux instants connus ; décalage observé, résolution de mesure et dérive après seek/changement |
| Ressources | RSS des processus identifiés, CPU et taille des buffers avec méthode ; démarrage à froid/chaud et surcoût du packaging séparés |

## Critères et risques avant décision

Un essai est positif seulement si image, signal de la langue sélectionnée,
restauration de position, pause, seek et traitement des erreurs sont observés.
Conserver les traces, répétitions et échecs. Comparer la vraie sélection des
pistes d'une présentation commune au remplacement de source actuel ; ne pas
promettre de changement sans interruption à partir d'un simple événement `playing`.
Fixer les tolérances et répétitions avant les mesures, en tenant compte des fenêtres
FFT, callbacks vidéo et délais d'échantillonnage. Les essais courts en local ne
prouvent ni le coût d'un film long, ni les performances du NAS ou d'un réseau lent.

Le packaging peut perdre dispositions, index ou informations Opus ; le MP4 actuel
expose déjà des dispositions `default` ajoutées par le muxeur. Contrôler ces
écarts au lieu de supposer que le manifeste les corrige. Changement de layout
5.1 → stéréo, politiques d'autoplay et différences MSE peuvent limiter un moteur.

Un graphe PCM/downmix ne prouve pas une sortie physique 5.1 ; un canvas ou une
frame visible ne prouve pas une chaîne d'affichage 10 bits. Prévoir écoute humaine
avec identification des six canaux et matériel d'affichage approprié dans des
vérifications séparées, sinon les marquer NT. Le choix du moteur attend ces
résultats et les compromis observés ; aucun ajout UI hors P0 n'est proposé ici.
