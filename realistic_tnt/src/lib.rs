//! Realistic TNT, a native mod for Horizon (docs/modding, "Your first native mod").
//!
//! Every explosion now:
//!  * throws part of the plain terrain it would have broken as flying debris: the blocks leave the
//!    blast list, become falling blocks launched away from the centre, trail smoke as they fly and
//!    land as rubble;
//!  * bursts in a flash, sparks, flames and crumbs of the ground, drives a ring of dust out along
//!    the ground, raises a column of smoke with a cloud at its top, and lets ash fall afterwards;
//!  * scorches the ground round the crater (grass to coarse dirt, plants burnt away);
//!  * shakes the camera and is heard when its shockwave arrives, later the farther away it is;
//!  * sends a shockwave out that hurts and throws mobs and players as it reaches them;
//!  * tells the shader pack where it happened (`frame.mods`), which draws the fireball, the
//!    mushroom cloud, the dust and the shockwave (shader/realistic_tnt_shockwave).
//!
//! Explosions add up. Each has a yield in TNT (one TNT is 1, a creeper about 0.4, an end crystal
//! 3.4: the cube of its power over TNT's). TNT blocks touching an explosion join it at once
//! instead of going off one by one after it, and explosions close together in space and time are
//! one blast whose yield is their sum. Everything grows with the cube root of the yield, as real
//! blasts do, with no limit: the crater, the cloud, the shockwave's reach, damage and push, how
//! long it lasts and how fast its front runs. A blast of more than one TNT digs its own crater, ring
//! by ring as the front passes (straight into the chunks, with no drops: what is in it is gone).
//!
//! The shader, the shake, the delayed sound and the push on the window's own player read the blasts
//! from process memory, so they work in single player and for the host of a LAN game, not for
//! guests or on a dedicated server. The countdown over the hotbar is sent by the server, so every
//! player sees it. Particles reach 32 blocks, as vanilla's do.
mod command;
mod config;

use mod_api::{
    BlockEdit, BlockRun, BlockState, BlockUpdateFlags, CameraShakeEvent, ClientHost,
    ClientTickPostEvent, Compound, EntityRef, ExplosionDetonateEvent, ExplosionKnockbackEvent,
    ExplosionStartEvent, Falling, Flow, Heightmap, Hooks, Host, HostSide, Identifier, LevelRef,
    LevelTickPostEvent, LevelUnloadEvent, LoopContext, ParticleKey, ParticleOptions, PlayerRef,
    PlaySoundEvent,
    RegistrationContext, Registrar, ServerHost, ServerStoppingEvent, ShaderParamsEvent,
    SoundAttenuation, SoundPlay, SoundSource, StatePredicate, Tag, Text, HostBlockStates,
    TicketController, TicketOwner, TntPrimedEvent, ValidateTicketsEvent,
};
use mod_api::text::TextStyle;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// The particles this mod sends, each requested once when the mod registers (`register`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Particle {
    Ash,
    Block,
    CampfireSignalSmoke,
    DustPillar,
    Flame,
    Flash,
    LargeSmoke,
    Lava,
    Poof,
    Smoke,
    WhiteAsh,
}

impl Particle {
    const ALL: [Particle; 11] = [
        Particle::Ash,
        Particle::Block,
        Particle::CampfireSignalSmoke,
        Particle::DustPillar,
        Particle::Flame,
        Particle::Flash,
        Particle::LargeSmoke,
        Particle::Lava,
        Particle::Poof,
        Particle::Smoke,
        Particle::WhiteAsh,
    ];

    fn id(self) -> &'static str {
        match self {
            Particle::Ash => "minecraft:ash",
            Particle::Block => "minecraft:block",
            Particle::CampfireSignalSmoke => "minecraft:campfire_signal_smoke",
            Particle::DustPillar => "minecraft:dust_pillar",
            Particle::Flame => "minecraft:flame",
            Particle::Flash => "minecraft:flash",
            Particle::LargeSmoke => "minecraft:large_smoke",
            Particle::Lava => "minecraft:lava",
            Particle::Poof => "minecraft:poof",
            Particle::Smoke => "minecraft:smoke",
            Particle::WhiteAsh => "minecraft:white_ash",
        }
    }

    /// The key the game gave it, once the mod has registered.
    fn key(self) -> Option<ParticleKey> {
        let index = Particle::ALL.iter().position(|&p| p == self)?;
        PARTICLES.get().map(|keys| keys[index])
    }
}

static PARTICLES: OnceLock<[ParticleKey; 11]> = OnceLock::new();
/// What holds the chunks round a burning mass loaded (`follow_fuses`).
static FUSE_TICKETS: OnceLock<TicketController> = OnceLock::new();

/// The most blocks one blast throws. Horizon refuses falling blocks past 1024 in a level.
const MAX_DEBRIS: usize = 96;
/// Of the plain blocks a blast breaks, this share (out of 100) flies instead of dropping.
const DEBRIS_PERCENT: u64 = 55;
/// How long a blast of one TNT lasts, in seconds, and how fast its shockwave runs, in blocks a
/// second; a bigger one lasts longer and runs faster (`time_scale`). The shader's `TNT_LIFE` and
/// `TNT_SHOCK_SPEED` are the same: the camera shakes, the boom is heard and the push lands as the
/// ring the pack draws arrives.
const LIFE_SECS: f32 = 12.0;
const SHOCK_SPEED: f32 = 42.0;
/// How big the pack draws one TNT, in blocks; a blast of yield Y is this times the cube root of Y.
const BASE_SIZE: f32 = 4.8;
/// The crater of a blast of yield Y reaches this times the cube root of Y, in blocks.
const CRATER: f64 = 4.5;
/// A blast bigger than this (in TNT) digs its own crater; a single explosion breaks blocks as
/// vanilla's does.
const BIG_YIELD: f32 = 1.5;
/// The most TNT blocks one explosion takes in at once (a bigger mass still goes off: the rest by
/// the blast that follows).
const MAX_FOLD: usize = 200_000;
/// The shockwave's damage near a one-TNT blast, in health points, and its push, in blocks a tick.
const SHOCK_DAMAGE: f32 = 16.0;
const SHOCK_PUSH: f64 = 1.3;
/// The most columns of crater dug in one tick: past it the digging falls behind the front.
const CARVE_PER_TICK: f64 = 6000.0;
/// The most flying blocks a big blast throws in one tick.
const EJECTA_PER_TICK: usize = 12;
/// A blast of at least this many TNT tears up the land its shockwave sweeps over beyond the crater
/// (`lift`), out to where it still pushes `LIFT_PUSH` blocks a tick: what it knocks down there
/// depends on how hard it pushes (`BREAKS_AT`).
const LIFT_YIELD: f32 = 64.0;
const LIFT_PUSH: f64 = 0.5;
/// The most columns it sweeps in one tick, and how deep it looks down each from its top.
const LIFT_PER_TICK: f64 = 8000.0;
const LIFT_DEPTH: u32 = 40;
/// What it tears off flies where a player is at most this far away (a falling block is not seen
/// farther), at most this many blocks a tick, as many as the level takes; crumbs burst round the
/// players, at most this many puffs a tick.
const SEEN_FLYING: f64 = 160.0;
const LIFT_THROWS: usize = 256;
const LIFT_PUFFS: usize = 40;
/// The shader pack's blast slots, two `vec4`s each.
const WAVE_SLOTS: usize = 3;
/// The most blasts remembered at once; the oldest goes first.
const MAX_BLASTS: usize = 24;
/// The most flying blocks of one explosion whose path smokes, and for how many ticks.
const MAX_TRAILS: usize = 28;
const TRAIL_TICKS: u32 = 34;
/// Whether the ground round a crater is scorched, and how many columns of it a tick.
const SCORCH: bool = true;
const SCORCH_PER_TICK: usize = 24;
/// What an explosion's own push on mobs and players is multiplied by (times the cube root of the
/// blast's yield).
const KNOCKBACK: f64 = 1.35;
/// The pitch of this mod's own boom (`minecraft:entity.generic.explode`): below anything vanilla
/// plays it at (`(1 ± 0.2) * 0.7`, so 0.56 to 0.84), which is how `sound_play` tells the two apart.
const BOOM_PITCH: f32 = 0.5;
/// A new explosion joins a blast this young (in ticks) when it is close enough: long enough for
/// a chain of TNT set off by the first to join it.
const MERGE_TICKS: u32 = 40;
/// Vanilla's fuse, in ticks: what TNT lit on its own burns.
const FUSE_TICKS: i32 = 80;
/// TNT lit in a mass burns longer, the bigger the mass: vanilla's fuse, plus the time it takes to
/// fly out of the shockwave's reach at this many blocks a second (creative flight is about 11).
const ESCAPE_SPEED: f32 = 10.0;
/// The most burning masses followed at once (TNT lit in a mass already burning is not followed on
/// its own: the mass gives it its fuse).
const MAX_FUSES: usize = 64;
/// How many ticks the mod looks for the TNT a lit block became before it gives up on it.
const FIND_TICKS: u32 = 10;
/// While TNT lit in a mass burns, the chunks round it (the TNT, the mass, the crater and the
/// scorched land round it) are held loaded and ticking, so that it burns and goes off on time
/// however far away the players fly: at most this many chunks out from it.
const HOLD_CHUNKS: i32 = 16;
/// After it has gone off they are held while its crater is dug, at least this many ticks and at
/// most `HOLD_MAX_TICKS`.
const HOLD_AFTER_TICKS: u32 = 40;
const HOLD_MAX_TICKS: u32 = 1200;
/// The countdown is shown this far from a burning mass: four times its shockwave's reach, and this.
const COUNTDOWN_MARGIN: f64 = 256.0;

/// An explosion that is still burning, smoking or shaking the picture.
struct Blast {
    id: u64,
    dimension: String,
    /// Where it went off.
    center: [f64; 3],
    /// The height of the ground under it, when it went off on the ground or just over it.
    ground: Option<f64>,
    /// For its age in real seconds: the picture is drawn once a frame.
    born: Instant,
    /// Server ticks so far.
    ticks: u32,
    /// About how far it reached, in blocks (the farthest block it broke).
    power: f32,
    /// Its yield in TNT: the sum of the explosions in it.
    yield_tnt: f32,
    /// How big the pack draws it, in blocks (`visual_size` of the yield).
    size: f32,
    /// Whether it digs its own crater (`BIG_YIELD`), and how far out it has dug and scorched.
    big: bool,
    carved: f64,
    scorched: f64,
    /// How far out its shockwave has torn up the land beyond the crater (`lift`).
    lifted: f64,
    /// The entities its shockwave has reached.
    hit: HashSet<u32>,
    /// Whether the window's own player has been pushed by it.
    pushed: bool,
    /// A number of its own in 0..1, so that no two clouds look alike.
    seed: f32,
    /// The flying blocks whose path still smokes.
    trails: Vec<Trail>,
    /// The columns of ground still to scorch, the nearest last.
    scorch: Vec<[i32; 2]>,
    /// Whether the window's game has queued its sound.
    heard: bool,
}

impl Blast {
    /// Where the pack draws it from: the ground under it, or under the fireball of one in the air.
    fn origin(&self) -> [f64; 3] {
        let y = self
            .ground
            .unwrap_or(self.center[1] - f64::from(self.size) * 0.45);
        [self.center[0], y, self.center[2]]
    }
}

/// A flying block as the mod follows it, to leave smoke along its path.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Trail {
    pos: [f64; 3],
    vel: [f64; 3],
    age: u32,
}

impl Trail {
    /// One tick of a falling block's flight: gravity, the move, then drag.
    fn advance(&mut self) {
        self.vel[1] -= 0.04;
        for axis in 0..3 {
            self.pos[axis] += self.vel[axis];
            self.vel[axis] *= 0.98;
        }
        self.age += 1;
    }
}

/// Shared by the server hooks (which see an explosion) and the window's hooks (which draw, shake and
/// play it). One process holds both in single player and for a LAN host, which is why they need no
/// message between them.
static BLASTS: Mutex<Vec<Blast>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn blasts() -> std::sync::MutexGuard<'static, Vec<Blast>> {
    BLASTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// TNT lit in a mass, whose fuse the mod stretches (`fuse_ticks`) and counts down on screen.
struct Fuse {
    id: u64,
    dimension: String,
    /// The block that was lit.
    pos: [i32; 3],
    /// How many TNT it will set off, itself included, the box they fill and the blocks they were
    /// in when it was lit (a TNT lit next to them later goes off with it).
    mass: usize,
    lo: [i32; 3],
    hi: [i32; 3],
    cells: HashSet<[i32; 3]>,
    /// Its whole fuse, and the server ticks since it was lit.
    total: i32,
    age: u32,
    /// Ticks left before it goes off: the lit TNT's own fuse, read each tick once it is found
    /// (it stands still where its chunk does not tick), and `total - age` before.
    left: i32,
    /// The lit TNT, once found.
    entity: Option<u32>,
    /// Server ticks since its TNT went off (or was lost).
    gone: Option<u32>,
    /// For how many more ticks the TNT lit in its mass since (a signal reaching several blocks,
    /// fire spreading, a second one lit) is given its fuse, so that the mass goes off together.
    sweep: u32,
    /// The chunks it holds loaded and ticking (`hold_area`, and wherever its TNT is thrown), under
    /// `owner`, once `held`.
    owner: TicketOwner,
    chunks: Vec<[i32; 2]>,
    held: bool,
}

/// A fuse just lit at `pos` in the mass `cluster`.
fn new_fuse(dimension: &str, pos: [i32; 3], cluster: &[[i32; 3]], total: i32) -> Fuse {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mass = mass_of(cluster, pos);
    let (lo, hi) = bounds_of(cluster, pos);
    Fuse {
        id,
        dimension: dimension.to_owned(),
        pos,
        mass,
        lo,
        hi,
        cells: cluster.iter().copied().collect(),
        total,
        age: 0,
        left: total,
        entity: None,
        gone: None,
        sweep: FIND_TICKS,
        owner: fuse_owner(id),
        chunks: hold_area(pos, cluster, mass),
        held: false,
    }
}

/// Follows `fuse` too. Past `MAX_FUSES` one goes: one that has gone off first, then one holding
/// no chunks yet, then the oldest.
fn add_fuse(all: &mut Vec<Fuse>, fuse: Fuse) {
    if all.len() >= MAX_FUSES {
        let drop = all
            .iter()
            .position(|f| f.gone.is_some())
            .or_else(|| all.iter().position(|f| !f.held))
            .unwrap_or(0);
        forget(all.remove(drop));
    }
    all.push(fuse);
}

/// The burning mass TNT lit at `cell` belongs to, if any: one whose blocks it lies within two
/// blocks of (as `tnt_cluster` walks), the one going off soonest if several.
fn burning_mass_of<'a>(
    all: &'a mut [Fuse],
    dimension: &str,
    cell: [i32; 3],
) -> Option<&'a mut Fuse> {
    all.iter_mut()
        .filter(|f| f.dimension == dimension && f.gone.is_none() && in_box(f.lo, f.hi, cell, 2))
        .filter(|f| f.pos == cell || same_mass(&f.cells, f.pos, cell))
        .min_by_key(|f| f.left)
}

/// The box (`lo`, `hi`, inclusive) round the TNT of `cluster` and `cell`.
fn bounds_of(cluster: &[[i32; 3]], cell: [i32; 3]) -> ([i32; 3], [i32; 3]) {
    let (mut lo, mut hi) = (cell, cell);
    for pos in cluster {
        for axis in 0..3 {
            lo[axis] = lo[axis].min(pos[axis]);
            hi[axis] = hi[axis].max(pos[axis]);
        }
    }
    (lo, hi)
}

/// Whether `pos` lies within `margin` blocks of the box `lo`..`hi`.
fn in_box(lo: [i32; 3], hi: [i32; 3], pos: [i32; 3], margin: i32) -> bool {
    (0..3).all(|axis| pos[axis] >= lo[axis] - margin && pos[axis] <= hi[axis] + margin)
}

/// Chunks a fuse held, to be let go of on the level's next tick (a fuse dropped from a hook
/// where the mod does not touch chunks).
struct Release {
    dimension: String,
    owner: TicketOwner,
    chunks: Vec<[i32; 2]>,
}

/// The ticket owner of fuse `id`: a number of its own, so that two fuses never let go of each
/// other's chunks. It names no real entity.
fn fuse_owner(id: u64) -> TicketOwner {
    TicketOwner::Entity((0x7265_616c_6973_7469_635f_746e_7400_0000_u128) | u128::from(id))
}

static FUSES: Mutex<Vec<Fuse>> = Mutex::new(Vec::new());
static RELEASES: Mutex<Vec<Release>> = Mutex::new(Vec::new());

/// The countdown a player was last shown over their hotbar: in whole seconds, the fuse it
/// counted, and the level tick it was sent on (the bar fades three seconds after).
#[derive(Clone, Copy, Debug)]
struct Counted {
    secs: i32,
    fuse: u64,
    at: i64,
}

/// The countdown each player was last shown, by player UUID.
static COUNTED: Mutex<Option<HashMap<u128, Counted>>> = Mutex::new(None);
/// The countdown is sent again at least this often, in ticks of the level's game time, so that it
/// does not fade while a slow server takes longer than that over one of its seconds.
const COUNTDOWN_RESEND: i64 = 40;
/// The ticks a second the server keeps now (`f32` bits, `kept_rate`), for the countdown's seconds.
static SERVER_TPS: AtomicU32 = AtomicU32::new(0x41A0_0000); // 20.0

fn fuses() -> std::sync::MutexGuard<'static, Vec<Fuse>> {
    FUSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn releases() -> std::sync::MutexGuard<'static, Vec<Release>> {
    RELEASES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where the window's own player is (its dimension and position), for the server to throw the
/// lifted land where it is seen: in single player the server lists no players.
static WINDOW_AT: Mutex<Option<(String, [f64; 3])>> = Mutex::new(None);

fn window_at() -> std::sync::MutexGuard<'static, Option<(String, [f64; 3])>> {
    WINDOW_AT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The last explosion the server announced (`explosion_start`): its level, centre and radius, which
/// `explosion_detonate` reads right after for its yield.
static LAST_START: Mutex<Option<(String, [f64; 3], f32)>> = Mutex::new(None);

fn last_start() -> std::sync::MutexGuard<'static, Option<(String, [f64; 3], f32)>> {
    LAST_START.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn register(&self, registrar: &mut dyn Registrar, context: &RegistrationContext) {
        config::load(context);
        // The particles it sends, and the controller that holds the chunks of a burning mass.
        let _ = PARTICLES.set(Particle::ALL.map(|p| registrar.particle_key(p.id())));
        let _ = FUSE_TICKETS.set(registrar.register_ticket_controller("realistic_tnt:fuse", true));
    }

    fn register_commands(&self, _host: &mut dyn ServerHost, event: &mut mod_api::RegisterCommandsEvent<'_>) {
        command::register(event);
    }

    fn run_mod_command(&self, host: &mut dyn ServerHost, event: &mut mod_api::RunModCommandEvent<'_>) -> Flow {
        command::run(host, event)
    }

    fn validate_tickets(&self, _host: &mut dyn ServerHost, event: &mut ValidateTicketsEvent<'_>) {
        // A fuse lives in memory only: the chunks a world saved for one (the game closed while a
        // mass burned) are let go of when it loads again.
        if FUSE_TICKETS.get() != Some(&event.controller) {
            return;
        }
        for (pos, _) in event.validation.block_tickets() {
            event.validation.remove_block_tickets(pos);
        }
        for (uuid, _) in event.validation.entity_tickets() {
            event.validation.remove_entity_tickets(uuid);
        }
    }

    fn explosion_start(&self, _host: &mut dyn ServerHost, event: &mut ExplosionStartEvent<'_>) -> Flow {
        let start = event.explosion;
        *last_start() = Some((start.dimension.to_owned(), start.center, start.radius));
        Flow::Continue
    }

    fn explosion_detonate(&self, host: &mut dyn ServerHost, event: &mut ExplosionDetonateEvent<'_>) {
        let center = event.center;
        let dimension = event.dimension.to_owned();
        let Some(level) = host.level(&dimension) else {
            return;
        };
        let own = last_start()
            .take()
            .filter(|(dim, at, _)| *dim == dimension && *at == center)
            .map_or(1.0, |(_, _, radius)| yield_of(radius));
        let cell = cell_of(center);
        // TNT blocks touching it go off with it, now, instead of one by one after it.
        let folded = {
            let look = |pos| block_at(&*host, level, pos);
            tnt_cluster(&look, cell, config::get().max_fold)
        };
        if !folded.is_empty() {
            remove_blocks(host, &dimension, &folded);
        }
        let added = own + folded.len() as f32;
        // A blast gone off beside it just now takes it in: their yields add up, and that blast
        // does the breaking.
        {
            let mut all = blasts();
            if let Some(blast) = all.iter_mut().find(|b| joins(b, &dimension, center)) {
                blast.yield_tnt += added;
                blast.size = visual_size(blast.yield_tnt);
                blast.big = blast.big || blast.yield_tnt > BIG_YIELD;
                event.blocks.clear();
                return;
            }
        }
        let big = added > BIG_YIELD;
        if big {
            // It digs its own crater as its front passes.
            event.blocks.clear();
        }
        let power = reach(center, event.blocks).max((crater_radius(added) * 0.6) as f32);
        let size = visual_size(added);
        let seed = unit(mix(cell, 77), 1) as f32;
        let ground = {
            let look = |pos| block_at(&*host, level, pos);
            ground_under(&look, center)
        };

        // Part of what it breaks flies instead.
        let mut trails = Vec::new();
        let mut crumbs: Vec<BlockState> = Vec::new();
        if let Some(air) = state_of(&*host, "minecraft:air") {
            let mut kept = Vec::with_capacity(event.blocks.len());
            let mut thrown = 0;
            for &pos in event.blocks.iter() {
                let roll = mix(pos, 0);
                if thrown >= MAX_DEBRIS || roll % 100 >= DEBRIS_PERCENT {
                    kept.push(pos);
                    continue;
                }
                let name = block_at(&*host, level, pos);
                let Some(state) = is_debris(&name).then(|| state_of(&*host, &name)).flatten() else {
                    kept.push(pos);
                    continue;
                };
                // Take the block out of the world, then throw it from where it stood. If Horizon
                // refuses the throw, put it back and let the blast break it as usual.
                if !set_block(host, level, pos, air) {
                    kept.push(pos);
                    continue;
                }
                let from = [pos[0] as f64 + 0.5, pos[1] as f64, pos[2] as f64 + 0.5];
                let velocity = launch_velocity(center, pos, f64::from(power), roll);
                let falling = falling(true, false);
                if host.spawn_falling_block(&dimension, state, from, velocity, falling) {
                    thrown += 1;
                    if trails.len() < MAX_TRAILS {
                        trails.push(Trail {
                            pos: [from[0], from[1] + 0.5, from[2]],
                            vel: velocity,
                            age: 0,
                        });
                    }
                    if crumbs.len() < 3 && !crumbs.contains(&state) {
                        crumbs.push(state);
                    }
                } else {
                    set_block(host, level, pos, state);
                    kept.push(pos);
                }
            }
            *event.blocks = kept;
        }

        // The burst itself: a flash, sparks, flames, smoke and crumbs of what it tore up.
        for puff in burst(center, power, size) {
            emit(host, level, puff);
        }
        for state in crumbs {
            let spread = [f64::from(power) * 0.4; 3];
            let mut puff = Puff::new(Particle::Block, center, spread, 0.45, 24);
            puff.state = Some(state);
            emit(host, level, puff);
        }

        // Remember it. A big one scorches the land round its crater as it digs (`carve`).
        let scorch = if config::get().scorch && ground.is_some() && !big {
            scorch_columns(center, power)
        } else {
            Vec::new()
        };
        let mut all = blasts();
        if all.len() >= MAX_BLASTS {
            all.remove(0);
        }
        all.push(Blast {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            dimension,
            center,
            ground,
            born: Instant::now(),
            ticks: 0,
            power,
            yield_tnt: added,
            size,
            big,
            carved: 0.0,
            scorched: 0.0,
            lifted: 0.0,
            hit: HashSet::new(),
            pushed: false,
            seed,
            trails,
            scorch,
            heard: false,
        });
    }

    fn tnt_primed(&self, host: &mut dyn ServerHost, event: &mut TntPrimedEvent) -> Flow {
        // TNT lit in a mass burns longer, the bigger the mass: time to get out of its reach. The
        // lit TNT is found and its fuse stretched on the level's next tick (`follow_fuses`), and
        // the chunks round it are held so that it burns however far away the players go.
        let cell = event.pos;
        // The level of the player who lit it, else the one in place (fire, a signal, an arrow).
        let dimension = event
            .player
            .and_then(|uuid| host.player(uuid))
            .and_then(|player| player.level(&*host))
            .map_or_else(|| host.dimension().to_owned(), |level| level_name(&*host, level));
        let Some(level) = host.level(&dimension) else {
            return Flow::Continue;
        };
        // TNT lit in a mass that is already burning (a signal reaching several blocks of it, fire
        // spreading over it, a second one lit) goes off with it, when it does: the mass's fuse
        // gives it its own fuse on the next ticks (`follow_fuses`), without walking the mass again.
        if let Some(burning) = burning_mass_of(&mut fuses(), &dimension, cell) {
            burning.sweep = FIND_TICKS;
            return Flow::Continue;
        }
        let cluster = {
            let look = |pos| block_at(&*host, level, pos);
            tnt_cluster(&look, cell, config::get().max_fold)
        };
        let mass = mass_of(&cluster, cell);
        let total = fuse_ticks(mass as f32);
        if total <= FUSE_TICKS {
            return Flow::Continue;
        }
        let mut all = fuses();
        // A burning mass this one's reaches (TNT laid next to it since it was lit) takes it in.
        let burning = all.iter().any(|f| f.dimension == dimension && f.gone.is_none());
        let cells: HashSet<[i32; 3]> = if burning {
            cluster.iter().copied().collect()
        } else {
            HashSet::new()
        };
        let reached = all
            .iter_mut()
            .filter(|f| f.dimension == dimension && f.gone.is_none())
            .filter(|f| same_mass(&cells, cell, f.pos))
            .min_by_key(|f| f.left);
        if let Some(burning) = reached {
            burning.sweep = FIND_TICKS;
            burning.cells.extend(cluster.iter().copied());
            let (lo, hi) = bounds_of(&cluster, cell);
            for axis in 0..3 {
                burning.lo[axis] = burning.lo[axis].min(lo[axis]);
                burning.hi[axis] = burning.hi[axis].max(hi[axis]);
            }
            return Flow::Continue;
        }
        let fuse = new_fuse(&dimension, cell, &cluster, total);
        add_fuse(&mut all, fuse);
        Flow::Continue
    }

    fn explosion_knockback(&self, _host: &mut dyn ServerHost, event: &mut ExplosionKnockbackEvent<'_>) {
        // The lit TNT of a mass still burning is not thrown out of it: it goes off with its mass.
        if event.entity.is_some_and(|id| followed_tnt().contains(&id)) {
            *event.knockback_velocity = [0.0; 3];
            return;
        }
        // The blast this explosion is part of pushes as hard as its yield.
        let yield_tnt = blasts()
            .iter()
            .filter(|b| b.dimension == event.dimension && b.ticks <= MERGE_TICKS)
            .find(|b| distance(b.center, event.center) <= crater_radius(b.yield_tnt) + 6.0)
            .map_or(1.0, |b| b.yield_tnt);
        let factor = config::get().knockback * f64::from(yield_tnt.max(1.0)).cbrt();
        for axis in event.knockback_velocity.iter_mut() {
            *axis = (*axis * factor).clamp(-3.9, 3.9);
        }
    }

    fn level_unload(&self, _host: &mut dyn ServerHost, event: &mut LevelUnloadEvent<'_>) {
        // Its chunks go with it, and the fuses burning in it are no longer followed.
        fuses().retain(|f| f.dimension != event.dimension);
        releases().retain(|r| r.dimension != event.dimension);
    }

    fn server_stopping(&self, _host: &mut dyn ServerHost, _event: &mut ServerStoppingEvent) {
        // The server lets go of every chunk held as it stops; the next world starts afresh.
        fuses().clear();
        releases().clear();
        *counted() = None;
    }

    fn level_tick_post(&self, host: HostSide, event: &mut LevelTickPostEvent) {
        let HostSide::Server(host) = host else {
            return;
        };
        let level = event.level;
        let dimension = level_name(&*host, level);
        let (rate, mspt) = host.tick_stats();
        let tps = kept_rate(rate, mspt);
        if tps.is_finite() && tps > 0.0 {
            SERVER_TPS.store(tps.to_bits(), Ordering::Relaxed);
        }
        follow_fuses(host, level, &dimension);
        countdown(host, level, &dimension);
        // Take this tick's work out while holding the lock; do it after, with the lock released
        // (the window's frame reads the same list).
        let mut work = Vec::new();
        {
            let mut all = blasts();
            for blast in all.iter_mut().filter(|b| b.dimension == dimension) {
                blast.ticks += 1;
                let take = if blast.ticks >= 3 {
                    blast.scorch.len().min(SCORCH_PER_TICK)
                } else {
                    0
                };
                let columns = blast.scorch.split_off(blast.scorch.len() - take);
                // How far the front has run, and the ring of crater (and scorched land round it)
                // to dig this tick.
                let speed = f64::from(shock_speed(blast.size));
                let front = speed * f64::from(blast.ticks) / 20.0;
                let radius = crater_radius(blast.yield_tnt);
                let carve = (blast.big && blast.carved < radius * 1.1).then(|| {
                    let to = capped_ring(blast.carved, front.min(radius * 1.1), CARVE_PER_TICK);
                    let ring = (blast.carved, to);
                    blast.carved = to;
                    ring
                });
                let scorch_ring = (config::get().scorch && blast.big && blast.scorched < radius * 1.7).then(|| {
                    let to = capped_ring(blast.scorched, front.min(radius * 1.7), CARVE_PER_TICK);
                    let ring = (blast.scorched.max(radius * 0.9), to);
                    blast.scorched = to;
                    ring
                });
                let lift_to = lift_reach(blast.yield_tnt, blast.size);
                let lift = (blast.big && blast.yield_tnt >= config::get().lift_yield && blast.lifted < lift_to)
                    .then(|| {
                        let from = blast.lifted.max(radius * 0.9);
                        let to = capped_ring(from, front.min(lift_to), LIFT_PER_TICK);
                        blast.lifted = to;
                        (from, to)
                    })
                    .filter(|(from, to)| to > from);
                let range = f64::from(shock_range(blast.size));
                let shock = (front - speed / 20.0 < range).then(|| (front.min(range), blast.hit.clone()));
                work.push(Work {
                    id: blast.id,
                    ticks: blast.ticks,
                    center: blast.center,
                    base: blast.origin(),
                    grounded: blast.ground.is_some(),
                    power: blast.power,
                    yield_tnt: blast.yield_tnt,
                    size: blast.size,
                    trails: std::mem::take(&mut blast.trails),
                    columns,
                    carve,
                    scorch_ring,
                    lift,
                    shock,
                });
            }
            all.retain(|b| b.dimension != dimension || b.ticks <= life_ticks(b.size));
        }
        if work.is_empty() {
            return;
        }
        let viewers = viewers(&*host, level);
        for job in work.iter_mut() {
            for puff in plume(job.ticks, job.base, job.power, job.size) {
                emit(host, level, puff);
            }
            if job.grounded {
                dust_ring(host, level, job);
            }
            job.trails = smoke_trails(host, level, std::mem::take(&mut job.trails));
            scorch(host, level, job);
            if let Some((from, to)) = job.carve {
                ejecta(host, level, &dimension, job, from, to);
                carve(host, &dimension, job, from, to);
            }
            if let Some((from, to)) = job.scorch_ring {
                scorch_ring(host, &dimension, job, from, to);
            }
            if let Some((from, to)) = job.lift {
                lift(host, level, &viewers, &dimension, job, from, to);
            }
        }
        // The shockwave reaches what is in its way: it hurts and throws it.
        let mut struck: Vec<(u64, Vec<u32>)> = Vec::new();
        for job in &work {
            if let Some((front, hit)) = &job.shock {
                struck.push((job.id, shockwave(host, level, job, *front, hit)));
            }
        }
        // Hand the trails and what was struck back (a blast may have changed meanwhile).
        let mut all = blasts();
        for job in work {
            if let Some(blast) = all.iter_mut().find(|b| b.id == job.id) {
                let mut trails = job.trails;
                trails.append(&mut blast.trails);
                blast.trails = trails;
            }
        }
        for (id, ids) in struck {
            if let Some(blast) = all.iter_mut().find(|b| b.id == id) {
                blast.hit.extend(ids);
            }
        }
    }

    fn client_tick_post(&self, host: &mut dyn ClientHost, _event: &mut ClientTickPostEvent) {
        // The window's game queues each blast's sound once, to be heard as its shockwave arrives.
        let Some(player) = host.local_player() else {
            return;
        };
        let Some(at) = player.entity().position(&*host) else {
            return;
        };
        let Some(dimension) = player.level(&*host).map(|level| level_name(&*host, level)) else {
            return;
        };
        *window_at() = Some((dimension.clone(), at));
        let now = Instant::now();
        let mut heard = Vec::new();
        let mut kicks = Vec::new();
        {
            let mut all = blasts();
            for blast in all.iter_mut().filter(|b| b.dimension == dimension) {
                if !blast.heard {
                    blast.heard = true;
                    heard.push((blast.center, blast.size));
                }
                // The shockwave throws the window's own player as it reaches them (the server
                // can't: a player's client moves itself).
                let dist = distance(at, blast.center);
                let age = now.duration_since(blast.born).as_secs_f64();
                if !blast.pushed && age * f64::from(shock_speed(blast.size)) >= dist {
                    blast.pushed = true;
                    if let Some((_, push)) = shock_effect(dist, blast.yield_tnt, blast.size) {
                        kicks.push(throw(blast.center, at, push));
                    }
                }
            }
        }
        for kick in kicks {
            host.push_player(kick.map(|v| v as f32));
        }
        for (center, size) in heard {
            let dist = distance(at, center) as f32;
            for cue in boom(dist, size) {
                let mut sound = SoundPlay::at(cue.sound, SoundSource::Blocks, center);
                sound.volume = cue.volume;
                sound.pitch = cue.pitch;
                sound.attenuation = SoundAttenuation::Linear;
                host.play_local_sound(&sound, cue.delay.max(0) as u32);
            }
        }
    }

    fn camera_shake(&self, host: &dyn ClientHost, event: &mut CameraShakeEvent<'_>) {
        let Some(camera) = host.camera() else {
            return;
        };
        let now = Instant::now();
        let all = blasts();
        for blast in all.iter().filter(|b| b.dimension == camera.dimension) {
            let age = now.duration_since(blast.born).as_secs_f32();
            let dist = distance(camera.position, blast.center) as f32;
            let k = shake_strength(blast.size, dist, age);
            if k < 0.002 {
                continue;
            }
            let phase = blast.seed * 40.0;
            event.shake.pitch += k * 0.9 * wobble(age, 5.3, phase);
            event.shake.yaw += k * 0.6 * wobble(age, 4.6, phase + 1.7);
            event.shake.roll += k * 0.5 * wobble(age, 6.1, phase + 3.1);
            event.shake.offset[1] += k * 0.04 * wobble(age, 5.8, phase + 5.9);
        }
    }

    fn play_sound(&self, _context: &dyn LoopContext, event: &mut PlaySoundEvent<'_>) {
        // Vanilla plays its explosion sound the moment the blast goes off; this mod plays its own
        // boom when the shockwave arrives. Both together sound like the same blast twice, so the
        // vanilla one is dropped where the mod has a blast of its own.
        let sound = event.original;
        if !event.starting || sound.identifier != "minecraft:entity.generic.explode" {
            return;
        }
        let now = Instant::now();
        let ours = blasts().iter().any(|b| {
            vanilla_boom_of(
                sound.pitch,
                distance(sound.position, b.center),
                b.size,
                now.duration_since(b.born).as_secs_f32(),
            )
        });
        if ours {
            *event.sound = None;
        }
    }

    fn shader_params(&self, host: &dyn ClientHost, event: &mut ShaderParamsEvent<'_>) {
        let dimension = host.dimension();
        let now = Instant::now();
        let shown: Vec<Shown> = blasts()
            .iter()
            .filter(|b| b.dimension == dimension)
            .map(|b| Shown {
                origin: b.origin(),
                age: now.duration_since(b.born).as_secs_f32(),
                size: b.size,
                seed: b.seed,
                grounded: b.ground.is_some(),
            })
            .collect();
        // Only the slots this mod's pack reads are written.
        for (slot, value) in slots(&shown).into_iter().enumerate() {
            event.params[slot] = value;
        }
    }
}

/// The block at `pos` of `level`, by name (`minecraft:tnt`); empty where it can't be read.
fn block_at(host: &dyn Host, level: LevelRef, pos: [i32; 3]) -> String {
    level.block(host, pos).map(|key| key.id(host)).unwrap_or_default()
}

/// The name of `level` (`minecraft:overworld`).
fn level_name(host: &dyn Host, level: LevelRef) -> String {
    level.dimension().id(host).unwrap_or_default()
}

/// What a falling block the mod throws does when it lands and when it hits.
fn falling(drop_item: bool, hurts: bool) -> Falling {
    let mut falling = Falling::default();
    falling.drop_item = drop_item;
    falling.hurts = hurts;
    falling
}

/// The default state of the block named `name` (`minecraft:air`).
fn state_of(host: &dyn Host, name: &str) -> Option<BlockState> {
    host.block_state_default(host.block_key(name)?)
}

/// The block's name (`minecraft:grass_block`), without its properties.
fn name_of(host: &dyn Host, state: BlockState) -> String {
    state.block().id(host)
}

/// Sets the block at `pos` as `/setblock` does (neighbours told, players sent the change):
/// whether it changed.
fn set_block(host: &mut dyn ServerHost, level: LevelRef, pos: [i32; 3], state: BlockState) -> bool {
    let flags = BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS;
    matches!(level.set_block(host, pos, state, flags), Ok(true))
}

fn counted() -> std::sync::MutexGuard<'static, Option<HashMap<u128, Counted>>> {
    COUNTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One blast's work for one tick, taken out of the shared list.
struct Work {
    id: u64,
    ticks: u32,
    center: [f64; 3],
    /// `Blast::origin`.
    base: [f64; 3],
    grounded: bool,
    power: f32,
    yield_tnt: f32,
    size: f32,
    trails: Vec<Trail>,
    /// The columns of ground to scorch this tick.
    columns: Vec<[i32; 2]>,
    /// The ring of crater to dig this tick, from and to how far out.
    carve: Option<(f64, f64)>,
    /// The ring of land round the crater to scorch this tick.
    scorch_ring: Option<(f64, f64)>,
    /// The ring of land beyond the crater whose shockwave tears it up this tick (`lift`).
    lift: Option<(f64, f64)>,
    /// How far the shockwave's front has run, and what it has already reached; none once it is
    /// past its reach.
    shock: Option<(f64, HashSet<u32>)>,
}

/// A player of the level, where they are: the land torn up flies where players can see it.
struct Viewer {
    position: [f64; 3],
}

/// The players of `level` the server knows.
fn viewers(host: &dyn Host, level: LevelRef) -> Vec<Viewer> {
    level
        .online_players(host)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|player| player.entity().position(host))
        .map(|position| Viewer { position })
        .collect()
}

/// One call to `send_particles`.
struct Puff {
    kind: Particle,
    /// The block of a block particle.
    state: Option<BlockState>,
    at: [f64; 3],
    spread: [f64; 3],
    speed: f64,
    count: i32,
}

impl Puff {
    fn new(kind: Particle, at: [f64; 3], spread: [f64; 3], speed: f64, count: i32) -> Self {
        Self {
            kind,
            state: None,
            at,
            spread,
            speed,
            count,
        }
    }
}

/// Sends a puff as vanilla sends particles: to the players of the level within 32 blocks.
fn emit(host: &mut dyn ServerHost, level: LevelRef, puff: Puff) {
    let Some(key) = puff.kind.key() else {
        return;
    };
    let options = ParticleOptions::simple(key);
    level.send_particles(
        host,
        &options,
        puff.state,
        puff.at,
        puff.spread,
        puff.speed,
        puff.count,
    );
}

/// The ring of dust the shockwave kicks up along the ground: pillars of the ground's own dust at
/// points round a circle that races out and slows, as the pack's dust skirt does.
fn dust_ring(host: &mut dyn ServerHost, level: LevelRef, job: &Work) {
    let secs = f64::from(job.ticks) / 20.0;
    if secs > 1.3 * f64::from(time_scale(job.size)) {
        return;
    }
    let radius = dust_ring_radius(job.size, secs);
    let points = ((6.0 + radius * 1.2) as usize).clamp(8, 22);
    let ground = job.base[1].floor() as i32;
    let turn = unit(job.id, u64::from(job.ticks));
    for i in 0..points {
        let angle = std::f64::consts::TAU * (i as f64 + turn) / points as f64;
        let x = job.base[0] + radius * angle.cos();
        let z = job.base[2] + radius * angle.sin();
        let found = {
            let look = |pos| block_at(&*host, level, pos);
            surface(&look, x.floor() as i32, z.floor() as i32, ground + 3, ground - 4)
        };
        let Some((y, name)) = found else {
            continue;
        };
        if name.contains("water") || name.contains("lava") {
            continue;
        }
        let Some(state) = state_of(&*host, &name) else {
            continue;
        };
        let at = [x, f64::from(y) + 1.05, z];
        let mut pillar = Puff::new(Particle::DustPillar, at, [0.35, 0.1, 0.35], 0.12, 5);
        pillar.state = Some(state);
        emit(host, level, pillar);
        if i % 2 == 0 {
            emit(
                host,
                level,
                Puff::new(Particle::Poof, at, [0.3, 0.15, 0.3], 0.04, 1),
            );
        }
    }
}

/// Moves each flying block's trail on a tick and leaves smoke (and at first flame) where it is;
/// the trails that go on. One ends when its time is up or it has flown into something.
fn smoke_trails(host: &mut dyn ServerHost, level: LevelRef, trails: Vec<Trail>) -> Vec<Trail> {
    let mut flying = Vec::with_capacity(trails.len());
    for mut trail in trails {
        trail.advance();
        if trail.age > TRAIL_TICKS {
            continue;
        }
        if trail.age > 2 && !is_air(&block_at(&*host, level, cell_of(trail.pos))) {
            continue;
        }
        emit(
            host,
            level,
            Puff::new(Particle::Smoke, trail.pos, [0.08; 3], 0.01, 2),
        );
        if trail.age <= 14 && trail.age % 2 == 0 {
            emit(
                host,
                level,
                Puff::new(Particle::Flame, trail.pos, [0.05; 3], 0.0, 1),
            );
        }
        flying.push(trail);
    }
    flying
}

/// Scorches this tick's columns of ground: plants burn away and the soil chars, more surely the
/// nearer the blast.
fn scorch(host: &mut dyn ServerHost, level: LevelRef, job: &Work) {
    if job.columns.is_empty() {
        return;
    }
    let Some(air) = state_of(&*host, "minecraft:air") else {
        return;
    };
    let radius = scorch_radius(job.power);
    let ground = job.center[1].floor() as i32;
    let depth = job.power.ceil() as i32 + 3;
    for &[x, z] in &job.columns {
        let dx = f64::from(x) + 0.5 - job.center[0];
        let dz = f64::from(z) + 0.5 - job.center[2];
        let chance = (1.15 - (dx * dx + dz * dz).sqrt() / radius).clamp(0.0, 1.0);
        if unit(mix([x, 0, z], 911), 0) >= chance {
            continue;
        }
        let found = {
            let look = |pos| block_at(&*host, level, pos);
            scorch_target(&look, x, z, ground + 4, ground - depth)
        };
        for plant in found.plants {
            set_block(host, level, plant, air);
        }
        if let Some((pos, charred)) = found.soil
            && let Some(state) = state_of(&*host, charred)
        {
            set_block(host, level, pos, state);
        }
    }
}

/// What a blast shows of itself to the shader pack.
struct Shown {
    origin: [f64; 3],
    /// Seconds.
    age: f32,
    size: f32,
    seed: f32,
    grounded: bool,
}

/// The pack's slots for the blasts to draw: the three that matter most (the biggest and the
/// youngest), two `vec4`s each, `(x, y, z, age)` and `(size, seed, grounded, on)`. The rest is
/// zero, which the pack reads as no blast.
fn slots(shown: &[Shown]) -> [[f32; 4]; WAVE_SLOTS * 2] {
    let mut live: Vec<&Shown> = shown
        .iter()
        .filter(|s| s.size > 0.0 && (0.0..life_secs(s.size)).contains(&s.age))
        .collect();
    let weight = |s: &Shown| s.size * (1.0 - s.age / life_secs(s.size));
    live.sort_by(|a, b| weight(b).total_cmp(&weight(a)));
    let mut out = [[0.0; 4]; WAVE_SLOTS * 2];
    for (slot, s) in live.into_iter().take(WAVE_SLOTS).enumerate() {
        let [x, y, z] = s.origin;
        out[slot * 2] = [x as f32, y as f32, z as f32, s.age];
        out[slot * 2 + 1] = [s.size, s.seed, f32::from(u8::from(s.grounded)), 1.0];
    }
    out
}

/// A sound the window plays for a blast.
struct Cue {
    sound: &'static str,
    volume: f32,
    pitch: f32,
    /// Ticks from now.
    delay: i32,
}

/// The sounds of a blast `dist` blocks away, timed to its shockwave: a deep boom and a thump as it
/// arrives, and thunder rolling after it. A volume past 1 carries farther (16 blocks times it).
fn boom(dist: f32, size: f32) -> [Cue; 3] {
    let delay = sound_delay(dist, size);
    // A bigger blast is heard farther, and deeper.
    let deep = 1.0 / time_scale(size).sqrt();
    [
        Cue {
            sound: "minecraft:entity.generic.explode",
            volume: (size * 1.3).clamp(4.0, 600.0),
            pitch: BOOM_PITCH * deep.max(0.6),
            delay,
        },
        Cue {
            sound: "minecraft:entity.warden.sonic_boom",
            volume: (size * 0.7).clamp(2.0, 300.0),
            pitch: 0.6 * deep.max(0.6),
            delay,
        },
        Cue {
            sound: "minecraft:entity.lightning_bolt.thunder",
            volume: size.clamp(3.0, 400.0),
            pitch: 0.55 * deep.max(0.6),
            delay: (delay + 4).min(1200),
        },
    ]
}

/// How many ticks the shockwave of a blast of `size` takes to go `dist` blocks.
fn sound_delay(dist: f32, size: f32) -> i32 {
    ((dist.max(0.0) / shock_speed(size)) * 20.0)
        .round()
        .min(1200.0) as i32
}

/// How hard a blast of `size`, `dist` blocks away and `age` seconds old, shakes the camera: a jolt
/// through the ground as it goes off, then the shockwave when it arrives, both dying away. About 1
/// for one TNT close by.
fn shake_strength(size: f32, dist: f32, age: f32) -> f32 {
    if size <= 0.0 || !(0.0..life_secs(size)).contains(&age) {
        return 0.0;
    }
    let amp = (size / 4.5).clamp(0.5, 6.0) / (1.0 + (dist / (size * 5.0)).powi(2));
    let jolt = 0.25 * (-age * 10.0).exp();
    let arrive = dist / shock_speed(size);
    // The wave rises over a few frames instead of all at once, and dies within half a second.
    let wave = if age >= arrive {
        let since = age - arrive;
        (since / 0.05).min(1.0) * (-since * 6.0).exp()
    } else {
        0.0
    };
    amp * (jolt + wave)
}

/// Whether a vanilla explosion sound (pitch `pitch`, `dist` blocks from a blast of `size` that
/// went off `age` seconds ago) is that blast's: vanilla's pitch, at the blast, as it goes off.
fn vanilla_boom_of(pitch: f32, dist: f64, size: f32, age: f32) -> bool {
    pitch > BOOM_PITCH + 0.03 && dist <= f64::from(size) + 2.0 && (0.0..1.5).contains(&age)
}

/// A shake's motion, -1 to 1: two tones that never line up.
fn wobble(age: f32, hz: f32, phase: f32) -> f32 {
    let t = age * std::f32::consts::TAU * hz;
    0.6 * (t + phase).sin() + 0.4 * (t * 1.73 + phase * 2.1).sin()
}

/// The puffs of the burst itself.
fn burst(center: [f64; 3], power: f32, size: f32) -> Vec<Puff> {
    let p = f64::from(power);
    let above = [center[0], center[1] + 0.5, center[2]];
    // (No `flash` particle: in 26.3 it carries a colour that `ParticleOptions::simple` doesn't
    // send, and clients drop the packet. The shader pack draws the flash.)
    vec![
        // Sparks: lava drops leap up and fall back glowing.
        Puff::new(
            Particle::Lava,
            center,
            [p * 0.25, 0.3, p * 0.25],
            0.0,
            8 + (size * 2.0) as i32,
        ),
        Puff::new(Particle::Flame, center, [p * 0.3; 3], 0.18, 30),
        Puff::new(
            Particle::LargeSmoke,
            above,
            [p * 0.45, p * 0.3, p * 0.45],
            0.1,
            40,
        ),
        Puff::new(
            Particle::Poof,
            above,
            [p * 0.4, p * 0.25, p * 0.4],
            0.22,
            20,
        ),
    ]
}

/// The puffs a blast adds on the tick it has reached (from 1): the smoke column and the cloud at its
/// top, which climb as the pack's mushroom does, embers at its foot, and ash falling afterwards.
fn plume(ticks: u32, base: [f64; 3], power: f32, size: f32) -> Vec<Puff> {
    // A bigger blast lives slower (`time_scale`): its puffs keep to the pack's clock.
    let t = (f64::from(ticks) / f64::from(time_scale(size))).round() as u32;
    let secs = f64::from(t) / 20.0;
    let s = f64::from(size);
    let p = f64::from(power).max(2.0);
    let [x, y, z] = base;
    // The cap's height and radius, as shader/realistic_tnt_shockwave's `tnt_cloud` works them out.
    let cap = s * (0.4 + 1.5 * (1.0 - (-secs / 1.6).exp()));
    let wide = s * 1.05 * (1.0 + 0.06 * secs);
    let mut out = Vec::new();
    if t <= 70 {
        out.push(Puff::new(
            Particle::CampfireSignalSmoke,
            [x, y + cap * 0.5, z],
            [s * 0.12, cap * 0.5, s * 0.12],
            0.01,
            2,
        ));
    }
    if t <= 60 && t.is_multiple_of(2) {
        out.push(Puff::new(
            Particle::LargeSmoke,
            [x, y + 0.5, z],
            [p * 0.5, 0.5, p * 0.5],
            0.03,
            4,
        ));
    }
    if (12..=200).contains(&t) && t.is_multiple_of(3) {
        let count = (5.0 * (1.0 - f64::from(t) / 200.0)).ceil() as i32;
        if count > 0 {
            out.push(Puff::new(
                Particle::LargeSmoke,
                [x, y + cap, z],
                [wide * 0.45, wide * 0.3, wide * 0.45],
                0.02,
                count,
            ));
        }
    }
    if (16..200).contains(&t) && t.is_multiple_of(5) {
        out.push(Puff::new(
            Particle::WhiteAsh,
            [x, y + s * 1.5, z],
            [s * 1.6, s * 0.8, s * 1.6],
            0.0,
            8,
        ));
    }
    if (20..200).contains(&t) && t.is_multiple_of(7) {
        out.push(Puff::new(
            Particle::Ash,
            [x, y + s * 1.2, z],
            [s * 1.4, s * 0.8, s * 1.4],
            0.0,
            6,
        ));
    }
    if t <= 100 && t % 10 == 5 {
        out.push(Puff::new(
            Particle::Lava,
            [x, y + 0.3, z],
            [p * 0.35, 0.1, p * 0.35],
            0.0,
            1,
        ));
    }
    out
}

/// How far out the ring of dust is `secs` after a blast of `size`: fast at first, then slowing to
/// rest about four sizes out. shader/realistic_tnt_shockwave's `tnt_dust` uses the same curve.
fn dust_ring_radius(size: f32, secs: f64) -> f64 {
    let secs = secs.max(0.0) / f64::from(time_scale(size));
    f64::from(size) * (0.5 + 2.2 * (1.0 - (-secs / 0.7).exp()))
}

/// About how far an explosion reached: the distance of the farthest block it breaks, at least 2.
fn reach(center: [f64; 3], blocks: &[[i32; 3]]) -> f32 {
    blocks
        .iter()
        .map(|p| {
            let d = [
                p[0] as f64 + 0.5 - center[0],
                p[1] as f64 + 0.5 - center[1],
                p[2] as f64 + 0.5 - center[2],
            ];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .fold(2.0_f64, f64::max) as f32
}

/// How big the pack draws a blast of `yield_tnt` TNT: its volume grows with the yield, so its size
/// with the cube root, with no limit.
fn visual_size(yield_tnt: f32) -> f32 {
    BASE_SIZE * yield_tnt.max(0.05).cbrt()
}

/// The yield in TNT of an explosion of radius (power) `radius`: the cube of its power over TNT's 4,
/// as its volume goes (a creeper 3, a bed 5, an end crystal 6).
fn yield_of(radius: f32) -> f32 {
    (radius / 4.0).powi(3)
}

/// How much slower a blast of `size` lives than one TNT's, and how much faster its front runs:
/// the square root of how much bigger it is.
fn time_scale(size: f32) -> f32 {
    (size / BASE_SIZE).max(1.0).sqrt()
}

/// How fast the shockwave of a blast of `size` runs, in blocks a second.
fn shock_speed(size: f32) -> f32 {
    SHOCK_SPEED * time_scale(size)
}

/// How long a blast of `size` lasts, in seconds and in server ticks.
fn life_secs(size: f32) -> f32 {
    LIFE_SECS * time_scale(size)
}

fn life_ticks(size: f32) -> u32 {
    (life_secs(size) * 20.0).ceil() as u32
}

/// How far the shockwave of a blast of `size` hurts and throws, in blocks.
fn shock_range(size: f32) -> f32 {
    size * 3.75
}

/// How far the crater of a blast of `yield_tnt` reaches, in blocks.
fn crater_radius(yield_tnt: f32) -> f64 {
    CRATER * f64::from(yield_tnt.max(0.0)).cbrt()
}

/// Whether an explosion at `center` in `dimension` joins `blast`: young, and within its crater.
fn joins(blast: &Blast, dimension: &str, center: [f64; 3]) -> bool {
    blast.dimension == dimension
        && blast.ticks <= MERGE_TICKS
        && distance(blast.center, center) <= crater_radius(blast.yield_tnt) + 6.0
}

/// The TNT blocks of the mass an explosion in block `cell` sets off: those within two blocks of it,
/// and every TNT block within two blocks of those (so TNT laid with a block of space between still
/// counts as one mass), at most `cap` of them.
fn tnt_cluster(look: &dyn Fn([i32; 3]) -> String, cell: [i32; 3], cap: usize) -> Vec<[i32; 3]> {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    for dx in -2..=2 {
        for dy in -2..=2 {
            for dz in -2..=2 {
                let pos = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                if seen.insert(pos) && look(pos) == "minecraft:tnt" {
                    queue.push_back(pos);
                }
            }
        }
    }
    let mut found = Vec::new();
    while let Some(pos) = queue.pop_front() {
        found.push(pos);
        if found.len() >= cap {
            break;
        }
        for dx in -2..=2 {
            for dy in -2..=2 {
                for dz in -2..=2 {
                    let next = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                    if seen.insert(next) && look(next) == "minecraft:tnt" {
                        queue.push_back(next);
                    }
                }
            }
        }
    }
    found
}

/// Takes `blocks` out of the world at once, telling no neighbour (TNT taken into a blast must not
/// set anything else off).
fn remove_blocks(host: &mut dyn ServerHost, dimension: &str, blocks: &[[i32; 3]]) {
    let Some(air) = state_of(&*host, "minecraft:air") else {
        return;
    };
    let edit = BlockEdit {
        palette: vec![air],
        runs: blocks
            .iter()
            .map(|&[x, y, z]| BlockRun {
                x,
                z,
                y,
                height: 1,
                surface: None,
                block: 0,
                only: None,
            })
            .collect(),
        ..Default::default()
    };
    let _ = host.edit_blocks(dimension, &edit);
}

/// How many TNT a block lit at `cell` sets off: the mass round it (`tnt_cluster`, as its explosion
/// will take it in), itself included when fire or a signal has already taken it out of the world.
#[cfg(test)]
fn tnt_mass(look: &dyn Fn([i32; 3]) -> String, cell: [i32; 3]) -> usize {
    mass_of(&tnt_cluster(look, cell, MAX_FOLD), cell)
}

/// `tnt_mass` of a mass already walked (`tnt_cluster`).
fn mass_of(cluster: &[[i32; 3]], cell: [i32; 3]) -> usize {
    if cluster.contains(&cell) { cluster.len() } else { cluster.len() + 1 }
}

/// Whether TNT lit at `cell`, whose mass is `cells`, belongs to the same mass as TNT lit earlier at
/// `pos`: within two blocks of it, or of a block of the mass (as `tnt_cluster` walks).
fn same_mass(cells: &HashSet<[i32; 3]>, cell: [i32; 3], pos: [i32; 3]) -> bool {
    let near = |a: [i32; 3], b: [i32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() <= 2);
    if near(cell, pos) {
        return true;
    }
    (-2..=2).any(|dx| {
        (-2..=2).any(|dy| (-2..=2).any(|dz| cells.contains(&[pos[0] + dx, pos[1] + dy, pos[2] + dz])))
    })
}

/// The chunks (`[x, z]`, block coordinates `>> 4`) held loaded and ticking while TNT lit at `cell`
/// in `cluster` burns and its crater is dug: the 3 by 3 round the TNT first, then those of the
/// mass and the ring round them, then a disc out to the scorched land round the crater, at most
/// `HOLD_CHUNKS` chunks out (a bigger crater is dug where its chunks are loaded).
fn hold_area(cell: [i32; 3], cluster: &[[i32; 3]], mass: usize) -> Vec<[i32; 2]> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut add = |chunk: [i32; 2], out: &mut Vec<[i32; 2]>| {
        if seen.insert(chunk) {
            out.push(chunk);
        }
    };
    let (cx, cz) = (cell[0] >> 4, cell[2] >> 4);
    for dx in -1..=1 {
        for dz in -1..=1 {
            add([cx + dx, cz + dz], &mut out);
        }
    }
    let mut of_mass = HashSet::new();
    for &[x, _, z] in cluster {
        of_mass.insert([x >> 4, z >> 4]);
    }
    let mut of_mass: Vec<[i32; 2]> = of_mass.into_iter().collect();
    of_mass.sort_unstable();
    for [x, z] in of_mass {
        for dx in -1..=1 {
            for dz in -1..=1 {
                add([x + dx, z + dz], &mut out);
            }
        }
    }
    let lifted = if mass as f32 >= config::get().lift_yield {
        lift_reach(mass as f32, visual_size(mass as f32))
    } else {
        0.0
    };
    let reach = (crater_radius(mass as f32) * 1.7).max(lifted) + 16.0;
    let r = ((reach / 16.0).ceil() as i32).clamp(1, config::get().hold_chunks);
    for dx in -r..=r {
        for dz in -r..=r {
            if dx * dx + dz * dz <= r * r {
                add([cx + dx, cz + dz], &mut out);
            }
        }
    }
    out
}

/// Lets go of the chunks a dropped fuse held, on its level's next tick.
fn forget(fuse: Fuse) {
    if fuse.held && !fuse.chunks.is_empty() {
        releases().push(Release {
            dimension: fuse.dimension,
            owner: fuse.owner,
            chunks: fuse.chunks,
        });
    }
}

/// The fuse of primed TNT `id`, as its data holds it; none once it is gone.
fn live_fuse(host: &mut dyn ServerHost, id: u32) -> Option<i32> {
    let nbt = host.entity(id)?.nbt(host)?;
    let (_, tag) = nbt
        .iter()
        .find(|(key, _)| key.0.iter().copied().eq("fuse".encode_utf16()))?;
    match tag {
        Tag::Short(fuse) => Some(i32::from(*fuse)),
        Tag::Int(fuse) => Some(*fuse),
        _ => None,
    }
}

/// How long TNT lit in a mass of `yield_tnt` burns, in ticks: vanilla's four seconds, and the time
/// to fly out of its shockwave's reach at `ESCAPE_SPEED`. One TNT keeps vanilla's fuse. A fuse is
/// a short in the TNT's data, which caps it at about 27 minutes.
fn fuse_ticks(yield_tnt: f32) -> i32 {
    let farther = (shock_range(visual_size(yield_tnt)) - shock_range(BASE_SIZE)).max(0.0);
    let ticks = FUSE_TICKS as f32 + farther / config::get().escape_speed * 20.0;
    ticks.min(f32::from(i16::MAX)).round() as i32
}

/// Follows the TNT lit in a mass in `dimension` (`tnt_primed_v2`), once a tick: holds the chunks
/// round it loaded and ticking (`hold_area`, and wherever its TNT is thrown), finds the TNT the lit
/// block became and gives it its whole fuse (`/data merge entity ... {fuse: ...}`), gives the same
/// fuse to TNT lit in the mass since, reads the fuse back each tick for the countdown, and lets go
/// of the chunks once it has gone off and its crater is dug.
fn follow_fuses(host: &mut dyn ServerHost, level: LevelRef, dimension: &str) {
    let mut release: Vec<Release> = {
        let mut queue = releases();
        let (here, rest): (Vec<Release>, Vec<Release>) =
            queue.drain(..).partition(|r| r.dimension == dimension);
        *queue = rest;
        here
    };
    let digging = digging_blasts(dimension);
    let mut hold = Vec::new();
    let mut wanted = Vec::new();
    let mut reads = Vec::new();
    let mut sweeps = Vec::new();
    let mut claimed = HashSet::new();
    {
        let mut all = fuses();
        all.retain_mut(|f| {
            if f.dimension != dimension {
                return true;
            }
            f.age += 1;
            if let Some(gone) = f.gone.as_mut() {
                *gone += 1;
            }
            let lost = f.entity.is_none() && f.gone.is_none() && f.age > FIND_TICKS;
            // A TNT that stopped burning (thrown where nothing ticks) is let go of in the end.
            let stuck = f.age as i64 > i64::from(f.total) + i64::from(HOLD_MAX_TICKS);
            let done = lost
                || stuck
                || f.gone.is_some_and(|gone| {
                    gone >= HOLD_MAX_TICKS
                        || (gone >= HOLD_AFTER_TICKS && !digging_near(&digging, f.pos))
                });
            if done {
                if f.held && !f.chunks.is_empty() {
                    release.push(Release {
                        dimension: dimension.to_owned(),
                        owner: f.owner,
                        chunks: std::mem::take(&mut f.chunks),
                    });
                }
                return false;
            }
            if !f.held {
                f.held = true;
                hold.push((f.owner, f.chunks.clone()));
            }
            match (f.entity, f.gone) {
                (Some(id), None) => {
                    claimed.insert(id);
                    reads.push((f.id, id));
                }
                (None, None) => {
                    f.left = f.total - f.age as i32;
                    wanted.push((f.id, f.pos, f.left));
                }
                _ => {}
            }
            if f.gone.is_none() && f.sweep > 0 {
                f.sweep -= 1;
                sweeps.push(f.id);
            }
            true
        });
    }
    for r in release {
        for chunk in r.chunks {
            hold_chunk(host, level, &r.owner, chunk, false);
        }
    }
    for (owner, chunks) in hold {
        for chunk in chunks {
            hold_chunk(host, level, &owner, chunk, true);
        }
    }
    // The fuses as the TNT itself counts them, and where it is: where its chunk does not tick, it
    // does not burn, so a TNT thrown out of the held chunks has its new chunk held too.
    let read: Vec<(u64, Option<i32>, Option<[i32; 2]>)> = reads
        .into_iter()
        .map(|(fuse, id)| {
            let left = live_fuse(host, id);
            let chunk = host
                .entity(id)
                .and_then(|entity| entity.position(&*host))
                .map(|at| [(at[0].floor() as i32) >> 4, (at[2].floor() as i32) >> 4]);
            (fuse, left, chunk)
        })
        .collect();
    let mut found = Vec::new();
    for (fuse, [x, y, z], left) in wanted {
        let at = [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5];
        let near = level
            .entities_in(
                &*host,
                [at[0] - 1.5, at[1] - 1.0, at[2] - 1.5],
                [at[0] + 1.5, at[1] + 2.0, at[2] + 1.5],
            )
            .unwrap_or_default();
        // The nearest lit TNT no other fuse holds.
        let mut best: Option<(f64, u32)> = None;
        for entity in near {
            let id = entity.id();
            if claimed.contains(&id) {
                continue;
            }
            if kind_of(&*host, entity).as_deref() != Some("minecraft:tnt") {
                continue;
            }
            let Some(pos) = entity.position(&*host) else {
                continue;
            };
            let d = distance(pos, at);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, id));
            }
        }
        let Some((_, id)) = best else {
            continue;
        };
        let left = left.clamp(1, i32::from(i16::MAX));
        if set_fuse(host, id, left) {
            claimed.insert(id);
            found.push((fuse, id, left));
        }
    }
    let mut extra_holds = Vec::new();
    let mut sweep_work = Vec::new();
    {
        let mut all = fuses();
        for f in all.iter_mut() {
            if let Some(&(_, fuse, chunk)) = read.iter().find(|(id, _, _)| *id == f.id) {
                match fuse {
                    Some(left) => f.left = left,
                    None => {
                        f.left = 0;
                        f.gone = Some(0);
                    }
                }
                if let Some(chunk) = chunk.filter(|c| f.gone.is_none() && !f.chunks.contains(c)) {
                    f.chunks.push(chunk);
                    extra_holds.push((f.owner, chunk));
                }
            }
            if let Some(&(_, id, left)) = found.iter().find(|(fuse, _, _)| *fuse == f.id) {
                f.entity = Some(id);
                f.left = left;
            }
            if f.gone.is_none() && sweeps.contains(&f.id) {
                sweep_work.push((f.lo, f.hi, f.left));
            }
        }
    }
    for (owner, chunk) in extra_holds {
        hold_chunk(host, level, &owner, chunk, true);
    }
    // TNT lit in a burning mass since: the same fuse as the mass, so that it all goes off together.
    for (lo, hi, left) in sweep_work {
        if left <= 0 {
            continue;
        }
        let near = level
            .entities_in(
                &*host,
                [f64::from(lo[0]) - 2.0, f64::from(lo[1]) - 2.0, f64::from(lo[2]) - 2.0],
                [f64::from(hi[0]) + 3.0, f64::from(hi[1]) + 3.0, f64::from(hi[2]) + 3.0],
            )
            .unwrap_or_default();
        for entity in near {
            let id = entity.id();
            if claimed.contains(&id) {
                continue;
            }
            if kind_of(&*host, entity).as_deref() != Some("minecraft:tnt") {
                continue;
            }
            if live_fuse(host, id).is_some_and(|fuse| fuse != left) {
                set_fuse(host, id, left);
            }
        }
    }
}

/// Gives primed TNT `id` the fuse `left` (in ticks). A merge that changes nothing says false:
/// then the TNT already had that fuse.
fn set_fuse(host: &mut dyn ServerHost, id: u32, left: i32) -> bool {
    let left = left.clamp(1, i32::from(i16::MAX));
    let Some(entity) = host.entity(id) else {
        return false;
    };
    let mut nbt = Compound::new();
    nbt.insert("fuse".into(), Tag::Short(left as i16));
    entity.merge_nbt(host, nbt) || live_fuse(host, id) == Some(left)
}

/// Holds chunk `chunk` (`[x, z]`) of `level` loaded and ticking for `owner`, or lets go of it.
fn hold_chunk(host: &mut dyn ServerHost, level: LevelRef, owner: &TicketOwner, chunk: [i32; 2], on: bool) {
    let Some(&controller) = FUSE_TICKETS.get() else {
        return;
    };
    if let Some(chunk) = level.chunk_ref(&*host, chunk) {
        chunk.set_mod_forced(host, controller, *owner, on, false);
    }
}

/// What entity `entity` is (`minecraft:tnt`).
fn kind_of(host: &dyn Host, entity: EntityRef) -> Option<String> {
    entity.kind(host).map(|kind| kind.id(host))
}

/// The primed TNT followed now (the lit TNT of each burning mass), not to be thrown out of its
/// mass by another blast.
fn followed_tnt() -> HashSet<u32> {
    fuses()
        .iter()
        .filter(|f| f.gone.is_none())
        .filter_map(|f| f.entity)
        .collect()
}

/// The big blasts of `dimension` still digging their crater or scorching the land round it: where,
/// and how far out the chunks they dig reach.
fn digging_blasts(dimension: &str) -> Vec<([f64; 3], f64)> {
    blasts()
        .iter()
        .filter(|b| b.dimension == dimension && b.big)
        .filter_map(|b| {
            let radius = crater_radius(b.yield_tnt);
            let lifted = if b.yield_tnt >= config::get().lift_yield { lift_reach(b.yield_tnt, b.size) } else { 0.0 };
            (b.carved < radius * 1.1 || b.scorched < radius * 1.7 || b.lifted < lifted)
                .then_some((b.center, (radius * 1.7).max(lifted) + 16.0))
        })
        .collect()
}

fn digging_near(digging: &[([f64; 3], f64)], pos: [i32; 3]) -> bool {
    let at = [f64::from(pos[0]) + 0.5, f64::from(pos[1]), f64::from(pos[2]) + 0.5];
    digging.iter().any(|&(center, reach)| distance(center, at) <= reach)
}

/// The countdown over each player's hotbar: the biggest lit TNT mass in their level whose shockwave
/// could reach them (with a wide margin), in seconds as the server runs them, shown again as each
/// second passes. When it goes off, the bar says when its shockwave (the boom, the shake and the
/// push) gets to them, or that it won't.
fn countdown(host: &mut dyn ServerHost, level: LevelRef, dimension: &str) {
    let players = level.online_players(&*host).unwrap_or_default();
    if players.is_empty() {
        return;
    }
    let now = level.game_time(&*host).unwrap_or(0);
    let tps = f32::from_bits(SERVER_TPS.load(Ordering::Relaxed));
    let center_of = |pos: [i32; 3]| [f64::from(pos[0]) + 0.5, f64::from(pos[1]), f64::from(pos[2]) + 0.5];
    // What to tell whom, decided with the locks held and sent after (a host call may fire a hook).
    let mut lines: Vec<(PlayerRef, Text)> = Vec::new();
    {
        let all = fuses();
        let mut counted = counted();
        let counted = counted.get_or_insert_with(HashMap::new);
        for player in players {
            let Some(at) = player.entity().position(&*host) else {
                continue;
            };
            let uuid = player.uuid();
            let last = counted.get(&uuid).copied();
            let shown = all
                .iter()
                .filter(|f| {
                    f.dimension == dimension && f.entity.is_some() && f.gone.is_none() && f.left > 0
                })
                .filter(|f| {
                    let reach = f64::from(shock_range(visual_size(f.mass as f32)));
                    distance(at, center_of(f.pos)) <= reach * 4.0 + COUNTDOWN_MARGIN
                })
                .max_by_key(|f| (f.mass, std::cmp::Reverse(f.left)))
                .map(|f| (f.id, f.mass, f.left));
            let Some((id, mass, left)) = shown else {
                if let Some(last) = last {
                    counted.remove(&uuid);
                    let text = all
                        .iter()
                        .find(|f| f.id == last.fuse && f.gone.is_some())
                        .map_or_else(String::new, |f| {
                            went_off_text(f.mass, distance(at, center_of(f.pos)))
                        });
                    lines.push((player, colored(text, "#FFAA00")));
                }
                continue;
            };
            let mut secs = countdown_secs(left, tps);
            let same = last.is_some_and(|l| l.fuse == id);
            // The same countdown never goes back up (the server's pace wavering by a tick or two).
            if let Some(last) = last.filter(|_| same) {
                secs = secs.min(last.secs);
            }
            let changed = !same || last.is_some_and(|l| l.secs != secs);
            let due = last.is_none_or(|l| now.wrapping_sub(l.at) >= COUNTDOWN_RESEND);
            if changed || due {
                counted.insert(uuid, Counted { secs, fuse: id, at: now });
                lines.push((player, colored(countdown_text(mass, secs), "#FF5555")));
            } else if let Some(last) = last {
                counted.insert(uuid, Counted { secs, ..last });
            }
        }
    }
    for (player, line) in lines {
        let _ = player.action_bar(host, line);
    }
}

/// `text` in colour `color` (`#FF5555`).
fn colored(text: String, color: &str) -> Text {
    let mut style = TextStyle::default();
    style.color = Some(color.to_owned());
    Text::literal(text).with_style(style)
}

/// What the bar says as a mass of `mass` TNT goes off `dist` blocks away: when its shockwave gets
/// here (it runs at `shock_speed`), or that it won't.
fn went_off_text(mass: usize, dist: f64) -> String {
    let size = visual_size(mass as f32);
    let reach = f64::from(shock_range(size));
    let mass = grouped(mass as i64);
    if dist > reach {
        return format!(
            "{mass} TNT went off! You are out of its reach ({} blocks)",
            grouped(reach.round() as i64)
        );
    }
    let secs = (dist / f64::from(shock_speed(size))).ceil() as i64;
    if secs <= 0 {
        format!("{mass} TNT went off!")
    } else {
        format!("{mass} TNT went off! The shockwave reaches you in {secs} s")
    }
}

/// `n` with its thousands set apart by commas, as English writes them: "25,087".
fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        text.push('-');
    }
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// Whole seconds (rounded up) for `left` server ticks at `tps` ticks a second (`kept_rate`): 20 a
/// second when the server keeps up, fewer when it falls behind (then a tick lasts longer).
fn countdown_secs(left: i32, tps: f32) -> i32 {
    let tps = if tps.is_finite() { tps.max(1.0) } else { 20.0 };
    (left.max(0) as f32 / tps).ceil() as i32
}

/// The ticks a second a server set to `rate` keeps when its ticks take `mspt` milliseconds of
/// work on average (`tick_stats`, over its last 100 ticks, pauses left out): the rate while a
/// tick's work fits in a tick's time, as `/tick query` tells, and fewer when it does not.
fn kept_rate(rate: f32, mspt: f64) -> f32 {
    if !rate.is_finite() || rate <= 0.0 {
        return 20.0;
    }
    if mspt.is_finite() && mspt * f64::from(rate) > 1000.0 {
        (1000.0 / mspt) as f32
    } else {
        rate
    }
}

/// "512 TNT: explodes in 14 s, reach 144 blocks".
fn countdown_text(mass: usize, secs: i32) -> String {
    let reach = shock_range(visual_size(mass as f32)).round() as i64;
    format!(
        "{} TNT: explodes in {secs} s, reach {} blocks",
        grouped(mass as i64),
        grouped(reach)
    )
}

/// Where a ring may end that starts at `from` and runs toward `to`, so that it holds no more than
/// `cap` columns.
fn capped_ring(from: f64, to: f64, cap: f64) -> f64 {
    let most = (from * from + cap / std::f64::consts::PI).sqrt();
    to.min(most).max(from)
}

/// The columns (`x`, `z`) whose centres lie farther than `from` and no farther than `to` from
/// `center`: each column of a disc once, ring after ring.
fn ring_columns(center: [f64; 3], from: f64, to: f64) -> Vec<[i32; 2]> {
    let mut out = Vec::new();
    if to <= from {
        return out;
    }
    let (cx, cz) = (center[0], center[2]);
    let x0 = (cx - to).floor() as i32;
    let x1 = (cx + to).ceil() as i32;
    for x in x0..=x1 {
        let dx = f64::from(x) + 0.5 - cx;
        let outer = to * to - dx * dx;
        if outer < 0.0 {
            continue;
        }
        let z0 = (cz - outer.sqrt() - 1.0).floor() as i32;
        let z1 = (cz + outer.sqrt() + 1.0).ceil() as i32;
        for z in z0..=z1 {
            let dz = f64::from(z) + 0.5 - cz;
            let r2 = dx * dx + dz * dz;
            if r2 <= to * to && (r2 > from * from || (from == 0.0 && r2 == 0.0)) {
                out.push([x, z]);
            }
        }
    }
    out
}

/// The span of a crater column `r` blocks from its centre, for a crater reaching `radius` round a
/// blast at height `cy`: the lowest and highest block it takes out, or none outside it. A bowl half
/// as deep as it is wide below, and everything standing above it (trees, walls) cut away, less of
/// it toward the rim. Its edge is ragged (`rough`, 0..1 for the column).
fn crater_span(r: f64, radius: f64, cy: f64, rough: f64, floor: i32) -> Option<(i32, i32)> {
    let edge = radius * (0.9 + 0.2 * rough);
    if r > edge || edge <= 0.0 {
        return None;
    }
    let u = r / edge;
    let bowl = (1.0 - u * u).sqrt();
    let low = ((cy - radius * 0.5 * bowl).floor() as i32).max(floor);
    let high = (cy + radius * 0.85 * bowl + 2.0).floor() as i32;
    (high >= low).then_some((low, high))
}

/// The lowest block a crater may take out in `dimension`: never the bedrock.
fn crater_floor(dimension: &str) -> i32 {
    match dimension {
        "minecraft:the_nether" => 5,
        "minecraft:the_end" => 1,
        _ => -59,
    }
}

/// Digs one ring of a big blast's crater, straight into the chunks: the column empties, and its
/// new floor chars (soil to coarse dirt, stone to blackstone, molten rock near the middle).
fn carve(host: &mut dyn ServerHost, dimension: &str, job: &Work, from: f64, to: f64) {
    let radius = crater_radius(job.yield_tnt);
    let floor = crater_floor(dimension);
    let palette = ["minecraft:air", "minecraft:coarse_dirt", "minecraft:blackstone", "minecraft:magma_block"];
    let (Some(palette), Some(filters)) = (
        states_of(&*host, &palette),
        tag_filters(&*host, &["minecraft:dirt", "minecraft:base_stone_overworld"]),
    ) else {
        return;
    };
    let mut edit = BlockEdit {
        palette,
        filters,
        ..Default::default()
    };
    for [x, z] in ring_columns(job.center, from, to) {
        let r = column_distance(job.center, x, z);
        let rough = unit(mix([x, 7, z], job.id), 0);
        let Some((low, high)) = crater_span(r, radius, job.center[1], rough, floor) else {
            continue;
        };
        let height = (high - low + 1) as u32;
        edit.runs.push(BlockRun {
            x,
            z,
            y: low,
            height,
            surface: None,
            block: 0,
            only: None,
        });
        edit.runs.push(BlockRun {
            x,
            z,
            y: low - 1,
            height: 1,
            surface: None,
            block: 1,
            only: Some(0),
        });
        let molten = r < radius * 0.3 && unit(mix([x, 8, z], job.id), 0) < 0.3;
        edit.runs.push(BlockRun {
            x,
            z,
            y: low - 1,
            height: 1,
            surface: None,
            block: if molten { 3 } else { 2 },
            only: Some(1),
        });
    }
    if !edit.runs.is_empty() {
        if let Err(error) = host.edit_blocks(dimension, &edit) {
            eprintln!("[realistic_tnt] carve: block edit failed in {dimension}: {error}");
        }
    }
}

/// Scorches one ring of land round a big blast's crater: plants burn away and soil chars, less of
/// it the farther out.
fn scorch_ring(host: &mut dyn ServerHost, dimension: &str, job: &Work, from: f64, to: f64) {
    let radius = crater_radius(job.yield_tnt);
    let (Some(palette), Some(filters)) = (
        states_of(&*host, &["minecraft:air", "minecraft:coarse_dirt"]),
        tag_filters(&*host, &["minecraft:replaceable", "minecraft:flowers", "minecraft:dirt"]),
    ) else {
        return;
    };
    let mut edit = BlockEdit {
        palette,
        filters,
        ..Default::default()
    };
    for [x, z] in ring_columns(job.center, from, to) {
        let r = column_distance(job.center, x, z);
        let chance = (1.0 - (r - radius) / (radius * 0.8)).clamp(0.0, 1.0);
        if unit(mix([x, 9, z], job.id), 0) >= chance {
            continue;
        }
        let surface = Some(Heightmap::MotionBlockingNoLeaves);
        for (y, height, block, only) in [(0, 2, 0, 0), (0, 2, 0, 1), (-1, 1, 1, 2)] {
            edit.runs.push(BlockRun {
                x,
                z,
                y,
                height,
                surface,
                block,
                only: Some(only),
            });
        }
    }
    if !edit.runs.is_empty() {
        if let Err(error) = host.edit_blocks(dimension, &edit) {
            eprintln!("[realistic_tnt] scorch_ring: block edit failed in {dimension}: {error}");
        }
    }
}

/// Throws some of what a big blast's ring is about to dig out: blocks of the ground, flung up and
/// away, harder the bigger the blast. (Horizon keeps at most 1024 falling blocks in a level.)
fn ejecta(host: &mut dyn ServerHost, level: LevelRef, dimension: &str, job: &Work, from: f64, to: f64) {
    let columns = ring_columns(job.center, from, to);
    if columns.is_empty() {
        return;
    }
    let step = (columns.len() / EJECTA_PER_TICK).max(1);
    let ground = job.base[1].floor() as i32 - 1;
    let strength = f64::from(job.yield_tnt.max(1.0)).powf(1.0 / 6.0);
    for (i, &[x, z]) in columns.iter().step_by(step).take(EJECTA_PER_TICK).enumerate() {
        let name = block_at(&*host, level, [x, ground, z]);
        if !is_debris(&name) {
            continue;
        }
        let Some(state) = state_of(&*host, &name) else {
            continue;
        };
        let seed = mix([x, ground, z], job.id);
        let from_pos = [f64::from(x) + 0.5, f64::from(ground) + 1.2, f64::from(z) + 0.5];
        let out = throw(job.center, from_pos, 0.7 * strength * (0.7 + 0.6 * unit(seed, 1)));
        let velocity = [out[0], out[1] + 0.4 + 0.5 * unit(seed, 2), out[2]].map(|v| v.clamp(-3.9, 3.9));
        let falling = falling(false, true);
        host.spawn_falling_block(dimension, state, from_pos, velocity, falling);
        if i % 3 == 0 {
            emit(
                host,
                level,
                Puff::new(Particle::LargeSmoke, from_pos, [0.6, 0.4, 0.6], 0.05, 3),
            );
        }
    }
}

/// How far out the shockwave of a blast of `yield_tnt` and `size` tears up the land (`lift`): where
/// it still pushes `LIFT_PUSH` blocks a tick (`shock_effect`).
fn lift_reach(yield_tnt: f32, size: f32) -> f64 {
    let most = SHOCK_PUSH * (1.0 + 0.35 * f64::from(yield_tnt.max(1.0)).ln());
    if most <= LIFT_PUSH {
        return 0.0;
    }
    f64::from(shock_range(size)) * (1.0 - (LIFT_PUSH / most).sqrt())
}

/// What a block is to the shockwave sweeping over it (`lift`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cover {
    /// Nothing there.
    Air,
    /// Water or lava: the land under it is left alone.
    Fluid,
    /// Knocked down where the shockwave pushes at least `BREAKS_AT[kind]` (`Fragile`, `Wood`,
    /// `Soil`, `Masonry`). Of the soil only the top block goes.
    Breaks(u8),
    /// Stays, and so does what is under it: rock, ores, metal, blocks holding things (chests,
    /// signs, beds), other mods' blocks.
    Hard,
}

/// The kinds of `Cover::Breaks`, and the push (blocks a tick, `shock_effect`) each needs: glass,
/// leaves and plants go first, then wood (trees, wooden houses), then the turf, then brick and
/// stone buildings.
const FRAGILE: u8 = 0;
const WOOD: u8 = 1;
const SOIL: u8 = 2;
const MASONRY: u8 = 3;
const BREAKS_AT: [f64; 4] = [LIFT_PUSH, 0.8, 1.5, 2.0];

fn cover_of(name: &str) -> Cover {
    let Some(name) = name.strip_prefix("minecraft:") else {
        return Cover::Hard;
    };
    const FRAGILE_EXACT: &[&str] = &[
        "short_grass", "grass", "tall_grass", "fern", "large_fern", "dead_bush", "bush",
        "firefly_bush", "short_dry_grass", "tall_dry_grass", "vine", "snow", "sweet_berry_bush",
        "sugar_cane", "bamboo", "bamboo_sapling", "cactus", "cactus_flower", "moss_carpet", "azalea",
        "flowering_azalea", "hanging_roots", "glow_lichen", "pink_petals", "wildflowers",
        "leaf_litter", "dandelion", "poppy", "blue_orchid", "allium", "azure_bluet", "oxeye_daisy",
        "cornflower", "lily_of_the_valley", "sunflower", "lilac", "rose_bush", "peony", "torchflower",
        "pitcher_plant", "wither_rose", "open_eyeblossom", "closed_eyeblossom", "brown_mushroom",
        "red_mushroom", "wheat", "carrots", "potatoes", "beetroots", "melon", "pumpkin",
        "carved_pumpkin", "jack_o_lantern", "lily_pad", "glass", "glass_pane", "tinted_glass",
        "cocoa", "big_dripleaf", "big_dripleaf_stem", "small_dripleaf", "spore_blossom", "torch",
        "wall_torch", "soul_torch", "soul_wall_torch", "redstone_torch", "redstone_wall_torch",
        "lantern", "soul_lantern", "flower_pot", "cobweb", "ice", "frosted_ice", "hay_block",
        "ladder", "scaffolding", "rail", "powered_rail", "detector_rail", "activator_rail",
        "tripwire", "tripwire_hook", "lever", "candle", "sea_pickle", "weeping_vines",
        "weeping_vines_plant", "twisting_vines", "twisting_vines_plant", "crimson_roots",
        "warped_roots", "nether_sprouts", "crimson_fungus", "warped_fungus", "kelp_block",
    ];
    const FRAGILE_ENDS: &[&str] = &[
        "_leaves", "_sapling", "_tulip", "_stained_glass", "_stained_glass_pane", "_wool",
        "_carpet", "_candle", "_coral", "_coral_fan", "_coral_wall_fan",
    ];
    const WOOD_EXACT: &[&str] = &[
        "bamboo_block", "stripped_bamboo_block", "bamboo_mosaic", "bamboo_mosaic_stairs",
        "bamboo_mosaic_slab", "crafting_table", "bookshelf", "composter", "note_block", "loom",
        "cartography_table", "fletching_table", "smithing_table", "dried_kelp_block", "target",
        "mushroom_stem",
    ];
    const WOOD_ENDS: &[&str] = &["_log", "_wood", "_stem", "_hyphae", "_mushroom_block"];
    const WOODS: &[&str] = &[
        "oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "pale_oak",
        "bamboo", "crimson", "warped",
    ];
    const WOOD_PARTS: &[&str] = &[
        "planks", "stairs", "slab", "fence", "fence_gate", "door", "trapdoor", "pressure_plate",
        "button",
    ];
    const SOIL_EXACT: &[&str] = &[
        "grass_block", "dirt", "coarse_dirt", "podzol", "mycelium", "rooted_dirt", "mud", "sand",
        "red_sand", "gravel", "snow_block", "moss_block", "dirt_path", "farmland", "clay",
        "pale_moss_block", "suspicious_sand", "suspicious_gravel",
    ];
    const MASONRY_EXACT: &[&str] = &[
        "bricks", "cobblestone", "mossy_cobblestone", "cobbled_deepslate", "stone_bricks",
        "mossy_stone_bricks", "cracked_stone_bricks", "chiseled_stone_bricks", "mud_bricks",
        "packed_mud", "terracotta", "cut_sandstone", "chiseled_sandstone", "smooth_sandstone",
        "cut_red_sandstone", "chiseled_red_sandstone", "smooth_red_sandstone", "smooth_stone",
        "quartz_block", "quartz_bricks", "quartz_pillar", "smooth_quartz", "chiseled_quartz_block",
        "purpur_block", "purpur_pillar", "prismarine", "prismarine_bricks", "dark_prismarine",
        "polished_andesite", "polished_diorite", "polished_granite", "polished_deepslate",
        "polished_blackstone", "polished_tuff", "polished_basalt", "chiseled_deepslate",
        "chiseled_tuff", "chiseled_polished_blackstone", "infested_stone_bricks", "glowstone",
        "sea_lantern", "bone_block",
    ];
    const MASONRY_ENDS: &[&str] = &[
        "_bricks", "_tiles", "_concrete", "_concrete_powder", "_terracotta", "_stairs", "_slab",
        "_wall",
    ];
    // Blocks holding something (a chest's contents, a sign's text, a bed, a banner...) stay.
    const HOLDING: &[&str] = &[
        "_sign", "_hanging_sign", "_bed", "_banner", "_shulker_box", "chest", "barrel", "furnace",
        "smoker", "hopper", "dispenser", "dropper", "lectern", "jukebox", "beehive", "bee_nest",
        "spawner", "beacon", "bell", "campfire", "brewing_stand", "enchanting_table",
        "chiseled_bookshelf", "decorated_pot", "crafter", "_head", "_skull",
    ];
    if HOLDING.iter().any(|h| name.ends_with(h)) {
        return Cover::Hard;
    }
    let wooden_part = || {
        WOODS.iter().any(|wood| {
            name.strip_prefix(wood)
                .and_then(|rest| rest.strip_prefix('_'))
                .is_some_and(|part| WOOD_PARTS.contains(&part))
        })
    };
    match name {
        "air" | "cave_air" | "void_air" | "light" => Cover::Air,
        "water" | "lava" | "bubble_column" | "seagrass" | "tall_seagrass" | "kelp" | "kelp_plant" => {
            Cover::Fluid
        }
        _ if SOIL_EXACT.contains(&name) => Cover::Breaks(SOIL),
        _ if FRAGILE_EXACT.contains(&name) || FRAGILE_ENDS.iter().any(|end| name.ends_with(end)) => {
            Cover::Breaks(FRAGILE)
        }
        _ if WOOD_EXACT.contains(&name)
            || WOOD_ENDS.iter().any(|end| name.ends_with(end))
            || wooden_part() =>
        {
            Cover::Breaks(WOOD)
        }
        _ if MASONRY_EXACT.contains(&name) || MASONRY_ENDS.iter().any(|end| name.ends_with(end)) => {
            Cover::Breaks(MASONRY)
        }
        _ => Cover::Hard,
    }
}

/// What a shockwave pushing `push` blocks a tick tears off one column, its blocks from
/// `base - blocks.len()` up to its top (`base - 1`), bottom first: from the top down, every block
/// that push knocks down (`BREAKS_AT`), until one it can't, water, or the ground's top block (taken
/// too where it pushes hard enough). The lowest and highest block it takes, if any.
fn torn_off(blocks: &[Cover], base: i32, push: f64) -> Option<(i32, i32)> {
    let bottom = base - blocks.len() as i32;
    let mut top = None;
    let mut low = None;
    for (i, cover) in blocks.iter().enumerate().rev() {
        let y = bottom + i as i32;
        match *cover {
            Cover::Air => {}
            Cover::Breaks(kind) if push >= config::get().breaks_at[usize::from(kind)] => {
                top.get_or_insert(y);
                low = Some(y);
                if kind == SOIL {
                    break;
                }
            }
            Cover::Breaks(_) | Cover::Fluid | Cover::Hard => break,
        }
    }
    Some((low?, top?))
}

/// Whether a block torn off may fly: not half of a door or a tall plant, which would land as half.
fn may_fly(name: &str) -> bool {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    !(name.ends_with("_door")
        || matches!(
            name,
            "tall_grass" | "large_fern" | "sunflower" | "lilac" | "rose_bush" | "peony"
                | "pitcher_plant" | "tall_dry_grass" | "small_dripleaf" | "big_dripleaf"
        ))
}

/// The shockwave of a big blast sweeping over one ring of land beyond its crater (`from` to `to`
/// blocks out): from the top of each column down it knocks over what its push can (glass and
/// plants, then trees and wood, the turf, then brick and stone buildings: `cover_of`), straight
/// out of the chunks, and throws as much of it as the level takes up and away where players can
/// see it (Horizon keeps at most 1024 falling blocks in a level: as they land, more fly), the
/// nearest to them first. Crumbs of it burst round the players near it.
fn lift(
    host: &mut dyn ServerHost,
    level: LevelRef,
    viewers: &[Viewer],
    dimension: &str,
    job: &Work,
    from: f64,
    to: f64,
) {
    let columns = ring_columns(job.center, from, to);
    if columns.is_empty() {
        return;
    }
    let read_at: Vec<(i32, i32)> = columns.iter().map(|&[x, z]| (x, z)).collect();
    let Ok(read) = host.read_columns(
        dimension,
        &read_at,
        Heightmap::WorldSurface,
        (LIFT_DEPTH, 0),
    ) else {
        return;
    };
    let depth = LIFT_DEPTH as usize;
    if read.blocks.len() < columns.len() * depth || read.bases.len() < columns.len() {
        return;
    }
    let names: Vec<String> = read.palette.iter().map(|&state| name_of(&*host, state)).collect();
    let covers: Vec<Cover> = names.iter().map(|name| cover_of(name)).collect();
    let flies: Vec<bool> = names.iter().map(|name| may_fly(name)).collect();
    // Where the players are: the server's, and the window's own in single player.
    let mut watchers: Vec<[f64; 3]> = viewers.iter().map(|v| v.position).collect();
    if let Some((dim, at)) = window_at().as_ref()
        && dim == dimension
    {
        watchers.push(*at);
    }
    // How near the nearest player is, across the ground.
    let nearest = |at: [f64; 3]| {
        watchers
            .iter()
            .map(|w| ((w[0] - at[0]).powi(2) + (w[2] - at[2]).powi(2)).sqrt())
            .fold(f64::INFINITY, f64::min)
    };
    let Some(air) = state_of(&*host, "minecraft:air") else {
        return;
    };
    let mut edit = BlockEdit {
        palette: vec![air],
        ..Default::default()
    };
    // What could fly: (how near a player it is, where, which block, how hard it is pushed).
    let mut flying: Vec<(f64, [i32; 3], u16, f64)> = Vec::new();
    let mut column_covers = Vec::with_capacity(depth);
    for (i, &[x, z]) in columns.iter().enumerate() {
        let Some(base) = read.bases[i] else {
            continue;
        };
        let r = column_distance(job.center, x, z);
        let Some((_, push)) = shock_effect(r, job.yield_tnt, job.size) else {
            continue;
        };
        // A ragged edge: each column takes the push a little harder or softer.
        let seed = mix([x, 13, z], job.id);
        let push = push * (0.75 + 0.5 * unit(seed, 0));
        if push < LIFT_PUSH {
            continue;
        }
        let blocks = &read.blocks[i * depth..(i + 1) * depth];
        column_covers.clear();
        column_covers.extend(
            blocks
                .iter()
                .map(|&b| covers.get(usize::from(b)).copied().unwrap_or(Cover::Hard)),
        );
        let Some((low, high)) = torn_off(&column_covers, base, push) else {
            continue;
        };
        edit.runs.push(BlockRun {
            x,
            z,
            y: low,
            height: (high - low + 1) as u32,
            surface: None,
            block: 0,
            only: None,
        });
        // Its top, its middle and its foot may fly, where a player can see them fly.
        let at = [f64::from(x) + 0.5, f64::from(high), f64::from(z) + 0.5];
        let near = nearest(at);
        if near > SEEN_FLYING {
            continue;
        }
        let bottom = base - depth as i32;
        let mut last = None;
        for y in [high, (high + low) / 2, low] {
            if last == Some(y) {
                continue;
            }
            last = Some(y);
            let block = blocks[(y - bottom) as usize];
            let index = usize::from(block);
            if covers.get(index).copied() == Some(Cover::Air) || !flies.get(index).copied().unwrap_or(false) {
                continue;
            }
            let wanted = near + 24.0 * unit(seed, 2 + (y - low) as u64);
            flying.push((wanted, [x, y, z], block, push));
        }
    }
    if edit.runs.is_empty() {
        return;
    }
    if let Err(error) = host.edit_blocks(dimension, &edit) {
        eprintln!("[realistic_tnt] lift: block edit failed in {dimension}: {error}");
    }
    // The nearest fly first, out and up, harder the harder they are pushed, until the level holds
    // all it will; crumbs burst where a player is close.
    flying.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut thrown = 0;
    let mut refused = 0;
    let mut puffs = 0;
    for &(near, [x, y, z], block, push) in &flying {
        if (thrown >= LIFT_THROWS || refused >= 8) && (puffs >= LIFT_PUFFS || near > 48.0) {
            break;
        }
        // It flies as its block's default state, as a block placed by hand (a waterlogged one lands
        // dry instead of leaving water where it falls).
        let Some(state) = read
            .palette
            .get(usize::from(block))
            .and_then(|&state| host.block_state_default(state.block()))
        else {
            continue;
        };
        let at = [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5];
        let seed = mix([x, y, z], job.id ^ 0x5157);
        if thrown < LIFT_THROWS && refused < 8 {
            let out = throw(job.center, at, push.min(3.9) * (0.6 + 0.6 * unit(seed, 0)));
            let velocity = [out[0], out[1] + 0.25 + 0.45 * unit(seed, 1), out[2]].map(|v| v.clamp(-3.9, 3.9));
            let falling = falling(false, false);
            if host.spawn_falling_block(dimension, state, at, velocity, falling) {
                thrown += 1;
                refused = 0;
            } else {
                // The level holds all the falling blocks it will (or the chunk is gone).
                refused += 1;
            }
        }
        if puffs < LIFT_PUFFS && near <= 48.0 {
            let mut puff = Puff::new(Particle::Block, at, [0.7, 0.9, 0.7], 0.4, 10);
            puff.state = Some(state);
            emit(host, level, puff);
            puffs += 1;
        }
    }
}

/// How far column (`x`, `z`)'s centre is from `center`, across the ground.
fn column_distance(center: [f64; 3], x: i32, z: i32) -> f64 {
    let dx = f64::from(x) + 0.5 - center[0];
    let dz = f64::from(z) + 0.5 - center[2];
    (dx * dx + dz * dz).sqrt()
}

/// What the shockwave of a blast of `yield_tnt` and `size` does `dist` blocks out: its damage, in
/// health points, and its push, in blocks a tick; none past its reach. Both fall off with the
/// square of the distance left to its edge.
fn shock_effect(dist: f64, yield_tnt: f32, size: f32) -> Option<(f32, f64)> {
    let range = f64::from(shock_range(size));
    if !(0.0..range).contains(&dist) {
        return None;
    }
    let p = (1.0 - dist / range).powi(2);
    let y = f64::from(yield_tnt.max(1.0));
    let damage = SHOCK_DAMAGE * p as f32 * y.cbrt() as f32;
    let push = (SHOCK_PUSH * p * (1.0 + 0.35 * y.ln())).min(3.9);
    Some((damage, push))
}

/// A push of `strength` blocks a tick away from `center`, for something at `at`: out and somewhat
/// up, each axis cut to Horizon's largest push.
fn throw(center: [f64; 3], at: [f64; 3], strength: f64) -> [f64; 3] {
    let d = [at[0] - center[0], at[1] - center[1], at[2] - center[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(0.001);
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    [
        dir[0] * strength,
        dir[1].max(0.0) * strength + 0.35 * strength,
        dir[2] * strength,
    ]
    .map(|v| v.clamp(-3.9, 3.9))
}

/// The shockwave of a blast, this tick: everything its front (`front` blocks out) has reached that
/// it hadn't (`hit`) is hurt and thrown. The window's own player is thrown by the window (`tick`).
/// The entities reached.
fn shockwave(
    host: &mut dyn ServerHost,
    level: LevelRef,
    job: &Work,
    front: f64,
    hit: &HashSet<u32>,
) -> Vec<u32> {
    let c = job.center;
    let near = level
        .entities_in(
            &*host,
            [c[0] - front, c[1] - front, c[2] - front],
            [c[0] + front, c[1] + front, c[2] + front],
        )
        .unwrap_or_default();
    let mut reached = Vec::new();
    // The lit TNT of a mass still burning stays with its mass, to go off with it.
    let followed = followed_tnt();
    for entity in near {
        let id = entity.id();
        if hit.contains(&id) || followed.contains(&id) {
            continue;
        }
        let Some(at) = entity.position(&*host) else {
            continue;
        };
        let dist = distance(at, c);
        if dist > front {
            continue;
        }
        reached.push(id);
        let Some((damage, push)) = shock_effect(dist, job.yield_tnt, job.size) else {
            continue;
        };
        if damage >= 0.5 {
            let _ = entity.hurt(host, "minecraft:explosion", damage, None, None);
        }
        if kind_of(&*host, entity).as_deref() != Some("minecraft:player") {
            entity.push(host, throw(c, at, push));
        }
    }
    reached
}

/// The default states of the blocks named `names`, in order; none if one is unknown.
fn states_of(host: &dyn Host, names: &[&str]) -> Option<Vec<BlockState>> {
    names.iter().map(|name| state_of(host, name)).collect()
}

/// Filters on the block tags `tags` (`minecraft:dirt`), in order; none if one can't be made.
fn tag_filters(host: &dyn Host, tags: &[&str]) -> Option<Vec<StatePredicate>> {
    let states = host.block_states();
    tags.iter()
        .map(|tag| {
            let id = Identifier::parse(tag).ok()?;
            let key = host.tags().key(&id)?;
            StatePredicate::tag(&states, host.tags(), key, &[]).ok()
        })
        .collect()
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// The block a point is in.
fn cell_of(pos: [f64; 3]) -> [i32; 3] {
    [
        pos[0].floor() as i32,
        pos[1].floor() as i32,
        pos[2].floor() as i32,
    ]
}

fn is_air(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// A plant a blast burns away: grass, ferns, flowers, saplings, bushes, and a layer of snow.
fn is_plant(name: &str) -> bool {
    let Some(name) = name.strip_prefix("minecraft:") else {
        return false;
    };
    const EXACT: &[&str] = &[
        "fern",
        "large_fern",
        "dandelion",
        "poppy",
        "blue_orchid",
        "allium",
        "azure_bluet",
        "oxeye_daisy",
        "cornflower",
        "lily_of_the_valley",
        "sunflower",
        "lilac",
        "rose_bush",
        "peony",
        "dead_bush",
        "bush",
        "firefly_bush",
        "sweet_berry_bush",
        "leaf_litter",
        "wildflowers",
        "pink_petals",
        "snow",
    ];
    EXACT.contains(&name)
        || name.ends_with("_grass")
        || name.ends_with("_tulip")
        || name.ends_with("_sapling")
}

/// Nothing a blast's dust or scorching rests on: air, a plant, fire.
fn is_open(name: &str) -> bool {
    is_air(name) || is_plant(name) || name == "minecraft:fire"
}

/// What a blast chars a block of the ground into, if it does.
fn charred(name: &str) -> Option<&'static str> {
    match name.strip_prefix("minecraft:")? {
        "grass_block" | "podzol" | "mycelium" | "dirt" | "rooted_dirt" | "moss_block"
        | "dirt_path" | "farmland" => Some("minecraft:coarse_dirt"),
        "stone" => Some("minecraft:cobblestone"),
        _ => None,
    }
}

/// The ground in column (`x`, `z`), looking down from `top` to `bottom`: the height and name of
/// the first block that is not open, with open space over it. None where the column starts inside
/// the ground (a hillside) or has no ground in that span.
fn surface(
    look: &dyn Fn([i32; 3]) -> String,
    x: i32,
    z: i32,
    top: i32,
    bottom: i32,
) -> Option<(i32, String)> {
    let mut open = false;
    for y in (bottom..=top).rev() {
        let name = look([x, y, z]);
        if is_open(&name) {
            open = true;
            continue;
        }
        return open.then_some((y, name));
    }
    None
}

/// The height of the ground under a blast at `center`: the top of the first block that is not open
/// at its feet or within four blocks below. None for a blast in the air.
fn ground_under(look: &dyn Fn([i32; 3]) -> String, center: [f64; 3]) -> Option<f64> {
    let [x, y, z] = cell_of(center);
    (y - 4..=y)
        .rev()
        .find(|&y| !is_open(&look([x, y, z])))
        .map(|y| f64::from(y + 1))
}

/// How far from a blast the ground is scorched.
fn scorch_radius(power: f32) -> f64 {
    f64::from(power) + 1.5
}

/// The columns of ground round a blast to scorch, the farthest first (they are taken from the end,
/// so the scorching spreads outwards).
fn scorch_columns(center: [f64; 3], power: f32) -> Vec<[i32; 2]> {
    let radius = scorch_radius(power);
    let (cx, cz) = (center[0].floor() as i32, center[2].floor() as i32);
    let span = radius.ceil() as i32;
    let away = |column: &[i32; 2]| {
        let dx = f64::from(column[0]) + 0.5 - center[0];
        let dz = f64::from(column[1]) + 0.5 - center[2];
        dx * dx + dz * dz
    };
    let mut columns: Vec<[i32; 2]> = (cx - span..=cx + span)
        .flat_map(|x| (cz - span..=cz + span).map(move |z| [x, z]))
        .filter(|column| away(column) <= radius * radius)
        .collect();
    columns.sort_by(|a, b| away(b).total_cmp(&away(a)));
    columns
}

/// What scorching does to one column of ground.
#[derive(Debug, Default, PartialEq)]
struct Scorched {
    /// Plants to burn away.
    plants: Vec<[i32; 3]>,
    /// The block of soil to char, and into what.
    soil: Option<([i32; 3], &'static str)>,
}

/// What to scorch in column (`x`, `z`), looking down from `top` to `bottom`: the plants standing
/// on the ground, and the ground's top block if it chars.
fn scorch_target(
    look: &dyn Fn([i32; 3]) -> String,
    x: i32,
    z: i32,
    top: i32,
    bottom: i32,
) -> Scorched {
    let mut found = Scorched::default();
    let mut open = false;
    for y in (bottom..=top).rev() {
        let name = look([x, y, z]);
        if is_plant(&name) {
            found.plants.push([x, y, z]);
            open = true;
        } else if is_open(&name) {
            open = true;
        } else {
            if open {
                found.soil = charred(&name).map(|into| ([x, y, z], into));
            } else {
                // The column starts inside the ground: nothing of it faced the blast.
                found.plants.clear();
            }
            return found;
        }
    }
    found.plants.clear();
    found
}

/// Whether `name` is plain terrain or wood, the kind of block that can fly and land as rubble
/// without losing its loot or its contents. Ores, containers and anything with a block entity
/// are left to the blast.
fn is_debris(name: &str) -> bool {
    let Some(name) = name.strip_prefix("minecraft:") else {
        return false;
    };
    const EXACT: &[&str] = &[
        "dirt",
        "coarse_dirt",
        "rooted_dirt",
        "grass_block",
        "podzol",
        "mycelium",
        "mud",
        "clay",
        "sand",
        "red_sand",
        "gravel",
        "stone",
        "cobblestone",
        "mossy_cobblestone",
        "andesite",
        "diorite",
        "granite",
        "deepslate",
        "cobbled_deepslate",
        "tuff",
        "netherrack",
        "blackstone",
        "basalt",
        "sandstone",
        "red_sandstone",
        "terracotta",
        "snow_block",
        "stone_bricks",
        "bricks",
    ];
    const SUFFIXES: &[&str] = &["_planks", "_log", "_wood", "_leaves", "_terracotta"];
    EXACT.contains(&name) || SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// The velocity (blocks a tick) a block at `pos` leaves with: away from the centre, faster the
/// nearer it was, always up a little, and never the same twice (`seed` is the position's own).
fn launch_velocity(center: [f64; 3], pos: [i32; 3], power: f64, seed: u64) -> [f64; 3] {
    let d = [
        pos[0] as f64 + 0.5 - center[0],
        pos[1] as f64 + 0.5 - center[1],
        pos[2] as f64 + 0.5 - center[2],
    ];
    let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
    let dist = (flat * flat + d[1] * d[1]).sqrt().max(0.5);
    // 1 at the centre, 0 at the blast's edge.
    let near = (1.0 - dist / (power + 1.0)).clamp(0.0, 1.0);
    let jitter = |k: u64| unit(seed, k) - 0.5;
    let (dx, dz) = if flat > 1e-3 {
        (d[0] / flat, d[2] / flat)
    } else {
        // Straight above or below the centre: pick a direction at random.
        let angle = unit(seed, 9) * std::f64::consts::TAU;
        (angle.cos(), angle.sin())
    };
    let speed = 0.35 + 0.65 * near;
    let vy = 0.35 + 0.55 * near + (d[1] / dist) * 0.25 + jitter(3) * 0.2;
    [
        dx * speed + jitter(1) * 0.25,
        vy.max(0.15),
        dz * speed + jitter(2) * 0.25,
    ]
}

/// A well-mixed number from a block position and a salt (SplitMix64).
fn mix(pos: [i32; 3], salt: u64) -> u64 {
    let mut z = (pos[0] as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (pos[1] as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (pos[2] as i64 as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ salt;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A number in `[0, 1)` from a seed and an index.
fn unit(seed: u64, k: u64) -> f64 {
    let mut z = seed.wrapping_add(k.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A world of named blocks; everything else is air.
    struct World(HashMap<[i32; 3], &'static str>);

    impl World {
        /// Flat ground of `top` at y = 63 over stone, 20 blocks each way.
        fn flat(top: &'static str) -> Self {
            let mut blocks = HashMap::new();
            for x in -20..=20 {
                for z in -20..=20 {
                    blocks.insert([x, 63, z], top);
                    for y in 55..63 {
                        blocks.insert([x, y, z], "minecraft:stone");
                    }
                }
            }
            Self(blocks)
        }

        fn look(&self) -> impl Fn([i32; 3]) -> String + '_ {
            |pos| {
                self.0
                    .get(&pos)
                    .map_or("minecraft:air", |name| *name)
                    .to_owned()
            }
        }
    }

    /// One TNT's blast at (0.5, 64, 0.5), just gone off.
    fn test_blast() -> Blast {
        Blast {
            id: 1,
            dimension: String::new(),
            center: [0.5, 64.0, 0.5],
            ground: None,
            born: Instant::now(),
            ticks: 0,
            power: 4.0,
            yield_tnt: 1.0,
            size: BASE_SIZE,
            big: false,
            carved: 0.0,
            scorched: 0.0,
            lifted: 0.0,
            hit: HashSet::new(),
            pushed: false,
            seed: 0.0,
            trails: Vec::new(),
            scorch: Vec::new(),
            heard: false,
        }
    }

    fn shown(x: f64, age: f32, size: f32) -> Shown {
        Shown {
            origin: [x, 64.0, 0.0],
            age,
            size,
            seed: 0.5,
            grounded: true,
        }
    }

    #[test]
    fn only_plain_terrain_and_wood_flies() {
        for name in [
            "minecraft:dirt",
            "minecraft:grass_block",
            "minecraft:stone",
            "minecraft:oak_log",
            "minecraft:spruce_planks",
            "minecraft:oak_leaves",
            "minecraft:white_terracotta",
            "minecraft:cobbled_deepslate",
        ] {
            assert!(is_debris(name), "{name} should fly");
        }
        for name in [
            "minecraft:chest",
            "minecraft:diamond_ore",
            "minecraft:deepslate_iron_ore",
            "minecraft:furnace",
            "minecraft:obsidian",
            "minecraft:bedrock",
            "minecraft:air",
            "minecraft:tnt",
            "somemod:stone",
        ] {
            assert!(!is_debris(name), "{name} should be left to the blast");
        }
    }

    #[test]
    fn a_launch_is_finite_upward_and_within_horizons_speed_limit() {
        let center = [10.5, 64.0, 10.5];
        for x in -6..=6 {
            for y in -4..=4 {
                for z in -6..=6 {
                    let pos = [10 + x, 64 + y, 10 + z];
                    let v = launch_velocity(center, pos, 4.0, mix(pos, 0));
                    assert!(
                        v.iter().all(|c| c.is_finite() && c.abs() <= 3.9),
                        "{pos:?} {v:?}"
                    );
                    assert!(v[1] >= 0.15, "{pos:?} {v:?}");
                }
            }
        }
    }

    #[test]
    fn debris_flies_away_from_the_centre_and_faster_when_nearer() {
        let center = [0.5, 0.0, 0.5];
        let near = launch_velocity(center, [1, 0, 0], 4.0, 7);
        let far = launch_velocity(center, [4, 0, 0], 4.0, 7);
        assert!(near[0] > 0.0 && far[0] > 0.0);
        assert!(near[0] > far[0]);
        // Straight above the centre it still gets a sideways direction.
        let up = launch_velocity(center, [0, 3, 0], 4.0, 11);
        assert!(up.iter().all(|c| c.is_finite()) && up[1] > 0.0);
    }

    #[test]
    fn the_unit_stays_in_range_and_positions_mix() {
        for k in 0..1000 {
            assert!((0.0..1.0).contains(&unit(k, k * 3)));
        }
        assert_ne!(mix([1, 2, 3], 0), mix([1, 2, 4], 0));
        assert_ne!(mix([1, 2, 3], 0), mix([3, 2, 1], 0));
    }

    #[test]
    fn reach_is_the_farthest_block_and_at_least_two() {
        assert_eq!(reach([0.5, 0.5, 0.5], &[]), 2.0);
        let r = reach([0.5, 0.5, 0.5], &[[0, 0, 0], [3, 0, 0]]);
        assert!((r - 3.0).abs() < 1e-5);
    }

    #[test]
    fn yields_add_up_and_everything_grows_with_their_cube_root_without_limit() {
        // One TNT, whatever it stands in.
        assert_eq!(yield_of(4.0), 1.0);
        assert!((visual_size(1.0) - BASE_SIZE).abs() < 1e-6);
        // A creeper is smaller, an end crystal and a bed bigger.
        assert!(yield_of(3.0) < 0.5);
        assert!(yield_of(6.0) > 3.0);
        assert!(yield_of(5.0) > 1.5);
        // 1000 TNT: ten times the size, the crater and the reach; no limit past it.
        assert!((visual_size(1000.0) - BASE_SIZE * 10.0).abs() < 1e-3);
        assert!((crater_radius(1000.0) - CRATER * 10.0).abs() < 1e-9);
        assert!((shock_range(visual_size(1000.0)) - shock_range(BASE_SIZE) * 10.0).abs() < 1e-2);
        assert!(visual_size(1.0e9) > 4000.0);
        // A bigger blast lives longer and its front runs faster, by the square root of its size.
        assert_eq!(time_scale(BASE_SIZE), 1.0);
        assert_eq!(time_scale(1.0), 1.0);
        assert!((time_scale(visual_size(1000.0)) - 10.0_f32.sqrt()).abs() < 1e-4);
        assert!(shock_speed(visual_size(1000.0)) > SHOCK_SPEED * 3.0);
        assert!(life_ticks(visual_size(1000.0)) > 700);
        assert_eq!(life_ticks(BASE_SIZE), 240);
    }

    #[test]
    fn an_explosion_takes_in_the_tnt_mass_it_touches_and_nothing_apart_from_it() {
        let mut world = World(HashMap::new());
        // A 3x3x3 cube of TNT beside the blast, and another cube two blocks of air away from it.
        for x in 1..=3 {
            for y in 64..=66 {
                for z in 0..=2 {
                    world.0.insert([x, y, z], "minecraft:tnt");
                    world.0.insert([x + 6, y, z], "minecraft:tnt");
                }
            }
        }
        let look = world.look();
        let found = tnt_cluster(&look, [0, 64, 0], 1000);
        assert_eq!(found.len(), 27);
        assert!(found.iter().all(|p| p[0] <= 3));
        let unique: HashSet<_> = found.iter().collect();
        assert_eq!(unique.len(), 27);
        // A cap stops it.
        assert_eq!(tnt_cluster(&look, [0, 64, 0], 10).len(), 10);
        // TNT laid in a grid with a block of space between is still one mass.
        let mut grid = World(HashMap::new());
        for x in 0..5 {
            for z in 0..5 {
                grid.0.insert([2 + x * 2, 64, z * 2], "minecraft:tnt");
            }
        }
        assert_eq!(tnt_cluster(&grid.look(), [0, 64, 0], 1000).len(), 25);
        // Nothing near: nothing taken.
        assert!(tnt_cluster(&look, [100, 64, 100], 1000).is_empty());
    }

    #[test]
    fn a_bigger_mass_of_tnt_burns_longer_before_it_goes_off() {
        // One TNT keeps vanilla's four seconds.
        assert_eq!(fuse_ticks(1.0), FUSE_TICKS);
        assert_eq!(fuse_ticks(0.4), FUSE_TICKS);
        // Then the time to fly out of the shockwave's reach at ESCAPE_SPEED.
        let reach = |n: f32| shock_range(visual_size(n));
        for n in [2.0, 27.0, 512.0, 4096.0] {
            let extra = (fuse_ticks(n) - FUSE_TICKS) as f32 / 20.0;
            let wanted = (reach(n) - reach(1.0)) / ESCAPE_SPEED;
            assert!((extra - wanted).abs() < 0.05, "{n}: {extra} s, not {wanted} s");
        }
        assert!(fuse_ticks(27.0) > 140 && fuse_ticks(27.0) < 160);
        assert!(fuse_ticks(512.0) > 20 * 15 && fuse_ticks(512.0) < 20 * 18);
        // Always longer the bigger, up to what a fuse can hold.
        let mut last = 0;
        for k in 0..40 {
            let ticks = fuse_ticks(2.0_f32.powi(k));
            assert!(ticks >= last);
            last = ticks;
        }
        assert_eq!(fuse_ticks(1.0e15), i32::from(i16::MAX));
        assert_eq!(countdown_text(512, 14), "512 TNT: explodes in 14 s, reach 144 blocks");
        assert_eq!(countdown_text(25_087, 51), "25,087 TNT: explodes in 51 s, reach 527 blocks");
        assert_eq!(
            countdown_text(1_000_000, 180),
            "1,000,000 TNT: explodes in 180 s, reach 1,800 blocks"
        );
        for (n, text) in [(0, "0"), (999, "999"), (1000, "1,000"), (25_087, "25,087"), (-4096, "-4,096")] {
            assert_eq!(grouped(n), text);
        }
    }

    #[test]
    fn a_lit_block_counts_the_mass_round_it_and_itself() {
        let mut world = World(HashMap::new());
        for x in 0..3 {
            for z in 0..3 {
                world.0.insert([x, 64, z], "minecraft:tnt");
            }
        }
        // Lit with flint and steel: still in the world.
        assert_eq!(tnt_mass(&world.look(), [1, 64, 1]), 9);
        // Lit by redstone: taken out before the mods are asked.
        world.0.remove(&[1, 64, 1]);
        assert_eq!(tnt_mass(&world.look(), [1, 64, 1]), 9);
        // On its own.
        let alone = World(HashMap::from([([40, 64, 40], "minecraft:tnt")]));
        assert_eq!(tnt_mass(&alone.look(), [40, 64, 40]), 1);
        assert_eq!(tnt_mass(&World(HashMap::new()).look(), [40, 64, 40]), 1);
    }

    #[test]
    fn a_big_blast_tears_up_the_land_far_past_its_crater() {
        // Where the shockwave still pushes half a block a tick: well past the crater, and farther
        // the bigger the blast; a small one tears up nothing past its own crater's scorch.
        let reach = |y: f32| lift_reach(y, visual_size(y));
        let big = reach(20_000.0);
        assert!(big > crater_radius(20_000.0) * 2.0, "{big}");
        assert!((300.0..400.0).contains(&big), "{big}");
        assert!(reach(1000.0) < big && reach(1000.0) > crater_radius(1000.0));
        // Its edge is where the push is LIFT_PUSH.
        let (_, push) = shock_effect(big - 0.01, 20_000.0, visual_size(20_000.0)).unwrap();
        assert!((push - LIFT_PUSH).abs() < 0.01, "{push}");
    }

    #[test]
    fn the_shockwave_knocks_down_what_its_push_can_and_leaves_rock_and_water() {
        use Cover::*;
        let kind = |name: &str| cover_of(&format!("minecraft:{name}"));
        for fragile in ["oak_leaves", "short_grass", "red_tulip", "glass_pane", "white_wool", "torch", "ice"] {
            assert_eq!(kind(fragile), Breaks(FRAGILE), "{fragile}");
        }
        for wood in ["stripped_spruce_log", "oak_planks", "dark_oak_stairs", "spruce_door", "birch_fence", "crafting_table"] {
            assert_eq!(kind(wood), Breaks(WOOD), "{wood}");
        }
        for soil in ["grass_block", "sand", "dirt_path"] {
            assert_eq!(kind(soil), Breaks(SOIL), "{soil}");
        }
        for masonry in ["bricks", "stone_brick_stairs", "cobblestone", "white_concrete", "orange_terracotta", "cobblestone_wall", "smooth_stone_slab"] {
            assert_eq!(kind(masonry), Breaks(MASONRY), "{masonry}");
        }
        for hard in ["stone", "deepslate", "iron_ore", "obsidian", "iron_block", "chest", "oak_sign", "red_bed", "white_banner", "furnace"] {
            assert_eq!(kind(hard), Hard, "{hard}");
        }
        assert_eq!(kind("water"), Fluid);
        assert_eq!(kind("cave_air"), Air);
        assert_eq!(cover_of("other:oak_leaves"), Hard);
        assert!(BREAKS_AT.windows(2).all(|w| w[0] < w[1]));

        let (f, w, s, m) = (Breaks(FRAGILE), Breaks(WOOD), Breaks(SOIL), Breaks(MASONRY));
        // A tree on grass, its top at y 73: dirt, the turf, a trunk, air in the canopy, leaves.
        let tree = [s, s, w, w, w, Air, f, f];
        // A weak push strips its leaves, a stronger one the trunk, a hard one the turf too.
        assert_eq!(torn_off(&tree, 74, 0.6), Some((72, 73)));
        assert_eq!(torn_off(&tree, 74, 1.0), Some((68, 73)));
        assert_eq!(torn_off(&tree, 74, 1.6), Some((67, 73)));
        assert_eq!(torn_off(&tree, 74, 0.4), None);
        // A brick house with a wooden roof: the roof goes first, the walls only where it pushes
        // hard; the turf under it is the last thing taken.
        let house = [s, s, m, m, m, w];
        assert_eq!(torn_off(&house, 70, 1.0), Some((69, 69)));
        assert_eq!(torn_off(&house, 70, 2.5), Some((65, 69)));
        // Rock, or a lake: nothing under it is touched.
        assert_eq!(torn_off(&[f, Hard, Air], 70, 3.9), None);
        assert_eq!(torn_off(&[s, Fluid, f], 70, 3.9), Some((69, 69)));
        // Doors and tall plants go, but do not fly as halves.
        assert!(!may_fly("minecraft:oak_door") && !may_fly("minecraft:tall_grass"));
        assert!(may_fly("minecraft:oak_log") && may_fly("minecraft:bricks"));
    }

    #[test]
    fn the_countdown_counts_the_seconds_the_server_runs() {
        // Twenty ticks a second: a tick is a twentieth.
        assert_eq!(countdown_secs(800, 20.0), 40);
        assert_eq!(countdown_secs(801, 20.0), 41);
        assert_eq!(countdown_secs(1, 20.0), 1);
        assert_eq!(countdown_secs(0, 20.0), 0);
        assert_eq!(countdown_secs(-5, 20.0), 0);
        // A server that falls behind takes longer over the same ticks.
        assert_eq!(countdown_secs(800, 10.0), 80);
        // A faster rate (`/tick rate 40`) shortens it; nonsense reads as twenty.
        assert_eq!(countdown_secs(800, 40.0), 20);
        assert_eq!(countdown_secs(800, f32::NAN), 40);
        assert_eq!(countdown_secs(800, 0.0), 800);
        // The pace the server keeps: its rate while a tick's work fits in 50 ms, less past it.
        assert_eq!(kept_rate(20.0, 12.0), 20.0);
        assert_eq!(kept_rate(20.0, 50.0), 20.0);
        assert_eq!(kept_rate(20.0, 100.0), 10.0);
        assert_eq!(kept_rate(40.0, 20.0), 40.0);
        assert_eq!(kept_rate(f32::NAN, 10.0), 20.0);
        assert_eq!(kept_rate(20.0, f64::NAN), 20.0);
        // As it goes off: when the shockwave gets here. 9,261 TNT reach 378 blocks at about
        // 191 blocks a second.
        assert_eq!(
            went_off_text(9261, 300.0),
            "9,261 TNT went off! The shockwave reaches you in 2 s"
        );
        assert_eq!(went_off_text(9261, 0.0), "9,261 TNT went off!");
        assert_eq!(
            went_off_text(9261, 500.0),
            "9,261 TNT went off! You are out of its reach (378 blocks)"
        );
    }

    #[test]
    fn tnt_lit_by_a_burning_mass_joins_it_and_the_burning_ones_stay_followed() {
        let cluster = [[3, 64, -2], [5, 66, 0], [4, 65, -1]];
        let (lo, hi) = bounds_of(&cluster, [4, 67, -1]);
        assert_eq!((lo, hi), ([3, 64, -2], [5, 67, 0]));
        assert!(in_box(lo, hi, [1, 64, 2], 2));
        assert!(!in_box(lo, hi, [0, 64, 0], 2));
        assert!(!in_box(lo, hi, [4, 70, -1], 2));

        // A 2 by 1 by 8 mass lit at one end, and a lone TNT inside its box but three blocks off it.
        let line: Vec<[i32; 3]> = (0..8).flat_map(|z| [[0, 64, z], [8, 64, z]]).collect();
        let mut all = vec![new_fuse("minecraft:overworld", [0, 64, 0], &line, 300)];
        all[0].left = 120;
        all[0].sweep = 0;
        // Next to a block of the mass: it joins, and the mass sweeps its box again.
        assert!(burning_mass_of(&mut all, "minecraft:overworld", [1, 64, 5]).is_some());
        assert!(burning_mass_of(&mut all, "minecraft:overworld", [10, 64, 7]).is_some());
        // In the box, but three blocks from any of it: not the same mass.
        assert!(burning_mass_of(&mut all, "minecraft:overworld", [4, 64, 3]).is_none());
        // Another dimension, or once it has gone off: no.
        assert!(burning_mass_of(&mut all, "minecraft:the_nether", [1, 64, 5]).is_none());
        all[0].gone = Some(0);
        assert!(burning_mass_of(&mut all, "minecraft:overworld", [1, 64, 5]).is_none());

        // Past MAX_FUSES one that has gone off goes first, then one holding no chunks yet; a
        // burning one holding its chunks goes last.
        let mut all = Vec::new();
        for x in 0..MAX_FUSES as i32 {
            let mut fuse = new_fuse("minecraft:overworld", [x * 100, 64, 0], &cluster, 300);
            fuse.held = true;
            all.push(fuse);
        }
        all[7].gone = Some(3);
        let (first, gone) = (all[0].id, all[7].id);
        add_fuse(&mut all, new_fuse("minecraft:overworld", [-500, 64, 0], &cluster, 300));
        assert_eq!(all.len(), MAX_FUSES);
        assert!(all.iter().all(|f| f.id != gone));
        assert_eq!(all[0].id, first);
        // Fuses have ticket owners of their own, so that one never lets go of another's chunks.
        assert_eq!(all[0].owner, fuse_owner(all[0].id));
        assert_ne!(all[0].owner, all[1].owner);
    }

    #[test]
    fn tnt_lit_next_to_a_burning_mass_joins_it() {
        let cells: HashSet<[i32; 3]> = (0..5).map(|x| [x, 64, 0]).collect();
        // The first one lit stood at the end of the line, now taken out of the world.
        assert!(same_mass(&cells, [4, 64, 0], [-1, 64, 0]));
        // Within two blocks of the line's far end.
        assert!(same_mass(&cells, [0, 64, 0], [6, 64, 0]));
        // Three blocks off is another mass.
        assert!(!same_mass(&cells, [0, 64, 0], [7, 64, 0]));
        assert!(!same_mass(&HashSet::new(), [0, 64, 0], [0, 64, 9]));
    }

    #[test]
    fn a_burning_mass_holds_its_chunks_and_its_crater() {
        // 9,261 TNT (21 a side) round block (1000, 64, -1000).
        let mut cube = Vec::new();
        for x in 990..1011 {
            for y in 64..85 {
                for z in -1010..-989 {
                    cube.push([x, y, z]);
                }
            }
        }
        let cell = [1000, 85, -1000];
        let chunks = hold_area(cell, &cube, cube.len());
        let own = [1000 >> 4, -1000 >> 4];
        // The TNT's own chunk first, and no chunk twice.
        assert_eq!(chunks[0], [own[0] - 1, own[1] - 1]);
        assert!(chunks.contains(&own));
        let unique: HashSet<_> = chunks.iter().collect();
        assert_eq!(unique.len(), chunks.len());
        // Every chunk of the mass, and out to the land its shockwave tears up (about 264 blocks and
        // a margin), as far as HOLD_CHUNKS.
        for &[x, _, z] in &cube {
            assert!(chunks.contains(&[x >> 4, z >> 4]));
        }
        assert!(chunks.contains(&[own[0] + HOLD_CHUNKS, own[1]]));
        assert!(!chunks.contains(&[own[0] + HOLD_CHUNKS + 1, own[1]]));
        // A disc, not a square.
        assert!(!chunks.contains(&[own[0] + 12, own[1] + 12]));
        // A mass too small to tear up the land holds out to its crater's scorch only (27 TNT:
        // 13.5 blocks, 1.7 times that and a margin: 3 chunks).
        let small: Vec<[i32; 3]> = cube.iter().copied().take(27).collect();
        let near = hold_area(cell, &small, small.len());
        assert!(near.contains(&[own[0] + 3, own[1]]));
        assert!(!near.contains(&[own[0] + 4, own[1]]));
        // However big, never past HOLD_CHUNKS out.
        let huge = hold_area(cell, &[cell], 50_000_000);
        assert!(huge.contains(&[own[0] + HOLD_CHUNKS, own[1]]));
        assert!(!huge.contains(&[own[0] + HOLD_CHUNKS + 1, own[1]]));
        assert!(huge.len() <= ((2 * HOLD_CHUNKS + 1) * (2 * HOLD_CHUNKS + 1)) as usize);
        // A line of TNT longer than its crater: every chunk it runs through, and one round them.
        let line: Vec<[i32; 3]> = (0..2000).map(|x| [x * 2, 64, 0]).collect();
        let along = hold_area([0, 64, 0], &line, line.len());
        assert!(along.contains(&[(3998 >> 4) + 1, 0]));
        assert!(along.contains(&[3998 >> 4, -1]));
    }

    #[test]
    fn rings_dig_every_column_of_the_crater_once() {
        let center = [10.3, 70.0, -4.6];
        let radius = 23.0;
        let mut dug = HashSet::new();
        let mut from = 0.0;
        let mut rings = 0;
        while from < radius {
            let to = capped_ring(from, radius, 300.0);
            assert!(to > from);
            let ring = ring_columns(center, from, to);
            assert!(ring.len() as f64 <= 300.0 + 2.0 * std::f64::consts::PI * to + 8.0, "{}", ring.len());
            for column in ring {
                assert!(dug.insert(column), "{column:?} dug twice");
            }
            from = to;
            rings += 1;
        }
        assert!(rings > 3);
        let whole: HashSet<_> = ring_columns(center, 0.0, radius).into_iter().collect();
        assert_eq!(dug, whole);
        for [x, z] in &whole {
            assert!(column_distance(center, *x, *z) <= radius);
        }
        // About the area of the disc.
        let area = std::f64::consts::PI * radius * radius;
        assert!((whole.len() as f64 - area).abs() < area * 0.05);
    }

    #[test]
    fn the_crater_is_a_bowl_that_spares_the_bedrock() {
        let (radius, cy) = (40.0, 64.0);
        let (low, high) = crater_span(0.0, radius, cy, 0.5, -59).unwrap();
        assert_eq!(low, 44);
        assert!(high >= 64 + 34);
        // Shallower toward the rim, nothing past it.
        let (mid_low, mid_high) = crater_span(20.0, radius, cy, 0.5, -59).unwrap();
        assert!(mid_low > low && mid_high < high);
        assert_eq!(crater_span(50.0, radius, cy, 0.5, -59), None);
        // A huge crater stops above the bedrock.
        let (deep, _) = crater_span(0.0, 1000.0, cy, 0.5, -59).unwrap();
        assert_eq!(deep, -59);
        assert_eq!(crater_floor("minecraft:overworld"), -59);
    }

    #[test]
    fn the_shockwave_hurts_and_throws_less_the_farther_and_more_the_bigger() {
        let tnt = BASE_SIZE;
        let (near_damage, near_push) = shock_effect(2.0, 1.0, tnt).unwrap();
        let (far_damage, far_push) = shock_effect(12.0, 1.0, tnt).unwrap();
        assert!(near_damage > far_damage && near_push > far_push);
        assert!(near_damage <= SHOCK_DAMAGE && near_push <= 3.9);
        assert_eq!(shock_effect(f64::from(shock_range(tnt)) + 0.1, 1.0, tnt), None);
        // 1000 TNT: deadly where one TNT only scratches.
        let big = visual_size(1000.0);
        let (big_damage, big_push) = shock_effect(12.0, 1000.0, big).unwrap();
        assert!(big_damage > 20.0 && big_damage > far_damage * 10.0, "{big_damage}");
        assert!(big_push > far_push && big_push <= 3.9);
        // Thrown away from the blast and up, within Horizon's largest push.
        let v = throw([0.0, 64.0, 0.0], [10.0, 64.0, 0.0], 3.0);
        assert!(v[0] > 0.0 && v[1] > 0.0 && v[2].abs() < 1e-9);
        let v = throw([0.0, 64.0, 0.0], [0.0, 64.0, 0.0], 100.0);
        assert!(v.iter().all(|c| c.is_finite() && c.abs() <= 3.9));
    }

    #[test]
    fn an_explosion_joins_a_young_blast_close_by_only() {
        let mut blast = test_blast();
        blast.dimension = "minecraft:overworld".to_owned();
        assert!(joins(&blast, "minecraft:overworld", [3.0, 64.0, 0.0]));
        assert!(!joins(&blast, "minecraft:the_nether", [3.0, 64.0, 0.0]));
        assert!(!joins(&blast, "minecraft:overworld", [30.0, 64.0, 0.0]));
        // Bigger blasts take in explosions farther out.
        blast.yield_tnt = 1000.0;
        assert!(joins(&blast, "minecraft:overworld", [30.0, 64.0, 0.0]));
        blast.ticks = MERGE_TICKS + 1;
        assert!(!joins(&blast, "minecraft:overworld", [3.0, 64.0, 0.0]));
    }

    #[test]
    fn the_pack_is_handed_the_three_blasts_that_matter_most() {
        // Nothing going on: every slot is zero, which the pack reads as no blast.
        assert_eq!(slots(&[]), [[0.0; 4]; 6]);
        // An old small one loses its slot to three fresh ones; a dead one is never shown.
        let all = [
            shown(1.0, 10.0, 4.0),
            shown(2.0, 0.1, 4.0),
            shown(3.0, 0.2, 9.0),
            shown(4.0, 0.3, 4.0),
            shown(5.0, 25.0, 16.0),
            shown(6.0, -1.0, 16.0),
        ];
        let out = slots(&all);
        let xs: Vec<f32> = (0..3).map(|k| out[k * 2][0]).collect();
        assert_eq!(xs, vec![3.0, 2.0, 4.0]);
        for k in 0..3 {
            assert_eq!(out[k * 2 + 1][3], 1.0, "slot {k} is on");
            assert_eq!(out[k * 2 + 1][2], 1.0, "slot {k} is on the ground");
        }
        assert_eq!(out[1], [9.0, 0.5, 1.0, 1.0]);
        assert_eq!(out[0][3], 0.2);
        // One blast alone leaves the other slots zero.
        let one = slots(&[shown(7.0, 1.0, 4.0)]);
        assert_eq!(one[0], [7.0, 64.0, 0.0, 1.0]);
        assert_eq!(&one[2..], &[[0.0; 4]; 4]);
    }

    #[test]
    fn a_blast_in_the_air_is_drawn_round_its_centre() {
        let mut blast = test_blast();
        blast.center = [0.5, 100.0, 0.5];
        blast.size = 4.0;
        // The fireball starts 0.45 sizes over the pack's origin.
        assert_eq!(blast.origin(), [0.5, 100.0 - 1.8, 0.5]);
        blast.ground = Some(64.0);
        assert_eq!(blast.origin(), [0.5, 64.0, 0.5]);
    }

    #[test]
    fn the_shake_jolts_at_once_then_peaks_when_the_shockwave_arrives() {
        let (size, dist) = (4.5, 42.0);
        // One second away: a small jolt at first, nothing much in between, the wave at 1 s (it
        // rises over 0.05 s, so its peak is just after).
        let start = shake_strength(size, dist, 0.0);
        let between = shake_strength(size, dist, 0.8);
        let arrival = shake_strength(size, dist, 1.05);
        let later = shake_strength(size, dist, 2.5);
        assert!(start > between, "{start} {between}");
        assert!(arrival > start && arrival > later, "{start} {arrival} {later}");
        assert!(later < arrival * 0.01);
        // Nearer is stronger, bigger is stronger, and it never runs away.
        assert!(shake_strength(size, 5.0, 0.2) > shake_strength(size, 30.0, 0.8));
        assert!(shake_strength(9.0, 20.0, 0.5) > shake_strength(4.5, 20.0, 0.5));
        for dist in [0.0, 1.0, 10.0, 100.0, 1000.0] {
            for age in [0.0, 0.01, 0.5, 3.0, 11.9] {
                let k = shake_strength(visual_size(1000.0), dist, age);
                assert!(k.is_finite() && (0.0..=7.6).contains(&k), "{dist} {age} {k}");
            }
        }
        // Gentle: one TNT right beside you turns the view by under 2 degrees, and from one frame
        // to the next (60 a second) by under a degree.
        let mut last = 0.0_f32;
        for frame in 0..240 {
            let age = frame as f32 / 60.0;
            let pitch = shake_strength(4.8, 3.0, age) * 0.9 * wobble(age, 5.3, 1.0);
            assert!(pitch.abs() < 2.0, "frame {frame}: {pitch}");
            assert!((pitch - last).abs() < 1.0, "frame {frame}: {last} -> {pitch}");
            last = pitch;
        }
        assert_eq!(shake_strength(size, 5.0, 12.0), 0.0);
        assert_eq!(shake_strength(size, 5.0, -0.1), 0.0);
        for i in 0..200 {
            let w = wobble(i as f32 * 0.013, 9.3, 2.0);
            assert!((-1.0..=1.0).contains(&w));
        }
    }

    #[test]
    fn only_vanillas_own_explosion_sound_is_dropped() {
        // Vanilla's pitch range, at the blast, as it goes off: dropped.
        for pitch in [0.56, 0.7, 0.84] {
            assert!(vanilla_boom_of(pitch, 0.4, 4.8, 0.05), "{pitch}");
        }
        // This mod's own boom, another blast far away, or a late one: kept.
        assert!(!vanilla_boom_of(BOOM_PITCH, 0.0, 4.8, 0.05));
        assert!(!vanilla_boom_of(0.7, 30.0, 4.8, 0.05));
        assert!(!vanilla_boom_of(0.7, 0.4, 4.8, 3.0));
        assert!(boom(10.0, 4.8)[0].pitch == BOOM_PITCH);
    }

    #[test]
    fn the_boom_is_heard_when_the_shockwave_arrives() {
        assert_eq!(sound_delay(0.0, BASE_SIZE), 0);
        assert_eq!(sound_delay(SHOCK_SPEED, BASE_SIZE), 20);
        assert_eq!(sound_delay(SHOCK_SPEED * 2.5, BASE_SIZE), 50);
        assert_eq!(sound_delay(1.0e9, BASE_SIZE), 1200);
        // A big blast's front runs faster: heard sooner from as far.
        assert!(sound_delay(200.0, visual_size(1000.0)) < sound_delay(200.0, BASE_SIZE));
        for (dist, size) in [(0.0, 4.0), (30.0, 4.5), (500.0, 16.0), (1.0e7, 16.0)] {
            let cues = boom(dist, size);
            for cue in &cues {
                // What `play_local_sound` takes: a namespaced event, and numbers in range.
                assert!(cue.sound.starts_with("minecraft:"));
                assert!(cue.volume.is_finite() && cue.volume > 0.0);
                assert!(cue.pitch.is_finite() && cue.pitch > 0.0);
                assert!((0..=1200).contains(&cue.delay), "{}", cue.delay);
            }
            // The thunder rolls after the boom.
            assert!(cues[2].delay >= cues[0].delay);
        }
    }

    #[test]
    fn the_dust_ring_races_out_then_slows() {
        let size = 4.5;
        assert!((dust_ring_radius(size, 0.0) - 2.25).abs() < 1e-9);
        let mut last = 0.0;
        let mut last_step = f64::MAX;
        for tick in 0..40 {
            let r = dust_ring_radius(size, f64::from(tick) / 20.0);
            if tick > 0 {
                let step = r - last;
                assert!(step > 0.0 && step < last_step, "tick {tick}");
                last_step = step;
            }
            last = r;
        }
        // It comes to rest under 3.9 sizes out.
        assert!(dust_ring_radius(size, 60.0) < f64::from(size) * 3.91);
    }

    #[test]
    fn a_trail_flies_as_a_falling_block_does() {
        let mut trail = Trail {
            pos: [0.0, 70.0, 0.0],
            vel: [0.5, 0.6, 0.0],
            age: 0,
        };
        trail.advance();
        assert_eq!(trail.age, 1);
        assert!((trail.pos[0] - 0.5).abs() < 1e-12 && (trail.pos[1] - 70.56).abs() < 1e-12);
        assert!((trail.vel[0] - 0.49).abs() < 1e-12 && (trail.vel[1] - 0.5488).abs() < 1e-12);
        // It goes up, comes down, and keeps going sideways more and more slowly.
        let mut top = trail.pos[1];
        for _ in 0..80 {
            trail.advance();
            top = top.max(trail.pos[1]);
        }
        assert!(top > 73.0 && trail.pos[1] < 60.0 && trail.pos[0] > 10.0);
        assert!(trail.vel[0] > 0.0 && trail.vel[0] < 0.11);
    }

    #[test]
    fn the_burst_and_the_plume_are_particles_horizon_accepts() {
        let ok = |puff: &Puff| {
            assert!(puff.count > 0);
            assert!(puff.at.iter().chain(puff.spread.iter()).all(|c| c.is_finite()));
            assert!(puff.speed.is_finite());
        };
        let first = burst([0.5, 64.0, 0.5], 4.0, 4.6);
        assert_eq!(first.len(), 4);
        assert!(first.iter().all(|p| p.kind != Particle::Flash));
        first.iter().for_each(ok);
        let base = [0.5, 64.0, 0.5];
        let mut total = 0;
        for tick in 1..=life_ticks(4.6) {
            let puffs = plume(tick, base, 4.0, 4.6);
            puffs.iter().for_each(ok);
            total += puffs.len();
            if tick > 200 {
                assert!(puffs.is_empty(), "tick {tick}");
            }
        }
        assert!(total > 150, "{total}");
        // The cloud's puffs climb with the mushroom: later ones are higher up.
        let head = |tick| {
            plume(tick, base, 4.0, 4.6)
                .iter()
                .map(|p| p.at[1])
                .fold(f64::MIN, f64::max)
        };
        assert!(head(120) > head(12) + 3.0);
    }

    #[test]
    fn block_names_are_told_apart() {
        for name in ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"] {
            assert!(is_air(name) && is_open(name));
        }
        for name in [
            "minecraft:short_grass",
            "minecraft:tall_grass",
            "minecraft:poppy",
            "minecraft:red_tulip",
            "minecraft:oak_sapling",
            "minecraft:snow",
        ] {
            assert!(is_plant(name) && is_open(name) && !is_air(name), "{name}");
        }
        for name in [
            "minecraft:grass_block",
            "minecraft:snow_block",
            "minecraft:stone",
            "minecraft:water",
            "othermod:fern",
        ] {
            assert!(!is_plant(name) && !is_open(name), "{name}");
        }
        assert!(is_open("minecraft:fire"));
        assert_eq!(charred("minecraft:grass_block"), Some("minecraft:coarse_dirt"));
        assert_eq!(charred("minecraft:stone"), Some("minecraft:cobblestone"));
        assert_eq!(charred("minecraft:sand"), None);
        assert_eq!(charred("minecraft:chest"), None);
        assert_eq!(charred("stone"), None);
    }

    #[test]
    fn the_ground_is_found_under_open_space_only() {
        let mut world = World::flat("minecraft:grass_block");
        world.0.insert([2, 64, 0], "minecraft:short_grass");
        // A hill at x = 5 that reaches over the span looked at.
        for y in 64..=70 {
            world.0.insert([5, y, 0], "minecraft:stone");
        }
        let look = world.look();
        assert_eq!(
            surface(&look, 0, 0, 67, 60),
            Some((63, "minecraft:grass_block".to_owned()))
        );
        // The plant is looked through.
        assert_eq!(
            surface(&look, 2, 0, 67, 60),
            Some((63, "minecraft:grass_block".to_owned()))
        );
        assert_eq!(surface(&look, 5, 0, 67, 60), None);
        // Nothing but air in the span.
        assert_eq!(surface(&look, 0, 0, 80, 70), None);

        // A blast on the ground stands on it, one a few blocks up still does, one high up doesn't.
        assert_eq!(ground_under(&look, [0.5, 64.06, 0.5]), Some(64.0));
        assert_eq!(ground_under(&look, [2.5, 64.5, 0.5]), Some(64.0));
        assert_eq!(ground_under(&look, [0.5, 67.9, 0.5]), Some(64.0));
        assert_eq!(ground_under(&look, [0.5, 68.0, 0.5]), None);
    }

    #[test]
    fn scorching_burns_plants_and_chars_the_soil_under_them() {
        let mut world = World::flat("minecraft:grass_block");
        world.0.insert([1, 64, 0], "minecraft:tall_grass");
        world.0.insert([1, 65, 0], "minecraft:tall_grass");
        world.0.insert([2, 63, 0], "minecraft:sand");
        world.0.insert([2, 64, 0], "minecraft:dead_bush");
        for y in 64..=70 {
            world.0.insert([3, y, 0], "minecraft:stone");
        }
        let look = world.look();
        let bare = scorch_target(&look, 0, 0, 68, 58);
        assert_eq!(bare.plants, Vec::<[i32; 3]>::new());
        assert_eq!(bare.soil, Some(([0, 63, 0], "minecraft:coarse_dirt")));
        let grown = scorch_target(&look, 1, 0, 68, 58);
        assert_eq!(grown.plants, vec![[1, 65, 0], [1, 64, 0]]);
        assert_eq!(grown.soil, Some(([1, 63, 0], "minecraft:coarse_dirt")));
        // Sand does not char, but its bush burns.
        let sandy = scorch_target(&look, 2, 0, 68, 58);
        assert_eq!(sandy.plants, vec![[2, 64, 0]]);
        assert_eq!(sandy.soil, None);
        // Inside a hill, and where there is no ground at all: nothing.
        assert_eq!(scorch_target(&look, 3, 0, 68, 58), Scorched::default());
        assert_eq!(scorch_target(&look, 0, 0, 90, 80), Scorched::default());
    }

    #[test]
    fn the_scorched_columns_are_a_disc_taken_from_the_middle_out() {
        let center = [0.5, 64.0, 0.5];
        let columns = scorch_columns(center, 4.0);
        let radius = scorch_radius(4.0);
        let away = |c: &[i32; 2]| {
            ((f64::from(c[0]) + 0.5 - center[0]).powi(2)
                + (f64::from(c[1]) + 0.5 - center[2]).powi(2))
            .sqrt()
        };
        assert!(columns.len() > 80 && columns.len() < 110, "{}", columns.len());
        assert!(columns.iter().all(|c| away(c) <= radius));
        assert!(columns.windows(2).all(|w| away(&w[0]) >= away(&w[1])));
        // The last one, the first taken, is the column of the blast itself.
        assert_eq!(columns.last(), Some(&[0, 0]));
        let mut unique = columns.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), columns.len());
    }
}
