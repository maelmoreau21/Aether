# Pull request Aether

## Objectif et périmètre

<!-- Quel problème concret est résolu ? Quel comportement doit rester inchangé ? -->
**But :**

**Fichiers ou domaines concernés :**

**Issue liée (facultatif) :**

## Type de modification

- [ ] Correction de bug (reproduction et non-régression)
- [ ] Fonctionnalité (parcours fonctionnel complet)
- [ ] Refactorisation (contrats et comportement préservés)
- [ ] Documentation / instructions IA
- [ ] CI / outillage / packaging

## Changements apportés

<!-- Résumer les modifications réalisées, sans présenter un plan ou un test non exécuté comme livré. -->

## Contrats et sécurité (si concernés)

- [ ] Autorisations et rôles préservés côté Core ; aucune élévation implicite de privilèges.
- [ ] Médias personnels conservés en lecture seule ; aucune migration ou suppression de données.
- [ ] API, schémas SQL, identifiants et protocole Core/Node compatibles, ou changements explicitement décrits.
- [ ] Aucun secret, jeton ni donnée personnelle ajouté au dépôt ou aux logs.
- [ ] Comportement AV1/Opus, langues, sous-titres et HTTP Range non régressé lorsqu'il est concerné.
- [ ] Non applicable : cette PR ne touche pas ces contrats.

## Validations réellement exécutées

Indiquer les commandes **effectivement lancées**, leur environnement et leur résultat ; écrire « non exécuté » plutôt que de cocher fictivement un contrôle.

| Contrôle | Exécuté ? | Résultat / raison si non exécuté |
| --- | --- | --- |
| Rust : formatage / Clippy / tests ciblés | | |
| Web : Svelte check / lint / tests / build | | |
| Navigateur / API / Core / Media Node | | |
| Docker / plateformes / migrations | | |
| Documentation / liens / diff | | |

<!-- Pour des changements documentaires uniquement, ne pas lancer toute la suite Rust/Docker sans nécessité.
Pour les chemins critiques, inclure les tests de non-régression pertinents. -->

## Risques, limites et déploiement

**Risques et cas non validés :**

**Migrations, configuration ou intervention de production nécessaires :**

**Retour arrière :**

## Coordination avec d'autres agents

<!-- Si Claude Code/Codex travaillent en parallèle, indiquer les fichiers partagés ou conflits potentiels. -->
**Branche ou worktree de travail :**

**Changements concurrents connus :**

---

Consignes communes : [AGENTS.md](../AGENTS.md) et [docs/AI_WORKFLOW.md](../docs/AI_WORKFLOW.md). Les anciens jalons P0–P5 sont historiques, pas des cases obligatoires pour toute PR. Ne pas fusionner avant revue des changements et des contrôles adaptés.
