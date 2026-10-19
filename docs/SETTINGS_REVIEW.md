# Paramètres : état et améliorations — 19 octobre 2026

Périmètre : interface existante et endpoints effectivement présents. Les captures Jellyfin
fournies servent de référence pour le parcours liste → tâche → déclencheur, en conservant
la charte Pierre Douce / Cinéma OLED et les accents ambre-corail.

| Domaine | État vérifié dans le code | Traitement de ce jalon |
| --- | --- | --- |
| Compte, apparence, langue, lecture et SSO | Préférences par compte et liaison SSO existantes | Navigation personnelle en quatre pages ; plus de grille de cartes ni d’alerte de succès permanente |
| Synchronisation audio / sous-titres | `preferences.rs` et lecteur acceptent −30 à +30 s | Champs ajoutés dans Lecture et enregistrés via le contrat existant |
| Tâches de bibliothèques | GET/PUT/DELETE dans `scheduler.rs` | Liste compacte, page dédiée, déclencheur en minutes/heures/jours ; intervalle serveur conservé |
| Sauvegardes automatiques | `admin_backups.rs` expose état, création et planification | Tâche Maintenance ajoutée au planificateur ; configuration et archives dans Sauvegardes |
| Utilisateurs et groupes | CRUD, bibliothèques, rôles et mots de passe administrés côté Core | Groupes à côté des utilisateurs ; parcours existant conservé |
| Nodes et clés API | Fonctions déjà raccordées | Navigation existante conservée |
| Informations générales serveur | `/health` ne fournit que statut, mode, version | Pas de formulaire de configuration générale inventé |
| Mot de passe personnel et sessions/appareils | Pas de routes personnelles dédiées dans `auth.rs` | À développer avec vérification du mot de passe actuel et révocation ciblée |
| Journal des tâches | État de scan et dernier lancement planifié ; dates de sauvegarde | Affichage des données réellement connues ; aucune durée de scan inventée |
| Déclencheurs calendrier / plusieurs déclencheurs | Un intervalle persistant par bibliothèque ; un plan de sauvegarde | Non simulés ; nécessitent une évolution du scheduler et de ses tests de reprise |
| Journaux, cache, stockage et réseau | Aucun contrat d’administration Web dédié identifié dans cette revue | Prochain lot distinct : diagnostic borné avant commandes de maintenance |
| Fournisseurs de métadonnées et extensions | Parcours d’extensions documenté, runtime non implémenté (`EXTENSIONS.md`) | Pas de tâches LDAP, Intro Skipper ou autres intégrations fictives |

Prochains petits lots proposés, à valider après retour sur l’interface :

1. Compte et sécurité : changement de mot de passe personnel, liste/révocation des sessions.
2. Général et diagnostic : configuration publique explicite, chemins de caches et espace disque,
   journaux bornés, distinction entre réglages modifiables et configuration de déploiement.
3. Historique des tâches et déclencheurs calendaires : exécutions horodatées, durée mesurée,
   fuseau, reprise après redémarrage, concurrence et conservation bornée.
4. Métadonnées/extensions : uniquement après le parcours de permissions et d’exécution dédié.

Ces éléments sont des manques de produit identifiés, pas des fonctionnalités livrées.
Les résultats des contrôles réellement exécutés sont consignés dans MEMORY.md.
