---
name: aether-halo
description: Règles UI et design system Halo d'Aether. À utiliser pour les changements d'interface dans apps/web, en partant des sources suivies par Git.
---

# Halo — références disponibles dans un clone propre

Le code et les documents versionnés priment sur les copies locales ou fichiers optionnels d'agents.

Avant une intervention visuelle, consulter **uniquement les références pertinentes** :
1. docs/UI_DIRECTION.md : décisions durables sur la navigation, les thèmes, les écrans et l'accessibilité.
2. apps/web/src/lib/styles/halo.css : jetons et règles CSS réellement utilisés.
3. apps/web/src/lib/styles/tokens.css : alias de compatibilité.
4. Composants réels sous apps/web/src/lib/ui/, catalog/, player/, music/ ou admin/ selon le périmètre.

Le dossier .agents/skills/aether-halo/ peut contenir des références locales supplémentaires, mais il est ignoré par Git : **son absence ne doit jamais bloquer la tâche ni conduire à inventer son contenu**.

Conserver le comportement fonctionnel et les droits ; vérifier la page modifiée en clair/sombre, desktop/mobile et clavier lorsque pertinent. Ne pas modifier l'ensemble du design system pour une correction isolée.
