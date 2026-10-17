"""Writes the arcade sets' full names for the shelf, from libretro's arcade databases.

    python tools/arcade/names.py

Arcade games are zips named for their set, sf2ce.zip, which says little on a label. This reads
the FinalBurn Neo and MAME 2003-Plus databases in libretro-database, the two arcade cores unslot
ships, and writes crates/slot-store/assets/arcade_names.tsv: one `set<TAB>name` a line, FinalBurn
Neo's name first where both know a set. slot-store builds it in and looks each zip up by its stem.
"""

from __future__ import annotations

import re
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "crates" / "slot-store" / "assets" / "arcade_names.tsv"
BASE = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat"
DATS = ["fbneo-split/FBNeo - Arcade Games.dat", "mame-nonmerged/MAME 2003-Plus.dat"]

GAME = re.compile(r'^game \(\s*$\s*name "(?P<name>[^"]*)".*?rom \( name (?P<set>\S+)\.zip ', re.M | re.S)


def sets(text: str) -> dict[str, str]:
    out = {}
    for block in text.split("\ngame (")[1:]:
        m = GAME.search("game (" + block)
        if m and m["set"].lower() not in out:
            out[m["set"].lower()] = " ".join(m["name"].split())
    return out


def main() -> None:
    names: dict[str, str] = {}
    for dat in DATS:
        url = f"{BASE}/{urllib.parse.quote(dat)}"
        with urllib.request.urlopen(url) as r:
            text = r.read().decode("utf-8", "replace")
        for s, n in sets(text).items():
            names.setdefault(s, n)
        print(f"{dat}: {len(names)} sets so far")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open("w", encoding="utf-8", newline="\n") as f:
        for s in sorted(names):
            f.write(f"{s}\t{names[s]}\n")
    print(f"wrote {OUT.relative_to(ROOT)}: {len(names)} sets")


main()
