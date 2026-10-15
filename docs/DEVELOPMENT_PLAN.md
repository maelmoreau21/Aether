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
