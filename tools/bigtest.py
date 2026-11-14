#!/usr/bin/env python3
"""Sets off a cube of TNT in the test game (world TntTest, its own socket: see shoot.py), with cows
standing at several distances, photographs it and reports what the blast did to the cows.

    python bigtest.py NAME --cube SIDE [--distance D] [--height H] [--times 0.1,1,3] [--cows 1,2,3]

`--cows` are distances in crater radii (crater radius = 4.5 * cube root of the TNT count).
Pictures go to testgame-home/shots/NAME-*.png and NAME-sheet.jpg.
"""
import argparse
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import shoot  # noqa: E402

ctl = shoot.ctl


def ground(x, z):
    """The height of the ground in column (x, z): the highest block that is ground, from y 160 down."""
    for y in range(160, -40, -1):
        if shoot.is_ground(shoot.block(x, y, z)):
            return y
    raise SystemExit(f"no ground in column {x}, {z}")


def cows_near(center, radius):
    """The cows within `radius` of `center`: id -> (position, health)."""
    reply = ctl("entities", int(radius) + 8, "cow")
    found = {}
    for e in reply.get("entities", reply if isinstance(reply, list) else []):
        pos = e.get("pos") or e.get("position")
        if pos is None:
            continue
        if math.dist(pos, center) <= radius:
            found[e.get("id")] = (pos, e.get("health"))
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("name")
    parser.add_argument("--cube", type=int, required=True, help="the side of the TNT cube")
    parser.add_argument("--distance", type=int, default=0, help="blocks from the cube (default: 3 crater radii)")
    parser.add_argument("--height", type=int, default=12)
    parser.add_argument("--times", default="0.1,0.6,1.5,4,8")
    parser.add_argument("--cows", default="0.7,1.5,2.5")
    parser.add_argument("--spacing", type=int, default=1, help="lay the TNT one layer thick, this far apart (a grid)")
    args = parser.parse_args()
    count = args.cube ** 3 if args.spacing == 1 else args.cube ** 2
    crater = 4.5 * count ** (1 / 3)
    distance = args.distance or max(24, int(crater * 3))

    state = ctl("state")
    if state.get("world") != shoot.WORLD:
        raise SystemExit(f"the game plays {state.get('world')!r}, not {shoot.WORLD!r}: refusing")
    ctl("command", "gamemode creative")
    ctl("time", 6000)
    ctl("command", "weather clear")
    px, py, pz = (int(round(v - 0.5)) for v in state["player"]["position"])
    sx, sz = px, pz + distance
    ctl("tp", sx + 0.5, py + 40, sz + 0.5)
    ctl("wait-ready", 120, timeout=180)
    sy = ground(sx, sz)
    # The player's perch, looking at the cube.
    feet = ground(px, pz) + args.height
    ctl("setblock", px, feet, pz, "glass")
    ctl("tp", px + 0.5, feet + 1, pz + 0.5)
    ctl("wait-ready", 60, timeout=120)
    half = args.cube // 2
    x0, z0, y0 = sx - half, sz - half, sy + 1
    if args.spacing == 1:
        ctl("fill", x0, y0, z0, x0 + args.cube - 1, y0 + args.cube - 1, z0 + args.cube - 1, "tnt")
        centre = [sx + 0.5, y0 + args.cube / 2, sz + 0.5]
        light = (sx, y0 + args.cube, sz)
    else:
        # A grid one layer thick; lit from a corner, the farthest point from the rest.
        for i in range(args.cube):
            for k in range(args.cube):
                ctl("setblock", x0 + i * args.spacing, ground(x0 + i * args.spacing, z0 + k * args.spacing) + 1,
                    z0 + k * args.spacing, "tnt")
        centre = [x0 + args.cube * args.spacing / 2, y0 + 0.5, z0 + args.cube * args.spacing / 2]
        light = (x0, ground(x0, z0) + 1, z0)
    # Cows to the side, at the distances asked, standing on the ground.
    placed = []
    for k, d in enumerate(float(v) for v in args.cows.split(",")):
        cx = int(sx + crater * d + 1)
        cz = sz + (k - 1) * 3
        cy = ground(cx, cz) + 1
        ctl("summon", "cow", cx + 0.5, cy, cz + 0.5)
        placed.append((d, [cx + 0.5, cy, cz + 0.5]))
    ctl("wait", 5)
    before = cows_near(centre, crater * 4 + 12)
    ctl("look-at", centre[0], centre[1] + crater * 0.6, centre[2])
    ctl("key", "f1", "tap")
    heard_before = ctl("status").get("heard", {}).get("minecraft:entity.generic.explode", 0)
    ctl("setblock", *light, "redstone_block")
    ctl("wait", 79)
    times = sorted(float(t) for t in args.times.split(","))
    shots, last = [], 0.0
    for t in times:
        ticks = round((t - last) * 20)
        if ticks > 0:
            ctl("wait", ticks)
            last += ticks / 20
        path = os.path.join(shoot.SHOTS, f"{args.name}-{t:05.2f}.png").replace("\\", "/")
        ctl("screenshot", path)
        shots.append(path)
    ctl("key", "f1", "tap")
    # Each explosion asks for vanilla's sound (dropped by the mod), and each blast for the mod's
    # own: one blast of everything together asks for two.
    explosions = ctl("status").get("heard", {}).get("minecraft:entity.generic.explode", 0) - heard_before
    after = cows_near(centre, crater * 6 + 40)
    shoot.sheet(shots, os.path.join(shoot.SHOTS, f"{args.name}-sheet.jpg"), columns=min(4, len(shots)))
    report = []
    for cid, (pos, health) in before.items():
        d0 = math.dist(pos, centre)
        if cid in after:
            pos1, health1 = after[cid]
            report.append({"start": round(d0 / crater, 2), "moved": round(math.dist(pos, pos1), 1),
                           "health": [health, health1]})
        else:
            report.append({"start": round(d0 / crater, 2), "gone": True})
    print(json.dumps({"tnt": count, "explode_sounds": explosions, "crater": round(crater, 1), "site": [sx, sy, sz],
                      "player": [px, feet + 1, pz], "cows": sorted(report, key=lambda r: r["start"])}))


if __name__ == "__main__":
    sys.exit(main())
