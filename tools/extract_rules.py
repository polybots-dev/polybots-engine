#!/usr/bin/env python3
"""Copy the game's rule data out of an installed copy of The Battle of Polytopia (optional).

    python3 -m pip install UnityPy==1.25.4
    python3 tools/extract_rules.py "<game folder>" rules.json
    export ENGINE_FAST_RULES="$PWD/rules.json"

The rule data is the game's text asset GameLogicData28, inside the game's `data.unity3d`.
This reads that file only; it does not start or change the game. The engine reads one rule
data file (rules versions 123 and 124 share it), so the copy is checked against its SHA-256
and refused if the installed game carries other rule data.
"""

import argparse
import hashlib
from pathlib import Path

ASSET = "GameLogicData28"
SHA256 = "c6806c77963b5377bdbc2570bf0a3abece622e4e148883754dd190b02f890086"


def extract(data):
    import UnityPy

    for obj in UnityPy.load(str(data)).objects:
        if obj.type.name == "TextAsset":
            asset = obj.read()
            if asset.m_Name == ASSET:
                text = asset.m_Script
                if isinstance(text, str):
                    return text.encode("utf-8", "surrogateescape")
                return bytes(text)
    return None


def main():
    a = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    a.add_argument("game", help="the game's install folder")
    a.add_argument("out", type=Path, help="where to write the rule data")
    args = a.parse_args()
    found = sorted(Path(args.game).rglob("data.unity3d"), key=lambda p: len(p.parts))
    if not found:
        raise SystemExit(f"no data.unity3d under {args.game}")
    for data in found:
        raw = extract(data)
        if raw is None:
            continue
        digest = hashlib.sha256(raw).hexdigest()
        if digest != SHA256:
            raise SystemExit(f"{ASSET} in {data} has SHA-256 {digest}, not the supported rules")
        args.out.write_bytes(raw)
        print(f"wrote {args.out} ({len(raw)} bytes)")
        return
    raise SystemExit(f"no text asset {ASSET} in the game's data")


if __name__ == "__main__":
    main()
