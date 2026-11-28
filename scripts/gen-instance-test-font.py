#!/usr/bin/env python3
"""Write the variable test slice the font instancer's tests read, and its
reference instances.

Usage: python3 scripts/gen-instance-test-font.py <NotoSerifTC[wght].ttf>   (needs fontTools)

The slice is a few characters of Noto Serif TC (OFL-1.1), the variable Han
face: ideographs, full-width punctuation that `GPOS` adjusts per weight, and
Latin letters that kern. fontTools' `varLib.instancer` pins it at 400 and 700;
those two files are the outside reference the instancer is checked against.

It also writes `noto-serif-tc-ruby.ttf`, two bopomofo tone marks whose ruby
forms the slicer keeps only when a stylesheet asks for them.
"""
import pathlib
import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/moss-build/tests/fixtures/fonts"
TEXT = "永國書「」、。，（）AVTo "

font = TTFont(sys.argv[1])
options = subset.Options()
options.layout_features = ["*"]
options.hinting = False
options.notdef_outline = True
options.name_IDs = list(range(15))
subsetter = subset.Subsetter(options)
subsetter.populate(text=TEXT)
subsetter.subset(font)
OUT.mkdir(parents=True, exist_ok=True)
font.save(OUT / "noto-serif-tc-slice.ttf")
for weight in (400, 700):
    pinned = instancer.instantiateVariableFont(TTFont(OUT / "noto-serif-tc-slice.ttf"), {"wght": weight})
    pinned.save(OUT / f"noto-serif-tc-slice-{weight}.ttf")

ruby = TTFont(sys.argv[1])
subsetter = subset.Subsetter(options)
subsetter.populate(text="ˇˊ")
subsetter.subset(ruby)
ruby.save(OUT / "noto-serif-tc-ruby.ttf")
