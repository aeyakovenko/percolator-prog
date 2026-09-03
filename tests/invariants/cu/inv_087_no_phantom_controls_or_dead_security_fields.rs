//! INV-087 - No phantom controls or dead security fields.
//!
//! Normative obligation: every persisted security control needs a writer, an
//! enforcement read, and an executable witness that changing it affects the
//! intended route. Default-only or dead controls must not be mistaken for active
//! protection.
//!
//! Evidence in this file (I/C): public LiteSVM tests cover two high-impact
//! controls with writer/read/enforcement behavior: permissionless resolve timing
//! policy and asset activation cooldown. Four unwritable insurance-withdraw pseudo-controls remain
//! zero-validated reserved wire space; the fifth former reserve is now the bounded terminal-scan
//! cursor, and the maximum-shape CloseSlab witness proves it changes only as real scan progress.
//! A public top-up/withdrawal composition proves the remaining reserve is not hidden accounting
//! state. The static rosters below also inventory every
//! field in all six wrapper-owned persisted structs and require category-specific
//! writer/read/validation edges plus exactly one named executable mutation witness for every
//! non-padding field. Engine-owned fields remain the engine proof boundary.

use super::*;

fn wrapper_config_fields_from_source(source: &str) -> Vec<&str> {
    let start = source
        .find("pub struct WrapperConfigV16 {")
        .expect("WrapperConfigV16 definition must stay present");
    let rest = &source[start..];
    let end = rest
        .find("pub struct AssetOracleProfileV16 {")
        .expect("AssetOracleProfileV16 must follow WrapperConfigV16");
    let body = &rest[..end];
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let field = trimmed.strip_prefix("pub ")?;
            let (name, _) = field.split_once(':')?;
            Some(name.trim())
        })
        .collect()
}

fn persisted_struct_fields_from_source<'a>(source: &'a str, name: &str) -> Vec<&'a str> {
    let marker = format!("pub struct {name} {{");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("{name} definition must stay present"));
    source[start + marker.len()..]
        .lines()
        .take_while(|line| line.trim() != "}")
        .filter_map(|line| {
            let field = line.trim().strip_prefix("pub ")?;
            let (name, _) = field.split_once(':')?;
            Some(name.trim())
        })
        .collect()
}

fn assert_exact_persisted_fields(source: &str, name: &str, expected: &[&str]) {
    let mut parsed = persisted_struct_fields_from_source(source, name);
    let mut expected = expected.to_vec();
    parsed.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        parsed, expected,
        "{name} persisted-field inventory changed without an INV-087 classification",
    );
}

fn assert_source_edges(source: &str, class: &str, edges: &[&str]) {
    for edge in edges {
        assert!(
            source.contains(edge),
            "{class} missing writer/read/validation edge: {edge}",
        );
    }
}

fn assert_named_witness(source: &str, label: &str, witness: &str) {
    assert!(
        source.contains(&format!("fn {witness}")),
        "{label} missing executable public mutation witness {witness}",
    );
}

#[test]
fn v16_program_wrapper_config_static_inventory_covers_every_persisted_field() {
    let source = include_str!("../../../src/v16_program.rs");
    let tests = include_str!("inv_087_no_phantom_controls_or_dead_security_fields.rs");
    let inventory: &[(&str, &str, &[&str])] = &[
        (
            "marketauth",
            "security-control",
            &[
                "cfg.marketauth = new_pubkey;",
                "expect_live_authority(&cfg.marketauth",
            ],
        ),
        (
            "collateral_mint",
            "security-control",
            &[
                "cfg.collateral_mint = primary_mint;",
                "primary_collateral_mint(&cfg",
            ],
        ),
        (
            "secondary_collateral_mint",
            "security-control",
            &[
                "cfg.secondary_collateral_mint = secondary_mint;",
                "secondary_collateral_mint(&cfg",
            ],
        ),
        (
            "maintenance_fee_per_slot",
            "security-control",
            &[
                "maintenance_fee_per_slot > percolator::MAX_PROTOCOL_FEE_ABS",
                "cfg_pre.maintenance_fee_per_slot",
            ],
        ),
        (
            "permissionless_market_init_fee",
            "security-control",
            &[
                "cfg.permissionless_market_init_fee = value",
                "cfg_pre.permissionless_market_init_fee",
            ],
        ),
        (
            "trade_fee_base_bps",
            "security-control",
            &[
                "cfg.trade_fee_base_bps = trade_fee_base_bps;",
                "cfg_pre.trade_fee_base_bps",
            ],
        ),
        (
            "permissionless_resolve_stale_slots",
            "security-control",
            &[
                "cfg.permissionless_resolve_stale_slots = stale_slots;",
                "permissionless_stale_matured(&cfg",
            ],
        ),
        (
            "force_close_delay_slots",
            "security-control",
            &[
                "cfg.force_close_delay_slots = force_close_delay_slots;",
                "cfg.force_close_delay_slots",
            ],
        ),
        (
            "last_good_oracle_slot",
            "oracle-liveness-mirror",
            &[
                "cfg.last_good_oracle_slot",
                "config.last_good_oracle_slot",
                "profile.last_good_oracle_slot",
            ],
        ),
        (
            "terminal_slab_scan_progress",
            "liveness-progress",
            &[
                "terminal_slab_scan_progress: 0",
                "encode_terminal_slab_scan_progress(",
                "cfg.terminal_slab_scan_progress = 0",
            ],
        ),
        (
            "_reserved_insurance_withdraw_max_bps",
            "layout-reserved",
            &[
                "_reserved_insurance_withdraw_max_bps: 0",
                "config._reserved_insurance_withdraw_max_bps != 0",
            ],
        ),
        (
            "liquidation_cranker_fee_share_bps",
            "security-control",
            &[
                "cfg.liquidation_cranker_fee_share_bps = value;",
                "cfg.liquidation_cranker_fee_share_bps",
            ],
        ),
        (
            "maintenance_cranker_fee_share_bps",
            "security-control",
            &[
                "cfg.maintenance_cranker_fee_share_bps = value;",
                "cfg_pre.maintenance_cranker_fee_share_bps",
            ],
        ),
        (
            "backing_trade_fee_bps_long",
            "security-control-and-oracle-profile-mirror",
            &[
                "cfg.backing_trade_fee_bps_long = fee_bps;",
                "config.backing_trade_fee_bps_long",
            ],
        ),
        (
            "unit_scale",
            "oracle-profile-mirror",
            &["config.unit_scale", "cfg.unit_scale = profile.unit_scale;"],
        ),
        (
            "conf_filter_bps",
            "oracle-profile-mirror",
            &[
                "config.conf_filter_bps",
                "cfg.conf_filter_bps = profile.conf_filter_bps;",
            ],
        ),
        (
            "backing_trade_fee_bps_short",
            "security-control-and-oracle-profile-mirror",
            &[
                "cfg.backing_trade_fee_bps_short = fee_bps;",
                "config.backing_trade_fee_bps_short",
            ],
        ),
        (
            "_reserved_insurance_withdraw_deposits_only",
            "layout-reserved",
            &[
                "_reserved_insurance_withdraw_deposits_only: 0",
                "config._reserved_insurance_withdraw_deposits_only != 0",
            ],
        ),
        (
            "oracle_mode",
            "oracle-profile-mirror",
            &[
                "config.oracle_mode",
                "cfg.oracle_mode = profile.oracle_mode;",
            ],
        ),
        (
            "oracle_leg_count",
            "oracle-profile-mirror",
            &[
                "config.oracle_leg_count",
                "cfg.oracle_leg_count = profile.oracle_leg_count;",
            ],
        ),
        (
            "oracle_leg_flags",
            "oracle-profile-mirror",
            &[
                "config.oracle_leg_flags",
                "cfg.oracle_leg_flags = profile.oracle_leg_flags;",
            ],
        ),
        (
            "invert",
            "oracle-profile-mirror",
            &["config.invert", "cfg.invert = profile.invert;"],
        ),
        ("_padding0", "layout-padding", &["_padding0: u8"]),
        (
            "free_market_slot_count",
            "derived-counter",
            &["cfg.free_market_slot_count", "free_market_slot_count: 0"],
        ),
        (
            "_reserved_insurance_withdraw_cooldown_slots",
            "layout-reserved",
            &[
                "_reserved_insurance_withdraw_cooldown_slots: 0",
                "config._reserved_insurance_withdraw_cooldown_slots != 0",
            ],
        ),
        (
            "_reserved_last_insurance_withdraw_slot",
            "layout-reserved",
            &[
                "_reserved_last_insurance_withdraw_slot: 0",
                "config._reserved_last_insurance_withdraw_slot != 0",
            ],
        ),
        (
            "max_staleness_secs",
            "oracle-profile-mirror",
            &[
                "config.max_staleness_secs",
                "cfg.max_staleness_secs = profile.max_staleness_secs;",
            ],
        ),
        (
            "hybrid_soft_stale_slots",
            "oracle-profile-mirror",
            &[
                "config.hybrid_soft_stale_slots",
                "cfg.hybrid_soft_stale_slots = profile.hybrid_soft_stale_slots;",
            ],
        ),
        (
            "mark_ewma_e6",
            "oracle-profile-mirror",
            &[
                "config.mark_ewma_e6",
                "cfg.mark_ewma_e6 = profile.mark_ewma_e6;",
            ],
        ),
        (
            "mark_ewma_last_slot",
            "oracle-profile-mirror",
            &[
                "config.mark_ewma_last_slot",
                "cfg.mark_ewma_last_slot = profile.mark_ewma_last_slot;",
            ],
        ),
        (
            "mark_ewma_halflife_slots",
            "oracle-profile-mirror",
            &[
                "config.mark_ewma_halflife_slots",
                "cfg.mark_ewma_halflife_slots = profile.mark_ewma_halflife_slots;",
            ],
        ),
        (
            "mark_min_fee",
            "oracle-profile-mirror",
            &[
                "config.mark_min_fee",
                "cfg.mark_min_fee = profile.mark_min_fee;",
            ],
        ),
        (
            "oracle_target_price_e6",
            "oracle-profile-mirror",
            &[
                "config.oracle_target_price_e6",
                "cfg.oracle_target_price_e6 = profile.oracle_target_price_e6;",
            ],
        ),
        (
            "oracle_target_publish_time",
            "oracle-profile-mirror",
            &[
                "config.oracle_target_publish_time",
                "cfg.oracle_target_publish_time = profile.oracle_target_publish_time;",
            ],
        ),
        (
            "oracle_leg_feeds",
            "oracle-profile-mirror",
            &[
                "config.oracle_leg_feeds",
                "cfg.oracle_leg_feeds = profile.oracle_leg_feeds;",
            ],
        ),
        (
            "oracle_leg_prices_e6",
            "oracle-profile-mirror",
            &[
                "config.oracle_leg_prices_e6",
                "cfg.oracle_leg_prices_e6 = profile.oracle_leg_prices_e6;",
            ],
        ),
        (
            "oracle_leg_publish_times",
            "oracle-profile-mirror",
            &[
                "config.oracle_leg_publish_times",
                "cfg.oracle_leg_publish_times = profile.oracle_leg_publish_times;",
            ],
        ),
        (
            "backing_trade_fee_policy_count",
            "derived-counter",
            &[
                "cfg.backing_trade_fee_policy_count",
                "backing_trade_fee_policy_count: 0",
            ],
        ),
        (
            "backing_trade_fee_insurance_share_bps_long",
            "security-control-and-oracle-profile-mirror",
            &[
                "cfg.backing_trade_fee_insurance_share_bps_long = insurance_share_bps;",
                "config.backing_trade_fee_insurance_share_bps_long",
            ],
        ),
        (
            "backing_trade_fee_insurance_share_bps_short",
            "security-control-and-oracle-profile-mirror",
            &[
                "cfg.backing_trade_fee_insurance_share_bps_short = insurance_share_bps;",
                "config.backing_trade_fee_insurance_share_bps_short",
            ],
        ),
        (
            "fee_redirect_to_market_0_bps",
            "security-control",
            &[
                "cfg.fee_redirect_to_market_0_bps = value",
                "cfg.fee_redirect_to_market_0_bps",
            ],
        ),
        (
            "matcher_req_seq",
            "derived-counter",
            &["config.matcher_req_seq", "bump_matcher_req_seq"],
        ),
        (
            "next_portfolio_id",
            "derived-counter",
            &[
                "state::allocate_portfolio_id(cfg.next_portfolio_id)",
                "cfg.next_portfolio_id = next_portfolio_id;",
            ],
        ),
    ];

    let mut parsed = wrapper_config_fields_from_source(source);
    let mut inventoried: Vec<&str> = inventory.iter().map(|(field, _, _)| *field).collect();
    parsed.sort_unstable();
    inventoried.sort_unstable();
    assert_eq!(
        parsed, inventoried,
        "every persisted WrapperConfigV16 field must be explicitly classified"
    );
    inventoried.dedup();
    assert_eq!(
        inventoried.len(),
        inventory.len(),
        "WrapperConfigV16 field inventory must not contain duplicates"
    );

    for (field, class, source_edges) in inventory {
        assert!(
            matches!(
                *class,
                "security-control"
                    | "security-control-and-oracle-profile-mirror"
                    | "oracle-liveness-mirror"
                    | "oracle-profile-mirror"
                    | "derived-counter"
                    | "liveness-progress"
                    | "layout-reserved"
                    | "layout-padding"
            ),
            "{field} has an unknown INV-087 inventory classification: {class}"
        );
        assert!(
            !source_edges.is_empty(),
            "{field} must carry at least one source edge"
        );
        for edge in *source_edges {
            assert!(
                source.contains(edge),
                "{field} ({class}) missing source edge: {edge}"
            );
        }
    }

    for witness in [
        "v16_program_configure_permissionless_resolve_gated_and_bounded",
        "v16_program_asset_activation_cooldown_is_enforced_and_then_reopens",
        "v16_bpf_policy_authority_and_base_unit_tags_are_bounded_and_persist",
        "v16_program_trade_fee_policy_is_an_enforced_public_admission_control",
        "v16_program_liquidation_cranker_share_policy_is_enforced_at_public_crank",
    ] {
        assert!(
            tests.contains(&format!("fn {witness}")),
            "WrapperConfigV16 inventory depends on executable witness {witness}"
        );
    }
}

fn assert_removed_insurance_policy_reserved_zero(cfg: &state::WrapperConfigV16) {
    assert_eq!(cfg._reserved_insurance_withdraw_max_bps, 0);
    assert_eq!(cfg._reserved_insurance_withdraw_deposits_only, 0);
    assert_eq!(cfg._reserved_insurance_withdraw_cooldown_slots, 0);
    assert_eq!(cfg._reserved_last_insurance_withdraw_slot, 0);
}

#[test]
fn v16_program_removed_insurance_policy_is_zero_reserved_and_not_hidden_state() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let original_market_len = env.svm.get_account(&env.market).unwrap().data.len();
    let (initial_cfg, _) = env.market_state();
    assert_removed_insurance_policy_reserved_zero(&initial_cfg);

    let mut invalid_cursor = initial_cfg;
    invalid_cursor.terminal_slab_scan_progress = u128::from(u64::MAX) + 1;
    let mut candidate = env.svm.get_account(&env.market).unwrap().data;
    let before = candidate.clone();
    assert!(
        state::write_wrapper_config(&mut candidate, &invalid_cursor).is_err(),
        "a terminal cursor wider than the deployed index encoding must fail closed"
    );
    assert_eq!(candidate, before, "failed cursor write must be atomic");

    // Every nonzero reserved field fails before write and leaves canonical bytes unchanged.
    for (label, field) in [
        ("maximum bps", 0u8),
        ("deposits only", 1),
        ("cooldown", 2),
        ("last withdrawal slot", 3),
    ] {
        let mut invalid = initial_cfg;
        match field {
            0 => invalid._reserved_insurance_withdraw_max_bps = 1,
            1 => invalid._reserved_insurance_withdraw_deposits_only = 1,
            2 => invalid._reserved_insurance_withdraw_cooldown_slots = 1,
            3 => invalid._reserved_last_insurance_withdraw_slot = 1,
            _ => unreachable!(),
        }
        let mut candidate = env.svm.get_account(&env.market).unwrap().data;
        let before = candidate.clone();
        assert!(
            state::write_wrapper_config(&mut candidate, &invalid).is_err(),
            "nonzero reserved {label} must fail closed"
        );
        assert_eq!(candidate, before, "failed {label} write must be atomic");
    }

    let source = env.top_up_insurance(1_000);
    assert_eq!(env.token_amount(source), 0);
    assert_removed_insurance_policy_reserved_zero(&env.market_state().0);
    let (destination, _) = env
        .try_withdraw_insurance_asset_with_authority(&admin, 0, 1_000)
        .expect("live insurance authority withdraws its exact top-up");
    assert_eq!(env.token_amount(destination), 1_000);
    assert_removed_insurance_policy_reserved_zero(&env.market_state().0);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data.len(),
        original_market_len,
        "renaming reserved bytes must preserve the deployed account layout"
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault),
        "public insurance round trip preserves engine/SPL custody"
    );
}

#[test]
fn v16_program_all_wrapper_owned_persisted_structs_have_complete_field_rosters() {
    let source = include_str!("../../../src/v16_program.rs");
    let inventories: &[(&str, &[&str])] = &[
        (
            "AssetOracleProfileV16",
            &[
                "oracle_mode",
                "oracle_leg_count",
                "oracle_leg_flags",
                "invert",
                "unit_scale",
                "conf_filter_bps",
                "backing_trade_fee_bps_long",
                "backing_trade_fee_bps_short",
                "backing_trade_fee_insurance_share_bps_long",
                "backing_trade_fee_insurance_share_bps_short",
                "price_move_remainder_bps_num",
                "_padding0",
                "insurance_authority",
                "insurance_operator",
                "backing_bucket_authority",
                "oracle_authority",
                "max_staleness_secs",
                "hybrid_soft_stale_slots",
                "mark_ewma_e6",
                "mark_ewma_last_slot",
                "mark_ewma_halflife_slots",
                "mark_min_fee",
                "oracle_target_price_e6",
                "oracle_target_publish_time",
                "last_good_oracle_slot",
                "oracle_leg_feeds",
                "oracle_leg_prices_e6",
                "oracle_leg_publish_times",
                "asset_admin",
                "funding_mark_e6",
                "funding_mark_pending_e6",
                "funding_mark_pending_slot",
            ],
        ),
        (
            "AssetControlSequencesV16",
            &[
                "oracle_observation",
                "backing_fee",
                "authority_epoch",
                "trade_fee",
                "liquidation_fee",
                "maintenance_fee",
                "fee_redirect",
                "market_init_fee",
                "permissionless_resolve",
                "insurance_top_up",
                "backing_top_up",
            ],
        ),
        (
            "BackingDomainLedgerAccountV16",
            &[
                "market_group",
                "authority",
                "total_principal_atoms",
                "total_deposited_atoms",
                "total_principal_withdrawn_atoms",
                "total_earnings_atoms",
                "total_earnings_withdrawn_atoms",
                "last_observed_bucket_earnings_atoms",
                "cumulative_loss_atoms",
                "cumulative_recovery_atoms",
                "last_observed_unavailable_principal_atoms",
                "domain",
                "_padding",
            ],
        ),
        (
            "InsuranceLedgerAccountV16",
            &[
                "market_group",
                "authority",
                "total_principal_atoms",
                "total_deposited_atoms",
                "total_withdrawn_atoms",
                "cumulative_profit_atoms",
                "cumulative_loss_atoms",
                "last_observed_insurance_atoms",
            ],
        ),
        (
            "PortfolioMatcherConfigV16",
            &[
                "matcher_program",
                "matcher_context",
                "matcher_delegate",
                "control",
            ],
        ),
    ];

    for (name, fields) in inventories {
        assert_exact_persisted_fields(source, name, fields);
        for field in *fields {
            assert!(
                source.matches(field).count() >= 2,
                "{name}.{field} has no source use beyond its persisted declaration",
            );
        }
    }

    // Category-specific edges prevent a field-name occurrence from being mistaken for an
    // enforcement path. These cover all five wrapper-owned structs outside WrapperConfigV16.
    assert_source_edges(
        source,
        "asset oracle profile provenance and authority",
        &[
            "fn validate_asset_oracle_profile(",
            "expect_live_authority(&existing_profile.oracle_authority",
            "expect_live_authority(&authorities.insurance_authority",
            "expect_live_authority(&authorities.backing_bucket_authority",
            "record_funding_mark_transition_view",
            "advance_funding_mark_checkpoint_view",
            "price_move_remainder_bps_num_view",
            "write_oracle_profile_to_view",
        ],
    );
    assert_source_edges(
        source,
        "asset retained-control watermarks",
        &[
            "fn validate_asset_control_sequences(",
            "state::require_newer_control_sequence",
            "fn control_sequence_mut(",
            "write_control_sequences_to_view",
        ],
    );
    assert_source_edges(
        source,
        "backing-domain ledger identity and counters",
        &[
            "fn validate_backing_domain_ledger(",
            "state::read_backing_domain_ledger",
            "write_or_init_backing_domain_ledger",
            "ledger.total_principal_withdrawn_atoms =",
            "ledger.last_observed_bucket_earnings_atoms =",
            "ledger.cumulative_loss_atoms =",
            "ledger.cumulative_recovery_atoms =",
        ],
    );
    assert_source_edges(
        source,
        "insurance ledger identity and counters",
        &[
            "fn validate_insurance_ledger(",
            "state::read_insurance_ledger",
            "write_or_init_insurance_ledger",
            "ledger.last_observed_insurance_atoms =",
            "ledger.cumulative_profit_atoms =",
            "ledger.cumulative_loss_atoms =",
        ],
    );
    assert_source_edges(
        source,
        "portfolio matcher capability binding",
        &[
            "pub fn read_portfolio_matcher_config(",
            "pub fn write_portfolio_matcher_config(",
            "cfg.validate()?;",
            "matcher_program: matcher_prog.key.to_bytes()",
            "matcher_context: matcher_ctx.key.to_bytes()",
            "matcher_delegate: matcher_delegate.key.to_bytes()",
            "read_portfolio_matcher_asset_generation_frontier",
            "write_portfolio_matcher_asset_generation_frontier",
            "matcher_asset_generation_frontier_authorizes",
            "next_portfolio_position_control_for_matcher_sync",
        ],
    );
}

#[test]
fn v16_program_wrapper_security_control_roster_has_source_edges_and_witnesses() {
    let source = include_str!("../../../src/v16_program.rs");
    let tests = include_str!("inv_087_no_phantom_controls_or_dead_security_fields.rs");
    let controls: &[(&str, &[&str], &str)] = &[
        (
            "permissionless resolve policy",
            &[
                "cfg.permissionless_resolve_stale_slots = stale_slots;",
                "cfg.force_close_delay_slots = force_close_delay_slots;",
                "permissionless_resolve_matured_now_view",
                "cfg.force_close_delay_slots",
            ],
            "v16_program_configure_permissionless_resolve_gated_and_bounded",
        ),
        (
            "trade fee floor",
            &[
                "cfg.trade_fee_base_bps = trade_fee_base_bps;",
                "cfg_pre.trade_fee_base_bps > fee_bps",
                "core::cmp::max(caller_fee_bps, cfg.trade_fee_base_bps)",
            ],
            "v16_program_trade_fee_policy_is_an_enforced_public_admission_control",
        ),
        (
            "liquidation cranker share",
            &[
                "cfg.liquidation_cranker_fee_share_bps = value;",
                "cfg.liquidation_cranker_fee_share_bps != 0",
                "policy_v16::fee_share_floor(\n                        retained_fee,\n                        cfg.liquidation_cranker_fee_share_bps,",
            ],
            "v16_program_liquidation_cranker_share_policy_is_enforced_at_public_crank",
        ),
        (
            "maintenance cranker share",
            &[
                "cfg.maintenance_cranker_fee_share_bps = value;",
                "policy_v16::fee_share_floor(\n                        charged,\n                        cfg_pre.maintenance_cranker_fee_share_bps,",
                "credit_maintenance_fee_to_active_market_budgets_view",
            ],
            "v16_bpf_policy_authority_and_base_unit_tags_are_bounded_and_persist",
        ),
        (
            "backing fee policy",
            &[
                "cfg.backing_trade_fee_bps_long = fee_bps;",
                "profile.backing_trade_fee_bps_long = fee_bps;",
                "profile.backing_trade_fee_bps_short = fee_bps;",
            ],
            "v16_bpf_policy_authority_and_base_unit_tags_are_bounded_and_persist",
        ),
        (
            "fee redirect policy",
            &[
                "cfg.fee_redirect_to_market_0_bps = value",
                "policy_v16::fee_share_floor(amount, cfg.fee_redirect_to_market_0_bps)",
            ],
            "v16_bpf_policy_authority_and_base_unit_tags_are_bounded_and_persist",
        ),
        (
            "base-unit mint tags",
            &[
                "cfg.collateral_mint = primary_mint;",
                "cfg.secondary_collateral_mint = secondary_mint;",
                "primary_collateral_mint(&cfg_pre)",
                "secondary_collateral_mint(&cfg)",
                "is_withdrawable_collateral_mint(cfg, &dest.mint)",
            ],
            "v16_bpf_policy_authority_and_base_unit_tags_are_bounded_and_persist",
        ),
    ];

    for (label, source_edges, witness) in controls {
        for edge in *source_edges {
            assert!(
                source.contains(edge),
                "{label} missing source writer/enforcement edge: {edge}"
            );
        }
        assert!(
            tests.contains(&format!("fn {witness}")),
            "{label} missing executable INV-087 witness {witness}"
        );
    }
}

#[test]
fn v16_program_every_wrapper_persisted_security_field_has_a_named_mutation_witness() {
    let local = include_str!("inv_087_no_phantom_controls_or_dead_security_fields.rs");
    let inv_003 = include_str!("inv_003_portfolio_incarnation_binding.rs");
    let inv_005 = include_str!("inv_005_authority_incarnation_binding.rs");
    let inv_012 = include_str!("inv_012_capability_and_delegate_scope.rs");
    let inv_018 = include_str!("inv_018_quote_mint_vault_token_program_and_authority_integrity.rs");
    let inv_019 = include_str!("inv_019_cpi_invocation_and_return_data_binding.rs");
    let inv_020 = include_str!("inv_020_authenticated_clock_slot_and_oracle_provenance.rs");
    let inv_024 = include_str!("inv_024_attributed_quote_value_conservation.rs");
    let inv_025 = include_str!("inv_025_exact_stock_reconciliation.rs");
    let inv_034 = include_str!("inv_034_domain_and_instance_isolation.rs");
    let inv_036 = include_str!("inv_036_fee_destination_and_policy_version_integrity.rs");
    let inv_037 = include_str!("inv_037_exact_residual_partition.rs");
    let inv_038 = include_str!("inv_038_rounding_and_ratio_conservation.rs");
    let inv_045 = include_str!("inv_045_no_free_mark_movement.rs");
    let inv_052 = include_str!("inv_052_split_merge_invariance.rs");
    let inv_054 = include_str!("inv_054_certificate_epoch_completeness.rs");
    let inv_077 = include_str!("inv_077_bounded_work_and_maximum_shape_compute.rs");
    let inv_078 = include_str!("inv_078_permissionless_recovery_coverage.rs");
    let inv_089 = include_str!("inv_089_activation_reactivation_and_initialization_equivalence.rs");
    let inv_008_stateful =
        include_str!("../stateful/inv_008_intent_uniqueness_and_bounded_replay.rs");
    let inv_005_stateful = include_str!("../stateful/inv_005_authority_incarnation_binding.rs");
    let inv_014_stateful =
        include_str!("../stateful/inv_014_delayed_policy_and_policy_epoch_safety.rs");

    // Each group names an executable test that changes or consumes the listed persisted fields
    // through public instructions and checks their security effect. Padding is excluded; reserved
    // wire fields are included because their required effect is fail-closed zero validation.
    let groups: &[(&str, &str, &[&str], &str, &str)] = &[
        (
            "market authority",
            "WrapperConfigV16",
            &["marketauth"],
            inv_005,
            "v16_attack_update_authority_requires_new_authority_signature",
        ),
        (
            "base-unit mint policy",
            "WrapperConfigV16",
            &["collateral_mint", "secondary_collateral_mint"],
            inv_018,
            "v16_attack_base_unit_mints_changeable_only_when_empty",
        ),
        (
            "maintenance charge",
            "WrapperConfigV16",
            &["maintenance_fee_per_slot"],
            inv_024,
            "v16_attack_maintenance_fee_with_open_position_conserves",
        ),
        (
            "permissionless activation fee",
            "WrapperConfigV16",
            &["permissionless_market_init_fee"],
            inv_036,
            "v16_attack_permissionless_create_fee_funds_asset0_insurance",
        ),
        (
            "trade fee floor",
            "WrapperConfigV16",
            &["trade_fee_base_bps"],
            local,
            "v16_program_trade_fee_policy_is_an_enforced_public_admission_control",
        ),
        (
            "permissionless resolve age",
            "WrapperConfigV16",
            &["permissionless_resolve_stale_slots"],
            inv_020,
            "v16_attack_permissionless_resolve_uses_authenticated_clock_slot",
        ),
        (
            "force-close delay",
            "WrapperConfigV16",
            &["force_close_delay_slots"],
            inv_078,
            "v16_bpf_permissionless_market_shutdown_force_closes_recovers_and_reuses_slot",
        ),
        (
            "asset-zero oracle mirror",
            "WrapperConfigV16",
            &[
                "last_good_oracle_slot",
                "unit_scale",
                "conf_filter_bps",
                "oracle_mode",
                "oracle_leg_count",
                "oracle_leg_flags",
                "invert",
                "max_staleness_secs",
                "hybrid_soft_stale_slots",
                "mark_ewma_e6",
                "mark_ewma_last_slot",
                "mark_ewma_halflife_slots",
                "mark_min_fee",
                "oracle_target_price_e6",
                "oracle_target_publish_time",
                "oracle_leg_feeds",
                "oracle_leg_prices_e6",
                "oracle_leg_publish_times",
            ],
            inv_020,
            "v16_program_composite_profile_shutdown_restart_clears_old_provenance",
        ),
        (
            "terminal slab scan progress",
            "WrapperConfigV16",
            &["terminal_slab_scan_progress"],
            inv_077,
            "v16_bpf_terminal_claim_free_surplus_close_stays_bounded_on_10m_market",
        ),
        (
            "removed insurance-policy wire reserve",
            "WrapperConfigV16",
            &[
                "_reserved_insurance_withdraw_max_bps",
                "_reserved_insurance_withdraw_deposits_only",
                "_reserved_insurance_withdraw_cooldown_slots",
                "_reserved_last_insurance_withdraw_slot",
            ],
            local,
            "v16_program_removed_insurance_policy_is_zero_reserved_and_not_hidden_state",
        ),
        (
            "liquidation reward share",
            "WrapperConfigV16",
            &["liquidation_cranker_fee_share_bps"],
            local,
            "v16_program_liquidation_cranker_share_policy_is_enforced_at_public_crank",
        ),
        (
            "maintenance reward share",
            "WrapperConfigV16",
            &["maintenance_cranker_fee_share_bps"],
            inv_077,
            "v16_bpf_sync_maintenance_fee_with_cranker_share_is_bounded",
        ),
        (
            "backing fee policy",
            "WrapperConfigV16",
            &[
                "backing_trade_fee_bps_long",
                "backing_trade_fee_bps_short",
                "backing_trade_fee_policy_count",
                "backing_trade_fee_insurance_share_bps_long",
                "backing_trade_fee_insurance_share_bps_short",
            ],
            inv_038,
            "v16_attack_backing_fee_split_conserves",
        ),
        (
            "free asset slot count",
            "WrapperConfigV16",
            &["free_market_slot_count"],
            inv_089,
            "v16_program_reused_slot_matches_fresh_persisted_state_after_public_history",
        ),
        (
            "market-zero fee redirect",
            "WrapperConfigV16",
            &["fee_redirect_to_market_0_bps"],
            inv_036,
            "v16_attack_fee_redirect_split_lands_correctly",
        ),
        (
            "matcher request sequence",
            "WrapperConfigV16",
            &["matcher_req_seq"],
            inv_019,
            "v16_program_tradecpi_matcher_req_id_advances_monotonically_on_market",
        ),
        (
            "portfolio incarnation allocator",
            "WrapperConfigV16",
            &["next_portfolio_id"],
            inv_003,
            "v16_portfolio_incarnation_id_separates_close_and_reuse",
        ),
        (
            "asset oracle configuration and provenance",
            "AssetOracleProfileV16",
            &[
                "oracle_mode",
                "oracle_leg_count",
                "oracle_leg_flags",
                "invert",
                "unit_scale",
                "conf_filter_bps",
                "max_staleness_secs",
                "hybrid_soft_stale_slots",
                "oracle_target_price_e6",
                "oracle_target_publish_time",
                "last_good_oracle_slot",
                "oracle_leg_feeds",
                "oracle_leg_prices_e6",
                "oracle_leg_publish_times",
            ],
            inv_020,
            "v16_program_composite_provider_roles_cross_lifecycles_and_freshness_boundaries",
        ),
        (
            "asset backing fee mirror",
            "AssetOracleProfileV16",
            &[
                "backing_trade_fee_bps_long",
                "backing_trade_fee_bps_short",
                "backing_trade_fee_insurance_share_bps_long",
                "backing_trade_fee_insurance_share_bps_short",
            ],
            inv_038,
            "v16_attack_backing_fee_split_conserves",
        ),
        (
            "asset mark state",
            "AssetOracleProfileV16",
            &[
                "mark_ewma_e6",
                "mark_ewma_last_slot",
                "mark_ewma_halflife_slots",
                "mark_min_fee",
            ],
            inv_045,
            "v16_program_ewma_mark_respects_per_slot_circuit_breaker",
        ),
        (
            "asset movement remainder",
            "AssetOracleProfileV16",
            &["price_move_remainder_bps_num"],
            inv_052,
            "v16_program_target_change_resets_prior_price_movement_remainder",
        ),
        (
            "asset authority tuple",
            "AssetOracleProfileV16",
            &[
                "insurance_authority",
                "insurance_operator",
                "backing_bucket_authority",
                "oracle_authority",
                "asset_admin",
            ],
            inv_005,
            "v16_attack_per_asset_admin_rotates_keys_isolated_and_burnable",
        ),
        (
            "funding mark checkpoints",
            "AssetOracleProfileV16",
            &[
                "funding_mark_e6",
                "funding_mark_pending_e6",
                "funding_mark_pending_slot",
            ],
            inv_054,
            "v16_attack_target_and_funding_epochs_invalidate_public_released_pnl_cert",
        ),
        (
            "superseding retained controls",
            "AssetControlSequencesV16",
            &[
                "oracle_observation",
                "backing_fee",
                "trade_fee",
                "liquidation_fee",
                "maintenance_fee",
                "fee_redirect",
                "market_init_fee",
                "permissionless_resolve",
            ],
            inv_014_stateful,
            "v16_program_superseded_control_matrix_rejects_stale_overwrites",
        ),
        (
            "authority incarnation",
            "AssetControlSequencesV16",
            &["authority_epoch"],
            inv_005_stateful,
            "v16_program_authority_incarnation_operation_matrix_rejects_aba_replays",
        ),
        (
            "retryable top-up controls",
            "AssetControlSequencesV16",
            &["insurance_top_up", "backing_top_up"],
            inv_008_stateful,
            "v16_program_retry_operation_matrix_rejects_every_stale_retry",
        ),
        (
            "backing ledger market identity",
            "BackingDomainLedgerAccountV16",
            &["market_group"],
            inv_034,
            "v16_attack_backing_ledger_market_binding_enforced",
        ),
        (
            "backing ledger authority and domain identity",
            "BackingDomainLedgerAccountV16",
            &["authority", "domain"],
            inv_034,
            "v16_attack_backing_ledger_domain_binding_enforced",
        ),
        (
            "backing principal ledger",
            "BackingDomainLedgerAccountV16",
            &[
                "total_principal_atoms",
                "total_deposited_atoms",
                "total_principal_withdrawn_atoms",
            ],
            inv_025,
            "v16_program_value_routes_reconcile_vault_capital_insurance_and_backing_stocks",
        ),
        (
            "backing earnings ledger",
            "BackingDomainLedgerAccountV16",
            &[
                "total_earnings_atoms",
                "total_earnings_withdrawn_atoms",
                "last_observed_bucket_earnings_atoms",
            ],
            inv_018,
            "v16_public_backing_earnings_withdrawal_matches_spl_and_internal_quote_deltas",
        ),
        (
            "backing loss and recovery ledger",
            "BackingDomainLedgerAccountV16",
            &[
                "cumulative_loss_atoms",
                "cumulative_recovery_atoms",
                "last_observed_unavailable_principal_atoms",
            ],
            inv_037,
            "v16_bpf_backing_residual_reward_counter_covers_all_trade_paths",
        ),
        (
            "insurance ledger market identity",
            "InsuranceLedgerAccountV16",
            &["market_group"],
            inv_034,
            "v16_attack_insurance_ledger_market_binding_enforced",
        ),
        (
            "insurance ledger authority identity",
            "InsuranceLedgerAccountV16",
            &["authority"],
            inv_034,
            "v16_attack_insurance_ledger_authority_binding_enforced",
        ),
        (
            "insurance principal ledger",
            "InsuranceLedgerAccountV16",
            &["total_principal_atoms", "total_deposited_atoms"],
            inv_025,
            "v16_bpf_accounting_ledger_tags_are_bounded_and_update_state",
        ),
        (
            "insurance withdrawal ledger",
            "InsuranceLedgerAccountV16",
            &["total_withdrawn_atoms"],
            inv_005,
            "v16_attack_live_asset_insurance_withdraw_uses_operator_not_authority",
        ),
        (
            "insurance profit and loss ledger",
            "InsuranceLedgerAccountV16",
            &[
                "cumulative_profit_atoms",
                "cumulative_loss_atoms",
                "last_observed_insurance_atoms",
            ],
            inv_025,
            "v16_program_insurance_ledger_profit_and_loss_follow_public_routes",
        ),
        (
            "portfolio matcher capability",
            "PortfolioMatcherConfigV16",
            &[
                "matcher_program",
                "matcher_context",
                "matcher_delegate",
                "control",
            ],
            inv_012,
            "v16_program_tradecpi_requires_exact_lp_authorized_matcher_tuple",
        ),
    ];

    let production = include_str!("../../../src/v16_program.rs");
    let mut witnessed = std::collections::BTreeSet::new();
    for (label, struct_name, fields, witness_source, witness) in groups {
        assert_named_witness(witness_source, label, witness);
        let persisted = persisted_struct_fields_from_source(production, struct_name);
        for field in *fields {
            assert!(
                persisted.contains(field),
                "{label} names absent persisted field {struct_name}.{field}",
            );
            assert!(
                witnessed.insert(format!("{struct_name}.{field}")),
                "{struct_name}.{field} has duplicate mutation-witness ownership",
            );
        }
    }

    let mut expected = std::collections::BTreeSet::new();
    for struct_name in [
        "WrapperConfigV16",
        "AssetOracleProfileV16",
        "AssetControlSequencesV16",
        "BackingDomainLedgerAccountV16",
        "InsuranceLedgerAccountV16",
        "PortfolioMatcherConfigV16",
    ] {
        for field in persisted_struct_fields_from_source(production, struct_name) {
            if !field.starts_with("_padding") {
                expected.insert(format!("{struct_name}.{field}"));
            }
        }
    }
    assert_eq!(
        witnessed, expected,
        "every non-padding wrapper-owned persisted field needs exactly one executable mutation witness",
    );
}

#[test]
fn v16_program_configure_permissionless_resolve_gated_and_bounded() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let market = env.market;
    let metas = |signer: Pubkey| {
        vec![
            AccountMeta::new(signer, true),
            AccountMeta::new(market, false),
        ]
    };
    let market_before = env.svm.get_account(&env.market).unwrap();

    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.svm.expire_blockhash();
    let unauthorized = env.send(
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 0,
            policy_sequence: u64::MAX,
            stale_slots: 1_000,
            force_close_delay_slots: 1_000,
            authority_epoch: 0,
        },
        metas(mallory.pubkey()),
        &[&mallory],
    );
    assert!(unauthorized.is_err());
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);

    for (stale_slots, force_close_delay_slots, label) in [
        (0, 1_000, "zero stale_slots"),
        (
            percolator_prog::constants::MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS + 1,
            1_000,
            "oversized stale_slots",
        ),
        (1_000, 0, "zero force_close_delay_slots"),
        (
            1_000,
            percolator_prog::constants::MAX_FORCE_CLOSE_DELAY_SLOTS + 1,
            "oversized force_close_delay_slots",
        ),
    ] {
        env.svm.expire_blockhash();
        let rejected = env.send(
            ProgInstruction::ConfigurePermissionlessResolve {
                asset_generation_frontier: 0,
                policy_sequence: u64::MAX,
                stale_slots,
                force_close_delay_slots,
                authority_epoch: 0,
            },
            metas(admin.pubkey()),
            &[&admin],
        );
        assert!(rejected.is_err(), "{label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label} must not mutate the policy",
        );
    }

    env.svm.expire_blockhash();
    let ok = env.send(
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 0,
            policy_sequence: u64::MAX,
            stale_slots: 1_000,
            force_close_delay_slots: 1_000,
            authority_epoch: 0,
        },
        metas(admin.pubkey()),
        &[&admin],
    );
    assert!(ok.is_ok(), "valid admin policy update must land: {ok:?}");
    let cfg = env.market_state().0;
    assert_eq!(cfg.permissionless_resolve_stale_slots, 1_000);
    assert_eq!(cfg.force_close_delay_slots, 1_000);
}

#[test]
fn v16_program_asset_activation_cooldown_is_enforced_and_then_reopens() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let price = 100u64;
    let first = env.market_state().1.config.max_market_slots as u16;

    env.activate_asset(first, 5, price);
    assert_eq!(
        env.market_state().1.config.max_market_slots as u16,
        first + 1
    );

    let metas = vec![
        AccountMeta::new(admin.pubkey(), true),
        AccountMeta::new(env.market, false),
    ];
    let market_id = env.market_state().1.next_market_id;
    let ix = |idx: u16, slot: u64| ProgInstruction::UpdateAssetLifecycle {
        action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
        asset_index: idx,
        market_id,
        authority_epoch: 0,
        now_slot: slot,
        initial_price: price,
        max_init_fee: u128::MAX,
        insurance_authority: admin.pubkey().to_bytes(),
        insurance_operator: admin.pubkey().to_bytes(),
        backing_bucket_authority: admin.pubkey().to_bytes(),
        oracle_authority: admin.pubkey().to_bytes(),
    };

    env.svm.expire_blockhash();
    let same_slot = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ix(first + 1, 5),
        metas.clone(),
        &[&admin],
    );
    assert!(same_slot.is_err(), "same-slot activation must reject");
    assert_eq!(
        env.market_state().1.config.max_market_slots as u16,
        first + 1
    );

    env.svm.warp_to_slot(6);
    env.svm.expire_blockhash();
    let after_cooldown = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ix(first + 1, 6),
        metas,
        &[&admin],
    );
    assert!(
        after_cooldown.is_ok(),
        "activation after cooldown should succeed: {after_cooldown:?}"
    );
    assert_eq!(
        env.market_state().1.config.max_market_slots as u16,
        first + 2
    );
}

#[test]
fn v16_bpf_policy_authority_and_base_unit_tags_are_bounded_and_persist() {
    let mut env = V16CuEnv::new();

    let liquidation_cu = env.update_liquidation_fee_policy_with_cu(1_234);
    assert_cu_within(
        "UpdateLiquidationFeePolicy",
        liquidation_cu,
        CUSTODY_CU_LIMIT,
    );
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.liquidation_cranker_fee_share_bps, 1_234);

    let maintenance_cu = env.update_maintenance_fee_policy_with_cu(2_345);
    assert_cu_within(
        "UpdateMaintenanceFeePolicy",
        maintenance_cu,
        CUSTODY_CU_LIMIT,
    );
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.maintenance_cranker_fee_share_bps, 2_345);

    let backing_cu = env.update_backing_fee_policy_with_cu(0, 77, 5_000);
    assert_cu_within("UpdateBackingFeePolicy", backing_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.backing_trade_fee_bps_long, 77);
    assert_eq!(cfg.backing_trade_fee_insurance_share_bps_long, 5_000);
    assert_eq!(cfg.backing_trade_fee_policy_count, 1);

    let trade_fee_cu = env.update_trade_fee_policy_with_cu(88);
    assert_cu_within("UpdateTradeFeePolicy", trade_fee_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.trade_fee_base_bps, 88);

    let redirect_cu = env.update_fee_redirect_policy_with_cu(2_500);
    assert_cu_within("UpdateFeeRedirectPolicy", redirect_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.fee_redirect_to_market_0_bps, 2_500);

    let secondary_mint = env.create_mint();
    let base_unit_cu = env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    assert_cu_within("UpdateBaseUnitMints", base_unit_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.collateral_mint, env.mint.to_bytes());
    assert_eq!(cfg.secondary_collateral_mint, secondary_mint.to_bytes());

    let primary_source = env.token_account_for_mint(env.mint, env.admin.pubkey(), 50);
    let secondary_dest = env.token_account_for_mint(secondary_mint, env.admin.pubkey(), 0);
    // F-VAULT-FRAG fix: the secondary vault must be the canonical ATA of (vault_authority, secondary_mint).
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 50),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let before_swap_market = env.svm.get_account(&env.market).unwrap().data;
    let swap_cu = env.swap_secondary_for_primary_with_cu(
        primary_source,
        env.vault,
        secondary_dest,
        secondary_vault,
        50,
    );
    assert_cu_within("SwapSecondaryForPrimary", swap_cu, CUSTODY_CU_LIMIT);
    assert_eq!(env.token_amount(primary_source), 0);
    assert_eq!(env.token_amount(env.vault), 50);
    assert_eq!(env.token_amount(secondary_dest), 50);
    assert_eq!(env.token_amount(secondary_vault), 0);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_swap_market,
        "base-unit swap must only move SPL custody"
    );

    let new_asset_authority = Keypair::new();
    let authority_cu = env.update_asset_authority_with_cu(&new_asset_authority);
    assert_cu_within("UpdateAuthority", authority_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.marketauth, new_asset_authority.pubkey().to_bytes());
}

#[test]
fn v16_program_trade_fee_policy_is_an_enforced_public_admission_control() {
    const FEE_FLOOR_BPS: u64 = 50;

    let mut env = V16CuEnv::new();
    env.update_trade_fee_policy_with_cu(FEE_FLOOR_BPS);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_long = env.svm.get_account(&long).unwrap();
    let before_short = env.svm.get_account(&short).unwrap();
    let before_vault = env.svm.get_account(&env.vault).unwrap();
    let rejected = env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        100,
        FEE_FLOOR_BPS - 1,
    );
    assert!(
        rejected.is_err(),
        "trade below the persisted base-fee floor must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before_market,
        "rejected below-floor trade must not mutate market state"
    );
    assert_eq!(
        env.svm.get_account(&long).unwrap(),
        before_long,
        "rejected below-floor trade must not mutate taker state"
    );
    assert_eq!(
        env.svm.get_account(&short).unwrap(),
        before_short,
        "rejected below-floor trade must not mutate maker state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        before_vault,
        "rejected below-floor trade must not move custody"
    );

    let cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        100,
        FEE_FLOOR_BPS,
    );
    assert_cu_within("base-fee-floor TradeNoCpi", cu, TRADE_CU_LIMIT);
    let (_, group) = env.market_state();
    assert_eq!(
        group.assets[0].oi_eff_long_q, POS_SCALE,
        "at-floor trade must execute and create long OI"
    );
    assert_eq!(
        group.assets[0].oi_eff_short_q, POS_SCALE,
        "at-floor trade must execute and create short OI"
    );
}

#[test]
fn v16_program_liquidation_cranker_share_policy_is_enforced_at_public_crank() {
    const SHARE_BPS: u16 = 2_500;
    const LIQ_SLOT: u64 = 30;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(SHARE_BPS);
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let cranker = env.create_portfolio(&cranker_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&short_owner, short, 100_000);
    env.deposit(&cranker_owner, cranker, 1_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        1_000_000,
        0,
    );

    for slot in 1..=LIQ_SLOT {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        );
    }
    assert!(
        health_cert(&env.portfolio_state(short)).certified_liq_deficit != 0,
        "setup must make the short liquidatable before testing reward policy enforcement"
    );

    let cranker_cap_before = env.portfolio_state(cranker).capital.get();
    let (_, group_before) = env.market_state();
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: LIQ_SLOT,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(cranker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
                AccountMeta::new(cranker, false),
            ],
            &[&cranker_owner],
        )
        .expect("liquidation crank with reward portfolio");
    assert_cu_within(
        "liquidation crank with 25% cranker share",
        cu,
        CRANK_CU_LIMIT,
    );

    let (_, group_after) = env.market_state();
    let cranker_delta = env
        .portfolio_state(cranker)
        .capital
        .get()
        .checked_sub(cranker_cap_before)
        .expect("cranker capital increases monotonically");
    let retained_insurance_delta = group_after
        .insurance
        .checked_sub(group_before.insurance)
        .expect("liquidation retains non-cranker fee in insurance");
    let charged_fee = cranker_delta + retained_insurance_delta;
    assert!(charged_fee > 0, "liquidation charged a nonzero fee");
    assert_eq!(
        cranker_delta,
        charged_fee * SHARE_BPS as u128 / 10_000,
        "public liquidation crank must use the persisted cranker-share policy"
    );
    assert_eq!(
        retained_insurance_delta,
        charged_fee - cranker_delta,
        "the non-cranker portion remains attributed to insurance"
    );
    assert_eq!(
        health_cert(&env.portfolio_state(short)).certified_liq_deficit,
        0,
        "liquidation reward is paid only after reducing the account back to current"
    );
    assert_eq!(
        group_after.vault, group_before.vault,
        "liquidation fee split is internal and mints no vault tokens"
    );
    assert_eq!(
        group_after.vault as u64,
        env.token_amount(env.vault),
        "vault accounting remains tied to SPL custody"
    );
    assert!(
        group_after.vault >= group_after.c_tot + group_after.insurance,
        "senior conservation"
    );
    assert_domain_budget_remaining_total_consistent(
        &group_after,
        "liquidation cranker share policy enforcement",
    );
}
