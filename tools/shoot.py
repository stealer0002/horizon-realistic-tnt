#!/usr/bin/env python3
"""Sets off TNT in the running test game and photographs the explosion.

The test game is the one build-testgame.ps1 makes, started with horizon-ctl (see README). This
script only talks to it through horizon-ctl: it stands the player on a small glass perch, puts TNT
on the ground some way off, lights it, and writes a screenshot at each moment asked for.

    python shoot.py NAME [--count N] [--distance D] [--night] [--times 0.05,0.3,...] [--height H]

Screenshots go to testgame-home/shots/NAME-<seconds>.png.
"""
import argparse
import json
import os
import subprocess
import sys

DEV = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CTL = os.path.expandvars(
    r"%APPDATA%\horizon-launcher\versions\nightly-2026-11-08-fdc87ae\horizon-ctl.exe"
)
SHOTS = os.path.join(DEV, "testgame-home", "shots")
# The test game's own control socket, so that no command can reach another game that is open
# (the player's own uses the default one).
SOCKET = os.path.join(DEV, "testgame-home", "control.json")
# The only world this script sets off TNT in.
WORLD = "TntTest"
AIR = ("air", "cave_air", "void_air")


def ctl(*args, timeout=120):
    """Runs one horizon-ctl command and returns its reply (JSON when it is JSON)."""
    done = subprocess.run(
        [CTL, "--socket", SOCKET, "--timeout", str(timeout), *[str(a) for a in args]],
        capture_output=True,
        text=True,
        timeout=timeout + 30,
    )
    out = done.stdout.strip()
    if done.returncode != 0:
        raise SystemExit(f"horizon-ctl {' '.join(map(str, args))}: {done.stderr.strip() or out}")
    try:
        return json.loads(out)
    except ValueError:
        return out


def block(x, y, z):
    reply = ctl("block", x, y, z)
    name = reply.get("block", reply.get("name", "")) if isinstance(reply, dict) else str(reply)
    return name.replace("minecraft:", "").split("[")[0]


def is_ground(name):
    """Not air, and not a tree or a plant standing on the ground."""
    if name in AIR or name in ("snow", "short_grass", "tall_grass", "fern", "large_fern", "glass"):
        return False
    return not name.endswith(("_leaves", "_log", "_sapling", "_tulip"))


def ground(x, z, around, anything=False):
    """The height of the ground in column (x, z), near `around`: the highest block that is not
    air, a tree or a plant (with `anything`, the highest that is not air)."""
    for y in range(around + 24, around - 24, -1):
        name = block(x, y, z)
        if name not in AIR and (anything or is_ground(name)):
            return y
    raise SystemExit(f"no ground in column {x}, {z}")


def take(path, baseline, tries=3):
    """Writes the picture of this moment to `path`: of `tries` screenshots, the one that differs
    most from `baseline`."""
    try:
        from PIL import Image, ImageChops, ImageStat
    except ImportError:
        ctl("screenshot", path)
        return
    base = Image.open(baseline).convert("L")
    best, score = None, -1.0
    for i in range(tries):
        candidate = f"{path}.{i}.png"
        ctl("screenshot", candidate)
        diff = ImageStat.Stat(ImageChops.difference(Image.open(candidate).convert("L"), base)).mean[0]
        if diff > score:
            best, score = candidate, diff
    os.replace(best, path)
    for i in range(tries):
        candidate = f"{path}.{i}.png"
        if os.path.exists(candidate):
            os.remove(candidate)


def sheet(paths, out, columns=4, width=640):
    """Puts the pictures side by side in one JPEG, to look at a run at a glance."""
    from PIL import Image
    images = [Image.open(p).convert("RGB") for p in paths]
    height = round(width * images[0].height / images[0].width)
    rows = (len(images) + columns - 1) // columns
    board = Image.new("RGB", (width * columns, height * rows))
    for i, im in enumerate(images):
        board.paste(im.resize((width, height)), ((i % columns) * width, (i // columns) * height))
    board.save(out, quality=85)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("name")
    parser.add_argument("--count", type=int, default=1, help="TNT blocks, set off together")
    parser.add_argument("--distance", type=int, default=22, help="blocks from the player")
    parser.add_argument("--height", type=int, default=5, help="the perch's height over the ground")
    parser.add_argument("--night", action="store_true")
    parser.add_argument("--times", default="0.05,0.25,0.6,1.2,2.5,5,8")
    parser.add_argument("--site", help="x,z of the blast instead of ahead of the player")
    parser.add_argument("--hud", action="store_true", help="leave the HUD in the pictures")
    parser.add_argument("--socket", help="another game's control socket (default: the test game's)")
    parser.add_argument("--world", help="the world that game must be in (needed with --socket)")
    parser.add_argument("--keep-time", action="store_true", help="leave the time and weather alone")
    parser.add_argument("--state", action="store_true", help="print the player's eye and view at each picture")
    args = parser.parse_args()
    times = sorted(float(t) for t in args.times.split(","))
    os.makedirs(SHOTS, exist_ok=True)

    global SOCKET, WORLD
    if args.socket:
        if not args.world:
            raise SystemExit("--socket needs --world: the world it may set off TNT in")
        SOCKET, WORLD = args.socket, args.world
    state = ctl("state")
    if state.get("world") != WORLD:
        raise SystemExit(f"the game on {SOCKET} plays {state.get('world')!r}, not {WORLD!r}: refusing")
    player = state["player"]
    position = player.get("position", player.get("pos"))
    px, py, pz = (int(round(v - 0.5)) for v in position)
    if not args.keep_time:
        ctl("time", 18000 if args.night else 6000)
        ctl("command", "weather clear")

    # Where it goes off: south of the player (yaw 0 looks south), on the ground there.
    if args.site:
        sx, sz = (int(v) for v in args.site.split(","))
    else:
        sx, sz = px, pz + args.distance
    sy = ground(sx, sz, py)

    # A perch for the player, so that hills and trees are less in the way.
    feet = ground(px, pz, py) + args.height
    ctl("setblock", px, feet, pz, "glass")
    ctl("tp", px + 0.5, feet + 1, pz + 0.5)
    ctl("wait", 5)
    ctl("look-at", sx + 0.5, sy + 4, sz + 0.5)

    # A square of TNT on the ground, lit by a block of redstone on one of them.
    side = max(1, round(args.count ** 0.5))
    placed = 0
    for i in range(args.count):
        x, z = sx + i % side, sz + i // side
        y = ground(x, z, sy) + 1
        ctl("setblock", x, y, z, "tnt")
        placed += 1
    top = ground(sx, sz, sy, anything=True) + 1
    ctl("setblock", sx, top, sz, "redstone_block")
    if not args.hud:
        ctl("key", "f1", "tap")

    # TNT burns for 80 ticks. Wait in game ticks, so the pictures are at the same moments of the
    # explosion whatever the frame rate.
    fuse = 80
    ctl("wait", fuse - 4)
    # What the view looks like before it goes off: the pictures that differ most from it are the
    # ones that show the explosion (the offscreen game's screenshot now and then grabs a frame
    # without the shader pack's last passes).
    baseline = os.path.join(SHOTS, "_baseline.png")
    ctl("screenshot", baseline.replace("\\", "/"))
    ctl("wait", 3)
    last = 0.0
    shots = []
    for t in times:
        ticks = round((t - last) * 20)
        if ticks > 0:
            ctl("wait", ticks)
            last += ticks / 20
        path = os.path.join(SHOTS, f"{args.name}-{t:05.2f}.png").replace("\\", "/")
        take(path, baseline)
        shots.append(path)
        if args.state:
            now = ctl("state")["player"]
            print(f"{t:5.2f}s  eye {[round(v, 2) for v in now['eye']]}  yaw {now['yaw']:.2f}  pitch {now['pitch']:.2f}")
    if not args.hud:
        ctl("key", "f1", "tap")
    try:
        sheet(shots, os.path.join(SHOTS, f"{args.name}-sheet.jpg"))
    except ImportError:
        pass
    print(json.dumps({"site": [sx, sy, sz], "player": [px, feet + 1, pz], "tnt": placed, "shots": shots}))


if __name__ == "__main__":
    sys.exit(main())
