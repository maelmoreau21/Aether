# Cinema Design System — 23 octobre 2026

## Direction et périmètre

La direction historique améthyste est remplacée par Cinéma OLED (sombre) et
Pierre Douce (clair). Référence artistique : règles détaillées de la demande
utilisateur ; aucune image de maquette jointe à cette intervention.
Le logo officiel `static/images/brand-mark.svg` est conservé sans modification.

L’interface privilégie les images réelles du catalogue, une navigation horizontale,
un hero contenu et des rangées sans panneaux explicatifs vides. Aucun média,
image de test ou métadonnée fictive n’est ajouté au produit.

## Source visuelle unique

`apps/web/src/lib/styles/tokens.css` définit couleurs sémantiques, états, ombres,
polices locales, espacements et rayons 6/10/12/16/20/24 px et pill 9999 px.
Fond sombre #09090B, surfaces #121215/#1E1E24 ; fond clair #EFECE6,
surfaces #FAF9F6/#E5E0D8. Le corail marque la sélection, l’ambre la lecture sombre.
Le bouton clair utilise #C2410C avec texte blanc pour préserver le contraste ;
#EA580C reste la teinte de marque. Pas de dégradé ambre/corail sur les actions.
Les interactions usuelles durent 150–220 ms et respectent reduced-motion.

Clair/Sombre/Système restent synchronisés avec les préférences du compte.
`static/theme-init.js`, chargé dans le head avant le rendu, applique le miroir
local de la préférence puis le système. La préférence authentifiée reste
prioritaire après hydratation ; CSP script-src self, aucun script inline ajouté.

## Compositions

- Header 72 px, recherche et profil, bibliothèques accessibles défilables si
  nécessaire ; logo inchangé. Administration séparée avec navigation latérale.
- Hero avec vrai backdrop lorsque disponible, sans agrandir une affiche portrait
  en faux backdrop ; rayon 24 px, hauteur desktop 350–480 px, mobile 360 px.
- Films/séries : affiches 2:3, rayon 12 px, largeur de rangée 160–190 px,
  deux affiches et amorce de la suivante sur mobile. Synopsis sur la fiche.
- Épisodes et reprise : 16:9, reprise illustrée avec l’artwork connu du catalogue,
  repli neutre si absent. Aucune section de reprise affichée sans historique.
- Musique : morceaux en lignes, durée, numéro et titre actif. Albums regroupés
  par artiste d’album et nom ; artistes regroupés depuis les résultats chargés.
  Les regroupements concernent la page courante de 100 pistes, pas toute la
  bibliothèque. Couvertures carrées et artistes circulaires avec initiale de
  repli : l’API actuelle ne fournit aucune pochette ou photo d’artiste.
  Le lecteur persiste dans le layout, conserve ses fonctionnalités et safe area.
- Playlists : formulaires et listes existants, mêmes surfaces et rayons.
- Paramètres/administration : composants existants harmonisés, logique conservée.
  Les tableaux restent fonctionnels ; aucun hero cinéma dans l’administration.

## Composants accessibles

Évaluation du 23 octobre : [Bits UI](https://bits-ui.com/docs/introduction)
fournit des primitives sans style, avec gestion clavier/focus ;
[shadcn-svelte](https://shadcn-svelte.com/docs/components) propose des composants
à adapter. Décision : garder les composants natifs existants pour cette refonte,
sans migration générale ni nouvelle dépendance. Bits UI pourra être retenu pour
un futur composant complexe démontrant un besoin. Les flèches des rangées sont
accessibles au clavier et leur disponibilité est recalculée au redimensionnement.

## Vérifications et limites

Script reproductible : `node scripts/browser-cinema-ui.cjs`, contre le build Web
servi localement sur le port 5181 (Vite preview). Captures et mesures dans
`target/cinema-ui/`. Chrome headless ; clair/sombre ; 390/768/1440/1920 px.
Le script intercepte les APIs et les images avec des fixtures de test explicites.
Cette preuve porte sur la vraie interface Svelte, pas sur un serveur NAS ou sur
la disponibilité des artworks d’une bibliothèque personnelle.

Contrôles Web : `npm run check`, `npm run lint`, `npm test`, `npm run build`
depuis `apps/web`. Aucun changement de backend ou de format média.
Les essais navigateur/PWA physiques, Safari, Android/iOS, écoute humaine et
validation sur les médias personnels restent à effectuer. Les fiches musicales
indépendantes et images d’album/artiste ne sont pas des APIs existantes : aucune
prise en charge complète de ces fonctions n’est revendiquée ici.

Résultat final : 64 états de page dans le rapport, 74 captures (albums/artistes
et paysage inclus), sans erreur JavaScript ni débordement global détecté.
Les captures desktop/mobile, catalogue/fiche série, musique et administration
ont été inspectées visuellement. Les changements Clair/Sombre/Système, les
regroupements musicaux et le changement de saison sans navigation passent.
Références sélectionnées : `docs/evidence/2026-10-23/cinema-ui/`.
Contraste calculé des CTA lecture : 8,14:1 sombre, 5,18:1 clair ; ce calcul ne
constitue pas un audit WCAG complet de tous les états et composants.

## Jalon UX du 28 octobre 2026 — navigation et découverte

Le shell utilise désormais une navigation mobile/tablette nommée jusqu’à 900 px
(Accueil, Bibliothèques, Recherche, Playlists). Les bibliothèques personnalisées
restent accessibles dans un dialogue natif défilant ; Téléchargements est accessible
dans ce dialogue et directement dans le header desktop. Le lecteur musical se
place au-dessus de cette navigation sur petit écran. La route hors ligne garde son
indépendance de la session réseau.

L’accueil privilégie reprise puis ajouts récents films/séries. Les rangées au hasard,
classiques, formats courts et genres répétant le même sous-ensemble sont retirées
de l’accueil ; ces œuvres restent dans le catalogue. Une sélection fondée sur les
notes réellement présentes peut suivre. Le hero est stable dans la page chargée,
avec vrai backdrop, logo ou titre et deux actions. Cartes simplifiées : affiche 2:3,
titre, année/note disponible, progression et états utiles. Replis d’images neutres.
Les règles de langue, variantes manuelles et rotation restent dans selectArtwork.

Audit, décisions, protocole navigateur, limites et prochains jalons :
[Refonte UX 2026-10-28](UX_REDESIGN_2026_10_28.md).
Ce jalon ne constitue pas la refonte complète des fiches ou de l’administration.

## Jalon fiches du 29 octobre 2026

Les fiches film/série utilisent `DetailHero.svelte` : logo ou titre unique,
backdrop réel seulement, poster 2:3 desktop et composition adaptée au téléphone.
La lecture précède les commandes secondaires ; les versions et l’inventaire
technique sont repliables. Les résumés longs sont extensibles, les images cassées
reviennent au texte ou à une surface neutre.

Un seul select de saison pilote les épisodes. Les lignes desktop et cartes
mobile gardent les vignettes 16:9, une lecture visible et les commandes avancées
repliables. Pagination et callbacks existants conservés. Méthode, preuves et
limites : [Fiches UX 2026-10-29](UX_DETAILS_2026_10_29.md).

## Jalon administration du 29 octobre 2026

`AdminShell.svelte` porte le cadre et une liste unique de rubriques. Sidebar
persistante à partir de 1100 px ; dialogue natif en dessous, avec fermeture fixe
et liste défilante. Fragments existants et liens de tâches conservés. Une seule
navigation est rendue à la fois ; focus et défilement sont rétablis après sélection.

La vue d’ensemble affiche d’abord serveur, Nodes, lecteurs et indexations, sur
deux colonnes sur téléphone. Les erreurs de diagnostic restent visibles et les
informations techniques secondaires restent disponibles. Méthode et limites :
[Administration UX 2026-10-29](UX_ADMIN_2026_10_29.md).

## Jalon bibliothèques du 29 octobre 2026

L’éditeur distingue six tâches : Général, Sources et dossiers, Métadonnées et
images, Indexation, Accès et Avancé. Rail desktop, sélecteur mobile/tablette et
paysage bas, contenu défilant et actions fixes. Les groupes techniques sont
repliables ; la validation révèle le champ invalide avant de lui donner le focus.
Un dossier doit être explicitement ajouté avant enregistrement. Réglages et
contrats conservés ; détails et limites : [Bibliothèques UX](UX_LIBRARIES_2026_10_29.md).

## Jalon tâches du 29 octobre 2026

La file d’opérations précède les planifications et l’automatisation, désormais
repliables. Filtres par état limités explicitement à la page chargée ; type,
bibliothèque, état et actions avant les détails techniques. Les erreurs d’action
survivent aux actualisations automatiques. Aucun état d’arrêt ou de reprise
n’est inventé ; garde du lease et contrats conservés. Preuves et limites :
[Tâches UX](UX_TASKS_2026_10_29.md).

## Jalon musique du 29 octobre 2026

Albums/artistes puis groupe sélectionné, sans répéter toute la liste des titres.
Pochettes carrées, initiales d’artistes faute de portrait, replis d’images neutres.
La portée des regroupements par page est explicite. Lecture principale visible,
file/playlist/format en actions secondaires. Barre mobile compacte au-dessus de
la navigation, file et paroles dans un dialogue natif avec transport fixe et
options de lecture. État/contrats conservés ; [preuves et limites musique](UX_MUSIC_2026_10_29.md).
