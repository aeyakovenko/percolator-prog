//! INV-083 - Boundary completeness.
//!
//! Normative obligation: public routes cover zero, one, maximum, maximum-plus
//! and shape-boundary cases. Any value excluded by a handler must reject before
//! allocation, mutation, panic, or custody movement.
//!
//! Evidence in this file (I/C): oversized batch leg vectors at the public decode
//! boundary reject as instruction data errors rather than allocating a large
//! vector or panicking the SBF program. The machine-readable class roster and
//! source-locked caller-input inventory assign all 230 field-or-no-data subjects across 52 public
//! input types to 20 semantic boundary profiles, per-field public evidence, and
//! profile-level boundary evidence. InitMarket's complete validation predicate
//! is exercised through public exact-rollback failures and live retries. Other
//! boundary cases remain distributed across their economic invariant owners and
//! the full-width instruction-decoder Kani proofs.
//!
//! Guarantee boundary: this closes the current public input surface. A new input
//! type or field, changed profile count, scalar validation predicate, or wider
//! supported shape reopens INV-083. Deployed wide-arithmetic equivalence remains
//! separately owned by INV-085.

use super::*;

const INV_083_BOUNDARY_ROSTER: &str = include_str!("../inv_083_boundary_roster.tsv");
const INV_083_CALLER_INPUT_ROSTER: &str = include_str!("../inv_023_caller_input_roster.tsv");
const INV_083_BOUNDARY_ROSTER_HEADER: &str =
    "class\tinvariant\towner_file\ttest_function\tboundary_value\tcoverage_note";
const INV_083_REQUIRED_BOUNDARY_CLASSES: &[&str] = &[
    "zero",
    "one",
    "max-1",
    "max",
    "expiry-1",
    "expiry-equal",
    "expiry+1",
    "cross-zero",
    "empty",
    "full",
    "near-overflow",
];

#[derive(Clone, Copy)]
struct Inv083BoundaryProfile {
    name: &'static str,
    evidence: &'static str,
}

const INV_083_BOUNDARY_PROFILES: &[Inv083BoundaryProfile] = &[
    Inv083BoundaryProfile { name: "amount", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_attack_large_amount_deposit_withdraw_exact" },
    Inv083BoundaryProfile { name: "authenticated-time", evidence: "tests/invariants/cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs#v16_bpf_permissionless_crank_uses_authenticated_clock_slot_not_caller_slot" },
    Inv083BoundaryProfile { name: "basis-points", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_attack_init_market_rejects_grief_config_without_burning_market_account" },
    Inv083BoundaryProfile { name: "bitmask", evidence: "tests/invariants/cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs#v16_program_hybrid_oracle_rejects_duplicate_or_malformed_leg_config" },
    Inv083BoundaryProfile { name: "count", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_attack_init_market_rejects_grief_config_without_burning_market_account" },
    Inv083BoundaryProfile { name: "duration", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_attack_init_market_rejects_grief_config_without_burning_market_account" },
    Inv083BoundaryProfile { name: "enum", evidence: "tests/invariants/cu/inv_065_reset_recovery_and_retired_state_isolation.rs#v16_attack_finalize_reset_side_requires_empty_side_counts" },
    Inv083BoundaryProfile { name: "expiry", evidence: "tests/invariants/cu/inv_028_source_domain_realizability_cap.rs#v16_attack_backing_bucket_topup_withdraw_input_gates" },
    Inv083BoundaryProfile { name: "identity", evidence: "tests/invariants/cu/inv_002_asset_generation_binding.rs#v16_program_asset_generation_field_and_guard_roster_is_source_complete" },
    Inv083BoundaryProfile { name: "ignored", evidence: "tests/invariants/cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs#v16_attack_close_resolved_ignores_spoofed_fee_rate_param" },
    Inv083BoundaryProfile { name: "index", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_attack_out_of_range_asset_index_rejected" },
    Inv083BoundaryProfile { name: "key", evidence: "tests/invariants/cu/inv_089_activation_reactivation_and_initialization_equivalence.rs#v16_program_reuse_rejects_every_zero_authority_before_mutation" },
    Inv083BoundaryProfile { name: "no-data", evidence: "tests/invariants/public_sbf/inv_022_instruction_decoding_and_schema_upgrade_safety.rs#host_instruction_canonical_corpus_roundtrips_and_rejects_truncation_or_trailing_bytes" },
    Inv083BoundaryProfile { name: "price", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_attack_mark_input_bounds_reject_atomically" },
    Inv083BoundaryProfile { name: "rate", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_attack_init_market_rejects_grief_config_without_burning_market_account" },
    Inv083BoundaryProfile { name: "ratio", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_attack_init_market_rejects_grief_config_without_burning_market_account" },
    Inv083BoundaryProfile { name: "replay", evidence: "tests/invariants/cu/inv_014_delayed_policy_and_policy_epoch_safety.rs#v16_control_sequences_accept_gaps_reject_replays_and_keep_lanes_independent" },
    Inv083BoundaryProfile { name: "scale", evidence: "tests/invariants/cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs#v16_program_hybrid_oracle_rejects_duplicate_or_malformed_leg_config" },
    Inv083BoundaryProfile { name: "shape", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_program_batch_decode_oversized_vectors_reject_before_allocation" },
    Inv083BoundaryProfile { name: "signed-quantity", evidence: "tests/invariants/cu/inv_083_boundary_completeness.rs#v16_attack_extreme_size_trade_rejected_no_panic" },
];

fn inv_083_boundary_profile(type_name: &str, field: &str, classification: &str) -> &'static str {
    if field == "-" {
        return "no-data";
    }
    match classification {
        "IDENTITY_BINDING" => return "identity",
        "REPLAY_GUARD" => return "replay",
        "AUTHENTICATED_TIME" => return "authenticated-time",
        "IGNORED_LEGACY" => return "ignored",
        "BOUNDED_WORK" => return "amount",
        _ => {}
    }

    if matches!(
        field,
        "new_pubkey"
            | "primary_mint"
            | "secondary_mint"
            | "insurance_authority"
            | "insurance_operator"
            | "backing_bucket_authority"
            | "oracle_authority"
            | "oracle_leg_feeds"
    ) {
        return "key";
    }
    if field == "observations" || field == "legs" {
        return "shape";
    }
    if field == "asset_index" || field == "domain" {
        return "index";
    }
    if matches!(field, "action" | "kind" | "side" | "enabled" | "invert") {
        return "enum";
    }
    if field == "oracle_leg_flags" {
        return "bitmask";
    }
    if field == "size_q" {
        return "signed-quantity";
    }
    if field == "expiry_slot" {
        return "expiry";
    }
    if field.contains("price")
        || matches!(
            field,
            "exec_price" | "limit_price" | "mark_e6" | "initial_mark_e6"
        )
    {
        return "price";
    }
    if field.ends_with("_bps") || field.contains("_bps_") {
        return "basis-points";
    }
    if matches!(field, "h_min" | "h_max") {
        return "ratio";
    }
    if field == "unit_scale" {
        return "scale";
    }
    if matches!(
        field,
        "max_portfolio_assets"
            | "oracle_leg_count"
            | "oracle_accounts"
            | "max_account_b_settlement_chunks"
            | "max_bankrupt_close_chunks"
    ) {
        return "count";
    }
    if field.ends_with("_slots") || field.ends_with("_secs") {
        return "duration";
    }
    if matches!(
        field,
        "maintenance_fee_per_slot" | "max_abs_funding_e9_per_slot"
    ) {
        return "rate";
    }
    if field == "mark_min_fee"
        || field == "reduce_q"
        || field == "min_init_fee"
        || field == "max_init_fee"
        || field == "optional_deposit"
        || field.contains("amount")
        || field.ends_with("_cap")
        || field.ends_with("_abs")
        || field.ends_with("_req")
        || field.ends_with("_atoms")
    {
        return "amount";
    }

    panic!("unclassified INV-083 boundary field {type_name}.{field} ({classification})");
}

fn inv_083_source_contains_test(source: &str, test_function: &str) -> bool {
    let top_level = format!("#[test]\nfn {test_function}(");
    let indented = format!("#[test]\n    fn {test_function}(");
    source.contains(&top_level) || source.contains(&indented)
}

#[test]
fn v16_program_every_public_input_field_has_a_boundary_profile_and_executable_witness() {
    use std::collections::{BTreeMap, BTreeSet};

    const HEADER: &str = "type\tfields\tclassification\tevidence";
    const EXPECTED_FIELD_COUNT: usize = 234;
    const EXPECTED_TYPE_COUNT: usize = 52;

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let profile_names = INV_083_BOUNDARY_PROFILES
        .iter()
        .map(|profile| profile.name)
        .collect::<BTreeSet<_>>();
    assert_eq!(profile_names.len(), INV_083_BOUNDARY_PROFILES.len());

    let mut saw_header = false;
    let mut subjects = BTreeSet::new();
    let mut types = BTreeSet::new();
    let mut profile_counts = BTreeMap::<&str, usize>::new();
    let mut field_evidence_sources = BTreeMap::<String, String>::new();
    for (line_index, line) in INV_083_CALLER_INPUT_ROSTER.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !saw_header {
            assert_eq!(line, HEADER, "INV-023 caller-input roster header changed");
            saw_header = true;
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(
            columns.len(),
            4,
            "line {}: malformed caller-input roster row",
            line_index + 1
        );
        let type_name = columns[0];
        let classification = columns[2];
        let (evidence_file, evidence_test) = columns[3]
            .split_once('#')
            .expect("field evidence is path#test");
        let evidence_source = field_evidence_sources
            .entry(evidence_file.to_owned())
            .or_insert_with(|| {
                std::fs::read_to_string(manifest.join(evidence_file))
                    .unwrap_or_else(|error| panic!("read {evidence_file}: {error}"))
            });
        assert!(
            evidence_source.contains(&format!("fn {evidence_test}(")),
            "{type_name} boundary owner is missing executable evidence {}",
            columns[3]
        );
        types.insert(type_name);
        for field in columns[1].split(',') {
            let profile = inv_083_boundary_profile(type_name, field, classification);
            assert!(
                profile_names.contains(profile),
                "{type_name}.{field} maps to unknown boundary profile {profile}"
            );
            assert!(
                subjects.insert(format!("{type_name}.{field}")),
                "duplicate boundary subject {type_name}.{field}"
            );
            *profile_counts.entry(profile).or_default() += 1;
        }
    }
    assert!(saw_header);
    assert_eq!(
        subjects.len(),
        EXPECTED_FIELD_COUNT,
        "public input field count changed; classify every new or removed field deliberately"
    );
    assert_eq!(
        types.len(),
        EXPECTED_TYPE_COUNT,
        "public input type count changed; classify every new or removed type deliberately"
    );
    assert_eq!(
        profile_counts.keys().copied().collect::<BTreeSet<_>>(),
        profile_names,
        "every boundary profile must own at least one current public input field"
    );
    let expected_profile_counts = BTreeMap::from([
        ("amount", 23),
        ("authenticated-time", 12),
        ("basis-points", 21),
        ("bitmask", 1),
        ("count", 5),
        ("duration", 9),
        ("enum", 5),
        ("expiry", 1),
        ("identity", 76),
        ("ignored", 1),
        ("index", 24),
        ("key", 9),
        ("no-data", 3),
        ("price", 12),
        ("rate", 2),
        ("ratio", 2),
        ("replay", 20),
        ("scale", 1),
        ("shape", 3),
        ("signed-quantity", 4),
    ]);
    assert_eq!(
        profile_counts, expected_profile_counts,
        "public input boundary profile changed; review every added, removed, or reclassified field"
    );

    for profile in INV_083_BOUNDARY_PROFILES {
        let (owner_file, test_function) = profile
            .evidence
            .split_once('#')
            .expect("boundary profile evidence is path#test");
        let source = std::fs::read_to_string(manifest.join(owner_file))
            .unwrap_or_else(|error| panic!("read {owner_file}: {error}"));
        assert!(
            inv_083_source_contains_test(&source, test_function),
            "boundary profile {} lacks executable evidence {}",
            profile.name,
            profile.evidence
        );
    }
}

#[test]
fn v16_program_boundary_roster_maps_required_classes_to_owned_tests() {
    use std::collections::{BTreeMap, BTreeSet};

    let mut covered = BTreeMap::new();
    for &class in INV_083_REQUIRED_BOUNDARY_CLASSES {
        covered.insert(class, 0usize);
    }

    let mut lines = INV_083_BOUNDARY_ROSTER.lines().enumerate();
    let (_, header) = lines
        .next()
        .expect("INV-083 boundary roster must not be empty");
    assert_eq!(
        header, INV_083_BOUNDARY_ROSTER_HEADER,
        "INV-083 boundary roster header changed; update the parser deliberately"
    );

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut seen_rows = BTreeSet::new();
    let mut row_count = 0usize;

    for (line_index, line) in lines {
        let line_no = line_index + 1;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }

        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(
            columns.len(),
            6,
            "line {line_no}: expected 6 tab-separated columns"
        );
        let class = columns[0];
        let invariant = columns[1];
        let owner_file = columns[2];
        let test_function = columns[3];
        let boundary_value = columns[4];
        let coverage_note = columns[5];

        assert!(
            columns.iter().all(|column| !column.trim().is_empty()),
            "line {line_no}: roster columns must be non-empty"
        );
        assert!(
            covered.contains_key(class),
            "line {line_no}: unexpected INV-083 boundary class {class:?}"
        );
        assert!(
            invariant.len() == "INV-000".len()
                && invariant.starts_with("INV-")
                && invariant[4..].chars().all(|digit| digit.is_ascii_digit()),
            "line {line_no}: invalid invariant id {invariant:?}"
        );
        assert!(
            test_function.starts_with("v16_")
                && test_function
                    .chars()
                    .all(|ch| ch == '_' || ch.is_ascii_alphanumeric()),
            "line {line_no}: invalid test function name {test_function:?}"
        );
        assert_ne!(
            test_function, "v16_program_boundary_roster_maps_required_classes_to_owned_tests",
            "line {line_no}: the roster cannot satisfy itself"
        );
        assert!(
            !boundary_value.contains('\n') && !coverage_note.contains('\n'),
            "line {line_no}: roster fields must stay single-line"
        );

        let owner_path = std::path::Path::new(owner_file);
        assert!(
            owner_path.is_relative()
                && owner_file.starts_with("tests/invariants/")
                && owner_file.ends_with(".rs")
                && owner_file
                    .split('/')
                    .all(|part| !part.is_empty() && part != ".."),
            "line {line_no}: owner file must be a relative tests/invariants/*.rs path: {owner_file:?}"
        );
        let owner_token = format!("inv_{}", invariant[4..].to_ascii_lowercase());
        assert!(
            owner_file.contains(&owner_token),
            "line {line_no}: {owner_file:?} does not match {invariant}"
        );

        let owner_source_path = manifest_dir.join(owner_path);
        let owner_source = std::fs::read_to_string(&owner_source_path).unwrap_or_else(|error| {
            panic!("line {line_no}: failed to read {owner_file:?}: {error}")
        });
        assert!(
            owner_source.contains(&format!("//! {invariant} -")),
            "line {line_no}: {owner_file:?} is not owned by {invariant}"
        );
        assert!(
            inv_083_source_contains_test(&owner_source, test_function),
            "line {line_no}: {owner_file:?} does not contain #[test] fn {test_function}"
        );

        assert!(
            seen_rows.insert(line.to_string()),
            "line {line_no}: duplicate roster row"
        );
        *covered.get_mut(class).expect("class checked above") += 1;
        row_count += 1;
    }

    assert!(
        row_count >= INV_083_REQUIRED_BOUNDARY_CLASSES.len(),
        "INV-083 boundary roster has fewer rows than required classes"
    );
    for (class, count) in covered {
        assert!(count > 0, "INV-083 boundary roster is missing {class}");
    }
}

#[test]
fn v16_program_batch_decode_oversized_vectors_reject_before_allocation() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    let taker = Keypair::new();
    let maker = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let maker_account = env.create_portfolio(&maker);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let maker_before = env.svm.get_account(&maker_account).unwrap();

    let no_cpi_legs: Vec<BatchTradeLeg> = (0..u16::from(u8::MAX))
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let no_cpi = env.send(
        env.batch_trade_no_cpi_ix(taker_account, maker_account, no_cpi_legs),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(maker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(maker_account, false),
        ],
        &[&taker, &maker],
    );
    let no_cpi_err = no_cpi.expect_err("oversized BatchTradeNoCpi must reject");
    assert!(no_cpi_err.contains("InvalidInstructionData"));
    assert!(
        !no_cpi_err.contains("ProgramFailedToComplete")
            && !no_cpi_err.contains("memory allocation failed"),
        "oversized BatchTradeNoCpi must not panic the program: {no_cpi_err}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&maker_account).unwrap(), maker_before);

    let cpi_legs: Vec<BatchTradeCpiLeg> = (0..u16::from(u8::MAX))
        .map(|asset_index| BatchTradeCpiLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let cpi = env.send(
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: env.portfolio_id(taker_account),
            account_a_position_epoch: 0,
            account_b_portfolio_id: env.portfolio_id(maker_account),
            account_b_position_epoch: 0,
            legs: cpi_legs,
        },
        vec![],
        &[],
    );
    let cpi_err = cpi.expect_err("oversized BatchTradeCpi must reject");
    assert!(cpi_err.contains("InvalidInstructionData"));
    assert!(
        !cpi_err.contains("ProgramFailedToComplete")
            && !cpi_err.contains("memory allocation failed"),
        "oversized BatchTradeCpi must not panic the program: {cpi_err}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&maker_account).unwrap(), maker_before);
}

// security.md sweep — numerical boundary (#37 i128::MIN negation / #38 wide overflow):
// extreme trade sizes must be rejected cleanly (no panic, no OI, no value movement).
#[test]
fn v16_attack_extreme_size_trade_rejected_no_panic() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    for sz in [i128::MIN, i128::MAX, i128::MIN + 1] {
        env.svm.expire_blockhash();
        let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, sz, 100, 0);
        assert!(r.is_err(), "extreme size {} must be rejected cleanly", sz);
    }
    let (_, g) = env.market_state();
    assert_eq!(
        g.assets[0].oi_eff_long_q, 0,
        "no OI from rejected extreme-size trades"
    );
    assert_eq!(g.c_tot, 2_000_000, "no capital moved");
}

// security.md sweep — asset_index bounds (#37/#39): an out-of-range asset_index on any instruction
// must reject cleanly (no OOB access / panic / state corruption).
#[test]
fn v16_attack_out_of_range_asset_index_rejected() {
    let mut env = V16CuEnv::new(); // 1 asset (index 0 valid)
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();
    for bad in [1u16, 7, 255, 9999, u16::MAX] {
        // trade on a bad asset index
        env.svm.expire_blockhash();
        let rt = env.try_trade_asset_with_cu(bad, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
        assert!(
            rt.is_err(),
            "trade on out-of-range asset_index {} must reject",
            bad
        );
        // crank on a bad asset index
        env.svm.expire_blockhash();
        let rc = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(bad),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(pa, false),
            ],
            &[],
        );
        assert!(
            rc.is_err(),
            "crank on out-of-range asset_index {} must reject",
            bad
        );
        // push auth mark on a bad asset index (admin)
        env.svm.expire_blockhash();
        let rm = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::PushAuthMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: bad,
                now_slot: 1,
                mark_e6: 100,
                authority_epoch: 0,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        );
        assert!(
            rm.is_err(),
            "push auth mark on out-of-range asset_index {} must reject",
            bad
        );
    }
    // no corruption from any rejected OOB attempt.
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI created");
}

// security.md sweep — domain-indexed public calls must reject domains outside the configured market
// slots before touching accounting, ledgers, or SPL custody. On a one-asset market, domains 0/1 are
// valid and domain 2 is out of range; a real market authority with valid token accounts still cannot
// write or move funds through that phantom domain.
#[test]
fn v16_attack_domain_indexed_calls_reject_out_of_range_atomically() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    const BAD_DOMAIN: u16 = 2;

    env.top_up_insurance(1_000);
    env.top_up_backing_bucket_with_cu(0, 1_000, 10_000);
    let (_, funded) = env.market_state();
    assert_eq!(
        funded.insurance_domain_budget.len(),
        2,
        "one-asset market has exactly domains 0 and 1"
    );
    assert!(
        funded.vault >= 2_000,
        "setup leaves real withdrawable vault balance"
    );

    let insurance_src = env.token_account_for_mint(env.mint, admin.pubkey(), 123);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let source_before = env.svm.get_account(&insurance_src).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let topup_ins = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: BAD_DOMAIN,
            amount: 123,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(insurance_src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        topup_ins.is_err(),
        "phantom insurance domain top-up must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&insurance_src).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let backing_src = env.token_account_for_mint(env.mint, admin.pubkey(), 456);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let source_before = env.svm.get_account(&backing_src).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let topup_backing = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain: BAD_DOMAIN,
            backing_fee_bps: 0,
            insurance_share_bps: 0,
            amount: 456,
            expiry_slot: 10_000,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        topup_backing.is_err(),
        "phantom backing bucket top-up must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&backing_src).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let insurance_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let dest_before = env.svm.get_account(&insurance_dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let withdraw_ins = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: BAD_DOMAIN as u16,
            amount: 1,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(insurance_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_ins.is_err(),
        "phantom insurance asset withdraw must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&insurance_dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let backing_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let dest_before = env.svm.get_account(&backing_dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let withdraw_backing = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: BAD_DOMAIN,
            market_id: 0,
            authority_epoch: 0,
            amount: 1,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_backing.is_err(),
        "phantom backing bucket withdraw must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&backing_dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let earnings_ledger = env.backing_domain_ledger_account();
    let earnings_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&earnings_ledger).unwrap();
    let dest_before = env.svm.get_account(&earnings_dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let withdraw_earnings = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: BAD_DOMAIN,
            market_id: 0,
            authority_epoch: 0,
            amount: 1,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(earnings_ledger, false),
            AccountMeta::new(earnings_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_earnings.is_err(),
        "phantom backing earnings withdraw must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&earnings_ledger).unwrap(),
        ledger_before
    );
    assert_eq!(env.svm.get_account(&earnings_dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let sync_ledger = env.backing_domain_ledger_account();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&sync_ledger).unwrap();
    env.svm.expire_blockhash();
    let sync = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncBackingDomainLedger { domain: BAD_DOMAIN },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(sync_ledger, false),
        ],
        &[&admin],
    );
    assert!(sync.is_err(), "phantom backing ledger sync must reject");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&sync_ledger).unwrap(), ledger_before);

    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let fee_policy = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBackingFeePolicy {
            market_id: 0,
            policy_sequence: u64::MAX,
            domain: BAD_DOMAIN,
            fee_bps: 77,
            insurance_share_bps: 5_000,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        fee_policy.is_err(),
        "phantom backing fee policy update must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);

    let (_, g) = env.market_state();
    assert_eq!(
        g.insurance_domain_budget.len(),
        2,
        "no phantom domain was appended"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == canonical vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — large-amount deposit boundary + TVL cap (#37): the vault is capped at
// MAX_VAULT_TVL (overflow prevention). A deposit above the cap must reject; a large deposit just below
// it must credit exactly (no truncation/wraparound in the u128 aggregates) and round-trip exactly.
#[test]
fn v16_attack_large_amount_deposit_withdraw_exact() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    const MAX_TVL: u128 = 10_000_000_000_000_000;
    // over-cap deposit -> reject.
    let over = MAX_TVL + 1;
    let src_over = env.token_account_for_mint(env.mint, owner.pubkey(), over as u64);
    env.svm.expire_blockhash();
    let r = env.send(
        env.deposit_ix(p, over),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(src_over, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "deposit above MAX_VAULT_TVL must reject (overflow/abuse cap)"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        0,
        "no capital credited on over-cap deposit"
    );

    // large below-cap deposit -> exact credit, no overflow.
    let big: u128 = MAX_TVL - 7;
    env.deposit(&owner, p, big);
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        big,
        "capital credited exactly (no overflow/truncation)"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.c_tot, big, "c_tot == the large deposit");
    assert_eq!(g1.vault, big, "vault == the large deposit");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    // withdraw it all back -> exact.
    let (dest, _) = env.withdraw_with_cu(&owner, p, big);
    assert_eq!(
        env.token_amount(dest) as u128,
        big,
        "withdrew exactly the large amount"
    );
    let (_, g2) = env.market_state();
    assert_eq!(g2.c_tot, 0, "c_tot back to 0");
    assert_eq!(g2.vault, 0, "vault drained exactly");
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation");
}

// security.md sweep - mark input bounds (#37/#39): the mark authority controls settlement input, but
// invalid marks or an EWMA halflife of zero must not even transiently rewrite the oracle profile. Existing
// conservation tests cover "no panic"; this asserts exact market rollback for every public mark entrypoint.
#[test]
fn v16_attack_mark_input_bounds_reject_atomically() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let over_max = percolator::MAX_ORACLE_PRICE + 1;
    env.svm.warp_to_slot(1);

    let reject_unchanged = |env: &mut V16CuEnv, ix: ProgInstruction, label: &str| {
        let before = env.svm.get_account(&env.market).unwrap();
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&admin],
        );
        assert!(rejected.is_err(), "{label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before,
            "{label} must leave the market byte-identical"
        );
    };

    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 0,
            authority_epoch: 0,
        },
        "ConfigureAuthMark zero initial mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: over_max,
            authority_epoch: 0,
        },
        "ConfigureAuthMark over-max initial mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 0,
            mark_ewma_halflife_slots: 4,
            mark_min_fee: 0,
            authority_epoch: 0,
        },
        "ConfigureEwmaMark zero initial mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: over_max,
            mark_ewma_halflife_slots: 4,
            mark_min_fee: 0,
            authority_epoch: 0,
        },
        "ConfigureEwmaMark over-max initial mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 0,
            mark_min_fee: 0,
            authority_epoch: 0,
        },
        "ConfigureEwmaMark zero halflife",
    );

    env.svm.expire_blockhash();
    let ewma = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 4,
            mark_min_fee: 0,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        ewma.is_ok(),
        "valid EWMA configuration should succeed: {ewma:?}"
    );
    let (cfg_ewma, _) = env.market_state();
    assert_eq!(
        cfg_ewma.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_EWMA_MARK
    );
    assert_eq!(cfg_ewma.mark_ewma_e6, 100);
    assert_eq!(cfg_ewma.mark_ewma_halflife_slots, 4);

    env.svm.warp_to_slot(2);
    reject_unchanged(
        &mut env,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 0,
            authority_epoch: 0,
        },
        "PushEwmaMark zero mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: over_max,
            authority_epoch: 0,
        },
        "PushEwmaMark over-max mark",
    );

    env.svm.expire_blockhash();
    let valid_ewma_push = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 120,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        valid_ewma_push.is_ok(),
        "valid EWMA push remains live after rejected bounds probes: {valid_ewma_push:?}"
    );
    assert_eq!(env.market_state().0.mark_ewma_last_slot, 2);

    env.svm.warp_to_slot(3);
    env.svm.expire_blockhash();
    let auth = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: 3,
            asset_index: 0,
            now_slot: 3,
            initial_mark_e6: 200,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        auth.is_ok(),
        "valid AuthMark configuration should succeed: {auth:?}"
    );

    env.svm.warp_to_slot(4);
    reject_unchanged(
        &mut env,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 4,
            mark_e6: 0,
            authority_epoch: 0,
        },
        "PushAuthMark zero mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 4,
            mark_e6: over_max,
            authority_epoch: 0,
        },
        "PushAuthMark over-max mark",
    );

    env.svm.expire_blockhash();
    let valid_auth_push = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: 4,
            asset_index: 0,
            now_slot: 4,
            mark_e6: 220,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        valid_auth_push.is_ok(),
        "valid AuthMark push remains live after rejected bounds probes: {valid_auth_push:?}"
    );
    let (cfg_auth, _) = env.market_state();
    assert_eq!(
        cfg_auth.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_AUTH_MARK
    );
    assert_eq!(cfg_auth.mark_ewma_e6, 220);
    assert_eq!(cfg_auth.oracle_target_price_e6, 220);
    assert_eq!(cfg_auth.mark_ewma_last_slot, 4);
}

// security.md sweep - sparse append DoS: permissionless activation may append exactly the next
// configured slot, or reuse a retired slot, but it must not accept sparse jumps. Otherwise a stranger
// could force large market-account growth or create holes in the asset table.
#[test]
fn v16_attack_permissionless_sparse_append_indices_rejected_without_realloc_or_fee() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);
    env.svm.warp_to_slot(1);

    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let (cfg_before, group_before) = env.market_state();
    assert_eq!(
        group_before.config.max_market_slots, 1,
        "starts as a one-asset market"
    );
    assert_eq!(
        cfg_before.free_market_slot_count, 0,
        "no retired slots are reusable"
    );
    let activation_market_id = group_before.next_market_id;

    for bad_index in [2u16, 7, u16::MAX] {
        let source = env.token_account(creator.pubkey(), FEE as u64);
        let source_before = env.svm.get_account(&source).unwrap();
        env.svm.expire_blockhash();
        let rejected = env.send(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index: bad_index,
                market_id: activation_market_id,
                authority_epoch: 0,
                now_slot: 1,
                initial_price: 100,
                max_init_fee: u128::MAX,
                insurance_authority: creator.pubkey().to_bytes(),
                insurance_operator: creator.pubkey().to_bytes(),
                backing_bucket_authority: creator.pubkey().to_bytes(),
                oracle_authority: creator.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&creator],
        );
        assert!(
            rejected.is_err(),
            "permissionless sparse append at index {bad_index} must reject"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "rejected sparse append at index {bad_index} did not realloc or mutate the market"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "rejected sparse append at index {bad_index} did not move vault tokens"
        );
        assert_eq!(
            env.svm.get_account(&source).unwrap(),
            source_before,
            "rejected sparse append at index {bad_index} did not debit the creator"
        );
        let (_, rejected_group) = env.market_state();
        assert_eq!(
            rejected_group.config.max_market_slots, group_before.config.max_market_slots,
            "rejected sparse append at index {bad_index} did not advance configured slots"
        );
        assert_eq!(
            rejected_group.insurance_domain_budget, group_before.insurance_domain_budget,
            "rejected sparse append at index {bad_index} did not credit any domain budget"
        );
    }

    let valid_source = env.token_account(creator.pubkey(), FEE as u64);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            market_id: activation_market_id,
            authority_epoch: 0,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(valid_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    )
    .expect("contiguous permissionless append still succeeds after sparse attempts");
    let (_, valid_group) = env.market_state();
    assert_eq!(
        valid_group.config.max_market_slots, 2,
        "valid append advances exactly one slot"
    );
    assert_eq!(valid_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(
        env.token_amount(valid_source),
        0,
        "valid append pulls only the real fee"
    );
}

// Fresh InitMarket is a public bootstrap boundary: an attacker or misconfigured launcher should not be
// able to burn a newly allocated market account into a half-written, unusable slab with grief params.
#[test]
fn v16_attack_init_market_rejects_grief_config_without_burning_market_account() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let valid = V16CuMarketParams::default();
    let market_len = state::market_account_len_for_capacity(valid.max_portfolio_assets as usize)
        .expect("market len");
    let portfolio_len =
        state::portfolio_account_len_for_market_slots(valid.max_portfolio_assets as usize)
            .expect("portfolio len");

    macro_rules! invalid_case {
        ($label:literal, $field:ident, $value:expr) => {{
            let mut params = V16CuMarketParams::default();
            params.$field = $value;
            ($label, params)
        }};
    }

    let mut h_min_above_h_max = V16CuMarketParams::default();
    h_min_above_h_max.h_min = 11;
    h_min_above_h_max.h_max = 10;
    let mut equal_nonzero_requirements = V16CuMarketParams::default();
    equal_nonzero_requirements.min_nonzero_mm_req = 2;
    equal_nonzero_requirements.min_nonzero_im_req = 2;
    let mut maintenance_above_initial = V16CuMarketParams::default();
    maintenance_above_initial.maintenance_margin_bps = 10_000;
    maintenance_above_initial.initial_margin_bps = 9_999;
    let mut fee_floor_above_max = V16CuMarketParams::default();
    fee_floor_above_max.max_trading_fee_bps = 99;
    fee_floor_above_max.trade_fee_base_bps = 100;
    let mut minimum_liquidation_above_cap = V16CuMarketParams::default();
    minimum_liquidation_above_cap.min_liquidation_abs = 1;
    minimum_liquidation_above_cap.liquidation_fee_cap = 0;
    let mut funding_lifetime_below_accrual = V16CuMarketParams::default();
    funding_lifetime_below_accrual.max_accrual_dt_slots = 2;
    funding_lifetime_below_accrual.min_funding_lifetime_slots = 1;

    let invalid_cases = vec![
        invalid_case!("zero portfolio cap", max_portfolio_assets, 0),
        invalid_case!(
            "portfolio cap above wrapper limit",
            max_portfolio_assets,
            percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS + 1
        ),
        invalid_case!("zero h_max", h_max, 0),
        ("h_min above h_max", h_min_above_h_max),
        invalid_case!("h_max above bound scale", h_max, (BOUND_SCALE + 1) as u64),
        invalid_case!("zero initial oracle price", initial_price, 0),
        invalid_case!(
            "initial oracle price above max",
            initial_price,
            percolator::MAX_ORACLE_PRICE + 1
        ),
        invalid_case!("zero maintenance requirement", min_nonzero_mm_req, 0),
        (
            "maintenance and initial requirements equal",
            equal_nonzero_requirements,
        ),
        (
            "maintenance margin above initial margin",
            maintenance_above_initial,
        ),
        invalid_case!("initial margin above maximum", initial_margin_bps, 10_001),
        invalid_case!(
            "maximum trading fee above maximum",
            max_trading_fee_bps,
            10_001
        ),
        ("trade fee floor above maximum fee", fee_floor_above_max),
        invalid_case!("liquidation fee above maximum", liquidation_fee_bps, 10_001),
        (
            "minimum liquidation above fee cap",
            minimum_liquidation_above_cap,
        ),
        invalid_case!(
            "liquidation fee cap above protocol maximum",
            liquidation_fee_cap,
            percolator::MAX_PROTOCOL_FEE_ABS + 1
        ),
        invalid_case!("zero price movement bound", max_price_move_bps_per_slot, 0),
        invalid_case!("zero accrual horizon", max_accrual_dt_slots, 0),
        invalid_case!(
            "funding rate above maximum",
            max_abs_funding_e9_per_slot,
            10_001
        ),
        (
            "funding lifetime below accrual horizon",
            funding_lifetime_below_accrual,
        ),
        invalid_case!(
            "zero account B chunk count",
            max_account_b_settlement_chunks,
            0
        ),
        invalid_case!(
            "zero bankrupt close chunk count",
            max_bankrupt_close_chunks,
            0
        ),
        invalid_case!(
            "zero bankrupt close lifetime",
            max_bankrupt_close_lifetime_slots,
            0
        ),
        invalid_case!("zero public B chunk", public_b_chunk_atoms, 0),
        invalid_case!(
            "maintenance fee above protocol cap",
            maintenance_fee_per_slot,
            percolator::MAX_PROTOCOL_FEE_ABS + 1
        ),
    ];

    for (label, params) in invalid_cases {
        let market = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &market,
            market_len,
            env.program_id,
        );
        let market_before = env
            .svm
            .get_account(&market.pubkey())
            .expect("market account");

        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            init_market_instruction(&params),
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market.pubkey(), false),
                AccountMeta::new_readonly(env.mint, false),
            ],
            &[&admin],
        );
        assert!(
            rejected.is_err(),
            "{label}: hostile InitMarket config must reject"
        );
        assert_eq!(
            env.svm
                .get_account(&market.pubkey())
                .expect("market account"),
            market_before,
            "{label}: rejected InitMarket must not dirty the freshly allocated market account"
        );

        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            init_market_instruction(&valid),
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market.pubkey(), false),
                AccountMeta::new_readonly(env.mint, false),
            ],
            &[&admin],
        )
        .expect("valid InitMarket after rejected grief config");
        let market_after_valid = env
            .svm
            .get_account(&market.pubkey())
            .expect("market account");
        let (cfg, group) = state::read_market(&market_after_valid.data).expect("valid market");
        assert_eq!(
            cfg.marketauth,
            admin.pubkey().to_bytes(),
            "{label}: valid retry keeps the real initializer as market authority"
        );
        assert_eq!(
            cfg.collateral_mint,
            env.mint.to_bytes(),
            "{label}: valid retry pins the intended collateral mint"
        );
        assert_eq!(
            group.assets[0].effective_price, valid.initial_price,
            "{label}: valid retry initializes a sane base oracle"
        );

        let owner = Keypair::new();
        env.ensure_signer_account(owner.pubkey());
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio,
            portfolio_len,
            env.program_id,
        );
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(market.pubkey(), false),
                AccountMeta::new(portfolio.pubkey(), false),
            ],
            &[&owner],
        )
        .expect("portfolio init after valid market retry");
        let portfolio_account = env
            .svm
            .get_account(&portfolio.pubkey())
            .expect("portfolio account");
        state::read_portfolio(&portfolio_account.data).expect("valid portfolio after retry");
    }
}
