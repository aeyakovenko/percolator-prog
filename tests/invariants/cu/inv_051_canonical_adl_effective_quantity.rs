//! INV-051 - Canonical ADL-effective quantity.
//!
//! Normative obligation: every route uses the engine's pooled effective OI as the amount that can
//! still be reduced, while raw per-portfolio basis remains an attribution record. A route that
//! consumes a leg's final effective exposure must clear its retained raw basis in the same
//! transition. If the side-wide A index still needs normalization after pooled OI reaches zero,
//! the side enters `ResetPending` and one bounded permissionless finalizer restores the unit index.
//!
//! Evidence in this file (I/C/M over public routes): the crossed-trade and owner-signed unilateral
//! matrices independently create partial ADL through ordinary deposits, trades, authenticated
//! marks, maintenance, and permissionless cranks. Each then consumes exactly the remaining pooled
//! OI through a different public route. Both require `(oi_long, oi_short) == (0, 0)`, a
//! `ResetPending` side, exact rollback from absent-leg retries and reset-time risk reopening, one
//! bounded side finalizer, conserved SPL custody, and recovery
//! of the owner's remaining capital. The liquidation matrix exercises the same zero-effective-OI
//! boundary through public maintenance-fee pressure and permissionless liquidation. The stateful
//! global oracle in `support/fuzz_model.rs` applies the same zero-OI/reset condition after every
//! successful generated public instruction.
//! Secondary coverage: INV-073, because each matrix also proves that the funded owner has a
//! bounded public cleanup and capital-exit sequence after pooled OI reaches zero.
//!
//! Current-surface closure composes these directed routes with INV-048's source-complete roster of
//! every wrapper position mutation and the pinned engine's canonical attach/resize/clear,
//! effective-quantity inverse, and OI contracts. INV-077 adds four maximum-shape order worlds with
//! fourteen active legs, twenty-eight source domains, eleven authenticated liquidation episodes,
//! and four raw-basis owner reductions; every episode independently matches canonical effective
//! quantity and equal two-sided OI removal. Transfer/import and caller-sized liquidation are absent
//! from the public wrapper. A new position transition, wrapper OI writer, engine pin, or supported
//! shape reopens this closure.

use super::*;

#[test]
fn v16_program_crossed_adl_effective_exit_matrix_preserves_bounded_cleanup() {
    super::inv_073_no_permanent_user_lock::assert_inv_051_crossed_adl_effective_exit_matrix_preserves_bounded_cleanup();
}

#[test]
fn v16_program_unilateral_adl_effective_exit_matrix_preserves_bounded_cleanup() {
    super::inv_073_no_permanent_user_lock::assert_inv_051_unilateral_adl_effective_exit_matrix_preserves_bounded_cleanup();
}

#[test]
fn v16_program_liquidation_adl_effective_exit_matrix_preserves_bounded_cleanup() {
    super::inv_073_no_permanent_user_lock::assert_inv_051_liquidation_adl_effective_exit_matrix_preserves_bounded_cleanup();
}

#[test]
fn v16_program_partitioned_adl_reset_flip_and_resolved_clear_preserve_leg_census() {
    use support::reference_math::mul_div_ceil;

    let run = |split: bool| {
        let q = POS_SCALE as i128;
        let mut env = V16CuEnv::new();
        env.configure_auth_mark_with_cu(0, 100);
        let owners = std::array::from_fn::<_, 4, _>(|_| Keypair::new());
        let accounts = owners.each_ref().map(|owner| env.create_portfolio(owner));
        for i in 0..4 {
            env.deposit(&owners[i], accounts[i], 1_000_000);
        }

        let census = |env: &V16CuEnv, expected: [i128; 4]| {
            let group = env.market_state().1;
            let asset = group.assets[0];
            let mut oi = [0u128; 2];
            let mut counts = [0u64; 2];
            let mut weights = [0u128; 2];
            let mut capital = 0;
            let legs = accounts.map(|key| {
                let state = env.portfolio_state(key);
                capital += state.capital.get();
                assert_eq!(state.pnl.get(), 0, "constant-price fixture has no PnL");
                let mut seen = false;
                let mut position = (0i128, 0i128, 0u128, 0u128);
                for (slot, stored) in state.legs.iter().enumerate() {
                    let leg = stored.try_to_runtime().expect("decode every leg");
                    assert_eq!(
                        percolator::active_bitmap_get(active_bitmap(&state), slot),
                        leg.active,
                    );
                    if !leg.active {
                        continue;
                    }
                    assert_eq!(leg.asset_index, 0);
                    assert!(!seen, "duplicate logical leg in portfolio {key}");
                    seen = true;
                    let (side, a, epoch, sign) = match leg.side {
                        SideV16::Long => (0, asset.a_long, asset.epoch_long, 1),
                        SideV16::Short => (1, asset.a_short, asset.epoch_short, -1),
                    };
                    assert_eq!(leg.epoch_snap, epoch);
                    assert_eq!(leg.basis_pos_q.signum(), sign);
                    assert!(a > 0 && a <= leg.a_basis);
                    let raw = leg.basis_pos_q.unsigned_abs();
                    let effective = mul_div_ceil(raw, a, leg.a_basis).unwrap();
                    let weight =
                        mul_div_ceil(raw, percolator::SOCIAL_WEIGHT_SCALE, leg.a_basis).unwrap();
                    assert_eq!(leg.loss_weight, weight);
                    oi[side] += effective;
                    counts[side] += 1;
                    weights[side] += weight;
                    position = (
                        sign * effective as i128,
                        leg.basis_pos_q,
                        leg.a_basis,
                        weight,
                    );
                }
                position
            });
            assert_eq!(legs.map(|leg| leg.0), expected);
            assert_eq!(oi, [asset.oi_eff_long_q, asset.oi_eff_short_q]);
            assert_eq!(
                counts,
                [asset.stored_pos_count_long, asset.stored_pos_count_short]
            );
            assert_eq!(
                weights,
                [asset.loss_weight_sum_long, asset.loss_weight_sum_short]
            );
            assert_eq!(
                [
                    asset.pending_obligation_count_long,
                    asset.pending_obligation_count_short
                ],
                [0; 2],
            );
            if group.mode == MarketModeV16::Live {
                assert_eq!(expected.iter().sum::<i128>(), 0);
                assert_eq!(oi[0], oi[1]);
            }
            assert_eq!(group.c_tot, capital);
            assert_eq!(group.vault, capital);
            assert_eq!(group.insurance, 0);
            assert_eq!(group.vault, env.token_amount(env.vault) as u128);
            (
                legs,
                oi,
                counts,
                weights,
                [asset.a_long, asset.a_short],
                capital,
            )
        };
        let mut max_trade_cu = 0;
        let trade = |env: &mut V16CuEnv, taker: usize, maker: usize, size: i128| {
            env.svm.expire_blockhash();
            let cu = env.trade_asset_with_cu(
                0,
                &owners[taker],
                accounts[taker],
                &owners[maker],
                accounts[maker],
                size,
                100,
                0,
            );
            assert_cu_within("INV-048/050/051 matched leg census", cu, TRADE_CU_LIMIT);
            cu
        };
        let mut expected = [0; 4];
        for (long, short) in [(0, 1), (2, 3)] {
            max_trade_cu = max_trade_cu.max(trade(&mut env, long, short, 4 * q));
            expected[long] = 4 * q;
            expected[short] = -4 * q;
            census(&env, expected);
        }

        // Dyadic ratios make the independent sum exact: both untouched short
        // legs retain 4Q raw basis but end with 2Q effective exposure each.
        let mut max_reduce_cu = 0;
        let parts = if split {
            vec![2 * q, 2 * q]
        } else {
            vec![4 * q]
        };
        for part in parts {
            env.svm.expire_blockhash();
            let cu = env.rebalance_reduce_with_cu(&owners[0], accounts[0], 0, part as u128);
            assert_cu_within("partitioned owner ADL", cu, CUSTODY_CU_LIMIT);
            max_reduce_cu = max_reduce_cu.max(cu);
            expected[0] -= part;
            expected[1] = -(expected[0] + expected[2]) / 2;
            expected[3] = expected[1];
            census(&env, expected);
        }
        let mut checkpoints = vec![census(&env, [0, -2 * q, 4 * q, -2 * q])];
        assert_eq!(env.market_state().1.assets[0].a_short, ADL_ONE / 2);
        for short in [1, 3] {
            assert_eq!(
                active_leg_for_asset(&env.portfolio_state(accounts[short]), 0).basis_pos_q,
                -4 * q,
            );
        }

        // Clearing one ADL leg must not consume its peer's exposure or count.
        for short in [1, 3] {
            max_trade_cu = max_trade_cu.max(trade(&mut env, 2, short, -2 * q));
            expected[2] -= 2 * q;
            expected[short] = 0;
            checkpoints.push(census(&env, expected));
        }
        assert_eq!(
            env.market_state().1.assets[0].mode_short,
            SideModeV16::ResetPending
        );
        let reset_cu = env.finalize_reset_side_with_cu(0, 1);
        assert_cu_within("ADL census side reset", reset_cu, CUSTODY_CU_LIMIT);
        let reset = env.market_state().1.assets[0];
        assert_eq!(
            [reset.mode_long, reset.mode_short],
            [SideModeV16::Normal; 2]
        );
        assert_eq!([reset.a_long, reset.a_short], [ADL_ONE; 2]);
        checkpoints.push(census(&env, [0; 4]));

        // Reuse all four former legs, flip the first pair, then grow and reduce
        // the new sides while the second matched pair remains present.
        for (taker, maker, size) in [
            (0, 1, 3 * q),
            (2, 3, q),
            (0, 1, -5 * q),
            (0, 1, -q),
            (0, 1, q),
        ] {
            max_trade_cu = max_trade_cu.max(trade(&mut env, taker, maker, size));
            expected[taker] += size;
            expected[maker] -= size;
            checkpoints.push(census(&env, expected));
        }
        assert_eq!(expected, [-2 * q, 2 * q, q, -q]);

        let resolve_cu = env.resolve();
        assert_cu_within("ADL census resolve", resolve_cu, CRANK_CU_LIMIT);
        assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
        checkpoints.push(census(&env, expected));
        let mut max_clear_cu = 0;
        for actor in [0, 3, 1, 2] {
            let (destination, cu) = env.close_resolved_with_cu(&owners[actor], accounts[actor]);
            assert_cu_within("ADL census resolved clear", cu, CUSTODY_CU_LIMIT);
            max_clear_cu = max_clear_cu.max(cu);
            assert_eq!(env.token_amount(destination), 1_000_000);
            expected[actor] = 0;
            checkpoints.push(census(&env, expected));
        }
        assert_eq!(env.market_state().1.vault, 0);
        println!(
            "split={split}: trade={max_trade_cu}, reduce={max_reduce_cu}, reset={reset_cu}, resolve={resolve_cu}, resolved_clear={max_clear_cu} CU"
        );
        checkpoints
    };
    assert_eq!(
        run(false),
        run(true),
        "ADL partition changed the canonical book history"
    );
}

// The fee cap is not a liveness gate for no-CPI EWMA discovery. If the full EWMA candidate would
// require more fee than the market cap allows, the trade still executes and the internal mark move
// Trade-driven EWMA discovery may advance while the engine's effective price remains at its old
// anchor. Exercise the maximum valid uncranked price envelope over many alternating wash trades:
// movement fees must still cover the attacker's eventual base-unit repricing gain.

// A permissionless asset creator controls that asset's oracle and can intentionally bankrupt its
// own book. Even when this activates the engine's bankruptcy hlock, unrelated base-asset trading
// must remain live; local loss isolation is insufficient if the global flag freezes normal users.

// security.md sweep — ADL deleverage precision/conservation (#9/#22/#33): when a bankrupt side is
// partially liquidated, the engine auto-deleverages the WINNING (opposite) side by scaling its a-factor
// by oi_after/oi_before (percolator/src/v16.rs:9834). Attacker goal: have the winner keep its full claim
// while the loser's shortfall is socialized (value creation), or have the deleverage mint vault value.
// Protection: the winner's a-factor is reduced exactly proportionally, and the vault is never minted.
#[test]
fn v16_attack_adl_deleverage_conserves_and_shrinks_winner_claim() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new();
    let a = env.create_portfolio(&la); // long = winner (gets deleveraged)
    let lb = Keypair::new();
    let b = env.create_portfolio(&lb); // short = loser (driven insolvent)
    env.deposit(&la, a, 1_000);
    env.deposit(&lb, b, 900);
    env.trade_asset_with_cu(0, &la, a, &lb, b, (2 * POS_SCALE) as i128, 100, 0);
    let g0 = env.market_state().1;
    assert_eq!(g0.assets[0].a_long, ADL_ONE, "a_long starts at ADL_ONE");
    assert_eq!(g0.assets[0].a_short, ADL_ONE, "a_short starts at ADL_ONE");
    let oi_long_pre = g0.assets[0].oi_eff_long_q;
    assert_eq!(oi_long_pre, 2 * POS_SCALE, "balanced OI of 2*POS_SCALE");
    let vault_pre = g0.vault;

    // price 1x->5x: the short is under maintenance but still has enough capital to avoid
    // recovery-mode bankruptcy, so this reaches the live liquidation/ADL path.
    env.svm.warp_to_slot(6);
    env.push_auth_mark_with_cu(6, 500);
    for p in [b, a] {
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
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
    // Engine-selected partial liquidation restores health and proportionally deleverages the winner.
    env.crank_steps_after_market_catchup(
        b,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations(0),
        },
        2,
    );

    let g1 = env.market_state().1;
    let oi_long_post = g1.assets[0].oi_eff_long_q;
    // ADL TRIGGERED: the WINNING long side is deleveraged exactly proportionally to the OI it lost.
    assert!(
        oi_long_post < oi_long_pre,
        "winning-side OI reduced by the liquidation"
    );
    let expected_a_long = (ADL_ONE as u128) * oi_long_post / oi_long_pre;
    assert_eq!(
        g1.assets[0].a_long, expected_a_long,
        "a_long deleveraged exactly oi_after/oi_before"
    );
    assert!(
        g1.assets[0].a_long < ADL_ONE,
        "winner's claim factor strictly shrunk (ADL applied, non-vacuous)"
    );
    assert_eq!(
        g1.assets[0].a_short, ADL_ONE,
        "bankrupt (short) side a-factor unchanged"
    );
    // CONSERVATION: the deleverage mints NOTHING — vault unchanged, senior conservation holds.
    assert_eq!(g1.vault, vault_pre, "ADL deleverage minted no vault value");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation through ADL"
    );
    assert_eq!(
        g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q,
        "OI still balanced post-liquidation"
    );
}

// security.md sweep — ADL deleverage + subsequent settlement interaction (#9/#22/#33): after a partial
// liquidation deleverages the winning side (a_long < ADL_ONE), the winner's NEXT mark settlement uses
// its a_basis vs the reduced a_long (scaled_adl_delta). Attacker goal: have the winner still realize its
// FULL pre-ADL gain into spendable capital (escaping the deleverage), or have the combined ADL+settle
// sequence mint vault value. Protection: the winner's realizable value stays bounded by capital+residual
// and the vault is never minted across the whole sequence. (Interaction not covered by single-mechanism
// tests: #141 tests ADL's a-factor; this exercises ADL THEN settlement of the deleveraged leg.)
#[test]
fn v16_attack_adl_then_settlement_winner_cannot_escape_deleverage() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new();
    let a = env.create_portfolio(&la); // long winner
    let lb = Keypair::new();
    let b = env.create_portfolio(&lb); // short loser
    env.deposit(&la, a, 1_000);
    env.deposit(&lb, b, 900);
    env.trade_asset_with_cu(0, &la, a, &lb, b, (2 * POS_SCALE) as i128, 100, 0);
    let vault0 = env.market_state().1.vault; // the only real tokens in the system

    // Price up: short is under maintenance but not bankrupt; settle both, then let the engine select
    // the health-restoring partial liquidation so a_long deleverages.
    env.svm.warp_to_slot(6);
    env.push_auth_mark_with_cu(6, 500);
    for p in [b, a] {
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
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
    env.crank_steps_after_market_catchup(
        b,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations(0),
        },
        2,
    );
    let g_adl = env.market_state().1;
    assert!(
        g_adl.assets[0].a_long < ADL_ONE,
        "winner deleveraged (ADL engaged), a_long={}",
        g_adl.assets[0].a_long
    );

    // SECOND mark move + crank the winner: this settles the deleveraged leg (a_basis vs reduced a_long).
    env.svm.warp_to_slot(7);
    env.push_auth_mark_with_cu(7, 800);
    env.crank(
        a,
        ProgInstruction::PermissionlessCrank {
            now_slot: 7,
            observations: crank_observations(0),
        },
    );

    let win = env.portfolio_state(a);
    let g = env.market_state().1;
    // non-vacuity: the winner really does carry a paper gain after the moves.
    assert!(
        win.pnl.get() > 0,
        "winner carries a positive paper gain (non-vacuous), pnl={}",
        win.pnl.get()
    );
    // NO MINT across the whole ADL+settle sequence: the vault still holds exactly the original deposits.
    assert_eq!(g.vault, vault0, "ADL + settlement minted no vault tokens");
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    // the winner cannot escape the deleverage: its REALIZABLE value (capital + backed pnl) is bounded by
    // capital + residual — the deleveraged/unbacked gain is NOT spendable (certified equity reflects it).
    let residual = g.vault.saturating_sub(g.c_tot).saturating_sub(g.insurance);
    assert!(
        health_cert(&win).valid,
        "winner cert valid after settlement"
    );
    assert!(
        (health_cert(&win).certified_equity as u128) <= win.capital.get() + residual + 1,
        "winner realizable value bounded by capital+residual (deleverage not escaped): eq={} cap={} residual={}",
        health_cert(&win).certified_equity, win.capital.get(), residual
    );
    // The deleverage caps how much the winner can realize: the surviving gain equals exactly the backed
    // portion (a winner can never pull more than the system holds — and the vault was never minted, above).
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation through ADL + settlement"
    );
}
