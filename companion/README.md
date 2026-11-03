# Module Companion pour JingleStar

Module [Bitfocus Companion](https://bitfocus.io/companion) qui pilote JingleStar en OSC et lit son
état sur le serveur OSCQuery. Aucun autre logiciel n'est nécessaire côté JingleStar.

## Installer le paquet (Companion 4.x)

1. Dans ce dossier : `npm install` puis `npm run package` : fabrique `jinglestar-<version>.tgz`.
2. Dans Companion : page **Modules** → importer un module, choisir le fichier `.tgz`.
3. Ajouter une connexion « JingleStar », renseigner l'adresse IP.

## Essayer le module en développement (Companion 3.x ou 4.x)

1. Installer les dépendances, dans ce dossier : `npm install`
2. Dans Companion : Settings → Developer → **Developer modules path** : choisir le dossier
   parent de `companion` (Companion charge chaque sous-dossier comme un module).
3. Ajouter une connexion « JingleStar », renseigner l'adresse IP.

## Tests

`npm test` : lecture de l'état et encodage OSC (sans Companion).

## Fichiers

- `src/main.js` : connexion, interrogation de l'état, envoi OSC
- `src/actions.js`, `src/feedbacks.js`, `src/variables.js`, `src/presets.js`
- `src/state.js` : lecture de l'arborescence OSCQuery
- `companion/manifest.json`, `companion/HELP.md` : description du module pour Companion
