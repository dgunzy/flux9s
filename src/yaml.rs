//! YAML parsing and rendering for all of flux9s, in one place.
//!
//! Backed by [serde-saphyr](https://crates.io/crates/serde-saphyr), a YAML
//! implementation in safe Rust (it replaced the deprecated `serde_yaml`, whose
//! `unsafe-libyaml` parser is unmaintained — RUSTSEC-2024-0449, #296).
//! Dynamic documents use `serde_json::Value`: Kubernetes objects are
//! JSON-shaped, and with serde_json's `preserve_order` feature keys keep their
//! document order (config field order, API server field order).
//!
//! Scalar typing for untyped targets (`serde_json::Value`):
//! - Booleans are only `true` / `false`, as YAML 1.2 (and `serde_yaml`) had
//!   it — `yes` / `on` stay strings, so an unquoted namespace like `on` in a
//!   config file still loads. Rendering quotes them, so round trips are safe.
//! - Leading-zero numbers are octal, as kubectl and Helm read them
//!   (`sigs.k8s.io/yaml`): `defaultMode: 0644` in an edited object means 420,
//!   exactly what `kubectl edit` would apply.
//!
//! Parse errors are single-line (`line L column C: …`) so they fit a status
//! line or an `anyhow` context chain.

use serde::Serialize;
use serde::de::DeserializeOwned;

pub use serde_saphyr::{Error, SerializeError};

fn options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        strict_booleans: true,
        legacy_octal_numbers: true,
        with_snippet: false,
    }
}

/// Parse one YAML document (JSON is valid YAML too).
pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T, Error> {
    serde_saphyr::from_str_with_options(input, options())
}

/// Render a value as a YAML document.
pub fn to_string<T: Serialize>(value: &T) -> Result<String, SerializeError> {
    serde_saphyr::to_string(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn booleans_are_yaml_1_2_and_octal_follows_kubectl() {
        let v: Value = from_str("a: yes\nb: on\nc: true\nmode: 0644\nn: 42\ns: '0644'\n").unwrap();
        assert_eq!(
            v,
            json!({"a": "yes", "b": "on", "c": true, "mode": 420, "n": 42, "s": "0644"})
        );
    }

    #[test]
    fn rendering_quotes_ambiguous_strings_so_they_round_trip() {
        let original = json!({
            "on": "yes",
            "mode": "0644",
            "empty": "",
            "tilde": "~",
            "multi": "line one\nline two\n",
            "list": [],
            "map": {},
        });
        let yaml = to_string(&original).unwrap();
        let back: Value = from_str(&yaml).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn keys_keep_document_order() {
        let v: Value = from_str("kind: Deployment\napiVersion: apps/v1\nmetadata: {}\n").unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["kind", "apiVersion", "metadata"]);
    }

    #[test]
    fn parse_errors_are_one_line_with_a_position() {
        let err = from_str::<Value>("a: [1, 2\nb: c").unwrap_err().to_string();
        assert!(!err.contains('\n'), "{err}");
        assert!(err.contains("line 2"), "{err}");
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        assert!(from_str::<Value>("a: 1\na: 2\n").is_err());
    }
}
