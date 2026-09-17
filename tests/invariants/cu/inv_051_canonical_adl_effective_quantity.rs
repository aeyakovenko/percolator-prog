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
//! The nonunit-index partition matrix adds aggregate/split/reversed owner reductions after the
//! reducing owner has already been ADL-haircutted. Independent raw-basis and effective-quantity
//! equations are checked at each prefix, including one-atom requests, exact stale-request rollback,
//! both side resets and full principal withdrawal. This finite fixed-price matrix does not cover
//! changing-price accrual, source liens or maximum-shape work.
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

#[path = "inv_051_dual_adl_matched_partitions.rs"]
mod dual_adl_matched_partitions;

#[test]
fn v16_program_nonunit_adl_reduction_partitions_preserve_raw_basis_and_funded_exit() {
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    // One quantity atom must reduce risk notional, or the public progress gate rejects dust.
    const PRICE: u64 = POS_SCALE as u64;
    const CAPITAL: u128 = 10 * POS_SCALE;
    let mut worlds = 0;
    let mut reductions = 0;
    let mut rejections = 0;
    let mut raw_subtraction_controls = 0;
    let mut inverse_ceil_controls = 0;
    let mut max_cu = 0;
    for open_q in [11, 3 * POS_SCALE + 7] {
        for sign in [-1i128, 1] {
            let effective_q = open_q - open_q / 3;
            let partitions = [
                vec![effective_q - 1],
                vec![1, effective_q - 2],
                vec![effective_q - 2, 1],
            ];
            let mut canonical_endpoint = None;
            for parts in partitions {
                let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
                    initial_price: PRICE,
                    ..V16CuMarketParams::default()
                });
                env.configure_auth_mark_with_cu(0, PRICE);
                let seed_owner = Keypair::new();
                let owner = Keypair::new();
                let seed = env.create_portfolio(&seed_owner);
                let target = env.create_portfolio(&owner);
                let seed_tokens = env.deposit(&seed_owner, seed, CAPITAL);
                let target_tokens = env.deposit(&owner, target, CAPITAL);
                env.trade_asset_with_cu(
                    0,
                    &seed_owner,
                    seed,
                    &owner,
                    target,
                    sign * open_q as i128,
                    PRICE,
                    0,
                );
                let cu = env.rebalance_reduce_with_cu(&seed_owner, seed, 0, open_q / 3);
                assert_cu_within("nonunit ADL partition setup", cu, CUSTODY_CU_LIMIT);
                max_cu = max_cu.max(cu);
                let staged = env.market_state().1;
                let staged_leg = active_leg_for_asset(&env.portfolio_state(target), 0);
                let current_a = if sign == 1 {
                    staged.assets[0].a_short
                } else {
                    staged.assets[0].a_long
                };
                assert!(current_a > 0 && current_a < staged_leg.a_basis);
                assert_eq!(
                    current_a,
                    support::reference_math::mul_div_floor(ADL_ONE, effective_q, open_q).unwrap()
                );
                assert_eq!(staged_leg.basis_pos_q, -sign * open_q as i128);
                assert_eq!(
                    reference_current_epoch_effective_abs(&staged, staged_leg),
                    effective_q
                );
                // These seeds distinguish ceil exposure from floor, including sub-unit positions.
                assert_ne!(
                    support::reference_math::mul_div_floor(open_q, current_a, staged_leg.a_basis)
                        .unwrap(),
                    effective_q,
                );
                let tracked = [
                    env.market,
                    seed,
                    target,
                    env.vault,
                    env.mint,
                    seed_tokens,
                    target_tokens,
                    seed_owner.pubkey(),
                    owner.pubkey(),
                    env.admin.pubkey(),
                ];
                let frame = |env: &V16CuEnv| tracked.map(|key| env.svm.get_account(&key).unwrap());
                let request = |env: &V16CuEnv, reduce_q| ProgInstruction::RebalanceReduce {
                    portfolio_id: env.portfolio_id(target),
                    position_epoch: env.portfolio_position_epoch(target),
                    asset_index: 0,
                    reduce_q,
                };
                let reject = |env: &mut V16CuEnv, instruction: ProgInstruction, error| {
                    let before = frame(env);
                    env.svm.expire_blockhash();
                    let ix = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(target, false),
                        ],
                        data: instruction.encode(),
                    };
                    let tx = Transaction::new_signed_with_payer(
                        &[heap_ix(), cu_ix(), ix],
                        Some(&env.payer.pubkey()),
                        &[&env.payer, &owner],
                        env.svm.latest_blockhash(),
                    );
                    let failed = env
                        .svm
                        .send_transaction(tx)
                        .expect_err("invalid reduction must reject");
                    assert_eq!(
                        failed.err,
                        TransactionError::InstructionError(2, InstructionError::Custom(error))
                    );
                    assert_eq!(
                        frame(env),
                        before,
                        "rejection must frame all economic accounts"
                    );
                    assert_cu_within(
                        "nonunit ADL rejected reduction",
                        failed.meta.compute_units_consumed,
                        CUSTODY_CU_LIMIT,
                    );
                    failed.meta.compute_units_consumed
                };
                let zero = request(&env, 0);
                max_cu = max_cu.max(reject(
                    &mut env,
                    zero,
                    PercolatorError::InvalidInstruction as u32,
                ));
                rejections += 1;
                let mut remaining = effective_q;
                for reduce_q in parts {
                    let before = env.market_state().1;
                    let old_leg = active_leg_for_asset(&env.portfolio_state(target), 0);
                    let passive = env.svm.get_account(&seed).unwrap();
                    let stale_request = request(&env, reduce_q);
                    remaining -= reduce_q;
                    let expected_raw =
                        reference_raw_basis_for_current_effective(&before, old_leg, remaining);
                    raw_subtraction_controls +=
                        usize::from(old_leg.basis_pos_q.unsigned_abs() - reduce_q != expected_raw);
                    inverse_ceil_controls += usize::from(
                        support::reference_math::mul_div_ceil(
                            remaining,
                            old_leg.a_basis,
                            current_a,
                        )
                        .unwrap()
                            != expected_raw,
                    );
                    env.svm.expire_blockhash();
                    let cu = env.rebalance_reduce_with_cu(&owner, target, 0, reduce_q);
                    assert_cu_within("nonunit ADL partition reduction", cu, CUSTODY_CU_LIMIT);
                    max_cu = max_cu.max(cu);
                    reductions += 1;
                    let after = env.market_state().1;
                    let next_leg = active_leg_for_asset(&env.portfolio_state(target), 0);
                    assert_eq!(next_leg.basis_pos_q, -sign * expected_raw as i128);
                    assert_eq!(next_leg.a_basis, old_leg.a_basis);
                    assert_eq!(
                        if sign == 1 {
                            after.assets[0].a_short
                        } else {
                            after.assets[0].a_long
                        },
                        current_a
                    );
                    assert_eq!(
                        reference_current_epoch_effective_abs(&after, next_leg),
                        remaining
                    );
                    assert_eq!(
                        (
                            after.assets[0].oi_eff_long_q,
                            after.assets[0].oi_eff_short_q
                        ),
                        (remaining, remaining)
                    );
                    assert_eq!(env.svm.get_account(&seed).unwrap(), passive);
                    assert_eq!(
                        (after.vault, after.c_tot, after.insurance),
                        (2 * CAPITAL, 2 * CAPITAL, 0)
                    );
                    assert_eq!(env.token_amount(env.vault) as u128, after.vault);
                    for portfolio in [seed, target] {
                        let account = env.portfolio_state(portfolio);
                        assert_eq!(
                            (
                                account.capital.get(),
                                account.pnl.get(),
                                account.fee_credits.get()
                            ),
                            (CAPITAL, 0, 0)
                        );
                    }
                    max_cu = max_cu.max(reject(
                        &mut env,
                        stale_request,
                        PercolatorError::EngineProvenanceMismatch as u32,
                    ));
                    rejections += 1;
                }
                assert_eq!(remaining, 1);
                let endpoint = active_leg_for_asset(&env.portfolio_state(target), 0);
                let endpoint = (endpoint.basis_pos_q, endpoint.a_basis);
                assert_eq!(
                    *canonical_endpoint.get_or_insert(endpoint),
                    endpoint,
                    "aggregate, split and reversed reductions retain identical owner basis"
                );
                env.svm.expire_blockhash();
                let cu = env.rebalance_reduce_with_cu(&owner, target, 0, 1);
                assert_cu_within("nonunit ADL one-atom exit", cu, CUSTODY_CU_LIMIT);
                max_cu = max_cu.max(cu);
                assert!(!has_active_leg_for_asset(&env.portfolio_state(target), 0));
                let closed = env.market_state().1;
                assert_eq!(
                    (
                        closed.assets[0].oi_eff_long_q,
                        closed.assets[0].oi_eff_short_q
                    ),
                    (0, 0)
                );
                assert_eq!(
                    (closed.assets[0].mode_long, closed.assets[0].mode_short),
                    (SideModeV16::ResetPending, SideModeV16::ResetPending)
                );
                let cu = env
                    .send(
                        ProgInstruction::PermissionlessCrank {
                            now_slot: env.svm.get_sysvar::<Clock>().slot,
                            observations: Vec::new(),
                        },
                        vec![
                            AccountMeta::new(env.payer.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(seed, false),
                        ],
                        &[],
                    )
                    .expect("zero-effective passive leg has bounded permissionless cleanup");
                assert_cu_within("nonunit ADL passive cleanup", cu, CUSTODY_CU_LIMIT);
                max_cu = max_cu.max(cu);
                assert!(!has_active_leg_for_asset(&env.portfolio_state(seed), 0));
                for side in [0, 1] {
                    let cu = env.finalize_reset_side_with_cu(0, side);
                    assert_cu_within("nonunit ADL reset finalization", cu, CUSTODY_CU_LIMIT);
                    max_cu = max_cu.max(cu);
                }
                let reset = env.market_state().1.assets[0];
                assert_eq!((reset.a_long, reset.a_short), (ADL_ONE, ADL_ONE));
                assert_eq!(
                    (reset.mode_long, reset.mode_short),
                    (SideModeV16::Normal, SideModeV16::Normal)
                );
                assert_eq!(
                    (reset.stored_pos_count_long, reset.stored_pos_count_short),
                    (0, 0)
                );
                for (authority, portfolio) in [(&seed_owner, seed), (&owner, target)] {
                    let (destination, cu) = env.withdraw_with_cu(authority, portfolio, CAPITAL);
                    assert_cu_within("nonunit ADL funded withdrawal", cu, CUSTODY_CU_LIMIT);
                    max_cu = max_cu.max(cu);
                    assert_eq!(env.token_amount(destination) as u128, CAPITAL);
                }
                let terminal = env.market_state().1;
                assert_eq!(
                    (terminal.vault, terminal.c_tot, terminal.insurance),
                    (0, 0, 0)
                );
                assert_eq!(env.token_amount(env.vault), 0);
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, reductions, rejections), (12, 20, 32));
    assert_eq!((raw_subtraction_controls, inverse_ceil_controls), (16, 20));
    eprintln!("INV-051 nonunit partitions: worlds={worlds}, partition_reductions={reductions}, exact_rejections={rejections}, raw_subtraction_controls={raw_subtraction_controls}, inverse_ceil_controls={inverse_ceil_controls}, max_exit_cu={max_cu}");
}

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
