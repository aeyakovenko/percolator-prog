//! INV-062 - no identity assumptions; self-trade containment is economic.
//!
//! The protocol must remain solvent when both sides are controlled by the same
//! signer. These public no-CPI tests use one owner for both portfolios and assert
//! that self-trading creates no free value: zero-fee round trips preserve capital,
//! and fee-bearing self-trades only move value from the trader pair into protocol
//! insurance while keeping custody exactly reconciled. The route/mode matrix below
//! repeats that terminal ledger check for single and batch CPI/no-CPI trades in
//! AuthMark, EwmaMark, and stale-hybrid operation. Paid off-mark coalition attacks
//! are independently exercised by INV-045's fee-reserve and liquidation-reward
//! models; this file owns the identity-independence and terminal-custody assertion.

use super::*;

fn active_leg_count(account: &PortfolioAccountV16) -> usize {
    account
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active)
        .count()
}

#[derive(Clone, Copy, Debug)]
enum CommonControlRoute {
    NoCpi,
    BatchNoCpi,
    Cpi,
    BatchCpi,
}

#[derive(Clone, Copy, Debug)]
enum CommonControlMarkMode {
    Auth,
    Ewma,
    HybridAfterHours,
}

#[derive(Clone, Copy)]
struct CommonControlMatcher {
    program: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
}

fn configure_common_control_mark(env: &mut V16CuEnv, mode: CommonControlMarkMode, mark: u64) {
    match mode {
        CommonControlMarkMode::Auth => {
            env.svm.warp_to_slot(1);
            env.configure_auth_mark_with_cu(1, mark);
        }
        CommonControlMarkMode::Ewma => {
            env.svm.warp_to_slot(1);
            env.configure_ewma_mark_with_cu(1, mark, 1, 0);
            env.svm.warp_to_slot(2);
        }
        CommonControlMarkMode::HybridAfterHours => {
            set_test_clock(env, 1, 100);
            let feed = [0x62; 32];
            let pyth = env.set_pyth_price(&feed, mark as i64, -6, 100);
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                0,
                1,
                0,
                [feed, [0; 32], [0; 32]],
                &[pyth],
                1,
                100,
                0,
                0,
                1,
                0,
            )
            .expect("configure hybrid mark for common-control matrix");
            set_test_clock(env, 3, 1_000);
        }
    }
}

fn execute_common_control_trade(
    env: &mut V16CuEnv,
    route: CommonControlRoute,
    owner: &Keypair,
    account_a: Pubkey,
    account_b: Pubkey,
    matcher: Option<CommonControlMatcher>,
    size_q: i128,
    exec_price: u64,
    fee_bps: u64,
) -> u64 {
    match route {
        CommonControlRoute::NoCpi => env
            .try_trade_asset_with_cu(
                0, owner, account_a, owner, account_b, size_q, exec_price, fee_bps,
            )
            .expect("same-owner TradeNoCpi must execute"),
        CommonControlRoute::BatchNoCpi => env
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
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(account_a, false),
                    AccountMeta::new(account_b, false),
                ],
                &[owner],
            )
            .expect("same-owner BatchTradeNoCpi must execute"),
        CommonControlRoute::Cpi => {
            let matcher = matcher.expect("CPI route matcher");
            env.try_trade_cpi_with_cu_on_asset(
                owner,
                account_a,
                owner,
                account_b,
                matcher.program,
                matcher.context,
                matcher.delegate,
                0,
                size_q,
                fee_bps,
            )
            .expect("same-owner TradeCpi must execute")
        }
        CommonControlRoute::BatchCpi => {
            let matcher = matcher.expect("batch CPI route matcher");
            env.send(
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
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(account_a, false),
                    AccountMeta::new(account_b, false),
                    AccountMeta::new_readonly(matcher.program, false),
                    AccountMeta::new(matcher.context, false),
                    AccountMeta::new_readonly(matcher.delegate, false),
                ],
                &[owner],
            )
            .expect("same-owner BatchTradeCpi must execute")
        }
    }
}

#[test]
fn v16_program_common_control_round_trip_is_conserved_across_routes_and_mark_modes() {
    const MARK: u64 = 1_000_000;
    const DEPOSIT: u128 = 100_000_000;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const FEE_BPS: u64 = 100;

    for mode in [
        CommonControlMarkMode::Auth,
        CommonControlMarkMode::Ewma,
        CommonControlMarkMode::HybridAfterHours,
    ] {
        for route in [
            CommonControlRoute::NoCpi,
            CommonControlRoute::BatchNoCpi,
            CommonControlRoute::Cpi,
            CommonControlRoute::BatchCpi,
        ] {
            let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
                initial_price: MARK,
                trade_fee_base_bps: FEE_BPS,
                ..V16CuMarketParams::default()
            });
            configure_common_control_mark(&mut env, mode, MARK);

            let owner = Keypair::new();
            let account_a = env.create_portfolio(&owner);
            let account_b = env.create_portfolio(&owner);
            env.deposit(&owner, account_a, DEPOSIT);
            env.deposit(&owner, account_b, DEPOSIT);

            let matcher = if matches!(
                route,
                CommonControlRoute::Cpi | CommonControlRoute::BatchCpi
            ) {
                let program = Pubkey::new_unique();
                let matcher_bytes =
                    std::fs::read(matcher_program_path()).expect("read matcher BPF");
                env.svm.add_program(program, &matcher_bytes);
                let (context, delegate, _) = env
                    .init_matcher_context_with_passive_spread_authorized(
                        program, &owner, account_b, 0, 0,
                    );
                Some(CommonControlMatcher {
                    program,
                    context,
                    delegate,
                })
            } else {
                None
            };

            let vault_before = env.token_amount(env.vault);
            assert_eq!(u128::from(vault_before), 2 * DEPOSIT);
            execute_common_control_trade(
                &mut env, route, &owner, account_a, account_b, matcher, SIZE_Q, MARK, FEE_BPS,
            );
            execute_common_control_trade(
                &mut env, route, &owner, account_a, account_b, matcher, -SIZE_Q, MARK, FEE_BPS,
            );

            let account_a_state = env.portfolio_state(account_a);
            let account_b_state = env.portfolio_state(account_b);
            let (_, group) = env.market_state();
            let coalition_capital = account_a_state
                .capital
                .get()
                .checked_add(account_b_state.capital.get())
                .expect("coalition capital sum");

            assert_eq!(account_a_state.pnl.get(), 0, "{mode:?} {route:?}");
            assert_eq!(account_b_state.pnl.get(), 0, "{mode:?} {route:?}");
            assert_eq!(active_leg_count(&account_a_state), 0, "{mode:?} {route:?}");
            assert_eq!(active_leg_count(&account_b_state), 0, "{mode:?} {route:?}");
            assert_eq!(group.assets[0].oi_eff_long_q, 0, "{mode:?} {route:?}");
            assert_eq!(group.assets[0].oi_eff_short_q, 0, "{mode:?} {route:?}");
            assert!(
                group.insurance > 0,
                "{mode:?} {route:?} must charge real fees"
            );
            assert_eq!(group.c_tot, coalition_capital, "{mode:?} {route:?}");
            assert_eq!(
                coalition_capital + group.insurance,
                2 * DEPOSIT,
                "{mode:?} {route:?} common control cannot create or redirect value",
            );
            assert_eq!(group.vault, 2 * DEPOSIT, "{mode:?} {route:?}");
            assert_eq!(
                u128::from(env.token_amount(env.vault)),
                group.vault,
                "{mode:?} {route:?} internal custody must equal real SPL custody",
            );

            let (destination_a, _) =
                env.withdraw_with_cu(&owner, account_a, account_a_state.capital.get());
            let (destination_b, _) =
                env.withdraw_with_cu(&owner, account_b, account_b_state.capital.get());
            assert_eq!(
                u128::from(env.token_amount(destination_a))
                    + u128::from(env.token_amount(destination_b)),
                coalition_capital,
                "{mode:?} {route:?} owner can recover every remaining capital atom",
            );
            let (_, terminal) = env.market_state();
            assert_eq!(terminal.c_tot, 0, "{mode:?} {route:?}");
            assert_eq!(terminal.vault, terminal.insurance, "{mode:?} {route:?}");
            assert_eq!(
                u128::from(env.token_amount(env.vault)),
                terminal.insurance,
                "{mode:?} {route:?} only the conserved protocol fee remains",
            );
        }
    }
}

#[test]
fn v16_program_same_owner_zero_fee_self_trade_round_trip_creates_no_value() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let long = env.create_portfolio(&owner);
    let short = env.create_portfolio(&owner);
    env.deposit(&owner, long, 500_000);
    env.deposit(&owner, short, 500_000);

    let initial_total_capital =
        env.portfolio_state(long).capital.get() + env.portfolio_state(short).capital.get();
    env.trade_asset_with_cu(
        0,
        &owner,
        long,
        &owner,
        short,
        1_000 * POS_SCALE as i128,
        100,
        0,
    );
    let (_, opened) = env.market_state();
    assert_eq!(opened.vault, initial_total_capital);
    assert_eq!(opened.c_tot, initial_total_capital);
    assert_eq!(opened.insurance, 0);
    assert_eq!(
        opened.assets[0].oi_eff_long_q,
        opened.assets[0].oi_eff_short_q
    );

    env.trade_asset_with_cu(
        0,
        &owner,
        long,
        &owner,
        short,
        -(1_000 * POS_SCALE as i128),
        100,
        0,
    );
    let (_, closed) = env.market_state();
    let long_state = env.portfolio_state(long);
    let short_state = env.portfolio_state(short);
    assert_eq!(closed.vault, initial_total_capital);
    assert_eq!(closed.c_tot, initial_total_capital);
    assert_eq!(closed.insurance, 0);
    assert_eq!(closed.assets[0].oi_eff_long_q, 0);
    assert_eq!(closed.assets[0].oi_eff_short_q, 0);
    assert_eq!(active_leg_count(&long_state), 0);
    assert_eq!(active_leg_count(&short_state), 0);
    assert_eq!(
        long_state.capital.get() + short_state.capital.get(),
        initial_total_capital,
        "self-controlled round trip cannot mint or burn user capital",
    );
}

#[test]
fn v16_program_same_owner_fee_self_trade_is_negative_sum_not_profitable() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let long = env.create_portfolio(&owner);
    let short = env.create_portfolio(&owner);
    env.deposit(&owner, long, 1_000_000);
    env.deposit(&owner, short, 1_000_000);

    let initial_total_capital =
        env.portfolio_state(long).capital.get() + env.portfolio_state(short).capital.get();
    env.trade_asset_with_cu(
        0,
        &owner,
        long,
        &owner,
        short,
        1_000 * POS_SCALE as i128,
        100,
        100,
    );

    let (_, group) = env.market_state();
    let final_total_capital =
        env.portfolio_state(long).capital.get() + env.portfolio_state(short).capital.get();
    assert_eq!(group.vault, initial_total_capital);
    assert_eq!(
        group.vault,
        group.c_tot + group.insurance,
        "self-trade fee remains an internal, conserved transfer",
    );
    assert_eq!(
        initial_total_capital - final_total_capital,
        group.insurance,
        "all value lost by the self-controlled pair is retained as protocol insurance",
    );
    assert!(
        final_total_capital < initial_total_capital,
        "fee-bearing self-trade is EV-negative for the common owner",
    );
}

// security.md sweep — TradeCpi self-trade (#49 wash): taker == maker (same portfolio) on the matcher
// CPI path must reject like TradeNoCpi self-trade — no wash position / OI fabrication.
#[test]
fn v16_attack_tradecpi_self_trade_rejected() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let owner = Keypair::new();
    let acct = env.create_portfolio(&owner);
    env.deposit(&owner, acct, 1_000_000);
    let (ctx, delegate, _) = env.init_matcher_context(matcher_program, acct);
    let (_, g0) = env.market_state();
    // taker == maker == acct.
    env.svm.expire_blockhash();
    let r = env.send(
        env.trade_cpi_ix(acct, acct, 0, (10 * POS_SCALE) as i128, 100, 0),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(acct, false),
            AccountMeta::new(acct, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&owner],
    );
    assert!(r.is_err(), "TradeCpi self-trade (taker==maker) must reject");
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.assets[0].oi_eff_long_q, 0,
        "no OI fabricated by self-trade"
    );
    assert_eq!(
        env.portfolio_state(acct).legs[0].basis_pos_q.get(),
        0,
        "no wash position"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
}

// security.md sweep - stale-resolve abandoned-asset drift (#30/#35/#48): ForceCloseAbandonedAsset
// is intentionally unsigned after a shutdown timeout, but it still mutates live exposure and both
// portfolios. Once the base market is resolve-matured, an abandoned-asset force-close must freeze
// security.md sweep — off-market exec_price wash trade (#9/#22/#33): exec_price is validated only as
// 0 < exec_price <= MAX_ORACLE_PRICE (NOT clamped to the oracle), so two colluding accounts can open a
// position at a price far from the mark, handing one side an instant profit. Attacker goal: print
// withdrawable value out of the off-market gap. Protection: the off-market profit settles as JUNIOR pnl
// backed only by the counterparty's realized loss (residual), and the vault is never minted into —
// strictly zero-sum. Asserts no value creation across the off-market open + crank.
#[test]
fn v16_attack_off_market_exec_price_wash_trade_prints_nothing() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100); // oracle/mark = 100
    let oa = Keypair::new();
    let a = env.create_portfolio(&oa); // both controlled by one attacker
    let ob = Keypair::new();
    let b = env.create_portfolio(&ob);
    let d: u128 = 1_000_000;
    env.deposit(&oa, a, d);
    env.deposit(&ob, b, d);
    let (_, g0) = env.market_state();
    assert_eq!(g0.vault, 2 * d, "vault holds exactly both deposits");

    // OFF-MARKET OPEN: a goes LONG at exec_price=50 while the mark is 100 -> a is handed an instant
    // (100-50)*size paper profit; b (short) takes the symmetric loss. Far below the mark on purpose.
    let size = POS_SCALE as i128;
    let r = env.try_trade_asset_with_cu(0, &oa, a, &ob, b, size, 50, 0);
    // Either the post-trade margin check rejects the lopsided open (b can't cover) -> nothing printed,
    // or it succeeds and the profit is junior. Drive settlement either way and check the invariants.
    for p in [a, b] {
        env.svm.warp_to_slot(1);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(p, false),
            ],
            &[],
        );
    }
    let g = env.market_state().1;

    // INVARIANT 1: no tokens were minted — the vault still holds exactly the two deposits.
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real on-chain vault"
    );
    assert_eq!(g.vault, 2 * d, "off-market trade minted no vault tokens");
    // INVARIANT 2: senior conservation — junior (positive) pnl is fully backed by residual, never senior.
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation: residual backs junior pnl"
    );
    // INVARIANT 3: withdrawable capital was never inflated past the deposits by the paper profit.
    assert!(
        g.c_tot <= 2 * d,
        "no phantom capital minted (c_tot <= total deposited)"
    );
    // INVARIANT 4: any junior positive-pnl claim is bounded by the residual that actually backs it.
    let residual = g.vault.saturating_sub(g.c_tot).saturating_sub(g.insurance);
    assert!(
        g.pnl_pos_tot <= residual + 1,
        "junior positive-pnl claim bounded by residual (no over-claim)"
    );
    if r.is_err() {
        // lopsided open rejected outright: nothing moved at all.
        assert_eq!(
            g.c_tot,
            2 * d,
            "rejected off-market open left both capitals intact"
        );
    }
}

// security.md sweep — TradeNoCpi self-trade (#49 wash): a direct trade with account_a == account_b (the
// same portfolio trading with itself) must reject — the wash-trade guard on the NON-matcher path
// (src/v16_program.rs:5992), complementing the CPI version (#9882). Attacker goal: fabricate OI / a wash
// position / churn fees against oneself. Protection: same-account trade rejects, no OI, no fee, intact.
#[test]
fn v16_attack_tradenocpi_self_trade_rejected() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();

    // TradeNoCpi with the SAME portfolio (and owner) on both sides.
    env.svm.expire_blockhash();
    let r = env.send(
        env.trade_no_cpi_ix(p, p, 0, POS_SCALE as i128, 100, 100),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(p, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "TradeNoCpi self-trade (account_a == account_b) must reject"
    );

    // no wash position / OI / fee created; portfolio + vault intact.
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.assets[0].oi_eff_long_q, 0,
        "no OI fabricated by self-trade"
    );
    assert_eq!(
        g1.insurance, g0.insurance,
        "no fee churned by a rejected self-trade"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(p))),
        "no position opened"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "capital intact"
    );
}

#[test]
fn v16_attack_batch_trade_self_trade_rejected() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let batch_leg = BatchTradeLeg {
        asset_index: 0,
        market_id: first_generation_market_id((0) as u16),
        size_q: POS_SCALE as i128,
        exec_price: 100,
        fee_bps: 100,
    };

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&p).unwrap();
    env.svm.expire_blockhash();
    let direct = env.send(
        env.batch_trade_no_cpi_ix(p, p, vec![batch_leg]),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(p, false),
        ],
        &[&owner],
    );
    assert!(direct.is_err(), "BatchTradeNoCpi self-trade must reject");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "direct batch self-trade must not mutate market state"
    );
    assert_eq!(
        env.svm.get_account(&p).unwrap(),
        portfolio_before,
        "direct batch self-trade must not mutate the portfolio"
    );

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (ctx, delegate, _) = env.init_matcher_context_authorized(matcher_program, &owner, p);
    let market_before_cpi = env.svm.get_account(&env.market).unwrap();
    let portfolio_before_cpi = env.svm.get_account(&p).unwrap();
    let matcher_before = env.svm.get_account(&ctx).unwrap();
    env.svm.expire_blockhash();
    let cpi = env.send(
        env.batch_trade_cpi_ix(
            p,
            p,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: POS_SCALE as i128,
                fee_bps: 100,
                limit_price: 0,
            }],
        ),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(p, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&owner],
    );
    assert!(
        cpi.is_err(),
        "BatchTradeCpi self-trade must reject before matcher CPI"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_cpi,
        "CPI batch self-trade must not mutate market state"
    );
    assert_eq!(
        env.svm.get_account(&p).unwrap(),
        portfolio_before_cpi,
        "CPI batch self-trade must not mutate the portfolio"
    );
    assert_eq!(
        env.svm.get_account(&ctx).unwrap(),
        matcher_before,
        "CPI batch self-trade must reject before the matcher context is touched"
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.assets[0].oi_eff_long_q, 0,
        "no OI fabricated by batch self-trades"
    );
    assert_eq!(
        group.insurance, 0,
        "no fee churned by rejected batch self-trades"
    );
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(p)
    )));
    assert_eq!(env.portfolio_state(p).capital.get(), 1_000_000);
}
