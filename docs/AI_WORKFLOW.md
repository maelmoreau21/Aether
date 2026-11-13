# Workflow de développement avec Claude Code, Codex et autres agents

Ce document décrit une méthode de collaboration, pas une nouvelle tâche à exécuter automatiquement. Les invariants applicables à tout le dépôt sont dans [AGENTS.md](../AGENTS.md).

## Démarrage d'une mission

1. Identifier la demande exacte, la branche/commit, git status et les modifications non commitées.
2. Limiter la lecture aux fichiers, tests et documents de ce domaine. Ne jamais lire toutes les archives par défaut.
3. Définir une unité de travail livrable, les fichiers concernés, la preuve attendue et les risques.
4. Distinguer ce qui existe dans le code, ce qui est simplement documenté et ce qui reste hypothétique.
5. Vérifier les outils disponibles avant de choisir des commandes ou une stratégie de test.

**La demande actuelle prime sur toute ancienne roadmap.** Les textes P0, P1 et « prochaine étape » datés ne sont pas des instructions permanentes.

## Exécution par petits lots

- Préférer une correction complète et vérifiée à dix changements partiellement intégrés.
- Pour un bug : reproduction → cause → correctif minimal → non-régression.
- Pour un refactoring : interfaces et invariants → extraction limitée → tests → revue du diff.
- Pour l'interface : composants et parcours existants → rendu/accessibilité → API réellement branchée → tests navigateur.
- Pour les fonctions de sécurité, la base, le lecteur et le protocole Core/Node : conserver les contrats et faire examiner les changements à risque.
- Ne pas changer de stack, reconstruire un composant ou ajouter des fonctionnalités hors demande.
- Ne pas utiliser de longs tests distants ou de déploiement réel sans besoin et autorisation.

## Agents parallèles

- Deux agents travaillant simultanément ne doivent pas éditer le même worktree ou les mêmes fichiers sans coordination.
- Préférer une branche et un worktree distincts par mission, dérivés d'une base connue.
- Ne pas écraser, réinitialiser ou nettoyer des modifications préexistantes.
- Intégrer par PR distinctes et relire le diff de chaque lot ; ne pas fusionner des refactorings non liés.
- Changer de conversation réinitialise le dialogue, **pas** les fichiers d'instructions ni le worktree.

## Contexte et outils

- Commun à tous : AGENTS.md, état Git et code concerné.
- Design : docs/UI_DIRECTION.md et styles dans apps/web/src/lib/styles/.
- Historique : docs/archive/ uniquement pour retrouver un contexte passé sur demande.
- Tests : préférer les commandes locales pertinentes ; distinguer tests synthétiques, système, navigateur et matériel physique.

## Livrable

Chaque mission se termine avec un rapport bref : modification réelle, fichiers changés, commandes/tests exécutés et résultats, vérifications non réalisées, risques éventuels. Ne pas déclarer terminé un travail uniquement parce qu'il compile ou qu'un vieux rapport l'affirme.

Aucun push, création de tag, publication, déploiement ou intervention sur les volumes/médias réels sans autorisation explicite.
