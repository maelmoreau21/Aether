# Demandes du 20 octobre 2026

Suivi demandé par l’utilisateur. Une ligne ouverte ne constitue pas une fonction livrée.
L’utilisateur a confirmé l’exécution de l’ensemble sans arrêt entre les lots.
Conserver les rôles `administrator` et `user`. État de travail : `../DEV_STATE.md`.

1. **PWA** : manifeste, icônes, lancement autonome, service worker réseau uniquement
   et message de perte de connexion. Vérifier l’installation sur appareil réel.
   HTTPS requis hors localhost ; HTTP local existant reste utilisable dans un onglet.
   La PWA ne fournit pas encore de téléchargement hors ligne.
2. **Bibliothèques musicales** : définir artistes/albums/pistes, scan audio et
   permissions, catalogue et lecture audio réels ; ne pas recycler un film en album.
3. **Lecture AV1 + Opus** : reprise ultérieure des essais lecteur MKV/MP4,
   seek/pause/Range et pistes multiples avec preuves navigateur.
   **Correction à conserver : nettoyage du cache temporaire après crash**.
   Reproduire un arrêt brutal pendant la préparation, isoler les répertoires
   applicatifs orphelins, protéger les travaux actifs et tester le redémarrage.
   Aucune suppression globale de dossiers temporaires ou de médias.
4. **Compte personnel** : changement du mot de passe avec vérification du secret
   actuel ; liste des sessions/appareils avec identité explicite, révocation
   individuelle et globale, y compris refus effectif des ressources après révocation.
5. **Multi-Node** : identité globale et regroupement explicite des copies d’un film,
   choix du serveur et de la version disponibles, état hors ligne et préparation
   distante des pistes. Valider permissions/révocation/Range en direct et relais,
   puis sur deux machines. Ne pas fusionner sur le seul titre.
6. **Restrictions parentales** : politique serveur, filtrage catalogue, accès direct
   et lecture ; comportement défini lorsque la classification est absente.
7. **Préférences de pistes entre appareils** : auditer la synchronisation actuelle,
   définir la priorité du choix manuel et tester deux sessions du même compte.
8. **Choix automatique de version** : codecs réellement décodables, qualité et
   disponibilité ; permettre le choix manuel et expliquer une incompatibilité.
9. **Chapitres vidéo** : timestamps réels, navigation clavier et seek vérifié.
10. **Filtres** : genre, année, qualité, codec et langue ; contrats/pagination cohérents.
11. **Métadonnées et affiches** : édition depuis l’interface, identification et
    correction manuelles, permissions et persistance applicative sans modifier
    les médias ni les fichiers Jellyfin.
12. **Listes personnalisées / watchlists** : propriété, isolation des comptes,
    ajout/retrait et persistance ; distinguer des favoris déjà existants.
13. **Téléchargements hors ligne** : jalon distinct ; quotas, annulation, suppression,
    confidentialité sur appareil partagé et limites de révocation hors connexion
    à définir avant le stockage de médias privés.

Ordre proposé : PWA → musique → comptes → multi-Node, avec le correctif cache
suivi explicitement et les autres demandes découpées en parcours vérifiables.
