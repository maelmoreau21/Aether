# Exécution du prototype

Ce jalon s'exécute localement via les commandes du README. Aucun Compose,
conteneur, déploiement Synology ou service public n'est livré ni testé.
Pas de DB, PostgreSQL, Valkey, OIDC, proxy HTTPS ou Node.js permanent nécessaire.
Les bibliothèques personnelles/Jellyfin ne doivent pas être utilisées par ce banc.

Le futur standalone placera les bibliothèques sur volumes `:ro`, les données,
caches et miniatures séparément. L'administration locale sécurisée précédera
toute exposition ; HTTP local et cookies/SSO devront être documentés et testés
avant P1/P2. Aucune migration ni conversion de médias existants n'est effectuée.
