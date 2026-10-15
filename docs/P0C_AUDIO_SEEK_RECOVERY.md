# P0C — sélection audio après seek (15 octobre 2026)

## Conclusion et périmètre

La conservation de l'ancienne langue dans les buffers est reproduite. Une
correction fiable de la lecture après seek **n'est pas démontrée**. La purge
publique de variante constitue un **contournement expérimental**, désactivé
par défaut dans `/shaka`. La marge normale reste 0 ; MSE et Shaka 5.2.13 restent
les références. Aucun média, packaging ou codec n'a été modifié.

## Cause établie et incertitude restante

Dans les sources locales épinglées 5.2.13, `selectAudioTrack(track, margin)`
conserve les anciens paquets avant `currentTime + margin`. Un seek dans cette
partie peut rejouer l'ancienne langue, bien que la variante active indique la
nouvelle. Resélectionner la piste active avec marge 0 ne force pas sa purge.
Les deux représentations DASH `audio-fr` et `audio-en` ont des identifiants
distincts. Le `track_ID=1` propre à chaque fichier indépendant n'établit pas
une collision. Aucun défaut du MPD n'a été démontré.

`selectVariantTrack(active, true, 0)` force une purge publique, audio **et vidéo**.
Aucune API publique de purge de l'audio actif seul n'a été identifiée dans cette
version. Un double changement de langue n'est pas adopté : courses possibles.
Les blocages vidéo observés après purge et lors de la préparation restent
inexpliqués : ces essais ne permettent pas d'attribuer toute la panne à Shaka,
au navigateur ou au packaging. Pas de modification spéculative du manifeste.

## Mesures navigateur

Windows, Chrome 155.0.8059.40 et Edge 155.0.4283.45, mêmes fichiers DASH locaux.
Chaque série : deux sens FR/EN, seeks 0,5 / 1,99 / 2,01 s, état pause à 1,99 s,
vitesse 1,25 à 2,01 s. Sélection préalable avec marge 0,5 après seek 3,25 s et
attente 500 ms. Un contrôle sans récupération précède chaque essai protégé.
Les séries ont partiellement chevauché leur exécution : les latences ne sont
pas un benchmark isolé. Aucun P95 représentatif n'est calculé sur ces petits
effectifs incomplets.

| Série | Essais tentés | Seeks protégés mesurés / langue correcte finale | Marqueurs A/V réussis | Contrôles mesurés rejouant l'ancienne langue | Latence signal médiane / maximum |
|---|---:|---:|---:|---:|---:|
| Chrome v1 |12|2 / 2|1 / 2|4 / 4|3678 / 5570 ms|
| Edge v1 |12|3 / 2|1 / 3|2 / 2|2073 / 5184 ms|
| Chrome v2 |12|3 / 3|1 / 3|6 / 6|1973 / 2365 ms|

Les succès globaux incluent les contrôles qui **doivent échouer à lire la
langue sélectionnée** : ne pas les présenter comme des succès de récupération.
Les autres essais ont échoué en préparation (contrôle de continuité vidéo,
timeout PCM) ou au marqueur A/V. v2 conserve explicitement un avertissement de
préparation lorsque seul le contrôle de continuité vidéo échoue ; les anciens
tests stricts n'ont pas été assouplis. Un pilote précédent est invalide : le
nouveau script avait omis le choix manuel de sous-titres attendu par le contrôle.
Tous ces échecs sont archivés.

Silences RMS maximaux observés sur les seeks protégés : Chrome v1 5352 ms,
Edge 5120 ms, Chrome v2 2280 ms. Écarts maximaux entre callbacks vidéo :
791 / 817 / 1259 ms ; frames perdues cumulées : 12 / 17 / 19. Les silences
incluent le temps nécessaire à la récupération ; pour la pause, le script
distingue délai depuis la requête et délai depuis la reprise. Les callbacks
n'incluent pas nécessairement tout l'intervalle initial sans image.

La position enregistrée à la reprise est conservée à moins de 11 ms dans les
huit seeks protégés mesurés ; pause et vitesse sont conservées. Une seule
tentative et aucune restauration supplémentaire de position dans ces huit cas.
Cela ne prouve ni une lecture continue ni la synchronisation : plusieurs
marqueurs ont expiré et Edge a présenté un silence après une reprise initiale.
Le marqueur restant est une observation grossière à ±200 ms, pas une mesure
physique de synchronisation.

Firefox 155.0.1 : deux démarrages WebDriver ont expiré après 45 s, y compris
sur un autre port. **Aucun test média Firefox de ce jalon**. Les six essais
ponctuels de purge précédents restent historiques, sans validation du nouveau
contrôleur.

Oracle exclusivement dans les tests : les fixtures représentatives ne sont pas
le couple mono 440/880 Hz initial. FR 5.1 utilise 440/550/660/80/770/880 Hz,
EN stéréo 990/1100 Hz. Les six/deux voies sont contrôlées ; pas de preuve de
restitution physique 5.1. Les anciennes fixtures mono ne sont pas retestées ici.

## Contrôleur expérimental et limites

`audio-seek-policy.js` mémorise la variante manuelle et le risque de données
anciennes dès qu'une marge positive a été utilisée. À l'événement `seeking`,
une tentative au maximum force la purge publique. Verrou avant l'appel Shaka,
événements récursifs au même emplacement ignorés, seek concurrent distinct
annulant la tentative. Un append audio frais couvrant la cible et les buffers
audio/vidéo sont attendus ; position restaurable une seule fois, pause et
vitesse restaurées. Timeout 6 s, reprise bornée 2 s, erreur terminale sans retry.

Après réussite API, un watchdog de 5 s détecte une absence de progression de
plus de 2 s en lecture et produit une erreur explicite, sans nouvelle purge.
Une pause volontaire ne déclenche pas ce défaut. Cet état API ne prouve jamais
la langue décodée : aucune FFT ni fréquence dans le contrôleur. Pas de garantie
contre un défaut plus tardif. Recharger la présentation est requis après erreur.
L'option est désactivée par défaut car les essais de fluidité échouent encore.

Les journaux incluent variantes, langues, requêtes et plages HTTP après seek ;
la version finale ajoute les requêtes précédentes pour distinguer réutilisation
de buffers et téléchargement de segments. Les traces antérieures seules ne
prouvent pas l'identité des paquets déjà présents en MSE.

Un changement effectif vers une autre piste avec marge 0 nettoie déjà l'audio
seul : le contrôleur retire alors le risque mémorisé et évite une purge vidéo
inutile. Une resélection de la même piste avec marge 0 conserve ce risque.
Cette distinction est testée unitairement.

### Contrôle final limité, avec watchdog et option désactivée par défaut

Préparation à 1,25 s, FR→EN, seek 0,5 s : un essai protégé sur Chrome et un
sur Edge réussissent, marqueur A/V inclus, position à 8,7 ms près, une tentative,
aucune restauration de position et source blob conservée. Chrome : signal en
306,9 ms, silence maximal 237,6 ms, écart callbacks 199,3 ms, 6 frames perdues,
marqueur à +90,7 ms.
Edge : signal en 1755 ms, silence maximal 1676,6 ms, écart callbacks 916,6 ms,
46 frames perdues, marqueur à +19,7 ms. Ces interruptions interdisent de qualifier
ces deux essais de lecture fluide.
Les traces Chrome de requêtes montrent `audio-en.mp4`, initialisation et plages
de segments, aucune requête audio FR après la purge ; variante 1,
audioId 3, langue en. Cela confirme la piste téléchargée et le signal décodé.
Le contrôle Chrome sans protection joue déjà EN dans cet essai : ce protocole
court dépend du moment exact du changement et n'est pas une reproduction
déterministe. Le contrôle Edge rejoue FR. Les deux essais complets Chromium
ne valident ni Firefox, ni une fiabilité statistique, ni l'absence de coupure.
Une tentative finale antérieure à 3,25 s a encore échoué en préparation PCM.

## Reproduction et prochaine vérification

Préparer les fixtures et démarrer `cargo run` selon README. Dans un autre
PowerShell depuis le dépôt, avec Playwright disponible via NODE_PATH :

```powershell
$env:MEDIA_BROWSER_CHANNEL='chrome'
$env:MEDIA_REPORT_DIR='artifacts/seek-policy-nouveau'
node scripts/browser-seek-policy.cjs
```

Pour Edge, préciser `MEDIA_BROWSER_EXECUTABLE` avec le chemin installé et
`MEDIA_BROWSER_CHANNEL=msedge`. Pour Firefox, retirer cet override, utiliser
`MEDIA_BROWSER_CHANNEL=firefox` et au besoin `MEDIA_WEBDRIVER_PORT=4455`.
Le répertoire de rapport doit être neuf. Filtres : `MEDIA_SEEK_ONLY_FROM=fr`
ou `en`, `MEDIA_SEEK_ONLY_POSITION=0.5`, `1.99` ou `2.01`.
`MEDIA_SEEK_PREPARATION_POSITION=1.25` permet le protocole initial plus court,
distinct de la préparation à 3,25 s ; ne pas fusionner leurs statistiques.

Si WebDriver reste indisponible : ouvrir `/shaka` dans Firefox, préparer,
cocher l'option expérimentale, effectuer les mêmes seeks dans les deux sens,
en pause et en lecture, vitesses 1/1,25. Consigner versions, état, position,
événements et erreurs. L'écoute humaine seule ne remplace pas l'oracle PCM ni
les traces automatisées ; ne pas cocher cette validation comme accomplie.
Pour demander la marge positive depuis la console du navigateur, utiliser
`await window.shakaBench.switchLanguage('en', 0.5)` après lecture de la piste FR
à environ 3,75 s ; attendre le nouveau signal puis régler `document.querySelector('video').currentTime=0.5`
(répéter à 1,99 et 2,01 s). Recharger entre essais et inverser FR/EN pour l'autre
sens. Inspecter `window.shakaBench.seekPolicy.history`, `.state`, `.requests`
et `.events`. Pour une comparaison témoin, décocher l'option avant le seek.

Avant adoption : récupérer un Firefox automatisé fonctionnel, isoler les
blocages vidéo, exécuter des répétitions séquentielles homogènes, puis des
changements multiples dans une même présentation et seeks vers l'avant.
La récupération actuelle ne justifie pas un choix définitif de lecteur.

Preuves : `evidence/2026-10-15/audio-seek-policy/`. Tests unitaires :
`node --test scripts/audio-seek-policy.test.cjs` (10 cas). Les tests de statistiques
précédents restent présents ; vérification Rust fmt, clippy strict et 9 tests
d'intégration réussis. CI modifiée mais non exécutée à distance.
