//! INV-088 - Global summaries are not account-local proofs.
//!
//! Normative obligation: A market/global accumulator or last-touched summary
//! cannot substitute for an account-, asset-, or domain-local proof unless it is
//! independently complete for that scope and updated on every relevant transition.
//!
//! Evidence in this file (I/C): public LiteSVM wrapper tests move and crank one
//! asset while another asset carries live exposure, then assert the untouched
//! asset's price, OI, and settlement index remain byte-for-byte local. This
//! catches last-touched/global-summary bleed without mutating program state out
//! of band.
//!
//! Guarantee boundary: this is a public-route regression for one summary-locality
//! class. Full certification still requires an independent recomputation model
//! over every global summary and all update orders.

use super::*;

fn inv_088_scan_asset(
    portfolios: &[PortfolioAccountV16],
    asset_index: usize,
    a_long: u128,
    a_short: u128,
) -> (u64, u64, u128, u128, u128, u128) {
    let mut long_count = 0u64;
    let mut short_count = 0u64;
    let mut raw_long_oi = 0u128;
    let mut raw_short_oi = 0u128;
    let mut long_oi = 0u128;
    let mut short_oi = 0u128;
    for portfolio in portfolios {
        for leg in portfolio
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
        {
            if !leg.active || leg.asset_index as usize != asset_index {
                continue;
            }
            match leg.side {
                SideV16::Long => {
                    long_count += 1;
                    let abs = leg.basis_pos_q.unsigned_abs();
                    raw_long_oi += abs;
                    long_oi += abs * a_long / leg.a_basis;
                }
                SideV16::Short => {
                    short_count += 1;
                    let abs = leg.basis_pos_q.unsigned_abs();
                    raw_short_oi += abs;
                    short_oi += abs * a_short / leg.a_basis;
                }
            }
        }
    }
    (
        long_count,
        short_count,
        raw_long_oi,
        raw_short_oi,
        long_oi,
        short_oi,
    )
}

fn inv_088_assert_asset_summary_matches_scan(env: &V16CuEnv, portfolios: &[Pubkey], asset: usize) {
    let states: Vec<_> = portfolios
        .iter()
        .map(|portfolio| env.portfolio_state(*portfolio))
        .collect();
    let group = env.market_state().1;
    let engine_asset = group.assets[asset];
    let (long_count, short_count, raw_long_oi, raw_short_oi, long_oi_floor, short_oi_floor) =
        inv_088_scan_asset(&states, asset, engine_asset.a_long, engine_asset.a_short);
    assert_eq!(
        engine_asset.stored_pos_count_long, long_count,
        "asset {asset} stored long count must equal independent portfolio scan"
    );
    assert_eq!(
        engine_asset.stored_pos_count_short, short_count,
        "asset {asset} stored short count must equal independent portfolio scan"
    );
    assert!(
        engine_asset.oi_eff_long_q <= raw_long_oi,
        "asset {asset} long OI exceeds raw independent portfolio scan"
    );
    assert!(
        engine_asset.oi_eff_short_q <= raw_short_oi,
        "asset {asset} short OI exceeds raw independent portfolio scan"
    );
    assert!(
        engine_asset.oi_eff_long_q >= long_oi_floor
            && engine_asset.oi_eff_long_q - long_oi_floor <= long_count as u128,
        "asset {asset} long OI must match independent ADL-effective scan up to one atom per leg"
    );
    assert!(
        engine_asset.oi_eff_short_q >= short_oi_floor
            && engine_asset.oi_eff_short_q - short_oi_floor <= short_count as u128,
        "asset {asset} short OI must match independent ADL-effective scan up to one atom per leg"
    );
}

#[test]
fn v16_program_per_asset_crank_isolation() {
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

    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 2_000_000);
    env.deposit(&lb, pb, 2_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);

    let (_, before) = env.market_state();
    let asset_1_price = before.assets[1].effective_price;
    let asset_1_oi_long = before.assets[1].oi_eff_long_q;
    let asset_1_oi_short = before.assets[1].oi_eff_short_q;
    let asset_1_k_long = before.assets[1].k_long;

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 130);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for portfolio in [pa, pb] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
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

    let (_, after) = env.market_state();
    assert!(
        after.assets[0].effective_price > asset_1_price,
        "asset 0 price moved, so the probe is non-vacuous"
    );
    assert_eq!(
        after.assets[1].effective_price, asset_1_price,
        "asset 1 effective price changed after an asset-0-only crank"
    );
    assert_eq!(
        after.assets[1].oi_eff_long_q, asset_1_oi_long,
        "asset 1 long OI changed after an asset-0-only crank"
    );
    assert_eq!(
        after.assets[1].oi_eff_short_q, asset_1_oi_short,
        "asset 1 short OI changed after an asset-0-only crank"
    );
    assert_eq!(
        after.assets[1].k_long, asset_1_k_long,
        "asset 1 settlement index changed after an asset-0-only crank"
    );
    assert_eq!(after.vault as u64, env.token_amount(env.vault));
    assert!(after.vault >= after.c_tot + after.insurance);
}

#[test]
fn v16_program_stored_position_summaries_match_portfolio_scan_after_cross_asset_updates() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 0, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 2_000_000);
    env.deposit(&short_owner, short, 2_000_000);

    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        100,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long,
        &short_owner,
        short,
        2 * POS_SCALE as i128,
        100,
        0,
    );
    inv_088_assert_asset_summary_matches_scan(&env, &[long, short], 0);
    inv_088_assert_asset_summary_matches_scan(&env, &[long, short], 1);

    let asset1_before = env.market_state().1.assets[1];
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        100,
        0,
    );

    inv_088_assert_asset_summary_matches_scan(&env, &[long, short], 0);
    inv_088_assert_asset_summary_matches_scan(&env, &[long, short], 1);
    let asset1_after = env.market_state().1.assets[1];
    assert_eq!(
        asset1_after.stored_pos_count_long, asset1_before.stored_pos_count_long,
        "closing asset 0 must not use a last-touched summary to alter asset 1 long count"
    );
    assert_eq!(
        asset1_after.stored_pos_count_short, asset1_before.stored_pos_count_short,
        "closing asset 0 must not use a last-touched summary to alter asset 1 short count"
    );
    assert_eq!(
        asset1_after.oi_eff_long_q, asset1_before.oi_eff_long_q,
        "closing asset 0 must not alter asset 1 long OI"
    );
    assert_eq!(
        asset1_after.oi_eff_short_q, asset1_before.oi_eff_short_q,
        "closing asset 0 must not alter asset 1 short OI"
    );
}

#[test]
fn v16_program_batch_nocpi_updates_each_asset_summary_from_portfolio_scan() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 0, 100);

    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, 2_000_000);
    env.deposit(&owner_b, account_b, 2_000_000);

    env.svm.expire_blockhash();
    let open_cu = env
        .send(
            env.batch_trade_no_cpi_ix(
                account_a,
                account_b,
                vec![
                    BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: POS_SCALE as i128,
                        exec_price: 100,
                        fee_bps: 0,
                    },
                    BatchTradeLeg {
                        asset_index: 1,
                        market_id: env.asset_market_id(1),
                        size_q: -(2 * POS_SCALE as i128),
                        exec_price: 100,
                        fee_bps: 0,
                    },
                ],
            ),
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[&owner_a, &owner_b],
        )
        .expect("multi-asset BatchTradeNoCpi open");
    assert_cu_within(
        "INV-088 multi-asset BatchTradeNoCpi open",
        open_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let portfolios = [account_a, account_b];
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 1);
    let after_open = env.market_state().1;
    assert_eq!(after_open.assets[0].stored_pos_count_long, 1);
    assert_eq!(after_open.assets[0].stored_pos_count_short, 1);
    assert_eq!(after_open.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(after_open.assets[0].oi_eff_short_q, POS_SCALE);
    assert_eq!(after_open.assets[1].stored_pos_count_long, 1);
    assert_eq!(after_open.assets[1].stored_pos_count_short, 1);
    assert_eq!(after_open.assets[1].oi_eff_long_q, 2 * POS_SCALE);
    assert_eq!(after_open.assets[1].oi_eff_short_q, 2 * POS_SCALE);

    env.svm.expire_blockhash();
    let reduce_cu = env
        .send(
            env.batch_trade_no_cpi_ix(
                account_a,
                account_b,
                vec![
                    BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: -(POS_SCALE as i128),
                        exec_price: 100,
                        fee_bps: 0,
                    },
                    BatchTradeLeg {
                        asset_index: 1,
                        market_id: env.asset_market_id(1),
                        size_q: POS_SCALE as i128,
                        exec_price: 100,
                        fee_bps: 0,
                    },
                ],
            ),
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[&owner_a, &owner_b],
        )
        .expect("multi-asset BatchTradeNoCpi partial exit");
    assert_cu_within(
        "INV-088 multi-asset BatchTradeNoCpi partial exit",
        reduce_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 1);
    let after_reduce = env.market_state().1;
    assert_eq!(
        after_reduce.assets[0].stored_pos_count_long, 0,
        "batch exit of asset 0 must clear only asset-0 long summary"
    );
    assert_eq!(
        after_reduce.assets[0].stored_pos_count_short, 0,
        "batch exit of asset 0 must clear only asset-0 short summary"
    );
    assert_eq!(after_reduce.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_reduce.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        after_reduce.assets[1].stored_pos_count_long, 1,
        "batch asset-0 exit must not clear asset-1 long summary"
    );
    assert_eq!(
        after_reduce.assets[1].stored_pos_count_short, 1,
        "batch asset-0 exit must not clear asset-1 short summary"
    );
    assert_eq!(after_reduce.assets[1].oi_eff_long_q, POS_SCALE);
    assert_eq!(after_reduce.assets[1].oi_eff_short_q, POS_SCALE);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(account_a),
        0
    ));
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(account_b),
        0
    ));
    assert!(has_active_leg_for_asset(&env.portfolio_state(account_a), 1));
    assert!(has_active_leg_for_asset(&env.portfolio_state(account_b), 1));
}

#[test]
fn v16_program_same_asset_summary_preserves_other_portfolios_after_one_pair_exits() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);

    let long_owner_a = Keypair::new();
    let short_owner_a = Keypair::new();
    let long_owner_b = Keypair::new();
    let short_owner_b = Keypair::new();
    let long_a = env.create_portfolio(&long_owner_a);
    let short_a = env.create_portfolio(&short_owner_a);
    let long_b = env.create_portfolio(&long_owner_b);
    let short_b = env.create_portfolio(&short_owner_b);
    for (owner, portfolio) in [
        (&long_owner_a, long_a),
        (&short_owner_a, short_a),
        (&long_owner_b, long_b),
        (&short_owner_b, short_b),
    ] {
        env.deposit(owner, portfolio, 2_000_000);
    }

    env.trade_asset_with_cu(
        0,
        &long_owner_a,
        long_a,
        &short_owner_a,
        short_a,
        POS_SCALE as i128,
        100,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &long_owner_b,
        long_b,
        &short_owner_b,
        short_b,
        2 * POS_SCALE as i128,
        100,
        0,
    );
    let portfolios = [long_a, short_a, long_b, short_b];
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    let before = env.market_state().1.assets[0];
    assert_eq!(before.stored_pos_count_long, 2);
    assert_eq!(before.stored_pos_count_short, 2);
    assert_eq!(before.oi_eff_long_q, 3 * POS_SCALE);
    assert_eq!(before.oi_eff_short_q, 3 * POS_SCALE);

    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &long_owner_a,
        long_a,
        &short_owner_a,
        short_a,
        -(POS_SCALE as i128),
        100,
        0,
    );

    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    let after_group = env.market_state().1;
    let after = after_group.assets[0];
    assert_eq!(
        after.stored_pos_count_long, 1,
        "closing one long must not clear another portfolio's same-asset long summary"
    );
    assert_eq!(
        after.stored_pos_count_short, 1,
        "closing one short must not clear another portfolio's same-asset short summary"
    );
    assert_eq!(
        after.oi_eff_long_q,
        2 * POS_SCALE,
        "remaining same-asset long OI must be preserved after another pair exits"
    );
    assert_eq!(
        after.oi_eff_short_q,
        2 * POS_SCALE,
        "remaining same-asset short OI must be preserved after another pair exits"
    );
    assert!(has_active_leg_for_asset(&env.portfolio_state(long_b), 0));
    assert!(has_active_leg_for_asset(&env.portfolio_state(short_b), 0));
    assert_eq!(after_group.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_program_liquidation_updates_same_asset_summaries_without_clobbering_other_portfolios() {
    const LIQ_SLOT: u64 = 30;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let long_owner_a = Keypair::new();
    let short_owner_a = Keypair::new();
    let long_owner_b = Keypair::new();
    let short_owner_b = Keypair::new();
    let long_a = env.create_portfolio(&long_owner_a);
    let short_a = env.create_portfolio(&short_owner_a);
    let long_b = env.create_portfolio(&long_owner_b);
    let short_b = env.create_portfolio(&short_owner_b);
    env.deposit(&long_owner_a, long_a, 100_000_000);
    env.deposit(&short_owner_a, short_a, 100_000);
    env.deposit(&long_owner_b, long_b, 100_000_000);
    env.deposit(&short_owner_b, short_b, 100_000_000);

    env.trade_asset_with_cu(
        0,
        &long_owner_a,
        long_a,
        &short_owner_a,
        short_a,
        POS_SCALE as i128,
        1_000_000,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &long_owner_b,
        long_b,
        &short_owner_b,
        short_b,
        2 * POS_SCALE as i128,
        1_000_000,
        0,
    );
    let portfolios = [long_a, short_a, long_b, short_b];
    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    let before = env.market_state().1.assets[0];
    assert_eq!(before.stored_pos_count_long, 2);
    assert_eq!(before.stored_pos_count_short, 2);
    assert_eq!(before.oi_eff_long_q, 3 * POS_SCALE);
    assert_eq!(before.oi_eff_short_q, 3 * POS_SCALE);

    for slot in 1..=LIQ_SLOT {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 2_000_000);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short_a, false),
            ],
            &[],
        );
    }
    assert!(
        health_cert(&env.portfolio_state(short_a)).certified_liq_deficit != 0,
        "setup must make one short liquidatable while another pair remains live"
    );

    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQ_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_a, false),
        ],
        &[],
    )
    .expect("permissionless liquidation");

    inv_088_assert_asset_summary_matches_scan(&env, &portfolios, 0);
    let after_group = env.market_state().1;
    let after = after_group.assets[0];
    assert!(
        after.oi_eff_long_q < before.oi_eff_long_q,
        "liquidation reduced same-asset long OI"
    );
    assert!(
        after.oi_eff_short_q < before.oi_eff_short_q,
        "liquidation reduced same-asset short OI"
    );
    assert_eq!(
        health_cert(&env.portfolio_state(short_a)).certified_liq_deficit,
        0,
        "liquidated account is back to current"
    );
    assert!(has_active_leg_for_asset(&env.portfolio_state(long_b), 0));
    assert!(has_active_leg_for_asset(&env.portfolio_state(short_b), 0));
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(long_b), 0)
            .basis_pos_q
            .unsigned_abs(),
        2 * POS_SCALE,
        "unrelated same-asset long exposure is preserved"
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(short_b), 0)
            .basis_pos_q
            .unsigned_abs(),
        2 * POS_SCALE,
        "unrelated same-asset short exposure is preserved"
    );
    assert_eq!(after_group.vault as u64, env.token_amount(env.vault));
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
}
