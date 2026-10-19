//! Combined kind filters and user-defined views (#302).
//!
//! `:ks,hr` narrows the Flux list to several kinds at once, and
//! `views.<name>.kinds` in the config names such a set: `:<name>` opens it.
//! Precedence in `:` dispatch: flux9s commands, then views, then comma
//! lists, then kinds — so a view can stand in for a kind name, and
//! [`view_problems`] warns when one does.
//!
//! `-c` / `--command` runs any `:` command once flux9s has connected, and
//! `defaultResourceFilter` takes a view name or a kind list.

use super::core::App;
use super::state::View;
use crate::config::ViewConfig;
use crate::models::kind_filter::KindFilter;
use crate::models::kinds::{self, Family};
use std::collections::BTreeMap;

impl App {
    /// Show `filter` in the Flux list — `:ks`, `:ks,hr`, or a view.
    pub(super) fn show_kinds(&mut self, filter: KindFilter) {
        if self.view_state.current_view == View::KindList {
            self.go_home_flux();
        }
        self.view_state.kind_filter = filter;
        self.reset_list_position();
        self.invalidate_layout_cache(); // The kind filter is shown in the header
    }

    /// The configured view called `name` (case-insensitive), with its
    /// configured name.
    pub(super) fn view_named(&self, name: &str) -> Option<(String, ViewConfig)> {
        self.config
            .views
            .iter()
            .find(|(view, _)| view.eq_ignore_ascii_case(name))
            .map(|(view, config)| (view.clone(), config.clone()))
    }

    /// `:<view>`: the view's kinds, titled with its name. Kinds resolve
    /// when the view opens, so Flux-adjacent kinds work once discovered.
    pub(super) fn open_view(&mut self, name: &str, view: &ViewConfig) {
        match KindFilter::parse(&view.kinds.join(",")) {
            Ok(filter) => self.show_kinds(filter.named(name)),
            Err(e) => self.set_status_message((format!("View '{name}': {e}"), true)),
        }
    }

    /// `:ks,hr`: several Flux kinds in one list.
    pub(super) fn cmd_kind_set(&mut self, spec: &str) {
        match KindFilter::parse(spec) {
            Ok(filter) => self.show_kinds(filter),
            Err(e) => self.set_status_message((e, true)),
        }
    }

    /// `defaultResourceFilter` at startup: a view name, or one or more
    /// kinds. `Err` carries the warning to show.
    pub fn apply_default_filter(&mut self, value: &str) -> Result<(), String> {
        let filter = match self.view_named(value) {
            Some((name, view)) => KindFilter::parse(&view.kinds.join(","))
                .map(|filter| filter.named(name))
                .map_err(|e| format!("defaultResourceFilter: view '{value}': {e}"))?,
            // A kind flux9s learns at runtime (Flux-adjacent) is kept as
            // written; it matches once its resources arrive.
            None => KindFilter::parse_allowing_unknown(value)
                .map(|(filter, _)| filter)
                .map_err(|e| format!("defaultResourceFilter '{value}': {e}"))?,
        };
        self.view_state.kind_filter = filter;
        self.invalidate_layout_cache();
        Ok(())
    }

    /// `-c` / `--command`: run a `:` command as if typed, once connected.
    /// A Kubernetes kind waits for discovery like a typed one. Returns
    /// whether it asked to quit (`-c q`), as a typed `:q` would.
    pub fn run_startup_command(&mut self, command: &str) -> bool {
        self.ui_state.command_buffer = command.trim().trim_start_matches(':').to_string();
        let quit = self.execute_command() == Some(true);
        self.ui_state.command_buffer.clear();
        self.ui_state.command_mode = false;
        quit
    }

    /// `(":name", "Kustomization + HelmRelease")` for each view — the help
    /// overlay's entries.
    pub(crate) fn view_help(&self) -> Vec<(String, String)> {
        self.config
            .views
            .iter()
            .map(|(name, view)| (format!(":{name}"), view.kinds.join(", ")))
            .collect()
    }

    /// After kind discovery: warn about views named like a kind the cluster
    /// serves (not knowable before connecting) — Kubernetes kinds and
    /// Flux-adjacent ones. Once per name per session, not on every
    /// rediscovery; names the static check reported aren't repeated.
    pub(crate) fn warn_views_shadowing_discovered_kinds(&mut self) {
        let found: Vec<(String, String)> = self
            .config
            .views
            .keys()
            .filter(|name| !kinds::looks_like_kubernetes_kind(name))
            .filter(|name| crate::watcher::get_display_name_for_command(name).is_none())
            .filter(|name| !self.view_shadows_warned.contains(&name.to_lowercase()))
            .filter_map(|name| {
                let spec = kinds::resolve(name)?;
                (!matches!(spec.family, Family::Flux(_))).then(|| {
                    (
                        name.to_lowercase(),
                        format!(
                            "view '{name}' hides the {} kind — reach it as :{}",
                            spec.names.kind,
                            other_name(&spec, name)
                        ),
                    )
                })
            })
            .collect();
        let shadowing: Vec<String> = found
            .into_iter()
            .map(|(name, warning)| {
                self.view_shadows_warned.insert(name);
                warning
            })
            .collect();
        if let Some(first) = shadowing.first() {
            tracing::warn!("Views shadowing discovered kinds: {}", shadowing.join("; "));
            self.set_status_message((first.clone(), true));
        }
    }
}

/// A name for `spec` that the view called `hidden` doesn't shadow: another
/// of its names, else kubectl's `plural.group`.
fn other_name(spec: &kinds::KindSpec, hidden: &str) -> String {
    let hidden = hidden.to_lowercase();
    spec.names
        .tokens()
        .find(|token| *token != hidden)
        .unwrap_or_else(|| format!("{}.{}", spec.names.plural.to_lowercase(), spec.gvk.group))
}

/// What's wrong with the configured views — for the startup warning and
/// `flux9s config`. Checks what's knowable without a cluster: names, kinds,
/// and collisions with flux9s commands, Flux kinds, and common Kubernetes
/// kind names. (Kinds a cluster serves are checked after discovery.)
pub fn view_problems(views: &BTreeMap<String, ViewConfig>) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, view) in views {
        let lower = name.to_lowercase();
        if lower.is_empty() || lower.contains(|c: char| c.is_whitespace() || ",:/".contains(c)) {
            problems.push(format!(
                "view '{name}': names can't contain spaces, commas, colons, or slashes"
            ));
            continue;
        }
        if super::command_mode::is_builtin_command(&lower) {
            problems.push(format!(
                "view '{name}' can't be opened: :{lower} is a flux9s command — rename the view"
            ));
            continue;
        }
        match kinds::resolve(&lower) {
            Some(spec) if spec.family != Family::Native => problems.push(format!(
                "view '{name}' hides :{lower} for {} — the kind's other names still work",
                spec.names.kind
            )),
            _ if kinds::looks_like_kubernetes_kind(&lower) => problems.push(format!(
                "view '{name}' hides the Kubernetes kind name :{lower} — reach the kind by another name"
            )),
            _ => {}
        }
        match KindFilter::parse_allowing_unknown(&view.kinds.join(",")) {
            Err(e) => problems.push(format!("view '{name}': {e}")),
            // Before discovery (and in the CLI) a Flux-adjacent kind is
            // unknown too, so this is a note, not a verdict.
            Ok((_, unknown)) => problems.extend(unknown.into_iter().map(|kind| {
                format!(
                    "view '{name}': '{kind}' isn't a Flux kind flux9s knows — fine if it's a \
                     Flux-adjacent kind (discoverFluxResources), else a typo"
                )
            })),
        }
    }
    // Views match case-insensitively: `Apps` and `apps` are one name.
    let names: Vec<&String> = views.keys().collect();
    for (i, name) in names.iter().enumerate() {
        if let Some(other) = names[..i].iter().find(|o| o.eq_ignore_ascii_case(name)) {
            problems.push(format!(
                "views '{other}' and '{name}' differ only in case — :{} opens '{other}'",
                name.to_lowercase()
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn views(entries: &[(&str, &[&str])]) -> BTreeMap<String, ViewConfig> {
        entries
            .iter()
            .map(|(name, kinds)| {
                (
                    name.to_string(),
                    ViewConfig {
                        kinds: kinds.iter().map(|k| k.to_string()).collect(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn views_differing_only_in_case_collide() {
        let problems = view_problems(&views(&[("Apps", &["ks"]), ("apps", &["hr"])]));
        assert_eq!(
            problems,
            ["views 'Apps' and 'apps' differ only in case — :apps opens 'Apps'"]
        );
    }

    #[test]
    fn a_good_view_has_no_problems() {
        assert!(view_problems(&views(&[("apps", &["ks", "hr"])])).is_empty());
    }

    #[test]
    fn views_named_like_commands_kinds_or_with_bad_kinds_are_reported() {
        let problems = view_problems(&views(&[
            ("events", &["ks"]),
            ("ks", &["ks", "hr"]),
            ("po", &["hr"]),
            ("bad name", &["ks"]),
            ("mixed", &["ks", "deploy"]),
            ("typo", &["kss"]),
            ("empty", &[]),
        ]));
        let has = |needle: &str| problems.iter().any(|p| p.contains(needle));
        assert!(
            has("view 'events' can't be opened: :events is a flux9s command"),
            "{problems:#?}"
        );
        assert!(
            has("view 'ks' hides :ks for Kustomization"),
            "{problems:#?}"
        );
        assert!(
            has("view 'po' hides the Kubernetes kind name :po"),
            "{problems:#?}"
        );
        assert!(has("view 'bad name': names can't contain"), "{problems:#?}");
        assert!(
            has("view 'mixed': 'deploy' is a Kubernetes kind"),
            "{problems:#?}"
        );
        assert!(
            has("view 'typo': 'kss' isn't a Flux kind flux9s knows"),
            "{problems:#?}"
        );
        assert!(has("view 'empty': No kinds given"), "{problems:#?}");
    }
}
