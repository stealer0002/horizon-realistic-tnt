"""Writes the mod's TNT blocks of greater worth (TNT x2 up to TNT x1,000,000) and the remote
detonator (`realistic_tnt:detonator`) as content:

  realistic_tnt/data/realistic_tnt/horizon/block/<id>.json      the block (a plain, flammable block
                                                                 the mod lights itself: src/lib.rs
                                                                 `ValuedTnt`), breaking at once as TNT
  realistic_tnt/data/realistic_tnt/horizon/item/detonator.json  the remote detonator item
  realistic_tnt/data/realistic_tnt/horizon/creative_tab/tnt.json their creative tab
  realistic_tnt/data/realistic_tnt/loot_table/blocks/<id>.json   it drops itself
  realistic_tnt/data/realistic_tnt/recipe/<id>.json              crafting recipes
  realistic_tnt/assets/realistic_tnt/blockstates, models, items  how blocks and items look
  realistic_tnt/assets/realistic_tnt/textures/block, item        own pixel art, drawn here
  realistic_tnt/assets/realistic_tnt/lang/en_us.json, pt_br.json names

What each block is worth, in TNT, is in src/lib.rs (`VALUED_TNT`); unit tests check content.
Needs Pillow:   python tools/make_blocks.py
"""

import json
import pathlib

from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parent.parent / "realistic_tnt"
NS = "realistic_tnt"
DATA = ROOT / "data" / NS
ASSETS = ROOT / "assets" / NS

# id, worth in TNT, label on its band, body colour, band colour, label colour, made from (count, item).
TIERS = [
    ("tnt_x2", 2, "x2", (196, 42, 38), (236, 228, 206), (40, 30, 28), (2, "minecraft:tnt")),
    ("tnt_x10", 10, "x10", (222, 112, 30), (240, 230, 205), (48, 30, 18), (5, "tnt_x2")),
    ("tnt_x100", 100, "x100", (214, 172, 36), (246, 240, 214), (60, 44, 10), (9, "tnt_x10")),
    ("tnt_x1000", 1_000, "1K", (58, 150, 62), (232, 240, 220), (20, 50, 22), (9, "tnt_x100")),
    ("tnt_x10000", 10_000, "10K", (40, 138, 172), (222, 238, 244), (12, 44, 60), (9, "tnt_x1000")),
    ("tnt_x100000", 100_000, "100K", (128, 58, 172), (238, 226, 246), (44, 16, 64), (9, "tnt_x10000")),
    ("tnt_x1000000", 1_000_000, "1M", (38, 38, 44), (232, 200, 40), (24, 24, 24), (9, "tnt_x100000")),
]

FONT = {
    "x": ["...", "#.#", ".#.", "#.#", "..."],
    "0": ["###", "#.#", "#.#", "#.#", "###"],
    "1": [".#.", "##.", ".#.", ".#.", "###"],
    "2": ["###", "..#", "###", "#..", "###"],
    "K": ["#.#", "#.#", "##.", "#.#", "#.#"],
    "M": ["#.#", "###", "#.#", "#.#", "#.#"],
}


def shade(c, k):
    return tuple(max(0, min(255, int(v * k))) for v in c)


def side(body, band, ink, label):
    img = Image.new("RGBA", (16, 16))
    px = img.load()
    for x in range(16):
        # Sticks side by side, each lighter in its middle.
        k = (0.82, 1.0, 1.06, 0.92)[x % 4]
        for y in range(16):
            px[x, y] = shade(body, k * (0.94 if y in (0, 15) else 1.0)) + (255,)
    for x in range(16):
        px[x, 4] = shade(body, 0.5) + (255,)
        px[x, 12] = shade(body, 0.5) + (255,)
        for y in range(5, 12):
            px[x, y] = shade(band, 0.96 if (x + y) % 5 == 0 else 1.0) + (255,)
    width = len(label) * 4 - 1
    x0 = (16 - width) // 2
    for i, ch in enumerate(label):
        for row, line in enumerate(FONT[ch]):
            for col, on in enumerate(line):
                if on == "#":
                    px[x0 + i * 4 + col, 6 + row] = ink + (255,)
    return img


def top(body, bottom=False):
    img = Image.new("RGBA", (16, 16))
    px = img.load()
    for x in range(16):
        for y in range(16):
            rim = x in (0, 15) or y in (0, 15)
            k = 0.6 if rim else (0.85 if (x // 4 + y // 4) % 2 else 0.95)
            px[x, y] = shade(body, k * (0.8 if bottom else 1.0)) + (255,)
    if not bottom:
        # The fuse in the middle.
        for x in range(6, 10):
            for y in range(6, 10):
                px[x, y] = (70, 64, 60, 255) if x in (6, 9) or y in (6, 9) else (150, 140, 128, 255)
        px[7, 7] = px[8, 8] = (30, 26, 24, 255)
    return img


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(value, Image.Image):
        value.save(path)
    else:
        path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def grouped(n, sep):
    return f"{n:,}".replace(",", sep)


def detonator_texture():
    """16x16 pixel-art remote detonator: metallic handheld unit, antenna with redstone tip, red button."""
    img = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
    px = img.load()
    # Antenna (top right-of-center)
    for y in range(2, 6):
        px[10, y] = (150, 158, 168, 255)
        px[9, y] = (92, 98, 108, 255) if y == 5 else (0, 0, 0, 0)
    # Glowing redstone tip
    px[9, 1] = (232, 48, 36, 255)
    px[10, 1] = (255, 96, 72, 255)
    px[10, 2] = (210, 36, 28, 255)
    # Outer dark iron casing (x=4..11, y=6..14)
    for y in range(6, 15):
        for x in range(4, 12):
            border = x in (4, 11) or y in (6, 14)
            if border:
                px[x, y] = (46, 50, 58, 255)
            else:
                k = 1.12 if (x <= 6 or y == 7) else (0.86 if x >= 10 or y >= 13 else 1.0)
                px[x, y] = shade((112, 120, 132), k) + (255,)
    # Dark recessed display / housing slit near top of body
    for x in range(6, 10):
        px[x, 8] = (32, 36, 42, 255)
    px[6, 8] = (210, 54, 42, 255)
    # Bright red trigger button (x=6..9, y=10..12)
    for y in range(10, 13):
        for x in range(6, 10):
            if (x, y) == (6, 10):
                px[x, y] = (255, 110, 92, 255)
            elif x == 9 or y == 12:
                px[x, y] = (150, 22, 18, 255)
            else:
                px[x, y] = (218, 36, 28, 255)
    return img


def write_detonator(en, pt):
    write(DATA / "horizon" / "item" / "detonator.json", {
        "template": "minecraft:stick",
        "max_stack_size": 1,
    })
    write(DATA / "recipe" / "detonator.json", {
        "type": "minecraft:crafting_shaped",
        "category": "redstone",
        "key": {
            "T": "minecraft:redstone_torch",
            "I": "minecraft:iron_ingot",
            "R": "minecraft:redstone",
        },
        "pattern": [
            " T ",
            "IRI",
            " I ",
        ],
        "result": {"id": f"{NS}:detonator", "count": 1},
    })
    write(ASSETS / "items" / "detonator.json", {
        "model": {"type": "minecraft:model", "model": f"{NS}:item/detonator"},
    })
    write(ASSETS / "models" / "item" / "detonator.json", {
        "parent": "minecraft:item/generated",
        "textures": {"layer0": f"{NS}:item/detonator"},
    })
    write(ASSETS / "textures" / "item" / "detonator.png", detonator_texture())
    en[f"item.{NS}.detonator"] = "Remote Detonator"
    pt[f"item.{NS}.detonator"] = "Detonador remoto"


def main():
    en, pt = {}, {}
    for block, worth, label, body, band, ink, (count, made_of) in TIERS:
        # Not on TNT's template: the game would light it as one TNT. A plank burns, the mod's code on
        # it (`ValuedTnt`) lights it, and it breaks at once and sounds as TNT does.
        write(DATA / "horizon" / "block" / f"{block}.json", {
            "template": "minecraft:oak_planks",
            "properties": {"hardness": 0.0, "sounds": "minecraft:tnt"},
            "item": True,
        })
        write(DATA / "loot_table" / "blocks" / f"{block}.json", {
            "type": "minecraft:block",
            "pools": [{
                "rolls": 1,
                "entries": [{"type": "minecraft:item", "name": f"{NS}:{block}"}],
                "conditions": [{"condition": "minecraft:survives_explosion"}],
            }],
        })
        item = made_of if ":" in made_of else f"{NS}:{made_of}"
        write(DATA / "recipe" / f"{block}.json", {
            "type": "minecraft:crafting_shapeless",
            "category": "redstone",
            "ingredients": [item] * count,
            "result": {"id": f"{NS}:{block}", "count": 1},
        })
        model = f"{NS}:block/{block}"
        write(ASSETS / "blockstates" / f"{block}.json",
              {"variants": {"": {"model": model}}})
        write(ASSETS / "models" / "block" / f"{block}.json", {
            "parent": "minecraft:block/cube_bottom_top",
            "textures": {
                "side": f"{NS}:block/{block}_side",
                "top": f"{NS}:block/{block}_top",
                "bottom": f"{NS}:block/{block}_bottom",
            },
        })
        write(ASSETS / "items" / f"{block}.json", {"model": {"type": "minecraft:model", "model": model}})
        textures = ASSETS / "textures" / "block"
        write(textures / f"{block}_side.png", side(body, band, ink, label))
        write(textures / f"{block}_top.png", top(body))
        write(textures / f"{block}_bottom.png", top(body, bottom=True))
        en[f"block.{NS}.{block}"] = f"TNT x{grouped(worth, ',')}"
        pt[f"block.{NS}.{block}"] = f"TNT x{grouped(worth, '.')}"
    write_detonator(en, pt)
    en[f"itemGroup.{NS}.tnt"] = "Realistic TNT"
    pt[f"itemGroup.{NS}.tnt"] = "TNT realista"
    write(DATA / "horizon" / "creative_tab" / "tnt.json", {
        "title": f"itemGroup.{NS}.tnt",
        "icon": f"{NS}:tnt_x1000000",
        "items": ["minecraft:tnt", f"{NS}:detonator"] + [f"{NS}:{t[0]}" for t in TIERS],
    })
    write(ASSETS / "lang" / "en_us.json", en)
    write(ASSETS / "lang" / "pt_br.json", pt)
    print(f"{len(TIERS)} blocks + detonator written under {ROOT}")


if __name__ == "__main__":
    main()
