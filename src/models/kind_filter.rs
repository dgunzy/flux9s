//! Which Flux kinds the Flux list shows (#302): all of them, one kind
//! (`:ks`), several (`:ks,hr`), or a named view from the config
//! (`views.apps.kinds`).
//!
//! Only kinds the Flux list watches combine — Flux kinds and Flux-adjacent
//! CRDs. A Kubernetes kind (`deploy`) is refused with a clear message rather
//! than half-working: mixing Flux and native kinds in one list needs per-row
//! actions, which is later work.

use super::kinds::{self, Family};

/// The kinds the Flux list is narrowed to. Empty means all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KindFilter {
    /// Kind names (`Kustomization`), in the order given, without repeats.
    kinds: Vec<String>,
    /// The view these kinds came from, for titles and the header.
    view: Option<String>,
}

impl KindFilter {
    /// No filter: every kind.
    pub fn all() -> Self {
        Self::default()
    }

    /// Exactly one kind (`Kustomization`), already resolved.
    pub fn kind(kind: impl Into<String>) -> Self {
        Self {
            kinds: vec![kind.into()],
            view: None,
        }
    }

    /// Parse `ks,hr` (or `ks`): each comma-separated token is a Flux kind's
    /// name, plural, or alias, resolved like `:<kind>`. `Err` carries the
    /// message to show.
    pub fn parse(spec: &str) -> Result<Self, String> {
        Self::parse_with(spec, false).map(|(filter, _)| filter)
    }

    /// [`Self::parse`] for kinds that may only be known later: a token that
    /// isn't a known kind (a Flux-adjacent CRD before discovery, or in the
    /// CLI, which never discovers) is kept as written instead of refused,
    /// and returned so the caller can mention it. Kubernetes kinds are still
    /// refused.
    pub fn parse_allowing_unknown(spec: &str) -> Result<(Self, Vec<String>), String> {
        Self::parse_with(spec, true)
    }

    fn parse_with(spec: &str, keep_unknown: bool) -> Result<(Self, Vec<String>), String> {
        let mut filter = Self::default();
        let mut unknown = Vec::new();
        for token in spec.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let kind = match kinds::resolve(token) {
                Some(found) if found.family != Family::Native => found.names.kind,
                found if found.is_some() || kinds::looks_like_kubernetes_kind(token) => {
                    return Err(format!(
                        "'{token}' is a Kubernetes kind — only Flux kinds combine in one list (for now)"
                    ));
                }
                _ if keep_unknown => {
                    unknown.push(token.to_string());
                    token.to_string()
                }
                _ => return Err(format!("Unknown Flux kind '{token}'")),
            };
            if !filter.kinds.contains(&kind) {
                filter.kinds.push(kind);
            }
        }
        if filter.kinds.is_empty() {
            return Err("No kinds given (e.g. ks,hr)".to_string());
        }
        Ok((filter, unknown))
    }

    /// The same kinds, shown as the named view.
    pub fn named(mut self, view: impl Into<String>) -> Self {
        self.view = Some(view.into());
        self
    }

    pub fn is_all(&self) -> bool {
        self.kinds.is_empty()
    }

    /// The kind when exactly one is selected — the list then shows that
    /// kind's own columns.
    pub fn single(&self) -> Option<&str> {
        match self.kinds.as_slice() {
            [kind] => Some(kind),
            _ => None,
        }
    }

    pub fn kinds(&self) -> &[String] {
        &self.kinds
    }

    pub fn view(&self) -> Option<&str> {
        self.view.as_deref()
    }

    /// Whether resources of `kind` are shown (always, with no filter).
    /// Case-insensitive: a kind kept as written (Flux-adjacent, before
    /// discovery) may differ in case from its resources' kind.
    pub fn contains(&self, kind: &str) -> bool {
        self.is_all() || self.kinds.iter().any(|k| k.eq_ignore_ascii_case(kind))
    }

    /// `Kustomization + HelmRelease`; `None` with no filter.
    pub fn kinds_label(&self) -> Option<String> {
        (!self.is_all()).then(|| self.kinds.join(" + "))
    }

    /// Drop kinds that no longer exist (discovered kinds whose discovery was
    /// turned off). Returns whether anything changed; a filter left with no
    /// kinds shows everything again.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let before = self.kinds.len();
        self.kinds.retain(|k| keep(k));
        let changed = self.kinds.len() != before;
        if changed && self.kinds.is_empty() {
            self.view = None;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flux_kinds_by_name_plural_or_alias() {
        let filter = KindFilter::parse("ks, hr,helmreleases").unwrap();
        assert_eq!(
            filter.kinds(),
            ["Kustomization", "HelmRelease"],
            "deduplicated"
        );
        assert_eq!(filter.single(), None);
        assert_eq!(
            filter.kinds_label().as_deref(),
            Some("Kustomization + HelmRelease")
        );
        assert!(filter.contains("HelmRelease") && !filter.contains("GitRepository"));

        let one = KindFilter::parse("gitrepo").unwrap();
        assert_eq!(one.single(), Some("GitRepository"));
        assert_eq!(one, KindFilter::kind("GitRepository"));
    }

    #[test]
    fn refuses_kubernetes_and_unknown_kinds() {
        let native = KindFilter::parse("ks,deploy").unwrap_err();
        assert!(native.contains("'deploy' is a Kubernetes kind"), "{native}");
        let unknown = KindFilter::parse("ks,nope").unwrap_err();
        assert_eq!(unknown, "Unknown Flux kind 'nope'");
        assert!(KindFilter::parse(" , ").is_err());
    }

    #[test]
    fn unknown_kinds_can_wait_for_discovery() {
        // A Flux-adjacent kind isn't known before discovery (or in the CLI).
        let (filter, unknown) = KindFilter::parse_allowing_unknown("ks,RuntimeOnlyKind").unwrap();
        assert_eq!(filter.kinds(), ["Kustomization", "RuntimeOnlyKind"]);
        assert_eq!(unknown, ["RuntimeOnlyKind"]);
        // Kubernetes kinds are still refused.
        assert!(KindFilter::parse_allowing_unknown("ks,deploy").is_err());
    }

    #[test]
    fn all_shows_everything() {
        let all = KindFilter::all();
        assert!(all.is_all() && all.contains("Anything"));
        assert_eq!(all.kinds_label(), None);
    }

    #[test]
    fn retain_drops_vanished_kinds_and_empties_to_all() {
        let mut filter = KindFilter::parse("ks,hr").unwrap().named("apps");
        assert!(!filter.retain(|_| true));
        assert!(filter.retain(|k| k != "HelmRelease"));
        assert_eq!(filter.single(), Some("Kustomization"));
        assert_eq!(filter.view(), Some("apps"));
        assert!(filter.retain(|_| false));
        assert!(filter.is_all());
        assert_eq!(filter.view(), None);
    }
}
