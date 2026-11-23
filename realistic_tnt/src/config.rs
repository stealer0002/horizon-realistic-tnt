use std::{io::ErrorKind, path::PathBuf, str::FromStr, sync::OnceLock};

use mod_api::{InstanceDirectory, RegistrationContext};

#[derive(Clone, Copy)]
pub(super) struct Tuning {
    pub escape_speed: f32,
    pub max_fold: usize,
    pub hold_chunks: i32,
    pub lift_yield: f32,
    pub breaks_at: [f64; 4],
    pub scorch: bool,
    pub knockback: f64,
}

const DEFAULTS: Tuning = Tuning {
    escape_speed: super::ESCAPE_SPEED,
    max_fold: super::MAX_FOLD,
    hold_chunks: super::HOLD_CHUNKS,
    lift_yield: super::LIFT_YIELD,
    breaks_at: super::BREAKS_AT,
    scorch: super::SCORCH,
    knockback: super::KNOCKBACK,
};

static CONFIG: OnceLock<Tuning> = OnceLock::new();

pub(super) fn get() -> &'static Tuning {
    CONFIG.get().unwrap_or(&DEFAULTS)
}

pub(super) fn load(context: &RegistrationContext) {
    CONFIG.get_or_init(|| {
        let Ok(directory) = context.instance_dir() else {
            return DEFAULTS;
        };
        let Some(path) = native_path(directory) else {
            return DEFAULTS;
        };
        let path = path.join("config").join("realistic_tnt.conf");
        match std::fs::read_to_string(&path) {
            Ok(contents) => parse(&contents),
            Err(error) => {
                if error.kind() != ErrorKind::NotFound {
                    eprintln!("[realistic_tnt] Could not read {}: {error}", path.display());
                }
                DEFAULTS
            }
        }
    });
}

pub(super) fn native_path(directory: &InstanceDirectory) -> Option<PathBuf> {
    match directory {
        #[cfg(unix)]
        InstanceDirectory::Unix(bytes) => {
            use std::os::unix::ffi::OsStrExt;
            Some(std::ffi::OsStr::from_bytes(bytes).into())
        }
        #[cfg(windows)]
        InstanceDirectory::Windows(units) => {
            use std::os::windows::ffi::OsStringExt;
            Some(std::ffi::OsString::from_wide(units).into())
        }
        _ => None,
    }
}

fn parse(contents: &str) -> Tuning {
    let mut config = DEFAULTS;
    for (index, line) in contents.lines().enumerate() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if !line
            .split_once('=')
            .is_some_and(|(key, value)| apply(&mut config, key.trim(), value.trim()))
        {
            eprintln!("[realistic_tnt] Invalid config entry on line {}", index + 1);
        }
    }
    config
}

fn apply(config: &mut Tuning, key: &str, value: &str) -> bool {
    match key {
        "ESCAPE_SPEED" => set(&mut config.escape_speed, value, |n| {
            n.is_finite() && n > 0.0
        }),
        "MAX_FOLD" => set(&mut config.max_fold, value, |n| n > 0),
        "HOLD_CHUNKS" => set(&mut config.hold_chunks, value, |n| (1..=32).contains(&n)),
        "LIFT_YIELD" => set(&mut config.lift_yield, value, |n| n.is_finite() && n > 0.0),
        "BREAKS_AT" => {
            let Ok(values) = value
                .split(',')
                .map(|n| n.trim().parse::<f64>())
                .collect::<Result<Vec<_>, _>>()
            else {
                return false;
            };
            let Ok(values) = <[f64; 4]>::try_from(values) else {
                return false;
            };
            if !values.iter().all(|n| n.is_finite() && *n >= 0.0)
                || !values.windows(2).all(|pair| pair[0] < pair[1])
            {
                return false;
            }
            config.breaks_at = values;
            true
        }
        "SCORCH" => set(&mut config.scorch, value, |_| true),
        "KNOCKBACK" => set(&mut config.knockback, value, |n| n.is_finite() && n >= 0.0),
        _ => false,
    }
}

fn set<T: FromStr + Copy>(field: &mut T, value: &str, valid: impl FnOnce(T) -> bool) -> bool {
    let Ok(value) = value.parse::<T>() else {
        return false;
    };
    if !valid(value) {
        return false;
    }
    *field = value;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_accepts_all_overrides() {
        let config = parse(
            "ESCAPE_SPEED = 20 # blocks/s\nMAX_FOLD = 900\nHOLD_CHUNKS = 8\n\
             LIFT_YIELD = 32\nBREAKS_AT = 1, 2, 3, 4\nSCORCH = false\nKNOCKBACK = 0",
        );
        assert_eq!(config.escape_speed, 20.0);
        assert_eq!(config.max_fold, 900);
        assert_eq!(config.hold_chunks, 8);
        assert_eq!(config.lift_yield, 32.0);
        assert_eq!(config.breaks_at, [1.0, 2.0, 3.0, 4.0]);
        assert!(!config.scorch);
        assert_eq!(config.knockback, 0.0);
    }

    #[test]
    fn invalid_or_missing_entries_use_defaults() {
        let config = parse("ESCAPE_SPEED=NaN\nHOLD_CHUNKS=0\nBREAKS_AT=4,3,2,1\nSCORCH=off");
        assert_eq!(config.escape_speed, DEFAULTS.escape_speed);
        assert_eq!(config.hold_chunks, DEFAULTS.hold_chunks);
        assert_eq!(config.breaks_at, DEFAULTS.breaks_at);
        assert_eq!(config.scorch, DEFAULTS.scorch);
        assert_eq!(config.max_fold, DEFAULTS.max_fold);
    }
}
