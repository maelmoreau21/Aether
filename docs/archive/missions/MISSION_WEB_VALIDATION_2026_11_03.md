# Validation Web des parcours manuels — 3 novembre 2026

Validation sur le worktree courant, après le commit `525b61f`, avec les changements
visuels concurrents conservés. Aucun composant de présentation n'a été réécrit
pour ce lot. Le nouveau harnais est
[`scripts/browser-mission-features.cjs`](../../../scripts/browser-mission-features.cjs).

## Résultats réellement obtenus

- `npm run check` : **0 erreur, 5 avertissements** de sélecteurs CSS inutilisés
  dans `AppearanceCustomization.svelte`, issu du travail visuel concurrent.
- ESLint ciblé sur `RecommendationSettings.svelte`, `RecommendationShelf.svelte`
  et `NotificationSettings.svelte` : réussi.
- `npm run build` : réussi, avec les mêmes avertissements CSS.
- `node --check scripts/browser-mission-features.cjs` : réussi.
- `node scripts/browser-mission-features.cjs` : réussi, **Chrome 156.0.8078.80**,
  headless Windows, build Svelte de production servi uniquement en boucle locale.
- `MEDIA_UI_URL=http://127.0.0.1:5183/app/ node scripts/browser-subtitle-downloads.cjs` :
  réussi sur le même build et navigateur.

Le nouveau scénario vérifie l'activation facultative des recommandations,
l'enregistrement de la fenêtre de 30 jours avec CSRF, l'affichage d'une raison
sur l'accueil puis la disparition de la section après désactivation. Il vérifie
aussi les préférences de notifications par compte, le formulaire administrateur,
l'enregistrement explicite d'une destination webhook **désactivée**, le champ de
secret vide lors de la réouverture d'une configuration masquée et l'absence de
configuration administrateur pour le rôle `user`.

Le scénario de sous-titres vérifie recherche explicite sans téléchargement
implicite, pagination, CSRF, téléchargement, bouton de doublon désactivé,
suppression, actualisation du quota, fermeture Échap et action absente pour
le rôle `user`.

Rapport JSON et captures locales :
`output/playwright/mission-features/report.json`, `recommendations.png`,
`notifications-admin.png`, `notifications-user.png`. La capture administrateur
a été inspectée : les contrôles fonctionnels restent accessibles et aucun
secret enregistré n'est affiché. Les images ne constituent pas une revue de la
refonte visuelle concurrente.

## Portée et limites

Les API sont explicitement synthétiques et interceptées par Playwright. Ces
résultats prouvent les interactions et états du frontend compilé, **pas** les
permissions backend, la persistance Core, l'accès OpenSubtitles/TVDB, un envoi
SMTP/ntfy/Gotify/webhook, ni une lecture vidéo/audio. Les validations Core,
PostgreSQL, Wasm et lecteur sont consignées séparément dans la matrice de mission.
Aucun service externe n'a reçu de notification et aucune installation réelle
n'a été modifiée. Aucun gain de performance ou p95 n'est déduit de ce scénario.

Le harnais est réversible indépendamment du produit ; le retour arrière des
fonctions reste leur désactivation dans les préférences et réglages existants.
Les cinq avertissements de styles concurrents restent à traiter dans leur
mission propriétaire.

## 4 novembre 2026 — Préférence nouveau contenu

`NotificationSettings.svelte` expose désormais le réglage opt-in `content`.
`npx eslint src/lib/catalog/NotificationSettings.svelte` passe ; `npm run check`
passe avec **zéro erreur et zéro avertissement** dans le worktree courant.
Le scénario `scripts/browser-mission-features.cjs` enrichi vérifie aussi que le
nouveau réglage est désactivé par défaut pour un compte utilisateur, puis que
son activation est envoyée avec CSRF, sans demander la configuration admin.

Le scénario Chrome **156.0.8078.80** passe sur un **serveur Vite de développement**
et des API synthétiques interceptées, sans reconstruire le frontend pendant
le build Docker. Les interactions recommandations/destinations précédentes
passent dans le même scénario (cinq mutations). Rapport et captures :
`output/playwright/mission-content-20261104/`. Cela ne remplace ni la preuve
précédente du build de production ni les contrats Core/persistance/droits.
Le serveur de test a été arrêté ; aucun envoi externe n'a été effectué.
