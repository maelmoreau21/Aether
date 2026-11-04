# PROMPT_DEMARRAGE_IA.md — Première tâche à donner à l'agent

Tu travailles sur **Aether** (nom de code provisoire), une plateforme multimédia open source qui **n'a pas encore commencé**. Ton rôle est d'implémenter progressivement, avec des tests réels et une transparence totale sur les limites.

## Avant de coder

Lis **dans cet ordre** :

1. `AGENTS.md`
2. `MEMORY.md`
3. `ARCHITECTURE.md`
4. `docs/SPECIFICATION.md`
5. `docs/MEDIA_FORMATS.md`
6. `docs/STREAMING_COMPATIBILITY.md`
7. `docs/TEST_PLAN.md`
8. Les autres documents `docs/` pertinents.

Vérifie l'état réel du dépôt (`git status`, structure existante, outils disponibles). Considère tout composant comme **non implémenté tant que tu ne l'as pas vérifié**. Ne demande pas d'inventer des tests ni ne prétends avoir réalisé un test navigateur si tu ne peux pas lancer de navigateur.

## Mission autorisée MAINTENANT — P0 uniquement

Construis **un démonstrateur minimal et reproductible** permettant de déterminer comment lire des fichiers **AV1 vidéo + Opus audio dans des conteneurs MKV et MP4**, avec Rust comme backend et un lecteur Web minimal si nécessaire.

- Génère ou documente la génération de **petits médias synthétiques sous licences compatibles** : `AV1+Opus.mkv` et `AV1+Opus.mp4` qui sont vraiment valides. Conserve commandes, versions, métadonnées, checksums. **Ne télécharge aucun film protégé**.
- Probe le conteneur et les pistes : nombre, type, codecs/profils, langue, durée, index et toutes les informations disponibles.
- Teste la lecture directe sur navigateurs ciblés et note précisément les limites. Si échec, fais une expérience **de remux/packaging sans réencoder** (fMP4/WebM/DASH/HLS suivant les compatibilités et contraintes), pas un faux manifeste DASH contenant du MKV brut.
- Vérifie `HEAD`, `Range/206/416`, seek, erreurs et mémoire bornée.
- Compare Video.js 10 et Shaka uniquement sur une même fixture et une grille de tests objective ; ne choisis pas arbitrairement avant les résultats.
- Prépare la prise en charge **de pistes vidéo/audio/sous-titres multiples**, mais **ne commence pas** le format raw sans conteneur : ce sera P3 après P0/P1/P2.
- Remplis `docs/STREAMING_COMPATIBILITY.md` avec **uniquement les résultats observés** et laisse `NT` pour le reste.
- Mets à jour `MEMORY.md` avec ce qui a réellement été fait et testé.

## Contraintes produit à respecter

- **Seulement deux rôles** quand l'auth arrivera : `administrator` et `user` ; **pas de `guest`**.
- **Mode `standalone` sera prioritaire** pour le vrai produit ; multi-Node ensuite et multi-Core **seulement plus tard**. Garde des frontières Web/Core/Node claires dans le design, sans créer tous les modules dès P0.
- Les médias du propriétaire restent montés **read-only** et Jellyfin continue de fonctionner à côté ; aucune opération de modification/conversion destructive.
- Ne rends pas obligatoire le transcodage vidéo et ne suppose pas que tous les navigateurs acceptent MKV/MP4 AV1/Opus.
- La demande future de **pistes séparées réellement sans conteneur** existe ; garde le modèle d'assets extensible, mais **ne conçois pas un format raw propriétaire maintenant**.
- Le nom « Aether » doit pouvoir changer : pas de chemins API ou variables hardcodés à ce nom.

## Résultat attendu de ta PREMIÈRE intervention

1. Un plan **court** du P0 et des hypothèses.
2. Un prototype ciblé, avec instructions reproductibles et tests (pas une usine à microservices).
3. Un récapitulatif **MKV vs MP4** AV1+Opus, démontrant ce qui marche vraiment, ce qui échoue et sur quel navigateur/client.
4. Les commandes exactes de génération et de test exécutées avec leurs résultats ; toute impossibilité doit être déclarée.
5. La documentation de compatibilité mise à jour ; **aucune fonctionnalité inventée ni résultat présumé**.
6. La prochaine **petite** tâche recommandée, sans partir directement sur le multi-Core, l'UI Netflix ou des plugins.

**Règle de décision :** si tu découvres un obstacle fondamental de compatibilité MP4/Opus, ne l'escamote pas ; documente-le et propose une voie testée avant de coder le reste.
