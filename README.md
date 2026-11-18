# Realistic TNT

**Explosions that look, sound and hit like the real thing, for [Horizon](https://github.com/horizon-rust/horizon),
the Rust rewrite of Minecraft Java Edition 26.3.** A native mod: a fireball and a mushroom cloud, a
visible shockwave that hurts, throws and flattens what it sweeps over, a real crater, flying debris, and
a boom you hear when the shockwave reaches you. TNT adds up: the more you pile, the bigger it gets,
with no limit.

*Made with [Claude Code](https://claude.com/claude-code), using Claude Opus 5.5 and Claude Fable 5.2.*

![25,087 TNT going off](media/explosion.gif)

**▶ The full clip, with sound** (15 s, 1080p 60 fps): 25,087 TNT, watched from about 300 blocks
away. Shown with the `wisdom_horizon_tnt` shader pack from this repository.

https://github.com/user-attachments/assets/d2015f20-e6d9-42c7-b8ef-e2eb0959690b

## The blast in the video: 25,087 TNT

A cube of 25,087 TNT, lit with flint and steel. The countdown over the hotbar gave **55 seconds** to fly
away; the shockwave reached **527 blocks**.

![What 25,087 TNT does, seen from above, to scale](media/blast-zones.svg)

| | 25,087 TNT |
|---|---|
| Fuse (time to get away) | 55 s, counted down over the hotbar |
| Fireball and mushroom cloud | 141 blocks, lasting 65 s |
| Crater | 132 blocks out from the centre, charred floor, magma near the middle |
| Brick and stone buildings fall | out to 220 blocks |
| Turf and topsoil torn off | out to 261 blocks |
| Trees and wooden houses fall | out to 333 blocks |
| Glass, leaves and plants go | out to 374 blocks |
| Shockwave hurts and throws mobs and players | out to 527 blocks, its front running at 227 blocks a second |
| Damage at the centre | 468 health points (`minecraft:explosion`), falling off with the square of the distance |
| Knockback | 40 times vanilla's push, capped at Horizon's top speed |

## How it scales

Every explosion has a **yield** in TNT: one TNT is 1, a creeper about 0.4, a bed or a respawn anchor
about 2, an end crystal about 3.4. TNT blocks touching an explosion go off with it at once (up to
200,000 of them), and explosions close together within two seconds are one blast whose yield is their
sum. Everything grows with the **cube root of the yield**, as real blasts do, with no limit.

![How far each effect reaches, by the TNT in the mass](media/blast-scaling.svg)

Distances in blocks from the centre of the blast:

| TNT | Fuse | Crater | Brick and stone fall | Turf torn off | Trees and wood fall | Glass and plants go | Shockwave reach | Front speed |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 4 s | vanilla's | — | — | — | — | 18 | 42 blocks/s |
| 27 | 7.6 s | 14 | — | — | — | — | 54 | 73 blocks/s |
| 512 | 16.6 s | 36 | 44 | 57 | 81 | 94 | 144 | 119 blocks/s |
| 4,096 | 31 s | 72 | 107 | 132 | 174 | 198 | 288 | 168 blocks/s |
| **25,087** (the video) | 55 s | 132 | 220 | 261 | 333 | 374 | 527 | 227 blocks/s |
| 1,000,000 | 3 min | 450 | 876 | 1,000 | 1,215 | 1,338 | 1,800 | 420 blocks/s |

Land is torn up past the crater by blasts of 64 TNT or more; a single TNT keeps vanilla's blast, with
the mod's effects on top.

## What every explosion does

TNT, creepers, beds, end crystals: every explosion gets all of this.

| Part | What happens | Needs the shader pack |
|---|---|---|
| Fireball | A white-hot ball that cools to yellow, orange and red as it rises, lighting the land round it. | yes |
| Mushroom cloud | The fireball rolls up into black smoke on a stem, which greys, spreads and fades. | yes |
| Flash | The whole picture whitens for an instant (an option of the pack; 0 turns it off). | yes |
| Shockwave | A sphere of bent air races out, with colour fringes. | yes |
| Dust skirt | A low ring of dust races out along the ground and settles. | yes, plus dust pillars without it |
| Debris | Plain terrain and wood the blast breaks flies as falling blocks, trailing smoke, and lands as rubble. Ores, containers and block-entity blocks are left to the blast, so loot is not lost. | no |
| Sparks, flames, crumbs | A flash particle, lava sparks, a burst of flame and crumbs of the ground it tore up. | no |
| Smoke and ash | A column of smoke with a cloud at its top, embers at its foot, ash falling afterwards. | no |
| Scorched ground | Grass and soil round the crater char to coarse dirt, plants burn away. | no |
| Camera shake | A jolt as it goes off, then a shake when the shockwave reaches you (scaled by the game's Distortion Effects option). | no |
| Delayed boom | A deep boom, a thump and rolling thunder, heard when the shockwave arrives: later the farther you are. Vanilla's own explosion sound is dropped, so the boom is not heard twice. | no |
| Shockwave damage and push | As the front reaches them, mobs and players are hurt and thrown away from the blast, less the farther out. | no |
| Knockback | The explosion's own push is 35% harder, times the cube root of the yield. | no |
| Crater | A blast of more than one TNT digs its own crater, ring by ring as its front passes: a bowl, everything above it cut away, a charred floor (blackstone, coarse dirt, magma near the middle), the land round it scorched, blocks flung out of it. | no |
| Land torn up | A blast of 64 TNT or more knocks down what its shockwave sweeps over beyond the crater, by how hard it still pushes there: glass, leaves, plants, wool and torches first, then wood (trees, wooden houses, doors, fences), then the turf, then brick, cut stone, cobblestone and concrete. As much of it as the game allows flies up and away where players can see it. Natural rock, ores, metal and blocks holding things (chests, signs, beds) stay. | no |

## Lighting a big mass

**A bigger mass burns longer.** TNT lit in a mass gets vanilla's 4 seconds plus the time it takes to
fly out of the shockwave's reach at 10 blocks a second (`ESCAPE_SPEED`). TNT on its own keeps vanilla's
fuse.

**A countdown** over the hotbar shows the mass, the seconds left and the shockwave's reach:
`25,087 TNT: explodes in 51 s, reach 527 blocks`. It reads the lit TNT's own fuse every tick, so it
ends exactly as the TNT goes off. Then it tells you when the shockwave will reach you
(`25,087 TNT went off! The shockwave reaches you in 2 s`), or that you are out of its reach.

**It goes off on time however far you fly.** While a mass burns, the mod holds the chunks round it
loaded and ticking (the TNT's, the mass's, and out to the land its shockwave tears up, at most 16
chunks out), so it does not freeze when you leave it behind. The chunks are let go once the crater is
dug; a world closed while a mass burns lets go of them when it is opened again.

**The whole mass goes together.** TNT lit later in a mass that is already burning (a second one lit, a
redstone signal reaching several blocks, fire spreading over it) takes the same fuse, and another
blast does not throw the lit TNT out of its mass.

## Install

Requirements:

- Horizon's launcher 0.2.70 or newer, and a game instance it can build native mods into.
- The mod is made with the mod SDK of Horizon nightly 2026-11-18
  ([`0b14f8a`](https://github.com/horizon-rust/horizon/commit/0b14f8acfd97fa2c750e42795249dcc981178ca4),
  `[native]` in `realistic_tnt/horizon-mod.toml`).

**1. Build the mod into your game.** Native mods are native code with full access to your computer,
so the launcher asks you to trust them: read the source first (`realistic_tnt/src/lib.rs`).

```sh
horizon-launcher native-build realistic_tnt --instance <your instance> --trust-native-code
```

`realistic_tnt/` is the mod's project on its own (the folder a `.hmod` is packed from), so the shader
packs, scripts and media live beside it. `cargo horizon-mod build` in it writes
`target/horizon-mod/realistic_tnt.hmod`.

**2. Install a shader pack** for the fireball, the cloud, the flash and the shockwave: copy a pack
from `shader/` into the game's `shaderpacks/` folder and pick it in Options > Video Settings >
Shaders.

| Pack | What it is |
|---|---|
| `realistic_tnt_shockwave-0.4.0.hshader` | The explosion pass alone, over the game's vanilla look (shader API 0.2) |
| `wisdom_horizon_tnt-1.3.7-tnt6.hshader` | Wisdom Shaders V3 (Horizon port) with the explosion pass added: the look in the video. It is a shader API 0.1 pack, which Horizon no longer loads since 2026-11-13: kept for older builds only |

Without a shader pack, everything marked "no" in the table above still works.

## Tuning

The knobs are constants at the top of `realistic_tnt/src/lib.rs`:

| Constant | Default | What it does |
|---|---|---|
| `ESCAPE_SPEED` | 10 blocks/s | How fast you are expected to fly away: the longer a mass's fuse, the lower it is |
| `MAX_FOLD` | 200,000 | The most TNT blocks one explosion takes in at once |
| `HOLD_CHUNKS` | 16 | How far out (in chunks) a burning mass keeps the world loaded |
| `LIFT_YIELD` | 64 TNT | The smallest blast that tears up the land past its crater |
| `BREAKS_AT` | 0.5 / 0.8 / 1.5 / 2.0 | The shockwave push (blocks a tick) that knocks down glass and plants / wood / turf / brick and stone |
| `SCORCH` | on | Whether the land round a crater is scorched (it changes the world for good) |
| `KNOCKBACK` | 1.35 | How much harder the explosion's own push is |

## Limits

- A huge mass freezes the game for a moment as it goes off (the mod walks the whole mass), and a huge
  crater takes a few seconds to dig.
- Horizon keeps at most 1024 falling blocks in a world: what a big shockwave knocks down flies while
  there is room (as flying blocks land, more fly), only within 160 blocks of a player, and the rest is
  simply gone. It looks 40 blocks down from the top of each column at most.
- The fireball, cloud, flash, shockwave picture, camera shake and delayed boom read the blasts from
  the game's own memory: they work in single player and for the host of a LAN game, not for guests or
  on a dedicated server. Debris, particles, craters, damage, knockback and the countdown work
  everywhere.
- The shockwave throws the window's own player; other players in a LAN game are hurt but not thrown.
- A world saved and reopened while a mass burns keeps the TNT's long fuse, but not its countdown nor
  its held chunks.
- The shader pack draws three blasts at a time: the biggest and youngest.
- Particles are seen up to 32 blocks away, as vanilla's are; the fireball and cloud are seen from any
  distance.
- **Photosensitivity:** explosions flash. The pack's "Explosion flash" option at 0 removes the flash.

## Repository layout

```
realistic_tnt/                 the mod's project: what `native-build` builds and `.hmod` packs
  horizon-mod.toml             id, version, the Horizon build it is made for
  src/lib.rs                   the mod: one Rust file, with its unit tests
shader/
  realistic_tnt_shockwave/     the explosion pass on its own (source)
  wisdom_horizon_tnt/          Wisdom Shaders V3 (Horizon port) with that pass added (source)
  *.hshader                    the same packs, packed, ready to drop into shaderpacks/
tools/
  check.ps1                    checks and unit-tests the mod with the pinned build's mod SDK
  diagrams.py                  draws media/blast-*.svg from the mod's own formulas
  shoot.py, bigtest.py, ...    drive a test copy of the game through `horizon-ctl` to photograph
                               explosions and check fuses, far-away blasts and torn-up land
media/                         the clip, its preview and the diagrams
```

The shader pass and the mod share numbers that must stay the same in both: the layout of `frame.mods`,
the shockwave's speed (`SHOCK_SPEED`, `TNT_SHOCK_SPEED`), a blast's life (`LIFE_SECS`, `TNT_LIFE`) and
the curves of the mushroom and of the dust ring. `post/shockwave.wgsl` is the same pass in both packs;
the explosion pack's copy is the shader API 0.2 one, which works in linear light.

## License and credits

- Made with [Claude Code](https://claude.com/claude-code), Anthropic's coding agent, using the
  Claude Opus 5.5 and Claude Fable 5.2 models: the mod's code, the shader pass, the test scripts, the
  diagrams and this README were written with it.
- The mod, the explosion shader pass and the scripts: MIT ([LICENSE](LICENSE)).
- `shader/wisdom_horizon_tnt/` is a modified copy of **Wisdom Shaders V3 (Horizon port)**: Wisdom
  Shaders by bobcao3, ported to Horizon by tomasmendes01, under the Apache License 2.0
  ([its LICENSE](shader/wisdom_horizon_tnt/LICENSE.txt)). The changes are listed in
  [its README](shader/wisdom_horizon_tnt/README.md).
- Horizon is its own project; this mod is not affiliated with it, nor with Mojang or Microsoft.
