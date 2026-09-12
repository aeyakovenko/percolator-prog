//! INV-087/088: executable source contract for the wrapper's liveness read boundary.
//!
//! The field inventory owns existence/writers; the transition census owns engine calls.
//! This guard instead binds complete predicate bodies to policy/profile/Clock inputs and
//! inventories direct `last_good_oracle_slot`/`permissionless_resolve_stale_slots` accesses
//! and four maturity-predicate references (including arguments) in the production
//! translation unit, including code after the host-test module. In-memory
//! mutations exercise the guard, not a modified program or an engine proof.

use std::collections::BTreeMap;

// A lexical guard, not a Rust type/dataflow checker. Keep literals opaque and remove
// comments before examining tokens so documentation cannot satisfy an enforcement edge.
fn tokens(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else if bytes[i..].starts_with(b"//") {
            i += bytes[i..]
                .iter()
                .position(|b| *b == b'\n')
                .unwrap_or(bytes.len() - i);
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while depth != 0 {
                assert!(i < bytes.len(), "unterminated source comment");
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else {
            let mut quote = i + usize::from(bytes[i] == b'r');
            while bytes.get(quote) == Some(&b'#') {
                quote += 1;
            }
            if bytes[i] == b'r' && bytes.get(quote) == Some(&b'"') {
                let end = format!("\"{}", "#".repeat(quote - i - 1));
                i = quote + 1;
                i += source[i..].find(&end).expect("unterminated raw string") + end.len();
            } else if bytes[i] == b'"'
                || (bytes[i] == b'\''
                    && (bytes.get(i + 2) == Some(&b'\'') || bytes.get(i + 1) == Some(&b'\\')))
            {
                let delimiter = bytes[i];
                i += 1;
                loop {
                    assert!(i < bytes.len(), "unterminated literal");
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == delimiter {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            } else if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                assert!(
                    &source[start..i] != "r" || bytes.get(i) != Some(&b'#'),
                    "raw identifiers require liveness-guard review"
                );
            } else {
                i += 1;
            }
            result.push(&source[start..i]);
        }
    }
    result
}

fn closing_brace(code: &[&str], open: usize) -> usize {
    assert_eq!(code[open], "{");
    let mut depth = 0;
    for (index, token) in code.iter().enumerate().skip(open) {
        match *token {
            "{" => depth += 1,
            "}" => {
                depth -= 1;
                if depth == 0 {
                    return index;
                }
            }
            _ => {}
        }
    }
    panic!("unterminated source body")
}

fn production_tokens(source: &str) -> Vec<&str> {
    let mut code = tokens(source);
    let host_tests = tokens("#[cfg(test)] mod tests {");
    let start = code
        .windows(host_tests.len())
        .position(|part| part == host_tests)
        .expect("explicit host-test module boundary");
    let end = closing_brace(&code, start + host_tests.len() - 1);
    code.drain(start..=end);
    // This translation unit is currently self-contained. An external module/include
    // must extend the scan, not silently escape the read boundary.
    assert!(!code
        .windows(3)
        .any(|part| part[0] == "mod" && part[2] == ";"));
    assert!(!code.windows(2).any(|part| part == ["include", "!"]));
    code
}

struct Function<'a> {
    name: &'a str,
    start: usize,
    open: usize,
    end: usize,
}

fn functions<'a>(code: &[&'a str]) -> Vec<Function<'a>> {
    code.windows(3)
        .enumerate()
        .filter_map(|(start, part)| {
            if part[0] != "fn"
                || !part[1].starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            {
                return None;
            }
            let open = start + code[start..].iter().position(|token| *token == "{")?;
            Some(Function {
                name: part[1],
                start,
                open,
                end: closing_brace(code, open),
            })
        })
        .collect()
}

const PREDICATES: &[(&str, &str)] = &[
    (
        "authenticated_slot_or_fallback",
        "{ Clock::get().map(|c| c.slot).unwrap_or(fallback_slot) }",
    ),
    (
        "permissionless_stale_matured",
        r#"{
        config.permissionless_resolve_stale_slots != 0
            && now_slot.saturating_sub(config.last_good_oracle_slot)
                >= config.permissionless_resolve_stale_slots
    }"#,
    ),
    (
        "permissionless_resolve_matured_for_profile_at_slot",
        r#"{
        cfg.permissionless_resolve_stale_slots != 0
            && now_slot.saturating_sub(profile.last_good_oracle_slot)
                >= cfg.permissionless_resolve_stale_slots
    }"#,
    ),
    (
        "global_or_profile_resolve_matured_at_slot",
        r#"{
        oracle_v16::permissionless_stale_matured(cfg, now_slot)
            || (oracle_v16::profile_is_price_managed(profile)
                && permissionless_resolve_matured_for_profile_at_slot(cfg, profile, now_slot))
    }"#,
    ),
    (
        "authenticated_market_slot_or_fallback_view",
        r#"{
        core::cmp::max(
            Clock::get().map(|c| c.slot).unwrap_or(group.header.current_slot.get()),
            group.header.current_slot.get(),
        )
    }"#,
    ),
    (
        "permissionless_resolve_matured_now_view",
        r#"{
        oracle_v16::permissionless_stale_matured(
            cfg, authenticated_market_slot_or_fallback_view(group),
        )
    }"#,
    ),
    (
        "reject_permissionless_resolve_matured_live_view",
        r#"{
        if group.header.mode == 0 && permissionless_resolve_matured_now_view(cfg, group) {
            return Err(PercolatorError::OracleStale.into());
        }
        Ok(())
    }"#,
    ),
    (
        "reject_permissionless_resolve_matured_live_for_profile_view",
        r#"{
        if group.header.mode == 0
            && global_or_profile_resolve_matured_at_slot(
                cfg, profile, authenticated_market_slot_or_fallback_view(group),
            )
        {
            return Err(PercolatorError::OracleStale.into());
        }
        Ok(())
    }"#,
    ),
];

// Receiver-sensitive access census, not a persisted-field or engine-transition roster.
// Counts include assignments and mirrors: adding a new read even to an existing writer
// cannot inherit that writer's exemption. The predicate bodies above own enforcement.
const FIELD_EDGES: &[(&str, &str, usize)] = &[
    (
        "asset_oracle_profile_from_config",
        "config.last_good_oracle_slot",
        1,
    ),
    (
        "hybrid_soft_stale_matured",
        "config.last_good_oracle_slot",
        1,
    ),
    (
        "profile_hybrid_soft_stale_matured",
        "profile.last_good_oracle_slot",
        1,
    ),
    (
        "permissionless_stale_matured",
        "config.last_good_oracle_slot",
        1,
    ),
    (
        "permissionless_stale_matured",
        "config.permissionless_resolve_stale_slots",
        2,
    ),
    (
        "permissionless_resolve_matured_for_profile_at_slot",
        "profile.last_good_oracle_slot",
        1,
    ),
    (
        "permissionless_resolve_matured_for_profile_at_slot",
        "cfg.permissionless_resolve_stale_slots",
        2,
    ),
    (
        "shutdown_asset_matured_at_slot_view",
        "profile.last_good_oracle_slot",
        1,
    ),
    (
        "mirror_manual_profile_to_base_config",
        "cfg.last_good_oracle_slot",
        1,
    ),
    (
        "mirror_manual_profile_to_base_config",
        "profile.last_good_oracle_slot",
        1,
    ),
    (
        "mirror_oracle_profile_to_base_config",
        "cfg.last_good_oracle_slot",
        2,
    ),
    (
        "mirror_oracle_profile_to_base_config",
        "profile.last_good_oracle_slot",
        1,
    ),
    (
        "handle_force_close_abandoned_asset",
        "profile.last_good_oracle_slot",
        1,
    ),
    (
        "handle_update_asset_lifecycle",
        "profile.last_good_oracle_slot",
        1,
    ),
    (
        "handle_configure_permissionless_resolve",
        "cfg.permissionless_resolve_stale_slots",
        1,
    ),
    (
        "handle_configure_hybrid_oracle",
        "profile.last_good_oracle_slot",
        1,
    ),
    (
        "handle_push_managed_mark",
        "profile.last_good_oracle_slot",
        1,
    ),
    (
        "handle_permissionless_crank_zero_copy",
        "oracle_profile.last_good_oracle_slot",
        2,
    ),
    (
        "handle_permissionless_crank_zero_copy",
        "cfg.last_good_oracle_slot",
        2,
    ),
    (
        "hybrid_target_for_crank_view",
        "cfg.permissionless_resolve_stale_slots",
        2,
    ),
    (
        "hybrid_target_for_crank_view",
        "profile.last_good_oracle_slot",
        2,
    ),
    (
        "canonical_accrual_path_for_target_view",
        "simulated_profile.last_good_oracle_slot",
        1,
    ),
];

const CALL_EDGES: &[(&str, &str, usize)] = &[
    ("hard_stale_matured", "permissionless_stale_matured(config, now_slot)", 1),
    (
        "permissionless_resolve_matured_now_view",
        "permissionless_stale_matured(cfg, authenticated_market_slot_or_fallback_view(group),)",
        1,
    ),
    (
        "global_or_profile_resolve_matured_at_slot",
        "permissionless_stale_matured(cfg, now_slot)",
        1,
    ),
    (
        "global_or_profile_resolve_matured_at_slot",
        "permissionless_resolve_matured_for_profile_at_slot(cfg, profile, now_slot)",
        1,
    ),
    (
        "reject_permissionless_resolve_matured_live_view",
        "permissionless_resolve_matured_now_view(cfg, group)",
        1,
    ),
    (
        "reject_permissionless_resolve_matured_live_for_profile_view",
        "global_or_profile_resolve_matured_at_slot(cfg, profile, authenticated_market_slot_or_fallback_view(group),)",
        1,
    ),
    (
        "reject_non_base_oracle_update_after_global_resolve_matured",
        "permissionless_stale_matured(cfg, authenticated_slot)",
        1,
    ),
    (
        "handle_trade_cpi",
        "global_or_profile_resolve_matured_at_slot(&cfg_pre, &oracle_profile_pre, authenticated_slot_or_fallback(current_slot_pre),)",
        1,
    ),
    (
        "handle_batch_trade_cpi",
        "global_or_profile_resolve_matured_at_slot(&cfg_pre, &oracle_profile_pre, authenticated_slot,)",
        1,
    ),
    (
        "handle_convert_released_pnl",
        "permissionless_resolve_matured_now_view(cfg, group)",
        1,
    ),
    (
        "handle_forfeit_recovery_leg",
        "permissionless_resolve_matured_now_view(cfg, group)",
        1,
    ),
    (
        "handle_rebalance_reduce",
        "permissionless_resolve_matured_now_view(cfg, group)",
        1,
    ),
    (
        "handle_restart_asset_oracle",
        "permissionless_stale_matured(&cfg, authenticated_slot)",
        1,
    ),
    (
        "handle_update_asset_lifecycle",
        "permissionless_stale_matured(&cfg_pre, authenticated_slot)",
        3,
    ),
    (
        "handle_configure_permissionless_resolve",
        "permissionless_stale_matured(&cfg, authenticated_slot_or_fallback(0))",
        1,
    ),
    (
        "handle_resolve_stale_permissionless",
        "permissionless_stale_matured(&cfg, authenticated_slot)",
        1,
    ),
    (
        "handle_permissionless_crank_zero_copy",
        "permissionless_resolve_matured_now_view(&cfg, &group)",
        1,
    ),
    (
        "handle_permissionless_crank_zero_copy",
        "global_or_profile_resolve_matured_at_slot(&cfg, &oracle_profile, authenticated_now_slot,)",
        1,
    ),
];

fn expected_edges(rows: &[(&str, &str, usize)]) -> BTreeMap<(String, String), usize> {
    let mut expected = BTreeMap::new();
    for &(owner, edge, count) in rows {
        assert!(count > 0);
        assert!(expected
            .insert((owner.into(), tokens(edge).join(" ")), count)
            .is_none());
    }
    expected
}

fn check_contract(source: &str) -> Result<(), String> {
    let code = production_tokens(source);
    let functions = functions(&code);
    for &(name, expected) in PREDICATES {
        let definitions: Vec<_> = functions.iter().filter(|f| f.name == name).collect();
        if definitions.len() != 1
            || code[definitions[0].open..=definitions[0].end] != tokens(expected)
        {
            return Err(format!("enforcement body drift: {name}"));
        }
    }
    let mut fields = BTreeMap::new();
    let mut calls = BTreeMap::new();
    for (index, token) in code.iter().enumerate() {
        let owner = functions
            .iter()
            .rev()
            .find(|f| f.open < index && index < f.end)
            .map_or("<module>", |f| f.name);
        if matches!(
            *token,
            "last_good_oracle_slot" | "permissionless_resolve_stale_slots"
        ) && index >= 2
            && code[index - 1] == "."
        {
            *fields
                .entry((owner.to_owned(), code[index - 2..=index].join(" ")))
                .or_insert(0) += 1;
        }
        if CALL_EDGES
            .iter()
            .any(|(_, call, _)| call.split_once('(').unwrap().0 == *token)
            && !functions.iter().any(|f| f.start + 1 == index)
        {
            // Count references as well as calls so taking a function pointer cannot
            // hide a new path from this boundary.
            let mut end = index;
            if code.get(index + 1) == Some(&"(") {
                let mut depth = 0;
                for (offset, token) in code.iter().enumerate().skip(index + 1) {
                    match *token {
                        "(" => depth += 1,
                        ")" => {
                            depth -= 1;
                            if depth == 0 {
                                end = offset;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                assert!(end > index, "unterminated maturity call");
            }
            *calls
                .entry((owner.to_owned(), code[index..=end].join(" ")))
                .or_insert(0) += 1;
        }
    }
    if fields != expected_edges(FIELD_EDGES) {
        return Err(format!("liveness field edge drift: {fields:?}"));
    }
    if calls != expected_edges(CALL_EDGES) {
        return Err(format!("maturity consumer edge drift: {calls:?}"));
    }
    Ok(())
}

fn rewrite_body(source: &str, name: &str, change: impl FnOnce(&str) -> String) -> String {
    let code = tokens(source);
    let function = functions(&code)
        .into_iter()
        .find(|f| f.name == name)
        .unwrap();
    let start = code[function.open].as_ptr() as usize - source.as_ptr() as usize;
    let end = code[function.end].as_ptr() as usize - source.as_ptr() as usize + 1;
    format!(
        "{}{}{}",
        &source[..start],
        change(&source[start..end]),
        &source[end..]
    )
}

#[test]
fn v16_program_liveness_read_contract_rejects_dead_policy_and_global_scope_substitution() {
    let source = include_str!("../../../src/v16_program.rs");
    check_contract(source).expect("shipping wrapper liveness read contract");

    let local_name = "permissionless_resolve_matured_for_profile_at_slot";
    let local = PREDICATES
        .iter()
        .find(|(name, _)| *name == local_name)
        .unwrap()
        .1;
    // Replace only the selected production body in memory, retaining every writer,
    // field declaration, named witness and engine callsite in the original source.
    let mutations = [
        ("dead policy", "{ false }".to_owned()),
        (
            "hardcoded policy",
            local.replace("cfg.permissionless_resolve_stale_slots", "5"),
        ),
        (
            "base summary as local evidence",
            local.replace("profile.last_good_oracle_slot", "cfg.last_good_oracle_slot"),
        ),
        (
            "dead branch with original reads",
            format!("{{ if false {local} else {{ false }} }}"),
        ),
        ("comment decoy", format!("{{ /* {local} */ false }}")),
        (
            "literal decoy",
            format!("{{ let _ = r###\"{local}\"###; false }}"),
        ),
    ];
    for (label, body) in &mutations {
        let mutant = rewrite_body(source, local_name, |_| body.clone());
        assert_eq!(
            check_contract(&mutant),
            Err(format!("enforcement body drift: {local_name}")),
            "{label}"
        );
    }
    let stale_clock = rewrite_body(source, "authenticated_market_slot_or_fallback_view", |_| {
        "{ group.header.current_slot.get() }".into()
    });
    assert_eq!(
        check_contract(&stale_clock),
        Err("enforcement body drift: authenticated_market_slot_or_fallback_view".into())
    );

    let stale_argument = rewrite_body(source, "handle_trade_cpi", |body| {
        assert_eq!(
            body.matches("authenticated_slot_or_fallback(current_slot_pre)")
                .count(),
            1
        );
        body.replace(
            "authenticated_slot_or_fallback(current_slot_pre)",
            "current_slot_pre",
        )
    });
    assert!(check_contract(&stale_argument)
        .unwrap_err()
        .starts_with("maturity consumer edge drift:"));
    let extra_read = rewrite_body(source, "mirror_manual_profile_to_base_config", |body| {
        body.replacen('{', "{ let _ = cfg.last_good_oracle_slot;", 1)
    });
    assert!(check_contract(&extra_read)
        .unwrap_err()
        .starts_with("liveness field edge drift:"));

    for (suffix, error) in [
        ("fn new_wrapper_path(cfg: &state::WrapperConfigV16) -> u64 { cfg.last_good_oracle_slot }", "liveness field edge drift:"),
        ("fn new_wrapper_path(cfg: &state::WrapperConfigV16) { let _ = oracle_v16::permissionless_stale_matured(cfg, 0); }", "maturity consumer edge drift:"),
    ] {
        assert!(check_contract(&format!("{source}\n{suffix}")).unwrap_err().starts_with(error));
    }
    // Whitespace and both comment/literal forms are non-enforcement positive controls.
    let decorated = source.replace(
        "cfg.permissionless_resolve_stale_slots",
        "cfg /* outer /* nested */ comment */ . permissionless_resolve_stale_slots",
    );
    check_contract(&format!("{decorated}\n// cfg.last_good_oracle_slot\nconst DECOY: &str = r###\"cfg.last_good_oracle_slot {{ }}\"###;"))
        .expect("comments and literals must not create or satisfy edges");
    println!("INV-087/088 liveness read contract: 8 exact predicates, 22 field-edge classes/29 accesses, 18 consumer classes/20 argument-bound calls; 11/11 mutations rejected, lexical positive control passed");
}
