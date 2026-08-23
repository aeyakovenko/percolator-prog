//! INV-036 - Fee destination and policy-version integrity.
//!
//! Normative obligation: Charged fees reach only the authorized destination under the bound policy version.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_program_signed_direction_route_matrix_preserves_side_attribution_and_terminal_value`, `v16_program_mixed_direction_fee_allocation_matches_independent_side_ledger`, `v16_attack_mixed_direction_batch_fees_conserve_by_asset`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: the route matrix certifies the independently discovered negative-size
//! account-ordering case through terminal payout. Broader fee-policy and route cross-products
//! remain tracked in the invariant roadmap.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectionalFeeTerminalOutcome {
    winner_payout: u128,
    long_budget: u128,
    short_budget: u128,
    terminal_vault: u128,
}

#[derive(Clone, Copy, Debug)]
enum DirectionalFeePath {
    SingleNoCpi,
    BatchNoCpi,
    SingleCpi,
    BatchCpi,
}

impl DirectionalFeePath {
    const ALL: [Self; 4] = [
        Self::SingleNoCpi,
        Self::BatchNoCpi,
        Self::SingleCpi,
        Self::BatchCpi,
    ];
}

#[allow(clippy::too_many_arguments)]
fn execute_directional_fee_trade(
    env: &mut V16CuEnv,
    path: DirectionalFeePath,
    owner_a: &Keypair,
    account_a: Pubkey,
    owner_b: &Keypair,
    account_b: Pubkey,
    size_q: i128,
    exec_price: u64,
    fee_bps: u64,
) {
    env.svm.expire_blockhash();
    match path {
        DirectionalFeePath::SingleNoCpi => env
            .try_trade_asset_with_cu(
                0, owner_a, account_a, owner_b, account_b, size_q, exec_price, fee_bps,
            )
            .expect("directional TradeNoCpi"),
        DirectionalFeePath::BatchNoCpi => env
            .send(
                env.batch_trade_no_cpi_ix(
                    account_a,
                    account_b,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
                        exec_price,
                        fee_bps,
                    }],
                ),
                vec![
                    AccountMeta::new(owner_a.pubkey(), true),
                    AccountMeta::new(owner_b.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(account_a, false),
                    AccountMeta::new(account_b, false),
                ],
                &[owner_a, owner_b],
            )
            .expect("directional BatchTradeNoCpi"),
        DirectionalFeePath::SingleCpi | DirectionalFeePath::BatchCpi => {
            let (matcher_program, ctx, delegate) =
                auth_matcher_for_lp_via_system_create(env, owner_b, account_b);
            match path {
                DirectionalFeePath::SingleCpi => env
                    .try_trade_cpi_with_cu_on_asset(
                        owner_a,
                        account_a,
                        owner_b,
                        account_b,
                        matcher_program,
                        ctx,
                        delegate,
                        0,
                        size_q,
                        fee_bps,
                    )
                    .expect("directional TradeCpi"),
                DirectionalFeePath::BatchCpi => env
                    .send(
                        env.batch_trade_cpi_ix(
                            account_a,
                            account_b,
                            vec![BatchTradeCpiLeg {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                size_q,
                                fee_bps,
                                limit_price: 0,
                            }],
                        ),
                        vec![
                            AccountMeta::new(owner_a.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(account_a, false),
                            AccountMeta::new(account_b, false),
                            AccountMeta::new_readonly(matcher_program, false),
                            AccountMeta::new(ctx, false),
                            AccountMeta::new_readonly(delegate, false),
                        ],
                        &[owner_a],
                    )
                    .expect("directional BatchTradeCpi"),
                _ => unreachable!(),
            }
        }
    };
}

fn run_directional_fee_terminal_world(
    path: DirectionalFeePath,
    opening_size_q: i128,
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
    let refresh_owner = Keypair::new();
    let low = env.create_portfolio(&low_owner);
    let victim = env.create_portfolio(&victim_owner);
    let reward = env.create_portfolio(&reward_owner);
    let refresh = env.create_portfolio(&refresh_owner);
    env.deposit(&low_owner, low, FEE);
    env.deposit(&victim_owner, victim, 2 * FEE);
    env.trade_asset_with_cu(
        0,
        &low_owner,
        low,
        &victim_owner,
        victim,
        opening_size_q,
        MARK,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, MARK);
    env.crank(
        refresh,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    // Keep this directional trade-fee regression independent of maintenance-fee ordering. A
    // self-rewarded public sync advances the victim's cursor without changing its capital or any
    // domain budget, so the terminal delta below still isolates single-vs-batch side attribution.
    env.sync_maintenance_fee_with_cu(victim, Some(victim), 1);
    assert_eq!(env.portfolio_state(victim).capital.get(), 2 * FEE);
    env.sync_maintenance_fee_with_cu(low, Some(reward), 1);
    assert_eq!(env.portfolio_state(low).capital.get(), 1);

    // CPI routes can debit the unsigned LP only under the authenticated market base-fee policy.
    // Install the same fee as the two owners' no-CPI ceiling so all four routes execute one
    // economically identical fee intent.
    env.update_trade_fee_policy_with_cu(10_000);
    execute_directional_fee_trade(
        &mut env,
        path,
        &low_owner,
        low,
        &victim_owner,
        victim,
        -opening_size_q,
        MARK,
        10_000,
    );
    let after_fee = env.market_state().1;
    let long_budget = after_fee.insurance_domain_budget[0];
    let short_budget = after_fee.insurance_domain_budget[1];
    assert_eq!(env.portfolio_state(low).capital.get(), 0);
    assert_eq!(env.portfolio_state(victim).capital.get(), FEE);
    env.update_trade_fee_policy_with_cu(0);

    // The reward account was created one slot earlier. Advance its own fee cursor through a
    // self-rewarded sync so its withdrawal cannot reintroduce maintenance ordering into this test.
    env.sync_maintenance_fee_with_cu(reward, Some(reward), 1);
    assert_eq!(env.portfolio_state(reward).capital.get(), FEE - 1);
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
            let _ = env.send_crank_if_actionable(
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
fn v16_program_signed_direction_route_matrix_preserves_side_attribution_and_terminal_value() {
    for opening_size_q in [POS_SCALE as i128, -(POS_SCALE as i128)] {
        let expected =
            run_directional_fee_terminal_world(DirectionalFeePath::SingleNoCpi, opening_size_q);
        if opening_size_q > 0 {
            assert_eq!(expected.winner_payout, 2_000_000);
            assert_eq!(expected.terminal_vault, 1_000_001);
            assert_eq!(
                (expected.long_budget, expected.short_budget),
                (1_000_000, 1)
            );
        } else {
            assert_eq!(expected.winner_payout, 1_000_000);
            assert_eq!(expected.terminal_vault, 2_000_001);
            assert_eq!(
                (expected.long_budget, expected.short_budget),
                (1, 1_000_000)
            );
        }

        for path in DirectionalFeePath::ALL.into_iter().skip(1) {
            let actual = run_directional_fee_terminal_world(path, opening_size_q);
            assert_eq!(
                actual, expected,
                "{path:?} must equal single no-CPI for opening size {opening_size_q}"
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum MixedDirectionFeePath {
    SequentialNoCpi,
    BatchNoCpi,
    SequentialCpi,
    BatchCpi,
}

impl MixedDirectionFeePath {
    const ALL: [Self; 4] = [
        Self::SequentialNoCpi,
        Self::BatchNoCpi,
        Self::SequentialCpi,
        Self::BatchCpi,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MixedDirectionFeeOutcome {
    domain_budgets: [u128; 4],
    insurance: u128,
    terminal_vault: u128,
}

fn run_mixed_direction_fee_world(
    path: MixedDirectionFeePath,
    reverse_leg_order: bool,
) -> MixedDirectionFeeOutcome {
    const MARK: u64 = 1_000_000;
    const FEE: u128 = MARK as u128;
    const LOW_CAPITAL: u128 = 250_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: 2,
        initial_price: MARK,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_for_asset_as_admin(0, 1, MARK);
    env.configure_auth_mark_for_asset_as_admin(1, 1, MARK);

    let low_owner = Keypair::new();
    let funded_owner = Keypair::new();
    let low = env.create_portfolio(&low_owner);
    let funded = env.create_portfolio(&funded_owner);
    env.deposit(&low_owner, low, LOW_CAPITAL);
    env.deposit(&funded_owner, funded, 4 * FEE);

    // Account A starts long asset 0 and short asset 1. Its two closing requests therefore have
    // opposite signs while the physical fee payer remains the same account in both legs.
    env.send(
        env.batch_trade_no_cpi_ix(
            low,
            funded,
            vec![
                BatchTradeLeg {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    size_q: POS_SCALE as i128,
                    exec_price: MARK,
                    fee_bps: 0,
                },
                BatchTradeLeg {
                    asset_index: 1,
                    market_id: env.asset_market_id(1),
                    size_q: -(POS_SCALE as i128),
                    exec_price: MARK,
                    fee_bps: 0,
                },
            ],
        ),
        vec![
            AccountMeta::new(low_owner.pubkey(), true),
            AccountMeta::new(funded_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(low, false),
            AccountMeta::new(funded, false),
        ],
        &[&low_owner, &funded_owner],
    )
    .expect("route-neutral mixed-direction opening batch");

    env.update_trade_fee_policy_with_cu(10_000);
    let mut close_legs = [(0u16, -(POS_SCALE as i128)), (1u16, POS_SCALE as i128)];
    if reverse_leg_order {
        close_legs.reverse();
    }
    match path {
        MixedDirectionFeePath::SequentialNoCpi => {
            for (asset_index, size_q) in close_legs {
                env.svm.expire_blockhash();
                env.trade_asset_with_cu(
                    asset_index,
                    &low_owner,
                    low,
                    &funded_owner,
                    funded,
                    size_q,
                    MARK,
                    10_000,
                );
            }
        }
        MixedDirectionFeePath::BatchNoCpi => {
            env.svm.expire_blockhash();
            env.send(
                env.batch_trade_no_cpi_ix(
                    low,
                    funded,
                    close_legs
                        .into_iter()
                        .map(|(asset_index, size_q)| BatchTradeLeg {
                            asset_index,
                            market_id: env.asset_market_id(asset_index),
                            size_q,
                            exec_price: MARK,
                            fee_bps: 10_000,
                        })
                        .collect(),
                ),
                vec![
                    AccountMeta::new(low_owner.pubkey(), true),
                    AccountMeta::new(funded_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(low, false),
                    AccountMeta::new(funded, false),
                ],
                &[&low_owner, &funded_owner],
            )
            .expect("mixed-direction BatchTradeNoCpi close");
        }
        MixedDirectionFeePath::SequentialCpi | MixedDirectionFeePath::BatchCpi => {
            let (matcher_program, ctx, delegate) =
                auth_matcher_for_lp_via_system_create(&mut env, &funded_owner, funded);
            match path {
                MixedDirectionFeePath::SequentialCpi => {
                    for (asset_index, size_q) in close_legs {
                        env.svm.expire_blockhash();
                        env.trade_cpi_with_cu_on_asset(
                            &low_owner,
                            low,
                            &funded_owner,
                            funded,
                            matcher_program,
                            ctx,
                            delegate,
                            asset_index,
                            size_q,
                            10_000,
                        );
                    }
                }
                MixedDirectionFeePath::BatchCpi => {
                    env.svm.expire_blockhash();
                    env.send(
                        env.batch_trade_cpi_ix(
                            low,
                            funded,
                            close_legs
                                .into_iter()
                                .map(|(asset_index, size_q)| BatchTradeCpiLeg {
                                    asset_index,
                                    market_id: env.asset_market_id(asset_index),
                                    size_q,
                                    fee_bps: 10_000,
                                    limit_price: 0,
                                })
                                .collect(),
                        ),
                        vec![
                            AccountMeta::new(low_owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(low, false),
                            AccountMeta::new(funded, false),
                            AccountMeta::new_readonly(matcher_program, false),
                            AccountMeta::new(ctx, false),
                            AccountMeta::new_readonly(delegate, false),
                        ],
                        &[&low_owner],
                    )
                    .expect("mixed-direction BatchTradeCpi close");
                }
                _ => unreachable!(),
            }
        }
    }

    let low_after = env.portfolio_state(low);
    let funded_after = env.portfolio_state(funded);
    for asset_index in 0..2 {
        assert!(!has_active_leg_for_asset(&low_after, asset_index));
        assert!(!has_active_leg_for_asset(&funded_after, asset_index));
    }
    assert_eq!(low_after.capital.get(), 0);
    assert_eq!(funded_after.capital.get(), 2 * FEE);

    let after_fee = env.market_state().1;
    let domain_budgets = [
        after_fee.insurance_domain_budget[0],
        after_fee.insurance_domain_budget[1],
        after_fee.insurance_domain_budget[2],
        after_fee.insurance_domain_budget[3],
    ];
    // Independent account-to-side ledger. B pays one full fee on each leg. A's finite capital is
    // consumed by the first leg and must follow that leg's signed economic side, not account A's
    // physical index in the engine result.
    let expected_budgets = if reverse_leg_order {
        [FEE, 0, LOW_CAPITAL, FEE]
    } else {
        [FEE, LOW_CAPITAL, 0, FEE]
    };
    assert_eq!(domain_budgets, expected_budgets);
    assert_eq!(after_fee.insurance, 2 * FEE + LOW_CAPITAL);
    assert_eq!(
        domain_budgets.into_iter().sum::<u128>(),
        after_fee.insurance
    );
    assert_eq!(after_fee.vault, 4 * FEE + LOW_CAPITAL);
    assert_eq!(after_fee.c_tot + after_fee.insurance, after_fee.vault);
    assert_domain_budget_remaining_total_consistent(&after_fee, "mixed side fee budgets");

    env.update_trade_fee_policy_with_cu(0);
    let funded_dest = env.withdraw(&funded_owner, funded, 2 * FEE);
    assert_eq!(env.token_amount(funded_dest) as u128, 2 * FEE);
    env.close_portfolio_with_cu(&low_owner, low);
    env.close_portfolio_with_cu(&funded_owner, funded);
    let terminal = env.market_state().1;
    assert_eq!(terminal.vault, terminal.insurance);
    assert_eq!(terminal.vault, 2 * FEE + LOW_CAPITAL);
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));

    MixedDirectionFeeOutcome {
        domain_budgets,
        insurance: terminal.insurance,
        terminal_vault: terminal.vault,
    }
}

#[test]
fn v16_program_mixed_direction_fee_allocation_matches_independent_side_ledger() {
    for reverse_leg_order in [false, true] {
        let expected = run_mixed_direction_fee_world(
            MixedDirectionFeePath::SequentialNoCpi,
            reverse_leg_order,
        );
        for path in MixedDirectionFeePath::ALL.into_iter().skip(1) {
            assert_eq!(
                run_mixed_direction_fee_world(path, reverse_leg_order),
                expected,
                "{path:?} must preserve the same per-side fee attribution and terminal custody; \
                 reverse_leg_order={reverse_leg_order}"
            );
        }
    }
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
                    env.batch_trade_no_cpi_ix(
                        taker_account,
                        lp_account,
                        vec![
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
                    ),
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
                    env.batch_trade_cpi_ix(
                        taker_account,
                        lp_account,
                        vec![
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
                    ),
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

// security.md sweep — fee redirect policy (#6/#33): fee_redirect_to_market_0_bps splits fees to
// market 0's domain (INTERNAL), not an external party. It must be admin-gated, bounded to <=10000,
// and must never leak value out of the protocol (vault unchanged on a fee'd trade).
#[test]
fn v16_attack_fee_redirect_gated_bounded_no_leak() {
    let mut env = V16CuEnv::new();
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    // non-admin can't set the redirect.
    env.svm.expire_blockhash();
    let r_auth = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateFeeRedirectPolicy {
            policy_sequence: u64::MAX,
            redirect_bps: 5_000,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(r_auth.is_err(), "non-admin fee redirect update must reject");
    // out-of-range redirect rejected (admin).
    env.svm.expire_blockhash();
    let r_oob = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateFeeRedirectPolicy {
            policy_sequence: u64::MAX,
            redirect_bps: 20_000,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    assert!(r_oob.is_err(), "redirect_bps > 10000 must reject");
    // valid redirect set by admin.
    env.svm.expire_blockhash();
    let r_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateFeeRedirectPolicy {
            policy_sequence: u64::MAX,
            redirect_bps: 5_000,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    assert!(
        r_ok.is_ok(),
        "admin redirect update should succeed: {:?}",
        r_ok
    );

    // a fee'd trade with redirect active: fee stays INTERNAL (vault unchanged, no external leak).
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 100);
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.vault, g0.vault,
        "fee with redirect stays internal: vault unchanged (no external leak)"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real on-chain vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
    // total value (c_tot + insurance + any domain attribution) still bounded by the vault.
    assert!(
        g1.c_tot + g1.insurance <= g1.vault,
        "no value created by the redirect split"
    );
}

// security.md sweep — fee-redirect split lands in the correct domains (#32/#33): with
// fee_redirect_to_market_0_bps set, a fee'd trade on market N must split EXACTLY: the redirect share
// to market 0's domain budget(s), the rest to market N's local domain budget(s). Total == fee
// charged (conservation), no value created/lost in the split.
#[test]
fn v16_attack_fee_redirect_split_lands_correctly() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("cfg mark");
    env.update_fee_redirect_policy_with_cu(2_000); // 20% of market 1..N fees -> market 0 domain
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 5_000_000);
    env.deposit(&lb, pb, 5_000_000);
    let dom = |env: &V16CuEnv, d: usize| env.market_state().1.insurance_domain_budget[d];
    let (b0, b1, b2, b3) = (dom(&env, 0), dom(&env, 1), dom(&env, 2), dom(&env, 3));
    let ins0 = env.market_state().1.insurance;
    // fee'd trade on ASSET 1 (market 1) -> fees split between market 0 (domains 0,1) and market 1 (2,3).
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, (10_000 * POS_SCALE) as i128, 100, 100); // notional 1M -> fee large enough for the redirect
    let (g0d, g1d, g2d, g3d) = (
        dom(&env, 0) - b0,
        dom(&env, 1) - b1,
        dom(&env, 2) - b2,
        dom(&env, 3) - b3,
    );
    let total_to_mkt0 = g0d + g1d; // domains 0,1 belong to market 0
    let total_to_mkt1 = g2d + g3d; // domains 2,3 belong to market 1
    let total_fee = total_to_mkt0 + total_to_mkt1;
    assert!(total_fee > 0, "a fee was charged (non-vacuous)");
    // global insurance grew by exactly the total fee (conservation).
    assert_eq!(
        env.market_state().1.insurance,
        ins0 + total_fee,
        "insurance += total fee"
    );
    // the redirect share (20%) landed in market 0's domains; the rest (80%) stayed local in market 1.
    // each side: redirect = floor(fee_side * 2000/10000); allow +-1 per side for flooring.
    assert!(
        total_to_mkt0 >= total_fee * 2 / 10 - 2 && total_to_mkt0 <= total_fee * 2 / 10 + 2,
        "~20% of fee redirected to market 0 (got {} of {})",
        total_to_mkt0,
        total_fee
    );
    assert!(
        total_to_mkt1 >= total_fee * 8 / 10 - 2,
        "~80% of fee stayed local in market 1 (got {})",
        total_to_mkt1
    );
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_domain_budget_remaining_total_consistent(&g, "trade fee redirect split");
}

// security.md sweep — global policy bounds (#3/#6/#37): marketauth controls cranker reward and
// fee policies, but even the authorized key must not be able to install over-100% reward shares,
// over-max trade fees, oversized permissionless-init fees, or a nonzero insurance split on a zero
// backing fee. These bad knobs can turn later public reward/top-up paths into DoS or over-credit
// surfaces. Rejected writes must leave the whole market byte-identical, and the prior bounded
// maintenance reward policy must remain live.
#[test]
fn v16_attack_global_policy_bounds_reject_grief_values() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.update_maintenance_fee_policy_with_cu(4_000);
    env.update_trade_fee_policy_with_cu(88);
    env.update_market_init_fee_policy_with_cu(40);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let reject_unchanged = |env: &mut V16CuEnv, ix: ProgInstruction, label: &str| {
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        );
        assert!(rejected.is_err(), "{label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label} must leave the market byte-identical"
        );
    };

    reject_unchanged(
        &mut env,
        ProgInstruction::UpdateLiquidationFeePolicy {
            policy_sequence: u64::MAX,
            cranker_share_bps: 10_001,
        },
        "liquidation cranker share above 100%",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::UpdateMaintenanceFeePolicy {
            policy_sequence: u64::MAX,
            cranker_share_bps: 10_001,
        },
        "maintenance cranker share above 100%",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::UpdateTradeFeePolicy {
            policy_sequence: u64::MAX,
            trade_fee_base_bps: 10_001,
        },
        "trade fee above the market maximum",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::UpdateMarketInitFeePolicy {
            policy_sequence: u64::MAX,
            min_init_fee: u128::from(u64::MAX) + 1,
        },
        "permissionless init fee that cannot fit a token transfer",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::UpdateBackingFeePolicy {
            market_id: 0,
            policy_sequence: u64::MAX,
            domain: 0,
            fee_bps: 0,
            insurance_share_bps: 1,
        },
        "nonzero backing insurance split on a zero backing fee",
    );

    let (cfg, _) = env.market_state();
    assert_eq!(cfg.liquidation_cranker_fee_share_bps, 5_000);
    assert_eq!(cfg.maintenance_cranker_fee_share_bps, 4_000);
    assert_eq!(cfg.trade_fee_base_bps, 88);
    assert_eq!(cfg.permissionless_market_init_fee, 40);
    assert_eq!(cfg.backing_trade_fee_policy_count, 0);

    let payer_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let payer_portfolio = env.create_portfolio(&payer_owner);
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.deposit(&payer_owner, payer_portfolio, 100_000_000);
    env.svm.warp_to_slot(10);
    env.sync_maintenance_fee_with_cu(payer_portfolio, Some(cranker_portfolio), 10);

    let payer = env.portfolio_state(payer_portfolio);
    let cranker = env.portfolio_state(cranker_portfolio);
    let (_, group) = env.market_state();
    assert_eq!(
        payer.capital.get(),
        100_000_000 - 580,
        "bounded policy charges exactly the elapsed maintenance fee"
    );
    assert_eq!(
        cranker.capital.get(),
        232,
        "preserved 40% maintenance cranker share still pays a real bounded reward"
    );
    assert_eq!(
        group.insurance, 348,
        "remaining maintenance fee stays in insurance"
    );
    assert_domain_budget_remaining_total_consistent(&group, "bounded global policy reward");
}

// security.md sweep — market 0 fees don't self-redirect (#32/#33) [fee-routing #2]: with
// fee_redirect_to_market_0_bps set, a fee on MARKET 0 itself must stay 100% local (the asset_index==0
// branch redirects 0). No spurious self-redirect / double-credit.
#[test]
fn v16_attack_market0_fees_stay_local() {
    let mut env = V16CuEnv::new();
    env.update_fee_redirect_policy_with_cu(2_000); // 20% redirect for markets 1..N
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 5_000_000);
    env.deposit(&lb, pb, 5_000_000);
    let dom = |env: &V16CuEnv, d: usize| env.market_state().1.insurance_domain_budget[d];
    let (b0, b1) = (dom(&env, 0), dom(&env, 1));
    let ins0 = env.market_state().1.insurance;
    // fee'd trade on ASSET 0 (market 0) with a large notional.
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, (10_000 * POS_SCALE) as i128, 100, 100);
    let g0d = dom(&env, 0) - b0;
    let g1d = dom(&env, 1) - b1;
    let total_local = g0d + g1d;
    let total_fee = env.market_state().1.insurance - ins0;
    assert!(total_fee > 0, "a fee was charged (non-vacuous)");
    // ALL of market 0's fee stayed in market 0's domains (0,1) -- nothing redirected away or double-counted.
    assert_eq!(
        total_local, total_fee,
        "100% of market-0 fee stays in market-0 domains (no self-redirect)"
    );
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_domain_budget_remaining_total_consistent(&g, "market0 trade fees stay local");
}

// security.md sweep — fee-redirect 100% boundary (#32/#33) [fee-routing #4]: with redirect=10000, ALL
// of market N's fees must route to market 0's domains and NONE stays local. Boundary precision of the
// redirect split.
#[test]
fn v16_attack_fee_redirect_full_boundary() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("cfg mark");
    env.update_fee_redirect_policy_with_cu(10_000); // 100% redirect to market 0
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 5_000_000);
    env.deposit(&lb, pb, 5_000_000);
    let dom = |env: &V16CuEnv, d: usize| env.market_state().1.insurance_domain_budget[d];
    let (b0, b1, b2, b3) = (dom(&env, 0), dom(&env, 1), dom(&env, 2), dom(&env, 3));
    let ins0 = env.market_state().1.insurance;
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, (10_000 * POS_SCALE) as i128, 100, 100);
    let to_mkt0 = (dom(&env, 0) - b0) + (dom(&env, 1) - b1);
    let to_mkt1 = (dom(&env, 2) - b2) + (dom(&env, 3) - b3);
    let total_fee = env.market_state().1.insurance - ins0;
    assert!(total_fee > 0, "fee charged (non-vacuous)");
    assert_eq!(
        to_mkt0, total_fee,
        "100% redirect: ALL of market-1 fee went to market 0"
    );
    assert_eq!(
        to_mkt1, 0,
        "100% redirect: NOTHING stayed local in market 1"
    );
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_domain_budget_remaining_total_consistent(&g, "full fee redirect boundary");
}

// security.md sweep — batch trades must not silently skip backing-domain fee accounting. Backing
// fees are collected on single-leg trades when source-credit backing grows; batch fee splitting does
// not implement that accounting yet, so both batch surfaces must reject atomically while the policy is
// active. A normal TradeNoCpi control still succeeds under the same policy, proving this is a narrow
// LoF/DoS sweep: backing-fee policy intentionally gates batch trade accounting, but it must not
// strand users who rely on the public single-fill CPI route where the LP does not co-sign. Proves
// a TradeCpi open and exit both remain live while the same active policy blocks BatchTradeCpi.

// security.md sweep — the batch-trade backing-fee gate must not become a sticky DoS. The wrapper
// keeps a global count of nonzero per-domain backing-fee policies because batch fee splitting is
// intentionally disabled while any policy is active. Updating one policy nonzero->nonzero must not
// CU/DoS hardening: CPI trade routes must reject inactive first-open assets before invoking the
// LP matcher. Matcher config can outlive lifecycle transitions; without a pre-CPI lifecycle gate,
// security.md sweep — BatchTradeCpi fee bounds on permissionless LP fills (#37/#49): in CPI mode the
// taker supplies fee_bps while the LP owner does not sign the fill. Over-max values are malformed,
// while an in-range caller value cannot raise the unsigned LP's charge above the market base fee.
#[test]
fn v16_attack_batch_cpi_fee_bps_bounded_for_permissionless_lp() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);
    let before = env.market_state().1;
    let taker_before = env.portfolio_state(taker_account);
    let lp_before = env.portfolio_state(lp_account);
    let send_fee = |env: &mut V16CuEnv, fee_bps: u64| {
        env.svm.expire_blockhash();
        env.send(
            env.batch_trade_cpi_ix(
                taker_account,
                lp_account,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: (5 * POS_SCALE) as i128,
                    fee_bps,
                    limit_price: 0,
                }],
            ),
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
    };

    for bad in [10_001u64, u64::MAX] {
        let rejected = send_fee(&mut env, bad);
        assert!(rejected.is_err(), "BatchTradeCpi fee_bps {bad} must reject");
        let group = env.market_state().1;
        assert_eq!(
            group.assets[0].oi_eff_long_q, 0,
            "no OI from rejected over-fee batch"
        );
        assert_eq!(
            group.insurance, before.insurance,
            "no fee accrued on rejected over-fee batch"
        );
        assert_eq!(
            group.c_tot, before.c_tot,
            "capital accounting unchanged by rejected over-fee batch"
        );
        assert_eq!(
            group.vault, before.vault,
            "vault unchanged by rejected over-fee batch"
        );
        assert_eq!(
            env.portfolio_state(taker_account).capital.get(),
            taker_before.capital.get(),
            "taker capital untouched"
        );
        assert_eq!(
            env.portfolio_state(lp_account).capital.get(),
            lp_before.capital.get(),
            "LP capital untouched"
        );
    }

    let ok = send_fee(&mut env, 10_000);
    assert!(
        ok.is_ok(),
        "max allowed BatchTradeCpi fee_bps should still execute: {ok:?}"
    );
    let group = env.market_state().1;
    assert_eq!(
        group.insurance, before.insurance,
        "an in-range caller fee cannot charge the unsigned LP"
    );
    assert_eq!(env.portfolio_state(taker_account).capital.get(), 1_000_000);
    assert_eq!(env.portfolio_state(lp_account).capital.get(), 1_000_000);
    assert_eq!(group.vault, before.vault, "caller fee moves no custody");
    assert!(
        group.vault >= group.c_tot + group.insurance,
        "senior conservation"
    );
}

// CU/DoS hardening: the two legal BatchTradeCpi fanout dimensions compose badly at the 10MiB
// high-asset boundary. A 14-leg high-tail batch fits by itself, and a small batch may use the full
// 32-account matcher tail, but combining 14 high-tail legs with the full tail exhausts the 1.4M
// transaction meter. The wrapper must reject that combination before matcher CPI while preserving
// Isolation: per-leg fees in an atomic batch are credited to the CORRECT asset's insurance domains,
// never aggregated or cross-credited across assets. The batch path reconstructs per-leg fees out of
// the engine's AGGREGATE outcome (unlike single trades), so this guards that reconstruction against
// cross-asset fee leakage. Two legs with different fee_bps must move each asset's own domain budget
// by its own fee.
#[test]
fn v16_attack_batch_fees_isolated_to_each_asset_domain() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 10_000_000);
    env.deposit(&lp, la, 10_000_000);
    let before = env.market_state().1;
    let b0 = before.insurance_domain_budget[0] + before.insurance_domain_budget[1];
    let b1 = before.insurance_domain_budget[2] + before.insurance_domain_budget[3];
    let sz = (10 * POS_SCALE) as i128;
    env.send(
        env.batch_trade_no_cpi_ix(
            ta,
            la,
            vec![
                BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: sz,
                    exec_price: 100,
                    fee_bps: 100,
                },
                BatchTradeLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id((1) as u16),
                    size_q: sz,
                    exec_price: 100,
                    fee_bps: 500,
                },
            ],
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
        ],
        &[&taker, &lp],
    )
    .expect("batch");
    let after = env.market_state().1;
    let a0 = (after.insurance_domain_budget[0] + after.insurance_domain_budget[1]) - b0;
    let a1 = (after.insurance_domain_budget[2] + after.insurance_domain_budget[3]) - b1;
    assert!(
        a0 > 0 && a1 > 0,
        "each asset's own domain budget moved by its own fee"
    );
    assert!(
        a1 > a0,
        "asset1 (fee_bps 500) credit {a1} > asset0 (fee_bps 100) credit {a0}: per-asset, not aggregated/cross-credited"
    );
    assert_domain_budget_remaining_total_consistent(&after, "batch fee domain isolation");
}

// CU/DoS hardening: BatchTradeCpi must reject impossible caller fee_bps before invoking a matcher.
// The single-fill CPI path checks max(caller_fee_bps, trade_fee_base_bps) before CPI; batch CPI must
// do the same. The hostile over-fill matcher is the sentinel: a valid-fee call reaches matcher-return
// validation and fails InvalidAccountData, while an over-fee call must fail InvalidInstruction first.
#[test]
fn v16_attack_batch_tradecpi_fee_bps_rejects_before_hostile_matcher_cpi() {
    let mut env = V16CuEnv::new();
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);

    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &la,
        &lp.pubkey(),
        &hostile,
        &ctx,
    );
    env.svm
        .set_account(
            delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; MATCHER_CONTEXT_LEN],
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.set_matcher_config(hostile, &lp, la, ctx, delegate, 1);

    let send_fee = |env: &mut V16CuEnv, fee_bps: u64| {
        let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
        data[0] = 0; // hostile over-fill mode: if CPI occurs, validation fails InvalidAccountData.
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm.expire_blockhash();
        env.send(
            env.batch_trade_cpi_ix(
                ta,
                la,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: (5 * POS_SCALE) as i128,
                    fee_bps,
                    limit_price: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
                AccountMeta::new_readonly(hostile, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        )
    };

    let valid_fee_err = send_fee(&mut env, 100)
        .expect_err("valid-fee hostile batch should reach matcher-return validation");
    assert!(
        valid_fee_err.contains("InvalidAccountData"),
        "valid-fee hostile sentinel must fail from matcher-return validation, got {valid_fee_err}"
    );
    assert!(
        !valid_fee_err.contains("Custom(9)"),
        "valid-fee sentinel must not trip the fee preflight: {valid_fee_err}"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    for bad_fee in [10_001u64, u64::MAX] {
        let rejected = send_fee(&mut env, bad_fee)
            .expect_err("over-fee BatchTradeCpi must reject before matcher CPI");
        assert!(
            rejected.contains("Custom(9)"),
            "over-fee BatchTradeCpi must fail as InvalidInstruction before hostile matcher validation, got {rejected}"
        );
        assert!(
            !rejected.contains("InvalidAccountData"),
            "over-fee BatchTradeCpi must not reach hostile matcher validation: {rejected}"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "over-fee preflight leaves market bytes unchanged"
        );
        assert_eq!(
            env.svm.get_account(&ta).unwrap(),
            taker_before,
            "over-fee preflight leaves taker bytes unchanged"
        );
        assert_eq!(
            env.svm.get_account(&la).unwrap(),
            lp_before,
            "over-fee preflight leaves LP bytes unchanged"
        );
    }
}

// security.md sweep — permissionless create fee funds asset-0 insurance (#5 / README L59): the fee a
// stranger pays to permissionlessly create asset N flows entirely into asset-0's insurance (the market
// insurance pool + asset-0's per-domain budgets), conserving every atom.
#[test]
fn v16_attack_permissionless_create_fee_funds_asset0_insurance() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);
    env.svm.warp_to_slot(1);
    let (_, before) = env.market_state();
    let stranger = Keypair::new();
    let ins_auth = Keypair::new();
    let admin_pk = env.admin.pubkey();
    let (fee_src, _cu) = env.activate_permissionless_asset_with_fee(
        &stranger,
        1,
        1,
        100,
        ins_auth.pubkey(),
        ins_auth.pubkey(),
        ins_auth.pubkey(),
        admin_pk,
        FEE,
    );
    let (_, after) = env.market_state();
    assert_eq!(env.token_amount(fee_src), 0, "creator's fee fully pulled");
    assert_eq!(
        after.vault - before.vault,
        FEE,
        "fee deposited into the vault"
    );
    assert_eq!(
        after.insurance - before.insurance,
        FEE,
        "entire create fee funds asset-0 insurance pool"
    );
    let b0 = after.insurance_domain_budget[0] - before.insurance_domain_budget[0];
    let b1 = after.insurance_domain_budget[1] - before.insurance_domain_budget[1];
    assert_eq!(
        b0 + b1,
        FEE,
        "fee earmarked into asset-0's insurance domains (0=long,1=short), no leakage"
    );
    assert_eq!(after.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert!(
        after.vault >= after.c_tot + after.insurance,
        "senior conservation"
    );
    assert_domain_budget_remaining_total_consistent(&after, "permissionless create fee");
}
