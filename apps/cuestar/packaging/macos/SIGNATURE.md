# CueStar sur Mac : signature, notarisation et Gatekeeper

## Sans compte développeur (par défaut)

Le workflow **Build CueStar** produit un `.dmg` signé « ad hoc ». Il fonctionne, mais
Gatekeeper le bloque au premier lancement. Le `.dmg` contient alors le fichier
`LISEZ-MOI - Ouvrir CueStar.txt` (français et anglais), qui explique quoi faire :

- **macOS 15 (Sequoia) et suivants :** lancer CueStar une fois (macOS le bloque), puis
  Réglages Système > Confidentialité et sécurité > **Ouvrir quand même** en bas, et le mot
  de passe administrateur. Le clic droit > Ouvrir ne marche plus.
- **macOS 14 et antérieurs :** clic droit sur CueStar > Ouvrir > Ouvrir.
- **Terminal :** `xattr -dr com.apple.quarantine /Applications/CueStar.app`

C'est à faire une fois par version. Ça convient pour soi et quelques collègues ; pour
distribuer largement, mieux vaut signer et notariser.

## Avec un compte Apple Developer (99 $/an)

Une fois les secrets ci-dessous créés, le workflow signe l'application avec le
« hardened runtime », la fait notariser par Apple (`notarytool`) et agrafe le ticket au
`.dmg` (`stapler`). L'application s'ouvre alors sans alerte, et le fichier LISEZ-MOI n'est
plus mis dans le `.dmg` (il y reste tant que la notarisation n'est pas active). Sans ces secrets, rien ne change : le build continue de produire un
`.dmg` non signé.

### Ce qu'il faut créer, une fois

1. **S'inscrire** à l'Apple Developer Program : https://developer.apple.com/programs/
   (compte individuel, 99 $/an). Noter le **Team ID** (10 caractères), visible dans
   https://developer.apple.com/account sous « Membership details ».
2. **Créer le certificat « Developer ID Application »** : sur un Mac, Xcode > Réglages >
   Comptes > Gérer les certificats > « + » > Developer ID Application. Il arrive dans le
   Trousseau d'accès.
3. **L'exporter en .p12** : Trousseau d'accès > Mes certificats > clic droit sur
   « Developer ID Application: … » > Exporter, avec un mot de passe. Puis, dans le Terminal :
   `base64 -i certificat.p12 | pbcopy` (le contenu encodé est dans le presse-papiers).
4. **Créer un mot de passe pour app** pour notarytool : https://account.apple.com >
   Connexion et sécurité > Mots de passe pour app.
5. **Ajouter les secrets du dépôt** : GitHub > acetayls/mediastar > Settings > Secrets and
   variables > Actions > New repository secret :

| Secret | Contenu |
|---|---|
| `MACOS_CERT_P12` | le .p12 encodé en base64 (étape 3) |
| `MACOS_CERT_PASSWORD` | le mot de passe choisi à l'export du .p12 |
| `MACOS_SIGN_IDENTITY` | le nom exact du certificat, ex. `Developer ID Application: Pierrick Nom (AB12CD34EF)` |
| `APPLE_ID` | l'adresse e-mail du compte Apple |
| `APPLE_TEAM_ID` | le Team ID (étape 1) |
| `APPLE_APP_PASSWORD` | le mot de passe pour app (étape 4) |

Avec seulement les trois premiers, l'application est signée mais pas notarisée : Gatekeeper
l'accepte mieux, mais affiche encore une alerte au premier lancement. Il faut les six pour
supprimer l'alerte.

### À la main sur un Mac

`bundle.sh` lit les mêmes variables : `MACOS_SIGN_IDENTITY` (certificat déjà dans le
trousseau), puis `APPLE_ID`, `APPLE_TEAM_ID` et `APPLE_APP_PASSWORD` pour la notarisation.

```
export MACOS_SIGN_IDENTITY="Developer ID Application: … (AB12CD34EF)"
apps/cuestar/packaging/macos/bundle.sh aarch64-apple-darwin dist
```
