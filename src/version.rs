/// Public runtime name used in CLI and server output.
pub const MARRETA_NAME: &str = "MarretaLang";

/// Runtime/CLI version. Cargo.toml is the single source of truth.
pub const MARRETA_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Language compatibility floor (Spec 063): the most recent version that introduced a
/// **breaking** language/runtime change. `marreta init` stamps `requires_marreta =
/// ">=<COMPAT_FLOOR>"`, so a scaffold runs on any runtime from this floor up. The semver
/// policy (SPEC.md §1.5) advances this — in the same release — whenever a breaking change
/// ships; additive releases leave it unchanged. Always <= `MARRETA_VERSION`.
pub const COMPAT_FLOOR: &str = "0.2.0";

pub fn runtime_version_label() -> String {
    format!("{MARRETA_NAME} v{MARRETA_VERSION}")
}

/// Parses a plain semver `MAJOR.MINOR.PATCH` into a comparable tuple. Returns `None` for
/// anything else (wrong arity, non-numeric, `v` prefix, prerelease/build metadata).
pub fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Parses a `requires_marreta` value in the Spec 063 v1 format: `>=MAJOR.MINOR.PATCH`,
/// with optional surrounding whitespace and an optional single space after `>=`. Returns
/// the minimum version, or `None` if malformed (a `v` prefix, prerelease/build metadata,
/// a missing component, another operator, or a range).
pub fn parse_requires_marreta(value: &str) -> Option<(u64, u64, u64)> {
    let after_op = value.trim().strip_prefix(">=")?;
    // Allow exactly zero or one space after `>=` (the frozen format). Any other
    // whitespace — a second space, a tab, or whitespace inside/after the version — is
    // rejected.
    let version = after_op.strip_prefix(' ').unwrap_or(after_op);
    if version.contains(char::is_whitespace) {
        return None;
    }
    parse_version(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_version_comes_from_cargo_package_version() {
        assert_eq!(MARRETA_VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            runtime_version_label(),
            format!("MarretaLang v{MARRETA_VERSION}")
        );
    }

    #[test]
    fn compat_floor_is_valid_and_not_above_runtime() {
        let floor = parse_version(COMPAT_FLOOR).expect("COMPAT_FLOOR is valid semver");
        let runtime = parse_version(MARRETA_VERSION).expect("runtime version is valid semver");
        assert!(
            floor <= runtime,
            "COMPAT_FLOOR must be <= the runtime version"
        );
    }

    #[test]
    fn parse_version_accepts_plain_semver_only() {
        assert_eq!(parse_version("0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("10.20.30"), Some((10, 20, 30)));
        assert_eq!(parse_version("0.2"), None); // missing patch
        assert_eq!(parse_version("0.2.0.1"), None); // extra component
        assert_eq!(parse_version("v0.2.0"), None); // v prefix
        assert_eq!(parse_version("0.2.0-rc1"), None); // prerelease
        assert_eq!(parse_version("0.2.x"), None); // non-numeric
    }

    #[test]
    fn parse_requires_marreta_accepts_min_and_rejects_garbage() {
        // Accepted forms.
        assert_eq!(parse_requires_marreta(">=0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_requires_marreta("  >=0.2.0  "), Some((0, 2, 0))); // outer trim
        assert_eq!(parse_requires_marreta(">= 0.2.0"), Some((0, 2, 0))); // one space after >=
        // Rejected forms.
        assert_eq!(parse_requires_marreta(">=  0.2.0"), None); // two spaces after >=
        assert_eq!(parse_requires_marreta(">=\t0.2.0"), None); // tab after >=
        assert_eq!(parse_requires_marreta("0.2.0"), None); // no operator
        assert_eq!(parse_requires_marreta(">0.2.0"), None); // wrong operator
        assert_eq!(parse_requires_marreta("^0.2.0"), None);
        assert_eq!(parse_requires_marreta("~0.2.0"), None);
        assert_eq!(parse_requires_marreta(">=v0.2.0"), None); // v prefix
        assert_eq!(parse_requires_marreta(">=0.2.0-rc1"), None); // prerelease
        assert_eq!(parse_requires_marreta(">=0.2"), None); // missing component
        assert_eq!(parse_requires_marreta(">=0.2.0 extra"), None); // trailing junk
        assert_eq!(parse_requires_marreta(">="), None); // no version
    }
}
