//! Where a Flux object's artifact comes from — the **one** reading of source
//! references. Trace, graph, the app tree, and reconcile-with-source all go
//! through it, so they can't disagree about what an object's source is.
//!
//! It follows the Flux CLI and Flux Operator (`sourceRefOf`,
//! `traceBuiltFrom`, `GetHelmChartTemplateReference`):
//!
//! - [`source_ref`]: what an object pulls from, one hop — a Kustomization's
//!   or HelmChart's `spec.sourceRef`; a HelmRelease's `spec.chartRef`, else
//!   the source generated from its `spec.chart` template.
//! - [`producer_ref`]: what wrote an ExternalArtifact (its optional
//!   `spec.sourceRef`, e.g. an ArtifactGenerator).
//! - [`built_from`]: the sources a producer combines (`spec.sources`).
//! - [`origin_url`]: the external origin an artifact records.
//!
//! Every reference resolves to a Flux kind's canonical spelling; a reference
//! to anything else is not followed.

use serde_json::Value;

use crate::models::FluxResourceKind;
use crate::watcher::ResourceKey;

/// The `{kind, name, namespace?}` reference at `pointer`, its namespace
/// defaulting to `namespace` (the referring object's).
fn reference(obj: &Value, pointer: &str, namespace: &str) -> Option<ResourceKey> {
    reference_value(obj.pointer(pointer)?, namespace)
}

fn reference_value(reference: &Value, namespace: &str) -> Option<ResourceKey> {
    let field = |name: &str| {
        reference
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };
    flux_key(
        field("kind")?,
        field("namespace").unwrap_or(namespace),
        field("name")?,
    )
}

/// A key for a Flux kind, in its canonical spelling; `None` for any other kind.
fn flux_key(kind: &str, namespace: &str, name: &str) -> Option<ResourceKey> {
    let kind = FluxResourceKind::parse_optional(kind)?;
    Some(ResourceKey::new(kind.as_str(), namespace, name))
}

/// What `obj` (a `kind` in `namespace`) pulls its artifact from, one hop.
/// A HelmRelease's generated HelmChart is a hop of its own; follow it with
/// [`is_intermediate`] to reach the repository.
pub fn source_ref(kind: FluxResourceKind, obj: &Value, namespace: &str) -> Option<ResourceKey> {
    use FluxResourceKind as K;
    match kind {
        K::Kustomization | K::HelmChart => reference(obj, "/spec/sourceRef", namespace),
        K::HelmRelease => helm_release_source(obj, namespace),
        // Fetch from outside the cluster, produce artifacts, or don't pull
        // from a source at all. (ImageUpdateAutomation writes *to* its Git
        // repository; it isn't sourced from it.)
        K::GitRepository
        | K::OCIRepository
        | K::HelmRepository
        | K::Bucket
        | K::ExternalArtifact
        | K::ArtifactGenerator
        | K::ImageRepository
        | K::ImagePolicy
        | K::ImageUpdateAutomation
        | K::Alert
        | K::Provider
        | K::Receiver
        | K::ResourceSet
        | K::ResourceSetInputProvider
        | K::FluxReport
        | K::FluxInstance => None,
    }
}

/// Whether a source of `kind` stands between an applier and the artifact's
/// origin, and is resolved through: a HelmChart (built from a repository).
pub fn is_intermediate(kind: FluxResourceKind) -> bool {
    kind == FluxResourceKind::HelmChart
}

/// A HelmRelease's chart source: `spec.chartRef`; else the source the
/// controller generated from `spec.chart` — named in `status.helmChart`,
/// else derived the way helm-controller names it.
fn helm_release_source(obj: &Value, namespace: &str) -> Option<ResourceKey> {
    if let Some(chart_ref) = reference(obj, "/spec/chartRef", namespace) {
        return Some(chart_ref);
    }
    if let Some(generated) = obj.pointer("/status/helmChart").and_then(Value::as_str) {
        let key = crate::kube::standing::generated_chart_source(generated, namespace);
        return flux_key(&key.resource_type, &key.namespace, &key.name);
    }
    chart_template_reference(obj, namespace)
}

/// The source helm-controller generates from a HelmRelease's `spec.chart`
/// (`GetHelmChartTemplateReference`): `<namespace>-<name>`, an
/// OCIRepository in the release's namespace when `spec.chart.kind` says so,
/// else a HelmChart beside the chart's repository.
fn chart_template_reference(obj: &Value, namespace: &str) -> Option<ResourceKey> {
    let chart = obj.pointer("/spec/chart")?;
    let name = obj.pointer("/metadata/name").and_then(Value::as_str)?;
    let generated_name = format!("{namespace}-{name}");
    let oci = FluxResourceKind::OCIRepository;
    if chart
        .get("kind")
        .and_then(Value::as_str)
        .and_then(FluxResourceKind::parse_optional)
        == Some(oci)
    {
        return Some(ResourceKey::new(oci.as_str(), namespace, &generated_name));
    }
    let chart_ns = chart
        .pointer("/spec/sourceRef/namespace")
        .and_then(Value::as_str)
        .filter(|ns| !ns.is_empty())
        .unwrap_or(namespace);
    Some(ResourceKey::new(
        FluxResourceKind::HelmChart.as_str(),
        chart_ns,
        &generated_name,
    ))
}

/// The repository a HelmRelease's chart template names
/// (`spec.chart.spec.sourceRef`) — what its generated HelmChart pulls from,
/// readable before the HelmChart itself is fetched.
pub fn chart_template_source(obj: &Value, namespace: &str) -> Option<ResourceKey> {
    reference(obj, "/spec/chart/spec/sourceRef", namespace)
}

/// What produced an ExternalArtifact: its `spec.sourceRef` (e.g. an
/// ArtifactGenerator). Optional — an artifact may name no producer.
pub fn producer_ref(kind: FluxResourceKind, obj: &Value, namespace: &str) -> Option<ResourceKey> {
    (kind == FluxResourceKind::ExternalArtifact)
        .then(|| reference(obj, "/spec/sourceRef", namespace))
        .flatten()
}

/// The sources a producer combines into its artifacts: an
/// ArtifactGenerator's `spec.sources`, each defaulting to its namespace.
pub fn built_from(kind: FluxResourceKind, obj: &Value, namespace: &str) -> Vec<ResourceKey> {
    if kind != FluxResourceKind::ArtifactGenerator {
        return Vec::new();
    }
    obj.pointer("/spec/sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|source| reference_value(source, namespace))
        .collect()
}

/// The origin an artifact records (`status.artifact.metadata`
/// `org.opencontainers.image.source`) — e.g. the Git repository an OCI
/// artifact was pushed from.
pub fn origin_url(obj: &Value) -> Option<String> {
    obj.pointer("/status/artifact/metadata/org.opencontainers.image.source")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(kind: &str, namespace: &str, name: &str) -> Option<ResourceKey> {
        Some(ResourceKey::new(kind, namespace, name))
    }

    #[test]
    fn a_kustomization_pulls_from_its_source_ref() {
        let ks = json!({"spec": {"sourceRef": {"kind": "ExternalArtifact", "name": "apps",
            "namespace": "flux-system"}}});
        assert_eq!(
            source_ref(FluxResourceKind::Kustomization, &ks, "team-a"),
            key("ExternalArtifact", "flux-system", "apps")
        );
        let local = json!({"spec": {"sourceRef": {"kind": "GitRepository", "name": "repo"}}});
        assert_eq!(
            source_ref(FluxResourceKind::Kustomization, &local, "apps"),
            key("GitRepository", "apps", "repo"),
            "the namespace defaults to the object's"
        );
        let foreign = json!({"spec": {"sourceRef": {"kind": "Widget", "name": "x"}}});
        assert_eq!(
            source_ref(FluxResourceKind::Kustomization, &foreign, "apps"),
            None
        );
    }

    #[test]
    fn a_helm_release_pulls_from_its_chart_ref_else_its_generated_source() {
        let hr = FluxResourceKind::HelmRelease;
        let by_ref = json!({"spec": {"chartRef": {"kind": "OCIRepository", "name": "podinfo"}},
            "status": {"helmChart": "apps/apps-podinfo"}});
        assert_eq!(
            source_ref(hr, &by_ref, "apps"),
            key("OCIRepository", "apps", "podinfo")
        );

        let template = json!({"metadata": {"name": "podinfo"},
            "spec": {"chart": {"spec": {"chart": "podinfo", "sourceRef":
                {"kind": "HelmRepository", "name": "podinfo", "namespace": "flux-system"}}}}});
        let mut generated = template.clone();
        generated["status"] = json!({"helmChart": "flux-system/apps-podinfo"});
        assert_eq!(
            source_ref(hr, &generated, "apps"),
            key("HelmChart", "flux-system", "apps-podinfo")
        );
        assert_eq!(
            source_ref(hr, &template, "apps"),
            key("HelmChart", "flux-system", "apps-podinfo"),
            "before status names it, derived the way helm-controller names it"
        );
        assert_eq!(
            chart_template_source(&template, "apps"),
            key("HelmRepository", "flux-system", "podinfo")
        );

        let mut oci = template.clone();
        oci["spec"]["chart"]["kind"] = json!("OCIRepository");
        assert_eq!(
            source_ref(hr, &oci, "apps"),
            key("OCIRepository", "apps", "apps-podinfo")
        );
        let mut oci_status = template;
        oci_status["status"] = json!({"helmChart": "OCIRepository/apps/apps-podinfo"});
        assert_eq!(
            source_ref(hr, &oci_status, "apps"),
            key("OCIRepository", "apps", "apps-podinfo")
        );
    }

    #[test]
    fn a_helm_chart_is_resolved_through_to_its_repository() {
        let chart = json!({"spec": {"sourceRef": {"kind": "HelmRepository", "name": "podinfo"}}});
        assert!(is_intermediate(FluxResourceKind::HelmChart));
        assert!(!is_intermediate(FluxResourceKind::ExternalArtifact));
        assert_eq!(
            source_ref(FluxResourceKind::HelmChart, &chart, "flux-system"),
            key("HelmRepository", "flux-system", "podinfo")
        );
    }

    #[test]
    fn an_external_artifact_names_its_producer_which_names_its_sources() {
        let artifact = json!({"spec": {"sourceRef": {"apiVersion": "source.extensions.fluxcd.io/v1beta1",
            "kind": "ArtifactGenerator", "name": "gen"}}});
        let ea = FluxResourceKind::ExternalArtifact;
        assert_eq!(
            producer_ref(ea, &artifact, "flux-system"),
            key("ArtifactGenerator", "flux-system", "gen")
        );
        assert_eq!(source_ref(ea, &artifact, "flux-system"), None);
        // spec.sourceRef is optional: an artifact may name no producer.
        assert_eq!(producer_ref(ea, &json!({"spec": {}}), "flux-system"), None);

        let generator = json!({"spec": {"sources": [
            {"alias": "repo", "kind": "GitRepository", "name": "repo"},
            {"alias": "oci", "kind": "OCIRepository", "name": "charts", "namespace": "shared"},
            {"alias": "x", "kind": "NotAFluxKind", "name": "x"}]}});
        assert_eq!(
            built_from(
                FluxResourceKind::ArtifactGenerator,
                &generator,
                "flux-system"
            ),
            vec![
                ResourceKey::new("GitRepository", "flux-system", "repo"),
                ResourceKey::new("OCIRepository", "shared", "charts"),
            ]
        );
        assert!(built_from(ea, &generator, "flux-system").is_empty());
    }

    #[test]
    fn the_origin_is_the_artifacts_image_source() {
        let oci = json!({"status": {"artifact": {"metadata": {
            "org.opencontainers.image.source": "https://github.com/stefanprodan/podinfo"}}}});
        assert_eq!(
            origin_url(&oci).as_deref(),
            Some("https://github.com/stefanprodan/podinfo")
        );
        assert_eq!(origin_url(&json!({"status": {}})), None);
    }
}
