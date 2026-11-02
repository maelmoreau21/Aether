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
| Général | Nom du serveur et message d’accueil persistants, endpoints administrateur et lecture authentifiée | Formulaire Général, navigation et message sur l’accueil |
| Réseau | Origine effective, cookies HTTPS et chemins Web/API du Core | Page Réseau en lecture seule ; transport et ports restent des réglages de déploiement |
| Image de profil | Upload/lecture/suppression du compte courant, image normalisée en PNG 256×256 | Compte, en-tête et menu profil ; formats et ressources bornés |
| Mot de passe personnel et sessions/appareils | `account.rs`, vérification du secret actuel, révocation ciblée/globale | Raccordé dans Compte le 20 octobre ; [contrat et limites](PRODUCT_FEATURES.md) |
| Journal des tâches | État de scan et dernier lancement planifié ; dates de sauvegarde | Affichage des données réellement connues ; aucune durée de scan inventée |
| Déclencheurs calendrier / plusieurs déclencheurs | Un intervalle persistant par bibliothèque ; un plan de sauvegarde | Non simulés ; nécessitent une évolution du scheduler et de ses tests de reprise |
| Journaux, cache et stockage | Pas de contrat de maintenance Web dédié | Prochain lot distinct : diagnostic borné avant commandes de maintenance |
| Fournisseurs de métadonnées et extensions | Parcours d’extensions documenté, runtime non implémenté (`EXTENSIONS.md`) | Pas de tâches LDAP, Intro Skipper ou autres intégrations fictives |

Prochains petits lots proposés, à valider après retour sur l’interface :

1. Compte et sécurité : changement de mot de passe personnel, liste/révocation des sessions.
2. Diagnostic, caches et stockage : espace disque et journaux bornés, puis commandes de maintenance distinctes des paramètres de déploiement.
3. Historique des tâches et déclencheurs calendaires : exécutions horodatées, durée mesurée,
   fuseau, reprise après redémarrage, concurrence et conservation bornée.
4. Métadonnées/extensions : uniquement après le parcours de permissions et d’exécution dédié.

Les prochains lots restent des manques de produit, pas des fonctionnalités livrées.
Contrats actuels, quatre modes d’actualisation et préparation des fournisseurs : [SETTINGS_AND_REFRESH.md](SETTINGS_AND_REFRESH.md).
Les résultats des contrôles réellement exécutés sont consignés dans MEMORY.md.

## Navigation du compte — 25 octobre 2026

Compte réunit le profil, l’image, le thème et la langue. Sécurité réunit le mot de passe et le SSO. Appareils possède sa page de sessions ; Lecture conserve les préférences du lecteur. Les anciens liens `#appearance` et `#sso` arrivent respectivement sur Compte et Sécurité. Le texte générique d’enregistrement est supprimé.

GET `/api/v1/account/avatar/policy` authentifié retourne `{uploadAllowed,removeAllowed}` sans cache. Les extensions actives d’un éditeur actif avec permission `avatars` interdisent l’import via `disallow_custom_avatars` et la suppression via `force_random_on_creation` ou `require_avatar`. Ces restrictions sont aussi appliquées à PUT/DELETE avatar, y compris pour les administrateurs. Les boutons concernés sont masqués. Le bouton de sélection apparaît pour les collections disponibles ou les extensions actives et présente les propositions derrière ce bouton.

L’en-tête administratif présente le logo puis Menu et Retour. Le tiroir contient aussi un lien logo vers l’accueil et Retour.
