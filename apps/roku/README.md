# Aether pour Roku

Chaîne SceneGraph : adresse du Core, appairage par code (Quick Connect, à
valider depuis un appareil déjà connecté), Films et Séries avec affiches,
épisodes, lecture HLS du Core (pistes audio et sous-titres WebVTT proposés par
le lecteur Roku), reprise et sauvegarde de la progression.

## Installer (mode développeur)

1. Sur la Roku : Accueil ×3, Haut ×2, Droite, Gauche, Droite, Gauche, Droite,
   puis activer le mode développeur et noter l'adresse et le mot de passe.
2. Construire l'archive depuis ce dossier :

   ```bash
   cd apps/roku && zip -r ../../aether-roku.zip manifest source components
   ```

3. Ouvrir `http://<adresse-de-la-roku>` dans un navigateur, envoyer
   `aether-roku.zip` avec « Upload », puis « Install ».

La touche `*` de la télécommande change l'adresse du serveur pendant l'appairage.

## Vérifier

```bash
npx brighterscript@0.73.7 --rootDir apps/roku --createPackage false --files manifest "source/**/*" "components/**/*"
```

## Limites

- Le Core doit être joignable en HTTPS avec un certificat reconnu, ou en HTTP
  sur le réseau local : la Roku refuse les certificats auto-signés.
- Pas de télécommande depuis le téléphone : BrightScript n'a pas de client
  WebSocket.
- Pas encore testée sur un vrai téléviseur Roku.
