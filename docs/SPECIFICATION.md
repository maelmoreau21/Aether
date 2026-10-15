# Périmètre du premier jalon P0

Document créé le 15 octobre 2026 : absent dans le dépôt initial.
Les règles `AGENTS.md`, `ARCHITECTURE.md` et `PROMPT_DEMARRAGE_IA.md` priment.

Parcours livré : générer des fichiers synthétiques AV1/Opus, inspecter leurs
pistes au démarrage, les servir en lecture seule avec une API versionnée,
observer lecture/seek/erreurs dans un navigateur et archiver les résultats.
Le serveur sert quatre fixtures initiales, quatre remux mono-audio optionnels
et quatre fixtures représentatives 10 bits/5.1 optionnelles préparées hors
lecture en P0C, explicitement enregistrés. Il n'est
pas un scanner de bibliothèques ni un serveur multimédia multi-utilisateur.

P0 doit précéder P1 autonome (SQLite/admin local), P2 multi-Node/OIDC,
P3 pistes externes/raw, P4 ergonomie films/séries, puis P5 mesures/robustesse.
Seuls `administrator` et `user` seront des rôles. Aucun de ces rôles ni compte
n'est implémenté dans le banc local. Pas de transcodage pendant la lecture.
Pas de modification des médias personnels, de publication ou de changement de licence.

Critères de ce jalon : fichiers valides, pistes AV1/Opus identifiées, remux sans
modification des paquets, HTTP testé, résultat Chrome mesuré et limites explicites.
Les phases complètes et les autres navigateurs restent ouverts.
