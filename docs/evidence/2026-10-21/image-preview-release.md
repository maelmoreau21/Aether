# Miniature synthétique : mesure locale du 21 octobre 2026

Commande exécutée sur Windows, build Rust optimisé :

```powershell
cargo test --release --locked -p media-node --lib image_preview -j 1 -- --test-threads=1 --nocapture
```

Deux tests réussis sur la source finale. Affiche JPEG synthétique détaillée
1800 × 2700, représentation demandée à 320 pixels (obtenue : 320 × 480) :

| Mesure | Résultat |
| --- | ---: |
| Original | 3 862 387 octets |
| Miniature | 86 620 octets |
| Réduction des octets | 97,76 % |
| Première génération | 159,281 ms |
| Réutilisation du cache | 305,8 µs |

Observation unique incluant les contrôles de fichier et le traitement local,
sans réseau, affichage navigateur ni mesure du NAS. Aucun objectif universel de
latence n'en découle. Un précédent passage sur la même affiche avait observé
93 ms ; les durées ne sont pas des percentiles ni un comparatif Jellyfin.

Tests : dimensions et poids, partage du corps en cache, modification de la source,
transparence, GET/HEAD/Range/416/304 et validation des largeurs autorisées.
Le cache a une borne de 8 Mio de corps compressés et 128 entrées ; ce n'est pas
une limite RSS. La validation et le décodage partagent un seul permis avec les
originaux et le scanner. Les bibliothèques restent en lecture seule.

Docker Desktop indisponible : aucune mesure du poids final des couches, de RSS
Docker après cinq minutes, de consommation Linux ou de contention sous charge.
