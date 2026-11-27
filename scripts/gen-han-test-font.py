#!/usr/bin/env python3
"""Write the small Han test fonts the font slicing tests read.

Usage: python3 scripts/gen-han-test-font.py   (needs fontTools)

Every glyph is a box, so the files are a few kilobytes yet map 1,200 Han
ideographs plus ASCII, enough to be treated as a Han face. `han-boxes.ttf`
allows subsetting; `han-boxes-no-subset.ttf` sets OS/2 fsType's
no-subsetting bit. Both carry hinting tables (`fpgm`, `prep`, `cvt `), which
a slice drops. `han-boxes-wght.ttf` adds a `wght` axis from 100 to 900 whose
boxes widen with the weight, so a slice can be pinned at one weight;
`han-boxes-wght-composite.ttf` makes one of its glyphs a composite, which the
instancer refuses. `han-boxes-bad-gsub.ttf` carries a `GSUB` whose lists lie past
its end, which the subsetter refuses.
"""
import array
import pathlib

from fontTools.fontBuilder import FontBuilder
from fontTools.ttLib import newTable
from fontTools.ttLib.tables.DefaultTable import DefaultTable
from fontTools.ttLib.tables import ttProgram
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.pens.ttGlyphPen import TTGlyphPen

OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/moss-build/tests/fixtures/fonts"
CODEPOINTS = list(range(0x20, 0x7F)) + list(range(0x4E00, 0x4E00 + 1200)) + [0x20000, 0x20001]


def box():
    pen = TTGlyphPen(None)
    pen.moveTo((100, -100))
    pen.lineTo((100, 800))
    pen.lineTo((900, 800))
    pen.lineTo((900, -100))
    pen.closePath()
    return pen.glyph()


def build(fs_type):
    names = [".notdef"] + [f"u{cp:04X}" for cp in CODEPOINTS]
    fb = FontBuilder(1000, isTTF=True)
    fb.setupGlyphOrder(names)
    fb.setupCharacterMap({cp: f"u{cp:04X}" for cp in CODEPOINTS})
    fb.setupGlyf({name: box() for name in names})
    fb.setupHorizontalMetrics({name: (1000, 100) for name in names})
    fb.setupHorizontalHeader(ascent=880, descent=-120)
    fb.setupNameTable({"familyName": "Han Boxes", "styleName": "Regular"})
    fb.setupOS2(sTypoAscender=880, sTypoDescender=-120, usWinAscent=880, usWinDescent=120, fsType=fs_type)
    fb.setupPost()
    for tag in ("fpgm", "prep"):
        table = newTable(tag)
        table.program = ttProgram.Program()
        table.program.fromAssembly(["PUSHB[ ]", "0", "POP[ ]"])
        fb.font[tag] = table
    fb.font["cvt "] = newTable("cvt ")
    fb.font["cvt "].values = array.array("h", [0, 100])
    return fb.font


def variable():
    fb = FontBuilder(1000, isTTF=True)
    fb.font = build(0)
    fb.setupFvar([("wght", 100, 400, 900, "Weight")], [])
    # The box's right side moves 100 units at 900; the phantom points stay.
    wider = TupleVariation({"wght": (0, 1, 1)}, [None, None, (100, 0), (100, 0), None, None, None, None])
    fb.setupGvar({name: [wider] for name in fb.font.getGlyphOrder()})
    return fb.font


def composite():
    from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent
    font = variable()
    component = GlyphComponent()
    component.glyphName, component.x, component.y, component.flags = "u4E00", 0, 0, 0
    glyph = Glyph()
    glyph.numberOfContours, glyph.components = -1, [component]
    font["glyf"]["u4E01"] = glyph
    font["gvar"].variations["u4E01"] = []
    return font


def bad_gsub():
    font = build(0)
    table = DefaultTable("GSUB")
    # Version 1.0 whose script, feature and lookup lists all lie past its end.
    table.data = b"\x00\x01\x00\x00" + b"\xff\xf0" * 3
    font["GSUB"] = table
    return font


OUT.mkdir(parents=True, exist_ok=True)
build(0).save(OUT / "han-boxes.ttf")
build(0x0100).save(OUT / "han-boxes-no-subset.ttf")
variable().save(OUT / "han-boxes-wght.ttf")
composite().save(OUT / "han-boxes-wght-composite.ttf")
bad_gsub().save(OUT / "han-boxes-bad-gsub.ttf")
