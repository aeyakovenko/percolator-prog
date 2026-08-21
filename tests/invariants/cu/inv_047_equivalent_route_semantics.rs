//! INV-047 - Equivalent-route semantics.
//!
//! Normative obligation: Economically equivalent public routes have equivalent normalized state deltas.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): empty-target crank
//! equivalence, batch end-state margin protection, and exact normalized sequential/batch position
//! semantics across clear, lower-slot flip reuse, attach, and resize in one route. These tests
//! exercise the deployed public wrapper with real SBF/LiteSVM account construction and assert
//! economic state, token, rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[derive(Debug, PartialEq, Eq)]
struct OneLegRouteSnapshot {
    vault: u128,
    c_tot: u128,
    insurance: u128,
    oi_eff_long_q: u128,
    oi_eff_short_q: u128,
    account_a_capital: u128,
    account_b_capital: u128,
    account_a_basis_q: i128,
    account_b_basis_q: i128,
    account_a_pnl: i128,
    account_b_pnl: i128,
}

#[derive(Debug, PartialEq, Eq)]
struct MixedPositionRouteSnapshot {
    vault: u128,
    c_tot: u128,
    insurance: u128,
    oi: [(u128, u128); 4],
    account_a_capital: u128,
    account_b_capital: u128,
    account_a_pnl: i128,
    account_b_pnl: i128,
    account_a_legs: Vec<(usize, u32, i128)>,
    account_b_legs: Vec<(usize, u32, i128)>,
}

fn active_route_legs(account: &PortfolioAccountV16) -> Vec<(usize, u32, i128)> {
    account
        .legs
        .iter()
        .enumerate()
        .filter_map(|(slot, leg)| {
            let leg = leg.try_to_runtime().ok()?;
            leg.active
                .then_some((slot, leg.asset_index, leg.basis_pos_q))
        })
        .collect()
}

fn mixed_position_route_snapshot(batch: bool) -> MixedPositionRouteSnapshot {
    const ASSET_COUNT: u16 = 4;
    const PRICE: u64 = 1_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: ASSET_COUNT,
        initial_price: PRICE,
        ..V16CuMarketParams::default()
    });
    for asset_index in 0..ASSET_COUNT {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 0, PRICE);
    }

    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, 100_000_000);
    env.deposit(&owner_b, account_b, 100_000_000);
    for (asset_index, size_q) in [
        (0, 2 * POS_SCALE as i128),
        (1, POS_SCALE as i128),
        (2, POS_SCALE as i128),
    ] {
        env.trade_asset_with_cu(
            asset_index,
            &owner_a,
            account_a,
            &owner_b,
            account_b,
            size_q,
            PRICE,
            0,
        );
    }

    let route_legs = [
        (1, -(POS_SCALE as i128)),
        (2, -(2 * POS_SCALE as i128)),
        (3, POS_SCALE as i128),
        (0, -(POS_SCALE as i128)),
    ];
    if batch {
        let legs = route_legs
            .iter()
            .map(|(asset_index, size_q)| BatchTradeLeg {
                asset_index: *asset_index,
                market_id: first_generation_market_id(*asset_index),
                size_q: *size_q,
                exec_price: PRICE,
                fee_bps: 0,
            })
            .collect();
        env.svm.expire_blockhash();
        env.send(
            env.batch_trade_no_cpi_ix(account_a, account_b, legs),
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[&owner_a, &owner_b],
        )
        .expect("mixed clear/flip/attach/resize batch");
    } else {
        for (asset_index, size_q) in route_legs {
            env.trade_asset_with_cu(
                asset_index,
                &owner_a,
                account_a,
                &owner_b,
                account_b,
                size_q,
                PRICE,
                0,
            );
        }
    }

    let group = env.market_state().1;
    let account_a = env.portfolio_state(account_a);
    let account_b = env.portfolio_state(account_b);
    MixedPositionRouteSnapshot {
        vault: group.vault,
        c_tot: group.c_tot,
        insurance: group.insurance,
        oi: core::array::from_fn(|asset_index| {
            (
                group.assets[asset_index].oi_eff_long_q,
                group.assets[asset_index].oi_eff_short_q,
            )
        }),
        account_a_capital: account_a.capital.get(),
        account_b_capital: account_b.capital.get(),
        account_a_pnl: account_a.pnl.get(),
        account_b_pnl: account_b.pnl.get(),
        account_a_legs: active_route_legs(&account_a),
        account_b_legs: active_route_legs(&account_b),
    }
}

fn route_basis_for_asset(account: &PortfolioAccountV16, asset_index: usize) -> i128 {
    account
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index as usize == asset_index)
        .map(|leg| leg.basis_pos_q)
        .unwrap_or(0)
}

fn one_leg_trade_route_snapshot(batch: bool) -> OneLegRouteSnapshot {
    let mut env = V16CuEnv::new();
    let owner_a = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let owner_b = Keypair::new();
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, 1_000_000);
    env.deposit(&owner_b, account_b, 1_000_000);
    let size_q = (7 * POS_SCALE) as i128;
    let fee_bps = 333;

    if batch {
        env.svm.expire_blockhash();
        let accepted = env.send(
            env.batch_trade_no_cpi_ix(
                account_a,
                account_b,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q,
                    exec_price: 100,
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
            &[&owner_a, &owner_b],
        );
        assert!(
            accepted.is_ok(),
            "one-leg batch trade should execute: {accepted:?}"
        );
    } else {
        env.trade_asset_with_cu(
            0, &owner_a, account_a, &owner_b, account_b, size_q, 100, fee_bps,
        );
    }

    let (_, group) = env.market_state();
    let account_a_state = env.portfolio_state(account_a);
    let account_b_state = env.portfolio_state(account_b);
    OneLegRouteSnapshot {
        vault: group.vault,
        c_tot: group.c_tot,
        insurance: group.insurance,
        oi_eff_long_q: group.assets[0].oi_eff_long_q,
        oi_eff_short_q: group.assets[0].oi_eff_short_q,
        account_a_capital: account_a_state.capital.get(),
        account_b_capital: account_b_state.capital.get(),
        account_a_basis_q: route_basis_for_asset(&account_a_state, 0),
        account_b_basis_q: route_basis_for_asset(&account_b_state, 0),
        account_a_pnl: account_a_state.pnl.get(),
        account_b_pnl: account_b_state.pnl.get(),
    }
}

#[test]
fn v16_program_one_leg_batch_nocpi_matches_single_nocpi_fee_trade() {
    let single = one_leg_trade_route_snapshot(false);
    let batch = one_leg_trade_route_snapshot(true);
    assert_eq!(
        batch, single,
        "a one-leg BatchTradeNoCpi must preserve the exact economic delta of TradeNoCpi, including fees",
    );
}

#[test]
fn v16_program_unique_batch_position_plan_matches_sequential_route_and_slot_semantics() {
    let sequential = mixed_position_route_snapshot(false);
    let batch = mixed_position_route_snapshot(true);
    assert_eq!(batch, sequential);
    assert_eq!(
        batch.account_a_legs,
        vec![
            (0, 0, POS_SCALE as i128),
            (1, 2, -(POS_SCALE as i128)),
            (2, 3, POS_SCALE as i128)
        ],
        "clear frees slot 1, flip reuses the lowest slot, and attach takes slot 2"
    );
}

#[test]
fn v16_audit_empty_target_oracle_crank_matches_exposed_target_settlement() {
    fn run(commit_through_empty: bool) -> (u128, i128, u128, i128, [u64; 2], u128, u128) {
        const OPEN_PRICE: u64 = 1_000_000;
        const SETTLE_PRICE: u64 = 1_100_000;

        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
        env.configure_auth_mark_with_cu(0, OPEN_PRICE);

        let long_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short_owner = Keypair::new();
        let short = env.create_portfolio(&short_owner);
        let empty_owner = Keypair::new();
        let empty = env.create_portfolio(&empty_owner);
        env.deposit(&long_owner, long, 2_000_000);
        env.deposit(&short_owner, short, 2_000_000);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        );

        env.svm.warp_to_slot(1);
        env.push_auth_mark_with_cu(1, SETTLE_PRICE);
        let first_target = if commit_through_empty { empty } else { long };
        env.svm.expire_blockhash();
        let first = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(first_target, false),
            ],
            &[],
        );
        assert!(
            first.is_ok(),
            "authenticated mark must commit through either valid target: {first:?}"
        );
        assert_eq!(
            env.market_state().1.assets[0].effective_price,
            SETTLE_PRICE,
            "the observation advances the exposed market even when the target portfolio is empty"
        );

        for target in [long, short] {
            env.svm.expire_blockhash();
            let refresh = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 1,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(target, false),
                ],
                &[],
            );
            assert!(
                refresh.is_ok(),
                "both exposed accounts retain a bounded no-observation refresh: {refresh:?}"
            );
        }

        let long_after = env.portfolio_state(long);
        let short_after = env.portfolio_state(short);
        assert_eq!(long_after.pnl.get(), 100_000);
        assert_eq!(long_after.capital.get(), 2_000_000);
        assert_eq!(short_after.pnl.get(), 0);
        assert_eq!(
            short_after.capital.get(),
            1_900_000,
            "the adverse PnL is crystallized from short principal during refresh"
        );

        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            -(POS_SCALE as i128),
            SETTLE_PRICE,
            0,
        );
        env.resolve();
        let long_dest = env.close_resolved(&long_owner, long);
        let short_dest = env.close_resolved(&short_owner, short);
        let payouts = [env.token_amount(long_dest), env.token_amount(short_dest)];
        let (_, group) = env.market_state();
        (
            long_after.capital.get(),
            long_after.pnl.get(),
            short_after.capital.get(),
            short_after.pnl.get(),
            payouts,
            group.vault,
            group.c_tot,
        )
    }

    let exposed_target = run(false);
    let empty_target = run(true);
    assert_eq!(
        empty_target, exposed_target,
        "wrapper pre-accrual through an empty target must not change settlement or terminal value"
    );
    assert_eq!(empty_target.4, [2_100_000, 1_900_000]);
    assert_eq!(empty_target.5, 0);
    assert_eq!(empty_target.6, 0);
}

// engine certifies BOTH accounts (long and short) on the final portfolio, so the batch reverts.
#[test]
fn v16_program_batch_cannot_force_counterparty_underwater() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 100_000_000); // taker funded
    env.deposit(&lp, la, 50); // LP cannot margin a large short
    let sz = (50 * POS_SCALE) as i128; // notional 5000, IM 500 >> LP capital 50
    let res = env.send(
        env.batch_trade_no_cpi_ix(
            ta,
            la,
            vec![
                BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: sz,
                    exec_price: 100,
                    fee_bps: 0,
                },
                BatchTradeLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id((1) as u16),
                    size_q: sz,
                    exec_price: 100,
                    fee_bps: 0,
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
    );
    assert!(
        res.is_err(),
        "batch must reject when the LP cannot meet end-state initial margin (no LOF)"
    );
}

// security.md sweep — TradeCpi zero-fill (#39): a zero-capacity matcher (max_fill_abs=0) returns
// exec_size=0. The wrapper must handle it cleanly — reject or no-op — never create phantom OI/basis,
// charge a fee on nothing, or corrupt conservation.
#[test]
fn v16_attack_tradecpi_zero_fill_is_clean() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new();
    let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&maker_owner, maker, 1_000_000);
    // matcher with ZERO fill capacity.
    let (ctx, delegate, _) = env.init_matcher_context_with_data_authorized(
        matcher_program,
        &maker_owner,
        maker,
        encode_matcher_init_passive(0),
    );
    let (_, g0) = env.market_state();
    env.svm.expire_blockhash();
    let r = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &maker_owner,
        maker,
        matcher_program,
        ctx,
        delegate,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    // whether reject or clean no-op: no OI, no basis, no fee, conservation intact.
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.assets[0].oi_eff_long_q, 0,
        "no phantom long OI from zero-fill"
    );
    assert_eq!(
        g1.assets[0].oi_eff_short_q, 0,
        "no phantom short OI from zero-fill"
    );
    assert_eq!(
        env.portfolio_state(taker).legs[0].basis_pos_q.get(),
        0,
        "taker has no basis from zero-fill"
    );
    assert_eq!(
        env.portfolio_state(maker).legs[0].basis_pos_q.get(),
        0,
        "maker has no basis from zero-fill"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(
        g1.c_tot, g0.c_tot,
        "c_tot unchanged (no fee charged on nothing)"
    );
    assert_eq!(g1.insurance, g0.insurance, "no fee accrued on a zero fill");
    assert_eq!(g1.vault, g1.c_tot + g1.insurance, "conservation intact");
    let _ = r;
}

// security.md sweep — TradeCpi maker margin protection (#19/#46): a taker trading against a maker via
// the matcher must not be able to force the maker (LP) into an under-margined position. If the maker
// can't margin the fill, the trade must reject — the maker is protected like any account.
#[test]
fn v16_attack_tradecpi_thin_maker_margin_protected() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new();
    let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 100_000_000); // taker well funded
    env.deposit(&maker_owner, maker, 1_000); // maker THIN
    let (ctx, delegate, _) =
        env.init_matcher_context_authorized(matcher_program, &maker_owner, maker);
    let (_, g0) = env.market_state();

    // taker tries to trade a LARGE size against the thin maker -> maker can't margin it -> reject.
    env.svm.expire_blockhash();
    let r = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &maker_owner,
        maker,
        matcher_program,
        ctx,
        delegate,
        0,
        (10_000 * POS_SCALE) as i128,
        100,
    );
    assert!(
        r.is_err(),
        "trade that would leave the thin maker under-margined must reject"
    );
    // no position opened, no value moved, maker capital intact.
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.assets[0].oi_eff_long_q, 0,
        "no OI created by the rejected over-fill"
    );
    assert_eq!(
        env.portfolio_state(maker).legs[0].basis_pos_q.get(),
        0,
        "maker took no position"
    );
    assert_eq!(
        env.portfolio_state(taker).legs[0].basis_pos_q.get(),
        0,
        "taker took no position"
    );
    assert_eq!(
        env.portfolio_state(maker).capital.get(),
        1_000,
        "maker capital intact"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    // a SMALL trade the maker CAN margin still works (control).
    env.svm.expire_blockhash();
    let r_ok = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &maker_owner,
        maker,
        matcher_program,
        ctx,
        delegate,
        0,
        POS_SCALE as i128,
        100,
    );
    assert!(
        r_ok.is_ok(),
        "small in-margin trade executes against the maker: {:?}",
        r_ok
    );
}

// security.md sweep — TradeCpi atomic fill vs matcher capacity (#33/#39): a request exceeding the
// matcher's fill capacity must reject ATOMICALLY (no partial/phantom position, no OI), while a
// within-capacity request fills correctly. No phantom over-fill, conservation holds.
#[test]
fn v16_attack_tradecpi_atomic_fill_vs_capacity() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new();
    let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 100_000_000);
    env.deposit(&maker_owner, maker, 100_000_000);
    let (ctx, delegate, _) = env.init_matcher_context_with_data_authorized(
        matcher_program,
        &maker_owner,
        maker,
        encode_matcher_init_passive(POS_SCALE),
    );
    let (_, g0) = env.market_state();
    // request 10x the cap -> rejects atomically (no partial/phantom fill).
    let r_over = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &maker_owner,
        maker,
        matcher_program,
        ctx,
        delegate,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(
        r_over.is_err(),
        "over-capacity TradeCpi must reject atomically"
    );
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.assets[0].oi_eff_long_q, 0,
        "no phantom OI from rejected over-capacity trade"
    );
    assert_eq!(
        env.portfolio_state(taker).legs[0].basis_pos_q.get(),
        0,
        "no partial/phantom position"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged by rejected trade");
    // within-capacity request fills correctly.
    let r_ok = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &maker_owner,
        maker,
        matcher_program,
        ctx,
        delegate,
        0,
        POS_SCALE as i128,
        100,
    );
    assert!(r_ok.is_ok(), "within-capacity TradeCpi fills: {:?}", r_ok);
    let basis = env.portfolio_state(taker).legs[0].basis_pos_q.get();
    assert_eq!(
        basis, POS_SCALE as i128,
        "taker filled exactly the requested within-capacity amount"
    );
    let (_, g2) = env.market_state();
    assert_eq!(
        g2.assets[0].oi_eff_long_q, g2.assets[0].oi_eff_short_q,
        "OI balanced to the fill"
    );
    assert_eq!(
        g2.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation");
}

// security.md sweep — TradeCpi fee is mark-pinned, not matcher-quoted (F-TRADENOCPI-FEE, CPI path):
// handle_trade_cpi_zero_copy delegates to handle_trade_nocpi_zero_copy (src/v16_program.rs:6428) passing
// the matcher's exec_price_e6, and the F-TRADENOCPI-FEE fix there pins the fee BASIS to the asset mark
// (effective_price), NOT the passed exec_price. Attacker goal: a colluding/malicious matcher quotes a
// low exec_price to under-bill the CPI trade fee (the TradeNoCpi attack, on the CPI path). Protection:
// security.md sweep — TradeCpi enforces the MAKER's margin too (#9/#22): the matcher fills the taker
// against the maker (LP), who takes the opposite side. Attacker goal: a matcher opens a large position
// against an under-capitalized maker (the maker can't margin its leg), fabricating OI / bad debt on the
// maker side. Protection: the post-trade margin check covers BOTH sides, so an under-margined maker
// causes the whole TradeCpi to reject; a well-capitalized maker fills cleanly.
#[test]
fn v16_attack_tradecpi_enforces_maker_margin() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let to = Keypair::new();
    let t = env.create_portfolio(&to);
    let mo = Keypair::new();
    let m = env.create_portfolio(&mo);
    env.deposit(&to, t, 100_000_000); // taker well funded
    env.deposit(&mo, m, 100); // maker thin: cannot margin a 10*POS_SCALE leg (IM=100% -> req 1000)
    let (ctx, del, _) = env.init_matcher_context_authorized(matcher_program, &mo, m);
    let (_, g0) = env.market_state();

    // ATTACK: 10*POS_SCALE fill -> the maker would owe IM ~1000 >> its 100 capital -> reject.
    env.svm.expire_blockhash();
    let r = env.try_trade_cpi_with_cu_on_asset(
        &to,
        t,
        &mo,
        m,
        matcher_program,
        ctx,
        del,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(
        r.is_err(),
        "TradeCpi against an under-margined maker must reject"
    );

    // ROLLBACK: no position on either side, no OI, vault unchanged.
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(t))),
        "taker has no position"
    );
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(m))),
        "maker has no position"
    );
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.assets[0].oi_eff_long_q, 0,
        "no OI fabricated against the thin maker"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");

    // DISCRIMINATING CONTROL: top the maker up so it CAN margin the leg -> the same trade fills cleanly.
    env.deposit(&mo, m, 2_000_000);
    env.svm.expire_blockhash();
    let ok = env.try_trade_cpi_with_cu_on_asset(
        &to,
        t,
        &mo,
        m,
        matcher_program,
        ctx,
        del,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(ok.is_ok(), "well-capitalized maker fills cleanly: {:?}", ok);
    assert_eq!(
        env.portfolio_state(m).legs[0].basis_pos_q.get(),
        -((10 * POS_SCALE) as i128),
        "maker took the opposite leg"
    );
    let g = env.market_state().1;
    assert_eq!(
        g.c_tot + g.insurance,
        g.vault,
        "conservation after the in-margin fill"
    );
}

// security.md sweep - duplicate asset legs in a batch (#22/#33): batch fee/accounting reconstruction
// is per-asset, so a batch must not contain two legs for the same asset. Both the direct and matcher-CPI
// batch paths reject duplicates atomically, then accept the same setup with distinct asset legs.
#[test]
fn v16_attack_batch_duplicate_asset_legs_reject_atomically() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);
    let before = env.market_state().1;
    let taker_before = env.portfolio_state(ta);
    let lp_before = env.portfolio_state(la);
    let sz = (5 * POS_SCALE) as i128;

    env.svm.expire_blockhash();
    let duplicate_nocpi = env.send(
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
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
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
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
        ],
        &[&taker, &lp],
    );
    assert!(
        duplicate_nocpi.is_err(),
        "duplicate-asset BatchTradeNoCpi must reject"
    );
    let after_nocpi = env.market_state().1;
    assert_eq!(
        after_nocpi.assets[0].oi_eff_long_q, 0,
        "no OI from rejected no-CPI batch"
    );
    assert_eq!(
        after_nocpi.assets[1].oi_eff_long_q, 0,
        "unrelated asset untouched"
    );
    assert_eq!(
        after_nocpi.insurance, before.insurance,
        "no fee credited by rejected no-CPI batch"
    );
    assert_eq!(
        after_nocpi.vault, before.vault,
        "vault unchanged by rejected no-CPI batch"
    );
    assert_eq!(
        env.portfolio_state(ta).capital.get(),
        taker_before.capital.get()
    );
    assert_eq!(
        env.portfolio_state(la).capital.get(),
        lp_before.capital.get()
    );

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, la);
    env.svm.expire_blockhash();
    let duplicate_cpi = env.send(
        env.batch_trade_cpi_ix(
            ta,
            la,
            vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: -sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
            ],
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker],
    );
    assert!(
        duplicate_cpi.is_err(),
        "duplicate-asset BatchTradeCpi must reject"
    );
    let after_cpi = env.market_state().1;
    assert_eq!(
        after_cpi.assets[0].oi_eff_long_q, 0,
        "no OI from rejected CPI batch"
    );
    assert_eq!(
        after_cpi.assets[1].oi_eff_long_q, 0,
        "unrelated asset untouched by rejected CPI batch"
    );
    assert_eq!(
        after_cpi.insurance, before.insurance,
        "no fee credited by rejected CPI batch"
    );
    assert_eq!(
        after_cpi.vault, before.vault,
        "vault unchanged by rejected CPI batch"
    );
    assert_eq!(
        env.portfolio_state(ta).capital.get(),
        taker_before.capital.get()
    );
    assert_eq!(
        env.portfolio_state(la).capital.get(),
        lp_before.capital.get()
    );

    env.svm.expire_blockhash();
    let clean = env.send(
        env.batch_trade_cpi_ix(
            ta,
            la,
            vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
                BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id((1) as u16),
                    size_q: -sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
            ],
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker],
    );
    assert!(
        clean.is_ok(),
        "distinct-asset BatchTradeCpi control must execute: {clean:?}"
    );
    let taker_after = env.portfolio_state(ta);
    assert!(
        has_active_leg_for_asset(&taker_after, 0),
        "clean control fills asset 0"
    );
    assert!(
        has_active_leg_for_asset(&taker_after, 1),
        "clean control fills asset 1"
    );
}

#[test]
fn v16_bpf_tradenocpi_executes_and_is_bounded() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 1_000_000);

    let trade_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        150,
        100,
    );
    println!("v16 TradeNoCpi BPF CU: {trade_cu}");
    assert!(
        trade_cu <= TRADE_CU_LIMIT,
        "TradeNoCpi CU {} exceeded limit {}",
        trade_cu,
        TRADE_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let long = state::read_portfolio(&long_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();

    assert_eq!(env.token_amount(env.vault), 2_000_000);
    println!(
        "TradeNoCpi BPF long_basis={}, short_basis={}, insurance={}",
        long.legs[0].basis_pos_q.get(),
        short.legs[0].basis_pos_q.get(),
        group.insurance
    );
    assert_eq!(long.legs[0].basis_pos_q.get(), (10 * POS_SCALE) as i128);
    assert_eq!(short.legs[0].basis_pos_q.get(), -((10 * POS_SCALE) as i128));
    assert_eq!(
        group.assets[0].effective_price, 100,
        "consented execution price must not move the effective oracle price"
    );
    assert_eq!(
        group.insurance, 20,
        "F-TRADENOCPI-FEE: the trade fee is billed on the MARK (effective_price=100), NOT the \
         consented exec_price=150 -> notional=1000, 100 bps charges 10 to each side (was 30 when the \
         fee tracked the caller-gameable exec_price)"
    );
    assert_eq!(group.vault, 2_000_000);
    assert_eq!(group.c_tot + group.insurance, group.vault);
}

// BatchTradeNoCpi: a single atomic batch carries a MIXED-direction spread (taker long asset 0,
// short asset 1) against one LP, applied with one end-state margin check.
#[test]
fn v16_bpf_batch_trade_executes_mixed_direction_spread() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);
    let sz = (5 * POS_SCALE) as i128;
    let cu = env
        .send(
            env.batch_trade_no_cpi_ix(
                ta,
                la,
                vec![
                    BatchTradeLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id((0) as u16),
                        size_q: sz,
                        exec_price: 100,
                        fee_bps: 0,
                    },
                    BatchTradeLeg {
                        asset_index: 1,
                        market_id: first_generation_market_id((1) as u16),
                        size_q: -sz,
                        exec_price: 100,
                        fee_bps: 0,
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
        .expect("mixed-direction batch must execute");
    println!("v16 batch mixed-direction 2-leg CU: {cu}");
    let t = state::read_portfolio(&env.svm.get_account(&ta).unwrap().data).unwrap();
    let l = state::read_portfolio(&env.svm.get_account(&la).unwrap().data).unwrap();
    assert_eq!(active_leg_for_asset(&t, 0).side, SideV16::Long);
    assert_eq!(active_leg_for_asset(&t, 1).side, SideV16::Short);
    assert_eq!(active_leg_for_asset(&l, 0).side, SideV16::Short);
    assert_eq!(active_leg_for_asset(&l, 1).side, SideV16::Long);
    assert_eq!(active_leg_for_asset(&t, 0).basis_pos_q, sz);
    assert_eq!(active_leg_for_asset(&t, 1).basis_pos_q, -sz);
}

// LoF/DoS sweep: DrainOnly is allowed to reduce existing risk, but a CPI request whose signed size
// can only increase an existing long/short pair is guaranteed to be rejected by the engine. That
// rejection should happen before invoking an external matcher; otherwise a hostile LP matcher can
// BatchTradeNoCpi end-state margin: a leg that is individually margin-INFEASIBLE (it would leave the
// taker holding two full positions at once) is rejected as a standalone trade, but the SAME leg in a
// batch that also closes the offsetting position SUCCEEDS, because the batch checks initial margin
// only on the final portfolio.
#[test]
fn v16_bpf_batch_trade_checks_margin_on_final_portfolio_only() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000); // at 10% IM, capital fits ONE 80-lot position (IM 800), not two (1600)
    env.deposit(&lp, la, 1_000_000);
    let sz = (80 * POS_SCALE) as i128;
    // taker opens a short on asset 0 (one position, feasible).
    env.trade_asset_with_cu(0, &lp, la, &taker, ta, sz, 100, 0);
    assert_eq!(
        active_leg_for_asset(
            &state::read_portfolio(&env.svm.get_account(&ta).unwrap().data).unwrap(),
            0
        )
        .side,
        SideV16::Short
    );

    // a standalone long on asset 1 would leave the taker holding TWO positions (short-0 + long-1):
    // initial margin on that interim portfolio exceeds capital, so it must be rejected.
    let interim = env.try_trade_asset_with_cu(1, &taker, ta, &lp, la, sz, 100, 0);
    assert!(
        interim.is_err(),
        "interim two-position state must fail a standalone trade: {interim:?}"
    );

    // the batch does the long-1 leg AND closes the short-0 leg; the FINAL portfolio is a single
    // long-1 position (feasible), so the batch is accepted even though an interim leg is not.
    let cu = env
        .send(
            env.batch_trade_no_cpi_ix(
                ta,
                la,
                vec![
                    BatchTradeLeg {
                        asset_index: 1,
                        market_id: first_generation_market_id((1) as u16),
                        size_q: sz,
                        exec_price: 100,
                        fee_bps: 0,
                    },
                    BatchTradeLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id((0) as u16),
                        size_q: sz,
                        exec_price: 100,
                        fee_bps: 0,
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
        .expect("batch must accept a final-IM-feasible basket despite an infeasible interim leg");
    println!("v16 batch end-state-margin 2-leg CU: {cu}");
    let t = state::read_portfolio(&env.svm.get_account(&ta).unwrap().data).unwrap();
    assert!(
        !has_active_leg_for_asset(&t, 0),
        "short asset-0 closed by the batch"
    );
    assert_eq!(
        active_leg_for_asset(&t, 1).side,
        SideV16::Long,
        "taker keeps only the asset-1 long"
    );
}

// BatchTradeCpi: a single batched matcher CPI fills a MIXED-direction spread (taker long asset 0,
// short asset 1) against one LP, then both fills apply with one end-state margin check.
#[test]
fn v16_bpf_batch_trade_cpi_executes_mixed_spread_through_matcher() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);
    let (ctx, delegate, init_cu) =
        env.init_auth_matcher_context_via_system_create(matcher_program, &lp, la);
    let sz = (5 * POS_SCALE) as i128;
    env.svm.expire_blockhash();
    let cu = env
        .send(
            env.batch_trade_cpi_ix(
                ta,
                la,
                vec![
                    BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id((0) as u16),
                        size_q: sz,
                        fee_bps: 100,
                        limit_price: 0,
                    },
                    BatchTradeCpiLeg {
                        asset_index: 1,
                        market_id: first_generation_market_id((1) as u16),
                        size_q: -sz,
                        fee_bps: 100,
                        limit_price: 0,
                    },
                ],
            ),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        )
        .expect("batch CPI mixed spread must execute through the matcher without LP signing fill");
    println!("v16 batch matcher system-init CU: {init_cu}, mixed-direction 2-leg CU: {cu}");
    let t = state::read_portfolio(&env.svm.get_account(&ta).unwrap().data).unwrap();
    let l = state::read_portfolio(&env.svm.get_account(&la).unwrap().data).unwrap();
    assert_eq!(active_leg_for_asset(&t, 0).side, SideV16::Long);
    assert_eq!(active_leg_for_asset(&t, 1).side, SideV16::Short);
    assert_eq!(active_leg_for_asset(&l, 0).side, SideV16::Short);
    assert_eq!(active_leg_for_asset(&l, 1).side, SideV16::Long);
    assert_eq!(active_leg_for_asset(&t, 0).basis_pos_q, sz);
    assert_eq!(active_leg_for_asset(&t, 1).basis_pos_q, -sz);
}
