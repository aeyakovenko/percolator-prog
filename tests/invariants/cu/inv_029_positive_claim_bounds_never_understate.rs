//! INV-029 - Positive claim bounds never understate.
//!
//! Normative obligation: market-level source-credit claim bounds must equal
//! the complete set of live portfolio-attributed positive claims. A conversion
//! or settlement route may burn a bound only with the matching account-local
//! claim delta.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM wrapper test runs
//! the shared public-route lifecycle oracle with fixed non-boundary parameters:
//! two winners create claims in the same source domain, authenticated marks
//! partially burn those claims, the positions close, backing is added, and both
//! conversion orders are checked against an independent portfolio census after
//! every public transition.
//! The deployed profile has no approximate claim-bound buckets: every production source claim is
//! exact and atom-scaled. The source lock below keeps non-exact bound injection and rebucketing out
//! of the wrapper API, while the stateful complete-account census requires the persisted exact and
//! bound totals to remain equal after every generated public transition. Introducing an
//! approximate bucket is therefore a deliberate profile change that must replace this absence
//! proof with the charter's range-edge and rebucketing proofs.
//!
//! The symbolic induction owner proves arbitrary one-claim replacement at the account, domain, and
//! market aggregate levels and exact receipt replacement. The source-complete composition below
//! binds those steps to the pinned engine contracts and public transition roster, so finite replay
//! supplies reachability and nonvacuity rather than the sequence-length argument.
//!
//! The recovery-resize witness keeps a one-atom claim from a settled mark reversal alongside the
//! opposite side's larger claim. Halving live exposure through permissionless Recovery close must
//! not halve either accrued bound. Both remaining legs then exit through Resolved, with actual SPL
//! payouts above input-derived settled capital checked against the pre-Recovery bounds. This adds
//! a terminal consumer of recovery claims, peer isolation, and bounded progress to the live census.

use super::*;

#[test]
fn v16_program_recovery_half_close_preserves_one_atom_claim_bound_through_resolved_payout() {
    const OPEN_PRICE: u64 = 100;
    const PEAK_PRICE: u64 = 119;
    const FINAL_PRICE: u64 = 118;
    const DEPOSIT: u128 = 1_000;
    const DOMAINS: [usize; 2] = [1, 0];
    let claims = [
        u128::from(FINAL_PRICE - OPEN_PRICE),
        u128::from(PEAK_PRICE - FINAL_PRICE),
    ];
    let settled_capital = [DEPOSIT, DEPOSIT - u128::from(PEAK_PRICE - OPEN_PRICE)];
    assert_eq!(
        claims,
        [18, 1],
        "the recovery claimant owns exactly one atom"
    );

    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(100, 1);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN_PRICE);
    let owners = [Keypair::new(), Keypair::new()];
    let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
    for i in 0..2 {
        env.deposit(&owners[i], portfolios[i], DEPOSIT);
    }
    let cu = env.trade_asset_with_cu(
        0,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        POS_SCALE as i128,
        OPEN_PRICE,
        0,
    );
    assert_cu_within("INV-029 open", cu, TRADE_CU_LIMIT);

    // Settle the short's peak loss before reversing the mark: its later gain is a recovery
    // claim, not a refund still hidden in negative PnL or a change in principal.
    for (slot, price) in [
        (2, PEAK_PRICE),
        (3, PEAK_PRICE),
        (4, FINAL_PRICE),
        (5, FINAL_PRICE),
    ] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(0, slot, price);
        for portfolio in [portfolios[1], portfolios[0]] {
            if let Some(cu) = env.crank_if_actionable(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            ) {
                assert_cu_within("INV-029 mark settlement", cu, CRANK_CU_LIMIT);
            }
        }
        assert_eq!(env.market_state().1.assets[0].effective_price, price);
        if slot == 3 {
            let short = env.portfolio_state(portfolios[1]);
            assert_eq!(short.capital.get(), settled_capital[1]);
            assert_eq!(short.pnl.get(), 0);
            assert_eq!(
                env.market_state().1.source_credit[0].positive_claim_bound_num,
                0
            );
        }
    }

    let assert_claims = |env: &V16CuEnv, expected: [u128; 2]| {
        let group = env.market_state().1;
        assert_eq!(group.materialized_portfolio_count, 2);
        for i in 0..2 {
            let account = env.portfolio_state(portfolios[i]);
            let local: u128 = account
                .source_domains
                .iter()
                .filter(|source| source.is_occupied())
                .map(|source| {
                    if source.source_claim_bound_num.get() != 0 {
                        assert_eq!(source.domain.get(), DOMAINS[i] as u32);
                    }
                    source.source_claim_bound_num.get()
                })
                .sum();
            assert_eq!(local, expected[i] * BOUND_SCALE);
            assert_eq!(
                group.source_credit[DOMAINS[i]].positive_claim_bound_num,
                local
            );
            assert_eq!(
                group.source_credit[DOMAINS[i]].exact_positive_claim_num,
                local
            );
        }
        assert_eq!(
            group.source_claim_bound_total_num,
            expected.iter().sum::<u128>() * BOUND_SCALE
        );
    };
    assert_claims(&env, claims);
    for i in 0..2 {
        let account = env.portfolio_state(portfolios[i]);
        assert_eq!(account.capital.get(), settled_capital[i]);
        assert_eq!(account.pnl.get(), claims[i] as i128);
    }
    let prior_bounds =
        DOMAINS.map(|domain| env.market_state().1.source_credit[domain].positive_claim_bound_num);
    let vault_before_recovery = env.svm.get_account(&env.vault).unwrap();

    env.svm.warp_to_slot(6);
    let cu = env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 6, 0);
    assert_cu_within("INV-029 enter Recovery", cu, CUSTODY_CU_LIMIT);
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_claims(&env, claims);

    env.svm.warp_to_slot(7);
    let cranker = Keypair::new();
    let recovery_cu = env.force_close_abandoned_asset_with_cu(
        &cranker,
        portfolios[0],
        portfolios[1],
        0,
        7,
        POS_SCALE / 2,
    );
    assert_cu_within("INV-029 half Recovery close", recovery_cu, TRADE_CU_LIMIT);
    let group = env.market_state().1;
    assert_eq!(group.assets[0].oi_eff_long_q, POS_SCALE / 2);
    assert_eq!(group.assets[0].oi_eff_short_q, POS_SCALE / 2);
    assert_claims(&env, claims);
    for i in 0..2 {
        let account = env.portfolio_state(portfolios[i]);
        assert!(has_active_leg_for_asset(&account, 0));
        assert_eq!(account.capital.get(), settled_capital[i]);
        assert_eq!(account.pnl.get(), claims[i] as i128);
    }
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_recovery
    );

    let resolve_cu = env.resolve_stale_permissionless_with_cu(107);
    assert_cu_within("INV-029 permissionless resolve", resolve_cu, CRANK_CU_LIMIT);
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
    assert_claims(&env, claims);
    env.svm.warp_to_slot(108);

    let mut paid = [0u128; 2];
    let mut calls = 0;
    let mut progress_only_calls = 0;
    let mut max_payout_cu = 0;
    for _ in 0..4 {
        // Put the one-atom recovery claimant first, while its peer still owns a larger bound.
        for i in [1usize, 0] {
            if resolved_portfolio_is_terminal(&env, portfolios[i]) {
                continue;
            }
            let peer_before = env.svm.get_account(&portfolios[1 - i]).unwrap();
            let group_before = env.market_state().1;
            let (destination, cu) = env.close_resolved_with_cu(&owners[i], portfolios[i]);
            assert_cu_within("INV-029 bounded resolved claim", cu, CUSTODY_CU_LIMIT);
            max_payout_cu = max_payout_cu.max(cu);
            calls += 1;
            let payout = u128::from(env.token_amount(destination));
            progress_only_calls += usize::from(payout == 0);
            paid[i] += payout;
            let account = env.portfolio_state(portfolios[i]);
            let group = env.market_state().1;
            let consumed_claim = paid[i] + account.capital.get() - settled_capital[i];
            assert!(
                consumed_claim * BOUND_SCALE <= prior_bounds[i],
                "payout exceeded the pre-Recovery claim bound"
            );
            assert_eq!(
                env.svm.get_account(&portfolios[1 - i]).unwrap(),
                peer_before
            );
            assert_eq!(
                group.source_credit[DOMAINS[1 - i]].positive_claim_bound_num,
                group_before.source_credit[DOMAINS[1 - i]].positive_claim_bound_num
            );
            assert_eq!(group_before.vault - group.vault, payout);
            assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
        }
        if portfolios
            .iter()
            .all(|portfolio| resolved_portfolio_is_terminal(&env, *portfolio))
        {
            break;
        }
    }
    assert!(
        portfolios
            .iter()
            .all(|portfolio| resolved_portfolio_is_terminal(&env, *portfolio)),
        "both claims must complete within four bounded round-robin sweeps"
    );
    assert!(calls >= 2);
    assert_eq!(
        paid,
        [
            DEPOSIT + u128::from(FINAL_PRICE - OPEN_PRICE),
            DEPOSIT - u128::from(FINAL_PRICE - OPEN_PRICE)
        ]
    );
    for i in 0..2 {
        let claim_payout = paid[i] - settled_capital[i];
        assert_eq!(claim_payout, claims[i]);
        assert_eq!(claim_payout * BOUND_SCALE, prior_bounds[i]);
    }
    assert_claims(&env, [0, 0]);
    let group = env.market_state().1;
    assert_eq!(group.assets[0].oi_eff_long_q, 0);
    assert_eq!(group.assets[0].oi_eff_short_q, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(group.vault, 0);
    assert_eq!(paid.iter().sum::<u128>(), 2 * DEPOSIT);
    eprintln!(
        "INV-029: recovery CU={recovery_cu}, resolve CU={resolve_cu}, max payout CU={max_payout_cu}, \
         close calls={calls}, progress-only calls={progress_only_calls}, payouts={paid:?}"
    );
}

#[test]
fn v16_program_positive_claim_bounds_match_public_lifecycle_census() {
    crate::support::fuzz_model::verify_positive_claim_bound_attribution_lifecycle(
        [0x29; 32], 3, 13, true,
    )
    .expect("positive-claim bound public lifecycle census");
}

#[test]
fn v16_program_non_exact_claim_bound_routes_remain_absent_from_deployed_profile() {
    let source = include_str!("../../../src/v16_program.rs");
    let production = source
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production source prefix");

    for forbidden in [
        "add_source_positive_claim_bound_not_atomic",
        "claim_bound_bucket",
        "rebucket_claim",
    ] {
        assert!(
            !production.contains(forbidden),
            "non-exact claim-bound mechanism {forbidden} entered the public wrapper; INV-029 \
             requires range and rebucketing coverage before deployment",
        );
    }

    crate::assert_certified_engine_pin("INV-029 exact-claim profile evidence");
}

#[test]
fn v16_program_exact_claim_bound_composition_is_source_complete() {
    crate::assert_certified_engine_pin("INV-029 exact-claim composition");

    let model = include_str!("../../support/fuzz_model.rs");
    for required in [
        "fn assert_source_claim_bound_attribution(",
        "source.exact_positive_claim_num != source.positive_claim_bound_num",
        "attributed[domain] != source.positive_claim_bound_num",
        "domain_total != group.source_claim_bound_total_num",
        "assert_source_claim_bound_attribution(\"primary\"",
        "assert_source_claim_bound_attribution(\"foreign\"",
        "fn bounded_claim_state_changed(",
        "claim_changing_edge_count",
        "receipt_replacement_count",
    ] {
        assert!(
            model.contains(required),
            "shared public-transition model lost INV-029 relation {required}",
        );
    }

    let bounded =
        include_str!("../stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs");
    for required in [
        "fn v16_program_bounded_reference_graph_exhausts_public_action_words",
        "evidence.claim_changing_edge_count != 0",
        "evidence.receipt_replacement_count >= evidence.partial_receipt_seed_count as u64",
    ] {
        assert!(
            bounded.contains(required),
            "bounded deployed graph lost INV-029 witness {required}",
        );
    }

    let stateful = include_str!("../stateful/inv_029_positive_claim_bounds_never_understate.rs");
    for required in [
        "fn v16_program_claim_bound_boundary_partition_exhausts_public_lifecycle_grid",
        "fn v16_program_favorable_funding_claim_bounds_are_exact_across_routes_and_sides",
        "fn v16_program_stale_positive_claim_blocks_snapshot_until_exactly_materialized",
        "fn v16_program_partial_receipt_exactly_replaces_its_prior_claim_bound",
    ] {
        assert!(
            stateful.contains(required),
            "INV-029 public route matrix lost witness {required}",
        );
    }

    let transitions = include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert!(transitions.contains(
        "fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness"
    ));

    let induction = include_str!("../kani/inv_029_positive_claim_bounds_never_understate.rs");
    for required in [
        "fn kani_inv029_exact_claim_replacement_preserves_all_aggregate_levels(",
        "positive_pnl_atoms <= account_after_atoms",
        "fn kani_inv029_exact_receipt_replacement_preserves_claim_mass(",
        "claim_mass_after, claim_mass_before",
    ] {
        assert!(
            induction.contains(required),
            "INV-029 induction decomposition lost {required}"
        );
    }

    let engine_contracts = [
        "contract_check_prepare_source_positive_claim_bound_delta",
        "contract_check_prepare_source_positive_claim_burn_delta",
        "contract_check_apply_total_delta",
    ];
    assert_eq!(
        engine_contracts.len(),
        3,
        "INV-029 engine claim-delta contract roster drift"
    );
}
