//! Effective HelmRelease values (#264).
//!
//! Reproduces how helm-controller builds a release's values — the same
//! algorithm the Flux Operator web UI ports:
//!
//! 1. Each `spec.valuesFrom` reference is applied in order. Without a
//!    `targetPath` its `valuesKey` (default `values.yaml`) is parsed as YAML
//!    and deep-merged; with one, the raw value is set at that path using
//!    Helm `--set` typing (quoted = string, `true`/`false`/`null`/integers
//!    coerced, `{a,b}` = list).
//! 2. `spec.values` is merged last, overriding everything.
//!
//! Where helm-controller fails the reconcile on a missing non-optional
//! reference, this view records the failure on that source and carries on,
//! so the rest of the values stay visible.
//!
//! Values that come from a Secret are replaced with [`REDACTED`] *before*
//! merging unless reveal is requested, so a later source overriding a Secret
//! value still shows through while anything the Secret still supplies stays
//! hidden.

use std::collections::{BTreeMap, HashMap};

use anyhow::Context;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use kube::Api;
use serde_json::{Map, Value};

/// Placeholder for values sourced from a Secret.
pub const REDACTED: &str = "<redacted>";

/// `valuesKey` default, as in the HelmRelease API.
const DEFAULT_VALUES_KEY: &str = "values.yaml";

/// Bounds on `targetPath` array indexes (mirrors the Flux Operator limits).
const MAX_ARRAY_INDEX: usize = 1000;
const MAX_ARRAY_ELEMENTS: usize = 10_000;

/// One `spec.valuesFrom` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValuesRef {
    pub kind: String,
    pub name: String,
    pub values_key: String,
    pub target_path: Option<String>,
    pub optional: bool,
}

impl ValuesRef {
    /// `Kind/name [key → path]` label for the sources list.
    pub fn label(&self) -> String {
        match &self.target_path {
            Some(path) => format!(
                "{}/{} [{} → {}]",
                self.kind, self.name, self.values_key, path
            ),
            None => format!("{}/{} [{}]", self.kind, self.name, self.values_key),
        }
    }

    fn is_secret(&self) -> bool {
        self.kind == "Secret"
    }
}

/// What happened to one values source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOutcome {
    /// Merged into the result.
    Applied,
    /// Merged, with every value it contributed redacted.
    AppliedRedacted,
    /// Optional and absent — skipped, as helm-controller does.
    Skipped(String),
    /// Could not be applied; helm-controller would fail the reconcile.
    Failed(String),
}

/// A values source and its outcome, in merge order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValuesSource {
    pub reference: ValuesRef,
    pub outcome: SourceOutcome,
}

/// The effective values plus how they were assembled.
#[derive(Debug, Clone, PartialEq)]
pub struct HelmValues {
    /// Merged values (a JSON object).
    pub values: Value,
    /// `valuesFrom` sources in merge order.
    pub sources: Vec<ValuesSource>,
    /// Whether `spec.values` contributed.
    pub has_inline: bool,
    /// Whether Secret values are shown in the clear.
    pub secrets_revealed: bool,
}

impl HelmValues {
    /// Whether any applied source had its values redacted.
    pub fn has_redactions(&self) -> bool {
        self.sources
            .iter()
            .any(|s| s.outcome == SourceOutcome::AppliedRedacted)
    }
}

/// Parse `spec.valuesFrom`, skipping malformed entries.
pub fn parse_values_refs(helm_release: &Value) -> Vec<ValuesRef> {
    helm_release
        .pointer("/spec/valuesFrom")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let field = |name: &str| entry.get(name).and_then(Value::as_str);
            Some(ValuesRef {
                kind: field("kind")?.to_string(),
                name: field("name")?.to_string(),
                values_key: field("valuesKey")
                    .filter(|k| !k.is_empty())
                    .unwrap_or(DEFAULT_VALUES_KEY)
                    .to_string(),
                target_path: field("targetPath")
                    .filter(|p| !p.is_empty())
                    .map(str::to_string),
                optional: entry
                    .get("optional")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

/// Result of looking up one referenced object: its string data (Secret data
/// already base64-decoded), or why it couldn't be read.
#[derive(Debug, Clone)]
pub enum Lookup {
    Found(BTreeMap<String, String>),
    NotFound,
    Error(String),
}

/// Deep-merge `overlay` into `base` (fluxcd `transform.MergeMaps`): nested
/// objects merge key by key; anything else in `overlay` replaces.
pub fn merge_maps(base: &mut Map<String, Value>, overlay: &Map<String, Value>) {
    for (key, value) in overlay {
        match (base.get_mut(key), value) {
            (Some(Value::Object(existing)), Value::Object(incoming)) => {
                merge_maps(existing, incoming)
            }
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Replace every scalar in `value` with [`REDACTED`], keeping the structure.
fn redact(value: &mut Value) {
    match value {
        Value::Object(map) => map.values_mut().for_each(redact),
        Value::Array(items) => items.iter_mut().for_each(redact),
        other => *other = Value::String(REDACTED.to_string()),
    }
}

/// Assemble the effective values from inline values, the references, and
/// what each reference resolved to. Pure, so every rule is unit-testable.
pub fn compose_values(
    inline: &Map<String, Value>,
    refs: &[ValuesRef],
    lookups: &HashMap<(String, String), Lookup>,
    reveal_secrets: bool,
) -> HelmValues {
    let mut result = Map::new();
    let mut sources = Vec::with_capacity(refs.len());

    for reference in refs {
        let outcome = apply_ref(&mut result, inline, reference, lookups, reveal_secrets);
        sources.push(ValuesSource {
            reference: reference.clone(),
            outcome,
        });
    }
    merge_maps(&mut result, inline);

    HelmValues {
        values: Value::Object(result),
        sources,
        has_inline: !inline.is_empty(),
        secrets_revealed: reveal_secrets,
    }
}

fn apply_ref(
    result: &mut Map<String, Value>,
    inline: &Map<String, Value>,
    reference: &ValuesRef,
    lookups: &HashMap<(String, String), Lookup>,
    reveal_secrets: bool,
) -> SourceOutcome {
    if reference.kind != "ConfigMap" && reference.kind != "Secret" {
        return SourceOutcome::Failed(format!("unsupported kind '{}'", reference.kind));
    }
    let lookup = lookups.get(&(reference.kind.clone(), reference.name.clone()));
    let data = match lookup {
        Some(Lookup::Found(data)) => data,
        Some(Lookup::Error(e)) => return SourceOutcome::Failed(e.clone()),
        Some(Lookup::NotFound) | None if reference.optional => {
            return SourceOutcome::Skipped("optional, not found".to_string());
        }
        Some(Lookup::NotFound) | None => {
            return SourceOutcome::Failed("not found".to_string());
        }
    };
    let Some(raw) = data.get(&reference.values_key) else {
        return if reference.optional {
            SourceOutcome::Skipped(format!("optional, key '{}' absent", reference.values_key))
        } else {
            SourceOutcome::Failed(format!("key '{}' not found", reference.values_key))
        };
    };
    let redacting = reference.is_secret() && !reveal_secrets;

    if let Some(path) = &reference.target_path {
        // helm-controller merges the inline values in before setting a path.
        merge_maps(result, inline);
        let value = if redacting {
            "'<redacted>'"
        } else {
            raw.as_str()
        };
        return match replace_path_value(result, path, value) {
            Ok(()) if redacting => SourceOutcome::AppliedRedacted,
            Ok(()) => SourceOutcome::Applied,
            Err(e) => SourceOutcome::Failed(format!("target path '{path}': {e}")),
        };
    }

    let mut parsed: Value = match serde_yaml::from_str(raw) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(Value::Null) => Value::Object(Map::new()), // empty document
        Ok(_) => return SourceOutcome::Failed("values are not a YAML mapping".to_string()),
        Err(e) => return SourceOutcome::Failed(format!("invalid YAML: {e}")),
    };
    if redacting {
        redact(&mut parsed);
    }
    if let Value::Object(map) = &parsed {
        merge_maps(result, map);
    }
    if redacting {
        SourceOutcome::AppliedRedacted
    } else {
        SourceOutcome::Applied
    }
}

/// One step of a `targetPath`: a map key, optionally followed by an index.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment {
    Key(String),
    Index(usize),
}

/// Parse a Helm `--set` style path: `.`-separated keys, `\.` for a literal
/// dot, and `[n]` indexes (`a.b[0].c`, `a[0][1]`).
fn parse_path(path: &str) -> anyhow::Result<Vec<PathSegment>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in path.chars() {
        match c {
            _ if escaped => {
                current.push(c);
                escaped = false;
            }
            '\\' => escaped = true,
            '.' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    let mut segments = Vec::new();
    let mut elements = 0usize;
    for token in tokens {
        let (key, mut rest) = match token.find('[') {
            Some(i) => (token[..i].to_string(), &token[i..]),
            None => (token.clone(), ""),
        };
        if !key.is_empty() {
            segments.push(PathSegment::Key(key));
        }
        while let Some(stripped) = rest.strip_prefix('[') {
            let end = stripped
                .find(']')
                .with_context(|| format!("unclosed '[' in '{token}'"))?;
            let index: usize = stripped[..end]
                .parse()
                .with_context(|| format!("invalid array index '{}'", &stripped[..end]))?;
            if index > MAX_ARRAY_INDEX {
                anyhow::bail!("array index {index} exceeds maximum {MAX_ARRAY_INDEX}");
            }
            elements += index + 1;
            if elements > MAX_ARRAY_ELEMENTS {
                anyhow::bail!("array elements exceed maximum {MAX_ARRAY_ELEMENTS}");
            }
            segments.push(PathSegment::Index(index));
            rest = &stripped[end + 1..];
        }
    }
    if segments.is_empty() {
        anyhow::bail!("empty path");
    }
    Ok(segments)
}

/// Helm `--set` scalar typing: `true`/`false`/`null` and integers without a
/// leading zero are coerced; everything else stays a string.
fn typed_value(value: &str) -> Value {
    if value.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if value.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if value.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if value == "0" {
        return Value::from(0);
    }
    if !value.starts_with('0')
        && let Ok(n) = value.parse::<i64>()
    {
        return Value::from(n);
    }
    Value::String(value.to_string())
}

/// Remove backslash escapes.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut escaped = false;
    for c in value.chars() {
        if escaped {
            out.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// Split on unescaped commas (for `{a,b,c}` lists).
fn split_list(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in inner.chars() {
        match c {
            _ if escaped => {
                current.push(c);
                escaped = false;
            }
            '\\' => escaped = true,
            ',' => items.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    if !current.is_empty() || !items.is_empty() {
        items.push(current);
    }
    items
}

/// Resolve a raw `targetPath` value the way the Flux Operator does.
fn resolve_value(raw: &str) -> Value {
    let quoted = raw.len() >= 2
        && ((raw.starts_with('\'') && raw.ends_with('\''))
            || (raw.starts_with('"') && raw.ends_with('"')));
    let value = if quoted {
        raw.trim_matches(|c| c == '\'' || c == '"')
    } else {
        raw
    };
    let typed = |item: &str| {
        if quoted {
            Value::String(item.to_string())
        } else {
            typed_value(item)
        }
    };
    if let Some(inner) = value.strip_prefix('{').and_then(|v| v.strip_suffix('}')) {
        return Value::Array(split_list(inner).iter().map(|i| typed(i)).collect());
    }
    typed(&unescape(value))
}

/// Set `raw` (resolved with Helm `--set` typing) at `path` in `values`,
/// creating intermediate maps and padding arrays with nulls as needed.
pub fn replace_path_value(
    values: &mut Map<String, Value>,
    path: &str,
    raw: &str,
) -> anyhow::Result<()> {
    let segments = parse_path(path)?;
    let resolved = resolve_value(raw);

    let mut root = Value::Object(std::mem::take(values));
    let outcome = set_at(&mut root, &segments, resolved);
    if let Value::Object(map) = root {
        *values = map;
    }
    outcome
}

fn set_at(node: &mut Value, segments: &[PathSegment], value: Value) -> anyhow::Result<()> {
    let Some((first, rest)) = segments.split_first() else {
        *node = value;
        return Ok(());
    };
    // The container this segment expects, used when a slot is empty or holds
    // a scalar that must be replaced to continue.
    let empty_child = |next: Option<&PathSegment>| match next {
        Some(PathSegment::Index(_)) => Value::Array(Vec::new()),
        _ => Value::Object(Map::new()),
    };
    match first {
        PathSegment::Key(key) => {
            if !node.is_object() {
                anyhow::bail!("cannot set key '{key}' inside a non-map value");
            }
            let Value::Object(map) = node else {
                return Ok(());
            };
            let child = map.entry(key.clone()).or_insert(Value::Null);
            let expects_array = matches!(rest.first(), Some(PathSegment::Index(_)));
            let wrong_shape = if expects_array {
                !child.is_array()
            } else {
                !rest.is_empty() && !child.is_object()
            };
            if wrong_shape {
                *child = empty_child(rest.first());
            }
            set_at(child, rest, value)
        }
        PathSegment::Index(index) => {
            let Value::Array(items) = node else {
                anyhow::bail!("cannot index [{index}] into a non-list value");
            };
            while items.len() <= *index {
                items.push(Value::Null);
            }
            let Some(child) = items.get_mut(*index) else {
                return Ok(());
            };
            let expects_array = matches!(rest.first(), Some(PathSegment::Index(_)));
            let wrong_shape = if expects_array {
                !child.is_array()
            } else {
                !rest.is_empty() && !child.is_object()
            };
            if wrong_shape {
                *child = empty_child(rest.first());
            }
            set_at(child, rest, value)
        }
    }
}

fn is_status(error: &kube::Error, code: u16) -> bool {
    matches!(error, kube::Error::Api(resp) if resp.code == code)
}

/// Read one ConfigMap/Secret's string data.
async fn lookup(client: &kube::Client, namespace: &str, kind: &str, name: &str) -> Lookup {
    let result = match kind {
        "ConfigMap" => Api::<ConfigMap>::namespaced(client.clone(), namespace)
            .get(name)
            .await
            .map(|cm| cm.data.unwrap_or_default()),
        "Secret" => Api::<Secret>::namespaced(client.clone(), namespace)
            .get(name)
            .await
            .map(|secret| {
                secret
                    .data
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(k, v)| (k, String::from_utf8_lossy(&v.0).into_owned()))
                    .collect()
            }),
        other => return Lookup::Error(format!("unsupported kind '{other}'")),
    };
    match result {
        Ok(data) => Lookup::Found(data),
        Err(e) if is_status(&e, 404) => Lookup::NotFound,
        Err(e) if is_status(&e, 403) => Lookup::Error("forbidden".to_string()),
        Err(e) => Lookup::Error(e.to_string()),
    }
}

/// Fetch a HelmRelease and every object it references, then compose its
/// effective values.
pub async fn fetch_helm_values(
    client: &kube::Client,
    namespace: &str,
    name: &str,
    reveal_secrets: bool,
) -> anyhow::Result<HelmValues> {
    let release = crate::kube::fetch_resource(client, "HelmRelease", namespace, name).await?;
    let refs = parse_values_refs(&release);
    let inline = release
        .pointer("/spec/values")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let mut lookups = HashMap::new();
    for reference in &refs {
        let key = (reference.kind.clone(), reference.name.clone());
        if let std::collections::hash_map::Entry::Vacant(slot) = lookups.entry(key) {
            slot.insert(lookup(client, namespace, &reference.kind, &reference.name).await);
        }
    }
    Ok(compose_values(&inline, &refs, &lookups, reveal_secrets))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn reference(kind: &str, name: &str) -> ValuesRef {
        ValuesRef {
            kind: kind.into(),
            name: name.into(),
            values_key: DEFAULT_VALUES_KEY.into(),
            target_path: None,
            optional: false,
        }
    }

    fn found(key: &str, data: &str) -> Lookup {
        Lookup::Found(BTreeMap::from([(key.to_string(), data.to_string())]))
    }

    #[test]
    fn parses_value_refs_with_defaults() {
        let hr = json!({"spec": {"valuesFrom": [
            {"kind": "ConfigMap", "name": "base"},
            {"kind": "Secret", "name": "creds", "valuesKey": "db.yaml", "targetPath": "db.password", "optional": true},
            {"kind": "ConfigMap"}
        ]}});
        let refs = parse_values_refs(&hr);
        assert_eq!(refs.len(), 2, "entry without a name is skipped");
        assert_eq!(refs[0].values_key, "values.yaml");
        assert_eq!(refs[1].target_path.as_deref(), Some("db.password"));
        assert!(refs[1].optional);
        assert_eq!(refs[1].label(), "Secret/creds [db.yaml → db.password]");
    }

    #[test]
    fn merges_in_order_with_inline_last() {
        let refs = vec![reference("ConfigMap", "a"), reference("ConfigMap", "b")];
        let lookups = HashMap::from([
            (
                ("ConfigMap".into(), "a".into()),
                found(
                    "values.yaml",
                    "replicas: 1\nimage:\n  tag: v1\n  pull: Always\n",
                ),
            ),
            (
                ("ConfigMap".into(), "b".into()),
                found("values.yaml", "image:\n  tag: v2\n"),
            ),
        ]);
        let inline = obj(json!({"replicas": 3}));
        let values = compose_values(&inline, &refs, &lookups, false);
        assert_eq!(
            values.values,
            json!({"replicas": 3, "image": {"tag": "v2", "pull": "Always"}})
        );
        assert!(values.has_inline);
        assert!(
            values
                .sources
                .iter()
                .all(|s| s.outcome == SourceOutcome::Applied)
        );
    }

    #[test]
    fn secret_values_are_redacted_unless_overridden_or_revealed() {
        let refs = vec![reference("Secret", "creds")];
        let lookups = HashMap::from([(
            ("Secret".into(), "creds".into()),
            found("values.yaml", "db:\n  password: hunter2\n  user: app\n"),
        )]);
        let inline = obj(json!({"db": {"user": "override"}}));

        let redacted = compose_values(&inline, &refs, &lookups, false);
        assert_eq!(
            redacted.values,
            json!({"db": {"password": REDACTED, "user": "override"}}),
            "inline override shows through; secret-only value stays hidden"
        );
        assert!(redacted.has_redactions());

        let revealed = compose_values(&inline, &refs, &lookups, true);
        assert_eq!(revealed.values["db"]["password"], "hunter2");
        assert!(!revealed.has_redactions());
    }

    #[test]
    fn missing_refs_skip_when_optional_and_fail_otherwise() {
        let mut optional = reference("ConfigMap", "gone");
        optional.optional = true;
        let required = reference("ConfigMap", "also-gone");
        let lookups = HashMap::from([
            (("ConfigMap".into(), "gone".into()), Lookup::NotFound),
            (("ConfigMap".into(), "also-gone".into()), Lookup::NotFound),
        ]);
        let values = compose_values(&Map::new(), &[optional, required], &lookups, false);
        assert!(matches!(
            values.sources[0].outcome,
            SourceOutcome::Skipped(_)
        ));
        assert!(matches!(
            values.sources[1].outcome,
            SourceOutcome::Failed(_)
        ));
        assert_eq!(values.values, json!({}));
    }

    #[test]
    fn missing_key_and_forbidden_degrade_per_source() {
        let mut keyed = reference("ConfigMap", "cm");
        keyed.values_key = "other.yaml".into();
        let forbidden = reference("Secret", "locked");
        let lookups = HashMap::from([
            (
                ("ConfigMap".into(), "cm".into()),
                found("values.yaml", "a: 1"),
            ),
            (
                ("Secret".into(), "locked".into()),
                Lookup::Error("forbidden".into()),
            ),
        ]);
        let values = compose_values(&Map::new(), &[keyed, forbidden], &lookups, false);
        assert_eq!(
            values.sources[0].outcome,
            SourceOutcome::Failed("key 'other.yaml' not found".into())
        );
        assert_eq!(
            values.sources[1].outcome,
            SourceOutcome::Failed("forbidden".into())
        );
    }

    #[test]
    fn target_path_sets_typed_values() {
        let mut map = Map::new();
        replace_path_value(&mut map, "a.b", "true").unwrap();
        replace_path_value(&mut map, "a.n", "42").unwrap();
        replace_path_value(&mut map, "a.zero", "007").unwrap();
        replace_path_value(&mut map, "a.quoted", "'42'").unwrap();
        replace_path_value(&mut map, "a.list", "{x,y}").unwrap();
        replace_path_value(&mut map, "a.items[1].name", "second").unwrap();
        replace_path_value(&mut map, r"annotations.example\.com/key", "v").unwrap();
        assert_eq!(
            Value::Object(map),
            json!({
                "a": {
                    "b": true,
                    "n": 42,
                    "zero": "007",
                    "quoted": "42",
                    "list": ["x", "y"],
                    "items": [null, {"name": "second"}]
                },
                "annotations": {"example.com/key": "v"}
            })
        );
    }

    #[test]
    fn target_path_rejects_bad_paths() {
        let mut map = Map::new();
        assert!(replace_path_value(&mut map, "", "v").is_err());
        assert!(replace_path_value(&mut map, "a[x]", "v").is_err());
        assert!(replace_path_value(&mut map, "a[5000]", "v").is_err());
    }

    #[test]
    fn secret_target_path_is_redacted() {
        let mut secret = reference("Secret", "creds");
        secret.values_key = "password".into();
        secret.target_path = Some("db.password".into());
        let lookups = HashMap::from([(
            ("Secret".into(), "creds".into()),
            found("password", "hunter2"),
        )]);
        let values = compose_values(&Map::new(), &[secret], &lookups, false);
        assert_eq!(values.values, json!({"db": {"password": REDACTED}}));
        assert_eq!(values.sources[0].outcome, SourceOutcome::AppliedRedacted);
    }
}
