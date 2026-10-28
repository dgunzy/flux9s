//! Command registry and handling for flux9s commands
//!
//! Centralizes command definitions, autocomplete, and execution logic
//! to keep app.rs focused on application state management.

use crate::tui::submenu::{CommandSubmenu, SubmenuItem, SubmenuState};
use anyhow::Result;

/// Ids of the popups (submenus), shared by each popup's builder and the
/// selection table in `app/submenus.rs`, so the two can't disagree.
pub mod submenu_ids {
    pub const CONTEXT: &str = "ctx";
    pub const NAMESPACE: &str = "ns";
    pub const SKIN: &str = "skin";
    pub const CONTROLLER_LOGS: &str = "logs";
    pub const POD_LOGS: &str = "pod-logs";
    pub const POD_DELETE: &str = "pod-delete";
    pub const LOG_CONTAINER: &str = "log-container";
    /// Every popup id (tests check each has a selection handler).
    #[cfg(test)]
    pub const ALL: &[&str] = &[
        CONTEXT,
        NAMESPACE,
        SKIN,
        CONTROLLER_LOGS,
        POD_LOGS,
        POD_DELETE,
        LOG_CONTAINER,
    ];
}

/// Command definition
#[derive(Debug, Clone)]
pub struct Command {
    /// Command name/alias
    pub name: &'static str,
    /// Whether this command takes arguments
    pub takes_args: bool,
}

/// Application commands (non-CRD commands)
pub const APP_COMMANDS: &[Command] = &[
    Command {
        name: "events",
        takes_args: false,
    },
    Command {
        name: "pulse",
        takes_args: false,
    },
    Command {
        name: "logs",
        takes_args: true,
    },
    Command {
        name: "unmanaged",
        takes_args: true,
    },
    Command {
        name: "orphaned",
        takes_args: true,
    },
    Command {
        name: "healthy",
        takes_args: false,
    },
    Command {
        name: "unhealthy",
        takes_args: false,
    },
    Command {
        name: "suspended",
        takes_args: false,
    },
    Command {
        name: "readonly",
        takes_args: false,
    },
    Command {
        name: "read-only",
        takes_args: false,
    },
    Command {
        name: "discover",
        takes_args: false,
    },
    Command {
        name: "help",
        takes_args: false,
    },
    Command {
        name: "quit",
        takes_args: false,
    },
    Command {
        name: "exit",
        takes_args: false,
    },
    Command {
        name: "skin",
        takes_args: true,
    },
    Command {
        name: "trace",
        takes_args: true,
    },
    Command {
        name: "context",
        takes_args: true,
    },
    Command {
        name: "namespace",
        takes_args: true,
    },
    Command {
        name: "favorites",
        takes_args: false,
    },
    Command {
        name: "fav",
        takes_args: false,
    },
    Command {
        name: "flux",
        takes_args: false,
    },
    Command {
        name: "native",
        takes_args: false,
    },
    Command {
        name: "all",
        takes_args: false,
    },
    Command {
        name: "clear",
        takes_args: false,
    },
];

/// Find all commands that match the given prefix
///
/// Returns commands sorted by priority (CRD commands first, then App commands)
/// and then alphabetically within each category.
/// Commands that take arguments are returned with a trailing space (e.g., "skin ").
/// Discovered-kind completions for `prefix`: sorted, deduplicated, and never
/// repeating a Flux kind or app command already offered.
fn native_kind_matches(
    prefix: &str,
    tokens: Vec<String>,
    flux: &[String],
    app: &[String],
) -> Vec<String> {
    let mut matches: Vec<String> = tokens
        .into_iter()
        .filter(|token| token.starts_with(prefix))
        .filter(|token| !flux.contains(token) && !app.contains(token))
        .collect();
    matches.sort();
    matches.dedup();
    matches
}

pub fn find_matching_commands(prefix: &str) -> Vec<String> {
    let prefix_lower = prefix.to_lowercase();
    let mut crd_matches: Vec<String> = Vec::new();
    let mut app_matches: Vec<String> = Vec::new();

    // Get CRD commands from registry
    let crd_commands = crate::watcher::get_all_commands();
    for (_, aliases) in crd_commands {
        for alias in aliases.iter() {
            if alias.starts_with(&prefix_lower) {
                crd_matches.push((*alias).to_string());
            }
        }
    }

    // Get app commands
    for cmd in APP_COMMANDS {
        let cmd_name_lower = cmd.name.to_lowercase();
        if cmd.takes_args {
            // For commands with args, check if prefix matches the command part
            let full_cmd = format!("{} ", cmd.name);
            if full_cmd.starts_with(&prefix_lower) && prefix_lower != full_cmd {
                if prefix_lower.len() <= cmd.name.len() {
                    // Return command with space for autocomplete
                    app_matches.push(full_cmd.clone());
                }
            } else if prefix_lower == cmd_name_lower {
                // Exact match - return with space
                app_matches.push(full_cmd.clone());
            }
        } else {
            // Simple command - check if prefix matches
            if cmd_name_lower.starts_with(&prefix_lower) {
                app_matches.push(cmd.name.to_string());
            }
        }
    }

    // Dynamically discovered kinds (#197): kind names, plurals, and CRD
    // short names complete like built-in resource commands. The registry is
    // empty unless discovery is enabled, so this adds nothing by default.
    for extra in crate::models::extra_kinds::global().kind_names() {
        if let Some(kind) = crate::models::extra_kinds::global().get(&extra) {
            for alias in std::iter::once(kind.kind.to_lowercase())
                .chain(std::iter::once(kind.plural.to_lowercase()))
                .chain(kind.short_names.iter().map(|s| s.to_lowercase()))
            {
                if alias.starts_with(&prefix_lower) && !crd_matches.contains(&alias) {
                    crd_matches.push(alias);
                }
            }
        }
    }

    // Sort matches alphabetically
    crd_matches.sort();
    app_matches.sort();

    // Kinds from API discovery (#267): plurals and short names, after Flux
    // kinds and app commands so they never displace them. Empty unless
    // `nativeResources` is on and discovery has run.
    let native_matches = native_kind_matches(
        &prefix_lower,
        crate::models::kinds::discovered_command_tokens(),
        &crd_matches,
        &app_matches,
    );

    // Combine: CRD commands first (higher priority), then app commands,
    // then discovered native kinds
    let mut all_matches = crd_matches;
    all_matches.extend(app_matches);
    all_matches.extend(native_matches);
    all_matches
}

// Command matching helpers - use these instead of hardcoding command strings

/// Check if command opens the live Kubernetes events view
pub fn is_events_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "events" || cmd_lower == "event" || cmd_lower == "ev"
}

/// Check if command opens the pulse dashboard
pub fn is_pulse_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "pulse" || cmd_lower == "dashboard"
}

/// Check if command opens the controller log viewer (with or without a pod argument)
pub fn is_logs_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "logs"
        || cmd_lower == "log"
        || cmd_lower.starts_with("logs ")
        || cmd_lower.starts_with("log ")
}

/// Check if command is readonly (handles both "readonly" and "read-only")
pub fn is_readonly_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "readonly" || cmd_lower == "read-only"
}

/// Check if command toggles CRD discovery (#245), the runtime half of the
/// `discoverFluxResources` setting. Handles "discover" and "discovery".
pub fn is_discover_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "discover" || cmd_lower == "discovery"
}

/// Check if command is help (handles "help", "h", "?")
pub fn is_help_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "help" || cmd_lower == "h" || cmd_lower == "?"
}

/// Check if command is quit (handles "q", "q!", "quit", "exit")
pub fn is_quit_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "q" || cmd_lower == "q!" || cmd_lower == "quit" || cmd_lower == "exit"
}

/// Check if command is healthy filter
pub fn is_healthy_command(cmd: &str) -> bool {
    cmd.to_lowercase() == "healthy"
}

/// Check if command is unhealthy filter
pub fn is_unhealthy_command(cmd: &str) -> bool {
    cmd.to_lowercase() == "unhealthy"
}

/// Check if command is suspended filter
pub fn is_suspended_command(cmd: &str) -> bool {
    cmd.to_lowercase() == "suspended"
}

/// Check if command is "all" or "clear"
/// `:native` — toggle native resource browsing (preview) for the session.
pub fn is_native_command(cmd: &str) -> bool {
    cmd.eq_ignore_ascii_case("native")
}

/// `:flux` — return to the Flux resource list from any kind list (#267).
pub fn is_flux_command(cmd: &str) -> bool {
    cmd.eq_ignore_ascii_case("flux")
}

pub fn is_all_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "all" || cmd_lower == "clear"
}

/// Check if command is skin command (with optional args)
pub fn is_skin_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "skin" || cmd_lower.starts_with("skin ")
}

/// Check if command is trace command (with optional args)
pub fn is_trace_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "trace" || cmd_lower.starts_with("trace ")
}

/// Split `<head> -n <namespace>` (also `--namespace <ns>` /
/// `--namespace=<ns>`), for views and kind sets (#306). `None` when the
/// command has no namespace flag; `Err` when it's malformed.
pub fn split_namespace_flag(cmd: &str) -> Option<Result<(String, String), String>> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let flag = tokens
        .iter()
        .position(|t| *t == "-n" || *t == "--namespace" || t.starts_with("--namespace="))?;
    let namespace = match tokens[flag].strip_prefix("--namespace=") {
        Some(ns) => (!ns.is_empty()).then(|| (ns.to_string(), flag + 1)),
        None => tokens.get(flag + 1).map(|ns| (ns.to_string(), flag + 2)),
    };
    let Some((namespace, after)) = namespace else {
        return Some(Err("-n needs a namespace".to_string()));
    };
    // The head is everything else; kind sets may be written `ks, hr`.
    let head: String = tokens[..flag]
        .iter()
        .chain(&tokens[after..])
        .copied()
        .collect::<Vec<_>>()
        .join("");
    if head.is_empty() {
        return Some(Err(
            "-n needs a view or kinds (e.g. :ks,hr -n apps)".to_string()
        ));
    }
    Some(Ok((head, namespace)))
}

/// `:trace` usage, shown for malformed arguments.
pub const TRACE_USAGE: &str = "Usage: :trace [<kind>/<name> [-n <namespace>]]";

/// Parsed `:trace` arguments (#266).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceArgs {
    /// `(kind token, name)`; `None` traces what the current view shows.
    pub target: Option<(String, String)>,
    /// kubectl-style `-n` / `--namespace`.
    pub namespace: Option<String>,
}

/// Parse `:trace [<kind>/<name>] [-n <namespace>]` — `-n <ns>`,
/// `--namespace <ns>`, or `--namespace=<ns>`, before or after the target.
/// `Err` carries the message to show.
pub fn parse_trace_args(cmd: &str) -> Result<TraceArgs, String> {
    let mut tokens = cmd.split_whitespace().skip(1);
    let mut target = None;
    let mut namespace = None;
    while let Some(token) = tokens.next() {
        let value = match token {
            "-n" | "--namespace" => Some(tokens.next().ok_or("-n needs a namespace")?),
            _ => token.strip_prefix("--namespace="),
        };
        if let Some(value) = value {
            if value.is_empty() || namespace.is_some() {
                return Err(TRACE_USAGE.to_string());
            }
            namespace = Some(value.to_string());
            continue;
        }
        if token.starts_with('-') || target.is_some() {
            return Err(TRACE_USAGE.to_string());
        }
        let (kind, name) = token
            .split_once('/')
            .filter(|(kind, name)| !kind.is_empty() && !name.is_empty() && !name.contains('/'))
            .ok_or_else(|| TRACE_USAGE.to_string())?;
        target = Some((kind.to_string(), name.to_string()));
    }
    if target.is_none() && namespace.is_some() {
        return Err("-n needs a <kind>/<name> to trace".to_string());
    }
    Ok(TraceArgs { target, namespace })
}

/// `:unmanaged [-n <ns> | -A]` (#274; `all` is still accepted).
pub fn is_unmanaged_command(cmd: &str) -> bool {
    cmd.split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("unmanaged"))
}

/// `:orphaned [<kind>/<name>] [-n <ns> | -A]` (#274).
pub fn is_orphaned_command(cmd: &str) -> bool {
    cmd.split_whitespace().next().is_some_and(|word| {
        word.eq_ignore_ascii_case("orphaned") || word.eq_ignore_ascii_case("orphans")
    })
}

/// `:unmanaged` / `:orphaned` usage, shown for malformed arguments.
pub const SWEEP_USAGE: &str =
    "Usage: :unmanaged [-n <ns> | -A] · :orphaned [<kind>/<name>] [-n <ns> | -A]";

/// Parsed `:unmanaged` / `:orphaned` arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepArgs {
    /// `all`: include objects other tools manage.
    pub include_other: bool,
    /// `<kind>/<name>`: one Flux object's orphans.
    pub target: Option<(String, String)>,
    /// `-n <ns>` → `Some(Some(ns))`; `-A` / `-n all` → `Some(None)`.
    pub namespace: Option<Option<String>>,
}

/// Parse `:unmanaged` / `:orphaned` arguments. `Err` carries the message.
pub fn parse_sweep_args(cmd: &str) -> Result<SweepArgs, String> {
    let mut tokens = cmd.split_whitespace().skip(1);
    let mut args = SweepArgs::default();
    let set_namespace = |args: &mut SweepArgs, value: &str| {
        if value.is_empty() || args.namespace.is_some() {
            return Err(SWEEP_USAGE.to_string());
        }
        args.namespace = Some((value != "all").then(|| value.to_string()));
        Ok(())
    };
    while let Some(token) = tokens.next() {
        match token {
            "-A" | "--all-namespaces" => set_namespace(&mut args, "all")?,
            "-n" | "--namespace" => {
                set_namespace(&mut args, tokens.next().ok_or("-n needs a namespace")?)?;
            }
            _ if token.starts_with("--namespace=") => {
                set_namespace(&mut args, &token["--namespace=".len()..])?;
            }
            "all" if !args.include_other => args.include_other = true,
            _ => {
                let target = token.split_once('/').filter(|(kind, name)| {
                    !kind.is_empty() && !name.is_empty() && !name.contains('/')
                });
                match target {
                    Some((kind, name)) if args.target.is_none() => {
                        args.target = Some((kind.to_string(), name.to_string()));
                    }
                    _ => return Err(SWEEP_USAGE.to_string()),
                }
            }
        }
    }
    Ok(args)
}

/// Check if command is context command (handles both "ctx" and "context")
pub fn is_context_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "ctx"
        || cmd_lower.starts_with("ctx ")
        || cmd_lower == "context"
        || cmd_lower.starts_with("context ")
}

/// Check if command is namespace command (handles both "ns" and "namespace")
pub fn is_namespace_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "ns"
        || cmd_lower.starts_with("ns ")
        || cmd_lower == "namespace"
        || cmd_lower.starts_with("namespace ")
}

/// Check if command is favorites command (handles both "favorites" and "fav")
pub fn is_favorites_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "favorites" || cmd_lower == "fav"
}

/// Extract argument from a command that takes arguments
/// Returns None if command doesn't match or has no argument
pub fn extract_command_arg(cmd: &str, command_name: &str) -> Option<String> {
    let cmd_lower = cmd.to_lowercase();
    let command_lower = command_name.to_lowercase();

    // Check exact match or starts with command + space
    if cmd_lower == command_lower {
        None // No argument provided
    } else if cmd_lower.starts_with(&format!("{} ", command_lower)) {
        // Extract everything after the command and space
        let arg = cmd
            .split_whitespace()
            .skip(1)
            .collect::<Vec<&str>>()
            .join(" ");
        if arg.is_empty() { None } else { Some(arg) }
    } else {
        None // Command doesn't match
    }
}

/// Context submenu provider for the :ctx command
pub struct ContextSubmenu {
    /// Current context name
    pub current_context: String,
}

impl ContextSubmenu {
    /// Create a new context submenu provider
    pub fn new(current_context: String) -> Self {
        Self { current_context }
    }
}

impl CommandSubmenu for ContextSubmenu {
    fn get_submenu(&self) -> Result<Option<SubmenuState>> {
        // Get available contexts from kubeconfig
        let contexts = crate::kube::list_contexts()?;

        if contexts.is_empty() {
            return Ok(None);
        }

        // Create submenu items, marking current context
        let items: Vec<SubmenuItem> = contexts
            .into_iter()
            .map(|ctx| {
                let display = if ctx == self.current_context {
                    format!("{} (current)", ctx)
                } else {
                    ctx.clone()
                };
                SubmenuItem::with_display(ctx, display)
            })
            .collect();

        // Create submenu state with title and help text
        let state = SubmenuState::new(submenu_ids::CONTEXT.to_string(), items)
            .with_title("Select Context".to_string())
            .with_help("j/k: Navigate | /: Filter | Enter: Select | Esc: Cancel".to_string());

        Ok(Some(state))
    }
}

/// Theme submenu provider for the :skin command
pub struct ThemeSubmenu {
    /// Current theme name
    pub current_theme: String,
}

impl ThemeSubmenu {
    /// Create a new theme submenu provider
    pub fn new(current_theme: String) -> Self {
        Self { current_theme }
    }
}

impl CommandSubmenu for ThemeSubmenu {
    fn get_submenu(&self) -> Result<Option<SubmenuState>> {
        // Get available themes (includes embedded + user-installed)
        let themes = crate::config::theme_loader::ThemeLoader::list_themes();

        if themes.is_empty() {
            return Ok(None);
        }

        // Create submenu items, marking current theme
        let items: Vec<SubmenuItem> = themes
            .into_iter()
            .map(|theme| {
                let display = if theme == self.current_theme {
                    format!("{} (current)", theme)
                } else {
                    theme.clone()
                };
                // Mark embedded themes
                let display = if crate::config::embedded_themes::is_embedded_theme(&theme) {
                    format!("{} [built-in]", display)
                } else {
                    display
                };
                SubmenuItem::with_display(theme, display)
            })
            .collect();

        // Create submenu state with title and help text
        let state = SubmenuState::new(submenu_ids::SKIN.to_string(), items)
            .with_title("Select Theme".to_string())
            .with_help(
                "j/k: Navigate | /: Filter | Enter: Apply | s: Save | Esc: Cancel".to_string(),
            );

        Ok(Some(state))
    }
}

/// Get submenu for a command if it supports submenus
///
/// Build the controller pod submenu for `:logs` from the watched controller
/// pods (shown with their readiness so a crashing controller stands out).
pub fn logs_submenu(
    controller_pods: &[crate::tui::app::state::ControllerPodInfo],
    no_icons: bool,
) -> Option<SubmenuState> {
    if controller_pods.is_empty() {
        return None;
    }

    let mut pods: Vec<_> = controller_pods.to_vec();
    pods.sort_by(|a, b| a.name.cmp(&b.name));

    let items: Vec<SubmenuItem> = pods
        .into_iter()
        .map(|pod| {
            // Readiness marker follows the list view's icon convention,
            // with text alternatives when icons are disabled.
            let marker = match (pod.ready, no_icons) {
                (true, false) => "●",
                (false, false) => "○",
                (true, true) => "OK ",
                (false, true) => "ERR",
            };
            let display = format!("{} {}", marker, pod.name);
            SubmenuItem::with_display(pod.name, display)
        })
        .collect();

    Some(
        SubmenuState::new(submenu_ids::CONTROLLER_LOGS.to_string(), items)
            .with_title("Controller Logs".to_string())
            .with_help("j/k: Navigate | /: Filter | Enter: Stream logs | Esc: Cancel".to_string()),
    )
}

/// The `:ns` picker. `namespaces` arrive in display order (see
/// `App::namespace_picker_options`); namespaces in `flux_namespaces` get a
/// marker so it's clear where Flux resources live.
pub fn namespace_submenu(
    namespaces: &[String],
    current: &Option<String>,
    flux_namespaces: &std::collections::HashSet<String>,
    no_icons: bool,
) -> Option<SubmenuState> {
    if namespaces.is_empty() {
        return None;
    }
    let marker = if no_icons { "[flux]" } else { "◆ flux" };

    let items: Vec<SubmenuItem> = namespaces
        .iter()
        .map(|ns| {
            let is_current = if ns == "all" {
                current.is_none()
            } else {
                current.as_deref() == Some(ns.as_str())
            };
            let mut display = ns.clone();
            if flux_namespaces.contains(ns) {
                display.push_str(&format!("  {marker}"));
            }
            if is_current {
                display.push_str(" (current)");
            }
            SubmenuItem::with_display(ns.clone(), display)
        })
        .collect();

    Some(
        SubmenuState::new(submenu_ids::NAMESPACE.to_string(), items)
            .with_title("Select Namespace".to_string())
            .with_help("j/k: Navigate | /: Filter | Enter: Select | Esc: Cancel".to_string()),
    )
}

/// Returns None if the command doesn't support submenus or if an argument was provided.
pub fn get_command_submenu(
    cmd: &str,
    current_context: &str,
    current_theme: &str,
) -> Option<SubmenuState> {
    // Only show submenu if command has no arguments
    if is_context_command(cmd) {
        let arg = extract_command_arg(cmd, "ctx").or_else(|| extract_command_arg(cmd, "context"));

        // If argument provided, don't show submenu (preserve existing behavior)
        if arg.is_some() {
            return None;
        }

        // Create context submenu
        let submenu_provider = ContextSubmenu::new(current_context.to_string());
        match submenu_provider.get_submenu() {
            Ok(Some(state)) => Some(state),
            _ => None,
        }
    } else if is_skin_command(cmd) {
        let arg = extract_command_arg(cmd, "skin");

        // If argument provided, don't show submenu (preserve existing behavior)
        if arg.is_some() {
            return None;
        }

        // Create theme submenu
        let submenu_provider = ThemeSubmenu::new(current_theme.to_string());
        match submenu_provider.get_submenu() {
            Ok(Some(state)) => Some(state),
            _ => None,
        }
    } else {
        // Other commands don't support submenus yet
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_args_take_all_a_target_and_a_namespace() {
        let parse = |cmd| parse_sweep_args(cmd).unwrap();
        assert_eq!(parse("unmanaged"), SweepArgs::default());
        let all = parse("unmanaged all -A");
        assert!(all.include_other);
        assert_eq!(all.namespace, Some(None));
        assert_eq!(
            parse("orphaned -n apps").namespace,
            Some(Some("apps".into()))
        );
        assert_eq!(parse("orphaned --namespace=all").namespace, Some(None));
        let target = parse("orphaned ks/apps -n flux-system");
        assert_eq!(target.target, Some(("ks".into(), "apps".into())));
        assert!(parse_sweep_args("orphaned a/b c/d").is_err());
        assert!(parse_sweep_args("unmanaged -n").is_err());
        assert!(parse_sweep_args("unmanaged -n a -n b").is_err());
        assert!(parse_sweep_args("unmanaged bogus").is_err());
        assert!(is_unmanaged_command("unmanaged all") && !is_unmanaged_command("unhealthy"));
        assert!(is_orphaned_command("orphaned") && is_orphaned_command("orphans ks/x"));
    }

    #[test]
    fn namespace_flags_split_off_views_and_kind_sets() {
        let split = |cmd: &str| split_namespace_flag(cmd);
        let ok = |head: &str, ns: &str| Some(Ok((head.to_string(), ns.to_string())));
        assert_eq!(split("apps"), None, "no flag");
        assert_eq!(split("ks,hr -n apps"), ok("ks,hr", "apps"));
        assert_eq!(
            split("-n apps ks, hr"),
            ok("ks,hr", "apps"),
            "kind sets may have spaces"
        );
        assert_eq!(split("apps --namespace=prod"), ok("apps", "prod"));
        assert_eq!(split("apps --namespace prod"), ok("apps", "prod"));
        assert!(matches!(split("apps -n"), Some(Err(_))));
        assert!(matches!(split("-n prod"), Some(Err(_))));
    }

    #[test]
    fn trace_args_take_kubectl_style_namespaces() {
        let parsed = |cmd: &str| parse_trace_args(cmd);
        let target = |kind: &str, name: &str| Some((kind.to_string(), name.to_string()));
        assert_eq!(
            parsed("trace"),
            Ok(TraceArgs {
                target: None,
                namespace: None
            })
        );
        for cmd in [
            "trace deploy/web -n apps",
            "trace -n apps deploy/web",
            "trace deploy/web --namespace apps",
            "trace deploy/web --namespace=apps",
        ] {
            assert_eq!(
                parsed(cmd),
                Ok(TraceArgs {
                    target: target("deploy", "web"),
                    namespace: Some("apps".into()),
                }),
                "{cmd}"
            );
        }
        for bad in [
            "trace deploy",
            "trace deploy/a/b",
            "trace /web",
            "trace deploy/web extra/thing",
            "trace deploy/web -n a -n b",
            "trace deploy/web --namespace=",
            "trace deploy/web -A",
        ] {
            assert_eq!(parsed(bad), Err(TRACE_USAGE.to_string()), "{bad}");
        }
        assert!(parsed("trace deploy/web -n").is_err());
        assert!(
            parsed("trace -n apps")
                .unwrap_err()
                .contains("<kind>/<name>")
        );
    }

    #[test]
    fn test_theme_submenu_creation() {
        let submenu = ThemeSubmenu::new("default".to_string());
        let result = submenu.get_submenu();
        assert!(result.is_ok(), "Should create theme submenu successfully");

        let submenu_state = result.unwrap();
        assert!(submenu_state.is_some(), "Should have submenu state");

        let state = submenu_state.unwrap();
        assert_eq!(state.command, "skin");
        assert_eq!(state.title, Some("Select Theme".to_string()));
        assert!(!state.items.is_empty(), "Should have theme items");
    }

    #[test]
    fn test_theme_submenu_marks_current() {
        let current_theme = "default".to_string();
        let submenu = ThemeSubmenu::new(current_theme.clone());
        let result = submenu.get_submenu().unwrap();

        assert!(result.is_some());
        let state = result.unwrap();

        // Find the current theme item
        let current_item = state.items.iter().find(|item| item.value == current_theme);

        assert!(current_item.is_some(), "Should find current theme in items");
        let item = current_item.unwrap();
        assert!(
            item.display_text.contains("(current)"),
            "Current theme should be marked"
        );
    }

    #[test]
    fn test_theme_submenu_marks_embedded() {
        // Use an embedded theme as current
        let current_theme = "dracula".to_string();
        let submenu = ThemeSubmenu::new(current_theme.clone());
        let result = submenu.get_submenu().unwrap();

        assert!(result.is_some());
        let state = result.unwrap();

        // Find dracula theme item
        if let Some(item) = state.items.iter().find(|item| item.value == "dracula") {
            assert!(
                item.display_text.contains("[built-in]"),
                "Embedded theme should be marked as [built-in]"
            );
        }
    }

    #[test]
    fn test_theme_submenu_includes_embedded_themes() {
        let submenu = ThemeSubmenu::new("default".to_string());
        let result = submenu.get_submenu().unwrap();

        assert!(result.is_some());
        let state = result.unwrap();

        // Should include at least some embedded themes
        let theme_names: Vec<&String> = state.items.iter().map(|item| &item.value).collect();
        let dracula_str = "dracula".to_string();
        let nord_str = "nord".to_string();
        assert!(
            theme_names.contains(&&dracula_str) || theme_names.contains(&&nord_str),
            "Should include embedded themes"
        );
    }

    #[test]
    fn test_get_command_submenu_skin_no_arg() {
        // Test that skin without args returns submenu
        // Note: commands are processed without the ':' prefix
        let result = get_command_submenu("skin", "context1", "default");
        assert!(
            result.is_some(),
            "Should return submenu for skin without args"
        );

        let submenu = result.unwrap();
        assert_eq!(submenu.command, "skin");
    }

    #[test]
    fn test_get_command_submenu_skin_with_arg() {
        // Test that skin with args doesn't return submenu
        let result = get_command_submenu("skin dracula", "context1", "default");
        assert!(
            result.is_none(),
            "Should not return submenu when arg provided"
        );
    }

    #[test]
    fn test_get_command_submenu_skin_variations() {
        // Test various skin command formats (without ':' prefix as commands are processed)
        assert!(get_command_submenu("skin", "context1", "default").is_some());
        assert!(get_command_submenu("skin ", "context1", "default").is_some());
        assert!(get_command_submenu("skin dracula", "context1", "default").is_none());
    }

    #[test]
    fn test_all_and_clear_appear_in_autocomplete() {
        // :all and :clear must be discoverable via autocomplete so users can find them
        let all_matches = find_matching_commands("al");
        assert!(
            all_matches.contains(&"all".to_string()),
            ":all should appear in autocomplete for prefix 'al'"
        );

        let clear_matches = find_matching_commands("cl");
        assert!(
            clear_matches.contains(&"clear".to_string()),
            ":clear should appear in autocomplete for prefix 'cl'"
        );
    }

    #[test]
    fn test_all_and_clear_are_clear_commands() {
        assert!(is_all_command("all"));
        assert!(is_all_command("clear"));
        assert!(is_all_command("ALL"));
        assert!(is_all_command("CLEAR"));
        assert!(!is_all_command("ks"));
    }

    #[test]
    fn test_discover_command_matches_and_autocompletes() {
        assert!(is_discover_command("discover"));
        assert!(is_discover_command("DISCOVERY"));
        assert!(!is_discover_command("discoverfluxresources"));
        assert!(!is_discover_command("ks"));

        // Discoverable from autocomplete like the other app commands
        assert!(find_matching_commands("disc").contains(&"discover".to_string()));
    }

    #[test]
    fn test_is_logs_command_with_and_without_args() {
        assert!(is_logs_command("logs"));
        assert!(is_logs_command("log"));
        assert!(is_logs_command("LOGS source-controller"));
        assert!(is_logs_command("log source-controller"));
        assert!(!is_logs_command("logstash"));
        assert!(!is_logs_command("ks"));
    }

    fn controller_pod(name: &str, ready: bool) -> crate::tui::app::state::ControllerPodInfo {
        crate::tui::app::state::ControllerPodInfo {
            name: name.to_string(),
            ready,
            version: None,
        }
    }

    #[test]
    fn test_namespace_submenu_marks_current_and_all() {
        assert!(
            namespace_submenu(&[], &None, &Default::default(), false).is_none(),
            "no namespaces means no submenu"
        );

        let namespaces = vec![
            "all".to_string(),
            "flux-system".to_string(),
            "cert-manager".to_string(),
        ];

        // A specific namespace is current: it is marked, "all" is not.
        let current = Some("flux-system".to_string());
        let submenu = namespace_submenu(&namespaces, &current, &Default::default(), false)
            .expect("namespaces make a submenu");
        assert_eq!(submenu.command, "ns");
        assert_eq!(submenu.title, Some("Select Namespace".to_string()));
        // Values stay the bare namespace names for the :ns dispatch.
        assert_eq!(submenu.items[0].value, "all");
        assert_eq!(submenu.items[1].value, "flux-system");
        assert!(!submenu.items[0].display_text.contains("(current)"));
        assert!(submenu.items[1].display_text.contains("(current)"));

        // Cluster-wide scope (None) marks "all" as current.
        let submenu = namespace_submenu(&namespaces, &None, &Default::default(), false).unwrap();
        assert!(submenu.items[0].display_text.contains("(current)"));
        assert!(!submenu.items[1].display_text.contains("(current)"));
    }

    #[test]
    fn test_namespace_submenu_marks_flux_namespaces() {
        let namespaces = vec![
            "all".to_string(),
            "flux-system".to_string(),
            "kube-system".to_string(),
        ];
        let flux = std::collections::HashSet::from(["flux-system".to_string()]);
        let submenu = namespace_submenu(&namespaces, &None, &flux, false).unwrap();
        assert_eq!(submenu.items[1].display_text, "flux-system  ◆ flux");
        assert_eq!(submenu.items[2].display_text, "kube-system");
        assert_eq!(
            submenu.items[1].value, "flux-system",
            "value stays the bare name"
        );
        let plain = namespace_submenu(&namespaces, &None, &flux, true).unwrap();
        assert_eq!(plain.items[1].display_text, "flux-system  [flux]");
    }

    #[test]
    fn test_logs_submenu_sorts_pods_and_marks_readiness() {
        assert!(
            logs_submenu(&[], false).is_none(),
            "no pods means no submenu"
        );

        let pods = vec![
            controller_pod("source-controller-b", true),
            controller_pod("helm-controller-a", false),
        ];
        let submenu = logs_submenu(&pods, false).expect("pods produce a submenu");
        assert_eq!(submenu.command, "logs");
        // Sorted by name; value is the bare pod name for the :logs dispatch
        assert_eq!(submenu.items[0].value, "helm-controller-a");
        assert_eq!(submenu.items[1].value, "source-controller-b");
        assert!(submenu.items[0].display_text.starts_with('○'));
        assert!(submenu.items[1].display_text.starts_with('●'));

        // no_icons swaps the markers for text alternatives
        let submenu = logs_submenu(&pods, true).unwrap();
        assert!(submenu.items[0].display_text.starts_with("ERR"));
        assert!(submenu.items[1].display_text.starts_with("OK"));
    }

    #[test]
    fn native_kind_completions_follow_flux_and_app_commands() {
        let tokens = vec![
            "deployments".to_string(),
            "deploy".to_string(),
            "deploy".to_string(), // same short name from two groups
            "daemonsets".to_string(),
            "ks".to_string(),   // shadowed by the Flux alias
            "flux".to_string(), // shadowed by the app command
        ];
        let flux = vec!["ks".to_string()];
        let app = vec!["flux".to_string()];
        assert_eq!(
            native_kind_matches("de", tokens.clone(), &flux, &app),
            ["deploy", "deployments"]
        );
        assert!(native_kind_matches("k", tokens.clone(), &flux, &app).is_empty());
        assert!(native_kind_matches("fl", tokens, &flux, &app).is_empty());
    }
}
