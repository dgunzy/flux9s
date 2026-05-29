//! Connectivity health checking and connection-error classification.
//!
//! Building a [`kube::Client`] does **not** perform any network I/O — it only
//! reads kubeconfig/in-cluster config and constructs an HTTP client. As a
//! result, an unreachable, timed-out, or misconfigured API server is not
//! detected until watch requests fail asynchronously in the background.
//!
//! This module provides an explicit, bounded health probe ([`check_connectivity`])
//! that hits the API server's `/version` endpoint (cheap, requires no RBAC) so
//! that connection problems are surfaced deterministically at startup, plus a
//! [`classify`] helper that maps raw errors to an actionable
//! [`ConnectionErrorKind`].
//!
//! This is the single source of truth for connection-error ergonomics: add new
//! patterns to [`classify_str`] / [`classify_kube_error`] as new failure modes
//! are observed.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use kube::config::Kubeconfig;

/// Default connection timeout, in seconds, when none is configured.
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Environment variable that overrides the configured connect timeout.
pub const CONNECT_TIMEOUT_ENV: &str = "FLUX9S_CONNECT_TIMEOUT";

/// A classified reason why connecting to the Kubernetes API server failed.
///
/// Used to render a clear, actionable message instead of a raw error string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionErrorKind {
    /// No kubeconfig could be found.
    KubeconfigMissing,
    /// A kubeconfig was found but could not be parsed.
    KubeconfigInvalid,
    /// The selected/current context is missing or invalid.
    ContextInvalid,
    /// The API server could not be reached (DNS, refused, no route, reset).
    Unreachable,
    /// The API server did not respond within the configured timeout.
    Timeout,
    /// The TLS handshake with the API server failed.
    TlsError,
    /// Authentication failed (e.g. expired or missing credentials).
    Unauthorized,
    /// Authenticated, but not permitted to access the cluster (RBAC).
    Forbidden,
    /// An unclassified failure; the underlying error is preserved for logs.
    Unknown,
}

impl ConnectionErrorKind {
    /// Short, human-readable summary of the failure.
    pub fn summary(&self) -> &'static str {
        match self {
            Self::KubeconfigMissing => "Kubeconfig not found",
            Self::KubeconfigInvalid => "Kubeconfig is invalid",
            Self::ContextInvalid => "Kubernetes context is invalid",
            Self::Unreachable => "Cannot reach the Kubernetes API server",
            Self::Timeout => "Connection to the Kubernetes API server timed out",
            Self::TlsError => "TLS handshake with the API server failed",
            Self::Unauthorized => "Not authorized to access the cluster",
            Self::Forbidden => "Access to the cluster is forbidden",
            Self::Unknown => "Failed to connect to the Kubernetes API server",
        }
    }

    /// Actionable remediation hint shown to the user.
    pub fn hint(&self) -> &'static str {
        match self {
            Self::KubeconfigMissing => {
                "Set KUBECONFIG or create ~/.kube/config, or pass --kubeconfig <path>."
            }
            Self::KubeconfigInvalid => {
                "Check that your kubeconfig is valid YAML with cluster, user, and context entries."
            }
            Self::ContextInvalid => {
                "Run `kubectl config get-contexts`, then select one with `kubectl config use-context`."
            }
            Self::Unreachable => {
                "Check that the cluster is running and reachable (VPN/network), e.g. `kubectl get nodes`."
            }
            Self::Timeout => {
                "The server did not respond in time. Check connectivity/VPN, or raise connectTimeoutSeconds."
            }
            Self::TlsError => {
                "Verify the cluster CA certificate and that the server URL is correct."
            }
            Self::Unauthorized => {
                "Your credentials may be expired. Re-authenticate (refresh your token or auth plugin)."
            }
            Self::Forbidden => {
                "Your user lacks the required RBAC permissions. Contact your cluster administrator."
            }
            Self::Unknown => "See the log file below for the full error details.",
        }
    }
}

/// A structured connection failure with enough context to show a clear message.
///
/// Carries the classified [`ConnectionErrorKind`], the offending context and
/// server URL (best-effort, for display), and the underlying error (preserved
/// for the detail line and logs).
#[derive(Debug)]
pub struct ConnectionError {
    /// The classified kind of failure.
    pub kind: ConnectionErrorKind,
    /// The Kubernetes context that was being connected to, if known.
    pub context: Option<String>,
    /// The API server URL, if it could be determined from the kubeconfig.
    pub server_url: Option<String>,
    /// The underlying error, preserved for logs and the detail line.
    pub source: anyhow::Error,
}

impl ConnectionError {
    /// Construct a connection error with an explicit kind and source.
    pub fn new(kind: ConnectionErrorKind, source: anyhow::Error) -> Self {
        Self {
            kind,
            context: None,
            server_url: None,
            source,
        }
    }

    /// Attach the context name for display.
    pub fn with_context(mut self, context: Option<String>) -> Self {
        self.context = context;
        self
    }

    /// Attach the server URL for display.
    pub fn with_server(mut self, server_url: Option<String>) -> Self {
        self.server_url = server_url;
        self
    }

    /// Classify an arbitrary error from the connection/bootstrap path.
    pub fn from_anyhow(err: anyhow::Error) -> Self {
        let kind = classify(&err);
        Self::new(kind, err)
    }

    /// The underlying error rendered as a single detail line (full cause chain).
    pub fn detail(&self) -> String {
        format!("{:#}", self.source)
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:#}", self.kind.summary(), self.source)
    }
}

impl std::error::Error for ConnectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Classify an error from the bootstrap/connection path into a [`ConnectionErrorKind`].
///
/// Prefers structured matching on [`kube::Error`] when present in the cause
/// chain, falling back to case-insensitive string matching for errors produced
/// earlier in the boot path (kubeconfig loading, context resolution).
pub fn classify(err: &anyhow::Error) -> ConnectionErrorKind {
    for cause in err.chain() {
        if let Some(kube_err) = cause.downcast_ref::<kube::Error>() {
            return classify_kube_error(kube_err);
        }
    }
    classify_str(&format!("{:#}", err))
}

/// Classify a [`kube::Error`] directly (used by the connectivity probe).
pub fn classify_kube_error(err: &kube::Error) -> ConnectionErrorKind {
    if let kube::Error::Api(resp) = err {
        match resp.code {
            401 => return ConnectionErrorKind::Unauthorized,
            403 => return ConnectionErrorKind::Forbidden,
            _ => {}
        }
    }
    if matches!(err, kube::Error::Auth(_)) {
        return ConnectionErrorKind::Unauthorized;
    }
    // Transport/config level: fall back to message inspection.
    classify_str(&err.to_string())
}

/// Classify a raw error message (case-insensitive) into a [`ConnectionErrorKind`].
///
/// Ordered most-specific first. Kubeconfig/context problems are checked before
/// generic connectivity keywords so a clear "context invalid" isn't mislabelled.
fn classify_str(msg: &str) -> ConnectionErrorKind {
    let m = msg.to_lowercase();

    // Kubeconfig / context problems (from our own bail! messages and the loader).
    if m.contains("no current context") {
        return ConnectionErrorKind::ContextInvalid;
    }
    if m.contains("context") && (m.contains("not found") || m.contains("does not exist")) {
        return ConnectionErrorKind::ContextInvalid;
    }
    if m.contains("kubeconfig") {
        if m.contains("does not exist") || m.contains("no such file") || m.contains("not found") {
            return ConnectionErrorKind::KubeconfigMissing;
        }
        if m.contains("parse")
            || m.contains("invalid")
            || m.contains("failed to load")
            || m.contains("malformed")
        {
            return ConnectionErrorKind::KubeconfigInvalid;
        }
    }

    // Timeouts.
    if m.contains("timed out") || m.contains("timeout") || m.contains("deadline") {
        return ConnectionErrorKind::Timeout;
    }

    // TLS / certificate.
    if m.contains("certificate")
        || m.contains("tls")
        || m.contains("ssl")
        || m.contains("handshake")
    {
        return ConnectionErrorKind::TlsError;
    }

    // Authentication / authorization.
    if m.contains("unauthorized") || m.contains("401") {
        return ConnectionErrorKind::Unauthorized;
    }
    if m.contains("forbidden") || m.contains("403") {
        return ConnectionErrorKind::Forbidden;
    }

    // Connectivity.
    if m.contains("connection refused")
        || m.contains("connection reset")
        || m.contains("no route to host")
        || m.contains("network is unreachable")
        || m.contains("network unreachable")
        || m.contains("dns")
        || m.contains("name resolution")
        || m.contains("failed to lookup")
        || m.contains("could not resolve")
        || m.contains("unreachable")
        || m.contains("broken pipe")
    {
        return ConnectionErrorKind::Unreachable;
    }

    ConnectionErrorKind::Unknown
}

/// Probe the API server for reachability with a bounded timeout.
///
/// Hits `/version` (via [`kube::Client::apiserver_version`]) which is cheap and
/// requires no RBAC. Returns the server version info on success, or a
/// classified [`ConnectionError`] on failure/timeout.
pub async fn check_connectivity(
    client: &kube::Client,
    timeout: Duration,
) -> Result<k8s_openapi::apimachinery::pkg::version::Info, ConnectionError> {
    match tokio::time::timeout(timeout, client.apiserver_version()).await {
        Ok(Ok(info)) => Ok(info),
        Ok(Err(e)) => {
            let kind = classify_kube_error(&e);
            Err(ConnectionError::new(kind, anyhow::Error::new(e)))
        }
        Err(_elapsed) => Err(ConnectionError::new(
            ConnectionErrorKind::Timeout,
            anyhow::anyhow!(
                "no response from the API server within {}s",
                timeout.as_secs()
            ),
        )),
    }
}

/// Resolve the effective connect timeout.
///
/// Precedence: `FLUX9S_CONNECT_TIMEOUT` env var > configured value > 1s floor.
pub fn resolve_connect_timeout(configured_secs: u64) -> Duration {
    resolve_connect_timeout_from_env(configured_secs, std::env::var(CONNECT_TIMEOUT_ENV).ok())
}

fn resolve_connect_timeout_from_env(configured_secs: u64, env_value: Option<String>) -> Duration {
    let secs = env_value
        .as_deref()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or_else(|| configured_secs.max(1));
    Duration::from_secs(secs)
}

/// Best-effort lookup of the API server URL for display purposes.
///
/// Parses the kubeconfig (no network I/O) and resolves the server for the given
/// context (or the current context). Returns `None` if it cannot be determined.
pub fn detect_cluster_server(
    kubeconfig_path: Option<&Path>,
    context: Option<&str>,
) -> Option<String> {
    let kc = match kubeconfig_path {
        Some(path) => Kubeconfig::read_from(path).ok()?,
        None => Kubeconfig::read().ok()?,
    };

    let ctx_name = context
        .map(|c| c.to_string())
        .or_else(|| kc.current_context.clone())?;

    let cluster_name = kc
        .contexts
        .iter()
        .find(|c| c.name == ctx_name)
        .and_then(|c| c.context.as_ref())
        .map(|c| c.cluster.clone())?;

    kc.clusters
        .iter()
        .find(|c| c.name == cluster_name)
        .and_then(|c| c.cluster.as_ref())
        .and_then(|c| c.server.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_kubeconfig_missing() {
        assert_eq!(
            classify_str("Kubeconfig file does not exist: /home/u/.kube/config"),
            ConnectionErrorKind::KubeconfigMissing
        );
        assert_eq!(
            classify_str("failed to load kubeconfig: no such file or directory"),
            ConnectionErrorKind::KubeconfigMissing
        );
    }

    #[test]
    fn test_classify_kubeconfig_invalid() {
        assert_eq!(
            classify_str("Failed to load or parse kubeconfig from /x: invalid yaml"),
            ConnectionErrorKind::KubeconfigInvalid
        );
    }

    #[test]
    fn test_classify_context_invalid() {
        assert_eq!(
            classify_str("No current context is set in kubeconfig /x"),
            ConnectionErrorKind::ContextInvalid
        );
        assert_eq!(
            classify_str("Context 'prod' not found. Available contexts: dev"),
            ConnectionErrorKind::ContextInvalid
        );
    }

    #[test]
    fn test_classify_timeout() {
        assert_eq!(
            classify_str("operation timed out"),
            ConnectionErrorKind::Timeout
        );
        assert_eq!(
            classify_str("deadline exceeded"),
            ConnectionErrorKind::Timeout
        );
    }

    #[test]
    fn test_classify_tls() {
        assert_eq!(
            classify_str("invalid certificate: unknown CA"),
            ConnectionErrorKind::TlsError
        );
        assert_eq!(
            classify_str("TLS handshake failed"),
            ConnectionErrorKind::TlsError
        );
    }

    #[test]
    fn test_classify_auth() {
        assert_eq!(
            classify_str("ApiError: Unauthorized (401)"),
            ConnectionErrorKind::Unauthorized
        );
        assert_eq!(
            classify_str("the server responded with 403 forbidden"),
            ConnectionErrorKind::Forbidden
        );
    }

    #[test]
    fn test_classify_unreachable() {
        assert_eq!(
            classify_str(
                "error trying to connect: tcp connect error: Connection refused (os error 61)"
            ),
            ConnectionErrorKind::Unreachable
        );
        assert_eq!(
            classify_str("failed to lookup address information: nodename nor servname provided"),
            ConnectionErrorKind::Unreachable
        );
        assert_eq!(
            classify_str("network is unreachable"),
            ConnectionErrorKind::Unreachable
        );
    }

    #[test]
    fn test_classify_unknown() {
        assert_eq!(
            classify_str("something completely unexpected happened"),
            ConnectionErrorKind::Unknown
        );
    }

    #[test]
    fn test_classify_anyhow_uses_string_fallback() {
        let err = anyhow::anyhow!("Context 'prod' not found. Available contexts: dev");
        assert_eq!(classify(&err), ConnectionErrorKind::ContextInvalid);
    }

    #[test]
    fn test_connection_error_display_includes_summary_and_source() {
        let err = ConnectionError::new(
            ConnectionErrorKind::Unreachable,
            anyhow::anyhow!("connection refused"),
        );
        let rendered = err.to_string();
        assert!(rendered.contains("Cannot reach the Kubernetes API server"));
        assert!(rendered.contains("connection refused"));
    }

    #[test]
    fn test_every_kind_has_summary_and_hint() {
        for kind in [
            ConnectionErrorKind::KubeconfigMissing,
            ConnectionErrorKind::KubeconfigInvalid,
            ConnectionErrorKind::ContextInvalid,
            ConnectionErrorKind::Unreachable,
            ConnectionErrorKind::Timeout,
            ConnectionErrorKind::TlsError,
            ConnectionErrorKind::Unauthorized,
            ConnectionErrorKind::Forbidden,
            ConnectionErrorKind::Unknown,
        ] {
            assert!(!kind.summary().is_empty());
            assert!(!kind.hint().is_empty());
        }
    }

    #[test]
    fn test_resolve_connect_timeout_uses_configured_when_no_env() {
        assert_eq!(
            resolve_connect_timeout_from_env(7, None),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn test_resolve_connect_timeout_floor() {
        assert_eq!(
            resolve_connect_timeout_from_env(0, None),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn test_resolve_connect_timeout_uses_env_override() {
        assert_eq!(
            resolve_connect_timeout_from_env(7, Some("12".to_string())),
            Duration::from_secs(12)
        );
        assert_eq!(
            resolve_connect_timeout_from_env(7, Some("0".to_string())),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn test_detect_cluster_server_from_explicit_kubeconfig_path() {
        use std::io::Write;

        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
apiVersion: v1
kind: Config
current-context: dev
clusters:
- name: dev-cluster
  cluster:
    server: https://dev.example.test
contexts:
- name: dev
  context:
    cluster: dev-cluster
    user: dev-user
users:
- name: dev-user
  user:
    token: test
"#
        )
        .unwrap();

        assert_eq!(
            detect_cluster_server(Some(file.path()), Some("dev")),
            Some("https://dev.example.test".to_string())
        );
    }

    #[test]
    fn test_detect_cluster_server_returns_none_for_unknown_context() {
        use std::io::Write;

        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
apiVersion: v1
kind: Config
current-context: dev
clusters:
- name: dev-cluster
  cluster:
    server: https://dev.example.test
contexts:
- name: dev
  context:
    cluster: dev-cluster
    user: dev-user
users:
- name: dev-user
  user:
    token: test
"#
        )
        .unwrap();

        assert_eq!(detect_cluster_server(Some(file.path()), Some("prod")), None);
    }
}
