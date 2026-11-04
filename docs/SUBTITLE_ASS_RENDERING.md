# Rendu ASS/SSA — 2 novembre 2026

## Correctif de transition en pause et CI — 3 novembre 2026

Avant de détacher une piste texte native, le lecteur la désactive explicitement.
Cela évite qu'un ancien sous-titre reste incrusté sur une image en pause sous le
nouveau canvas ASS. Le scénario scanner contrôle maintenant les pixels libass,
le décalage en pause, le nettoyage et le repli WebVTT séparément. Il attend la
fin d'initialisation du renderer, pas seulement son canvas provisoire.
Résultats MP4/MKV, commandes et limites : [bilan P0](archive/ci/CI_P0_REPAIRS_2026_11_03.md).

Les sidecars ASS/SSA conservent désormais leur texte original dans l'instantané
du Node, avec un repli WebVTT échappé. Les anciennes observations restent
lisibles ; un nouveau scan incrémental récupère l'original. La limite par fichier
reste de 1 Mio et les deux représentations comptent dans le plafond de 16 Mio
par vidéo.

Le descripteur expose `assUrl` seulement quand un original est disponible.
`GET/HEAD /api/v1/assets/{id}/subtitles/{index}?format=ass` reprend session,
permissions vivantes, contrôle parental et Range de la route existante. La
réponse est `text/plain`, privée et non mise en cache ; aucun chemin disque
n'est exposé. L'URL sans paramètre continue de servir du WebVTT.

Les pistes intégrées ASS/SSA exposent également une préparation
`subtitle-ass-{index}`. Le Node propriétaire extrait un script ASS avec FFmpeg,
sans réencodage vidéo, depuis son handle confiné. Le plafond de sortie est de
4 Mio, comme pour WebVTT. Le Core vérifie codec, index et politique des pistes
intégrées avant toute exécution locale ou tout relais vers un Node distant.

Le lecteur charge JASSUB/libass 2.5.23 à la sélection d'une piste originale,
dans un worker. Le module, les Wasm et la police de repli restent locaux. Les
Wasm/workers sont exclus du préchargement du service worker ; aucune recherche
de police distante n'est activée. Les limites de cache libass sont de 64 Mio
et 16 Mio pour les glyphes ; elles ne sont pas une mesure de RSS totale.
La CSP autorise `wasm-unsafe-eval` pour ce moteur, sans autoriser l'évaluation
de JavaScript arbitraire. La prise en charge exige Worker, OffscreenCanvas et
WebAssembly ; un échec d'initialisation revient au texte simple avec un message.

Les décalages de sous-titres existants sont appliqués au rendu libass, y compris
sur une image en pause. Les sélections périmées et la destruction du lecteur
libèrent leur renderer. Le délai d'initialisation est de dix secondes, avec
destruction bornée du worker en cas d'absence de réponse.

La police de repli garantit un texte lisible ; les polices jointes aux conteneurs
ne sont pas encore extraites, donc une typographie identique à la source n'est
pas garantie. Les réglages de style WebVTT ne remplacent pas les styles du script
ASS. Ce lot ne change pas les règles de sélection des forcés et SDH.

Le téléchargement OpenSubtitles et les téléchargements automatiques sont
reportés à la demande de l'utilisateur. Aucune clé ni requête fournisseur de
sous-titres n'est nécessaire à ce parcours.

Le scénario `node scripts/browser-ass-renderer.cjs` utilise le bundle de
production, Chrome, une vidéo AV1 synthétique et un script ASS avec couleur,
position et rotation. Il compare le rendu, vérifie le décalage en pause et la
suppression du canvas. Il ne prouve pas la compatibilité de tous les clients,
la fidélité de polices absentes, le NAS ou un fournisseur externe.
