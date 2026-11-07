"""Génère les présentations de test (python-pptx + Pillow).

    python3 make_fixtures.py   # écrit demo.pptx à côté de ce script
"""

import io
import os

from PIL import Image, ImageDraw
from pptx import Presentation
from pptx.chart.data import CategoryChartData
from pptx.dml.color import RGBColor
from pptx.enum.chart import XL_CHART_TYPE
from pptx.enum.dml import MSO_THEME_COLOR
from pptx.enum.shapes import MSO_CONNECTOR, MSO_SHAPE
from pptx.enum.text import MSO_ANCHOR, PP_ALIGN
from pptx.oxml.ns import qn
from pptx.util import Emu, Inches, Pt

HERE = os.path.dirname(os.path.abspath(__file__))


def picture(w, h):
    img = Image.new("RGB", (w, h), (30, 60, 110))
    d = ImageDraw.Draw(img)
    for i in range(0, w, 40):
        d.line([(i, 0), (i, h)], fill=(60, 110, 170), width=3)
    d.ellipse([w * 0.25, h * 0.2, w * 0.75, h * 0.8], fill=(250, 190, 40))
    d.rectangle([10, 10, 120, 60], fill=(220, 40, 40))
    buf = io.BytesIO()
    img.save(buf, "PNG")
    buf.seek(0)
    return buf


def demo():
    prs = Presentation()
    prs.slide_width = Inches(13.333)
    prs.slide_height = Inches(7.5)
    L = prs.slide_layouts

    # 1. Diapositive de titre
    s = prs.slides.add_slide(L[0])
    s.shapes.title.text = "Présentation de démonstration"
    s.placeholders[1].text = "CueStar — rendu PowerPoint maison"

    # 2. Titre et contenu : puces sur plusieurs niveaux, mise en forme
    s = prs.slides.add_slide(L[1])
    s.shapes.title.text = "Puces et mise en forme"
    tf = s.placeholders[1].text_frame
    tf.text = "Premier niveau de puce"
    p = tf.add_paragraph()
    p.text = "Deuxième niveau, un peu plus petit"
    p.level = 1
    p = tf.add_paragraph()
    p.text = "Troisième niveau"
    p.level = 2
    p = tf.add_paragraph()
    r = p.add_run()
    r.text = "Gras, "
    r.font.bold = True
    r = p.add_run()
    r.text = "italique, "
    r.font.italic = True
    r = p.add_run()
    r.text = "souligné, "
    r.font.underline = True
    r = p.add_run()
    r.text = "en couleur"
    r.font.color.rgb = RGBColor(0xC0, 0x20, 0x20)
    p = tf.add_paragraph()
    p.text = "Une longue phrase qui doit revenir à la ligne automatiquement parce qu'elle dépasse largement la largeur de la zone de texte prévue."
    s.notes_slide.notes_text_frame.text = "Penser à lancer la musique."

    # 3. Formes
    s = prs.slides.add_slide(L[5])
    s.shapes.title.text = "Formes"
    shapes = [
        (MSO_SHAPE.RECTANGLE, "Rectangle"),
        (MSO_SHAPE.ROUNDED_RECTANGLE, "Arrondi"),
        (MSO_SHAPE.OVAL, "Ellipse"),
        (MSO_SHAPE.ISOSCELES_TRIANGLE, "Triangle"),
        (MSO_SHAPE.STAR_5_POINT, ""),
        (MSO_SHAPE.CHEVRON, "Chevron"),
        (MSO_SHAPE.RIGHT_ARROW, "Flèche"),
        (MSO_SHAPE.HEXAGON, "Hexagone"),
    ]
    for i, (kind, label) in enumerate(shapes):
        x = Inches(0.6 + (i % 4) * 3.1)
        y = Inches(1.8 + (i // 4) * 2.6)
        sh = s.shapes.add_shape(kind, x, y, Inches(2.6), Inches(1.9))
        sh.text = label
        if i == 2:
            sh.fill.solid()
            sh.fill.fore_color.rgb = RGBColor(0x2E, 0x8B, 0x57)
            sh.line.color.rgb = RGBColor(0, 0, 0)
            sh.line.width = Pt(3)
        if i == 3:
            sh.fill.gradient()
            sh.fill.gradient_angle = 90
        if i == 5:
            sh.rotation = 20
        if i == 7:
            sh.fill.background()
            sh.line.color.theme_color = MSO_THEME_COLOR.ACCENT_2
            sh.line.width = Pt(2)
            sh.text_frame.paragraphs[0].runs[0].font.color.theme_color = MSO_THEME_COLOR.ACCENT_2
    c = s.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, Inches(0.6), Inches(7.0), Inches(12.6), Inches(6.6))
    c.line.width = Pt(2.5)
    ln = c.line._get_or_add_ln()
    tail = ln.makeelement(qn("a:tailEnd"), {"type": "triangle"})
    ln.append(tail)

    # 4. Images (dont une recadrée)
    s = prs.slides.add_slide(L[5])
    s.shapes.title.text = "Images"
    s.shapes.add_picture(picture(800, 600), Inches(0.8), Inches(1.8), Inches(5.5))
    pic = s.shapes.add_picture(picture(800, 600), Inches(7.0), Inches(1.8), Inches(4), Inches(4))
    pic.crop_left = 0.25
    pic.crop_right = 0.25

    # 5. Tableau
    s = prs.slides.add_slide(L[5])
    s.shapes.title.text = "Tableau"
    rows, cols = 4, 3
    t = s.shapes.add_table(rows, cols, Inches(1.5), Inches(2), Inches(10), Inches(3)).table
    data = [["Titre", "Durée", "Note"], ["Ouverture", "3 min", "Fondu"], ["Discours", "12 min", "Micro HF"], ["Clôture", "5 min", "Musique"]]
    for r in range(rows):
        for c_ in range(cols):
            t.cell(r, c_).text = data[r][c_]

    # 6. Groupe
    s = prs.slides.add_slide(L[5])
    s.shapes.title.text = "Groupe et zone de texte"
    g = s.shapes.add_group_shape()
    a = g.shapes.add_shape(MSO_SHAPE.OVAL, Inches(1), Inches(2), Inches(2), Inches(2))
    b = g.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(3.5), Inches(2.5), Inches(3), Inches(1))
    b.text = "dans un groupe"
    tb = s.shapes.add_textbox(Inches(7.5), Inches(2), Inches(5), Inches(3))
    tb.text_frame.word_wrap = True
    tb.text_frame.text = "Zone de texte centrée verticalement"
    tb.text_frame.vertical_anchor = MSO_ANCHOR.MIDDLE
    tb.line.color.rgb = RGBColor(0x80, 0x80, 0x80)
    tb.text_frame.paragraphs[0].alignment = PP_ALIGN.CENTER
    tb.text_frame.paragraphs[0].runs[0].font.size = Pt(28)
    p = tb.text_frame.add_paragraph()
    p.text = "Deuxième paragraphe à droite"
    p.alignment = PP_ALIGN.RIGHT

    # 7. Diapositive masquée
    s = prs.slides.add_slide(L[5])
    s.shapes.title.text = "Masquée"
    s._element.set("show", "0")

    # 8. Fond dégradé, liste numérotée
    s = prs.slides.add_slide(L[1])
    s.background.fill.gradient()
    s.background.fill.gradient_angle = 45
    stops = s.background.fill.gradient_stops
    stops[0].color.rgb = RGBColor(0x10, 0x20, 0x40)
    stops[1].color.rgb = RGBColor(0x40, 0x80, 0xC0)
    s.shapes.title.text = "Liste numérotée sur fond dégradé"
    s.shapes.title.text_frame.paragraphs[0].runs[0].font.color.rgb = RGBColor(255, 255, 255)
    tf = s.placeholders[1].text_frame
    for i, txt in enumerate(["Accueil du public", "Mot d'ouverture", "Projection", "Remerciements"]):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.text = txt
        pPr = p._p.get_or_add_pPr()
        for tag in ("a:buNone", "a:buChar", "a:buAutoNum"):
            for e in pPr.findall(qn(tag)):
                pPr.remove(e)
        pPr.append(pPr.makeelement(qn("a:buAutoNum"), {"type": "arabicPeriod"}))
        p.runs[0].font.color.rgb = RGBColor(255, 255, 255)

    # 9. Graphique (non pris en charge : la zone reste vide) et texte
    s = prs.slides.add_slide(L[5])
    s.shapes.title.text = "Graphique en histogramme"
    cd = CategoryChartData()
    cd.categories = ["A", "B", "C"]
    cd.add_series("Série", (1, 3, 2))
    s.shapes.add_chart(XL_CHART_TYPE.COLUMN_CLUSTERED, Inches(1), Inches(2), Inches(6), Inches(4), cd)

    prs.save(os.path.join(HERE, "demo.pptx"))


if __name__ == "__main__":
    demo()
