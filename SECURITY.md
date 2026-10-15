# Politique de sécurité

## Versions prises en charge

Le projet est actuellement en phase active de développement (jalons initiaux P0/P1). Seule la branche principale est suivie activement pour les correctifs de sécurité :

| Version / Branche | Prise en charge |
| ----------------- | --------------- |
| `main`            | :white_check_mark: |
| Releases futures  | :white_check_mark: |

---

## Signalement responsable de vulnérabilité

Nous prenons la sécurité très au sérieux. Si vous découvrez une faille de sécurité ou une vulnérabilité potentielle dans ce projet, **merci de NE PAS ouvrir d'issue publique**.

### Procédure de signalement privé

1. **Via GitHub Private Vulnerability Reporting (recommandé) :**
   - Rendez-vous sur l'onglet **Security** du dépôt GitHub.
   - Cliquez sur **Report a vulnerability** pour ouvrir un rapport privé et confidentiel avec les mainteneurs.

2. **Par e-mail privé :**
   - Si la fonctionnalité GitHub n'est pas disponible, contactez directement les mainteneurs via les coordonnées indiquées sur leur profil GitHub de dépôt.

### Informations utiles à inclure

Pour nous aider à analyser et corriger rapidement le problème, veuillez fournir :
- La description claire de la vulnérabilité et de son impact potentiel ;
- Les étapes reproductibles (PoC minimal, requête HTTP, payload) ;
- L'environnement testé (système d'exploitation, version du binaire, navigateur) ;
- Toute suggestion de correctif si vous en disposez.

### Notre engagement de réponse

- **Accusé de réception** sous 48 heures ouvrées ;
- **Évaluation technique** et échange confidentiel pour convenir d'un correctif ;
- **Publication coordonnée** : un avis de sécurité et un correctif seront publiés dès validation.

---

## Modèle de menaces et conception technique

Pour plus de détails sur le modèle de menaces, le confinement en lecture seule, les restrictions d'accès HTTP et les garde-fous architecturaux implémentés dans Aether, consultez le document technique dédié :
👉 [`docs/SECURITY.md`](docs/SECURITY.md).
