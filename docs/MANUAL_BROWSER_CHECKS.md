# Vérification humaine encore nécessaire — P0

## Profil représentatif 10 bits/5.1 — aucune preuve physique obtenue

Après les trois préparations du README et redémarrage du banc, charger
`fixture-representative-mkv` puis MP4 et leurs variantes EN. Confirmer mire,
flashs/hausses d'amplitude à 2, 6 et 9 s, pause, seek avant/arrière, reprise
et absence de décalage perceptible. Noter navigateur/OS/sortie et tout réglage.
Ce contrôle ne transforme pas les mesures grossières en certification lip-sync.

Pour le 5.1 physique, identifier explicitement OS, périphérique, connexion,
ampli et six haut-parleurs ; relever le mode de sortie. Le banc automatisé
n'a qu'une destination deux canaux. La fixture FR porte simultanément les
fréquences FL/FR/FC/LFE/BL/BR 440/550/660/80/770/880 Hz : une écoute globale
ne suffit pas. Une vérification canal par canal de la sortie effective avec
matériel identifié reste nécessaire ; sans cela conserver **NT**, même si le
graphe PCM expose six voies. EN est stéréo 990/1100 Hz, pas le 880 Hz mono
des anciennes fixtures.

La source 10 bits et les pixels décodés ne prouvent pas une chaîne physique
10 bits : mode OS/GPU/écran et mesure adaptée restent à vérifier séparément.
HDR et 4K ne sont pas testés. Pour le texte, inspecter aussi les contrôles
internes natifs : le menu du banc est désactivé car l'API expose zéro piste.
Ne déclarer ni FR forced, ni cue, ni rendu réussi sans observation réelle ;
aucun sous-titre externe n'est injecté pour contourner cette mesure.

**État : non effectuée par un humain pendant ce jalon.** Le signal PCM mesuré
ne prouve ni le fonctionnement des haut-parleurs, ni le ressenti audiovisuel.
Les fixtures FR/EN sont deux tonalités synthétiques, pas des dialogues parlés.

1. Depuis la racine : `cargo run --locked -p media-poc`. Ouvrir
   `http://127.0.0.1:3000` dans le navigateur à vérifier, avec un profil de test.
   Conserver les réglages médias par défaut. Noter version complète, OS,
   sortie audio choisie et éventuels réglages non standard.
2. Vérifier que le site et les haut-parleurs ne sont pas coupés ; choisir
   `fixture-mkv`, cliquer **Charger**, puis **Lecture** dans le lecteur.
   Confirmer visuellement la mire animée et, à faible volume, un son continu.
   La piste FR par défaut émet 440 Hz. Répéter avec `fixture-mp4`,
   `fixture-webm` et `fixture-fmp4` ; noter chaque cas séparément.
3. Mettre en pause : image et compteur doivent s'arrêter, le son se couper.
   Déplacer la barre vers 4 secondes, reprendre : vidéo et son doivent repartir
   jusqu'à la fin. Noter tout silence, blocage, décalage ou message d'erreur.
4. Inspecter **Audio natif** dans le banc et les menus intégrés du lecteur.
   Sur les trois clients automatisés, l'API JS est absente et le sélecteur du
   banc est désactivé. Si aucun choix audio n'est proposé par les contrôles
   internes, noter **indisponible**, pas « réussi ». Si un choix existe,
   demander EN puis FR pendant la lecture, noter la position et vérifier
   réellement un son plus aigu (880 Hz), puis le retour à 440 Hz. Répéter
   après seek et noter si la sélection est perdue. Un libellé ou un événement
   `audio-selection-requested` seul ne prouve pas un changement audible.
5. Pour l'erreur, dans la console développeur de cette page uniquement :

   ```javascript
   const testVideo = document.querySelector('video');
   testVideo.src = '/api/v1/assets/missing/content';
   testVideo.load();
   ```

   Vérifier HTTP 404 dans le réseau, l'événement d'erreur et le texte explicite
   sur la page. Recharger ensuite une source valide et confirmer la reprise.
6. Consigner date, navigateur/version, OS, fixture, méthode `native-url`,
   image/son/pause/seek/reprise/fin, possibilités de sélection, résultat FR/EN,
   et détails des échecs. Les champs non vérifiés restent NT. Ne pas modifier
   le tableau de preuves à partir d'une attente ou de la seule présence de pistes.

Ce parcours n'utilise pas MSE. Ni les remux préparés ni une capacité
`isTypeSupported=true` ne constituent un test MSE.

## Écoute humaine du remux P0C — encore non effectuée

Préparer les dérivés avec `./scripts/prepare-language-remux.ps1` si leur dossier
n'existe pas, puis redémarrer le serveur. Ne pas régénérer les originaux.
Charger MKV ou MP4, lancer la lecture vers 1 s et sélectionner EN dans
**Langue par remux préparé** : vérifier réellement 440 → 880 Hz, le retour FR,
et la position conservée. Noter silence, gel ou flash d'une image au début
du fichier. Répéter après seek. À 4,25 s en pause, changer EN puis FR : la
position doit rester identique sans reprise automatique ; reprendre et écouter.
Cette écoute est distincte des 48 changements automatisés réussis et reste NT
jusqu'à témoignage effectif. Le fichier est remplacé avec interruption possible ;
ce n'est pas une sélection interne transparente.
