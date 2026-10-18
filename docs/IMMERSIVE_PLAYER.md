# Lecteur immersif P1 — 15 octobre 2026

## Périmètre

L'identité et la composition de l'accueil sont conservées. Seul l'ancien bloc
lecteur/panneau permanent est remplacé par une fenêtre immersive. Les liens
Ouvrir le lecteur / La salle ouvrent un dialog modal couvrant la fenêtre et
demandent le vrai plein écran depuis le geste utilisateur. Refus/API absente :
mode fenêtre complet maintenu, message explicite. Aucune navigation de route.
Retour : lecture arrêtée, source/pistes retirées, listeners du moteur détruits,
scroll et focus d'origine restaurés après la sortie du plein écran.

## Architecture et interactions

- ImmersivePlayer.svelte : composition, cycle de vie, plein écran, focus,
  préférences et raccourcis ; reçoit une PlayerFactory. NativePlayer inchangé.
- PlayerControls.svelte : contrôles visuels uniquement ; PlaybackState et
  callbacks typés, aucun import du moteur natif ni détail remux/Shaka/MSE.
- track-labels.ts : langue, codec, disposition/canaux et résolution réels.
  Aucun codec, canal ou rôle absent n'est inventé.
- Vidéo HTML5 sans controls, fond noir et object-fit contain ; overlay haut
  Retour/titre, bas progression/lecture/volume/muet/temps et menus flottants.
- Audio et WebVTT/off réellement disponibles. Choix manuels préservés lors
  d'un changement de source et de langue ; langue conservée à la réouverture.
  Les choix FR/EN restent des copies mono-audio préparées, avec interruption.
- Menu Vidéo et sources : pistes de la source active en lecture seule ;
  sélection interne explicitement indisponible. MKV/MP4 sont deux sources de
  démonstration, pas deux œuvres, encodages ou versions éditoriales inventés.
  Changement de source : position, pause, volume, vitesse et texte conservés.
- Masquage après 3 secondes en lecture, curseur caché ; pause/menu/erreur/
  interaction clavier maintiennent les commandes. Mouvement/toucher les révèlent.
  Menus : flèches/Home/End, Entrée, Échap, Tab et retour du focus au déclencheur.
- Espace, gauche/droite ±5s, haut/bas ±5%, M, F et Échap ; inputs et menus
  gardent leurs propres touches. Échap peut d'abord être consommé par le
  navigateur pour sortir du plein écran ; le mode immersif reste utilisable.
- Zones de sécurité, cibles tactiles 44px, menus défilants, mouvement réduit.
  Aucun panneau latéral, contrôle natif vidéo, dépendance majeure ou API ajoutée.

## Commandes de vérification

Depuis la racine, serveur Rust existant et fixtures P0/P1 préparées :

```powershell
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm test --prefix apps/web
npm run build --prefix apps/web
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
$env:NODE_PATH = (Resolve-Path apps/web/node_modules).Path
node scripts/browser-immersive.cjs
node scripts/browser-p1.cjs
node scripts/browser-p1-layout.cjs
```

Les scénarios P1 existants gardent leurs assertions PCM/pixels/position/texte,
mais pilotent les nouveaux menus au lieu des anciens select. La comparaison de
position est prise au clic du choix audio, après l'ouverture du menu. Les retours à une position sont observés à seeked, séparément de la
progression normale pendant la reprise. La petite fixture est repositionnée
avant le changement en lecture pour laisser du temps au contrôle PCM. Chaque
cas sélectionne explicitement FR avant sa mesure, puisque le nouveau lecteur
conserve maintenant la langue choisie à la réouverture.
Edge : MEDIA_BROWSER_CHANNEL=msedge, MEDIA_BROWSER_EXECUTABLE selon TEST_PLAN.
Firefox : MEDIA_BROWSER_CHANNEL=firefox, MEDIA_TEST_SPEED=1 ; scénario UX dédié
scripts/browser-immersive-firefox.cjs avec geckodriver existant et vraies touches
W3C WebDriver. Les résultats finaux sont consignés dans MEMORY.md et les preuves.

## Limites conservées

Fixtures de 12 secondes et navigateurs Windows headless ; aucun résultat NAS,
film long, sortie physique 5.1/10 bits, Safari ou appareil mobile réel déduit.
Firefox multicanal à 1,5× reste une limite préexistante ; validation à 1×.
Pas de sélection de plusieurs vidéos internes ni de catalogue de vraies versions.
Pas d'authentification, de nouvelle bibliothèque, de transcodage ou de backend.

## Résultats UX observés

Chrome155.0.8059.40 et Edge155.0.4283.45 : ouverture avec vrai fullscreen,
repli simulé refusé, masquage/reprise des commandes, clavier, menus, source
MKV→MP4 avec pause/position/EN/texte, retour avec readyState0/source absente,
focus restauré, choix langue à la réouverture ; tactile simulé390×844/844×390.
Firefox155.0.1/geckodriver0.37.1 : vrai fullscreen, masquage, audio/texte,
source/position/pause, vraies touches WebDriver, Échap/menu, nettoyage et refus
fullscreen simulé passent. Layout360–1920px : zéro débordement/CSPerror et
pannes Core/audio/VTT explicitement traitées, pas de retry audio automatique.
8tests Vitest et10tests Rust passent, check/lint/build stricts réussis.
Preuves : evidence/2026-10-15/immersive-player/.

La précision du seek restauré est mesurée avant la progression normale de la
lecture. Les essais Edge multicanaux à1,5× ont aussi rencontré des échecs de
position/PCM sur cette petite fixture ; ne pas revendiquer leur fiabilité.
La vitesse normale fait l'objet d'un rapport distinct. Aucun contournement
codec, transcodage ou nouvelle récupération n'est ajouté pour masquer ces limites.

Validation média finale : Chrome à1,5×, Edge et Firefox à1×, deux conteneurs
chacun : pixels/PCM, audio/texte/position, pause/seek/fin/404 et intégrité des
médias passent. Les essais accélérés Edge non concluants restent une limite.
Les dix tests Rust P0/P1 et huit tests frontend sont réussis, sans modification
Rust pour ce jalon. Les scénarios UI existants ont été conservés et adaptés aux
menus réels, avec les contrôles de précision et de signal audio maintenus.
