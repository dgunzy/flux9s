//! A Flux list sort as written in the config (#306): `type`, `name`, `age`,
//! or `status`, reversed with a leading `-` (`-age`) or a `:desc` suffix
//! (`age:desc`). Used by `defaultSort` and a view's `sort`; `N` / `A` / `T`
//! / `S` change it at runtime.

/// What to sort by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// Namespace, kind, name — in a kind set, kind (in the set's order)
    /// first. What `N`/`A`/`T`/`S` return to.
    Default,
    Name,
    /// Oldest first.
    Age,
    /// By kind — in a kind set (`:ks,hr`) in the order given.
    Type,
    /// Problems first.
    Status,
}

/// A sort key and direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListSort {
    pub key: SortKey,
    pub reverse: bool,
}

/// The accepted spellings, for messages.
pub const SORT_VALUES: &str = "default, name, age, type, status (prefix - or add :desc to reverse)";

impl ListSort {
    /// Parse `type`, `-age`, `status:desc`, … `Err` carries the message.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim().to_lowercase();
        let (spec, mut reverse) = match spec.strip_prefix('-') {
            Some(rest) => (rest.to_string(), true),
            None => (spec, false),
        };
        let key = match spec.split_once(':') {
            Some((key, "desc")) => {
                reverse = !reverse;
                key
            }
            Some((key, "asc")) => key,
            Some(_) => return Err(format!("Unknown sort '{spec}'. Use {SORT_VALUES}")),
            None => spec.as_str(),
        };
        let key = match key {
            "default" => SortKey::Default,
            "name" => SortKey::Name,
            "age" => SortKey::Age,
            "type" | "kind" => SortKey::Type,
            "status" => SortKey::Status,
            _ => return Err(format!("Unknown sort '{key}'. Use {SORT_VALUES}")),
        };
        Ok(Self { key, reverse })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_and_directions() {
        let sort = |spec| ListSort::parse(spec).unwrap();
        assert_eq!(
            sort("type"),
            ListSort {
                key: SortKey::Type,
                reverse: false
            }
        );
        assert!(sort("-age").reverse && sort("age:desc").reverse);
        assert!(!sort("Name:ASC").reverse);
        assert_eq!(sort(" kind ").key, SortKey::Type);
        assert!(!sort("-status:desc").reverse, "two reversals cancel");
    }

    #[test]
    fn unknown_sorts_list_the_choices() {
        let err = ListSort::parse("size").unwrap_err();
        assert!(
            err.contains("Unknown sort 'size'") && err.contains("name, age"),
            "{err}"
        );
        assert!(ListSort::parse("age:sideways").is_err());
    }
}
