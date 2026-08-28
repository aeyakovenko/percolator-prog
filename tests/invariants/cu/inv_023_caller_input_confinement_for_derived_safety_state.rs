//! INV-023 - caller-input confinement for derived safety state.
//!
//! `CrankObservationHint` is discovery input: it tells the wrapper which oracle
//! accounts the caller supplied. It must not become an authority to partially
//! mutate market time, oracle checkpoints, funding, or account state when a later
//! caller-controlled hint proves malformed. These tests intentionally place a valid
//! observation before a bad one so the only correct outcome is full instruction
//! failure and exact SVM rollback. A production-source-bound roster also assigns
//! every field in all 50 public instructions and their nested batch/hint structs to
//! one semantic trust class and one executable invariant witness. The only public
//! B-settlement work cap is additionally compared with a one-atom cap versus an
//! unbounded cap on the first canonical chunk from the same publicly reached state;
//! the engine-selected chunk and final economic state must remain identical. The
//! resolved compatibility tag and the sole public crank are compared from an exact
//! snapshot, and a source-derived dispatch audit proves every shared handler receives
//! a compile-time scope discriminator rather than a caller-selected safety lane.

use super::*;

fn assert_late_bad_crank_hint_rolls_back(label: &str, bad_tail: CrankObservationHint) {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.svm.warp_to_slot(5);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let slot_before = env.market_state().1.assets[0].slot_last;

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                bad_tail,
            ],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "{label}: malformed late hint must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "{label}: failed crank rolls back market bytes and lamports",
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "{label}: failed crank rolls back portfolio bytes and lamports",
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "{label}: failed crank cannot move custody",
    );

    env.svm.expire_blockhash();
    let valid_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("same public route succeeds once the hostile hint is removed");
    assert_cu_within(label, valid_cu, CRANK_CU_LIMIT);
    assert!(
        env.market_state().1.assets[0].slot_last > slot_before,
        "{label}: non-vacuous control proves the first hint would have advanced state",
    );
}

#[test]
fn v16_program_duplicate_crank_hint_after_valid_hint_rolls_back_partial_state() {
    assert_late_bad_crank_hint_rolls_back(
        "INV-023 duplicate crank hint",
        CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        },
    );
}

#[test]
fn v16_program_out_of_range_crank_hint_after_valid_hint_rolls_back_partial_state() {
    assert_late_bad_crank_hint_rolls_back(
        "INV-023 out-of-range crank hint",
        CrankObservationHint {
            asset_index: 1,
            oracle_accounts: 0,
        },
    );
}

#[derive(Debug, PartialEq, Eq)]
struct Inv023ForfeitBudgetFrame {
    market_account: Account,
    counterparty_account: Account,
    vault_account: Account,
    group: MarketGroupV16,
    loss: PortfolioAccountV16,
    counterparty: PortfolioAccountV16,
    live_counterparty: PortfolioAccountV16,
    live_peer: PortfolioAccountV16,
    vault_atoms: u64,
}

fn inv023_forfeit_budget_frame(
    env: &mut V16CuEnv,
    loss: Pubkey,
    counterparty_owner: &Keypair,
    counterparty: Pubkey,
    live_counterparty: Pubkey,
    live_peer: Pubkey,
    target_b: u128,
    budgets: &[u128],
) -> Inv023ForfeitBudgetFrame {
    assert_eq!(
        env.market_state().1.assets[1].b_long_num,
        target_b,
        "caller budget must not change the canonical B target"
    );
    let before = active_leg_for_asset(&env.portfolio_state(counterparty), 1);
    assert!(target_b > before.b_snap, "setup must create a real B gap");

    for &budget in budgets {
        if !has_active_leg_for_asset(&env.portfolio_state(counterparty), 1) {
            break;
        }
        env.svm.expire_blockhash();
        let cu = env.forfeit_recovery_leg_with_cu(counterparty_owner, counterparty, 1, budget);
        assert_cu_within("INV-023 B-budget forfeit", cu, CUSTODY_CU_LIMIT);
    }

    let counterparty_state = env.portfolio_state(counterparty);
    let settled = active_leg_for_asset(&counterparty_state, 1);
    assert_eq!(
        settled.b_snap, target_b,
        "the supplied budget schedule must consume the complete B gap"
    );
    assert!(!settled.b_stale, "the B gap must no longer be actionable");
    Inv023ForfeitBudgetFrame {
        market_account: env.svm.get_account(&env.market).unwrap(),
        counterparty_account: env.svm.get_account(&counterparty).unwrap(),
        vault_account: env.svm.get_account(&env.vault).unwrap(),
        group: env.market_state().1,
        loss: env.portfolio_state(loss),
        counterparty: counterparty_state,
        live_counterparty: env.portfolio_state(live_counterparty),
        live_peer: env.portfolio_state(live_peer),
        vault_atoms: env.token_amount(env.vault),
    }
}

#[test]
fn v16_program_recovery_b_budget_changes_work_partition_not_economic_truth() {
    let PublicActiveCloseFixture {
        mut env,
        loss,
        asset1_counterparty_owner,
        asset1_counterparty,
        live_counterparty,
        live_peer,
        ..
    } = public_asset1_bankrupt_close_fixture();

    // The fixture's first owner forfeit creates a live close residual. This public
    // continuation books the second loss atom and makes the counterparty's leg
    // genuinely B-stale before the caller budget is varied.
    env.svm.expire_blockhash();
    let booking_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 4,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("public close continuation books the second loss atom");
    assert_cu_within("INV-023 B-budget setup", booking_cu, CRANK_CU_LIMIT);
    let target_b = env.market_state().1.assets[1].b_long_num;

    let market_before = env.svm.get_account(&env.market).unwrap();
    let counterparty_before = env.svm.get_account(&asset1_counterparty).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let unbounded_prefix = inv023_forfeit_budget_frame(
        &mut env,
        loss,
        &asset1_counterparty_owner,
        asset1_counterparty,
        live_counterparty,
        live_peer,
        target_b,
        &[u128::MAX, u128::MAX],
    );

    env.svm.set_account(env.market, market_before).unwrap();
    env.svm
        .set_account(asset1_counterparty, counterparty_before)
        .unwrap();
    env.svm.set_account(env.vault, vault_before).unwrap();
    let one_atom_prefix = inv023_forfeit_budget_frame(
        &mut env,
        loss,
        &asset1_counterparty_owner,
        asset1_counterparty,
        live_counterparty,
        live_peer,
        target_b,
        &[1, u128::MAX],
    );
    assert_eq!(
        one_atom_prefix, unbounded_prefix,
        "B-loss atom budget may cap bounded work but must not choose the canonical chunk or change value, attribution, OI, unrelated accounts, or custody"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct Inv023ResolvedAliasFrame {
    market_account: Account,
    portfolio_account: Account,
    vault_account: Account,
    destination_account: Account,
    group: MarketGroupV16,
    portfolio: PortfolioAccountV16,
    vault_atoms: u64,
    destination_atoms: u64,
}

fn inv023_resolved_alias_frame(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    destination: Pubkey,
    permissionless_crank: bool,
) -> Inv023ResolvedAliasFrame {
    let instruction = if permissionless_crank {
        ProgInstruction::PermissionlessCrank {
            now_slot: env.svm.get_sysvar::<Clock>().slot,
            observations: vec![],
        }
    } else {
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: u128::MAX,
        }
    };
    env.svm.expire_blockhash();
    let cu = env
        .send(
            instruction,
            vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
        .expect("resolved public route must settle the capital-only account");
    assert_cu_within("INV-023 resolved route alias", cu, CRANK_CU_LIMIT);

    Inv023ResolvedAliasFrame {
        market_account: env.svm.get_account(&env.market).unwrap(),
        portfolio_account: env.svm.get_account(&portfolio).unwrap(),
        vault_account: env.svm.get_account(&env.vault).unwrap(),
        destination_account: env.svm.get_account(&destination).unwrap(),
        group: env.market_state().1,
        portfolio: env.portfolio_state(portfolio),
        vault_atoms: env.token_amount(env.vault),
        destination_atoms: env.token_amount(destination),
    }
}

#[test]
fn v16_program_resolved_close_alias_cannot_change_auto_crank_economic_truth() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);
    env.resolve();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let close_destination = env.token_account(owner.pubkey(), 0);
    let close_frame =
        inv023_resolved_alias_frame(&mut env, &owner, portfolio, close_destination, false);

    env.svm.set_account(env.market, market_before).unwrap();
    env.svm.set_account(portfolio, portfolio_before).unwrap();
    env.svm.set_account(env.vault, vault_before).unwrap();
    let crank_destination = env.token_account(owner.pubkey(), 0);
    let crank_frame =
        inv023_resolved_alias_frame(&mut env, &owner, portfolio, crank_destination, true);

    assert_eq!(
        crank_frame, close_frame,
        "CloseResolved's ignored legacy scalar and PermissionlessCrank's discovery payload must not select different resolved work or value paths",
    );
    assert_eq!(crank_frame.destination_atoms, 1_000_000);
}

fn inv023_dispatcher_source(production: &str) -> &str {
    production
        .split("    pub fn process_instruction<'a>(\n")
        .nth(1)
        .expect("processor dispatcher exists")
        .split("    #[inline(never)]\n    fn handle_init_market")
        .next()
        .expect("dispatcher ends before the first handler")
}

fn inv023_dispatch_arm<'a>(dispatcher: &'a str, variant: &str) -> &'a str {
    let marker = format!("            Instruction::{variant}");
    let start = dispatcher
        .find(&marker)
        .unwrap_or_else(|| panic!("dispatcher arm for {variant}"));
    let tail = &dispatcher[start..];
    let end = tail[marker.len()..]
        .find("\n            Instruction::")
        .map_or(tail.len(), |offset| marker.len() + offset);
    &tail[..end]
}

fn inv023_handler_name(arm: &str) -> &str {
    let start = arm.find("handle_").expect("dispatch arm calls a handler");
    let tail = &arm[start..];
    let end = tail
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .unwrap_or(tail.len());
    &tail[..end]
}

fn inv023_source_contains_test(source: &str, test_name: &str) -> bool {
    source.contains(&format!("#[test]\nfn {test_name}("))
        || source.contains(&format!("#[test]\n    fn {test_name}("))
}

#[test]
fn v16_program_alternate_entrypoints_cannot_select_internal_safety_lanes() {
    use std::collections::{BTreeMap, BTreeSet};

    const PRODUCTION: &str = include_str!("../../../src/v16_program.rs");
    const ACCOUNT_ROLE_ROSTER: &str = include_str!("../inv_017_account_role_coverage.tsv");
    const ACCOUNT_ROLE_TESTS: &str =
        include_str!("inv_017_signer_writable_role_and_account_alias_safety.rs");
    const BOUNDARY_TESTS: &str = include_str!("inv_083_boundary_completeness.rs");
    const RETRY_TESTS: &str = include_str!("inv_008_intent_uniqueness_and_bounded_replay.rs");
    const STATEFUL_RETRY_TESTS: &str =
        include_str!("../stateful/inv_008_intent_uniqueness_and_bounded_replay.rs");
    const ROUTE_TESTS: &str = include_str!("inv_047_equivalent_route_semantics.rs");
    const STATEFUL_ROUTE_TESTS: &str =
        include_str!("../stateful/inv_047_equivalent_route_semantics.rs");
    const INSURANCE_TESTS: &str =
        include_str!("inv_064_insurance_withdrawal_policy_equivalence.rs");
    const THIS_SOURCE: &str =
        include_str!("inv_023_caller_input_confinement_for_derived_safety_state.rs");

    let production_variants = inv023_instruction_fields(PRODUCTION)
        .into_iter()
        .map(|(variant, _)| variant)
        .collect::<BTreeSet<_>>();
    assert_eq!(production_variants.len(), 50);

    // Compose INV-017 without duplicating its dynamic pairwise matrix: its source-locked
    // roster must remain exhaustive for exactly the same production variants, and every
    // roster owner must remain an actual mounted test.
    let mut role_variants = BTreeSet::new();
    for line in ACCOUNT_ROLE_ROSTER.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with("tag\t") {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 5, "malformed INV-017 role row: {line}");
        assert_eq!(
            columns[2], "EXHAUSTIVE",
            "{} role matrix reopened",
            columns[1]
        );
        assert!(
            role_variants.insert(columns[1].to_owned()),
            "duplicate INV-017 role variant {}",
            columns[1],
        );
        assert!(
            inv023_source_contains_test(ACCOUNT_ROLE_TESTS, columns[3]),
            "missing INV-017 account-role owner {}",
            columns[3],
        );
    }
    assert_eq!(role_variants, production_variants);

    // INV-083 owns the 228 field-or-no-data boundary matrix. Source-lock the composition edge so
    // its closure cannot silently disappear while INV-023 continues to claim it.
    assert!(inv023_source_contains_test(
        BOUNDARY_TESTS,
        "v16_program_every_public_input_field_has_a_boundary_profile_and_executable_witness",
    ));
    assert!(BOUNDARY_TESTS.contains("const EXPECTED_FIELD_COUNT: usize = 228;"));
    assert!(BOUNDARY_TESTS.contains("const EXPECTED_TYPE_COUNT: usize = 53;"));

    let dispatcher = inv023_dispatcher_source(PRODUCTION);
    let mut variants_by_handler = BTreeMap::<String, BTreeSet<String>>::new();
    for variant in &production_variants {
        let arm = inv023_dispatch_arm(dispatcher, variant);
        variants_by_handler
            .entry(inv023_handler_name(arm).to_owned())
            .or_default()
            .insert(variant.clone());
    }
    let shared_handlers = variants_by_handler
        .into_iter()
        .filter(|(_, variants)| variants.len() > 1)
        .collect::<BTreeMap<_, _>>();
    let expected_shared_handlers = BTreeMap::from([
        (
            "handle_configure_managed_mark".to_owned(),
            ["ConfigureAuthMark", "ConfigureEwmaMark"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        ),
        (
            "handle_push_managed_mark".to_owned(),
            ["PushAuthMark", "PushEwmaMark"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        ),
        (
            "handle_top_up_insurance".to_owned(),
            ["TopUpInsurance", "TopUpInsuranceDomain"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        ),
        (
            "handle_update_market_authority_policy".to_owned(),
            [
                "UpdateFeeRedirectPolicy",
                "UpdateLiquidationFeePolicy",
                "UpdateMaintenanceFeePolicy",
                "UpdateMarketInitFeePolicy",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        ),
    ]);
    assert_eq!(
        shared_handlers, expected_shared_handlers,
        "a new or regrouped shared handler needs typed-lane and alternate-route review",
    );

    for (variant, typed_lane) in [
        ("TopUpInsurance", "InsuranceTopUpScope::BaseMarket"),
        (
            "TopUpInsuranceDomain",
            "InsuranceTopUpScope::Domain(domain as usize)",
        ),
        (
            "UpdateLiquidationFeePolicy",
            "MarketAuthorityPolicyUpdate::LiquidationCrankerShare(cranker_share_bps)",
        ),
        (
            "UpdateMaintenanceFeePolicy",
            "MarketAuthorityPolicyUpdate::MaintenanceCrankerShare(cranker_share_bps)",
        ),
        (
            "UpdateFeeRedirectPolicy",
            "MarketAuthorityPolicyUpdate::FeeRedirect(redirect_bps)",
        ),
        (
            "UpdateMarketInitFeePolicy",
            "MarketAuthorityPolicyUpdate::MarketInitFee(min_init_fee)",
        ),
        ("ConfigureEwmaMark", "ManagedMarkKind::Ewma"),
        ("ConfigureAuthMark", "ManagedMarkKind::Authority"),
        ("PushEwmaMark", "ManagedMarkKind::Ewma"),
        ("PushAuthMark", "ManagedMarkKind::Authority"),
    ] {
        assert!(
            inv023_dispatch_arm(dispatcher, variant).contains(typed_lane),
            "{variant} must pass its compile-time typed lane {typed_lane}",
        );
    }

    // These are the current semantic multi-entrypoint families. Each edge is
    // exercised dynamically; the retained-family partition in INV-008 also fails
    // if a new retryable family gains a second public route.
    for (source, test_name) in [
        (
            RETRY_TESTS,
            "v16_public_replay_disposition_roster_is_source_complete",
        ),
        (
            STATEFUL_RETRY_TESTS,
            "v16_program_retry_operation_matrix_rejects_every_stale_retry",
        ),
        (
            STATEFUL_ROUTE_TESTS,
            "v16_program_nonzero_fee_trade_routes_are_byte_exact_after_transport_normalization",
        ),
        (
            ROUTE_TESTS,
            "v16_program_legacy_insurance_topup_matches_explicit_domain_split",
        ),
        (
            ROUTE_TESTS,
            "v16_program_authority_and_permissionless_resolution_match_at_maturity",
        ),
        (
            INSURANCE_TESTS,
            "v16_program_live_and_terminal_insurance_routes_share_one_finite_budget",
        ),
        (
            THIS_SOURCE,
            "v16_program_resolved_close_alias_cannot_change_auto_crank_economic_truth",
        ),
    ] {
        assert!(
            inv023_source_contains_test(source, test_name),
            "alternate-route composition witness {test_name} is missing",
        );
    }

    let crank_handler = PRODUCTION
        .split("    fn handle_permissionless_crank<'a>(")
        .nth(1)
        .expect("permissionless crank handler")
        .split("\n    fn account<'a>(")
        .next()
        .expect("permissionless crank handler boundary");
    assert!(crank_handler.contains("handle_permissionless_crank_zero_copy("));
    assert!(
        crank_handler.contains("handle_close_resolved(program_id, accounts, 0, Some(now_slot))")
    );
    assert!(PRODUCTION.contains(
        "build_actionable_summary_at_slot(&portfolio.as_view(), authenticated_now_slot)"
    ));
    assert!(PRODUCTION.contains("permissionless_auto_crank_not_atomic("));
}

fn inv023_is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn inv023_instruction_fields(source: &str) -> std::collections::BTreeSet<(String, String)> {
    let marker = "pub enum Instruction {";
    let start = source
        .find(marker)
        .expect("production public Instruction enum")
        + marker.len();
    let tail = &source[start..];
    let end = tail
        .find("\n    impl Instruction {")
        .expect("Instruction implementation after enum");
    let mut fields = std::collections::BTreeSet::new();
    let mut current_variant: Option<String> = None;

    for line in tail[..end].lines() {
        let line = line.trim();
        if let Some(variant) = current_variant.as_ref() {
            if line == "}," {
                current_variant = None;
                continue;
            }
            if let Some((field, _)) = line.split_once(':') {
                if inv023_is_identifier(field) {
                    assert!(
                        fields.insert((variant.clone(), field.to_owned())),
                        "duplicate production instruction field {variant}.{field}",
                    );
                }
            }
            continue;
        }

        if let Some(variant) = line.strip_suffix(" {") {
            if inv023_is_identifier(variant) {
                current_variant = Some(variant.to_owned());
            }
        } else if let Some(variant) = line.strip_suffix(',') {
            if inv023_is_identifier(variant)
                && variant
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_uppercase)
            {
                assert!(
                    fields.insert((variant.to_owned(), "-".to_owned())),
                    "duplicate unit instruction variant {variant}",
                );
            }
        }
    }
    fields
}

fn inv023_struct_fields(
    source: &str,
    type_name: &str,
) -> std::collections::BTreeSet<(String, String)> {
    let marker = format!("pub struct {type_name} {{");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("production public input struct {type_name}"))
        + marker.len();
    let tail = &source[start..];
    let end = tail
        .find("\n    }")
        .unwrap_or_else(|| panic!("closing brace for public input struct {type_name}"));
    let mut fields = std::collections::BTreeSet::new();
    for line in tail[..end].lines() {
        let line = line.trim();
        let Some(line) = line.strip_prefix("pub ") else {
            continue;
        };
        let Some((field, _)) = line.split_once(':') else {
            continue;
        };
        if inv023_is_identifier(field) {
            assert!(
                fields.insert((type_name.to_owned(), field.to_owned())),
                "duplicate production nested input field {type_name}.{field}",
            );
        }
    }
    fields
}

#[test]
fn v16_program_caller_input_roster_owns_every_production_field() {
    const PRODUCTION: &str = include_str!("../../../src/v16_program.rs");
    const ROSTER: &str = include_str!("../inv_023_caller_input_roster.tsv");
    const HEADER: &str = "type\tfields\tclassification\tevidence";
    const ALLOWED_CLASSES: [&str; 9] = [
        "SIGNED_CONFIG",
        "SIGNED_ECONOMIC",
        "IDENTITY_BINDING",
        "SCOPE_SELECTOR",
        "AUTHENTICATED_TIME",
        "DISCOVERY_HINT",
        "REPLAY_GUARD",
        "BOUNDED_WORK",
        "IGNORED_LEGACY",
    ];

    let mut production_fields = inv023_instruction_fields(PRODUCTION);
    for type_name in ["BatchTradeLeg", "BatchTradeCpiLeg", "CrankObservationHint"] {
        production_fields.extend(inv023_struct_fields(PRODUCTION, type_name));
    }

    let mut roster_fields = std::collections::BTreeSet::new();
    let mut covered_classes = std::collections::BTreeSet::new();
    let mut discovery_fields = std::collections::BTreeSet::new();
    let mut evidence_sources = std::collections::BTreeMap::<String, String>::new();
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut saw_header = false;
    for (line_number, line) in ROSTER.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !saw_header {
            assert_eq!(line, HEADER, "INV-023 roster header changed");
            saw_header = true;
            continue;
        }
        let columns: Vec<_> = line.split('\t').collect();
        assert_eq!(
            columns.len(),
            4,
            "malformed INV-023 roster row {}: {line}",
            line_number + 1,
        );
        let type_name = columns[0];
        let classification = columns[2];
        assert!(
            ALLOWED_CLASSES.contains(&classification) || classification == "NO_CALLER_DATA",
            "unknown INV-023 classification {classification}",
        );
        covered_classes.insert(classification);

        let (evidence_path, evidence_test) = columns[3]
            .split_once('#')
            .unwrap_or_else(|| panic!("evidence must be path#test on row {}", line_number + 1));
        let evidence_source = evidence_sources
            .entry(evidence_path.to_owned())
            .or_insert_with(|| {
                std::fs::read_to_string(manifest.join(evidence_path)).unwrap_or_else(|error| {
                    panic!("read INV-023 evidence {evidence_path}: {error}")
                })
            });
        assert!(
            evidence_source.contains(&format!("fn {evidence_test}(")),
            "INV-023 evidence function {evidence_path}#{evidence_test} is missing",
        );

        for field in columns[1].split(',') {
            assert!(inv023_is_identifier(field) || field == "-");
            let key = (type_name.to_owned(), field.to_owned());
            assert!(
                roster_fields.insert(key.clone()),
                "duplicate INV-023 roster field {}.{}",
                key.0,
                key.1,
            );

            if field == "-" {
                assert_eq!(classification, "NO_CALLER_DATA");
            }
            if matches!(field, "now_slot" | "now_unix_ts") {
                assert_eq!(classification, "AUTHENTICATED_TIME");
            }
            if field.ends_with("sequence") {
                assert_eq!(classification, "REPLAY_GUARD");
            }
            if field.ends_with("_epoch") {
                assert_eq!(classification, "IDENTITY_BINDING");
            }
            if matches!(field, "close_q" | "b_delta_budget") {
                assert_eq!(classification, "BOUNDED_WORK");
            }
            if field == "reduce_q" {
                assert_eq!(classification, "SIGNED_ECONOMIC");
            }
            if field == "fee_rate_per_slot" {
                assert_eq!(classification, "IGNORED_LEGACY");
            }
            if type_name == "CrankObservationHint"
                || (type_name == "PermissionlessCrank" && field == "observations")
            {
                assert_eq!(classification, "DISCOVERY_HINT");
            }
            if classification == "DISCOVERY_HINT" {
                discovery_fields.insert((type_name.to_owned(), field.to_owned()));
            }
        }
    }
    assert!(saw_header, "INV-023 roster header is missing");
    assert_eq!(
        roster_fields, production_fields,
        "INV-023 roster must classify every production caller-input field exactly once; missing={:?}; stale={:?}",
        production_fields
            .difference(&roster_fields)
            .collect::<Vec<_>>(),
        roster_fields
            .difference(&production_fields)
            .collect::<Vec<_>>(),
    );
    for classification in ALLOWED_CLASSES {
        assert!(
            covered_classes.contains(classification),
            "INV-023 trust class {classification} has no production field",
        );
    }
    assert!(covered_classes.contains("NO_CALLER_DATA"));
    assert_eq!(
        discovery_fields,
        [
            ("CrankObservationHint".to_owned(), "asset_index".to_owned()),
            (
                "CrankObservationHint".to_owned(),
                "oracle_accounts".to_owned(),
            ),
            ("PermissionlessCrank".to_owned(), "observations".to_owned()),
        ]
        .into_iter()
        .collect(),
        "a new discovery field requires hostile omission, duplication, order, and malformed-tail coverage",
    );

    let instruction_types = production_fields
        .iter()
        .map(|(type_name, _)| type_name)
        .filter(|type_name| {
            !matches!(
                type_name.as_str(),
                "BatchTradeLeg" | "BatchTradeCpiLeg" | "CrankObservationHint"
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        instruction_types.len(),
        50,
        "roster must remain bound to every public instruction variant",
    );
}
