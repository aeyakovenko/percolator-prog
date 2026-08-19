//! INV-038 - rounding and ratio conservation.
//!
//! Normative obligation: every rounded allocation preserves
//! `input = sum(outputs) + explicit residue`, with residue assigned only to a
//! non-user-value class. These public-route LiteSVM tests cover dust trade fees,
//! split subatom execution, batch reconstruction, funding direction/caps, and
//! backing-fee splits so rounding cannot mint principal, insurance, backing, or
//! withdrawable PnL.

use super::*;

// security.md sweep — rounding asymmetry (#37 dust): trade fees must round UP (ceil, protocol favor)
// so dust-notional trades are never free and repeated churn never leaks value to the trader. Attacker
// success = a fee that floors to 0 (free trade) or insurance that fails to grow on a fee'd dust trade.
#[test]
fn v16_attack_trade_fee_rounds_up_no_free_dust_trades() {
    let mut env = V16CuEnv::new(); // max_trading_fee_bps = 10_000
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let ins = |env: &V16CuEnv| env.market_state().1.insurance;
    // dust notional with the smallest nonzero fee: notional = size*price/POS_SCALE.
    // size = POS_SCALE/100 @ price 100 => notional = 1; fee_bps=1 => true fee = 0.0001 -> must ceil to >=1.
    let dust_size = (POS_SCALE / 100) as i128;
    let mut prev_ins = ins(&env);
    let mut opened: i128 = 0;
    for i in 0..5 {
        env.svm.expire_blockhash();
        let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, dust_size, 100, 1);
        if r.is_err() {
            break;
        } // if dust trade is rejected outright, that's also safe (no free trade)
        opened += dust_size;
        let now = ins(&env);
        assert!(
            now > prev_ins,
            "dust trade #{} charged a nonzero fee (insurance grew {} -> {})",
            i,
            prev_ins,
            now
        );
        prev_ins = now;
        // conservation after each dust trade.
        let (_, g) = env.market_state();
        assert_eq!(g.vault, 2_000_000, "no value created by dust trade");
        assert_eq!(g.vault, g.c_tot + g.insurance, "exact conservation");
    }
    assert!(opened > 0, "at least one dust trade executed (non-vacuous)");
    // close the accumulated dust position; conservation still exact, insurance only grew.
    if opened > 0 {
        env.svm.expire_blockhash();
        let _ = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, -opened, 100, 0);
    }
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 2_000_000, "vault conserved across dust churn");
    assert_eq!(
        g.vault,
        g.c_tot + g.insurance,
        "exact conservation after close"
    );
    assert!(
        g.insurance >= prev_ins,
        "insurance never decreased (fees are protocol-favorable)"
    );
}

// security.md sweep — fee-splitting dust below one atom: a caller can split a real position into
// fills whose floor notional is zero. The public TradeNoCpi path must still charge nonzero fees for
// nonzero risk-changing fills, otherwise repeated slices grow OI without paying the configured fee.
#[test]
fn v16_attack_subatom_fee_splits_cannot_accumulate_free_position() {
    let mut env = V16CuEnv::new(); // max_trading_fee_bps = 10_000
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);

    let sub_atom_size = (POS_SCALE / 100 - 1) as i128;
    assert!(sub_atom_size > 0, "probe size is nonzero");
    let mut opened = 0i128;
    let mut prev_insurance = env.market_state().1.insurance;
    for i in 0..5 {
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, sub_atom_size, 100, 1)
            .unwrap_or_else(|e| panic!("sub-atom public trade #{i} must execute, got {e}"));
        opened += sub_atom_size;
        let (_, group) = env.market_state();
        assert!(
            group.insurance > prev_insurance,
            "accepted sub-atom trade #{i} grew OI by {sub_atom_size}q but paid no fee"
        );
        assert_eq!(group.vault, 2_000_000, "no value created by sub-atom fill");
        assert_eq!(
            group.vault,
            group.c_tot + group.insurance,
            "exact conservation"
        );
        prev_insurance = group.insurance;
    }

    let taker = env.portfolio_state(pa);
    let lp = env.portfolio_state(pb);
    assert!(
        opened > (POS_SCALE / 100) as i128,
        "accepted sub-atom fills accumulate into a real position"
    );
    assert_eq!(active_leg_for_asset(&taker, 0).basis_pos_q, opened);
    assert_eq!(active_leg_for_asset(&lp, 0).basis_pos_q, -opened);
}

// Same fee invariant through BatchTradeNoCpi. This also exercises the wrapper's per-leg fee
// reconstruction: a stale floor-notional mirror computes zero and fails to match the fixed engine's
// aggregate fee outcome for a sub-atom leg.
#[test]
fn v16_attack_batch_subatom_fee_reconstruction_uses_ceil_notional() {
    let mut env = V16CuEnv::new(); // max_trading_fee_bps = 10_000
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);

    let sub_atom_size = (POS_SCALE / 100 - 1) as i128;
    let before_insurance = env.market_state().1.insurance;
    env.send(
        env.batch_trade_no_cpi_ix(
            taker_account,
            lp_account,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: sub_atom_size,
                exec_price: 100,
                fee_bps: 1,
            }],
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
    .expect("sub-atom BatchTradeNoCpi must execute with matching fee reconstruction");

    let (_, group) = env.market_state();
    assert!(
        group.insurance > before_insurance,
        "sub-atom batch leg must pay a nonzero fee"
    );
    assert_eq!(group.vault, 2_000_000, "no value created by sub-atom batch");
    assert_eq!(
        group.vault,
        group.c_tot + group.insurance,
        "exact conservation"
    );
    let taker_after = env.portfolio_state(taker_account);
    let lp_after = env.portfolio_state(lp_account);
    assert_eq!(
        active_leg_for_asset(&taker_after, 0).basis_pos_q,
        sub_atom_size
    );
    assert_eq!(
        active_leg_for_asset(&lp_after, 0).basis_pos_q,
        -sub_atom_size
    );
}

// security.md sweep — funding cap precision (#19 DoS): an extreme mark premium must be clamped to
// max_abs_funding_e9_per_slot. If funding scaled with the raw premium, a tiny mark push could drain
// a counterparty arbitrarily fast. Decisive check: a 2x-index premium and a 1000x-index premium must
// accrue IDENTICAL funding (both pinned to the cap), and value stays conserved.
#[test]
fn v16_attack_extreme_premium_funding_is_capped() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 100_000_000;
    fn run_scenario(mark_mult: u64) -> (i128, u128, u128) {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            initial_price: INITIAL_PRICE,
            max_price_move_bps_per_slot: 1_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 1,
            ..V16CuMarketParams::default()
        });
        env.svm.warp_to_slot(0);
        env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
        let lo_owner = Keypair::new();
        let lo = env.create_portfolio(&lo_owner);
        let sh_owner = Keypair::new();
        let sh = env.create_portfolio(&sh_owner);
        env.deposit(&lo_owner, lo, DEPOSIT);
        env.deposit(&sh_owner, sh, DEPOSIT);
        env.trade_with_cu(
            &lo_owner,
            lo,
            &sh_owner,
            sh,
            POS_SCALE as i128,
            INITIAL_PRICE,
            0,
        );
        env.svm.warp_to_slot(1);
        env.push_ewma_mark_with_cu(1, INITIAL_PRICE.saturating_mul(mark_mult)); // premium
        for slot in 1..=4u64 {
            env.svm.warp_to_slot(slot);
            for p in [lo, sh] {
                env.svm.expire_blockhash();
                let _ = env.send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
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
        }
        let (_, g) = env.market_state();
        (g.assets[0].f_long_num, g.vault, g.c_tot + g.insurance)
    }
    let (f2, vault2, senior2) = run_scenario(2); // 2x index premium
    let (f1000, vault1000, senior1000) = run_scenario(1000); // 1000x index premium (capped at clamp)
    assert!(f2 != 0, "funding actually accrued (non-vacuous)");
    // CRUX: extreme premium yields the SAME funding as the moderate one — both pinned to the cap.
    assert_eq!(
        f2, f1000,
        "extreme premium funding is clamped to the cap (identical to moderate)"
    );
    // conservation in both runs.
    assert_eq!(vault2, 2 * DEPOSIT, "scenario 2x: vault conserved");
    assert_eq!(vault1000, 2 * DEPOSIT, "scenario 1000x: vault conserved");
    assert!(
        vault2 >= senior2 && vault1000 >= senior1000,
        "senior conservation in both"
    );
}

// security.md sweep — funding direction symmetry (#33/#9): with the mark BELOW the index (opposite of
// batch 19), funding must flow the other way (shorts pay longs) and still be value-conserving. Probes
// the negative-premium branch of premium_funding_rate_e9.
#[test]
fn v16_attack_funding_direction_mark_below_index_conserves() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    // push the mark BELOW the index, then let funding accrue (no re-push).
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE / 2);
    for slot in 1..=5u64 {
        env.svm.warp_to_slot(slot);
        for p in [lo, sh] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
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
    }
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    assert!(
        g.assets[0].f_long_num != 0 || g.assets[0].f_short_num != 0,
        "funding accrued (non-vacuous)"
    );
    // mark < index => OPPOSITE direction from batch 19 (where longs were charged): longs are credited.
    assert!(
        g.assets[0].f_long_num > 0 && g.assets[0].f_short_num < 0,
        "mark < index => shorts pay longs (f_long>0, f_short<0)"
    );
    // value conservation (same widened invariant as batch 19).
    assert_eq!(g.vault, 2 * DEPOSIT, "no tokens minted/burned");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(
        residual >= a.pnl.get().max(0) + b.pnl.get().max(0),
        "positive pnl backed by residual"
    );
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert!(
        total_equity + g.insurance as i128 <= g.vault as i128,
        "no over-distribution"
    );
}

// Product spec — force-shutdown with a timeout so traders can exit (no rug): marketauth can
// shut down any asset (ASSET_ACTION_SHUTDOWN -> RECOVERY with a frozen mark), but the permissionless
// force-close (which winds the asset down) is gated behind force_close_delay_slots. So there is a
// window after shutdown during which the asset is NOT yet force-closed — traders can exit — and only
// after the delay can the wind-down proceed. Asserts: shutdown -> RECOVERY; force-close REJECTS before
// lifecycle sweep — explicit DrainOnly is a public UpdateAssetLifecycle action, distinct from
// shutdown/recovery. It must be marketauth-gated, reject malformed slot/price args, block new risk,
// security.md sweep — §6.2 backing-yield fee split (#5): when a risk-increasing trade GROWS an
// account's source-credit IM lien that draws a fresh counterparty backing bucket, the wrapper charges
// a backing-domain trade fee = fee_bps * Δbacking and splits it three ways: insurance_share to the
// market insurance pool (asset-0), the SAME amount earmarked into that domain's insurance budget, and
// the remainder to the backing provider's bucket earnings. Verify the split conserves on real BPF
// state: charged == insurance_pool_delta + provider_delta, and the domain budget mirrors the insurance
// share. The lien is formed organically (cross-margin positive PnL used as IM), not injected.
#[test]
fn v16_attack_backing_fee_split_conserves() {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_SIZE_Q: i128 = 200 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = 20 * POS_SCALE as i128;
    const DEPOSIT: u128 = 3_130;
    const WINNING_DOMAIN: usize = 1;
    const FEE_BPS: u16 = 5_000; // 10% of consumed backing
    const INSURANCE_SHARE_BPS: u16 = 2_500; // 25% of the fee to insurance, 75% to provider
    const EXPECTED_SOURCE_CLAIM: u128 = 500;
    const EXPECTED_NET_PNL: i128 = 500;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);
    env.update_backing_fee_policy_with_cu(WINNING_DOMAIN as u16, FEE_BPS, INSURANCE_SHARE_BPS);
    // Reconfigure the asset-0 oracle after setting the policy. Asset 0 carries both market-wide
    // config and a stored per-asset profile; the fee policy must survive this path because the
    // backing-fee collector reads the stored profile.
    env.svm.expire_blockhash();
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 10_000);
    env.top_up_backing_bucket(WINNING_DOMAIN as u16, 1_500, 10);

    // Build cross_account's source-backed positive PnL on asset0 (a long that wins as the mark rises).
    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for (portfolio, asset_index) in [
        (counterparty_account, 0),
        (cross_account, 0),
        (counterparty_account, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
            },
        );
    }
    // Force capital low so the parked positive-PnL claim is NEEDED as IM (triggers the source lien).
    env.force_portfolio_capital_for_benchmark(cross_account, 2_600);
    assert_eq!(
        env.portfolio_state(cross_account).pnl.get(),
        EXPECTED_NET_PNL,
        "setup must retain positive net PnL after complete refresh"
    );

    // Trim the bucket to the exact watermark, then refill generously so the risk increase liens against
    // fresh counterparty backing.
    let (_, g0) = env.market_state();
    assert_eq!(
        g0.source_credit[WINNING_DOMAIN].positive_claim_bound_num,
        EXPECTED_SOURCE_CLAIM * BOUND_SCALE,
        "the source-domain claim must match complete-account positive PnL",
    );
    let surplus = (g0.source_credit[WINNING_DOMAIN].fresh_reserved_backing_num
        - g0.source_credit[WINNING_DOMAIN].positive_claim_bound_num)
        / BOUND_SCALE;
    if surplus > 0 {
        let dest = env.token_account(env.admin.pubkey(), 0);
        env.withdraw_backing_bucket_to_admin_token_with_cu(dest, WINNING_DOMAIN as u16, surplus);
    }
    env.top_up_backing_bucket(WINNING_DOMAIN as u16, 50_000, 10);
    env.deposit(&cross_owner, cross_account, 500);
    env.deposit(&counterparty_owner, counterparty_account, 500);
    env.svm.warp_to_slot(3);

    // Snapshot the three sinks + the payers before the lien-growing trade.
    let (_, gb) = env.market_state();
    let insurance_before = gb.insurance;
    let budget_before = gb.insurance_domain_budget[WINNING_DOMAIN];
    let provider_before = gb.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings;
    let ctot_before = gb.c_tot;
    let cap_cross_before = env.portfolio_state(cross_account).capital.get();
    let cap_cp_before = env.portfolio_state(counterparty_account).capital.get();
    let lien_before: u128 = env
        .portfolio_state(cross_account)
        .source_domains
        .iter()
        .map(|slot| slot.source_lien_counterparty_backing_num.get())
        .sum();

    // Risk-increasing trade: grows the IM source-credit lien -> draws fresh backing -> charges the fee.
    let r = env.try_trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        SAFE_INCREASE_Q,
        95,
        0,
    );
    assert!(r.is_ok(), "backed risk increase must succeed: {r:?}");

    let (_, ga) = env.market_state();
    let insurance_delta = ga.insurance - insurance_before;
    let budget_delta = ga.insurance_domain_budget[WINNING_DOMAIN] - budget_before;
    let provider_delta =
        ga.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings - provider_before;
    let cap_cross_after = env.portfolio_state(cross_account).capital.get();
    let cap_cp_after = env.portfolio_state(counterparty_account).capital.get();
    let lien_after: u128 = env
        .portfolio_state(cross_account)
        .source_domains
        .iter()
        .map(|slot| slot.source_lien_counterparty_backing_num.get())
        .sum();
    let charged = (cap_cross_before - cap_cross_after) + (cap_cp_before - cap_cp_after);

    // The trade must actually grow the counterparty-backing lien (else the fee is vacuous).
    assert!(
        lien_after > lien_before,
        "trade must grow the counterparty-backing lien (before={lien_before} after={lien_after})"
    );
    assert!(charged > 0, "a positive backing fee must be charged");
    // Route 1+2+3: charged splits exactly into insurance pool + provider earnings, no atoms created/lost.
    assert_eq!(insurance_delta + provider_delta, charged, "fee must split with no leakage: ins={insurance_delta} prov={provider_delta} charged={charged}");
    // Insurance share is floor(charged * share_bps): asset-0 pool and the per-domain budget move together.
    let expected_insurance = charged * INSURANCE_SHARE_BPS as u128 / 10_000;
    assert_eq!(
        insurance_delta, expected_insurance,
        "insurance pool gets floor(fee*share)"
    );
    assert_eq!(
        budget_delta, insurance_delta,
        "per-domain insurance budget mirrors the insurance share"
    );
    // c_tot drops by exactly the fee debited from collateral (insurance + provider leave c_tot).
    assert_eq!(
        ctot_before - ga.c_tot,
        charged,
        "c_tot decreases by exactly the charged fee"
    );
    assert_domain_budget_remaining_total_consistent(&ga, "backing fee insurance share");
}

#[test]
fn v16_bpf_permissionless_crank_computes_funding_from_internal_mark_premium() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, DEPOSIT);
    env.deposit(&short_owner, short_account, DEPOSIT);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2);
    env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    let (cfg_after_first, group_after_first) = env.market_state();
    assert_eq!(cfg_after_first.mark_ewma_e6, 1_500_000);
    assert_eq!(group_after_first.assets[0].effective_price, 1_100_000);
    assert_eq!(
        group_after_first.funding_epoch, 0,
        "a newly pushed mark must not retroactively charge funding before its slot"
    );
    assert_eq!(group_after_first.assets[0].f_long_num, 0);
    assert_eq!(group_after_first.assets[0].f_short_num, 0);

    env.svm.warp_to_slot(2);
    let funding_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    assert_cu_within(
        "permissionless computed funding crank",
        funding_cu,
        CRANK_CU_LIMIT,
    );
    let (_, funded_group) = env.market_state();
    assert_eq!(funded_group.funding_epoch, 1);
    assert_eq!(
        funded_group.assets[0].effective_price, 1_200_000,
        "the two-slot movement envelope is linear from the stable trajectory anchor"
    );
    assert_eq!(funded_group.assets[0].f_long_num, -(ADL_ONE as i128));
    assert_eq!(funded_group.assets[0].f_short_num, ADL_ONE as i128);
}

#[test]
fn v16_bpf_existing_funding_ledger_refreshes_and_converts_between_sides() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const FUNDING_RATE_E9: i128 = 1_000;
    const DEPOSIT: u128 = 2_000_000;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, DEPOSIT);
    env.deposit(&short_owner, short_account, DEPOSIT);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    env.mutate_market(|_, group| {
        let out = group
            .accrue_asset_to_not_atomic(0, 1, INITIAL_PRICE, FUNDING_RATE_E9, true)
            .unwrap();
        assert!(out.funding_active);
        group.assets[0].raw_oracle_target_price = INITIAL_PRICE;
    });
    env.svm.warp_to_slot(1);
    let (_, funded_group) = env.market_state();
    assert_eq!(funded_group.funding_epoch, 1);
    assert_eq!(funded_group.assets[0].f_long_num, -(ADL_ONE as i128));
    assert_eq!(funded_group.assets[0].f_short_num, ADL_ONE as i128);

    let long_refresh_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    assert_cu_within(
        "funding smoke long loss refresh",
        long_refresh_cu,
        CRANK_CU_LIMIT,
    );
    let long_after = env.portfolio_state(long_account);
    assert_eq!(long_after.pnl.get(), 0);
    assert_eq!(long_after.capital.get(), DEPOSIT - 1);

    let short_refresh_cu = env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    assert_cu_within(
        "funding smoke short gain refresh",
        short_refresh_cu,
        CRANK_CU_LIMIT,
    );
    let short_after = env.portfolio_state(short_account);
    assert_eq!(short_after.pnl.get(), 1);
    assert_eq!(short_after.capital.get(), DEPOSIT);

    let close_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        INITIAL_PRICE,
        0,
    );
    assert_cu_within(
        "funding smoke close funded position",
        close_cu,
        TRADE_CU_LIMIT,
    );
    let long_flat = env.portfolio_state(long_account);
    let short_flat = env.portfolio_state(short_account);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &long_flat
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &short_flat
    )));
    assert_eq!(long_flat.capital.get(), DEPOSIT - 1);
    assert_eq!(short_flat.pnl.get(), 1);

    let convert_cu = env.convert_released_pnl_with_cu(&short_owner, short_account, 1);
    assert_cu_within(
        "funding smoke convert released pnl",
        convert_cu,
        CUSTODY_CU_LIMIT,
    );
    let short_after_convert = env.portfolio_state(short_account);
    assert_eq!(short_after_convert.pnl.get(), 0);
    assert_eq!(short_after_convert.capital.get(), DEPOSIT + 1);

    let (_, group) = env.market_state();
    assert_eq!(group.c_tot, DEPOSIT * 2);
    assert_eq!(group.vault, DEPOSIT * 2);
}

// regression (security.md sweep): premium-funding + price-move settlement value-conservation.
// Balanced long/short with a persistent mark premium so funding accrues across slots. Probe whether
// funding/price settlement creates or destroys net VAULT value, breaks senior conservation, or
// leaves the winner unbacked. (Initial probe fired on a too-narrow Σ(capital+pnl)==deposits invariant
// — funding fees accrue to insurance and §6.2 warmup holds an in-vault residual; widened below.)
#[test]
fn v16_regression_premium_funding_settlement_conserves_vault() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    // Push the mark premium ONCE at slot 1 (anti-retroactivity: it won't charge funding that slot),
    // then crank subsequent slots WITHOUT re-pushing so the established premium accrues funding.
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2);
    let crank_both = |env: &mut V16CuEnv, slot: u64| {
        for acct in [lo, sh] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(acct, false),
                ],
                &[],
            );
        }
    };
    crank_both(&mut env, 1);
    for slot in 2..=5u64 {
        env.svm.warp_to_slot(slot);
        crank_both(&mut env, slot);
    }
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // funding must actually have accrued (non-vacuous): the ledger moved off zero.
    assert!(
        g.assets[0].f_long_num != 0 || g.assets[0].f_short_num != 0,
        "funding actually accrued"
    );
    // Correct (widened) conservation invariant. NOTE: the account-level sum Σ(capital+pnl) is NOT
    // == deposits here, because funding fees legitimately accrue to INSURANCE and the §6.2 warmup
    // holds a RESIDUAL buffer in-vault before crediting the winner. The real guarantees are:
    //   1) no tokens minted/burned: the vault still holds exactly the deposited amount,
    //   2) senior conservation: vault >= c_tot + insurance,
    //   3) winner backed: positive pnl <= residual,
    //   4) no over-distribution: Σ(capital+pnl) + insurance <= vault.
    assert_eq!(
        g.vault,
        2 * DEPOSIT,
        "no tokens minted or burned: vault == total deposited"
    );
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation under funding"
    );
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(
        residual >= a.pnl.get().max(0) + b.pnl.get().max(0),
        "positive pnl backed by residual"
    );
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert!(
        total_equity + g.insurance as i128 <= g.vault as i128,
        "no value over-distributed beyond the vault"
    );
    assert!(
        g.assets[0].f_long_num < 0 && g.assets[0].f_short_num > 0,
        "longs pay shorts under mark premium"
    );
}
