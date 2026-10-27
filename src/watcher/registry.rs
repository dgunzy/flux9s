//! `:` command names for the Flux kinds — read from the `flux_kinds!` table
//! ([`FluxResourceKind::aliases`]), not kept here.

use crate::models::FluxResourceKind;

/// Get display name for a command alias
pub fn get_display_name_for_command(cmd: &str) -> Option<&'static str> {
    FluxResourceKind::from_alias(cmd).map(|kind| kind.as_str())
}

/// Get all command aliases for help text
pub fn get_all_commands() -> Vec<(&'static str, &'static [&'static str])> {
    FluxResourceKind::ALL
        .iter()
        .map(|kind| (kind.as_str(), kind.aliases()))
        .collect()
}
