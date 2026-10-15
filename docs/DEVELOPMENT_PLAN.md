# Découpage réel

1. Premier jalon P0A/P0B : banc Rust, quatre fixtures, probe des pistes,
   HTTP Range et lecture native Chrome ; résultats dans la matrice. Livré
   avec limites documentées, pas de clôture des phases complètes.
2. Deuxième jalon : lecture native sur Edge/Firefox vérifiée, Chrome rejoué.
   Sélection audio via API native indisponible avec leurs réglages par défaut ;
   contrôles humains encore ouverts selon `MANUAL_BROWSER_CHECKS.md`.
   Remux par langue maintenant testé : changement de source, position/pause
   restaurées et mesure 440/880 Hz sur les trois clients. Interruptions observées,
   comparaison à la vraie sélection interne/MSE encore ouverte. Suite
   10 bits/FR 5.1/EN stéréo testée sur les trois clients ; sous-titres intégrés
   détectés mais non exposés par les API natives. Voir `P0C_REPRESENTATIVE_MEDIA.md`.
3. MSE minimal puis Shaka DASH exécutés sur les mêmes flux copiés :
   voir P0C_SHAKA_EXPERIMENT.md. Choix définitif ouvert ; prochain essai ciblé
   sur safeMargin et clips longs, puis Video.js dans son jalon distinct.
   Plusieurs vidéos, index/offsets approfondis et preuves physiques restent ouverts.
4. Seulement après preuves P0 : P1 autonome, puis P2 Nodes/OIDC, P3 pistes
   externes et essences raw, P4 expérience, P5 robustesse. Multi-Core/plugins
   restent hors MVP conformément à `AGENTS.md`.

## Décision officielle du 15 octobre 2026 — P1 autorisé

Les recommandations historiques P0 ci-dessus ne sont plus la prochaine action.
Suspendre les comparaisons et récupérations expérimentales. HTML5 natif + copies
par langue retenus provisoirement ; interruption/rechargement acceptés. Mauvaise
langue silencieuse, modification d'original, transcodage vidéo obligatoire et
boucles de récupération refusés. Première intégration Web locale décrite dans
P1_WEB_INTEGRATION.md ; elle ne clôture ni P0 exhaustif ni P1 autonome complet.
Prochain jalon : admin local/SQLite ; bibliothèques ro après autorisation serveur.
