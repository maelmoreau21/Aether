# Claude Code — exécution dans le dépôt Aether

Les règles produit et de sécurité communes sont dans AGENTS.md. Le travail demandé dans la conversation fait foi pour définir le périmètre.

- Détecter le shell et l'OS réellement disponibles ; ne pas supposer que toute session s'exécute sous Windows/PowerShell.
- Pour les inspections/Git, utiliser rtk **uniquement s'il est installé et compatible** avec la commande. Sinon, utiliser git et les commandes natives du shell.
- Sous PowerShell, préférer Get-ChildItem, Get-Content et les commandes git habituelles ; ne pas présumer que ls est un exécutable externe pour rtk.
- Ne pas installer rtk ni modifier le PATH uniquement pour satisfaire cette préférence.
- Charger les skills seulement lorsqu'ils concernent la tâche. Pour le design Halo, voir .claude/skills/aether-halo/SKILL.md.
- Conserver les modifications déjà présentes, utiliser des tests ciblés et ne pas poursuivre des tâches hors périmètre.
