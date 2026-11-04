# Audit du lecteur — 21 octobre 2026

## Correctifs appliqués après l'audit

Le constat initial ci-dessous décrit l'état avant correction. À la demande de
l'utilisateur, les modifications de production suivantes ont ensuite été réalisées :

- Préparations vidéo/audio sérialisées, erreurs différenciées y compris sur HEAD
  et relais Node. Limites configurables : 900 secondes, 64 Gio vidéo, 4 Gio audio,
  96 Gio de cache par défaut ; plafonds détaillés dans PLAYER_TRACK_PREPARATION.md.
  Cache Docker dans `/data/preparation-cache` pour éviter le tmpfs `/tmp` de 64 Mio.
- Commutation audio transactionnelle : le candidat est chargé et positionné en
  pause avant activation. L'ancien audio reste disponible si le candidat échoue.
  Nettoyage à la fermeture et invalidation des sources en erreur terminale.
- Sélection automatique d'une piste concrète, normalisation des langues et
  respect des choix manuels. Original multi-audio déclaré `und` et libellé sans
  inventer la piste réellement choisie par le navigateur.
- Attente des données audio et du seek, recalage explicite aux transitions,
  correction au-delà de 80 ms au lieu de 150 ms. Blocage audio suspendant la vidéo,
  reprise bornée et annulable par pause manuelle. Aucun rattrapage par variation
  de vitesse ni graphe WebAudio de production.
- Test de conservation des paquets et huit canaux Opus en MKV/MP4 ajouté aux
  vérifications des pistes existantes. Le test de diagnostic devient une suite
  d'acceptation des cinq corrections ; les preuves JSON d'avant sont conservées.

Les médias ne sont jamais réencodés dans le parcours de production. Le premier
remux reste une opération d'entrée/sortie complète, le cache reste temporaire.
La séparation de deux horloges et la sortie physique 5.1/7.1 ne deviennent pas
des garanties universelles. Safari, Firefox, Edge, NAS et 4K personnel non validés
par ces modifications ; Edge et le Firefox Playwright ne sont pas installés dans
l'environnement testé. Les preuves après correction sont distinctes :
`evidence/2026-10-21/player-audit/browser-fixed.json` et `browser-fixed/`.

Validation des correctifs : Web check 0 erreur/avertissement, lint, build et
73 tests existants réussis ; cinq tests d'acceptation ciblés réussis ; deux tests
Rust de copie AV1/Opus (dont 7.1 MKV/MP4) réussis, ainsi que deux tests de leases
du cache (un helper ignoré, invoqué par le test d'arrêt brutal). Ces derniers ont
été exécutés avec `MEDIA_PREPARATION_CACHE_DIR` pointant vers le dossier absolu
`target/player-cache-check`, hors médias. Les deux Compose passent `config --quiet`
avec `.env.example` et un secret de pairing jetable ; aucune image construite ou
déployée pour ces changements. Fmt, diff et Clippy workspace/all-targets stricts
vérifiés. Pas de relance de la suite Rust générale.

Le parcours Core/SQLite/Chrome corrigé passe en 33,09 secondes : MP4, puis MKV
après redémarrage, quatre sélections FR/EN par conteneur, PCM réellement décodé,
seek/pause, sous-titres et progression. Le premier essai avait mis en évidence
une pause technique de buffering confondue avec une pause manuelle lors d'une
commutation ; l'intention de lecture est désormais conservée et le scénario a
été rejoué après correction. Le serveur HTTP isolé confirme séparément le
nettoyage du candidat HTTP 404 et le choix FR effectif (~662 Hz, contre ~990 Hz
pour l'ancienne mauvaise sélection EN).

Commandes complémentaires aux commandes d'audit initiales :

```powershell
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
cargo test --locked -p media-node --lib packaging::tests -j 1 -- --test-threads=1
cargo test --locked -p media-node --lib cache_dirs::tests -j 1 -- --test-threads=1
cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings
docker compose --env-file .env.example -f docker-compose.yml config --quiet
# Même validation pour compose.distributed.yaml, avec MEDIA_NODE_PAIRING_TOKEN jetable.
```

## Analyse initiale conservée

Analyse du code courant, sans modification du lecteur de production. Objectifs :
aucun réencodage vidéo/audio serveur, aucune perte de qualité supplémentaire,
conservation des canaux source, VF/VO et lecture fluide. Bibliothèques personnelles,
déploiement, publication et refonte exclus. Tests sur fixtures synthétiques.

## Résultat

Les sept sujets ne sont pas sept pannes systématiques. Les changements VF/VO,
seek/pause et décodage fonctionnent dans le parcours Chrome synthétique testé.
Trois défauts concrets concernent la préparation concurrente, l'état après échec
de commutation et la sélection française. La synchronisation et les blocages
audio présentent aussi des limites vérifiables. La sortie physique 5.1/7.1 et
la fluidité des gros médias sur NAS restent à valider.

| Sujet | Constat et niveau de preuve | Correction adaptée |
| --- | --- | --- |
| 1. Synchronisation | `external-audio.ts:35-43,62,69` : deux horloges, correction par seek si dérive >150 ms, intervalle 100 ms. Test déterministe : 140 ms restent présents après `seeked`. Ce seuil ne borne pas la dérive maximale ni la latence des sorties physiques. | Recaler explicitement après seek et changement ; coordonner le buffering des deux éléments et attendre la position audio avant reprise. Distinguer offset voulu et dérive. Ne pas promettre une précision garantie avec cette seule stratégie. |
| 2. Canaux | `packaging.rs` utilise `-c:v copy` et `-c:a copy`, sans `-ac` ni filtre audio. Test réel : paquets AV1/Opus SHA-256 identiques avant/après pour MKV/MP4 ; extraction de la piste FR conservant 6 canaux. Aucune preuve d'un downmix serveur dans ce parcours. Aucune mesure physique 5.1/7.1. | Garder la copie de paquets ; vérifier aussi disposition des canaux, mapping Opus, init et horodatages sur fixtures 5.1/7.1. Qualifier chaque chaîne navigateur/OS/périphérique avec signal distinct par canal. Afficher « piste 5.1 » comme propriété source, pas comme certification de sortie. |
| 3. VF/VO | `native.ts:72-81` détruit l'ancien audio avant validation du nouveau ; si le chargement échoue, l'ancien `source` reste enregistré et le nouvel `ExternalAudio` en échec reste attaché. `selectAudio` ignore alors la resélection de l'ancienne piste (`:168`). Reproduit en Chrome par HTTP 404 VO après VF fonctionnelle. | Faire une commutation transactionnelle : préparer/charger le candidat en pause, valider sa position, remplacer seulement à la réussite ; sinon détruire le candidat et restaurer l'ancienne piste ou entrer dans un état explicitement invalide. Aucun double audio audible. Conserver position, pause, volume, vitesse et choix manuel. |
| 4. Préparation | `native.ts:69` lance les deux HEAD via `Promise.all`. `packaging.rs:81-84` n'admet qu'un worker par `try_acquire`, avec refus immédiat. Test réel sur MKV et MP4 : une préparation concurrente est refusée, puis réussit en série. La course dépend du cache et de l'ordonnancement ; le parcours Chrome court a néanmoins réussi. | Sérialiser les préparations du lecteur immédiatement ; ne pas augmenter aveuglément le nombre de FFmpeg. Ensuite mutualiser les demandes identiques et proposer une préparation facultative en tâche bornée avant lecture, avec état/progression et cache réutilisable. |
| 5. Langue | `inventory.rs:2573` étiquette l'original selon le premier audio ; `ImmersivePlayer.svelte:80-81` et `native.ts:47` peuvent choisir cet original FR avant la source FR explicite. Chrome : premier audio FR non-default, second EN default, choix original étiqueté FR, signal décodé EN 990,23 Hz. | Ne pas attribuer à l'original une garantie de piste précise. Privilégier un ID de piste explicitement sélectionnable pour satisfaire la préférence ; normaliser BCP 47 et codes ISO, respecter le choix manuel, les dispositions default/commentary/accessibility et les langues absentes. Conserver le Direct Play quand son audio réel est identifié et approprié. |
| 6. Navigateurs | La présélection vérifie `canPlayType(source.mime)` sans codec/profil/canaux ; l'audio teste Ogg/Opus générique. Cela ne prouve ni AV1 10 bits ni sortie multicanal. Les expériences MSE/Shaka historiques sont séparées du moteur actuel. | Utiliser les métadonnées codec/profil/canaux pour les sondes de capacité, puis vérifier un vrai démarrage avec images décodées et PCM. Matrice Chrome/Edge/Firefox/Safari et appareils visés, seeks avant/arrière, changements répétés, durées longues, réseau lent et erreurs. Aucune substitution de codec/réencodage automatique. |
| 7. Erreurs | Le watchdog natif surveille `waiting/stalled` vidéo. L'audio n'écoute ni `waiting` ni `stalled` : événement audio seul sans pause vidéo ni erreur après 11 s dans le test déterministe. Les erreurs audio explicites sont signalées ; le bouton de nouvelle tentative existe. Le défaut de commutation laisse toutefois un état incohérent. | Surveiller les deux pipelines, suspendre ensemble pendant un blocage audio, borner la reprise et nettoyer les candidats/listeners/timers après rejet. Invalider les états périmés et tester la nouvelle tentative, la fermeture pendant chargement, la panne après seek et les changements rapides. |

## Ce qui freine réellement les gros fichiers

La préparation lit jusqu'à la fin du composant avant de répondre : ce n'est pas
un démarrage progressif pendant le remux. Seuls la vidéo et l'audio demandés sont
préparés, pas toutes les pistes systématiquement ; une nouvelle langue non cachée
nécessite un nouveau passage dans le fichier.

Bornes exactes : 120 secondes par préparation ; 16 Gio par vidéo ; 1 Gio par
audio ; 24 Gio et 64 composants par cache de processus. Une vidéo préparée
dépassant 16 Gio est refusée, même si l'espace disque disponible est suffisant.
À titre de calcul, 16 Gio sur deux heures correspondent à environ 19,1 Mbit/s
de vidéo : certains longs fichiers 4K dépasseront donc la limite. Ce calcul
n'est pas une mesure de débit ni un benchmark NAS. Le cache est temporaire et
ne constitue pas une préparation durable réutilisable après redémarrage.

Une commande de copie ne réencode pas mais doit toujours lire/écrire les données.
Le temps dépend du volume, du stockage, du réseau et de la concurrence. Aucun
chiffre de démarrage 4K réel n'a été mesuré dans cet audit. Les différents motifs
d'échec sont actuellement regroupés en HTTP 503 ; les distinguer aiderait
l'utilisateur à savoir si attendre, libérer le cache, installer FFmpeg ou changer
de client est utile.

## Choix technique recommandé

Premier lot limité aux fichiers existants : sérialisation des préparations,
commutation atomique/nettoyage, sélection explicite de piste/langue et surveillance
du buffering audio. Ajouter les scénarios d'acceptation correspondants avant de
déclarer les défauts corrigés. Garder les frontières Core/Node, l'API et les médias
en lecture seule. Les tests de diagnostic reproduisent aujourd'hui les défauts ;
ils ne sont pas des tests de comportement souhaité.

Si l'objectif exige une seule chronologie de lecture et des changements très
fluides, le prochain POC adapté est un seul élément `<video>` alimenté via MSE,
avec vidéo AV1 et pistes Opus remuxées/segmentées sans réencodage. MSE gère les
buffers et timestamps sur la même chronologie du média :
[spécification W3C](https://www.w3.org/TR/media-source/).
Il faut vérifier le format de segments accepté et le comportement du moteur
retenu ; les Ogg actuels ne deviennent pas automatiquement des segments MSE.
Réutiliser les expériences MSE/Shaka du dépôt comme base d'essai, sans choisir
Shaka définitivement ni remplacer tout le lecteur. Les anciennes difficultés
de purge/changement audio après seek doivent être rejouées.

Un flux remuxé vidéo+audio par langue simplifie la synchronisation mais peut
dupliquer les caches vidéo et provoquer un rechargement lors du changement ;
ce n'est pas la première correction recommandée. Réduire le seuil à 20 ms ou
augmenter les timers ne crée pas une horloge commune et peut multiplier les seeks.

La sortie multicanal reste dépendante du client, de l'OS, du pilote et du matériel.
La conservation des paquets serveur n'en garantit pas la restitution. Un
`AudioContext` de mesure ne constitue pas une preuve physique ; sa destination
peut avoir une configuration différente :
[Web Audio, AudioDestinationNode](https://www.w3.org/TR/webaudio-1.0/#AudioDestinationNode).
Si un client ne sait pas restituer les canaux demandés, les contraintes interdisent
de résoudre cela par un downmix serveur silencieux : exposer la limite du client.

## Vérifications et reproductibilité

Exécutées depuis le dépôt le 21 octobre 2026 :

```powershell
npm test --prefix apps/web -- --reporter=dot
apps/web/node_modules/.bin/vitest run --config docs/evidence/2026-10-21/player-audit/vitest.config.mts --reporter=verbose
node docs/evidence/2026-10-21/player-audit/browser-audit.cjs
cargo test --locked -p media-node packaging::tests::copies_av1_opus_tracks_and_reuses_cache_with_source_validation -j 1 -- --test-threads=1
cargo test --locked -p media-node --test preparation_admission -j 1 -- --test-threads=1 --nocapture
cargo clippy --locked -p media-node --test preparation_admission -j 1 -- -D warnings
$env:MEDIA_REPORT_DIR='docs/evidence/2026-10-21/player-audit/browser'
cargo test --locked -p media-poc --test browser_scanner -j 1 -- --ignored --nocapture --test-threads=1
```

- Web existant : 72/72 réussis. Le test de préparation parallèle simule deux
  réponses 200 et ne reproduit pas l'admission réelle du Node.
- Suite diagnostique : 5/5 observations reproduites sur le code courant,
  avec éléments média simulés ; aucune preuve matérielle déduite de ces tests.
- Chrome 155.0.8059.85, Windows 10.0.26300, headless : HTTP 404 audio et mauvaise
  langue de l'original reproduits avec le vrai code du lecteur transpilé, vrai
  décodage média et serveur HTTP isolé. Le serveur de test n'est pas une preuve
  Core/RBAC. Résultats : [browser-audit.json](evidence/2026-10-21/player-audit/browser-audit.json).
- Test Node de copie de paquets : réussi MKV/MP4, vidéo et deux audios Opus.
- Test Node d'admission : réussi MKV/MP4, refus concurrent et réussite sérielle,
  piste FR extraite conservant six canaux. Le premier essai du nouveau test
  supposait à tort quel composant gagnerait ; corrigé pour accepter les deux
  ordonnancements, sans modification de production.
- Parcours Core/SQLite/Chrome complet : 1/1 réussi en 56,08 secondes, MP4 puis
  MKV après redémarrage réel du Core. Quatre sélections FR/EN par conteneur avec
  signaux PCM distincts ; seek/pause, offsets, sous-titres, progression et plein
  écran passent. Ce succès court ne supprime pas la course d'admission reproduite
  séparément. Résultats dans [browser](evidence/2026-10-21/player-audit/browser/),
  à distinguer du serveur isolé ci-dessus.
- `cargo fmt --check` et `git diff --check` : réussis.
- Clippy strict ciblé sur le nouveau test Node : réussi.

Limites : pas de NAS, film personnel 4K, restitution matérielle 5.1/7.1,
7.1 décodé, autre navigateur/appareil, endurance ni mesure RSS dans ce lot.
Pas de correction de production, de changement de dépendance ou d'infrastructure.
