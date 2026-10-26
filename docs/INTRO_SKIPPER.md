# IntroSkipper — état vérifié le 26 octobre 2026

IntroSkipper est un plugin Wasm de classification de chapitres et de comparaison
d'empreintes fournies par l'hôte. Son installation seule ne lance aucun traitement.
Le catalogue signé local ne le contient pas encore. Le lecteur affiche déjà des
boutons issus des chapitres du descripteur média ; ces boutons sont produits par
une heuristique du Core, indépendamment du plugin, et ne persistent pas.

## Contrat sûr pour la suite

- Le Core doit créer une tâche durable par épisode ou source nouvelle ou modifiée,
  seulement si le plugin installé est activé et autorisé pour la bibliothèque.
  Identité de recalcul : source, empreinte du contenu ou des métadonnées utiles,
  version et digest de l'algorithme. Les résultats manuels ont priorité.
- Le Media Node propriétaire extrait d'abord les chapitres. Pour les épisodes
  encore ambigus, il calcule des empreintes audio bornées dans les premières
  minutes et la queue, avec CPU et concurrence limités et préemption en lecture.
  Aucun chemin arbitraire n'est transmis au Wasm.
- Le Core ne doit envoyer au plugin que des lots d'une série et d'une saison
  identifiées, avec cadence d'empreintes compatible. Les entrées et sorties sont
  bornées ; au-delà du plafond JSON du SDK, les empreintes exigent un protocole
  paginé ou des comparaisons hôtes bornées, à mesurer avant implémentation.
  Le plafond actuel est **65 536 octets** (`media_extension_sdk::MAX_JSON`) et
  **2 000 000 unités de fuel** par invocation, avec 16 MiB de mémoire Wasm.
  Une estimation JSON compacte de 5 400 mots `u32` (600 s à 9 Hz) représente
  59 510 octets pour un épisode et 1 427 932 octets pour 24 épisodes, sans
  chapitres ni métadonnées réelles. Ce calcul prouve que l'appel monolithique
  de 24 épisodes ne passe pas la limite d'entrée ; il ne mesure pas le fuel.

  Protocole proposé : le Node conserve les empreintes binaires par source dans
  son cache applicatif, avec référence et checksum durables au Core. Le Core
  sélectionne au plus deux épisodes d'une même saison et demande des pages
  de **256 mots** avec cadence et offset absolu. Chaque appel Wasm compare deux
  pages, renvoie au plus 16 candidats `(offsetA, offsetB, longueur, score)` ;
  le Core agrège les diagonales et redemande les seules fenêtres candidates
  avec chevauchement jusqu'à la durée maximale d'intro. Le plugin effectue la
  validation finale et le consensus à partir d'un résumé borné. Chaque page
  doit être identifiée par source, version d'algorithme, index et checksum,
  pour reprendre après interruption sans mélanger deux générations. Le nombre
  de comparaisons et le fuel par page doivent être plafonnés puis mesurés sur
  Wasm réel avant de fixer 256 comme valeur de production.
- Les marqueurs validés et leur confiance doivent être stockés au Core avec
  provenance, version d'algorithme, source et timeline. Une copie ne réutilise un
  marqueur que si sa durée, son point de départ et sa timeline sont vérifiés.
  Le lecteur ne doit proposer aucun saut automatique de faible confiance.
- Les préférences de saut sont par utilisateur. La correction manuelle et la
  désactivation doivent être servies par une API authentifiée avec contrôle de
  bibliothèque et CSRF. La correction prime sur tout recalcul.

## Preuve encore nécessaire

Le corpus annoté doit couvrir 24 épisodes, plusieurs saisons, cold opens,
récapitulatifs, intros variables, silence et crédits. Rapporter précision, rappel,
faux positifs et cas ambigus par méthode, ainsi que temps CPU, RSS, volume audio
lu et nombre d'empreintes. Les tests synthétiques actuels ne prouvent pas ces
mesures ni le fonctionnement sur deux Nodes ou après redémarrage.
