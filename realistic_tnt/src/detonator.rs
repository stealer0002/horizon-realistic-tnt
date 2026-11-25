//! Remote detonator item (`realistic_tnt:detonator`): links TNT blocks onto the item's
//! `minecraft:custom_data` component, keyed by dimension, and ignites all linked TNT blocks at
//! once with a fixed 5-second (100-tick) fuse when used in the air.

use mod_api::{
    ClientHost, Component, Compound, HostSide, InteractionResult, ItemPart, ItemStack,
    SoundSource, Tag, Text, TooltipEvent, UseItemEvent, UseOnEvent,
};
use std::collections::BTreeMap;

pub(super) const DETONATOR_ID: &str = "realistic_tnt:detonator";
pub(super) const MAX_LINKS: usize = 64;
pub(super) const DETONATOR_FUSE_TICKS: i32 = 100;
const LINKS_KEY: &str = "realistic_tnt_links";

struct CustomDataComponent;

impl Component for CustomDataComponent {
    type Value = Compound;

    fn id(&self) -> &str {
        "minecraft:custom_data"
    }

    fn decode(&self, value: &Tag) -> Option<Self::Value> {
        match value {
            Tag::Compound(data) => Some(data.clone()),
            _ => None,
        }
    }

    fn encode(&self, value: Self::Value) -> Tag {
        Tag::Compound(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ToggleResult {
    Linked { total: usize },
    Unlinked { total: usize },
    Full { max: usize },
}

/// TNT block positions stored on a detonator item, grouped by dimension name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DetonatorLinks {
    by_dim: BTreeMap<String, Vec<[i32; 3]>>,
}

impl DetonatorLinks {
    pub(super) fn total(&self) -> usize {
        self.by_dim.values().map(Vec::len).sum()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.total() == 0
    }

    #[cfg(test)]
    pub(super) fn contains(&self, dimension: &str, pos: [i32; 3]) -> bool {
        self.by_dim.get(dimension).is_some_and(|list| list.contains(&pos))
    }

    /// Toggles `pos` in `dimension`: removes if already linked, adds up to `MAX_LINKS`.
    pub(super) fn toggle(&mut self, dimension: &str, pos: [i32; 3]) -> ToggleResult {
        if let Some(list) = self.by_dim.get_mut(dimension)
            && let Some(idx) = list.iter().position(|&p| p == pos)
        {
            list.remove(idx);
            if list.is_empty() {
                self.by_dim.remove(dimension);
            }
            return ToggleResult::Unlinked { total: self.total() };
        }
        if self.total() >= MAX_LINKS {
            return ToggleResult::Full { max: MAX_LINKS };
        }
        self.by_dim.entry(dimension.to_owned()).or_default().push(pos);
        ToggleResult::Linked { total: self.total() }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, [i32; 3])> {
        self.by_dim
            .iter()
            .flat_map(|(dim, list)| list.iter().map(move |&pos| (dim.as_str(), pos)))
    }

    pub(super) fn clear(&mut self) {
        self.by_dim.clear();
    }

    /// Reads linked positions from a stack's `minecraft:custom_data` compound, ignoring malformed
    /// entries and capping total links at `MAX_LINKS`.
    pub(super) fn from_custom_data(custom_data: Option<&Compound>) -> Self {
        let mut out = Self::default();
        let Some(Tag::Compound(dims)) = custom_data.and_then(|c| c.get(&LINKS_KEY.into())) else {
            return out;
        };
        let mut count = 0usize;
        for (dim_key, tag) in dims {
            if count >= MAX_LINKS {
                break;
            }
            let Tag::IntArray(coords) = tag else {
                continue;
            };
            let dim = String::from_utf16_lossy(&dim_key.0);
            for &[x, y, z] in coords.as_chunks::<3>().0 {
                if count >= MAX_LINKS {
                    break;
                }
                let pos = [x, y, z];
                let list = out.by_dim.entry(dim.clone()).or_default();
                if !list.contains(&pos) {
                    list.push(pos);
                    count += 1;
                }
            }
        }
        out
    }

    pub(super) fn from_stack(stack: &ItemStack) -> Self {
        let data = stack.get(&CustomDataComponent);
        Self::from_custom_data(data.as_ref())
    }

    /// Writes linked positions into `base` while preserving unrelated keys; returns `None` if the
    /// resulting `minecraft:custom_data` compound would be empty.
    pub(super) fn write_to_custom_data(&self, base: Option<Compound>) -> Option<Compound> {
        let mut data = base.unwrap_or_default();
        if self.is_empty() {
            data.remove(&LINKS_KEY.into());
            return (!data.is_empty()).then_some(data);
        }
        let mut dims = Compound::new();
        for (dim, list) in &self.by_dim {
            if list.is_empty() {
                continue;
            }
            let mut flat = Vec::with_capacity(list.len() * 3);
            for &[x, y, z] in list {
                flat.push(x);
                flat.push(y);
                flat.push(z);
            }
            dims.insert(dim.as_str().into(), Tag::IntArray(flat));
        }
        data.insert(LINKS_KEY.into(), Tag::Compound(dims));
        Some(data)
    }

    pub(super) fn write_to_stack(&self, stack: &mut ItemStack) {
        let base = stack.get(&CustomDataComponent);
        match self.write_to_custom_data(base) {
            Some(data) => {
                let _ = stack.set(&CustomDataComponent, data);
            }
            None => stack.remove(&CustomDataComponent),
        }
    }
}

/// Formats the chat/action-bar feedback summary after firing a detonator.
pub(super) fn fire_summary(lit: usize, missing: usize, unloaded: usize) -> String {
    let mut skipped = Vec::new();
    if missing > 0 {
        skipped.push(format!("{missing} missing"));
    }
    if unloaded > 0 {
        skipped.push(format!("{unloaded} in unloaded chunks"));
    }
    if lit > 0 {
        if skipped.is_empty() {
            format!("Detonator: lit {lit} TNT (5 s fuse)")
        } else {
            format!("Detonator: lit {lit} TNT (5 s fuse; {})", skipped.join(", "))
        }
    } else if skipped.is_empty() {
        "Detonator: no TNT linked".to_owned()
    } else {
        format!("Detonator: no TNT lit ({})", skipped.join(", "))
    }
}

/// The `ItemPart` attached to `realistic_tnt:detonator`.
pub(super) struct Detonator;

#[mod_api::horizon_part]
impl ItemPart for Detonator {
    fn tooltip(&self, _host: &dyn ClientHost, event: &mut TooltipEvent<'_>) {
        let count = DetonatorLinks::from_stack(event.stack).total();
        if count == 0 {
            event.lines.push((Text::literal("No TNT linked"), 0xFFAA_AAAA));
        } else {
            event
                .lines
                .push((Text::literal(format!("Linked: {count} / {MAX_LINKS} TNT")), 0xFFFF_AA00));
        }
    }

    fn use_on(&self, host: HostSide, event: &mut UseOnEvent<'_>) -> Option<InteractionResult> {
        match host {
            HostSide::Client(client) => {
                let name = client
                    .local_player()
                    .and_then(|player| player.level(&*client))
                    .map(|level| super::block_at(&*client, level, event.pos));
                if name.is_some_and(|n| super::tnt_worth(&n).is_some()) {
                    Some(InteractionResult::Success)
                } else {
                    // Fail stops the click from falling through to `use_item` on non-TNT blocks.
                    Some(InteractionResult::Fail)
                }
            }
            HostSide::Server(host) => {
                let Some(player) = host.player(event.player) else {
                    return Some(InteractionResult::Fail);
                };
                let Some(level) = player.level(&*host) else {
                    return Some(InteractionResult::Fail);
                };
                let name = super::block_at(&*host, level, event.pos);
                if super::tnt_worth(&name).is_none() {
                    return Some(InteractionResult::Fail);
                }
                let dimension = super::level_name(&*host, level);
                let mut links = DetonatorLinks::from_stack(event.stack);
                match links.toggle(&dimension, event.pos) {
                    ToggleResult::Linked { total } => {
                        let mut updated = event.stack.clone();
                        links.write_to_stack(&mut updated);
                        if let Some(living) = host.living(player.entity()) {
                            let _ = living.set_hand_item(host, event.hand, Some(updated));
                        }
                        let _ = player.action_bar(host, Text::literal(format!("Linked TNT ({total}/{MAX_LINKS})")));
                        let _ = player.play_sound(host, "minecraft:block.note_block.pling", SoundSource::Players, 0.7, 1.4);
                        Some(InteractionResult::Success)
                    }
                    ToggleResult::Unlinked { total } => {
                        let mut updated = event.stack.clone();
                        links.write_to_stack(&mut updated);
                        if let Some(living) = host.living(player.entity()) {
                            let _ = living.set_hand_item(host, event.hand, Some(updated));
                        }
                        let _ = player.action_bar(host, Text::literal(format!("Unlinked TNT ({total}/{MAX_LINKS})")));
                        let _ = player.play_sound(host, "minecraft:block.note_block.pling", SoundSource::Players, 0.7, 0.8);
                        Some(InteractionResult::Success)
                    }
                    ToggleResult::Full { max } => {
                        let _ = player.action_bar(host, Text::literal(format!("Detonator full ({max}/{max})")));
                        let _ = player.play_sound(host, "minecraft:block.note_block.pling", SoundSource::Players, 0.6, 0.5);
                        Some(InteractionResult::Fail)
                    }
                }
            }
        }
    }

    fn use_item(&self, host: HostSide, event: &mut UseItemEvent<'_>) -> Option<InteractionResult> {
        match host {
            HostSide::Client(_) => {
                if DetonatorLinks::from_stack(event.stack).is_empty() {
                    Some(InteractionResult::Consume)
                } else {
                    Some(InteractionResult::Success)
                }
            }
            HostSide::Server(host) => {
                let Some(player) = host.player(event.player) else {
                    return Some(InteractionResult::Fail);
                };
                let mut links = DetonatorLinks::from_stack(event.stack);
                if links.is_empty() {
                    let _ = player.action_bar(host, Text::literal("Detonator: no TNT linked"));
                    let _ = player.play_sound(host, "minecraft:ui.button.click", SoundSource::Players, 0.5, 0.8);
                    return Some(InteractionResult::Consume);
                }
                let targets: Vec<(String, [i32; 3])> =
                    links.iter().map(|(dim, pos)| (dim.to_owned(), pos)).collect();
                links.clear();
                let mut updated = event.stack.clone();
                links.write_to_stack(&mut updated);
                if let Some(living) = host.living(player.entity()) {
                    let _ = living.set_hand_item(host, event.hand, Some(updated));
                }

                let (mut lit, mut missing, mut unloaded) = (0usize, 0usize, 0usize);
                for (dim, pos) in targets {
                    let Some(level) = host.level(&dim) else {
                        unloaded += 1;
                        continue;
                    };
                    if !level.is_loaded(&*host, pos) {
                        unloaded += 1;
                        continue;
                    }
                    let name = super::block_at(&*host, level, pos);
                    let Some(worth) = super::tnt_worth(&name) else {
                        missing += 1;
                        continue;
                    };
                    if let Some(id) = super::light(host, level, &dim, pos, &name, worth, DETONATOR_FUSE_TICKS, true) {
                        super::follow_lit_fixed(host, level, &dim, pos, worth, DETONATOR_FUSE_TICKS, id);
                        lit += 1;
                    } else {
                        unloaded += 1;
                    }
                }

                let summary = fire_summary(lit, missing, unloaded);
                let _ = player.tell(host, Text::literal(summary.clone()));
                if lit == 0 {
                    let _ = player.action_bar(host, Text::literal(summary));
                }
                let _ = player.play_sound(
                    host,
                    "minecraft:block.lever.click",
                    SoundSource::Players,
                    0.8,
                    if lit > 0 { 1.2 } else { 0.6 },
                );
                super::destruction::debug(format_args!(
                    "detonator fired: lit={lit}, missing={missing}, unloaded={unloaded}"
                ));
                Some(if lit > 0 { InteractionResult::Success } else { InteractionResult::Consume })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_links_unlinks_and_separates_dimensions() {
        let mut links = DetonatorLinks::default();
        assert!(links.is_empty());
        assert_eq!(links.toggle("minecraft:overworld", [1, 64, 2]), ToggleResult::Linked { total: 1 });
        assert_eq!(links.toggle("minecraft:the_nether", [1, 64, 2]), ToggleResult::Linked { total: 2 });
        assert!(links.contains("minecraft:overworld", [1, 64, 2]));
        assert!(links.contains("minecraft:the_nether", [1, 64, 2]));
        assert_eq!(links.toggle("minecraft:overworld", [1, 64, 2]), ToggleResult::Unlinked { total: 1 });
        assert!(!links.contains("minecraft:overworld", [1, 64, 2]));
        assert!(links.contains("minecraft:the_nether", [1, 64, 2]));
        assert_eq!(links.toggle("minecraft:the_nether", [1, 64, 2]), ToggleResult::Unlinked { total: 0 });
        assert!(links.is_empty());
    }

    #[test]
    fn cap_enforces_max_links_and_allows_readd_after_unlink() {
        let mut links = DetonatorLinks::default();
        for i in 0..MAX_LINKS as i32 {
            assert_eq!(
                links.toggle("minecraft:overworld", [i, 64, 0]),
                ToggleResult::Linked { total: i as usize + 1 }
            );
        }
        assert_eq!(links.total(), MAX_LINKS);
        assert_eq!(
            links.toggle("minecraft:overworld", [999, 64, 0]),
            ToggleResult::Full { max: MAX_LINKS }
        );
        assert_eq!(links.total(), MAX_LINKS);
        assert_eq!(
            links.toggle("minecraft:overworld", [0, 64, 0]),
            ToggleResult::Unlinked { total: MAX_LINKS - 1 }
        );
        assert_eq!(
            links.toggle("minecraft:overworld", [999, 64, 0]),
            ToggleResult::Linked { total: MAX_LINKS }
        );
    }

    #[test]
    fn custom_data_round_trip_preserves_other_keys_and_handles_corrupt_nbt() {
        let mut links = DetonatorLinks::default();
        links.toggle("minecraft:overworld", [10, 100, -20]);
        links.toggle("minecraft:overworld", [11, 100, -20]);
        links.toggle("minecraft:the_end", [0, 65, 0]);

        let mut base = Compound::new();
        base.insert("other_mod_tag".into(), Tag::Int(42));
        let encoded = links.write_to_custom_data(Some(base)).unwrap();
        assert_eq!(encoded.get(&"other_mod_tag".into()), Some(&Tag::Int(42)));

        let decoded = DetonatorLinks::from_custom_data(Some(&encoded));
        assert_eq!(decoded, links);

        // Clearing removes only `realistic_tnt_links`, keeping unrelated keys.
        let cleared_with_other = DetonatorLinks::default().write_to_custom_data(Some(encoded)).unwrap();
        assert_eq!(cleared_with_other.get(&"other_mod_tag".into()), Some(&Tag::Int(42)));
        assert!(!cleared_with_other.contains_key(&LINKS_KEY.into()));
        assert_eq!(DetonatorLinks::default().write_to_custom_data(None), None);

        // Truncated/oversized NBT is safely capped and trailing non-triples are ignored.
        let mut dims = Compound::new();
        let mut coords = Vec::new();
        for i in 0..(MAX_LINKS as i32 + 10) {
            coords.extend_from_slice(&[i, 64, 0]);
        }
        coords.extend_from_slice(&[999, 999]); // trailing partial triple
        dims.insert("minecraft:overworld".into(), Tag::IntArray(coords));
        let mut bad_root = Compound::new();
        bad_root.insert(LINKS_KEY.into(), Tag::Compound(dims));
        let clamped = DetonatorLinks::from_custom_data(Some(&bad_root));
        assert_eq!(clamped.total(), MAX_LINKS);
    }

    #[test]
    fn fire_summary_formats_lit_and_skipped_counts() {
        assert_eq!(fire_summary(4, 0, 0), "Detonator: lit 4 TNT (5 s fuse)");
        assert_eq!(
            fire_summary(3, 1, 2),
            "Detonator: lit 3 TNT (5 s fuse; 1 missing, 2 in unloaded chunks)"
        );
        assert_eq!(fire_summary(0, 1, 0), "Detonator: no TNT lit (1 missing)");
        assert_eq!(fire_summary(0, 0, 2), "Detonator: no TNT lit (2 in unloaded chunks)");
        assert_eq!(fire_summary(0, 0, 0), "Detonator: no TNT linked");
    }

    #[test]
    fn detonator_content_and_recipe_files_exist() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for rel in [
            "data/realistic_tnt/horizon/item/detonator.json",
            "data/realistic_tnt/recipe/detonator.json",
            "data/realistic_tnt/horizon/creative_tab/tnt.json",
            "assets/realistic_tnt/items/detonator.json",
            "assets/realistic_tnt/models/item/detonator.json",
            "assets/realistic_tnt/textures/item/detonator.png",
            "assets/realistic_tnt/lang/en_us.json",
            "assets/realistic_tnt/lang/pt_br.json",
        ] {
            assert!(root.join(rel).is_file(), "missing content file: {rel}");
        }
        let tab = std::fs::read_to_string(root.join("data/realistic_tnt/horizon/creative_tab/tnt.json")).unwrap();
        assert!(tab.contains("\"realistic_tnt:detonator\""));
        let en = std::fs::read_to_string(root.join("assets/realistic_tnt/lang/en_us.json")).unwrap();
        assert!(en.contains("\"item.realistic_tnt.detonator\""));
        let pt = std::fs::read_to_string(root.join("assets/realistic_tnt/lang/pt_br.json")).unwrap();
        assert!(pt.contains("\"item.realistic_tnt.detonator\""));
    }
}
