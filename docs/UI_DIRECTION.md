# Première direction visuelle — 15 octobre 2026

## Intention

Passer du formulaire technique autour d'une vidéo à une expérience de streaming
éditoriale : noir encre, blanc chaud, améthyste, grande composition cinématographique,
hiérarchie typographique et contrôles secondaires plus discrets. Le signe de marque
rose du dépôt est conservé. Pas de copie de navigation Netflix, de faux films,
de recherche factice ou de comptes/profils inventés.

L'écran principal est une première direction concrète, pas une expérience produit
terminée : un hero original, deux cartes des véritables éditions de fixture MKV/MP4,
un lecteur avec poster d'attente, un panneau de séance, puis un pied de page sobre.
Le visuel de paysage est signalé comme illustration d'ambiance ; le média réel reste
la mire synthétique P0. Les cartes sélectionnent réellement la source ; Charger
reste une action explicite. Les ancres Découvrir/La salle sont fonctionnelles.

## Fondations réutilisables

`apps/web/src/lib/styles/tokens.css`, importé une fois par le layout :

- Palette : canvas #09090e, surface #121219, raised #1b1b25, texte #f5f2ed,
  secondaire #a5a3b2, accent #b4a1f8, erreur #f3aaa9.
- Typographie locale : Segoe UI Variable/Segoe UI/system-ui, sans CDN ni téléchargement.
  Georgia italique pour l'accent éditorial. Titre principal 54–84 px desktop,
  47–66px mobile ; corps et textes fonctionnels distincts.
- Espacements 4/8/12/16/24/32/48/64, gouttières 20–80 px ; rayons 6/12/18 px.
- Ombre réservée à la zone de lecture, panneaux sans grille de cadres voyants.
  Boutons blanc chaud, variantes discrètes, cartes sélectionnées améthyste.
- Hover/focus/active/disabled, lien d'évitement, libellés explicites, icônes
  décoratives masquées aux technologies d'assistance et prefers-reduced-motion.

Composants Svelte légers dans lib/ui : Button, Field, Badge, Icon, MediaCard.
Ils portent uniquement la présentation et transmettent les événements. Pas de
nouvelle bibliothèque UI, police distante, store ou abstraction du lecteur ajoutée.
Les sélecteurs natifs conservent le clavier et leurs noms accessibles P1.

## Ressources et frontières

Image originale créée avec imagegen, copiée dans static/images/cinematic-horizon.png
(1 996 166 octets, 1672×941). Pas de média P0 modifié. SVG de marque existant copié
sans changement ; chevron SVG local. Tout est servi par Rust depuis le build statique.
CSP : ajout limité de img-src self pour ces images ; ni data ni domaine tiers ajouté.
Aucun changement de route, contrat API, inventaire, lecture/remux, audio, WebVTT,
cache, authentification ou persistance pour la refonte. Player/NativePlayer conservés.

## Vérification réellement exécutée

Windows 10.0.26300, Chrome 155.0.8059.40, Edge 155.0.4283.45, Firefox 155.0.1,
Playwright 1.64.1 ou geckodriver 0.37.1, headless, fixtures 12 s locales :

- scripts/browser-p1.cjs : six parcours MKV/MP4, pixels et PCM FR/EN,
  changements audio pause/lecture, position/volume/vitesse conservés, seeks
  arrière/avant EN, trois cues WebVTT/off, choix texte conservé, fin et erreur404.
  Chrome/Edge à 1,5× ; Firefox à 1×. Fichiers médias/VTT identiques avant/après.
- Limite Firefox déjà observée avant refonte : retour FR Opus 5.1 à 1,5× sans
  PCM attendu ; passage à 1× réussi. La cause précise n'est pas établie et aucune
  nouvelle récupération n'est ajoutée. Ne pas annoncer la vitesse multicanale
  Firefox comme validée. Ce point technique reste ouvert, hors refonte visuelle.
- scripts/browser-p1-layout.cjs : 360/390/768/1280/1440/1920 px, absence de
  débordement, focus clavier, images chargées, zéro violation CSP ; carte MP4
  chargeant la vraie source ; Core503/VTT404/audio404 visibles et une seule
  tentative audio. Captures desktop/mobile inspectées.
- npm run check : zéro erreur/avertissement ; lint,5 tests unitaires, build passent ;
  npm audit --audit-level=high : zéro vulnérabilité signalée.
- cargo fmt --check, cargo clippy --locked --all-targets -- -D warnings,
  cargo test --locked :10 tests intégration réussis, dont 9 P0 inchangés.
- git diff --check réussi. CI distante non exécutée.

Preuves dans evidence/2026-10-15/p1-ui-v1/. Les viewports mobiles ne prouvent pas
Safari/iOS/Android réels. Pas d'écoute humaine, de sortie physique 5.1 / 10 bits,
de film long, de déploiement NAS ou de mesure de charge.

Prochain petit jalon visuel : recueillir un retour sur cette composition et ses
proportions avant de l'étendre à d'autres écrans. L'admin local/SQLite et les
bibliothèques réelles restent des étapes P1 distinctes, non ajoutées ici.
