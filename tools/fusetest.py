#!/usr/bin/env python3
"""Lights a cube of TNT in the test game (world TntTest, its own socket: see shoot.py) and checks
that it burns as long as the mod says (`fuse_ticks` in api/src/lib.rs), with the countdown shown
over the hotbar.

    python fusetest.py [--cube SIDE] [--distance D]
"""
import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bigtest  # noqa: E402
import shoot  # noqa: E402

ctl = shoot.ctl


def fuse_ticks(n):
    """The mod's `fuse_ticks`: vanilla's 80, plus the time to fly out of the shockwave's reach at
    10 blocks a second."""
    reach = 4.8 * n ** (1 / 3) * 3.75
    return round(min(80 + max(reach - 18.0, 0.0) / 10.0 * 20.0, 32767))


def game_ticks():
    return ctl("status")["ticks"]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--cube", type=int, default=3)
    parser.add_argument("--distance", type=int, default=40)
    parser.add_argument("--step", type=int, default=5, help="ticks between looks")
    args = parser.parse_args()
    count = args.cube ** 3

    state = ctl("state")
    if state.get("world") != shoot.WORLD:
        raise SystemExit(f"the game plays {state.get('world')!r}, not {shoot.WORLD!r}: refusing")
    ctl("command", "gamemode creative")
    px, py, pz = (int(round(v - 0.5)) for v in state["player"]["position"])
    sx, sz = px, pz + args.distance
    sy = bigtest.ground(sx, sz)
    half = args.cube // 2
    x0, z0, y0 = sx - half, sz - half, sy + 1
    ctl("fill", x0, y0, z0, x0 + args.cube - 1, y0 + args.cube - 1, z0 + args.cube - 1, "tnt")
    ctl("look-at", sx + 0.5, y0 + args.cube / 2, sz + 0.5)
    # Lit from above its middle: one TNT lit, the rest set off by it.
    ctl("setblock", sx, y0 + args.cube, sz, "redstone_block")
    # The game's own tick count: the looks below take time of their own.
    lit = game_ticks()
    ticks, gone, bars = 0, None, []
    limit = fuse_ticks(count) + 200
    while ticks < limit:
        ctl("wait", args.step)
        ticks += args.step
        if gone is None and shoot.block(x0, y0, z0) != "tnt":
            gone = game_ticks() - lit
        bar = ctl("messages")
        if isinstance(bar, dict):
            line = bar.get("action_bar") or bar.get("actionbar") or bar.get("overlay")
            if line and (not bars or bars[-1][1] != line):
                bars.append((ticks, line))
        if gone is not None and ticks > gone + 20:
            break
    print(json.dumps({"tnt": count, "expected_ticks": fuse_ticks(count), "gone_after_ticks": gone,
                      "action_bar": bars[:8]}, ensure_ascii=False))


if __name__ == "__main__":
    sys.exit(main())
