// one global color switch, set from [interface] color in config.toml at startup.
// escape sequences collapse to empty strings when color is off, so callers keep
// using the same palette constants and don't need to branch everywhere.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// return the escape sequence when color is on, "" when it is off
pub fn seq(code: &'static str) -> &'static str {
    if enabled() {
        code
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_collapses_when_disabled() {
        set_enabled(false);
        assert_eq!(seq("\x1b[31m"), "");
        set_enabled(true);
        assert_eq!(seq("\x1b[31m"), "\x1b[31m");
    }
}
