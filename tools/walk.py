#!/usr/bin/env python3
"""Sets off TNT in the test game, then walks toward it in survival (view bobbing on) and takes
pictures one after another while walking: the smoke must stay put on the land, not swing.

    python walk.py NAME [--shots N] [--distance D]

Pictures go to testgame-home/shots/NAME-wNN.png and NAME-sheet.jpg. Only the test game (its own
socket, world TntTest) is ever touched; see shoot.py.
"""
import argparse
import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import shoot  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("name")
    parser.add_argument("--shots", type=int, default=12)
    parser.add_argument("--distance", type=int, default=24)
    args = parser.parse_args()
    ctl = shoot.ctl

    state = ctl("state")
    if state.get("world") != shoot.WORLD:
        raise SystemExit(f"the game plays {state.get('world')!r}, not {shoot.WORLD!r}: refusing")
    ctl("command", "gamemode creative")
    ctl("options-set", "bobView", "true")
    ctl("time", 6000)
    ctl("command", "weather clear")
    px, py, pz = (int(round(v - 0.5)) for v in state["player"]["position"])
    # Stand on the ground, the blast ahead (south).
    feet = shoot.ground(px, pz, py) + 1
    ctl("tp", px + 0.5, feet, pz + 0.5)
    ctl("wait", 5)
    sx, sz = px, pz + args.distance
    sy = shoot.ground(sx, sz, py)
    ctl("look-at", sx + 0.5, sy + 4, sz + 0.5)
    ctl("setblock", sx, sy + 1, sz, "tnt")
    ctl("setblock", sx, sy + 2, sz, "redstone_block")
    ctl("command", "gamemode survival")
    ctl("key", "f1", "tap")
    # The fuse, then a second for the cloud to rise.
    ctl("wait", 80 + 20)
    walker = subprocess.Popen(
        [shoot.CTL, "--socket", shoot.SOCKET, "--timeout", "60", "move", "forward", "4"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    shots = []
    for i in range(args.shots):
        path = os.path.join(shoot.SHOTS, f"{args.name}-w{i:02}.png").replace("\\", "/")
        ctl("screenshot", path)
        shots.append(path)
    walker.wait(timeout=60)
    ctl("key", "f1", "tap")
    ctl("command", "gamemode creative")
    shoot.sheet(shots, os.path.join(shoot.SHOTS, f"{args.name}-sheet.jpg"))
    print(json.dumps({"site": [sx, sy, sz], "shots": len(shots)}))


if __name__ == "__main__":
    sys.exit(main())
