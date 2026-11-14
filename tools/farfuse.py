#!/usr/bin/env python3
"""Lights a cube of TNT in the test game (world TntTest, its own socket: see shoot.py), flies the
player far away at once and checks that it still goes off on time: the mod holds its chunks, so its
fuse burns with nobody near. The countdown over the hotbar is logged as it runs (it reads the TNT's
own fuse); the cube must be gone when the player comes back.

    python farfuse.py [SIDE] [AWAY] [SHIFT]

SIDE is the cube's side (6: 216 TNT), AWAY how far along x the player goes (keep it within the
countdown's range, four times the reach and 256 blocks, to see it), SHIFT moves the cube along x to
fresh ground first. Far away the window has no chunks there, so blocks read as air: the cube is only
looked at once the player is back.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bigtest  # noqa: E402
import shoot  # noqa: E402

ctl = shoot.ctl


def fuse_ticks(n):
    """The mod's `fuse_ticks`."""
    reach = 4.8 * n ** (1 / 3) * 3.75
    return round(min(80 + max(reach - 18.0, 0.0) / 10.0 * 20.0, 32767))


def status():
    st = ctl("status")
    m = ctl("messages")
    bar = m.get("actionbar") if isinstance(m, dict) else None
    return st["ticks"], bar


def main():
    cube = int(sys.argv[1]) if len(sys.argv) > 1 else 6
    away = int(sys.argv[2]) if len(sys.argv) > 2 else 600
    shift = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    state = ctl("state")
    if state.get("world") != shoot.WORLD:
        raise SystemExit(f"the game plays {state.get('world')!r}, not {shoot.WORLD!r}: refusing")
    ctl("command", "gamemode creative")
    px, py, pz = (int(round(v - 0.5)) for v in state["player"]["position"])
    sx, sz = px + shift, pz + 40
    if shift:
        ctl("command", f"tp @s {sx} 150 {pz}")
        ctl("wait", 100)
    sy = bigtest.ground(sx, sz)
    half = cube // 2
    x0, z0, y0 = sx - half, sz - half, sy + 1
    for y in range(y0, y0 + cube):  # one layer at a time: /fill has a block limit
        ctl("fill", x0, y, z0, x0 + cube - 1, y, z0 + cube - 1, "tnt")
    ctl("setblock", sx, y0 + cube, sz, "redstone_block")
    lit = ctl("status")["ticks"]
    expected = fuse_ticks(cube ** 3)
    log = []
    ctl("wait", 20)
    t, bar = status()
    log.append(("near", t - lit, bar))
    ctl("command", f"tp @s ~{away} ~ ~")
    while True:
        ctl("wait", 20)
        t, bar = status()
        if not log or log[-1][2] != bar:
            log.append(("far", t - lit, bar))
        if t - lit > expected + 60:
            break
    ctl("command", f"tp @s {px} {py} {pz}")
    ctl("wait", 60)
    corner = shoot.block(x0, y0, z0)
    print(json.dumps({"tnt": cube ** 3, "expected_ticks": expected, "went_off": corner != "tnt",
                      "countdown": log}, ensure_ascii=False))


if __name__ == "__main__":
    sys.exit(main())
