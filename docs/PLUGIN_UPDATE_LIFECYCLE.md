# Cycle de mise à jour des extensions — contrat à réaliser

État au 27 octobre 2026 : le Core expose déjà `POST /api/v1/admin/extensions/{id}/upgrade`
et `POST /api/v1/admin/extensions` avec `upgrade=true`. La route vérifie la
signature et l'identifiant, remplace le paquet en base et la copie locale,
change la génération et conserve les données. Le test actuel prouve seulement
la conservation d'une valeur privée et le changement de génération. **Ce n'est
pas encore un parcours de mise à jour sûr pour une installation réelle.**
Aujourd'hui, la route ne vérifie pas une version strictement supérieure, la
continuité de la clé publique, les droits ajoutés, les schémas de paramètres
ou de stockage, ni la possibilité d'un retour arrière. L'écriture du cache de
paquet et la transaction SQL ne sont pas une seule opération atomique.

## Identité et compatibilité

- Une mise à jour garde le même `manifest.id`, `publisherId` et la même clé
  publique vérifiée. Une rotation de clé d'éditeur exige un parcours distinct,
  audité et approuvé par l'administrateur ; le seul nom d'éditeur ne suffit pas.
- La version cible doit être strictement supérieure à la version installée,
  selon les trois composantes numériques actuellement acceptées. Une version
  égale avec un digest différent est un conflit, pas une correction silencieuse.
- Vérifier avant toute mutation la signature, le digest, `apiVersion`, l'ABI,
  `minServerVersion`/`maxServerVersion`, les imports hôtes, les limites Wasm,
  le schéma des paramètres et les capacités demandées.
- Afficher le diff entre versions : permissions, domaines réseau, schéma de
  configuration, compatibilité serveur et migration de données. Toute nouvelle
  capacité, portée ou domaine attend un accord explicite. Les anciens grants
  ne doivent jamais autoriser implicitement un droit ajouté.

## Préparation et sauvegarde

1. Télécharger le paquet en quarantaine, le vérifier et l'identifier par son
   digest. Préparer les paramètres et migrations avec un plan versionné.
2. Sauvegarder de façon cohérente la base, les paramètres, collections,
   données privées KV/objets et le paquet précédent. Vérifier que la sauvegarde
   est restaurable dans un environnement privé. Le format courant est non
   chiffré : restreindre l'accès à l'archive et à ses copies. Conserver la version de
   l'ABI/Core et l'identité de l'éditeur. Les médias `ro` restent hors du plan.
3. Arrêter les nouvelles invocations du plugin, attendre ou annuler les tâches
   en cours avec une borne de temps, puis figer sa génération. Les opérations
   d'un ancien worker ne doivent plus écrire dans le nouveau namespace.

## Bascule, échec et retour arrière

L'état visé est `installé v1 → préparé v2 → attente d'accord → sauvegardé →
bascule → vérification → actif v2`. Un refus, une signature invalide ou une
incompatibilité laissent v1 active et ses données intactes. Une migration privée
doit être idempotente, bornée, liée aux versions source/cible et exécutée sur
une copie ou dans une transaction dont le résultat peut être abandonné.

La bascule de la version active, des grants, des paramètres et de la version de
schéma doit être atomique dans la base. La copie de paquet sur disque est un
cache reconstructible depuis le paquet signé en base ; son échec ne doit pas
laisser une version active partiellement installée. En cas d'échec avant commit,
réactiver v1 avec sa génération et ses droits. Après commit, vérifier une
invocation synthétique bornée avant réactivation générale ; si elle échoue,
désactiver v2 et restaurer le snapshot v1 si aucune migration irréversible n'a
été appliquée. Une migration irréversible interdit le rollback automatique :
laisser le plugin désactivé et proposer une restauration complète de la
sauvegarde hors ligne, avec avertissement sur les écritures postérieures.

Une désinstallation avec `deleteData=false` n'est pas une sauvegarde. Elle peut
conserver les données privées, mais ne garantit ni les paramètres, ni les
collections, ni un rollback cohérent. `deleteData=true` reste une suppression
explicite et distincte de toute mise à jour.

## Critères avant de déclarer la mise à jour disponible

- Tests de version égale/inférieure, signature/digest erronés, clé différente,
  ABI/Core incompatibles, nouveau droit ou domaine sans consentement.
- Tests de panne avant, pendant et après la transaction, cache absent/corrompu,
  migration interrompue, redémarrage Core et ancien worker encore actif.
- Tests de conservation et restauration des paramètres, collections, KV et
  objets privés, avec sauvegarde vérifiée et retour arrière d'une migration
  réversible. Tester séparément le refus de rollback irréversible.
- Parcours administrateur montrant le diff, l'accord et l'état d'échec sans
  dévoiler de secret. Vérifier les deux moteurs de base utilisés.

Jusqu'à ces preuves, ne pas présenter la route d'upgrade actuelle comme une
mise à jour avec sauvegarde et rollback garantis. Pour les données importantes,
faire une sauvegarde restaurable et tester la migration sur une installation
séparée avant toute opération réelle.
