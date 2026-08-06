//! INV-036 - Fee destination and policy-version integrity.
//!
//! Normative obligation: Charged fees reach only the authorized destination under the bound policy version.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_program_signed_direction_route_matrix_discovers_terminal_side_fee_loss`, `v16_attack_mixed_direction_batch_fees_conserve_by_asset`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[derive(Debug, PartialEq, Eq)]
struct DirectionalFeeTerminalOutcome {
    winner_payout: u128,
    long_budget: u128,
    short_budget: u128,
    terminal_vault: u128,
}

fn run_directional_fee_terminal_world(
    path: NoCpiReportedPricePath,
) -> DirectionalFeeTerminalOutcome {
    const MARK: u64 = 1_000_000;
    const FEE: u128 = MARK as u128;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        maintenance_fee_per_slot: FEE - 1,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, MARK);
    env.update_maintenance_fee_policy_with_cu(10_000);

    let low_owner = Keypair::new();
    let victim_owner = Keypair::new();
    let reward_owner = Keypair::new();
    let low = env.create_portfolio(&low_owner);
    let victim = env.create_portfolio(&victim_owner);
    let reward = env.create_portfolio(&reward_owner);
    env.deposit(&low_owner, low, FEE);
    env.deposit(&victim_owner, victim, 2 * FEE);
    env.trade_asset_with_cu(
        0,
        &low_owner,
        low,
        &victim_owner,
        victim,
        POS_SCALE as i128,
        MARK,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, MARK);
    for portfolio in [low, victim] {
        for _ in 0..2 {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 1,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }
    }
    env.sync_maintenance_fee_with_cu(low, Some(reward), 1);
    assert_eq!(env.portfolio_state(low).capital.get(), 1);

    env.svm.expire_blockhash();
    try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &low_owner,
        low,
        &victim_owner,
        victim,
        -(POS_SCALE as i128),
        MARK,
        10_000,
    )
    .expect("risk-reducing directional-fee trade");
    let after_fee = env.market_state().1;
    let long_budget = after_fee.insurance_domain_budget[0];
    let short_budget = after_fee.insurance_domain_budget[1];
    assert_eq!(env.portfolio_state(low).capital.get(), 0);
    assert_eq!(env.portfolio_state(victim).capital.get(), FEE);

    let reward_dest = env.withdraw(&reward_owner, reward, FEE - 1);
    assert_eq!(env.token_amount(reward_dest) as u128, FEE - 1);
    env.close_portfolio_with_cu(&reward_owner, reward);
    env.close_portfolio_with_cu(&low_owner, low);

    let bankrupt_owner = Keypair::new();
    let keeper_owner = Keypair::new();
    let bankrupt = env.create_portfolio(&bankrupt_owner);
    let keeper = env.create_portfolio(&keeper_owner);
    env.deposit(&bankrupt_owner, bankrupt, FEE);
    env.trade_asset_with_cu(
        0,
        &victim_owner,
        victim,
        &bankrupt_owner,
        bankrupt,
        POS_SCALE as i128,
        MARK,
        0,
    );

    for slot in [2u64, 3] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 3 * MARK);
        for _ in 0..3 {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(keeper, false),
                ],
                &[],
            );
        }
    }
    assert_eq!(env.market_state().1.assets[0].effective_price, 3 * MARK);
    env.resolve();

    let mut winner_payout = 0u128;
    for _ in 0..8 {
        let loser = env.close_resolved(&bankrupt_owner, bankrupt);
        assert_eq!(env.token_amount(loser), 0);
        let winner = env.close_resolved(&victim_owner, victim);
        winner_payout += u128::from(env.token_amount(winner));
        let victim_state = env.portfolio_state(victim);
        if victim_state.capital.get() == 0
            && victim_state.pnl.get() == 0
            && !has_active_leg_for_asset(&victim_state, 0)
        {
            break;
        }
    }
    let terminal = env.market_state().1;
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));
    DirectionalFeeTerminalOutcome {
        winner_payout,
        long_budget,
        short_budget,
        terminal_vault: terminal.vault,
    }
}

#[test]
fn v16_program_signed_direction_route_matrix_discovers_terminal_side_fee_loss() {
    let single = run_directional_fee_terminal_world(NoCpiReportedPricePath::Single);
    let batch = run_directional_fee_terminal_world(NoCpiReportedPricePath::Batch);
    assert_eq!(single.winner_payout, 1_000_000);
    assert!(single.long_budget > single.short_budget);
    assert_eq!(batch.winner_payout, 0);
    assert!(batch.short_budget > batch.long_budget);
    assert_eq!(single.long_budget, batch.short_budget);
    assert_eq!(single.short_budget, batch.long_budget);
    assert!(batch.terminal_vault > single.terminal_vault);
}

#[test]
fn v16_attack_mixed_direction_batch_fees_conserve_by_asset() {
    #[derive(Clone, Copy, Debug)]
    enum Path {
        NoCpi,
        Cpi,
    }

    for path in [Path::NoCpi, Path::Cpi] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            max_portfolio_assets: 2,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            trade_fee_base_bps: 100,
            ..V16CuMarketParams::default()
        });
        env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
        env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
        let taker = Keypair::new();
        let lp = Keypair::new();
        let taker_account = env.create_portfolio(&taker);
        let lp_account = env.create_portfolio(&lp);
        env.deposit(&taker, taker_account, 10_000_000);
        env.deposit(&lp, lp_account, 10_000_000);

        let before = env.market_state().1;
        let asset0_budget_before =
            before.insurance_domain_budget[0] + before.insurance_domain_budget[1];
        let asset1_budget_before =
            before.insurance_domain_budget[2] + before.insurance_domain_budget[3];
        let sz = (10 * POS_SCALE) as i128;

        env.svm.expire_blockhash();
        match path {
            Path::NoCpi => {
                env.send(
                    ProgInstruction::BatchTradeNoCpi {
                        legs: vec![
                            BatchTradeLeg {
                                asset_index: 0,
                                market_id: first_generation_market_id(0),
                                size_q: sz,
                                exec_price: 100,
                                fee_bps: 100,
                            },
                            BatchTradeLeg {
                                asset_index: 1,
                                market_id: first_generation_market_id(1),
                                size_q: -sz,
                                exec_price: 100,
                                fee_bps: 100,
                            },
                        ],
                    },
                    vec![
                        AccountMeta::new(taker.pubkey(), true),
                        AccountMeta::new(lp.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(taker_account, false),
                        AccountMeta::new(lp_account, false),
                    ],
                    &[&taker, &lp],
                )
                .unwrap_or_else(|err| panic!("{path:?} mixed-fee batch failed: {err}"));
            }
            Path::Cpi => {
                let matcher_program = Pubkey::new_unique();
                let matcher_bytes =
                    std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
                env.svm.add_program(matcher_program, &matcher_bytes);
                let (ctx, delegate, _) =
                    env.init_auth_matcher_context(matcher_program, &lp, lp_account);
                env.send(
                    ProgInstruction::BatchTradeCpi {
                        legs: vec![
                            BatchTradeCpiLeg {
                                asset_index: 0,
                                market_id: first_generation_market_id(0),
                                size_q: sz,
                                fee_bps: 100,
                                limit_price: 0,
                            },
                            BatchTradeCpiLeg {
                                asset_index: 1,
                                market_id: first_generation_market_id(1),
                                size_q: -sz,
                                fee_bps: 100,
                                limit_price: 0,
                            },
                        ],
                    },
                    vec![
                        AccountMeta::new(taker.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(taker_account, false),
                        AccountMeta::new(lp_account, false),
                        AccountMeta::new_readonly(matcher_program, false),
                        AccountMeta::new(ctx, false),
                        AccountMeta::new_readonly(delegate, false),
                    ],
                    &[&taker],
                )
                .unwrap_or_else(|err| panic!("{path:?} mixed-fee batch failed: {err}"));
            }
        }

        let after = env.market_state().1;
        let asset0_budget_delta = after.insurance_domain_budget[0]
            + after.insurance_domain_budget[1]
            - asset0_budget_before;
        let asset1_budget_delta = after.insurance_domain_budget[2]
            + after.insurance_domain_budget[3]
            - asset1_budget_before;
        let insurance_delta = after.insurance - before.insurance;
        assert!(insurance_delta > 0, "{path:?} must charge a nonzero fee");
        assert_eq!(
            asset0_budget_delta + asset1_budget_delta,
            insurance_delta,
            "{path:?} must budget every mixed-leg fee atom"
        );
        assert_eq!(
            asset0_budget_delta, asset1_budget_delta,
            "{path:?} same-size/same-fee mixed legs should credit equal per-asset fee budgets"
        );
        assert_eq!(after.vault, before.vault, "{path:?} must not move custody");
        assert_eq!(
            after.vault,
            after.c_tot + after.insurance,
            "{path:?} preserves senior conservation after mixed-fee batch"
        );
        assert_domain_budget_remaining_total_consistent(&after, "mixed-fee batch budgets");

        let taker_after = env.portfolio_state(taker_account);
        let lp_after = env.portfolio_state(lp_account);
        assert_eq!(active_leg_for_asset(&taker_after, 0).basis_pos_q, sz);
        assert_eq!(active_leg_for_asset(&taker_after, 1).basis_pos_q, -sz);
        assert_eq!(active_leg_for_asset(&lp_after, 0).basis_pos_q, -sz);
        assert_eq!(active_leg_for_asset(&lp_after, 1).basis_pos_q, sz);
    }
}
