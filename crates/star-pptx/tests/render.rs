//! Tests de rendu : petites présentations écrites à la main (XML dans une archive zip), et la
//! présentation `fixtures/demo.pptx` générée avec python-pptx (`fixtures/make_fixtures.py`).

use star_pptx::{Error, Presentation};
use std::io::Write;

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn rels(items: &[(&str, &str, &str)]) -> String {
    let mut s = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    for (id, kind, target) in items {
        s.push_str(&format!(r#"<Relationship Id="{id}" Type="{REL}/{kind}" Target="{target}"/>"#));
    }
    s.push_str("</Relationships>");
    s
}

fn zip(parts: &[(&str, String)]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in parts {
            z.start_file(*name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

/// Présentation 16:9 minimale : masque, disposition et thème, plus les diapositives données.
fn deck(slides: &[String]) -> Vec<u8> {
    let theme = format!(
        r#"<a:theme {NS} name="T"><a:themeElements><a:clrScheme name="c">
<a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1>
<a:dk2><a:srgbClr val="1F2A44"/></a:dk2><a:lt2><a:srgbClr val="EEEEEE"/></a:lt2>
<a:accent1><a:srgbClr val="0000FF"/></a:accent1><a:accent2><a:srgbClr val="FF0000"/></a:accent2>
<a:accent3><a:srgbClr val="00FF00"/></a:accent3><a:accent4><a:srgbClr val="FFFF00"/></a:accent4>
<a:accent5><a:srgbClr val="00FFFF"/></a:accent5><a:accent6><a:srgbClr val="FF00FF"/></a:accent6>
<a:hlink><a:srgbClr val="0000EE"/></a:hlink><a:folHlink><a:srgbClr val="551A8B"/></a:folHlink></a:clrScheme>
<a:fontScheme name="f"><a:majorFont><a:latin typeface="Calibri"/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/></a:minorFont></a:fontScheme>
<a:fmtScheme name="s"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>
<a:lnStyleLst><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>
<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    );
    let master = format!(
        r#"<p:sldMaster {NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg2"/></p:bgRef></p:bg><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Titre"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="500000" y="300000"/><a:ext cx="11000000" cy="1200000"/></a:xfrm></p:spPr><p:txBody><a:bodyPr anchor="ctr"/><a:p><a:r><a:t>Titre du masque</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="3" name="Bandeau"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="0" y="6400000"/><a:ext cx="12192000" cy="458000"/></a:xfrm><a:prstGeom prst="rect"/><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></p:spPr></p:sp>
</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
<p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr sz="4400"><a:solidFill><a:schemeClr val="tx2"/></a:solidFill><a:latin typeface="+mj-lt"/></a:defRPr></a:lvl1pPr></p:titleStyle>
<p:bodyStyle><a:lvl1pPr marL="228600" indent="-228600"><a:buChar char="•"/><a:defRPr sz="2800"/></a:lvl1pPr></p:bodyStyle><p:otherStyle/></p:txStyles></p:sldMaster>"#
    );
    let layout = format!(
        r#"<p:sldLayout {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Titre"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:t>Titre</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sldLayout>"#
    );
    let mut ids = String::new();
    let mut pres_rels = vec![("rIdM".to_string(), "slideMaster".to_string(), "slideMasters/slideMaster1.xml".to_string())];
    let mut parts: Vec<(String, String)> = Vec::new();
    for (i, s) in slides.iter().enumerate() {
        let n = i + 1;
        ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{n}"/>"#, 255 + n));
        pres_rels.push((format!("rId{n}"), "slide".into(), format!("slides/slide{n}.xml")));
        parts.push((format!("ppt/slides/slide{n}.xml"), s.clone()));
        parts.push((format!("ppt/slides/_rels/slide{n}.xml.rels"), rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")])));
    }
    let pres = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation {NS}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rIdM"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/></p:presentation>"#
    );
    let pres_rels: Vec<(&str, &str, &str)> = pres_rels.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
    let mut all = vec![
        ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#.to_string()),
        ("_rels/.rels", rels(&[("rId1", "officeDocument", "ppt/presentation.xml")])),
        ("ppt/presentation.xml", pres),
        ("ppt/_rels/presentation.xml.rels", rels(&pres_rels)),
        ("ppt/theme/theme1.xml", theme),
        ("ppt/slideMasters/slideMaster1.xml", master),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", rels(&[("rId1", "theme", "../theme/theme1.xml"), ("rId2", "slideLayout", "../slideLayouts/slideLayout1.xml")])),
        ("ppt/slideLayouts/slideLayout1.xml", layout),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels(&[("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")])),
    ];
    for (n, b) in &parts {
        all.push((n.as_str(), b.clone()));
    }
    zip(&all)
}

fn slide(body: &str, attrs: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><p:sld {NS} {attrs}><p:cSld>{bg}<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{body}</p:spTree></p:cSld></p:sld>"#,
        bg = if attrs.contains("bgred") { r#"<p:bg><p:bgPr><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></p:bgPr></p:bg>"# } else { "" }
    )
    .replace("bgred", "")
}

fn rect(x: i64, y: i64, w: i64, h: i64, extra: &str, fill: &str) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="9" name="r"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{fill}</p:spPr>{extra}</p:sp>"#
    )
}

fn px(img: &image::RgbaImage, fx: f32, fy: f32) -> [u8; 3] {
    let p = img.get_pixel((fx * img.width() as f32) as u32, (fy * img.height() as f32) as u32);
    [p[0], p[1], p[2]]
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (*x as i32 - *y as i32).abs() <= 6)
}

#[test]
fn basic_shapes_backgrounds_and_master() {
    let s1 = slide(&rect(1_000_000, 1_000_000, 2_000_000, 2_000_000, "", r#"<a:solidFill><a:schemeClr val="accent1"/></a:solidFill>"#), "");
    let s2 = slide(
        &format!(
            "{}{}",
            rect(6_096_000, 3_429_000, 3_000_000, 2_000_000, "", r#"<a:solidFill><a:srgbClr val="00FF00"><a:lumMod val="50000"/></a:srgbClr></a:solidFill>"#),
            // Ellipse : le coin de son cadre reste vide.
            r#"<p:sp><p:nvSpPr><p:cNvPr id="4" name="e"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="4000000" cy="4000000"/></a:xfrm><a:prstGeom prst="ellipse"/><a:solidFill><a:srgbClr val="0000FF"/></a:solidFill></p:spPr></p:sp>"#
        ),
        r#"bgred showMasterSp="0""#,
    );
    let s3 = slide("", r#"show="0""#);
    let pres = Presentation::from_bytes(deck(&[s1, s2, s3])).unwrap();
    assert_eq!(pres.slide_count(), 3);
    assert_eq!(pres.slide_size_emu(), (12_192_000, 6_858_000));
    assert!(!pres.is_hidden(0) && pres.is_hidden(2));
    assert_eq!(pres.fit_size(1000, 1000), (1000, 563));

    let img = pres.render_slide(0, 640, 360).unwrap();
    assert_eq!(img.dimensions(), (640, 360));
    // Fond du masque (bgRef → lt2), rectangle accent1, bandeau rouge du masque.
    assert!(close(px(&img, 0.9, 0.5), [0xEE, 0xEE, 0xEE]), "{:?}", px(&img, 0.9, 0.5));
    assert!(close(px(&img, 0.16, 0.29), [0, 0, 255]));
    assert!(close(px(&img, 0.5, 0.98), [255, 0, 0]));

    let img = pres.render_slide(1, 640, 360).unwrap();
    // Fond rouge propre à la diapositive, formes du masque masquées (showMasterSp="0").
    assert!(close(px(&img, 0.9, 0.2), [255, 0, 0]));
    assert!(close(px(&img, 0.5, 0.98), [255, 0, 0]));
    assert!(close(px(&img, 0.6, 0.6), [0, 128, 0]), "{:?}", px(&img, 0.6, 0.6));
    assert!(close(px(&img, 0.15, 0.3), [0, 0, 255]));
    assert!(close(px(&img, 0.01, 0.02), [255, 0, 0]));
}

#[test]
fn text_is_drawn_with_inherited_styles() {
    let title = r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Titre"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="fr-FR"/><a:t>MMMMMMMMMMMM</a:t></a:r></a:p></p:txBody></p:sp>"#;
    let empty_ph = r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Vide"/><p:cNvSpPr/><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="2000000"/><a:ext cx="12192000" cy="3000000"/></a:xfrm><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></p:spPr><p:txBody><a:bodyPr/><a:p><a:endParaRPr/></a:p></p:txBody></p:sp>"#;
    let pres = Presentation::from_bytes(deck(&[slide(&format!("{title}{empty_ph}"), "")])).unwrap();
    assert_eq!(pres.slide_title(0).as_deref(), Some("MMMMMMMMMMMM"));
    let img = pres.render_slide(0, 1280, 720).unwrap();
    // Le titre hérite de la position du masque et de la couleur tx2 (1F2A44) du style de titre.
    let (x0, y0, x1, y1) = (500_000.0 / 12_192_000.0, 300_000.0 / 6_858_000.0, 11_500_000.0 / 12_192_000.0, 1_500_000.0 / 6_858_000.0);
    let mut dark = 0;
    for y in (y0 * 720.0) as u32..(y1 * 720.0) as u32 {
        for x in (x0 * 1280.0) as u32..(x1 * 1280.0) as u32 {
            let p = img.get_pixel(x, y);
            if close([p[0], p[1], p[2]], [0x1F, 0x2A, 0x44]) {
                dark += 1;
            }
        }
    }
    assert!(dark > 2000, "glyphes du titre absents ({dark} pixels)");
    // Rien au-dessus ni en dessous de la zone de titre (hors bandeau).
    assert!(close(px(&img, 0.5, 0.3), [0xEE, 0xEE, 0xEE]));
    // Un espace réservé vide n'est pas affiché en diaporama, même rempli.
    assert!(close(px(&img, 0.5, 0.5), [0xEE, 0xEE, 0xEE]), "{:?}", px(&img, 0.5, 0.5));
}

#[test]
fn groups_rotation_and_custom_geometry() {
    // Groupe : enfants dans un repère 0..1000 étiré sur la moitié gauche de la diapositive.
    let group = format!(
        r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="5" name="g"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="6096000" cy="6858000"/><a:chOff x="0" y="0"/><a:chExt cx="1000" cy="1000"/></a:xfrm></p:grpSpPr>{}</p:grpSp>"#,
        rect(0, 500, 1000, 500, "", r#"<a:solidFill><a:srgbClr val="00FFFF"/></a:solidFill>"#)
    );
    // Triangle libre (custGeom) dans la moitié droite.
    let cust = r#"<p:sp><p:nvSpPr><p:cNvPr id="6" name="c"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="6096000" y="0"/><a:ext cx="6096000" cy="6858000"/></a:xfrm><a:custGeom><a:pathLst><a:path w="100" h="100"><a:moveTo><a:pt x="0" y="0"/></a:moveTo><a:lnTo><a:pt x="100" y="0"/></a:lnTo><a:lnTo><a:pt x="0" y="100"/></a:lnTo><a:close/></a:path></a:pathLst></a:custGeom><a:solidFill><a:srgbClr val="FF00FF"/></a:solidFill></p:spPr></p:sp>"#;
    let pres = Presentation::from_bytes(deck(&[slide(&format!("{group}{cust}"), r#"showMasterSp="0""#)])).unwrap();
    let img = pres.render_slide(0, 400, 225).unwrap();
    assert!(close(px(&img, 0.25, 0.75), [0, 255, 255]));
    assert!(close(px(&img, 0.25, 0.25), [0xEE, 0xEE, 0xEE]));
    assert!(close(px(&img, 0.55, 0.1), [255, 0, 255]));
    assert!(close(px(&img, 0.95, 0.9), [0xEE, 0xEE, 0xEE]));
}

#[test]
fn bad_files_are_errors_not_panics() {
    assert!(Presentation::from_bytes(b"pas un zip".to_vec()).is_err());
    let not_pptx = zip(&[("word/document.xml", "<w:document/>".into())]);
    assert!(matches!(Presentation::from_bytes(not_pptx), Err(Error::NotAPresentation(_))));
    // XML de diapositive cassé : erreur au rendu, pas de panique.
    let pres = Presentation::from_bytes(deck(&["<p:sld><oops".into()])).unwrap();
    assert!(pres.render_slide(0, 100, 100).is_err());
    assert!(matches!(pres.render_slide(5, 100, 100), Err(Error::NoSuchSlide { .. })));
    assert!(matches!(pres.render_slide(0, 0, 100), Err(Error::InvalidSize { .. })));
    // Éléments inconnus, valeurs absurdes : ignorés.
    let odd = slide(
        r#"<p:foo/><p:sp><p:nvSpPr><p:cNvPr id="2" name="x"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm rot="abc"><a:off x="-99999999999" y="1e30"/><a:ext cx="-5" cy="NaN"/></a:xfrm><a:prstGeom prst="nimporte"/><a:solidFill><a:schemeClr val="inconnu"/></a:solidFill></p:spPr><p:txBody><a:bodyPr/><a:p><a:pPr lvl="99"/><a:r><a:rPr sz="-3"/><a:t>x</a:t></a:r></a:p></p:txBody></p:sp>"#,
        "",
    );
    let pres = Presentation::from_bytes(deck(&[odd])).unwrap();
    pres.render_slide(0, 200, 100).unwrap();
}

fn demo() -> Presentation {
    Presentation::open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo.pptx")).unwrap()
}

#[test]
fn demo_fixture_metadata() {
    let pres = demo();
    assert_eq!(pres.slide_count(), 9);
    assert_eq!(pres.slide_title(0).as_deref(), Some("Présentation de démonstration"));
    assert_eq!(pres.slide_title(1).as_deref(), Some("Puces et mise en forme"));
    assert!(pres.is_hidden(6));
    assert_eq!(pres.next_visible(5), Some(7));
    assert_eq!(pres.prev_visible(7), Some(5));
    assert_eq!(pres.next_visible(8), None);
    assert_eq!(pres.notes(1).as_deref(), Some("Penser à lancer la musique."));
    assert_eq!(pres.notes(0), None);
    assert_eq!(pres.slide_title(8).as_deref(), Some("Graphique en histogramme"));
}

#[test]
fn demo_fixture_renders() {
    let pres = demo();
    for i in 0..pres.slide_count() {
        let img = pres.render_slide(i, 480, 480).unwrap();
        assert_eq!(img.dimensions(), (480, 270));
    }
    // Ellipse verte (2E8B57) au centre de la 3e forme.
    let img = pres.render_slide(2, 1333, 750).unwrap();
    let c = px(&img, (0.6 + 2.0 * 3.1 + 1.3) / 13.333, (1.8 + 0.95) / 7.5);
    assert!(close(c, [0x2E, 0x8B, 0x57]), "{c:?}");
    // Image : coin rouge de l'image (rectangle 10..120 × 10..60 px sur 800 × 600).
    let img = pres.render_slide(3, 1333, 750).unwrap();
    let c = px(&img, (0.8 + 5.5 * 60.0 / 800.0) / 13.333, (1.8 + 5.5 * 35.0 / 800.0) / 7.5);
    assert!(close(c, [220, 40, 40]), "{c:?}");
    // Fond dégradé sombre en bas à gauche, plus clair en haut à droite.
    let img = pres.render_slide(7, 400, 225).unwrap();
    let (a, b) = (px(&img, 0.02, 0.97), px(&img, 0.98, 0.03));
    assert!(a[2] < b[2] && a[0] < 0x30, "{a:?} {b:?}");
    // Histogramme (valeurs 1, 3, 2) en couleur accent1 (4F81BD) : la barre B est plus haute
    // que la barre A.
    let img = pres.render_slide(8, 1280, 720).unwrap();
    let accent = [0x4F, 0x81, 0xBD];
    assert!(close(px(&img, 0.31, 0.58), accent));
    assert!(close(px(&img, 0.175, 0.69), accent));
    assert!(close(px(&img, 0.175, 0.55), [255, 255, 255]));
}
