use std::collections::HashMap;

pub const FEATURE_ENV_PREFIX: &str = "MARRETA_FEATURE_";
pub const FEATURE_NAME_REGEX: &str = "^[a-z][a-z0-9]*(_[a-z0-9]+)*$";
pub const FEATURE_NAME_HELP: &str = concat!(
    "use lowercase letters, digits, and single underscores; ",
    "start with a letter; ",
    "do not use double underscores or trailing underscores"
);
pub const FEATURE_ENV_NAME_HELP: &str = concat!(
    "after MARRETA_FEATURE_, use uppercase letters, digits, and single underscores; ",
    "start with a letter; ",
    "do not use double underscores or trailing underscores"
);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureFlags {
    flags: HashMap<String, bool>,
}

impl FeatureFlags {
    pub fn new(flags: HashMap<String, bool>) -> Self {
        Self { flags }
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }

    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }

    pub fn entries(&self) -> Vec<(&str, bool)> {
        let mut entries: Vec<_> = self
            .flags
            .iter()
            .map(|(name, enabled)| (name.as_str(), *enabled))
            .collect();
        entries.sort_by_key(|(name, _)| *name);
        entries
    }
}

pub fn is_valid_feature_name(name: &str) -> bool {
    // Keep this manual validator in sync with FEATURE_NAME_REGEX. Avoiding a
    // regex dependency keeps config parsing lightweight and deterministic.
    let mut parts = name.split('_');
    let Some(first) = parts.next() else {
        return false;
    };
    if first.is_empty()
        || !first.as_bytes()[0].is_ascii_lowercase()
        || !first
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return false;
    }

    parts.all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

pub fn normalize_env_feature_name(env_key: &str) -> Option<String> {
    env_key
        .strip_prefix(FEATURE_ENV_PREFIX)
        .map(|suffix| suffix.to_ascii_lowercase())
}

pub fn env_key_for_feature_name(name: &str) -> String {
    format!("{}{}", FEATURE_ENV_PREFIX, name.to_ascii_uppercase())
}

pub fn parse_feature_bool(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" | "enabled" => Ok(true),
        "false" | "0" | "no" | "off" | "disabled" => Ok(false),
        _ => Err(raw.to_string()),
    }
}

pub fn load_feature_flags(vars: &HashMap<String, String>) -> (FeatureFlags, Vec<String>) {
    let mut flags = HashMap::new();
    let mut errors = Vec::new();

    for (key, value) in vars {
        let Some(name) = normalize_env_feature_name(key) else {
            continue;
        };

        if !is_valid_feature_name(&name) {
            errors.push(format!("invalid {}: {}", key, FEATURE_ENV_NAME_HELP));
            continue;
        }

        match parse_feature_bool(value) {
            Ok(enabled) => {
                flags.insert(name, enabled);
            }
            Err(raw) => errors.push(format!("{} has invalid boolean value '{}'", key, raw)),
        }
    }

    (FeatureFlags::new(flags), errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_feature_names_are_lower_snake_case_without_empty_segments() {
        for name in ["inventory_api", "a", "a1", "low_stock_alert"] {
            assert!(is_valid_feature_name(name), "{name}");
        }

        for name in [
            "",
            "_inventory",
            "inventory_",
            "inventory__api",
            "Inventory",
            "inventory-api",
            "1inventory",
        ] {
            assert!(!is_valid_feature_name(name), "{name}");
        }
    }

    #[test]
    fn feature_name_validator_matches_documented_regex_examples() {
        let valid = ["a", "a1", "inventory_api", "low_stock_alert"];
        let invalid = [
            "",
            "A",
            "_a",
            "a_",
            "a__b",
            "a-b",
            "1a",
            "inventory__api",
            "inventory_api_",
        ];

        for name in valid {
            assert!(
                is_valid_feature_name(name),
                "{name} should match {FEATURE_NAME_REGEX}"
            );
        }
        for name in invalid {
            assert!(
                !is_valid_feature_name(name),
                "{name} should not match {FEATURE_NAME_REGEX}"
            );
        }
    }

    #[test]
    fn parse_feature_bool_accepts_documented_values() {
        for value in ["true", "1", "yes", "on", "enabled", " TRUE "] {
            assert!(parse_feature_bool(value).unwrap());
        }

        for value in ["false", "0", "no", "off", "disabled", " FALSE "] {
            assert!(!parse_feature_bool(value).unwrap());
        }
    }

    #[test]
    fn parse_feature_bool_rejects_empty_and_unknown_values() {
        assert!(parse_feature_bool("").is_err());
        assert!(parse_feature_bool("maybe").is_err());
    }

    #[test]
    fn load_feature_flags_normalizes_names_and_collects_errors() {
        let vars = HashMap::from([
            (
                "MARRETA_FEATURE_INVENTORY_API".to_string(),
                "true".to_string(),
            ),
            ("MARRETA_FEATURE_LOW_STOCK".to_string(), "0".to_string()),
            ("MARRETA_FEATURE_BAD__NAME".to_string(), "true".to_string()),
            ("MARRETA_FEATURE_EMPTY".to_string(), "".to_string()),
            ("OTHER".to_string(), "true".to_string()),
        ]);

        let (flags, errors) = load_feature_flags(&vars);
        assert!(flags.enabled("inventory_api"));
        assert!(!flags.enabled("low_stock"));
        assert!(!flags.enabled("missing"));
        assert_eq!(errors.len(), 2);
        assert!(
            errors
                .iter()
                .any(|err| err.contains("MARRETA_FEATURE_BAD__NAME"))
        );
        assert!(
            errors
                .iter()
                .any(|err| err.contains("MARRETA_FEATURE_EMPTY"))
        );
    }
}
