//! `:` commands: the data-driven [`COMMAND_TABLE`] and its handlers.

use super::core::App;
use super::state::{HealthFilter, TraceRequest, TraceResponse, View};
use crate::kube::objects::ObjectRef;
use crate::models::FluxResourceKind;
use crate::models::kind_filter::KindFilter;
use crate::tui::commands;
use crate::watcher::ResourceKey;

/// A `:` command handler dispatched from [`App::execute_command`]. Receives the
/// app and the original (case-preserving) command string.
type CommandHandler = fn(&mut App, &str);

/// A uniform `:` command: predicate over the lowercased command, handler,
/// and its help lines (the help overlay reads them from here).
pub(super) struct CommandSpec {
    pub matches: fn(&str) -> bool,
    pub run: CommandHandler,
    pub help: &'static [(&'static str, &'static str)],
}

/// Data-driven dispatch for the uniform `:` commands. Checked in order; the
/// first matching predicate wins. Special commands (help/quit/readonly, the
/// connection gate) and the resource-type fallback are handled directly in
/// [`App::execute_command`]; their help is [`SPECIAL_COMMAND_HELP`].
pub(super) const COMMAND_TABLE: &[CommandSpec] = &[
    CommandSpec {
        matches: commands::is_skin_command,
        run: App::cmd_set_skin,
        help: &[
            (":skin <n>", "Change theme/skin"),
            (":skin", "Open theme picker"),
        ],
    },
    CommandSpec {
        matches: commands::is_trace_command,
        run: App::cmd_trace,
        help: &[(
            ":trace <kind>/<n> [-n ns]",
            "Trace an object to its manager",
        )],
    },
    CommandSpec {
        matches: commands::is_context_command,
        run: App::cmd_switch_context,
        help: &[
            (":ctx <n>", "Switch context"),
            (":ctx", "Open context submenu"),
        ],
    },
    CommandSpec {
        matches: commands::is_namespace_command,
        run: App::cmd_switch_namespace,
        help: &[
            (":ns <n>", "Switch namespace"),
            (":ns", "Open namespace picker"),
            (":ns all", "Show all namespaces"),
        ],
    },
    CommandSpec {
        matches: commands::is_unmanaged_command,
        run: App::cmd_unmanaged,
        help: &[(
            ":unmanaged [-n ns|-A]",
            "What Flux doesn't manage (o/u/a in it)",
        )],
    },
    CommandSpec {
        matches: commands::is_orphaned_command,
        run: App::cmd_orphaned,
        help: &[
            (":orphaned [-n ns|-A]", "What Flux left behind"),
            (":orphaned <kind>/<n>", "What one Flux object left behind"),
        ],
    },
    CommandSpec {
        matches: commands::is_healthy_command,
        run: App::cmd_filter_healthy,
        help: &[(":healthy", "Show healthy resources")],
    },
    CommandSpec {
        matches: commands::is_unhealthy_command,
        run: App::cmd_filter_unhealthy,
        help: &[(":unhealthy", "Show unhealthy resources")],
    },
    CommandSpec {
        matches: commands::is_suspended_command,
        run: App::cmd_filter_suspended,
        help: &[(":suspended", "Show suspended resources")],
    },
    CommandSpec {
        matches: commands::is_favorites_command,
        run: App::cmd_show_favorites,
        help: &[(":favorites", "View favorites"), (":fav", "View favorites")],
    },
    CommandSpec {
        matches: commands::is_events_command,
        run: App::cmd_show_events,
        help: &[(":events", "Live Kubernetes events feed")],
    },
    CommandSpec {
        matches: commands::is_pulse_command,
        run: App::cmd_show_pulse,
        help: &[(":pulse", "Cluster health dashboard")],
    },
    CommandSpec {
        matches: commands::is_logs_command,
        run: App::cmd_show_logs,
        help: &[(":logs [pod]", "Stream controller logs")],
    },
    CommandSpec {
        matches: commands::is_flux_command,
        run: App::cmd_show_flux,
        help: &[(":flux", "Back to the Flux view")],
    },
    CommandSpec {
        matches: commands::is_all_command,
        run: App::cmd_show_all,
        help: &[(":all", "Show all resources")],
    },
    CommandSpec {
        matches: commands::is_discover_command,
        run: App::cmd_toggle_discover,
        help: &[(":discover", "Toggle CRD discovery")],
    },
    CommandSpec {
        matches: commands::is_native_command,
        run: App::cmd_toggle_native,
        help: &[(":native", "Toggle kind browsing (preview)")],
    },
];

/// Help for the commands [`App::execute_command`] handles outside the table.
pub(super) const SPECIAL_COMMAND_HELP: &[(&str, &str)] = &[
    (":help", "Show/hide help"),
    (":readonly", "Toggle readonly mode"),
    (":<kind>", "Browse any kind (:deploy, :po)"),
    (":q", "Quit application"),
];

/// A namespace as `:ns` reads it — `all` / `-A` mean every namespace. The
/// one interpretation for `:ns`, a view's `namespace`, and `-n` (#306).
pub(super) fn namespace_arg(value: &str) -> Option<String> {
    match value.trim() {
        "all" | "-A" => None,
        name => Some(name.to_string()),
    }
}

/// Whether `name` is a flux9s `:` command — the special ones or any entry
/// of [`COMMAND_TABLE`]. A view named like one could never be opened (#302).
pub(crate) fn is_builtin_command(name: &str) -> bool {
    let name = name.to_lowercase();
    commands::is_help_command(&name)
        || commands::is_quit_command(&name)
        || commands::is_readonly_command(&name)
        || COMMAND_TABLE.iter().any(|spec| (spec.matches)(&name))
}

/// Every `:` command's help line: the special ones around the table's.
pub(crate) fn command_help() -> Vec<(&'static str, &'static str)> {
    let (head, tail) = SPECIAL_COMMAND_HELP.split_at(2);
    head.iter()
        .copied()
        .chain(
            COMMAND_TABLE
                .iter()
                .flat_map(|spec| spec.help.iter().copied()),
        )
        .chain(tail.iter().copied())
        .collect()
}

/// The kind `:trace <kind>/…` refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TraceKind {
    pub kind: String,
    /// Set for native kinds (resolved by `apiVersion`); `None` for Flux kinds
    /// and built-in kind names, resolved by name when the trace runs.
    pub api_version: Option<String>,
    pub namespaced: bool,
}

/// Resolve `:trace <kind>/…` through the kind registry like `:<kind>` —
/// Flux aliases (`ks`, `hr`), Flux-adjacent kinds, and (with
/// `nativeResources`) any discovered kind by plural or short name. An exact
/// kind name passes through unchanged. `None` when the token is a
/// Kubernetes short name that needs discovery, which is off.
pub(super) fn trace_kind_ref(token: &str, native_resources: bool) -> Option<TraceKind> {
    if let Some(spec) = crate::models::kinds::resolve(token) {
        let flux = FluxResourceKind::parse_optional(&spec.names.kind).is_some();
        return Some(TraceKind {
            api_version: (!flux).then(|| spec.gvk.api_version()),
            namespaced: spec.is_namespaced(),
            kind: spec.names.kind,
        });
    }
    let by_name = |kind: &str| TraceKind {
        kind: kind.to_string(),
        api_version: None,
        namespaced: true,
    };
    if !native_resources && crate::models::kinds::looks_like_kubernetes_kind(token) {
        // An exact built-in kind name ("Deployment") still works without
        // discovery; aliases ("deploy") need it.
        let exact = token.chars().next().is_some_and(char::is_uppercase);
        return exact.then(|| by_name(token));
    }
    Some(by_name(token))
}

impl App {
    /// Execute the command currently in the command buffer.
    ///
    /// A few commands are special and handled up front: help/quit toggle global
    /// state, readonly runs before the connection gate, and the gate blocks most
    /// commands while disconnected. Everything else is dispatched through
    /// [`COMMAND_TABLE`] (a data-driven `(predicate, handler)` list); unmatched
    /// input falls through to resource-type selection or an "unknown command"
    /// message. Returns `Some(true)` only to quit.
    pub(super) fn execute_command(&mut self) -> Option<bool> {
        // Own the command string so the per-command handlers can take `&mut self`
        // without conflicting with a borrow of the command buffer.
        let cmd = self.ui_state.command_buffer.trim().to_string();
        let cmd_lower = cmd.to_lowercase();

        if commands::is_help_command(&cmd_lower) {
            self.toggle_help();
            return None;
        }
        if commands::is_quit_command(&cmd_lower) {
            return Some(true);
        }
        if commands::is_readonly_command(&cmd_lower) {
            self.cmd_toggle_readonly();
            return None;
        }

        // While disconnected, only context/skin commands are allowed through.
        if self.has_connection_error()
            && !commands::is_context_command(&cmd_lower)
            && !commands::is_skin_command(&cmd_lower)
        {
            self.set_status_message((
                "Commands are disabled when disconnected (except :ctx, :skin, :q)".to_string(),
                true,
            ));
            return None;
        }

        // Data-driven dispatch: first predicate to match owns the command.
        for CommandSpec {
            matches,
            run: handle,
            ..
        } in COMMAND_TABLE
        {
            if matches(&cmd_lower) {
                handle(self, &cmd);
                return None;
            }
        }

        // `<view | kinds | kind> -n <namespace>` (#306): checked first, then
        // the namespace switches once and it opens there.
        if let Some(parsed) = commands::split_namespace_flag(&cmd_lower) {
            match parsed {
                Ok((head, namespace)) => self.open_in_namespace(&head, &namespace),
                Err(message) => self.set_status_message((message, true)),
            }
            return None;
        }

        // User views (#302) come before kinds, so a view can stand in for a
        // kind name (startup warns when one does); then kind sets (`ks,hr`).
        if let Some((name, view)) = self.view_named(&cmd_lower) {
            self.open_view(&name, &view);
            return None;
        }
        if cmd_lower.contains(',') {
            self.cmd_kind_set(&cmd_lower);
            return None;
        }

        // Fallback: a kind (#267). Flux and Flux-adjacent kinds filter the
        // Flux list as before; any other served kind opens a kind list.
        match crate::models::kinds::resolve(&cmd_lower) {
            Some(spec) if spec.family == crate::models::kinds::Family::Native => {
                self.open_kind_list(spec);
            }
            Some(spec) => self.show_kinds(KindFilter::kind(spec.names.kind)),
            // Discovery still running (just connected): hold the command and
            // open it when the kinds land, instead of calling it unknown.
            None if !cmd.is_empty()
                && self.config.native_resources
                && self.async_state.kind_discovery.is_loading() =>
            {
                self.hold_kind_command(&cmd_lower);
            }
            // Preview off: a recognisable Kubernetes kind gets pointed at
            // :native instead of an unhelpful "unknown command".
            None if !cmd.is_empty()
                && !self.config.native_resources
                && crate::models::kinds::looks_like_kubernetes_kind(&cmd_lower) =>
            {
                self.set_status_message((
                    format!(
                        ":{cmd} is a Kubernetes kind — browsing kinds is a preview. \
                         Try it with :native, or: flux9s config set nativeResources true"
                    ),
                    true,
                ));
            }
            None if !cmd.is_empty() => {
                // A CRD installed after connecting isn't in the catalog yet:
                // re-check the cluster's kinds (throttled) so a retry finds it.
                let rechecking = self.refresh_kind_discovery();
                let hint = if !self.config.native_resources {
                    "Browsing Kubernetes kinds is a preview — :native turns it on"
                } else if rechecking {
                    "Re-checking the cluster's kinds — if it was just installed, try again in a moment"
                } else {
                    "Type :help for available commands"
                };
                self.set_status_message((format!("Unknown command: '{}'. {}", cmd, hint), true));
            }
            None => {}
        }

        None
    }

    /// Reset the list selection and scroll to the top. Shared by the commands
    /// that change what the list shows (namespace, filters, resource type).
    pub(super) fn reset_list_position(&mut self) {
        self.view_state.selected_index = 0;
        self.view_state.scroll_offset = 0;
    }

    /// Toggle read-only mode and reload the matching skin.
    pub(super) fn cmd_toggle_readonly(&mut self) {
        self.config.read_only = !self.config.read_only;
        let status = if self.config.read_only {
            "enabled"
        } else {
            "disabled"
        };
        // Clone context name to avoid borrowing self across the reload.
        let context_name = self.context.clone();
        self.reload_skin_for_readonly_mode(Some(&context_name));
        self.set_status_message((format!("Readonly mode {}", status), false));
    }

    /// `:discover` — arm or disarm CRD discovery for this session (#245).
    ///
    /// The sibling of `:readonly`: it flips `discoverFluxResources` in the
    /// in-memory config (so a later context switch builds its watcher with the
    /// same setting) without writing the config file. Enabling starts the CRD
    /// discovery watcher, which re-reports every labeled CRD. Disabling stops
    /// it, drops the discovered kinds' `:` commands, and purges their resources
    /// from the list.
    pub(super) fn cmd_toggle_discover(&mut self, _cmd: &str) {
        // The opt-out means no discovery or extra watches at all (#267).
        if !self.config.native_resources {
            self.set_status_message((
                "CRD discovery needs native resources (preview) — turn them on with :native first"
                    .to_string(),
                true,
            ));
            return;
        }
        let enable = !self.config.discover_flux_resources;
        let removed = match self.apply_flux_crd_discovery(enable) {
            Ok(removed) => removed,
            Err(e) => {
                self.set_status_message((format!("Failed to start CRD discovery: {}", e), true));
                return;
            }
        };
        self.config.discover_flux_resources = enable;
        let discovered_len = removed;
        let status = if enable {
            "CRD discovery enabled (session only) - watching for Flux-labeled CRDs".to_string()
        } else {
            format!(
                "CRD discovery disabled (session only) - removed {} discovered kind(s)",
                discovered_len
            )
        };
        self.set_status_message((status, false));
    }

    /// Start or stop Flux-adjacent CRD discovery on the watcher. Stopping
    /// also drops the discovered kinds and their rows. Returns how many kinds
    /// were removed. Shared by `:discover` and `:native`.
    pub(super) fn apply_flux_crd_discovery(&mut self, enable: bool) -> Result<usize, String> {
        // Captured before the registry is cleared — needed to purge the
        // discovered resources out of the list state.
        let discovered = crate::models::extra_kinds::global().kind_names();

        if let Some(watcher) = self.watcher.as_mut() {
            watcher
                .set_discovery_enabled(enable)
                .map_err(|e| e.to_string())?;
        }
        if enable {
            return Ok(0);
        }
        // Also covers the no-watcher case, where nothing else clears it.
        crate::models::extra_kinds::global().clear();
        for kind in &discovered {
            self.purge_kind(kind);
        }
        // A discovered kind can't stay selected once it's gone.
        if self
            .view_state
            .kind_filter
            .retain(|kind| !discovered.iter().any(|gone| gone == kind))
        {
            self.reset_list_position();
        }
        self.notify_resource_types_changed();
        Ok(discovered.len())
    }

    /// `:native` — turn native resource browsing (preview, #267) on or off
    /// for this session, like `:readonly` / `:discover`: the in-memory
    /// config flips (a later context switch keeps it), the file doesn't.
    pub(super) fn cmd_toggle_native(&mut self, _cmd: &str) {
        let enable = !self.config.native_resources;
        self.config.native_resources = enable;

        // Flux-adjacent CRD discovery follows too (it needs both flags).
        if self.config.discover_flux_resources
            && let Err(e) = self.apply_flux_crd_discovery(self.config.flux_crd_discovery_enabled())
        {
            tracing::warn!("CRD discovery toggle failed: {e}");
        }

        if enable {
            self.request_kind_discovery();
            self.request_namespace_list();
            self.set_status_message((
                "Native resources on (preview, session only) — try :deploy, :po, :svc. \
                 Keep it on with: flux9s config set nativeResources true"
                    .to_string(),
                false,
            ));
        } else {
            // Kind lists and sweeps need the preview: close them.
            let in_native_views = self.view_state.current_view.needs_native()
                || self
                    .view_state
                    .nav_stack
                    .iter()
                    .any(|entry| entry.view().needs_native());
            if in_native_views {
                self.go_home_flux();
            }
            self.async_state.kind_list.clear();
            self.async_state.mixed.clear();
            self.clear_sweep();
            self.async_state.kind_discovery.clear();
            self.async_state.namespace_list.clear();
            crate::models::kinds::catalog().clear();
            crate::kube::ownership::clear_api_cache();
            self.all_namespaces.clear();
            self.pending_kind_command = None;
            self.set_status_message((
                "Native resources off (session only) — flux9s is Flux-only again".to_string(),
                false,
            ));
        }
    }

    /// Open the interactive submenu for `cmd` if it has one (contexts, skins, …),
    /// previewing the first skin entry. Returns whether a submenu was opened, so
    /// callers can fall back to a status-message listing when it wasn't.
    pub(super) fn try_open_command_submenu(&mut self, cmd: &str) -> bool {
        let current_theme = self.config.resolve_skin_name(Some(&self.context));
        let Some(submenu) =
            crate::tui::commands::get_command_submenu(cmd, &self.context, &current_theme)
        else {
            return false;
        };
        // Skin submenus preview as the user scrolls; remember the original to
        // restore on cancel, and preview the first entry immediately.
        if submenu.command == crate::tui::commands::submenu_ids::SKIN {
            self.view_state.preview_original_theme = Some(current_theme.clone());
            if let Some(first_theme) = submenu.selected_value() {
                let _ = self.preview_theme(&first_theme);
            }
        }
        self.view_state.submenu_state = Some(submenu);
        true
    }

    /// `:skin [name]` — change the theme, or open the skin submenu / list themes.
    pub(super) fn cmd_set_skin(&mut self, cmd: &str) {
        match crate::tui::commands::extract_command_arg(cmd, "skin") {
            Some(name) => match self.set_theme(&name) {
                Ok(_) => self.set_status_message((format!("Theme changed to: {}", name), false)),
                Err(e) => self.set_status_message((
                    format!(
                        "Failed to load theme '{}': {}. Use `default` to return to default theme",
                        name, e
                    ),
                    true,
                )),
            },
            None if !self.try_open_command_submenu(cmd) => {
                let themes = crate::config::theme_loader::ThemeLoader::list_themes();
                let current = self.config.resolve_skin_name(Some(&self.context));
                self.set_status_message((
                    format!(
                        "Available themes: {}. Current: {}. Usage: :skin <theme-name>",
                        themes.join(", "),
                        current
                    ),
                    false,
                ));
            }
            None => {}
        }
    }

    /// `:trace [<kind>/<name> [-n <namespace>]]` — trace an object up to
    /// what manages it (#266), or what the current view shows. Without `-n`
    /// it looks in the current namespace; watching all namespaces, it
    /// searches them: one match is traced, several ask for `-n`.
    pub(super) fn cmd_trace(&mut self, cmd: &str) {
        let args = match commands::parse_trace_args(cmd) {
            Ok(args) => args,
            Err(message) => {
                self.set_status_message((message, true));
                return;
            }
        };
        let Some((kind_token, name)) = args.target else {
            self.trace_current_view();
            return;
        };
        let Some(kind) = trace_kind_ref(&kind_token, self.config.native_resources) else {
            self.set_status_message((
                format!(":trace {kind_token}/… needs kind lookup — :native turns it on"),
                true,
            ));
            return;
        };
        let namespace = if !kind.namespaced {
            Some(String::new())
        } else {
            args.namespace.or_else(|| self.namespace().clone())
        };
        let target = |namespace: String| match &kind.api_version {
            Some(api_version) => ObjectRef::native(api_version, &kind.kind, namespace, &name),
            None => ResourceKey::new(&kind.kind, namespace, &name).into(),
        };
        if let Some(namespace) = namespace {
            self.request_trace_of(target(namespace));
            return;
        }

        // All namespaces and no `-n`: find it. Flux kinds are all watched,
        // so the watched state answers; anything else asks the cluster.
        if kind.api_version.is_none() && FluxResourceKind::parse_optional(&kind.kind).is_some() {
            let mut namespaces: Vec<String> = self
                .state
                .all()
                .into_iter()
                .filter(|r| r.resource_type == kind.kind && r.name == name)
                .map(|r| r.namespace)
                .collect();
            namespaces.sort();
            match namespaces.as_slice() {
                [] => self.set_status_message((
                    format!("No {} named {name} in the watched namespaces", kind.kind),
                    true,
                )),
                [namespace] => self.request_trace_of(target(namespace.clone())),
                _ => {
                    let target = format!("{}/{name}", kind.kind);
                    self.set_trace_response(Ok(TraceResponse::Ambiguous { target, namespaces }));
                }
            }
            return;
        }
        // The object found becomes the subject when the trace answers.
        self.async_state.trace.request(TraceRequest::Search {
            kind: kind.kind.clone(),
            api_version: kind.api_version.clone(),
            name,
        });
    }

    /// `:trace` with no target: what `t` would trace here — the native
    /// object on screen (whatever manages it), else the selected Flux
    /// resource.
    fn trace_current_view(&mut self) {
        if let Some(native) = self.native_subject() {
            self.request_trace_of(native);
        } else if let Some(resource) = self.get_current_resource() {
            self.request_trace_of(
                ResourceKey::new(resource.resource_type, resource.namespace, resource.name).into(),
            );
        } else if let Some(rk) = self
            .selection_state
            .selected_resource_key
            .as_deref()
            .and_then(ResourceKey::parse)
        {
            self.request_trace_of(rk.into());
        } else {
            self.set_status_message(("No resource selected".to_string(), true));
        }
    }

    /// `:ctx [name]` — switch kube context, or open the context submenu / list
    /// available contexts.
    pub(super) fn cmd_switch_context(&mut self, cmd: &str) {
        let context_name = commands::extract_command_arg(cmd, "context")
            .or_else(|| commands::extract_command_arg(cmd, "ctx"));

        let Some(ctx) = context_name else {
            // No argument: open the submenu, or list contexts as a fallback.
            if !self.try_open_command_submenu(cmd) {
                match crate::kube::list_contexts() {
                    Ok(contexts) => {
                        let current = self.context.clone();
                        self.set_status_message((
                            format!(
                                "Available contexts: {}. Current: {}. Usage: :ctx <context-name>",
                                contexts.join(", "),
                                current
                            ),
                            false,
                        ));
                    }
                    Err(e) => {
                        self.set_status_message((format!("Failed to list contexts: {}", e), true))
                    }
                }
            }
            return;
        };

        // Mark the switch as pending; the main loop performs the reconnect.
        self.pending_context_switch = Some(ctx.to_string());
        self.set_status_message((format!("Switching to context '{}'...", ctx), false));
    }

    /// `:ns [name|all]` — switch the watched namespace, restarting watchers.
    ///
    /// Without an argument, opens the searchable namespace picker submenu;
    /// `:ns <name>` (or `:ns all` / `:ns -A`) switches directly.
    pub(super) fn cmd_switch_namespace(&mut self, cmd: &str) {
        let ns = commands::extract_command_arg(cmd, "namespace")
            .or_else(|| commands::extract_command_arg(cmd, "ns"));
        match ns.as_deref() {
            Some(name) => self.switch_namespace(namespace_arg(name)),
            None => {
                // No argument: open the searchable picker (same reusable submenu
                // as :ctx / :skin) instead of listing options in the bars.
                // Keep the cluster-wide list fresh for the next open.
                // An open picker updates in place when the list arrives.
                self.request_namespace_list();
                match self.build_namespace_picker() {
                    Some(submenu) => self.view_state.submenu_state = Some(submenu),
                    None => {
                        self.set_status_message(("No namespaces discovered yet".to_string(), true))
                    }
                }
            }
        }
    }

    /// Switch the watched namespace, restarting watchers. `None` watches all
    /// namespaces. Shared by `:ns <name>` and the namespace picker submenu.
    pub(super) fn switch_namespace(&mut self, new_namespace: Option<String>) {
        if self.namespace != new_namespace {
            self.namespace = new_namespace.clone();

            // Clear state; the restarted watchers repopulate it. Stale degraded
            // state from the old watcher set would otherwise never clear.
            self.state().clear();
            self.resource_objects.clear();
            self.controller_pods.clear();
            self.kube_events.clear();
            self.degraded_watchers.clear();
            self.forbidden_watchers.clear();

            if let Some(ref mut watcher) = self.watcher
                && let Err(e) = watcher.set_namespace(new_namespace)
            {
                tracing::warn!("Failed to switch namespace: {}", e);
                self.set_status_message((format!("Failed to switch namespace: {}", e), true));
            }
        }

        self.reset_list_position();
        // A kind list, and a mixed Flux list's kinds, follow the namespace.
        self.refresh_kind_list_namespace();
        self.sync_mixed_watch();
    }

    /// `:healthy` — filter the list to healthy resources.
    pub(super) fn cmd_filter_healthy(&mut self, _cmd: &str) {
        self.view_state.health_filter = HealthFilter::Healthy;
        self.reset_list_position();
        self.set_status_message(("Showing healthy resources only".to_string(), false));
    }

    /// `:unhealthy` — filter the list to unhealthy resources.
    pub(super) fn cmd_filter_unhealthy(&mut self, _cmd: &str) {
        self.view_state.health_filter = HealthFilter::Unhealthy;
        self.reset_list_position();
        self.set_status_message(("Showing unhealthy resources only".to_string(), false));
    }

    /// `:suspended` — filter the list to suspended resources.
    pub(super) fn cmd_filter_suspended(&mut self, _cmd: &str) {
        self.view_state.health_filter = HealthFilter::Suspended;
        self.reset_list_position();
        self.set_status_message(("Showing suspended resources only".to_string(), false));
    }

    /// `:favorites` — switch to the favorites view.
    pub(super) fn cmd_show_favorites(&mut self, _cmd: &str) {
        self.navigate_root(View::ResourceFavorites);
        self.reset_list_position();
    }

    /// `:events` — open the live Kubernetes events feed for the current
    /// namespace scope, lazily starting the events watcher.
    pub(super) fn cmd_show_events(&mut self, _cmd: &str) {
        self.navigate_root(View::EventList);
        self.start_kube_events_watch();
        self.reset_list_position();
    }

    /// `:pulse` — open the cluster pulse dashboard.
    pub(super) fn cmd_show_pulse(&mut self, _cmd: &str) {
        self.view_state.pulse_scroll_offset = 0;
        self.navigate_root(View::Pulse);
    }

    /// `:logs [pod]` — stream a Flux controller pod's logs. Without an
    /// argument, opens a submenu of the discovered controller pods; with one,
    /// matches a pod by exact name or unique prefix.
    pub(super) fn cmd_show_logs(&mut self, cmd: &str) {
        let arg = commands::extract_command_arg(cmd, "logs")
            .or_else(|| commands::extract_command_arg(cmd, "log"));
        let pods = self.controller_pods.get_all_pods();

        let Some(prefix) = arg else {
            match commands::logs_submenu(&pods, self.config.ui.no_icons) {
                Some(submenu) => self.view_state.submenu_state = Some(submenu),
                None => {
                    self.set_status_message(("No controller pods discovered yet".to_string(), true))
                }
            }
            return;
        };

        let matched = pods
            .iter()
            .find(|pod| pod.name == prefix)
            .map(|pod| pod.name.clone())
            .or_else(|| {
                let mut prefixed = pods.iter().filter(|pod| pod.name.starts_with(&prefix));
                match (prefixed.next(), prefixed.next()) {
                    (Some(only), None) => Some(only.name.clone()),
                    _ => None, // no match, or ambiguous prefix
                }
            });
        match matched {
            Some(pod) => self.open_log_view(&pod),
            None => {
                self.set_status_message((format!("No controller pod matching '{}'", prefix), true))
            }
        }
    }

    /// `:flux` — back to the Flux resource list, as it was left.
    pub(super) fn cmd_show_flux(&mut self, _cmd: &str) {
        self.go_home_flux();
    }

    /// `:all` — clear resource-type and health filters and show the Flux
    /// list, from whichever view it was typed in.
    pub(super) fn cmd_show_all(&mut self, _cmd: &str) {
        if !self.view_state.current_view.is_flux_list() {
            self.go_home_flux();
        }
        if !self.view_state.kind_filter.is_all() {
            self.view_state.kind_filter = KindFilter::all();
            self.sync_mixed_watch();
            self.invalidate_layout_cache(); // Resource type filter affects header display
        }
        if self.view_state.health_filter != HealthFilter::All {
            self.view_state.health_filter = HealthFilter::All;
            self.set_status_message(("Showing all resources".to_string(), false));
        }
        self.reset_list_position();
    }
}
