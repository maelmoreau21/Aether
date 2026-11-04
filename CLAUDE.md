# Instructions d'exécution des outils CLI

- Toujours préfixer les commandes d'inspection et Git avec `rtk` (ex. `rtk git status`, `rtk git diff`, `rtk git log`).
- Système d'exploitation : Windows (PowerShell).
- Ne pas exécuter `rtk ls` directement car `ls` n'est pas un binaire natif dans le PATH Windows. Utiliser `rtk git ls-files` ou `rtk cmd /c dir` pour lister les fichiers.