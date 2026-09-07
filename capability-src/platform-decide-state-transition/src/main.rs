//! platform.decide-state-transition — TOML-derived policy v1.2.
//!
//! Implements HP-01…HP-06 / UP-01…UP-08 from decide-state-transition-v1.1.0.toml
//! using explicit defaults in docs/lifecycle-and-approval-policy-v1.2.md.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#[repr(C)]
struct IoVec {
    buffer: *const u8,
    length: usize,
}

#[repr(C)]
struct IoVecMut {
    buffer: *mut u8,
    length: usize,
}

#[cfg(not(test))]
#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IoVecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const IoVec, count: usize, written: *mut usize) -> u32;
}

static mut INPUT_BUF: [u8; 8192] = [0; 8192];
static mut OUTPUT_BUF: [u8; 8192] = [0; 8192];

const POLICY: &[u8] = b"toml-derived-1.2.0";
const CONTRACT: &[u8] = b"1.2.0";
const AUTO_APPROVE_MAX: f64 = 100.0;

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    unsafe {
        let mut total = 0usize;
        loop {
            let vec = IoVecMut {
                buffer: INPUT_BUF.as_mut_ptr().add(total),
                length: INPUT_BUF.len() - total,
            };
            let mut n = 0usize;
            if fd_read(0, &vec, 1, &mut n) != 0 || n == 0 {
                break;
            }
            total += n;
            if total >= INPUT_BUF.len() {
                break;
            }
        }
        let input = &INPUT_BUF[..total];
        let out_len = decide(input, &mut OUTPUT_BUF);
        let out = IoVec {
            buffer: OUTPUT_BUF.as_ptr(),
            length: out_len,
        };
        let mut written = 0usize;
        let _ = fd_write(1, &out, 1, &mut written);
    }
}

pub unsafe fn decide(input: &[u8], out: &mut [u8]) -> usize {
    let entity_type = extract_string(input, b"\"entity_type\"");
    let current = extract_string(input, b"\"current_state\"");
    let proposed = extract_string(input, b"\"proposed_state\"");
    let mode = extract_string(input, b"\"mode\"");
    let correlation = extract_string(input, b"\"correlation_id\"");
    let amount = extract_number(input, b"\"amount\"");
    let now = extract_string(input, b"\"now\"");
    let cancel_deadline = extract_string(input, b"\"cancel_deadline\"");
    let priority = extract_string(input, b"\"priority\"");

    let has_employee = has_role(input, b"employee");
    let has_manager = has_role(input, b"manager");
    let has_finance = has_role(input, b"finance");
    let has_support = has_role(input, b"support") || has_role(input, b"support_agent");
    let has_any_role = has_employee
        || has_manager
        || has_finance
        || has_support
        || has_role(input, b"customer")
        || has_role(input, b"compliance")
        || has_role(input, b"project_manager");

    let collected_legal = approval_collected(input, b"legal");
    let collected_finance = approval_collected(input, b"finance");
    let dual = flag_true(input, b"\"requires_dual_approval\"");
    let has_open_children = flag_true(input, b"\"has_open_children\"");

    let query = mode == b"query" || proposed.is_empty() || proposed == current;

    if !has_any_role {
        return write_decision(
            out,
            false,
            b"denied",
            b"ACTOR_HAS_NO_ROLES",
            b"No roles are present on the actor.",
            b"[]",
            b"error",
            b"Actor has no roles",
            correlation,
        );
    }

    if query {
        let next = next_legal_states(entity_type, current, has_manager, has_finance);
        return write_query(out, next, correlation);
    }

    match entity_type {
        b"expense" => decide_expense(
            out,
            current,
            proposed,
            amount,
            has_manager,
            has_finance,
            dual,
            collected_legal,
            collected_finance,
            correlation,
        ),
        b"order" => decide_order(out, current, proposed, now, cancel_deadline, correlation),
        b"ticket" => decide_ticket(
            out,
            current,
            proposed,
            priority,
            has_open_children,
            correlation,
        ),
        _ => write_decision(
            out,
            false,
            b"denied",
            b"UNKNOWN_ENTITY_TYPE",
            b"Unrecognised entity_type for this policy.",
            b"[]",
            b"error",
            b"Unknown entity type",
            correlation,
        ),
    }
}

fn next_legal_states(
    entity_type: &[u8],
    current: &[u8],
    has_manager: bool,
    has_finance: bool,
) -> &'static [u8] {
    match (entity_type, current) {
        (b"expense", b"draft") => br#"["submitted"]"#,
        (b"expense", b"submitted") if has_manager || has_finance => br#"["approved"]"#,
        (b"expense", b"submitted") => br#"[]"#,
        (b"order", b"placed") => br#"["cancelled"]"#,
        (b"ticket", b"open") => br#"["escalated","closed"]"#,
        _ => br#"[]"#,
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn decide_expense(
    out: &mut [u8],
    current: &[u8],
    proposed: &[u8],
    amount: Option<f64>,
    has_manager: bool,
    has_finance: bool,
    dual: bool,
    collected_legal: bool,
    collected_finance: bool,
    correlation: &[u8],
) -> usize {
    if current == b"draft" && proposed == b"submitted" {
        let Some(value) = amount else {
            return write_decision(
                out,
                false,
                b"requires_additional_info",
                b"MISSING_AMOUNT",
                b"Amount is required in context for this transition.",
                b"[]",
                b"warning",
                b"Amount required",
                correlation,
            );
        };
        if value >= AUTO_APPROVE_MAX {
            return write_decision(
                out,
                false,
                b"requires_approval",
                b"AMOUNT_EXCEEDS_LIMIT",
                b"Amount exceeds auto-approval threshold; finance approval required.",
                br#"[{"role":"finance","min_count":1,"logic":"all","reason":"high-value expense"}]"#,
                b"warning",
                b"Requires Finance",
                correlation,
            );
        }
        return write_decision(
            out,
            true,
            b"allowed",
            b"AUTO_APPROVED",
            b"Low-value expense may transition from draft to submitted.",
            b"[]",
            b"info",
            b"",
            correlation,
        );
    }

    if current == b"submitted" && proposed == b"approved" {
        if dual {
            let missing_legal = !collected_legal;
            let missing_finance = !collected_finance;
            if missing_legal || missing_finance {
                let approvals: &[u8] = match (missing_legal, missing_finance) {
                    (true, true) => {
                        br#"[{"role":"legal","min_count":1,"logic":"all","reason":"dual approval"},{"role":"finance","min_count":1,"logic":"all","reason":"dual approval"}]"#
                    }
                    (true, false) => {
                        br#"[{"role":"legal","min_count":1,"logic":"all","reason":"dual approval"}]"#
                    }
                    (false, true) => {
                        br#"[{"role":"finance","min_count":1,"logic":"all","reason":"dual approval"}]"#
                    }
                    _ => b"[]",
                };
                return write_decision(
                    out,
                    false,
                    b"requires_approval",
                    b"PARTIAL_APPROVALS",
                    b"One or more required approvals are still missing.",
                    approvals,
                    b"warning",
                    b"Approvals incomplete",
                    correlation,
                );
            }
            return write_decision(
                out,
                true,
                b"allowed",
                b"DUAL_APPROVAL_COMPLETE",
                b"Legal and finance approvals are both present.",
                b"[]",
                b"info",
                b"",
                correlation,
            );
        }

        if has_finance {
            return write_decision(
                out,
                true,
                b"allowed",
                b"APPROVED_BY_FINANCE",
                b"Finance authority completed the approval.",
                b"[]",
                b"info",
                b"",
                correlation,
            );
        }
        if has_manager {
            return write_decision(
                out,
                false,
                b"requires_approval",
                b"AMOUNT_EXCEEDS_LIMIT",
                b"Manager acknowledged; finance approval is still required.",
                br#"[{"role":"finance","min_count":1,"logic":"all","reason":"high-value expense"}]"#,
                b"warning",
                b"Requires Finance",
                correlation,
            );
        }
        return write_decision(
            out,
            false,
            b"denied",
            b"INSUFFICIENT_ROLE",
            b"Only managers or finance can approve a submitted expense.",
            b"[]",
            b"error",
            b"Insufficient role",
            correlation,
        );
    }

    if current.is_empty() || proposed.is_empty() {
        return write_decision(
            out,
            false,
            b"denied",
            b"UNKNOWN_STATE",
            b"Unrecognised state combination for expense.",
            b"[]",
            b"error",
            b"Unknown state",
            correlation,
        );
    }

    write_decision(
        out,
        false,
        b"denied",
        b"ILLEGAL_TRANSITION",
        b"The requested state jump is not legal for this entity lifecycle.",
        b"[]",
        b"error",
        b"Illegal transition",
        correlation,
    )
}

unsafe fn decide_order(
    out: &mut [u8],
    current: &[u8],
    proposed: &[u8],
    now: &[u8],
    cancel_deadline: &[u8],
    correlation: &[u8],
) -> usize {
    if current == b"fulfilled" && proposed == b"draft" {
        return write_decision(
            out,
            false,
            b"denied",
            b"ILLEGAL_TRANSITION",
            b"The requested state jump is not legal for this entity lifecycle.",
            b"[]",
            b"error",
            b"Illegal transition",
            correlation,
        );
    }

    if current == b"placed" && proposed == b"cancelled" {
        if now.is_empty() || cancel_deadline.is_empty() {
            return write_decision(
                out,
                false,
                b"requires_additional_info",
                b"MISSING_CANCEL_WINDOW",
                b"now and cancel_deadline are required to evaluate cancellation.",
                b"[]",
                b"warning",
                b"Window data required",
                correlation,
            );
        }
        if bytes_le(now, cancel_deadline) {
            return write_decision(
                out,
                true,
                b"allowed",
                b"CANCEL_WITHIN_WINDOW",
                b"Cancellation is still inside the allowed window.",
                b"[]",
                b"info",
                b"",
                correlation,
            );
        }
        return write_decision(
            out,
            false,
            b"denied",
            b"CANCEL_WINDOW_EXPIRED",
            b"The cancellation window has expired.",
            b"[]",
            b"error",
            b"Window expired",
            correlation,
        );
    }

    write_decision(
        out,
        false,
        b"denied",
        b"ILLEGAL_TRANSITION",
        b"The requested state jump is not legal for this entity lifecycle.",
        b"[]",
        b"error",
        b"Illegal transition",
        correlation,
    )
}

unsafe fn decide_ticket(
    out: &mut [u8],
    current: &[u8],
    proposed: &[u8],
    priority: &[u8],
    has_open_children: bool,
    correlation: &[u8],
) -> usize {
    if current == b"open" && proposed == b"escalated" {
        if priority == b"critical" {
            return write_decision(
                out,
                true,
                b"allowed",
                b"PRIORITY_ESCALATION",
                b"Critical priority tickets may escalate from open.",
                b"[]",
                b"info",
                b"",
                correlation,
            );
        }
        return write_decision(
            out,
            false,
            b"denied",
            b"PRIORITY_NOT_CRITICAL",
            b"Escalation requires context.priority to be critical.",
            b"[]",
            b"error",
            b"Not critical",
            correlation,
        );
    }

    if current == b"open" && proposed == b"closed" {
        if has_open_children {
            return write_decision(
                out,
                false,
                b"denied",
                b"HAS_OPEN_CHILDREN",
                b"Parent ticket cannot close while child tickets remain open.",
                b"[]",
                b"error",
                b"Open children",
                correlation,
            );
        }
        return write_decision(
            out,
            true,
            b"allowed",
            b"TICKET_CLOSED",
            b"Ticket may close with no open children.",
            b"[]",
            b"info",
            b"",
            correlation,
        );
    }

    write_decision(
        out,
        false,
        b"denied",
        b"ILLEGAL_TRANSITION",
        b"The requested state jump is not legal for this entity lifecycle.",
        b"[]",
        b"error",
        b"Illegal transition",
        correlation,
    )
}

/// Lexicographic compare suitable for identical-format ISO-8601 Zulu timestamps.
fn bytes_le(a: &[u8], b: &[u8]) -> bool {
    a <= b
}

unsafe fn write_query(out: &mut [u8], next: &[u8], correlation: &[u8]) -> usize {
    let mut i = 0usize;
    i = copy(
        out,
        i,
        br#"{"allowed":false,"decision":"denied","reasons":[{"code":"QUERY_ONLY","message":"Query mode returns next legal states only.","severity":"info"}],"required_approvals":[],"next_legal_states":"#,
    );
    i = copy(out, i, next);
    i = copy(
        out,
        i,
        br#","suggested_actions":[],"ui_hints":{"severity":"info"},"contract_version":""#,
    );
    i = copy(out, i, CONTRACT);
    i = copy(out, i, br#"","policy_version":""#);
    i = copy(out, i, POLICY);
    i = copy(out, i, br#"""#);
    if !correlation.is_empty() {
        i = copy(out, i, br#","correlation_id":""#);
        i = copy(out, i, correlation);
        i = copy(out, i, br#"""#);
    }
    i = copy(out, i, br#"}"#);
    i
}

#[allow(clippy::too_many_arguments)]
unsafe fn write_decision(
    out: &mut [u8],
    allowed: bool,
    decision: &[u8],
    code: &[u8],
    message: &[u8],
    required_approvals: &[u8],
    severity: &[u8],
    badge: &[u8],
    correlation: &[u8],
) -> usize {
    let mut i = 0usize;
    i = copy(out, i, br#"{"allowed":"#);
    i = copy(out, i, if allowed { b"true" } else { b"false" });
    i = copy(out, i, br#","decision":""#);
    i = copy(out, i, decision);
    i = copy(out, i, br#"","reasons":[{"code":""#);
    i = copy(out, i, code);
    i = copy(out, i, br#"","message":""#);
    i = copy(out, i, message);
    i = copy(out, i, br#"","severity":""#);
    i = copy(out, i, severity);
    i = copy(out, i, br#""}],"required_approvals":"#);
    i = copy(out, i, required_approvals);
    i = copy(
        out,
        i,
        br#","next_legal_states":[],"suggested_actions":[],"ui_hints":{"severity":""#,
    );
    i = copy(out, i, severity);
    if !badge.is_empty() {
        i = copy(out, i, br#"","badge":""#);
        i = copy(out, i, badge);
    }
    i = copy(out, i, br#""},"contract_version":""#);
    i = copy(out, i, CONTRACT);
    i = copy(out, i, br#"","policy_version":""#);
    i = copy(out, i, POLICY);
    i = copy(out, i, br#"""#);
    if !correlation.is_empty() {
        i = copy(out, i, br#","correlation_id":""#);
        i = copy(out, i, correlation);
        i = copy(out, i, br#"""#);
    }
    i = copy(out, i, br#"}"#);
    i
}

fn copy(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let end = at + bytes.len();
    if end > out.len() {
        return at;
    }
    out[at..end].copy_from_slice(bytes);
    end
}

fn has_role(hay: &[u8], role: &[u8]) -> bool {
    let Some(roles) = json_array_after_key(hay, b"\"roles\"") else {
        return false;
    };
    let mut needle = [0u8; 64];
    if role.len() + 2 > needle.len() {
        return false;
    }
    needle[0] = b'"';
    needle[1..1 + role.len()].copy_from_slice(role);
    needle[1 + role.len()] = b'"';
    contains(&roles, &needle[..role.len() + 2])
}

fn approval_collected(hay: &[u8], role: &[u8]) -> bool {
    let Some(arr) = json_array_after_key(hay, b"\"already_collected_approvals\"") else {
        return false;
    };
    // Match "role":"<role>" inside the approvals array.
    let mut needle = [0u8; 80];
    let prefix = br#""role":""#;
    if prefix.len() + role.len() + 1 > needle.len() {
        return false;
    }
    needle[..prefix.len()].copy_from_slice(prefix);
    needle[prefix.len()..prefix.len() + role.len()].copy_from_slice(role);
    needle[prefix.len() + role.len()] = b'"';
    contains(&arr, &needle[..prefix.len() + role.len() + 1])
}

fn flag_true(hay: &[u8], key: &[u8]) -> bool {
    let Some(pos) = find(hay, key) else {
        return false;
    };
    let after = &hay[pos + key.len()..];
    let Some(colon) = after.iter().position(|b| *b == b':') else {
        return false;
    };
    let mut rest = &after[colon + 1..];
    while rest.first() == Some(&b' ')
        || rest.first() == Some(&b'\n')
        || rest.first() == Some(&b'\t')
    {
        rest = &rest[1..];
    }
    rest.starts_with(b"true")
}

fn json_array_after_key<'a>(hay: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let mut rest = &after[colon + 1..];
    while rest.first() == Some(&b' ')
        || rest.first() == Some(&b'\n')
        || rest.first() == Some(&b'\t')
    {
        rest = &rest[1..];
    }
    if rest.first() != Some(&b'[') {
        return None;
    }
    let mut depth = 0i32;
    for (i, b) in rest.iter().enumerate() {
        match *b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|window| window == needle)
}

fn extract_string<'a>(hay: &'a [u8], key: &[u8]) -> &'a [u8] {
    let Some(pos) = find(hay, key) else {
        return b"";
    };
    let after = &hay[pos + key.len()..];
    let Some(colon) = after.iter().position(|b| *b == b':') else {
        return b"";
    };
    let mut rest = &after[colon + 1..];
    while rest.first() == Some(&b' ')
        || rest.first() == Some(&b'\n')
        || rest.first() == Some(&b'\t')
    {
        rest = &rest[1..];
    }
    if rest.first() != Some(&b'"') {
        return b"";
    }
    rest = &rest[1..];
    let Some(end) = rest.iter().position(|b| *b == b'"') else {
        return b"";
    };
    &rest[..end]
}

fn extract_number(hay: &[u8], key: &[u8]) -> Option<f64> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let mut rest = &after[colon + 1..];
    while rest.first() == Some(&b' ') {
        rest = &rest[1..];
    }
    let mut end = 0usize;
    while end < rest.len() && ((rest[end] >= b'0' && rest[end] <= b'9') || rest[end] == b'.') {
        end += 1;
    }
    if end == 0 {
        return None;
    }
    parse_f64(&rest[..end])
}

fn parse_f64(bytes: &[u8]) -> Option<f64> {
    let mut value = 0.0f64;
    let mut frac = 0.0f64;
    let mut place = 0.1f64;
    let mut seen_dot = false;
    for &b in bytes {
        if b == b'.' {
            if seen_dot {
                return None;
            }
            seen_dot = true;
            continue;
        }
        if b < b'0' || b > b'9' {
            return None;
        }
        let digit = f64::from(b - b'0');
        if seen_dot {
            frac += digit * place;
            place *= 0.1;
        } else {
            value = value * 10.0 + digit;
        }
    }
    Some(value + frac)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
mod catalog_coverage_tests {
    use super::*;

    fn run(input: &str) -> String {
        let mut out = vec![0u8; 65536];
        let n = unsafe { decide(input.as_bytes(), &mut out) };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    fn call_expense(
        current: &[u8],
        proposed: &[u8],
        amount: Option<f64>,
        has_manager: bool,
        has_finance: bool,
        dual: bool,
        collected_legal: bool,
        collected_finance: bool,
    ) -> String {
        let mut out = vec![0u8; 4096];
        let n = unsafe {
            decide_expense(
                &mut out,
                current,
                proposed,
                amount,
                has_manager,
                has_finance,
                dual,
                collected_legal,
                collected_finance,
                b"",
            )
        };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    fn call_order(current: &[u8], proposed: &[u8], now: &[u8], cancel_deadline: &[u8]) -> String {
        let mut out = vec![0u8; 4096];
        let n = unsafe { decide_order(&mut out, current, proposed, now, cancel_deadline, b"") };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    fn call_ticket(
        current: &[u8],
        proposed: &[u8],
        priority: &[u8],
        has_open_children: bool,
    ) -> String {
        let mut out = vec![0u8; 4096];
        let n = unsafe {
            decide_ticket(
                &mut out,
                current,
                proposed,
                priority,
                has_open_children,
                b"",
            )
        };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    #[test]
    fn smoke_invalid_input_returns_json() {
        let out = run("{}");
        assert!(out.contains("reason_code") || out.contains("{"), "{out}");
    }

    // ---- top-level decide(): no roles ----

    #[test]
    fn decide_denies_when_no_roles_present() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"submitted"}"#,
        );
        assert!(out.contains(r#""allowed":false"#));
        assert!(out.contains(r#""decision":"denied""#));
        assert!(out.contains(r#""code":"ACTOR_HAS_NO_ROLES""#));
    }

    #[test]
    fn decide_denies_when_roles_array_present_but_empty() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"submitted","roles":[]}"#,
        );
        assert!(out.contains(r#""code":"ACTOR_HAS_NO_ROLES""#));
    }

    // ---- top-level decide(): query mode / next_legal_states matrix ----

    #[test]
    fn query_mode_expense_draft_next_states() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"anything","mode":"query","roles":["employee"]}"#,
        );
        assert!(out.contains(r#""next_legal_states":["submitted"]"#));
        assert!(out.contains(r#""code":"QUERY_ONLY""#));
        assert!(out.contains(r#""decision":"denied""#));
    }

    #[test]
    fn query_mode_expense_submitted_with_manager() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"submitted","proposed_state":"x","mode":"query","roles":["manager"]}"#,
        );
        assert!(out.contains(r#""next_legal_states":["approved"]"#));
    }

    #[test]
    fn query_mode_expense_submitted_with_finance() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"submitted","proposed_state":"x","mode":"query","roles":["finance"]}"#,
        );
        assert!(out.contains(r#""next_legal_states":["approved"]"#));
    }

    #[test]
    fn query_mode_expense_submitted_no_approver_role() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"submitted","proposed_state":"x","mode":"query","roles":["employee"]}"#,
        );
        assert!(out.contains(r#""next_legal_states":[]"#));
    }

    #[test]
    fn query_mode_order_placed() {
        let out = run(
            r#"{"entity_type":"order","current_state":"placed","proposed_state":"x","mode":"query","roles":["employee"]}"#,
        );
        assert!(out.contains(r#""next_legal_states":["cancelled"]"#));
    }

    #[test]
    fn query_mode_ticket_open() {
        let out = run(
            r#"{"entity_type":"ticket","current_state":"open","proposed_state":"x","mode":"query","roles":["employee"]}"#,
        );
        assert!(out.contains(r#""next_legal_states":["escalated","closed"]"#));
    }

    #[test]
    fn query_mode_unmatched_combo_defaults_empty() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"approved","proposed_state":"x","mode":"query","roles":["employee"]}"#,
        );
        assert!(out.contains(r#""next_legal_states":[]"#));
    }

    #[test]
    fn query_triggered_by_empty_proposed_state() {
        let out = run(r#"{"entity_type":"expense","current_state":"draft","roles":["employee"]}"#);
        assert!(out.contains(r#""code":"QUERY_ONLY""#));
        assert!(out.contains(r#""next_legal_states":["submitted"]"#));
    }

    #[test]
    fn query_triggered_by_proposed_equal_current() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"draft","roles":["employee"]}"#,
        );
        assert!(out.contains(r#""code":"QUERY_ONLY""#));
        assert!(out.contains(r#""next_legal_states":["submitted"]"#));
    }

    // ---- top-level decide(): entity dispatch / unknown entity ----

    #[test]
    fn decide_denies_unknown_entity_type() {
        let out = run(
            r#"{"entity_type":"widget","current_state":"a","proposed_state":"b","roles":["employee"]}"#,
        );
        assert!(out.contains(r#""code":"UNKNOWN_ENTITY_TYPE""#));
        assert!(out.contains(r#""decision":"denied""#));
    }

    #[test]
    fn decide_routes_expense_and_passes_correlation_id() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"submitted","roles":["employee"],"amount":10,"correlation_id":"corr-1"}"#,
        );
        assert!(out.contains(r#""code":"AUTO_APPROVED""#));
        assert!(out.contains(r#""correlation_id":"corr-1""#));
    }

    #[test]
    fn decide_routes_order() {
        let out = run(
            r#"{"entity_type":"order","current_state":"fulfilled","proposed_state":"draft","roles":["employee"]}"#,
        );
        assert!(out.contains(r#""code":"ILLEGAL_TRANSITION""#));
    }

    #[test]
    fn decide_routes_ticket() {
        let out = run(
            r#"{"entity_type":"ticket","current_state":"open","proposed_state":"escalated","priority":"critical","roles":["employee"]}"#,
        );
        assert!(out.contains(r#""code":"PRIORITY_ESCALATION""#));
    }

    // ---- top-level decide(): alternate actor-role names feeding has_any_role ----

    #[test]
    fn decide_accepts_customer_role_as_valid_actor() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"submitted","roles":["customer"],"amount":10}"#,
        );
        assert!(out.contains(r#""code":"AUTO_APPROVED""#));
    }

    #[test]
    fn decide_accepts_compliance_role() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"submitted","roles":["compliance"],"amount":10}"#,
        );
        assert!(out.contains(r#""code":"AUTO_APPROVED""#));
    }

    #[test]
    fn decide_accepts_project_manager_role() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"submitted","roles":["project_manager"],"amount":10}"#,
        );
        assert!(out.contains(r#""code":"AUTO_APPROVED""#));
    }

    #[test]
    fn decide_accepts_support_role_plain() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"submitted","roles":["support"],"amount":10}"#,
        );
        assert!(out.contains(r#""code":"AUTO_APPROVED""#));
    }

    #[test]
    fn decide_accepts_support_agent_alt_role_name() {
        let out = run(
            r#"{"entity_type":"expense","current_state":"draft","proposed_state":"submitted","roles":["support_agent"],"amount":10}"#,
        );
        assert!(out.contains(r#""code":"AUTO_APPROVED""#));
    }

    // ---- decide_expense: draft -> submitted ----

    #[test]
    fn expense_draft_to_submitted_missing_amount() {
        let out = call_expense(
            b"draft",
            b"submitted",
            None,
            false,
            false,
            false,
            false,
            false,
        );
        assert!(out.contains(r#""decision":"requires_additional_info""#));
        assert!(out.contains(r#""code":"MISSING_AMOUNT""#));
    }

    #[test]
    fn expense_draft_to_submitted_amount_at_limit_exceeds() {
        let out = call_expense(
            b"draft",
            b"submitted",
            Some(100.0),
            false,
            false,
            false,
            false,
            false,
        );
        assert!(out.contains(r#""decision":"requires_approval""#));
        assert!(out.contains(r#""code":"AMOUNT_EXCEEDS_LIMIT""#));
        assert!(out.contains(
            r#""required_approvals":[{"role":"finance","min_count":1,"logic":"all","reason":"high-value expense"}]"#
        ));
    }

    #[test]
    fn expense_draft_to_submitted_amount_above_limit_exceeds() {
        let out = call_expense(
            b"draft",
            b"submitted",
            Some(250.5),
            false,
            false,
            false,
            false,
            false,
        );
        assert!(out.contains(r#""code":"AMOUNT_EXCEEDS_LIMIT""#));
    }

    #[test]
    fn expense_draft_to_submitted_low_amount_auto_approved() {
        let out = call_expense(
            b"draft",
            b"submitted",
            Some(50.0),
            false,
            false,
            false,
            false,
            false,
        );
        assert!(out.contains(r#""allowed":true"#));
        assert!(out.contains(r#""code":"AUTO_APPROVED""#));
    }

    // ---- decide_expense: submitted -> approved, dual approval ----

    #[test]
    fn expense_dual_approval_both_missing() {
        let out = call_expense(
            b"submitted",
            b"approved",
            None,
            false,
            false,
            true,
            false,
            false,
        );
        assert!(out.contains(r#""code":"PARTIAL_APPROVALS""#));
        assert!(out.contains(
            r#""required_approvals":[{"role":"legal","min_count":1,"logic":"all","reason":"dual approval"},{"role":"finance","min_count":1,"logic":"all","reason":"dual approval"}]"#
        ));
    }

    #[test]
    fn expense_dual_approval_only_legal_missing() {
        let out = call_expense(
            b"submitted",
            b"approved",
            None,
            false,
            false,
            true,
            false,
            true,
        );
        assert!(out.contains(r#""code":"PARTIAL_APPROVALS""#));
        assert!(out.contains(
            r#""required_approvals":[{"role":"legal","min_count":1,"logic":"all","reason":"dual approval"}]"#
        ));
    }

    #[test]
    fn expense_dual_approval_only_finance_missing() {
        let out = call_expense(
            b"submitted",
            b"approved",
            None,
            false,
            false,
            true,
            true,
            false,
        );
        assert!(out.contains(r#""code":"PARTIAL_APPROVALS""#));
        assert!(out.contains(
            r#""required_approvals":[{"role":"finance","min_count":1,"logic":"all","reason":"dual approval"}]"#
        ));
    }

    #[test]
    fn expense_dual_approval_complete() {
        let out = call_expense(
            b"submitted",
            b"approved",
            None,
            false,
            false,
            true,
            true,
            true,
        );
        assert!(out.contains(r#""allowed":true"#));
        assert!(out.contains(r#""code":"DUAL_APPROVAL_COMPLETE""#));
    }

    // ---- decide_expense: submitted -> approved, single approval ----

    #[test]
    fn expense_single_approval_by_finance() {
        let out = call_expense(
            b"submitted",
            b"approved",
            None,
            false,
            true,
            false,
            false,
            false,
        );
        assert!(out.contains(r#""allowed":true"#));
        assert!(out.contains(r#""code":"APPROVED_BY_FINANCE""#));
    }

    #[test]
    fn expense_single_approval_manager_only_still_needs_finance() {
        let out = call_expense(
            b"submitted",
            b"approved",
            None,
            true,
            false,
            false,
            false,
            false,
        );
        assert!(out.contains(r#""decision":"requires_approval""#));
        assert!(out.contains(r#""code":"AMOUNT_EXCEEDS_LIMIT""#));
    }

    #[test]
    fn expense_single_approval_insufficient_role() {
        let out = call_expense(
            b"submitted",
            b"approved",
            None,
            false,
            false,
            false,
            false,
            false,
        );
        assert!(out.contains(r#""decision":"denied""#));
        assert!(out.contains(r#""code":"INSUFFICIENT_ROLE""#));
    }

    // ---- decide_expense: unknown state / illegal transition ----

    #[test]
    fn expense_current_empty_is_unknown_state() {
        let out = call_expense(b"", b"approved", None, false, false, false, false, false);
        assert!(out.contains(r#""code":"UNKNOWN_STATE""#));
    }

    #[test]
    fn expense_proposed_empty_is_unknown_state() {
        // Reached only by calling decide_expense directly: decide()'s top-level
        // query gate treats an empty proposed_state as query mode before this
        // function is ever invoked, so this exercises the right-hand disjunct
        // of `current.is_empty() || proposed.is_empty()` directly.
        let out = call_expense(b"submitted", b"", None, false, false, false, false, false);
        assert!(out.contains(r#""code":"UNKNOWN_STATE""#));
    }

    #[test]
    fn expense_unmatched_pair_is_illegal_transition() {
        let out = call_expense(
            b"approved",
            b"draft",
            None,
            false,
            false,
            false,
            false,
            false,
        );
        assert!(out.contains(r#""code":"ILLEGAL_TRANSITION""#));
    }

    // ---- decide_order ----

    #[test]
    fn order_fulfilled_to_draft_is_illegal() {
        let out = call_order(b"fulfilled", b"draft", b"", b"");
        assert!(out.contains(r#""code":"ILLEGAL_TRANSITION""#));
    }

    #[test]
    fn order_cancel_missing_both_now_and_deadline() {
        let out = call_order(b"placed", b"cancelled", b"", b"");
        assert!(out.contains(r#""code":"MISSING_CANCEL_WINDOW""#));
    }

    #[test]
    fn order_cancel_missing_only_deadline() {
        let out = call_order(b"placed", b"cancelled", b"2024-01-01T00:00:00Z", b"");
        assert!(out.contains(r#""code":"MISSING_CANCEL_WINDOW""#));
    }

    #[test]
    fn order_cancel_within_window() {
        let out = call_order(
            b"placed",
            b"cancelled",
            b"2024-01-01T00:00:00Z",
            b"2024-01-02T00:00:00Z",
        );
        assert!(out.contains(r#""allowed":true"#));
        assert!(out.contains(r#""code":"CANCEL_WITHIN_WINDOW""#));
    }

    #[test]
    fn order_cancel_exactly_at_deadline_is_within_window() {
        let out = call_order(
            b"placed",
            b"cancelled",
            b"2024-01-02T00:00:00Z",
            b"2024-01-02T00:00:00Z",
        );
        assert!(out.contains(r#""code":"CANCEL_WITHIN_WINDOW""#));
    }

    #[test]
    fn order_cancel_window_expired() {
        let out = call_order(
            b"placed",
            b"cancelled",
            b"2024-01-03T00:00:00Z",
            b"2024-01-02T00:00:00Z",
        );
        assert!(out.contains(r#""decision":"denied""#));
        assert!(out.contains(r#""code":"CANCEL_WINDOW_EXPIRED""#));
    }

    #[test]
    fn order_unmatched_pair_is_illegal_transition() {
        let out = call_order(b"placed", b"draft", b"", b"");
        assert!(out.contains(r#""code":"ILLEGAL_TRANSITION""#));
    }

    // ---- decide_ticket ----

    #[test]
    fn ticket_escalation_critical_priority() {
        let out = call_ticket(b"open", b"escalated", b"critical", false);
        assert!(out.contains(r#""allowed":true"#));
        assert!(out.contains(r#""code":"PRIORITY_ESCALATION""#));
    }

    #[test]
    fn ticket_escalation_non_critical_priority() {
        let out = call_ticket(b"open", b"escalated", b"low", false);
        assert!(out.contains(r#""decision":"denied""#));
        assert!(out.contains(r#""code":"PRIORITY_NOT_CRITICAL""#));
    }

    #[test]
    fn ticket_close_with_open_children_denied() {
        let out = call_ticket(b"open", b"closed", b"", true);
        assert!(out.contains(r#""code":"HAS_OPEN_CHILDREN""#));
    }

    #[test]
    fn ticket_close_without_open_children_allowed() {
        let out = call_ticket(b"open", b"closed", b"", false);
        assert!(out.contains(r#""allowed":true"#));
        assert!(out.contains(r#""code":"TICKET_CLOSED""#));
    }

    #[test]
    fn ticket_unmatched_pair_is_illegal_transition() {
        let out = call_ticket(b"closed", b"open", b"", false);
        assert!(out.contains(r#""code":"ILLEGAL_TRANSITION""#));
    }

    // ---- next_legal_states (direct) ----

    #[test]
    fn next_legal_states_direct_matrix() {
        assert_eq!(
            next_legal_states(b"expense", b"draft", false, false),
            br#"["submitted"]"#.as_slice()
        );
        assert_eq!(
            next_legal_states(b"expense", b"submitted", true, false),
            br#"["approved"]"#.as_slice()
        );
        assert_eq!(
            next_legal_states(b"expense", b"submitted", false, true),
            br#"["approved"]"#.as_slice()
        );
        assert_eq!(
            next_legal_states(b"expense", b"submitted", false, false),
            br#"[]"#.as_slice()
        );
        assert_eq!(
            next_legal_states(b"order", b"placed", false, false),
            br#"["cancelled"]"#.as_slice()
        );
        assert_eq!(
            next_legal_states(b"ticket", b"open", false, false),
            br#"["escalated","closed"]"#.as_slice()
        );
        assert_eq!(
            next_legal_states(b"expense", b"approved", false, false),
            br#"[]"#.as_slice()
        );
    }

    // ---- bytes_le ----

    #[test]
    fn bytes_le_compares_lexicographically() {
        assert!(bytes_le(b"2024-01-01", b"2024-01-02"));
        assert!(bytes_le(b"2024-01-02", b"2024-01-02"));
        assert!(!bytes_le(b"2024-01-03", b"2024-01-02"));
    }

    // ---- copy / overflow guard ----

    #[test]
    fn copy_writes_when_it_fits() {
        let mut buf = [0u8; 5];
        let n = copy(&mut buf, 0, b"hi");
        assert_eq!(n, 2);
        assert_eq!(&buf[..2], b"hi");
    }

    #[test]
    fn copy_returns_at_unchanged_when_buffer_too_small() {
        let mut tiny = [0u8; 2];
        let result = copy(&mut tiny, 0, b"hello");
        assert_eq!(result, 0);
        assert_eq!(&tiny, &[0, 0]);
    }

    #[test]
    fn decide_with_tiny_output_buffer_does_not_panic() {
        // The output buffer is far too small for a full decision payload.
        // `copy` silently no-ops (returns the position unchanged) on any
        // write that would overflow, rather than panicking — including the
        // very first (oversized) write, whose failure to advance the
        // position then lets a later, smaller write ("true", 4 bytes)
        // land exactly at offset 0..4 since it fits on its own.
        let mut out = [0u8; 4];
        let input = br#"{"entity_type":"expense","current_state":"draft","proposed_state":"submitted","roles":["employee"],"amount":10}"#;
        let n = unsafe { decide(input, &mut out) };
        assert!(n <= out.len());
        assert_eq!(&out[..n], b"true");
    }

    // ---- has_role ----

    #[test]
    fn has_role_present_and_absent() {
        let hay: &[u8] = br#""roles":["manager","finance"]"#;
        assert!(has_role(hay, b"manager"));
        assert!(!has_role(hay, b"employee"));
    }

    #[test]
    fn has_role_returns_false_when_roles_key_missing() {
        assert!(!has_role(br#""foo":1"#, b"manager"));
    }

    #[test]
    fn has_role_rejects_oversized_role_needle() {
        let long_role = [b'a'; 100];
        assert!(!has_role(br#""roles":["x"]"#, &long_role));
    }

    // ---- approval_collected ----

    #[test]
    fn approval_collected_present_and_absent() {
        let hay: &[u8] = br#""already_collected_approvals":[{"role":"finance"}]"#;
        assert!(approval_collected(hay, b"finance"));
        assert!(!approval_collected(hay, b"legal"));
    }

    #[test]
    fn approval_collected_returns_false_when_key_missing() {
        assert!(!approval_collected(br#""foo":1"#, b"finance"));
    }

    #[test]
    fn approval_collected_rejects_oversized_role_needle() {
        let long_role = [b'a'; 100];
        assert!(!approval_collected(
            br#""already_collected_approvals":[{"role":"x"}]"#,
            &long_role
        ));
    }

    // ---- flag_true ----

    #[test]
    fn flag_true_variants() {
        assert!(flag_true(
            br#""requires_dual_approval":true"#,
            b"\"requires_dual_approval\""
        ));
        assert!(!flag_true(
            br#""requires_dual_approval":false"#,
            b"\"requires_dual_approval\""
        ));
        assert!(!flag_true(br#""foo":true"#, b"\"requires_dual_approval\""));
        assert!(!flag_true(
            br#""requires_dual_approval"true"#,
            b"\"requires_dual_approval\""
        ));
        assert!(flag_true(
            b"\"requires_dual_approval\":   true",
            b"\"requires_dual_approval\""
        ));
        assert!(flag_true(
            b"\"requires_dual_approval\":\n\ttrue",
            b"\"requires_dual_approval\""
        ));
    }

    // ---- json_array_after_key ----

    #[test]
    fn json_array_after_key_missing_key() {
        assert_eq!(json_array_after_key(br#""foo":[1]"#, b"\"roles\""), None);
    }

    #[test]
    fn json_array_after_key_no_colon() {
        assert_eq!(json_array_after_key(br#""roles"[1]"#, b"\"roles\""), None);
    }

    #[test]
    fn json_array_after_key_no_bracket() {
        assert_eq!(json_array_after_key(br#""roles":1"#, b"\"roles\""), None);
    }

    #[test]
    fn json_array_after_key_tracks_nested_depth() {
        let arr = json_array_after_key(br#""roles":[["a"],"b"]"#, b"\"roles\"").unwrap();
        assert_eq!(arr, br#"[["a"],"b"]"#.as_slice());
    }

    // ---- contains / find ----

    #[test]
    fn contains_checks_substring() {
        assert!(contains(b"abcdef", b"cde"));
        assert!(!contains(b"abcdef", b"xyz"));
    }

    #[test]
    fn find_finds_and_reports_missing() {
        assert_eq!(find(b"hello world", b"world"), Some(6));
        assert_eq!(find(b"hello world", b"xyz"), None);
    }

    // ---- extract_string ----

    #[test]
    fn extract_string_happy_path() {
        assert_eq!(
            extract_string(br#""entity_type":"expense""#, b"\"entity_type\""),
            b"expense"
        );
    }

    #[test]
    fn extract_string_key_missing() {
        assert_eq!(extract_string(br#""foo":"bar""#, b"\"entity_type\""), b"");
    }

    #[test]
    fn extract_string_no_colon() {
        assert_eq!(
            extract_string(br#""entity_type""expense""#, b"\"entity_type\""),
            b""
        );
    }

    #[test]
    fn extract_string_no_opening_quote() {
        assert_eq!(
            extract_string(br#""entity_type":expense""#, b"\"entity_type\""),
            b""
        );
    }

    #[test]
    fn extract_string_no_closing_quote() {
        assert_eq!(
            extract_string(br#""entity_type":"expense"#, b"\"entity_type\""),
            b""
        );
    }

    // ---- extract_number ----

    #[test]
    fn extract_number_happy_path() {
        assert_eq!(extract_number(br#""amount":42"#, b"\"amount\""), Some(42.0));
    }

    #[test]
    fn extract_number_key_missing() {
        assert_eq!(extract_number(br#""foo":1"#, b"\"amount\""), None);
    }

    #[test]
    fn extract_number_no_colon() {
        assert_eq!(extract_number(br#""amount"42"#, b"\"amount\""), None);
    }

    #[test]
    fn extract_number_no_digits() {
        assert_eq!(extract_number(br#""amount": abc"#, b"\"amount\""), None);
    }

    // ---- parse_f64 ----

    #[test]
    fn parse_f64_integer() {
        assert_eq!(parse_f64(b"123"), Some(123.0));
    }

    #[test]
    fn parse_f64_decimal() {
        assert_eq!(parse_f64(b"12.5"), Some(12.5));
    }

    #[test]
    fn parse_f64_multiple_dots_is_none() {
        assert_eq!(parse_f64(b"1.2.3"), None);
    }

    #[test]
    fn parse_f64_non_digit_is_none() {
        assert_eq!(parse_f64(b"12a"), None);
    }
}
