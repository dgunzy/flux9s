//! `status.conditions` lookups: the one reader every kind's Ready /
//! Reconciling / Stalled checks go through, instead of each module walking
//! the array itself.

use serde_json::Value;

pub const READY: &str = "Ready";
pub const RECONCILING: &str = "Reconciling";
pub const STALLED: &str = "Stalled";

/// One entry of a `conditions` array.
#[derive(Debug, Clone, Copy)]
pub struct Condition<'a>(&'a Value);

impl<'a> Condition<'a> {
    fn str_field(self, key: &str) -> Option<&'a str> {
        self.0.get(key).and_then(Value::as_str)
    }

    /// `True`, `False`, or `Unknown`.
    pub fn status(self) -> Option<&'a str> {
        self.str_field("status")
    }

    /// Whether the status is `True`.
    pub fn is_true(self) -> bool {
        self.status() == Some("True")
    }

    /// Whether the status is `False`.
    pub fn is_false(self) -> bool {
        self.status() == Some("False")
    }

    pub fn reason(self) -> Option<&'a str> {
        self.str_field("reason")
    }

    pub fn message(self) -> Option<&'a str> {
        self.str_field("message")
    }

    /// Any other field (`observedGeneration`, `lastTransitionTime`).
    pub fn get(self, key: &str) -> Option<&'a Value> {
        self.0.get(key)
    }
}

/// The condition of `type_` in a `conditions` array.
pub fn of_type<'a>(conditions: &'a Value, type_: &str) -> Option<Condition<'a>> {
    conditions
        .as_array()?
        .iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some(type_))
        .map(Condition)
}

/// The condition of `type_` on an object (`status.conditions`).
pub fn find<'a>(obj: &'a Value, type_: &str) -> Option<Condition<'a>> {
    of_type(obj.pointer("/status/conditions")?, type_)
}

/// Whether the object's condition of `type_` is `True`.
pub fn is_true(obj: &Value, type_: &str) -> bool {
    find(obj, type_).is_some_and(Condition::is_true)
}

/// The object's Ready condition as a bool; `None` when it has none.
pub fn is_ready(obj: &Value) -> Option<bool> {
    find(obj, READY).map(Condition::is_true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_conditions_by_type() {
        let obj = json!({"status": {"conditions": [
            {"type": "Reconciling", "status": "True", "message": "Applying step 1/2"},
            {"type": "Ready", "status": "False", "reason": "Progressing", "observedGeneration": 3},
        ]}});
        assert_eq!(is_ready(&obj), Some(false));
        assert!(is_true(&obj, RECONCILING));
        let ready = find(&obj, READY).unwrap();
        assert!(ready.is_false());
        assert_eq!(ready.reason(), Some("Progressing"));
        assert_eq!(ready.get("observedGeneration"), Some(&json!(3)));
        assert_eq!(
            find(&obj, RECONCILING).and_then(Condition::message),
            Some("Applying step 1/2")
        );
        assert!(find(&obj, STALLED).is_none());
    }

    #[test]
    fn objects_without_conditions_have_none() {
        assert_eq!(is_ready(&json!({})), None);
        assert_eq!(is_ready(&json!({"status": {"conditions": "bogus"}})), None);
        assert!(!is_true(&json!({"status": {}}), READY));
    }
}
