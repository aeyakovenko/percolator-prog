//! INV-010 - Out-of-order safety.
//!
//! Normative obligation: retained public requests that land out of order either
//! reject atomically or remain inside every affected signer’s latest authority
//! and economic bounds.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM witness exercises an
//! LP-signed retained matcher-enable request. After the LP revokes matcher
//! authority, CPI trade attempts and the stale retained enable must reject with
//! exact rollback and unchanged matcher sequence. A fresh enable then lands,
//! CPI open/close succeeds, both parties withdraw, and SPL supply is conserved.
//!
//! The source-composition gate closes arbitrary sequence length by induction rather than by
//! enumerating factorially larger schedules. The deployed one-step sequence theorem is joined to
//! every retained-family disposition, every delayed-control lane, identity/authority binding,
//! exact value attribution, and instruction-error rollback. The existing public `2!`, `3!`, and
//! 144-cell products provide noncommuting and terminal witnesses for that induction.
//!
//! Guarantee boundary: aggregate slippage, fee, and expiry terms absent from the current request
//! schema remain design gaps under INV-008/009/011/059. The composition proves ordering safety for
//! every economic bound the current schema actually carries.

#[derive(Clone, Copy)]
struct Inv010CompositionOwner {
    obligation: &'static str,
    path: &'static str,
    test: &'static str,
}

fn inv010_source_defines_test(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    let mut saw_test = false;
    for line in source.lines() {
        let line = line.trim();
        if line == "#[test]" {
            saw_test = true;
        } else if line.starts_with("fn ") {
            if saw_test
                && line
                    .strip_prefix(&marker)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            saw_test = false;
        } else if saw_test && !line.is_empty() && !line.starts_with('#') {
            saw_test = false;
        }
    }
    false
}

fn inv010_evidence_parts(evidence: &str) -> (&str, &str) {
    let evidence = evidence
        .split_once(':')
        .map_or(evidence, |(_, evidence)| evidence);
    evidence
        .split_once('#')
        .unwrap_or_else(|| panic!("history evidence must be path#test: {evidence}"))
}

#[test]
fn v16_program_every_public_route_has_an_explicit_history_relation() {
    const CLASS_RELATIONS: &[(&str, &str)] = &[
        ("initialization", "FirstLandedSerial"),
        ("linear_amount", "ExactEconomic"),
        ("state_progress", "SignedEnvelope"),
        ("trade_partition", "ConservativeBound"),
        ("terminal_administration", "FirstLandedSerial"),
        ("legacy_insurance_split", "ConservativeBound"),
        ("resolved_entitlement", "TerminalEntitlement"),
        ("claim_conversion", "ConservativeBound"),
        ("authority_serialization", "FirstLandedSerial"),
        ("control_supersession", "FirstLandedSerial"),
        ("lifecycle_serialization", "FirstLandedSerial"),
        ("close_episode", "FirstLandedSerial"),
        ("recovery_episode", "FirstLandedSerial"),
        ("reduction_partition", "ConservativeBound"),
        ("fee_cadence", "ConservativeBound"),
        ("observation_ledger", "SignedEnvelope"),
        ("insurance_withdrawal", "ExactEconomic"),
        ("authorized_configuration", "SignedEnvelope"),
    ];

    crate::assert_certified_engine_pin("INV-010 whole-route history relation census");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let class_relations = CLASS_RELATIONS
        .iter()
        .copied()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(class_relations.len(), CLASS_RELATIONS.len());

    let mut public_routes = std::collections::BTreeMap::new();
    for line in include_str!("../public_instruction_coverage.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields = line.splitn(5, '\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 5, "malformed public-route row: {line}");
        let tag = fields[0].parse::<u8>().expect("numeric public tag");
        assert!(
            public_routes.insert(tag, (fields[1], fields[2])).is_none(),
            "duplicate public tag {tag}"
        );
    }
    assert_eq!(public_routes.len(), 49, "public instruction census drift");

    let mut dispositions = std::collections::BTreeMap::new();
    let mut used_classes = std::collections::BTreeSet::new();
    let mut used_relations = std::collections::BTreeSet::new();
    let mut source_cache = std::collections::BTreeMap::<String, String>::new();
    for line in include_str!("../inv_010_history_relation_dispositions.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields = line.splitn(6, '\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 6, "malformed history row: {line}");
        let tag = fields[0].parse::<u8>().expect("numeric history tag");
        let variant = fields[1];
        let history_class = fields[2];
        let relation = fields[3];
        let evidence = fields[4];
        assert!(
            !fields[5].trim().is_empty(),
            "empty history boundary for {variant}"
        );
        assert_eq!(
            public_routes.get(&tag).map(|route| route.0),
            Some(variant),
            "history disposition must bind the production tag and variant"
        );
        assert_eq!(
            class_relations.get(history_class),
            Some(&relation),
            "invalid history class/relation pair for {variant}"
        );
        assert!(
            dispositions.insert(tag, variant).is_none(),
            "duplicate history disposition for tag {tag}"
        );
        used_classes.insert(history_class);
        used_relations.insert(relation);

        let (path, function) = inv010_evidence_parts(evidence);
        assert!(path.starts_with("tests/invariants/"));
        let source = source_cache.entry(path.to_owned()).or_insert_with(|| {
            std::fs::read_to_string(root.join(path))
                .unwrap_or_else(|error| panic!("read history evidence {path}: {error}"))
        });
        assert!(
            inv010_source_defines_test(source, function),
            "history row {variant} lacks executable evidence {evidence}"
        );

        let public_evidence = public_routes.get(&tag).unwrap().1;
        let (public_path, public_function) = inv010_evidence_parts(public_evidence);
        let public_source = source_cache
            .entry(public_path.to_owned())
            .or_insert_with(|| {
                std::fs::read_to_string(root.join(public_path))
                    .unwrap_or_else(|error| panic!("read public evidence {public_path}: {error}"))
            });
        assert!(
            inv010_source_defines_test(public_source, public_function),
            "history row {variant} lacks executable public-route evidence {public_evidence}"
        );
    }

    assert_eq!(dispositions.len(), 49, "history disposition census drift");
    assert_eq!(
        dispositions
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        public_routes.keys().copied().collect(),
        "every public route needs exactly one history relation"
    );
    assert_eq!(
        used_classes,
        class_relations.keys().copied().collect(),
        "unused history classes hide stale proof-equivalence categories"
    );
    assert_eq!(
        used_relations,
        [
            "ConservativeBound",
            "ExactEconomic",
            "FirstLandedSerial",
            "SignedEnvelope",
            "TerminalEntitlement",
        ]
        .into_iter()
        .collect(),
        "history relation vocabulary drift"
    );

    for (path, theorem) in [
        (
            "tests/invariants/kani/inv_024_attributed_quote_value_conservation.rs",
            "kani_inv024_entitlement_envelope_is_inductive_over_arbitrary_history_step",
        ),
        (
            "tests/invariants/kani/inv_025_exact_stock_reconciliation.rs",
            "kani_inv025_observation_ledger_net_identity_is_history_inductive",
        ),
    ] {
        let proof = std::fs::read_to_string(root.join(path))
            .unwrap_or_else(|error| panic!("read history theorem {path}: {error}"));
        assert!(proof.contains("#[kani::proof]"));
        assert!(
            proof.contains(&format!("fn {theorem}(")),
            "missing history theorem {path}#{theorem}"
        );
    }
}

#[test]
fn v16_program_out_of_order_induction_composition_is_source_complete() {
    const OWNERS: &[Inv010CompositionOwner] = &[
        Inv010CompositionOwner {
            obligation: "all retained operation families",
            path: "tests/invariants/cu/inv_008_intent_uniqueness_and_bounded_replay.rs",
            test: "v16_public_replay_disposition_roster_is_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "every public route has an explicit history relation",
            path: "tests/invariants/cu/inv_010_out_of_order_safety.rs",
            test: "v16_program_every_public_route_has_an_explicit_history_relation",
        },
        Inv010CompositionOwner {
            obligation: "all delayed policy and observation controls",
            path: "tests/invariants/public_sbf/inv_014_delayed_policy_and_policy_epoch_safety.rs",
            test: "v16_program_delayed_control_matrix_is_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "asset generation",
            path: "tests/invariants/cu/inv_002_asset_generation_binding.rs",
            test: "v16_program_asset_generation_field_and_guard_roster_is_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "portfolio incarnation",
            path: "tests/invariants/cu/inv_003_portfolio_incarnation_binding.rs",
            test: "v16_program_retained_portfolio_binding_roster_is_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "position episode",
            path: "tests/invariants/cu/inv_004_position_episode_binding.rs",
            test: "v16_program_retained_position_binding_and_writer_rosters_are_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "authority incarnation",
            path: "tests/invariants/cu/inv_005_authority_incarnation_binding.rs",
            test: "v16_program_configured_authority_route_dispositions_are_source_complete",
        },
        Inv010CompositionOwner {
            obligation: "attributed successful value delta",
            path: "tests/invariants/cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs",
            test: "v16_primary_quote_routes_match_actual_spl_and_internal_accounting_deltas",
        },
        Inv010CompositionOwner {
            obligation: "instruction error propagation and rollback boundary",
            path: "tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs",
            test: "v16_program_dispatch_and_entrypoints_preserve_every_handler_error",
        },
        Inv010CompositionOwner {
            obligation: "conflicting capability controls and trade",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_conflicting_matcher_controls_and_trade_exhaust_all_landing_orders",
        },
        Inv010CompositionOwner {
            obligation: "portfolio value and control permutations",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_portfolio_value_and_control_requests_exhaust_all_landing_orders",
        },
        Inv010CompositionOwner {
            obligation: "independent deposit and reduction order",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_deposit_and_owner_reduction_commute_across_independent_bindings",
        },
        Inv010CompositionOwner {
            obligation: "authority policy order",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_authority_handoff_and_retained_policy_obey_both_landing_orders",
        },
        Inv010CompositionOwner {
            obligation: "authority resolution order",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_underfunded_claims_survive_both_authority_resolve_orders",
        },
        Inv010CompositionOwner {
            obligation: "policy authority resolution higher-order product",
            path: "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            test: "v16_program_underfunded_policy_handoff_and_resolve_exhaust_all_landing_orders",
        },
    ];

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut obligations = std::collections::BTreeSet::new();
    for owner in OWNERS {
        assert!(
            obligations.insert(owner.obligation),
            "duplicate ordering obligation"
        );
        let source = std::fs::read_to_string(root.join(owner.path))
            .unwrap_or_else(|error| panic!("read {}: {error}", owner.path));
        assert!(
            inv010_source_defines_test(&source, owner.test),
            "ordering obligation '{}' lacks executable owner {}#{}",
            owner.obligation,
            owner.path,
            owner.test,
        );
    }
    assert_eq!(obligations.len(), 15, "ordering composition drift");

    let kani =
        std::fs::read_to_string(root.join("tests/invariants/kani/inv_010_out_of_order_safety.rs"))
            .expect("read INV-010 one-step theorem");
    assert!(kani.contains("#[kani::proof]"));
    assert!(kani.contains("fn kani_v16_matcher_sequence_accepts_only_current_expected_value("));
    assert!(kani.contains("if current != expected || current == u64::MAX"));
    assert!(kani.contains("assert_eq!(result.unwrap(), current + 1)"));
}

#[test]
fn v16_program_matcher_mutation_order_rejects_revoked_capability_fixed_case() {
    let protection =
        crate::support::invariant_discovery::verify_matcher_mutation_order_safety([0x10; 32])
            .expect("matcher mutation order safety");
    assert!(
        protection.satisfies_invariant(),
        "matcher mutation order invariant failed: {protection:?}"
    );
}

#[test]
fn v16_program_retained_single_cpi_quote_refresh_preserves_both_landing_orders() {
    use crate::support::v16_svm::{MarketConfig, V16Svm};
    use crate::*;
    use percolator_prog::matcher_abi::read_matcher_return;
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const MARK: u64 = 10_007;
    const FEE_BPS: u64 = 37;
    const SPREADS: [u64; 2] = [317, 653];
    let ceil = |n: u128, d: u128| n / d + u128::from(n % d != 0);
    let mut counts = [0; 3]; // Worlds, committed fills, rejected deliveries.
    for direction in [-1i128, 1] {
        for quote_first in [false, true] {
            let mut env = V16Svm::new(
                [0x10; 32],
                MarketConfig {
                    initial_price: MARK,
                    ..MarketConfig::default()
                },
            );
            env.update_trade_fee_policy(FEE_BPS).unwrap();
            env.set_matcher_spreads(1, SPREADS[0], SPREADS[1]).unwrap();
            let payer = Keypair::new();
            env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
            let price =
                MARK * if direction > 0 {
                    10_000 + SPREADS[1]
                } else {
                    10_000 - SPREADS[0]
                } / 10_000;
            let mut wider = SPREADS;
            wider[usize::from(direction > 0)] += 1;
            let wider_price =
                MARK * if direction > 0 {
                    10_000 + wider[1]
                } else {
                    10_000 - wider[0]
                } / 10_000;
            assert_eq!(i128::from(wider_price) - i128::from(price), direction);
            let initial_market = env.primary_market_state().1;
            let initial_capital = [0, 1].map(|i| env.primary_portfolio(i).capital.get());
            let initial_epochs = [0, 1].map(|i| env.primary_portfolio_position_epoch(i));
            let initial_sequence = env.primary_portfolio_matcher_sequence(1);
            let mut keys: Vec<_> = env
                .all_economic_account_lamports()
                .into_iter()
                .map(|(key, _)| key)
                .collect();
            keys.extend(env.actors.iter().map(|actor| actor.signer.pubkey()));
            let mutable = [
                env.market,
                env.actors[0].portfolio,
                env.actors[1].portfolio,
                env.actors[1].matcher_context,
            ];
            let passive: Vec<_> = keys
                .iter()
                .copied()
                .filter(|key| !mutable.contains(key))
                .collect();
            let frame = |env: &V16Svm, keys: &[Pubkey]| {
                keys.iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>()
            };
            let initial_passive = frame(&env, &passive);
            // Quote amounts describe execution, not an SPL transfer. Fees use the fixed mark.
            let economics = |size: i128, execution_price: u64| {
                let q = size.unsigned_abs();
                [
                    if size > 0 {
                        ceil(q * u128::from(execution_price), POS_SCALE)
                    } else {
                        0
                    },
                    if size < 0 {
                        q * u128::from(execution_price) / POS_SCALE
                    } else {
                        0
                    },
                    ceil(q * u128::from(execution_price.abs_diff(MARK)), POS_SCALE),
                    ceil(
                        ceil(q * u128::from(MARK), POS_SCALE) * u128::from(FEE_BPS),
                        10_000,
                    ),
                ]
            };
            let check = |env: &V16Svm, quantity: i128, totals: [u128; 4], fills: u64| {
                for i in [0, 1] {
                    let account = env.primary_portfolio(i);
                    assert_eq!(account.capital.get(), initial_capital[i] - totals[3]);
                    assert_eq!(account.pnl.get(), 0);
                    assert_eq!(
                        env.primary_portfolio_position_epoch(i),
                        initial_epochs[i] + fills
                    );
                    if fills == 0 {
                        assert!(!has_active_leg_for_asset(&account, 0));
                    } else {
                        assert_eq!(
                            active_leg_for_asset(&account, 0).basis_pos_q,
                            if i == 0 { quantity } else { -quantity }
                        );
                    }
                }
                assert_eq!(env.primary_portfolio_matcher_sequence(1), initial_sequence);
                let market = env.primary_market_state().1;
                assert_eq!(market.assets[0].effective_price, MARK);
                assert_eq!(market.assets[0].oi_eff_long_q, quantity.unsigned_abs());
                assert_eq!(market.assets[0].oi_eff_short_q, quantity.unsigned_abs());
                assert_eq!(market.c_tot, initial_market.c_tot - 2 * totals[3]);
                assert_eq!(market.insurance, initial_market.insurance + 2 * totals[3]);
                assert_eq!(market.vault, initial_market.vault);
                assert_eq!(market.vault, market.c_tot + market.insurance);
                assert_eq!(market.vault, u128::from(env.token_amount(env.vault)));
                assert_eq!(env.token_supply_observed(), env.initial_token_supply);
                assert_eq!(u128::from(env.mint_supply()), env.initial_token_supply);
                assert_eq!(frame(env, &passive), initial_passive);
            };
            let mut quantity = 0;
            let mut totals = [0u128; 4];
            let mut expected = [0u128; 4];
            check(&env, quantity, totals, 0);
            for (step, units) in [3, 2].into_iter().enumerate() {
                let size = direction * (units * POS_SCALE + 1) as i128;
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.actors[0].signer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.actors[0].portfolio, false),
                        AccountMeta::new(env.actors[1].portfolio, false),
                        AccountMeta::new_readonly(env.matcher_program, false),
                        AccountMeta::new(env.actors[1].matcher_context, false),
                        AccountMeta::new_readonly(env.actors[1].matcher_delegate, false),
                    ],
                    data: ProgInstruction::TradeCpi {
                        account_a_portfolio_id: env.primary_portfolio_id(0),
                        account_a_position_epoch: env.primary_portfolio_position_epoch(0),
                        account_b_portfolio_id: env.primary_portfolio_id(1),
                        account_b_position_epoch: env.primary_portfolio_position_epoch(1),
                        account_b_matcher_sequence: initial_sequence,
                        asset_index: 0,
                        market_id: initial_market.assets[0].market_id,
                        size_q: size,
                        fee_bps: FEE_BPS,
                        limit_price: price,
                        backing_fee_cap_bps: 0,
                    }
                    .encode(),
                };
                let sign = |priority| {
                    Transaction::new_signed_with_payer(
                        &[
                            heap_ix(),
                            cu_ix(),
                            ComputeBudgetInstruction::set_compute_unit_price(priority),
                            ix.clone(),
                        ],
                        Some(&payer.pubkey()),
                        &[&payer, &env.actors[0].signer],
                        env.svm.latest_blockhash(),
                    )
                };
                // Distinct signatures preserve normal runtime deduplication. Both wrapper messages
                // are signed before the quote writer; the control is never rebuilt after refusal.
                let retained = sign(0);
                let probe = sign(1);
                assert_ne!(retained.signatures, probe.signatures);
                assert_eq!(
                    retained.message.instructions[3],
                    probe.message.instructions[3]
                );
                retained.verify().unwrap();
                probe.verify().unwrap();
                let wire = bincode::serialize(&retained).unwrap();
                let before = frame(&env, &keys);
                env.svm
                    .simulate_transaction(retained.clone().into())
                    .expect("initially executable retained request");
                assert_eq!(frame(&env, &keys), before);
                if quote_first {
                    env.set_matcher_spreads(1, wider[0], wider[1]).unwrap();
                    check(&env, quantity, totals, step as u64);
                    let before = frame(&env, &keys);
                    let refusal = TransactionError::InstructionError(
                        3,
                        InstructionError::Custom(PercolatorError::InvalidInstruction as u32),
                    );
                    let preflight = env
                        .svm
                        .simulate_transaction(retained.clone().into())
                        .expect_err(
                            "the untouched retained envelope also exceeds the current quote limit",
                        );
                    assert_eq!(preflight.err, refusal);
                    assert_eq!(frame(&env, &keys), before);
                    let error = env
                        .svm
                        .send_transaction(probe)
                        .expect_err("one quote unit beyond signed limit");
                    assert_eq!(error.err, refusal);
                    assert!(
                        error
                            .meta
                            .logs
                            .iter()
                            .any(|log| log == &format!("Program {} success", env.matcher_program)),
                        "matcher must execute before the signed-limit refusal"
                    );
                    assert_eq!(
                        frame(&env, &keys),
                        before,
                        "matcher, wrapper, custody and economic lamports roll back"
                    );
                    check(&env, quantity, totals, step as u64);
                    counts[2] += 1;
                    env.set_matcher_spreads(1, SPREADS[0], SPREADS[1]).unwrap();
                    check(&env, quantity, totals, step as u64);
                }
                assert_eq!(
                    env.svm.latest_blockhash(),
                    retained.message.recent_blockhash
                );
                assert_eq!(bincode::serialize(&retained).unwrap(), wire);
                let result = env
                    .svm
                    .send_transaction(retained)
                    .expect("unchanged retained control remains live");
                assert_cu_within(
                    "retained single-CPI quote order",
                    result.compute_units_consumed,
                    1_400_000,
                );
                let ctx = env.svm.get_account(&env.actors[1].matcher_context).unwrap();
                let fill = read_matcher_return(&ctx.data).unwrap();
                assert_eq!((fill.exec_size, fill.exec_price_e6), (size, price));
                quantity += fill.exec_size;
                for (total, value) in totals
                    .iter_mut()
                    .zip(economics(fill.exec_size, fill.exec_price_e6))
                {
                    *total += value;
                }
                for (total, value) in expected.iter_mut().zip(economics(size, price)) {
                    *total += value;
                }
                assert_eq!(
                    totals, expected,
                    "exact cumulative signed quantity/quote/slippage/fee ledger"
                );
                check(&env, quantity, totals, step as u64 + 1);
                counts[1] += 1;
                if !quote_first {
                    env.set_matcher_spreads(1, wider[0], wider[1]).unwrap();
                    check(&env, quantity, totals, step as u64 + 1);
                    env.set_matcher_spreads(1, SPREADS[0], SPREADS[1]).unwrap();
                    check(&env, quantity, totals, step as u64 + 1);
                }
            }
            assert_eq!(quantity, direction * (5 * POS_SCALE + 2) as i128);
            assert_eq!(
                totals,
                if direction > 0 {
                    [53_302, 0, 3_267, 187]
                } else {
                    [0, 48_445, 1_592, 187]
                }
            );
            counts[0] += 1;
        }
    }
    assert_eq!(counts, [4, 8, 4]);
}
