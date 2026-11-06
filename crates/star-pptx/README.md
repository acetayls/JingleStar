# star-pptx

Rendu de présentations PowerPoint (`.pptx`) **simples** en images, en Rust pur : pas de
LibreOffice, pas de Java, pas de bibliothèque payante. Un `.pptx` est une archive zip de
fichiers XML ; la crate les lit et dessine chaque diapositive.

Utilisée par le module « Présentation » de CueStar.

```rust
let pres = star_pptx::Presentation::open("conference.pptx")?;
for i in 0..pres.slide_count() {
    if pres.is_hidden(i) {
        continue;
    }
    // Plus grande image aux proportions de la diapositive qui tient dans 1920 × 1080.
    let img = pres.render_slide(i, 1920, 1080)?;
    img.save(format!("diapo-{}.png", i + 1))?;
}
```

Exemple en ligne de commande (rend toutes les diapositives en PNG) :

```sh
cargo run -p star-pptx --release --example render -- conference.pptx sortie/ 1920 1080
```

## API

- `Presentation::open(chemin)` / `Presentation::from_bytes(octets)`
- `slide_count()`, `slide_size_emu()`, `aspect_ratio()`, `fit_size(l, h)`
- `is_hidden(i)`, `next_visible(i)`, `prev_visible(i)` : diapositives masquées (`show="0"`)
- `slide_title(i)` : texte de l'espace réservé titre, pour une liste
- `notes(i)` : notes du présentateur
- `render_slide(i, l, h)` → `image::RgbaImage`

`Presentation` est `Send + Sync` : le rendu peut se faire dans un autre fil d'exécution.
Les erreurs (`star_pptx::Error`) sont décrites en français ; un fichier bizarre ne provoque
jamais de panique, les éléments inconnus sont ignorés.

## Pris en charge

- **Structure** : ordre des diapositives (`presentation.xml` et relations), taille
  (`p:sldSz`), diapositives masquées, chaîne d'héritage diapositive → disposition → masque →
  thème, `showMasterSp` (« Masquer les graphiques d'arrière-plan »).
- **Couleurs** : `srgbClr`, `schemeClr` (avec `p:clrMap` et `p:clrMapOvr`), `sysClr`,
  `prstClr`, `scrgbClr`, `hslClr` ; modificateurs `lumMod`, `lumOff`, `tint`, `shade`,
  `alpha`, `satMod`, `hueOff`, `comp`, `inv`, `gray`…
- **Fonds** (`p:bg`) : couleur unie, dégradé linéaire ou radial, image (étirée ou en
  mosaïque), motif (rendu en teinte moyenne), références de style du thème (`p:bgRef`).
- **Espaces réservés** : position et taille héritées de la disposition puis du masque
  (correspondance par `idx` puis par type), styles de texte hérités (`p:titleStyle`,
  `p:bodyStyle`, `p:otherStyle`, `a:lstStyle` de chaque niveau, `defaultTextStyle` de la
  présentation). Les espaces réservés vides ne sont pas affichés, comme en diaporama.
- **Formes** (`p:sp`, `p:cxnSp`) : position, taille, rotation, miroirs ; formes prédéfinies
  courantes (rectangle, arrondi, ellipse, trait, connecteurs, triangles, losange,
  parallélogramme, trapèze, pentagone, hexagone, octogone, chevron, flèches, croix, étoiles,
  cadre, anneau, secteur, arc, cylindre…), formes libres (`a:custGeom`), repli sur un
  rectangle ; remplissage uni, dégradé, image, aucun ; contour (épaisseur, couleur, tirets,
  pointes de flèche) ; styles de forme du thème (`p:style` : `fillRef`, `lnRef`, `fontRef`,
  `effectRef`) ; ombre portée extérieure (`a:outerShdw`, nette ou floue).
- **Texte** : paragraphes et séries, taille (avec `normAutofit fontScale` et
  `lnSpcReduction`), gras, italique (simulés si la police ne les a pas), souligné, barré,
  couleur, surlignage, police (y compris `+mj-lt` / `+mn-lt`), majuscules, exposant/indice,
  espacement des caractères ; alignement gauche / centre / droite / justifié ; puces
  (`a:buChar` avec couleur, taille et police ; caractères Wingdings/Symbol convertis),
  numérotation automatique (`a:buAutoNum` : chiffres, lettres, chiffres romains) ; marges
  et retraits par niveau ; interligne (`spcPct`, `spcPts`) et espacement avant/après ;
  marges intérieures, ancrage haut / milieu / bas, retour à la ligne ou non, texte vertical
  (`vert`, `vert270`), champs (numéro de diapositive).
  Mise en forme avec cosmic-text (OpenType, polices de repli pour les autres écritures),
  glyphes tracés en contours anticrénelés : net à toutes les tailles.
- **Polices** : celles du système. Quand une police Office manque (machine Linux…), une
  police métriquement équivalente est prise : Calibri → Carlito, Cambria → Caladea,
  Arial → Liberation Sans, Times New Roman → Liberation Serif, Courier New → Liberation Mono…
- **Images** (`p:pic`) : PNG, JPEG, GIF, BMP, TIFF, WebP ; recadrage (`a:srcRect`),
  transparence (`alphaModFix`), niveaux de gris, découpe selon la forme (image dans une
  ellipse…), réduction de qualité pour les grandes photos ; aperçu d'une vidéo ou d'un
  objet OLE ; PNG tronqués tolérés.
- **Groupes** (`p:grpSp`) avec repère enfant (`chOff` / `chExt`), imbriqués.
- **Tableaux** (`a:tbl`) : colonnes, lignes (agrandies si le texte déborde), cellules
  fusionnées, remplissages et bordures des cellules, styles de tableau de
  `tableStyles.xml` (ligne d'en-tête, lignes à bandes…) et style par défaut de PowerPoint.
- **SmartArt** : dessiné à partir du rendu enregistré par PowerPoint
  (`ppt/diagrams/drawingN.xml`).
- `mc:AlternateContent` : la variante de repli est utilisée.

## Pas pris en charge (pour l'instant)

- Animations et transitions (volontairement : chaque diapositive est une image fixe).
- **Graphiques** (`c:chart`) : la zone reste vide. Équations : seulement si une image de
  repli est présente.
- Images vectorielles EMF / WMF / SVG seules (sans PNG de repli) : rien n'est dessiné.
- Effets autres que l'ombre extérieure : lueur, reflet, ombre intérieure, biseaux et 3D,
  flou artistique, effets d'image (duotone, luminosité…).
- Motifs de remplissage exacts (rendus en teinte moyenne), dégradés de forme exacts (rendus
  en radial).
- Formes prédéfinies rares (nuages, bulles avec pointe, accolades… : dessinées en rectangle
  ou en ellipse), formules de guides de `custGeom` (seules les valeurs numériques et
  `val N` sont lues).
- Ajustement automatique du texte au moment du rendu : on applique l'échelle enregistrée par
  PowerPoint, sans recalcul.
- Colonnes de texte, tabulations alignées, WordArt (`prstTxWarp`), texte sur chemin.
- Polices incorporées dans le fichier.
- Fichiers `.ppt` (ancien format binaire) et `.odp`.

## Tests

```sh
cargo test -p star-pptx
```

Les tests construisent de petites présentations en écrivant le XML dans une archive zip, et
utilisent `tests/fixtures/demo.pptx` (généré par `tests/fixtures/make_fixtures.py` avec
python-pptx). Pour comparer visuellement, LibreOffice peut servir de référence locale
(`soffice --headless --convert-to pdf`), jamais de dépendance.
