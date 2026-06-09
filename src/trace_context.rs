use std::fmt::Write as _;

const TRACEPARENT_VERSION: &str = "00";
const DEFAULT_TRACE_FLAGS: &str = "00";
const MAX_TRACESTATE_ENTRIES: usize = 32;
const MAX_TRACESTATE_LEN: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub trace_flags: String,
    pub tracestate: Option<String>,
}

impl TraceContext {
    pub fn root() -> Self {
        Self {
            trace_id: random_hex_nonzero::<16>(),
            span_id: random_hex_nonzero::<8>(),
            trace_flags: DEFAULT_TRACE_FLAGS.to_string(),
            tracestate: None,
        }
    }

    pub fn from_headers(traceparent: Option<&str>, tracestate: Option<&str>) -> Self {
        let Some(parent) = traceparent else {
            return Self::root();
        };

        let Some(parsed) = parse_traceparent(parent) else {
            return Self::root();
        };

        Self {
            trace_id: parsed.trace_id,
            span_id: random_hex_nonzero::<8>(),
            trace_flags: parsed.trace_flags,
            tracestate: sanitize_tracestate(tracestate),
        }
    }

    pub fn traceparent(&self) -> String {
        format!(
            "{}-{}-{}-{}",
            TRACEPARENT_VERSION, self.trace_id, self.span_id, self.trace_flags
        )
    }

    pub fn outbound_child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: random_hex_nonzero::<8>(),
            trace_flags: self.trace_flags.clone(),
            tracestate: self.tracestate.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTraceparent {
    trace_id: String,
    trace_flags: String,
}

fn parse_traceparent(value: &str) -> Option<ParsedTraceparent> {
    let mut parts = value.split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let parent_id = parts.next()?;
    let trace_flags = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    if version != TRACEPARENT_VERSION {
        return None;
    }
    if !is_lower_hex(trace_id, 32) || is_all_zero(trace_id) {
        return None;
    }
    if !is_lower_hex(parent_id, 16) || is_all_zero(parent_id) {
        return None;
    }
    if !is_lower_hex(trace_flags, 2) {
        return None;
    }

    Some(ParsedTraceparent {
        trace_id: trace_id.to_string(),
        trace_flags: trace_flags.to_string(),
    })
}

fn sanitize_tracestate(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.is_empty() || value.bytes().any(|b| b.is_ascii_control()) {
        return None;
    }

    let mut entries = Vec::new();
    for entry in value.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if !is_valid_tracestate_entry(entry) {
            continue;
        }
        entries.push(entry.to_string());
    }

    while entries.len() > MAX_TRACESTATE_ENTRIES || entries.join(",").len() > MAX_TRACESTATE_LEN {
        entries.pop();
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries.join(","))
    }
}

fn is_valid_tracestate_entry(entry: &str) -> bool {
    let Some((key, value)) = entry.split_once('=') else {
        return false;
    };
    is_valid_tracestate_key(key) && is_valid_tracestate_value(value)
}

fn is_valid_tracestate_key(key: &str) -> bool {
    if key.is_empty() {
        return false;
    }

    if let Some((tenant_id, system_id)) = key.split_once('@') {
        return !system_id.contains('@')
            && is_valid_tracestate_key_part(tenant_id, 241, true)
            && is_valid_tracestate_key_part(system_id, 14, false);
    }

    is_valid_tracestate_key_part(key, 256, true)
}

fn is_valid_tracestate_key_part(part: &str, max_len: usize, allow_digit_first: bool) -> bool {
    if part.is_empty() || part.len() > max_len {
        return false;
    }

    let mut bytes = part.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    let first_valid = first.is_ascii_lowercase() || (allow_digit_first && first.is_ascii_digit());
    first_valid && bytes.all(is_valid_tracestate_key_tail_byte)
}

fn is_valid_tracestate_key_tail_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'*' | b'/')
}

fn is_valid_tracestate_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(is_valid_tracestate_value_byte)
        && value
            .bytes()
            .last()
            .is_some_and(|byte| matches!(byte, 0x21..=0x2b | 0x2d..=0x3c | 0x3e..=0x7e))
}

fn is_valid_tracestate_value_byte(byte: u8) -> bool {
    matches!(byte, 0x20..=0x2b | 0x2d..=0x3c | 0x3e..=0x7e)
}

fn is_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_all_zero(value: &str) -> bool {
    value.bytes().all(|b| b == b'0')
}

fn random_hex_nonzero<const N: usize>() -> String {
    loop {
        let mut bytes = [0_u8; N];
        getrandom::fill(&mut bytes).expect("OS random source unavailable");
        if bytes.iter().any(|byte| *byte != 0) {
            return hex_lower(&bytes);
        }
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TRACEPARENT: &str = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

    #[test]
    fn parses_valid_traceparent_and_creates_server_span() {
        let ctx =
            TraceContext::from_headers(Some(VALID_TRACEPARENT), Some("rojo=00f067aa0ba902b7"));
        assert_eq!(ctx.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(ctx.trace_flags, "01");
        assert_eq!(ctx.span_id.len(), 16);
        assert_ne!(ctx.span_id, "b7ad6b7169203331");
        assert_eq!(ctx.tracestate.as_deref(), Some("rojo=00f067aa0ba902b7"));
    }

    #[test]
    fn invalid_traceparent_generates_root_and_drops_tracestate() {
        let ctx = TraceContext::from_headers(Some("bad"), Some("rojo=00f067aa0ba902b7"));
        assert_eq!(ctx.trace_id.len(), 32);
        assert_eq!(ctx.span_id.len(), 16);
        assert_eq!(ctx.trace_flags, "00");
        assert!(ctx.tracestate.is_none());
    }

    #[test]
    fn rejects_zero_trace_or_parent_id() {
        let zero_trace = "00-00000000000000000000000000000000-b7ad6b7169203331-01";
        let zero_parent = "00-0af7651916cd43dd8448eb211c80319c-0000000000000000-01";
        assert_ne!(
            TraceContext::from_headers(Some(zero_trace), None).trace_id,
            "00000000000000000000000000000000"
        );
        assert_ne!(
            TraceContext::from_headers(Some(zero_parent), None).span_id,
            "0000000000000000"
        );
    }

    #[test]
    fn tracestate_truncates_trailing_entries_to_w3c_limits() {
        let entries: Vec<String> = (0..40).map(|idx| format!("k{idx}=v")).collect();
        let ctx = TraceContext::from_headers(Some(VALID_TRACEPARENT), Some(&entries.join(",")));
        let tracestate = ctx.tracestate.unwrap();
        assert!(tracestate.split(',').count() <= MAX_TRACESTATE_ENTRIES);
        assert!(tracestate.len() <= MAX_TRACESTATE_LEN);
    }

    #[test]
    fn tracestate_accepts_empty_members_and_multi_tenant_keys() {
        let ctx = TraceContext::from_headers(
            Some(VALID_TRACEPARENT),
            Some("rojo=00f067aa0ba902b7, ,tenant1@vendor=value"),
        );
        assert_eq!(
            ctx.tracestate.as_deref(),
            Some("rojo=00f067aa0ba902b7,tenant1@vendor=value")
        );
    }

    #[test]
    fn tracestate_ignores_invalid_members_and_preserves_valid_ones() {
        let ctx = TraceContext::from_headers(
            Some(VALID_TRACEPARENT),
            Some("rojo=00f067aa0ba902b7,Vendor=value,congo=t61rcWkgMzE"),
        );
        assert_eq!(
            ctx.tracestate.as_deref(),
            Some("rojo=00f067aa0ba902b7,congo=t61rcWkgMzE")
        );
    }

    #[test]
    fn tracestate_drops_header_when_no_valid_members_remain() {
        assert!(
            TraceContext::from_headers(
                Some(VALID_TRACEPARENT),
                Some("Vendor=value,vendor=bad=value")
            )
            .tracestate
            .is_none()
        );
    }

    #[test]
    fn outbound_child_preserves_trace_and_flags_with_new_span() {
        let ctx =
            TraceContext::from_headers(Some(VALID_TRACEPARENT), Some("rojo=00f067aa0ba902b7"));
        let child = ctx.outbound_child();
        assert_eq!(child.trace_id, ctx.trace_id);
        assert_eq!(child.trace_flags, ctx.trace_flags);
        assert_ne!(child.span_id, ctx.span_id);
        assert_eq!(child.tracestate, ctx.tracestate);
        assert!(
            child
                .traceparent()
                .starts_with("00-0af7651916cd43dd8448eb211c80319c-")
        );
    }
}
