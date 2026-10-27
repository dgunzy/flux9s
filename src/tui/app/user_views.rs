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
use crate::config::ViewConfig;
use crate::models::kind_filter::KindFilter;
use crate::models::kinds::{self, Family};
use crate::models::list_sort::{ListSort, SortKey};
use std::collections::BTreeMap;

impl App {
    /// Show `filter` in the Flux list — `:ks`, `:ks,hr`, or a view — from
    /// whichever view the command was typed in.
    pub(super) fn show_kinds(&mut self, filter: KindFilter) {
        if !self.view_state.current_view.is_flux_list() {
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
        if let Err(e) = self.apply_view(name, view, false, None) {
            self.set_status_message((format!("View '{name}': {e}"), true));
        }
    }

    /// `<view | kinds | kind> -n <namespace>` (#306). Everything is checked
    /// before the namespace switches — once — so a broken view or a bad
    /// kind leaves the namespace and list as they were. `-n` wins over a
    /// view's own `namespace`.
    pub(super) fn open_in_namespace(&mut self, head: &str, namespace: &str) {
        let namespace = super::command_mode::namespace_arg(namespace);
        if let Some((name, view)) = self.view_named(head) {
            if let Err(e) = self.apply_view(&name, &view, false, Some(namespace)) {
                self.set_status_message((format!("View '{name}': {e}"), true));
            }
            return;
        }
        if head.contains(',') {
            match KindFilter::parse(head) {
                Ok(filter) => {
                    self.switch_namespace(namespace);
                    self.show_kinds(filter);
                }
                Err(e) => self.set_status_message((e, true)),
            }
            return;
        }
        match kinds::resolve(head) {
            Some(spec) if spec.family == Family::Native => {
                self.switch_namespace(namespace);
                self.open_kind_list(spec);
            }
            Some(spec) => {
                self.switch_namespace(namespace);
                self.show_kinds(KindFilter::kind(spec.names.kind));
            }
            // A Kubernetes kind typed before discovery finished: held until
            // it lands, like a plain `:deploy`, in the namespace asked for.
            None if self.config.native_resources
                && self.async_state.kind_discovery.is_loading() =>
            {
                self.switch_namespace(namespace);
                self.hold_kind_command(head);
            }
            None => self.set_status_message((
                format!("-n works with views and kinds; '{head}' is neither"),
                true,
            )),
        }
    }

    /// Apply a view (#306): its namespace (switched like `:ns`; `namespace`
    /// overrides it, for `-n`), its sort, then its kinds. Everything is
    /// checked before anything changes, so a broken view leaves the list as
    /// it was. `lenient` keeps kinds not known yet (startup, before
    /// Flux-adjacent kinds are discovered).
    fn apply_view(
        &mut self,
        name: &str,
        view: &ViewConfig,
        lenient: bool,
        namespace: Option<Option<String>>,
    ) -> Result<(), String> {
        let kinds = view.kinds.join(",");
        let filter = if lenient {
            KindFilter::parse_allowing_unknown(&kinds).map(|(filter, _)| filter)?
        } else {
            KindFilter::parse(&kinds)?
        };
        let sort = view
            .sort
            .as_deref()
            .map(ListSort::parse)
            .transpose()
            .map_err(|e| format!("sort: {e}"))?;
        let namespace = namespace.or_else(|| {
            view.namespace
                .as_deref()
                .map(super::command_mode::namespace_arg)
        });
        if let Some(namespace) = namespace {
            self.switch_namespace(namespace);
        }
        if let Some(sort) = sort {
            self.apply_sort(sort);
        }
        self.show_kinds(filter.named(name));
        Ok(())
    }

    /// Set the Flux list's sort (`defaultSort`, a view's `sort`).
    pub(super) fn apply_sort(&mut self, sort: ListSort) {
        use super::state::SortField;
        self.view_state.sort_field = match sort.key {
            SortKey::Default => SortField::Default,
            SortKey::Name => SortField::Name,
            SortKey::Age => SortField::Age,
            SortKey::Type => SortField::Type,
            SortKey::Status => SortField::Status,
        };
        self.view_state.sort_reverse = sort.reverse;
        self.reset_list_position();
    }

    /// `defaultSort` at startup. `Err` carries the warning to show.
    pub fn apply_default_sort(&mut self, value: &str) -> Result<(), String> {
        let sort = ListSort::parse(value).map_err(|e| format!("defaultSort: {e}"))?;
        self.apply_sort(sort);
        Ok(())
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
        // A view brings its namespace and sort along.
        if let Some((name, view)) = self.view_named(value) {
            return self
                .apply_view(&name, &view, true, None)
                .map_err(|e| format!("defaultResourceFilter: view '{value}': {e}"));
        }
        // A kind flux9s learns at runtime (Flux-adjacent) is kept as
        // written; it matches once its resources arrive.
        let filter = KindFilter::parse_allowing_unknown(value)
            .map(|(filter, _)| filter)
            .map_err(|e| format!("defaultResourceFilter '{value}': {e}"))?;
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
            .map(|(name, view)| {
                let mut about = view.kinds.join(", ");
                if let Some(namespace) = &view.namespace {
                    about.push_str(&format!(" · ns {namespace}"));
                }
                if let Some(sort) = &view.sort {
                    about.push_str(&format!(" · sort {sort}"));
                }
                (format!(":{name}"), about)
            })
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
        if let Some(Err(e)) = view.sort.as_deref().map(ListSort::parse) {
            problems.push(format!("view '{name}': sort: {e}"));
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
                        ..Default::default()
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
    fn a_bad_view_sort_is_reported() {
        let mut views = views(&[("apps", &["ks"])]);
        views.get_mut("apps").unwrap().sort = Some("size".into());
        let problems = view_problems(&views);
        assert!(
            problems[0].starts_with("view 'apps': sort: Unknown sort 'size'"),
            "{problems:#?}"
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
