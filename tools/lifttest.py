#!/usr/bin/env python3
"""Sets off a big mass of TNT in the test game (world TntTest, its own socket: see shoot.py) some
way from a forest, and checks that its shockwave tears up the land beyond the crater: counts the
trees, plants and turf in boxes round the forest before and after, and photographs the shockwave
passing over it from farther out.

    python lifttest.py [--side 27] [--forest X Z] [--distance 200] [--watch 300]

The forest is at FOREST; the mass goes DISTANCE blocks west of it, and the player watches from
WATCH blocks east of the mass, 50 blocks up, looking at the forest.
"""
import argparse
import collections
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bigtest  # noqa: E402
import shoot  # noqa: E402

ctl = shoot.ctl
LOOSE_ENDS = ("_leaves", "_log", "short_grass", "tall_grass", "fern", "_sapling", "snow")


def fuse_ticks(n):
    """The mod's `fuse_ticks`."""
    reach = 4.8 * n ** (1 / 3) * 3.75
    return round(min(80 + max(reach - 18.0, 0.0) / 10.0 * 20.0, 32767))


def survey(cx, cz, spread=48):
    """The loose blocks and the turf in nine 16 by 16 columns round (cx, cz), y 60 to 115."""
    total = collections.Counter()
    for dx in (-spread, 0, spread):
        for dz in (-spread, 0, spread):
            for y0 in (60, 88):
                reply = ctl("blocks", cx + dx - 8, y0, cz + dz - 8, cx + dx + 7, y0 + 27, cz + dz + 7)
                for block in reply.get("blocks", []):
                    total[block[3].split("[")[0].removeprefix("minecraft:")] += 1
    loose = sum(v for k, v in total.items() if k.endswith(LOOSE_ENDS))
    return {"loose": loose, "grass_block": total["grass_block"],
            "leaves": sum(v for k, v in total.items() if k.endswith("_leaves")),
            "logs": sum(v for k, v in total.items() if k.endswith("_log"))}


# Buildings put up beside the forest before the blast (east of the mass, 70 blocks south of the line
# to the forest): (name, how far from the mass, [(block, dx0, dy0, dz0, dx1, dy1, dz1)]).
BUILDINGS = [
    ("brick tower 150", 150, [("bricks", 0, 0, 0, 4, 7, 4)]),
    ("brick house, wooden roof 230", 230, [("bricks", 0, 0, 0, 4, 5, 4), ("oak_planks", 0, 6, 0, 4, 6, 4)]),
    ("plank house 270", 270, [("oak_planks", 0, 0, 0, 4, 4, 4)]),
    ("glass wall 330", 330, [("glass", 0, 0, 0, 0, 3, 6)]),
]


def contents(x0, y0, z0, x1, y1, z1):
    reply = ctl("blocks", x0, y0, z0, x1, y1, z1)
    counted = collections.Counter(b[3].split("[")[0].removeprefix("minecraft:") for b in reply.get("blocks", []))
    return dict(counted.most_common(4))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--side", type=int, default=27)
    parser.add_argument("--forest", type=int, nargs=2, default=[202, -1557])
    parser.add_argument("--distance", type=int, default=200)
    parser.add_argument("--watch", type=int, default=300)
    parser.add_argument("--no-build", dest="build", action="store_false",
                        help="light the TNT already there instead of filling a cube")
    args = parser.parse_args()
    state = ctl("state")
    if state.get("world") != shoot.WORLD:
        raise SystemExit(f"the game plays {state.get('world')!r}, not {shoot.WORLD!r}: refusing")
    ctl("command", "gamemode creative")
    fx, fz = args.forest
    sx, sz = fx - args.distance, fz
    wx = sx + args.watch
    shots = os.path.join(shoot.SHOTS, "lift")
    os.makedirs(shots, exist_ok=True)

    # The forest, before.
    ctl("command", f"tp @s {fx} 170 {fz}")
    ctl("wait", 120)
    before = survey(fx, fz)
    wy = bigtest.ground(fx, fz) + 50
    # The buildings, each put up while the player is near it.
    buildings = []
    for name, far, parts in BUILDINGS:
        bx, bz = sx + far, sz - 70
        ctl("command", f"tp @s {bx} 170 {bz}")
        ctl("wait", 60)
        by = bigtest.ground(bx, bz) + 1
        for block, dx0, dy0, dz0, dx1, dy1, dz1 in parts:
            ctl("fill", bx + dx0, by + dy0, bz + dz0, bx + dx1, by + dy1, bz + dz1, block)
        box = (bx, by, bz, bx + 4, by + 8, bz + 6)
        buildings.append((name, box, contents(*box)))
    # The mass, lit from above its middle while the player is near (a block can only be set where
    # the window has its chunk), unless it is already there (--no-build).
    ctl("command", f"tp @s {sx} 170 {sz}")
    ctl("wait", 120)
    if args.build:
        sy = bigtest.ground(sx, sz)
        half = args.side // 2
        x0, z0, y0 = sx - half, sz - half, sy + 1
        for y in range(y0, y0 + args.side):
            ctl("fill", x0, y, z0, x0 + args.side - 1, y, z0 + args.side - 1, "tnt")
    top = next(y for y in range(250, -60, -1) if shoot.block(sx, y, sz) == "tnt")
    ctl("setblock", sx, top + 1, sz, "redstone_block")
    ctl("wait", 5)
    bar = ctl("messages").get("actionbar")
    print("lit:", bar)
    ctl("command", f"tp @s {wx} {wy} {sz}")
    ctl("wait", 100)
    ctl("look-at", fx - 40, wy - 45, fz)
    ctl("screenshot", os.path.join(shots, "before.png").replace("\\", "/"))
    # It goes off when the bar says so; photographs from then until the front is past the forest.
    while "explodiu" not in (ctl("messages").get("actionbar") or ""):
        time.sleep(0.1)
    boom = ctl("status")["ticks"]
    taken = []
    falling = []
    marks = [5, 20, 30, 40, 50, 65, 90, 130]
    while True:
        t = ctl("status")
        age = t["ticks"] - boom
        falling.append((age, t.get("server", {}).get("entities", {}).get("minecraft:falling_block", 0)))
        if marks and age >= marks[0]:
            path = os.path.join(shots, f"t{marks.pop(0):+04d}.png")
            ctl("screenshot", path.replace("\\", "/"))
            taken.append(path)
        if age > 240:
            break
        time.sleep(0.1)
    after = survey(fx, fz)
    standing = []
    for name, box, before_box in buildings:
        bx, by, bz = box[0], box[1], box[2]
        ctl("command", f"tp @s {bx} {by + 30} {bz}")
        ctl("wait", 40)
        standing.append({"building": name, "before": before_box, "after": contents(*box)})
    try:
        shoot.sheet(taken, os.path.join(shots, "sheet.jpg"))
    except Exception as e:  # noqa: BLE001 - the pictures are a bonus
        print("no sheet:", e)
    print(json.dumps({"lit": bar, "forest_before": before, "forest_after": after,
                      "buildings": standing, "falling_near_player": falling[::5], "shots": taken},
                     ensure_ascii=False))


if __name__ == "__main__":
    sys.exit(main())
