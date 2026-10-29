//! Which kinds the Flux list shows (#302, #304): all Flux kinds, one kind
//! (`:ks`), several (`:ks,hr`), or a named view from the config
//! (`views.apps.kinds`). A set may mix in Kubernetes kinds (`:ks,deploy`):
//! the Flux list then also lists their objects, from a watch per kind that
//! runs while the list is on screen (needs `nativeResources`).

use super::kinds::{self, Family, KindSpec};

/// A Kubernetes kind mixed into the Flux list (#304).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeKind {
    /// As written (`deploy`, `buckets.s3.aws.upbound.io`): saved back to
    /// the config, and resolved later when it couldn't be yet.
    pub token: String,
    /// Group and kind once resolved — exact, so a same-named kind in
    /// another group (a Crossplane `Bucket`, a Knative `Service`) is never
    /// confused with it.
    pub resolved: Option<(String, String)>,
}

impl NativeKind {
    fn of(token: &str, spec: Option<&KindSpec>) -> Self {
        Self {
            token: token.to_string(),
            resolved: spec.map(|s| (s.gvk.group.clone(), s.gvk.kind.clone())),
        }
    }

    /// What titles and the header show: the kind name once resolved.
    pub fn label(&self) -> &str {
        self.resolved
            .as_ref()
            .map_or(self.token.as_str(), |(_, kind)| kind.as_str())
    }

    fn same(&self, other: &Self) -> bool {
        match (&self.resolved, &other.resolved) {
            (Some(a), Some(b)) => a == b,
            _ => self.token.eq_ignore_ascii_case(&other.token),
        }
    }
}

/// The kinds the Flux list is narrowed to. Empty means all Flux kinds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KindFilter {
    /// Flux (and Flux-adjacent) kind names (`Kustomization`), in the order
    /// given, without repeats.
    kinds: Vec<String>,
    /// Kubernetes kinds mixed in (#304), resolved when discovery allows.
    native: Vec<NativeKind>,
    /// The view these kinds came from, for titles and the header.
    view: Option<String>,
}

impl KindFilter {
    /// No filter: every Flux kind.
    pub fn all() -> Self {
        Self::default()
    }

    /// Exactly one kind (`Kustomization`), already resolved.
    pub fn kind(kind: impl Into<String>) -> Self {
        Self {
            kinds: vec![kind.into()],
            ..Self::default()
        }
    }

    /// Parse `ks,hr,deploy`: each comma-separated token is a kind's name,
    /// plural, or alias, resolved like `:<kind>`. `Err` carries the message
    /// to show.
    pub fn parse(spec: &str) -> Result<Self, String> {
        Self::parse_with(spec, false).map(|(filter, _)| filter)
    }

    /// [`Self::parse`] for kinds that may only be known later: a token that
    /// isn't a known kind (a Flux-adjacent CRD before discovery, or in the
    /// CLI, which never discovers) is kept as written instead of refused,
    /// and returned so the caller can mention it.
    pub fn parse_allowing_unknown(spec: &str) -> Result<(Self, Vec<String>), String> {
        Self::parse_with(spec, true)
    }

    fn parse_with(spec: &str, keep_unknown: bool) -> Result<(Self, Vec<String>), String> {
        Self::parse_resolving(spec, keep_unknown, kinds::resolve)
    }

    /// [`Self::parse_allowing_unknown`] with a given resolver (tests pass
    /// one over their own catalog).
    pub fn parse_resolving(
        spec: &str,
        keep_unknown: bool,
        resolve: impl Fn(&str) -> Option<KindSpec>,
    ) -> Result<(Self, Vec<String>), String> {
        let mut filter = Self::default();
        let mut unknown = Vec::new();
        for token in spec.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            match resolve(token) {
                Some(found) if found.family != Family::Native => filter.add_flux(found.names.kind),
                Some(found) => filter.add_native(NativeKind::of(token, Some(&found))),
                // Before discovery, or with native resources off.
                None if kinds::looks_like_kubernetes_kind(token) => {
                    filter.add_native(NativeKind::of(token, None));
                }
                None if keep_unknown => {
                    unknown.push(token.to_string());
                    filter.add_flux(token.to_string());
                }
                None => return Err(format!("Unknown kind '{token}'")),
            }
        }
        if filter.is_all() {
            return Err("No kinds given (e.g. ks,hr)".to_string());
        }
        Ok((filter, unknown))
    }

    fn add_flux(&mut self, kind: String) {
        if !self.kinds.contains(&kind) {
            self.kinds.push(kind);
        }
    }

    fn add_native(&mut self, kind: NativeKind) {
        if !self.native.iter().any(|k| k.same(&kind)) {
            self.native.push(kind);
        }
    }

    /// Discovery finished: kinds kept as written can be placed now. A token
    /// that wasn't known (`widgets` from a startup filter or view) moves to
    /// the Kubernetes kinds when it's one, and unresolved Kubernetes kinds
    /// learn their group. Returns whether anything changed.
    pub fn adopt_discovered(&mut self, resolve: impl Fn(&str) -> Option<KindSpec>) -> bool {
        let mut changed = false;
        let mut adopted = Vec::new();
        self.kinds.retain(|kind| match resolve(kind) {
            Some(spec) if spec.family == Family::Native => {
                adopted.push(NativeKind::of(kind, Some(&spec)));
                changed = true;
                false
            }
            _ => true,
        });
        for native in self.native.iter_mut().filter(|n| n.resolved.is_none()) {
            if let Some(spec) = resolve(&native.token).filter(|s| s.family == Family::Native) {
                native.resolved = Some((spec.gvk.group, spec.gvk.kind));
                changed = true;
            }
        }
        // Entries that now resolve alike (`Deployment`, `deployments`)
        // are one kind: keep the first, so it's watched and listed once.
        let entries = std::mem::take(&mut self.native);
        for kind in entries.into_iter().chain(adopted) {
            self.add_native(kind);
        }
        changed
    }

    /// The same kinds, shown as the named view.
    pub fn named(mut self, view: impl Into<String>) -> Self {
        self.view = Some(view.into());
        self
    }

    /// No filter: every Flux kind, nothing mixed in.
    pub fn is_all(&self) -> bool {
        self.kinds.is_empty() && self.native.is_empty()
    }

    /// The Flux kind when exactly one is selected and nothing is mixed in —
    /// the list then shows that kind's own columns.
    pub fn single(&self) -> Option<&str> {
        match (self.kinds.as_slice(), self.native.is_empty()) {
            ([kind], true) => Some(kind),
            _ => None,
        }
    }

    /// The Flux kinds.
    pub fn kinds(&self) -> &[String] {
        &self.kinds
    }

    /// The Kubernetes kinds mixed in (#304).
    pub fn native_kinds(&self) -> &[NativeKind] {
        &self.native
    }

    /// Every kind, Flux first, as written back to the config
    /// (`Kustomization,HelmRelease,deploy`).
    pub fn all_kinds(&self) -> impl Iterator<Item = &str> {
        self.kinds
            .iter()
            .map(String::as_str)
            .chain(self.native.iter().map(|k| k.token.as_str()))
    }

    /// Every kind's display name, Flux first.
    pub fn all_labels(&self) -> impl Iterator<Item = &str> {
        self.kinds
            .iter()
            .map(String::as_str)
            .chain(self.native.iter().map(NativeKind::label))
    }

    pub fn view(&self) -> Option<&str> {
        self.view.as_deref()
    }

    /// Whether watched Flux resources of `kind` are shown (always, with no
    /// filter). Case-insensitive: a kind kept as written (Flux-adjacent,
    /// before discovery) may differ in case from its resources' kind.
    pub fn contains(&self, kind: &str) -> bool {
        self.is_all() || self.kinds.iter().any(|k| k.eq_ignore_ascii_case(kind))
    }

    /// `Kustomization + HelmRelease + Deployment`; `None` with no filter.
    pub fn kinds_label(&self) -> Option<String> {
        (!self.is_all()).then(|| self.all_labels().collect::<Vec<_>>().join(" + "))
    }

    /// Drop Flux kinds that no longer exist (discovered kinds whose
    /// discovery was turned off). Returns whether anything changed; a filter
    /// left with no kinds shows everything again.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let before = self.kinds.len();
        self.kinds.retain(|k| keep(k));
        let changed = self.kinds.len() != before;
        if changed && self.is_all() {
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
    fn kubernetes_kinds_mix_in_and_unknown_ones_are_refused() {
        // Before discovery `deploy` doesn't resolve; it's kept as written.
        let mixed = KindFilter::parse("ks,deploy").unwrap();
        assert_eq!(mixed.kinds(), ["Kustomization"]);
        assert_eq!(mixed.native_kinds()[0].token, "deploy");
        assert_eq!(mixed.native_kinds()[0].resolved, None, "not discovered yet");
        assert_eq!(mixed.single(), None, "mixed lists use the shared columns");
        assert!(!mixed.is_all() && mixed.contains("Kustomization"));
        assert!(!mixed.contains("HelmRelease"), "only the Flux kinds given");
        assert_eq!(
            mixed.kinds_label().as_deref(),
            Some("Kustomization + deploy")
        );

        // Only Kubernetes kinds: no Flux rows at all.
        let native_only = KindFilter::parse("deploy,sts").unwrap();
        assert!(!native_only.is_all() && !native_only.contains("Kustomization"));

        let unknown = KindFilter::parse("ks,nope").unwrap_err();
        assert_eq!(unknown, "Unknown kind 'nope'");
        assert!(KindFilter::parse(" , ").is_err());
    }

    #[test]
    fn unknown_kinds_can_wait_for_discovery() {
        // A Flux-adjacent kind isn't known before discovery (or in the CLI).
        let (filter, unknown) = KindFilter::parse_allowing_unknown("ks,RuntimeOnlyKind").unwrap();
        assert_eq!(filter.kinds(), ["Kustomization", "RuntimeOnlyKind"]);
        assert_eq!(unknown, ["RuntimeOnlyKind"]);
        // A Kubernetes kind is mixed in, not unknown.
        let (filter, unknown) = KindFilter::parse_allowing_unknown("ks,deploy").unwrap();
        assert_eq!(filter.native_kinds()[0].token, "deploy");
        assert!(unknown.is_empty());
    }

    fn catalog(specs: Vec<KindSpec>) -> kinds::KindCatalog {
        let catalog = kinds::KindCatalog::default();
        catalog.replace(specs);
        catalog
    }

    fn native(group: &str, kind: &str, plural: &str) -> KindSpec {
        KindSpec::generic(
            kinds::Gvk {
                group: group.into(),
                version: "v1".into(),
                kind: kind.into(),
            },
            kinds::KindScope::Namespaced,
            plural.into(),
            vec![],
        )
    }

    /// A same-named kind in another group stays that kind: the Crossplane
    /// Bucket isn't Flux's, and keeps its group for the watch.
    #[test]
    fn qualified_kubernetes_kinds_keep_their_group() {
        let discovered = catalog(vec![native("s3.aws.upbound.io", "Bucket", "buckets")]);
        let resolve = |token: &str| kinds::resolve_in(token, &discovered);
        let (filter, _) =
            KindFilter::parse_resolving("ks,buckets.s3.aws.upbound.io", false, resolve).unwrap();
        assert_eq!(filter.kinds(), ["Kustomization"]);
        assert_eq!(
            filter.native_kinds()[0].resolved,
            Some(("s3.aws.upbound.io".to_string(), "Bucket".to_string()))
        );
        assert_eq!(
            filter.kinds_label().as_deref(),
            Some("Kustomization + Bucket")
        );
        assert_eq!(
            filter.all_kinds().collect::<Vec<_>>(),
            ["Kustomization", "buckets.s3.aws.upbound.io"],
            "saved as written"
        );
    }

    /// A startup filter is parsed before discovery: a custom kind's alias
    /// (`widgets`) is kept as unknown, then placed once discovery lands.
    #[test]
    fn discovery_places_kinds_kept_as_written() {
        let before = catalog(vec![]);
        let (mut filter, unknown) = KindFilter::parse_resolving("ks,widgets,deploy", true, |t| {
            kinds::resolve_in(t, &before)
        })
        .unwrap();
        assert_eq!(unknown, ["widgets"]);
        assert_eq!(filter.kinds(), ["Kustomization", "widgets"]);

        let mut deploy = native("apps", "Deployment", "deployments");
        deploy.names.short_names = vec!["deploy".into()];
        let after = catalog(vec![native("example.com", "Widget", "widgets"), deploy]);
        assert!(filter.adopt_discovered(|t| kinds::resolve_in(t, &after)));
        assert_eq!(filter.kinds(), ["Kustomization"]);
        let resolved: Vec<_> = filter
            .native_kinds()
            .iter()
            .map(|k| k.resolved.clone().unwrap().1)
            .collect();
        assert_eq!(resolved, ["Deployment", "Widget"]);
        assert!(
            !filter.adopt_discovered(|t| kinds::resolve_in(t, &after)),
            "idempotent"
        );
    }

    /// Two spellings of one kind, both unresolved at startup, become one
    /// entry once discovery resolves them — one watch, one set of rows.
    #[test]
    fn discovery_merges_spellings_of_one_kind() {
        let before = catalog(vec![]);
        let (mut filter, _) = KindFilter::parse_resolving("deploy,deployments", true, |t| {
            kinds::resolve_in(t, &before)
        })
        .unwrap();
        assert_eq!(filter.native_kinds().len(), 2, "not known apart yet");
        let mut deploy = native("apps", "Deployment", "deployments");
        deploy.names.short_names = vec!["deploy".into()];
        let after = catalog(vec![deploy]);
        assert!(filter.adopt_discovered(|t| kinds::resolve_in(t, &after)));
        assert_eq!(filter.native_kinds().len(), 1);
        assert_eq!(
            filter.native_kinds()[0].token,
            "deploy",
            "the first one kept"
        );
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
