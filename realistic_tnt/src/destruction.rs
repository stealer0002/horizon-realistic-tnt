//! What a big blast leaves behind beyond the bowl `carve` digs: a raised rim of thrown-out ground,
//! crater walls that slide in for a while after, fires the heat lights round the crater, and the
//! small craters the debris digs where it comes down.
use super::{
    Particle, Puff, Trail, Work, block_at, column_distance, crater_radius, emit, falling, is_debris, mix,
    ring_columns, state_of, states_of, tag_filters, unit,
};
use mod_api::{BlockEdit, BlockRun, Heightmap, HostBlockStates, LevelRef, ServerHost};

/// The rim stands about this much of the crater's radius high at its crest, and the blanket of
/// thrown-out ground thins with the cube of the distance past it, out to `RIM_REACH` radii.
const RIM_HEIGHT: f64 = 0.15;
pub(super) const RIM_REACH: f64 = 1.8;
/// How deep under the surface the rim looks for the ground (past tree trunks).
const RIM_DEPTH: u32 = 24;
/// Smaller blasts light no fires.
pub(super) const FIRE_YIELD: f32 = 16.0;
/// Fires are lit out to this many crater radii at least, and past where the shockwave knocks the
/// trees down (`fire_reach`): inside that the land is swept bare, so fire is lit where the forest
/// still stands. Each column catches with `FIRE_CHANCE`.
const FIRE_REACH: f64 = 2.5;
const FIRE_CHANCE: f64 = 0.04;
/// Columns of the crater's wall tried each tick while it slides in.
const SLUMP_TRIES: usize = 8;
/// A blast keeps at most this many debris impacts waiting.
pub(super) const MAX_IMPACTS: usize = 400;
/// Debris thrown slower than this (blocks a tick) at landing digs nothing.
const IMPACT_SPEED: f64 = 0.6;

/// Where a flying block comes down, and how big a hole it digs there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Impact {
    /// The blast's tick it lands on.
    pub due: u32,
    pub at: [f64; 3],
    /// The hole's radius, in blocks.
    pub radius: f64,
}

/// How many ticks the walls of a crater of `radius` slide in for once it is dug.
pub(super) fn slump_ticks(radius: f64) -> u32 {
    (60.0 + radius * 1.5).min(600.0) as u32
}

/// Raises one ring of the rim: thrown-out ground piled on the land round the crater, highest just
/// past its edge, as a real crater's lip is.
pub(super) fn rim(host: &mut dyn ServerHost, dimension: &str, job: &Work, from: f64, to: f64) {
    let radius = crater_radius(job.yield_tnt);
    let peak = radius * RIM_HEIGHT;
    let mut piles = Vec::new();
    for [x, z] in ring_columns(job.center, from, to) {
        let r = column_distance(job.center, x, z);
        let seed = mix([x, 21, z], job.id);
        let height = (rim_height(r, radius, peak) * (0.6 + 0.8 * unit(seed, 0))).round() as u32;
        if height > 0 {
            piles.push(((x, z), height, seed));
        }
    }
    if piles.is_empty() {
        return;
    }
    // The ground under each pile: the highest block that is not a tree or a plant, so the pile buries
    // the foot of a trunk instead of standing on top of it.
    let columns: Vec<(i32, i32)> = piles.iter().map(|p| p.0).collect();
    let Ok(read) = host.read_columns(dimension, &columns, Heightmap::MotionBlockingNoLeaves, (RIM_DEPTH, 0)) else {
        return;
    };
    let depth = RIM_DEPTH as usize;
    if read.blocks.len() < columns.len() * depth || read.bases.len() < columns.len() {
        return;
    }
    let standing: Vec<bool> = read
        .palette
        .iter()
        .map(|&state| stands_on_ground(&super::name_of(&*host, state)))
        .collect();
    let names = ["minecraft:coarse_dirt", "minecraft:dirt", "minecraft:gravel", "minecraft:cobblestone"];
    let tags = ["minecraft:air", "minecraft:logs", "minecraft:leaves", "minecraft:replaceable"];
    let (Some(palette), Some(filters)) = (states_of(&*host, &names), tag_filters(&*host, &tags)) else {
        return;
    };
    let mut edit = BlockEdit {
        palette,
        filters,
        ..Default::default()
    };
    for (i, &((x, z), height, seed)) in piles.iter().enumerate() {
        let Some(base) = read.bases[i] else {
            continue;
        };
        let blocks = &read.blocks[i * depth..(i + 1) * depth];
        // From the top down: the first block that is ground.
        let Some(below) = (0..depth)
            .rev()
            .find(|&k| !standing.get(usize::from(blocks[k])).copied().unwrap_or(false))
        else {
            continue;
        };
        let ground = base - depth as i32 + below as i32;
        let fill = 1 + (unit(seed, 1) * 3.0) as u32 % 3;
        for only in 0..tags.len() as u32 {
            if height > 1 {
                edit.runs.push(BlockRun {
                    x,
                    z,
                    y: ground + 1,
                    height: height - 1,
                    surface: None,
                    block: fill,
                    only: Some(only),
                });
            }
            edit.runs.push(BlockRun {
                x,
                z,
                y: ground + height as i32,
                height: 1,
                surface: None,
                block: 0,
                only: Some(only),
            });
        }
    }
    if !edit.runs.is_empty() {
        match host.edit_blocks(dimension, &edit) {
            Ok(changed) => debug(format_args!("rim {from:.0}..{to:.0}: {changed} blocks piled")),
            Err(error) => eprintln!("[realistic_tnt] rim: block edit failed in {dimension}: {error}"),
        }
    }
}

/// Whether a block named `name` stands on the ground rather than being it: air, trees, plants.
fn stands_on_ground(name: &str) -> bool {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    matches!(name, "air" | "cave_air" | "short_grass" | "tall_grass" | "fern" | "large_fern" | "snow" | "vine")
        || name.ends_with("_log")
        || name.ends_with("_wood")
        || name.ends_with("_stem")
        || name.ends_with("_leaves")
        || name.ends_with("_sapling")
        || name.ends_with("_flower")
        || name.ends_with("_tulip")
}

/// How high the rim stands `r` blocks out from a crater of `radius` whose crest is `peak` high.
fn rim_height(r: f64, radius: f64, peak: f64) -> f64 {
    let crest = radius * 1.05;
    if r < radius * 0.95 || r > radius * RIM_REACH {
        0.0
    } else if r <= crest {
        peak
    } else {
        peak * (crest / r).powi(3)
    }
}

/// Lets a few columns of the crater's wall slide in: where the ground stands well above the ground
/// just inside it, the top block breaks off and slides down into the crater.
pub(super) fn slump(host: &mut dyn ServerHost, level: LevelRef, dimension: &str, job: &Work, tick: u32) {
    let radius = crater_radius(job.yield_tnt);
    let (cx, cz) = (job.center[0], job.center[2]);
    let mut spots = Vec::with_capacity(SLUMP_TRIES);
    let mut columns = Vec::with_capacity(SLUMP_TRIES * 2);
    for k in 0..SLUMP_TRIES {
        let seed = mix([tick as i32, k as i32, 31], job.id);
        let angle = unit(seed, 0) * std::f64::consts::TAU;
        let r = radius * (0.75 + 0.4 * unit(seed, 1));
        let (dx, dz) = (angle.cos(), angle.sin());
        let at = ((cx + dx * r).floor() as i32, (cz + dz * r).floor() as i32);
        let inner = ((cx + dx * (r - 2.0)).floor() as i32, (cz + dz * (r - 2.0)).floor() as i32);
        spots.push((at, [dx, dz]));
        columns.push(at);
        columns.push(inner);
    }
    let Ok(tops) = host.surface_heights(dimension, &columns, Heightmap::MotionBlockingNoLeaves) else {
        return;
    };
    let Some(air) = state_of(&*host, "minecraft:air") else {
        return;
    };
    let mut slid = 0;
    for (k, &((x, z), [dx, dz])) in spots.iter().enumerate() {
        let (Some(Some(top)), Some(Some(inner))) = (tops.get(2 * k), tops.get(2 * k + 1)) else {
            continue;
        };
        if top - inner < 3 {
            continue;
        }
        // The top block (the heightmap is the first air above it) and the one under it.
        for y in [top - 1, top - 2] {
            let name = block_at(&*host, level, [x, y, z]);
            if !is_debris(&name) {
                break;
            }
            // The wall was baked with the bowl: it slides in as it was baked.
            let Some(state) = baked(&name).and_then(|name| state_of(&*host, &name)) else {
                break;
            };
            if !super::set_block(host, level, [x, y, z], air) {
                break;
            }
            let seed = mix([x, y, z], job.id ^ 0x51);
            let push = 0.12 + 0.15 * unit(seed, 0);
            let at = [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5];
            host.spawn_falling_block(dimension, state, at, [-dx * push, 0.05, -dz * push], falling(false, true));
            slid += 1;
        }
        let at = [f64::from(x) + 0.5, f64::from(*top), f64::from(z) + 0.5];
        emit(host, level, Puff::new(Particle::Poof, at, [0.6, 0.3, 0.6], 0.02, 3));
    }
    if slid > 0 {
        debug(format_args!("slump: {slid} blocks slid"));
    }
}

/// Lights one ring of fires round the crater: the flash sets the tops of trees, roofs and grass
/// alight, and the game's fire spreads from there. Each fire is set as `/setblock` sets it, so the
/// game ticks it (a fire written straight into the chunk would never spread nor go out).
pub(super) fn ignite(host: &mut dyn ServerHost, level: LevelRef, dimension: &str, job: &Work, from: f64, to: f64) {
    let reach = fire_reach(job.yield_tnt, job.size);
    let columns: Vec<(i32, i32)> = ring_columns(job.center, from, to)
        .into_iter()
        .filter(|&[x, z]| column_distance(job.center, x, z) <= reach && unit(mix([x, 41, z], job.id), 0) < FIRE_CHANCE)
        .map(|[x, z]| (x, z))
        .collect();
    if columns.is_empty() {
        return;
    }
    // The first air over whatever stands highest (grass, crops, tree crowns).
    let Ok(tops) = host.surface_heights(dimension, &columns, Heightmap::WorldSurface) else {
        return;
    };
    let Some(fire) = state_of(&*host, "minecraft:fire") else {
        return;
    };
    let mut lit = 0;
    for (&(x, z), top) in columns.iter().zip(&tops) {
        let Some(y) = *top else {
            continue;
        };
        if block_at(&*host, level, [x, y, z]) == "minecraft:air" && super::set_block(host, level, [x, y, z], fire) {
            lit += 1;
        }
    }
    debug(format_args!("ignite {from:.0}..{to:.0}: {} tried, {lit} lit", columns.len()));
}

/// What the heat of a blast turns each material into, `#` naming a block tag. In this order: a
/// result is never turned again by a later row (tuff from gravel comes after the stone row).
const HEAT: &[(&str, &str)] = &[
    ("#minecraft:sand", "minecraft:glass"),
    ("minecraft:sandstone", "minecraft:glass"),
    ("minecraft:red_sandstone", "minecraft:glass"),
    ("#minecraft:base_stone_overworld", "minecraft:blackstone"),
    ("minecraft:cobblestone", "minecraft:blackstone"),
    ("minecraft:mossy_cobblestone", "minecraft:blackstone"),
    ("minecraft:cobbled_deepslate", "minecraft:blackstone"),
    ("minecraft:stone_bricks", "minecraft:cracked_stone_bricks"),
    ("#minecraft:dirt", "minecraft:coarse_dirt"),
    ("minecraft:clay", "minecraft:terracotta"),
    ("minecraft:mud", "minecraft:packed_mud"),
    ("minecraft:gravel", "minecraft:tuff"),
    ("#minecraft:snow", "minecraft:air"),
    ("#minecraft:ice", "minecraft:air"),
    ("#minecraft:leaves", "minecraft:air"),
    ("minecraft:netherrack", "minecraft:magma_block"),
];

/// What a block named `name` (`minecraft:sand`) becomes in the heat, by name, for the ground the
/// crater throws out and its walls shed: as `HEAT` bakes the ground, `None` where it is gone (snow,
/// ice, leaves), and the block itself where the heat leaves it be.
pub(super) fn baked(name: &str) -> Option<String> {
    let bare = name.strip_prefix("minecraft:").unwrap_or(name);
    let into = match bare {
        "sand" | "red_sand" | "suspicious_sand" | "sandstone" | "red_sandstone" => "glass",
        "stone" | "granite" | "diorite" | "andesite" | "tuff" | "deepslate" | "calcite" | "cobblestone"
        | "mossy_cobblestone" | "cobbled_deepslate" => "blackstone",
        "stone_bricks" => "cracked_stone_bricks",
        "dirt" | "grass_block" | "podzol" | "mycelium" | "rooted_dirt" | "coarse_dirt" | "moss_block" => {
            "coarse_dirt"
        }
        "clay" => "terracotta",
        "mud" => "packed_mud",
        "gravel" => "tuff",
        "netherrack" => "magma_block",
        "snow" | "snow_block" | "powder_snow" | "ice" | "packed_ice" | "blue_ice" => return None,
        _ if bare.ends_with("_leaves") => return None,
        _ => return Some(name.to_owned()),
    };
    Some(format!("minecraft:{into}"))
}

/// A bulk edit that bakes ground in the heat of a blast (`HEAT`): sand to glass, stone to
/// blackstone, soil to coarse dirt, clay to terracotta, snow and ice to steam.
pub(super) struct Heat {
    edit: BlockEdit,
    /// (filter, palette entry) of each row of `HEAT` this game has.
    turns: Vec<(u32, u32)>,
}

impl Heat {
    pub(super) fn new(host: &dyn mod_api::Host) -> Option<Self> {
        let states = host.block_states();
        let mut edit = BlockEdit::default();
        let mut turns = Vec::new();
        for &(what, into) in HEAT {
            let filter = match what.strip_prefix('#') {
                Some(tag) => {
                    let id = mod_api::Identifier::parse(tag).ok()?;
                    let key = host.tags().key(&id)?;
                    mod_api::StatePredicate::tag(&states, host.tags(), key, &[]).ok()
                }
                None => host
                    .block_key(what)
                    .and_then(|key| mod_api::StatePredicate::block(&states, key, &[]).ok()),
            };
            // A block this game doesn't have is skipped; so is a row whose result it doesn't have.
            let (Some(filter), Some(state)) = (filter, state_of(host, into)) else {
                continue;
            };
            let block = match edit.palette.iter().position(|&s| s == state) {
                Some(i) => i as u32,
                None => {
                    edit.palette.push(state);
                    edit.palette.len() as u32 - 1
                }
            };
            edit.filters.push(filter);
            turns.push((edit.filters.len() as u32 - 1, block));
        }
        Some(Self { edit, turns })
    }

    /// Bakes `height` blocks of column (`x`, `z`) up from `y`.
    pub(super) fn bake(&mut self, x: i32, z: i32, y: i32, height: u32) {
        for &(only, block) in &self.turns {
            self.edit.runs.push(BlockRun {
                x,
                z,
                y,
                height,
                surface: None,
                block,
                only: Some(only),
            });
        }
    }

    pub(super) fn apply(self, host: &mut dyn ServerHost, dimension: &str) {
        if self.edit.runs.is_empty() {
            return;
        }
        match host.edit_blocks(dimension, &self.edit) {
            Ok(changed) => debug(format_args!("heat: {changed} blocks baked")),
            Err(error) => eprintln!("[realistic_tnt] heat: block edit failed in {dimension}: {error}"),
        }
    }
}

/// How far out a blast of `yield_tnt` and `size` lights fires: 2.5 crater radii, or a third past
/// where its shockwave knocks trees and wooden houses down, whichever is farther.
pub(super) fn fire_reach(yield_tnt: f32, size: f32) -> f64 {
    let wood = super::push_reach(yield_tnt, size, super::config::get().breaks_at[usize::from(super::WOOD)]);
    (crater_radius(yield_tnt) * FIRE_REACH).max(wood * 1.3)
}

/// Where a block thrown from `at` with `velocity` on the blast's tick `tick` comes down, flying as
/// the game flies a falling block, onto ground about as high as `ground`: none when it lands too
/// softly to dig, inside the crater, or not within 15 seconds.
pub(super) fn predict(at: [f64; 3], velocity: [f64; 3], ground: f64, tick: u32, job: &Work) -> Option<Impact> {
    let mut flight = Trail {
        pos: at,
        vel: velocity,
        age: 0,
    };
    while flight.age < 300 {
        flight.advance();
        if flight.vel[1] < 0.0 && flight.pos[1] <= ground {
            let speed = flight.vel.iter().map(|v| v * v).sum::<f64>().sqrt();
            let r = ((flight.pos[0] - job.center[0]).powi(2) + (flight.pos[2] - job.center[2]).powi(2)).sqrt();
            if speed < IMPACT_SPEED || r < crater_radius(job.yield_tnt) * 1.1 {
                return None;
            }
            return Some(Impact {
                // A tick early, so the block comes down into its own hole.
                due: tick + flight.age.saturating_sub(1),
                at: flight.pos,
                radius: (1.0 + speed * 0.8).min(3.0),
            });
        }
    }
    None
}

/// Digs the small craters of the debris coming down this tick: a little bowl of natural ground,
/// with a puff of dust.
pub(super) fn impacts(host: &mut dyn ServerHost, level: LevelRef, dimension: &str, due: &[Impact]) {
    if due.is_empty() {
        return;
    }
    let spots: Vec<(i32, i32)> = due.iter().map(|i| (i.at[0].floor() as i32, i.at[2].floor() as i32)).collect();
    let Ok(tops) = host.surface_heights(dimension, &spots, Heightmap::MotionBlockingNoLeaves) else {
        return;
    };
    let (Some(palette), Some(filters)) = (
        states_of(&*host, &["minecraft:air"]),
        tag_filters(&*host, &["minecraft:dirt", "minecraft:sand", "minecraft:base_stone_overworld"]),
    ) else {
        return;
    };
    let mut edit = BlockEdit {
        palette,
        filters,
        ..Default::default()
    };
    for (impact, top) in due.iter().zip(&tops) {
        let Some(top) = *top else {
            continue;
        };
        let reach = impact.radius.ceil() as i32;
        let (ix, iz) = (impact.at[0].floor() as i32, impact.at[2].floor() as i32);
        for x in ix - reach..=ix + reach {
            for z in iz - reach..=iz + reach {
                let d = column_distance(impact.at, x, z) / impact.radius;
                if d >= 1.0 {
                    continue;
                }
                let depth = (impact.radius * 0.8 * (1.0 - d * d)).round() as i32;
                if depth <= 0 {
                    continue;
                }
                for only in 0..3 {
                    edit.runs.push(BlockRun {
                        x,
                        z,
                        y: top - depth,
                        height: depth as u32,
                        surface: None,
                        block: 0,
                        only: Some(only),
                    });
                }
            }
        }
        let at = [impact.at[0], f64::from(top), impact.at[2]];
        let spread = impact.radius * 0.6;
        emit(host, level, Puff::new(Particle::LargeSmoke, at, [spread, 0.4, spread], 0.03, 4));
        emit(host, level, Puff::new(Particle::Poof, at, [spread, 0.3, spread], 0.08, 6));
    }
    if !edit.runs.is_empty() {
        match host.edit_blocks(dimension, &edit) {
            Ok(changed) => debug(format_args!("impacts: {} came down, {changed} blocks dug", due.len())),
            Err(error) => eprintln!("[realistic_tnt] impacts: block edit failed in {dimension}: {error}"),
        }
    }
}

/// A line on stderr when `REALISTIC_TNT_DEBUG` is set: what the destruction did, to check it.
fn debug(what: std::fmt::Arguments<'_>) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("REALISTIC_TNT_DEBUG").is_some()) {
        eprintln!("[realistic_tnt] {what}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rim_is_highest_at_the_crest_and_thins_out() {
        let radius = 40.0;
        let peak = radius * RIM_HEIGHT;
        assert_eq!(rim_height(radius * 0.5, radius, peak), 0.0);
        assert_eq!(rim_height(radius, radius, peak), peak);
        let near = rim_height(radius * 1.2, radius, peak);
        let far = rim_height(radius * 1.6, radius, peak);
        assert!(peak > near && near > far && far > 0.0);
        assert_eq!(rim_height(radius * 2.0, radius, peak), 0.0);
    }

    #[test]
    fn trees_and_plants_stand_on_the_ground() {
        for name in ["minecraft:oak_log", "minecraft:birch_leaves", "minecraft:short_grass", "minecraft:air"] {
            assert!(stands_on_ground(name), "{name}");
        }
        for name in ["minecraft:grass_block", "minecraft:stone", "minecraft:dirt", "minecraft:gravel"] {
            assert!(!stands_on_ground(name), "{name}");
        }
    }

    #[test]
    fn no_heat_result_is_turned_again_by_a_later_row() {
        // Each result, by name, must not be an input of a row after its own (tags aside, which
        // the game checks: blackstone, glass and the rest are in none of the tags used).
        for (i, &(_, into)) in HEAT.iter().enumerate() {
            assert!(HEAT[i + 1..].iter().all(|&(what, _)| what != into), "{into}");
        }
    }

    #[test]
    fn the_heat_bakes_thrown_ground_as_it_bakes_the_bowl() {
        assert_eq!(baked("minecraft:sand").as_deref(), Some("minecraft:glass"));
        assert_eq!(baked("minecraft:stone").as_deref(), Some("minecraft:blackstone"));
        assert_eq!(baked("minecraft:grass_block").as_deref(), Some("minecraft:coarse_dirt"));
        assert_eq!(baked("minecraft:clay").as_deref(), Some("minecraft:terracotta"));
        assert_eq!(baked("minecraft:oak_leaves"), None);
        assert_eq!(baked("minecraft:oak_log").as_deref(), Some("minecraft:oak_log"));
    }

    #[test]
    fn slumping_lasts_longer_for_a_bigger_crater_within_bounds() {
        assert_eq!(slump_ticks(0.0), 60);
        assert!(slump_ticks(100.0) > slump_ticks(10.0));
        assert_eq!(slump_ticks(10_000.0), 600);
    }
}
