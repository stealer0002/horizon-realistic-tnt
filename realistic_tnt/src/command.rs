use mod_api::{
    Command, CommandArgumentValue, CommandSource, ExplosionInteraction, ExplosionSettings, Flow,
    RegisterCommandsEvent, RunModCommandEvent, ServerHost, Text,
};

const REACH: f64 = 128.0;
const MAX_YIELD: i32 = 200_000;

pub(super) fn register(event: &mut RegisterCommandsEvent<'_>) {
    event.commands.push(Command::op("tnt <n:integer>", 2));
}

pub(super) fn run(host: &mut dyn ServerHost, event: &mut RunModCommandEvent<'_>) -> Flow {
    if event.command.literals != ["tnt"] {
        return Flow::Continue;
    }
    if let Err(message) = detonate(host, event) {
        let _ = host.commands().feedback(Text::literal(message));
    }
    Flow::Cancel
}

fn detonate(host: &mut dyn ServerHost, event: &RunModCommandEvent<'_>) -> Result<(), String> {
    let CommandSource::Player { player, .. } = event.source else {
        return Err("This command requires a player.".into());
    };
    let Some(CommandArgumentValue::Integer(n)) = event.command.argument("n") else {
        return Err("Usage: /tnt <n>".into());
    };
    if !(1..=MAX_YIELD).contains(n) {
        return Err(format!("TNT count must be between 1 and {MAX_YIELD}."));
    }
    let level = player
        .level(&*host)
        .ok_or("The player's level is unavailable.")?;
    let hit = player
        .entity()
        .pick(&*host, REACH)
        .filter(|hit| !hit.miss && !hit.world_border)
        .ok_or("Aim at a block within 128 blocks.")?;

    // Invert yield_of so the ordinary explosion hooks see the requested TNT yield.
    let radius = 4.0 * (*n as f32).cbrt();
    level
        .explode(
            host,
            None,
            hit.location,
            radius,
            false,
            ExplosionInteraction::Tnt,
            ExplosionSettings::default(),
        )
        .map_err(|error| format!("Explosion refused: {error:?}"))
}
