//! INV-039 - Pending-loss obligation durability.
//!
//! This CU/SVM owner invokes the shared public Recovery-order world formerly owned by INV-073.
//! Both owner landing orders create a real zero-basis, nonzero-loss-weight obligation. While it is
//! retained, `ClosePortfolio` must return an instruction error with exact market, portfolio, vault,
//! and lamport rollback. The opposite owner then exits, permissionless cranks release the
//! obligation in bounded work, every loss-weight/count aggregate reaches zero, all users receive
//! their exact terminal entitlement, and all portfolios close. A valid pending obligation does not
//! coexist with `ResetPending`: the pinned engine's retain/release/clear contracts and
//! `proof_v16_public_finalize_side_reset_rejects_each_blocker_without_mutation` own that
//! unreachability and exact reset-gate frame, while the wrapper exposes no direct state writer.
//! INV-088 source-rosters every wrapper-to-engine transition, so a new removal route reopens this
//! composition. Secondary coverage: INV-073. The same run proves both landing orders retain a
//! bounded owner exit and permissionless terminal continuation while preserving exact loss
//! attribution, without duplicating an expensive public lifecycle.
//!
//! The `Recovery`-to-`Resolved` order probe below is intentionally RED on the pinned engine. It
//! reaches global Recovery through an unrelated expired close, then proves that resolving the
//! retained winner before its debtor drops that winner's loss weight and transfers two SPL atoms
//! from an independent winner. Its expected frame is the exact debtor-first allocation.

use super::*;

#[test]
fn v16_program_pending_obligation_blocks_close_then_releases() {
    super::inv_073_no_permanent_user_lock::
        verify_recovery_forfeit_orders_preserve_loss_and_terminal_exit();
}

#[derive(Debug, PartialEq, Eq)]
struct RecoveryResolvedOrderOutcome {
    attacker_payout: u128,
    victim_payout: u128,
    debtor_payout: u128,
    auxiliary_winner_payout: u128,
    auxiliary_debtor_payout: u128,
    pending_long_before_debtor: u64,
    weight_long_before_debtor: u128,
    target_b_long_num: u128,
    target_b_short_num: u128,
    target_pending_long: u64,
    target_pending_short: u64,
    target_weight_long: u128,
    target_weight_short: u128,
    vault_remaining: u128,
}

fn try_close_resolved_until_stalled_or_asset_detaches(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    asset_index: usize,
    label: &str,
) -> u128 {
    let mut payout = 0u128;
    for _ in 0..16 {
        if !has_active_leg_for_asset(&env.portfolio_state(portfolio), asset_index) {
            return payout;
        }
        let market_before = env.svm.get_account(&env.market).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        let (destination, result) = env.try_close_resolved_with_cu(owner, portfolio);
        match result {
            Ok(cu) => assert_cu_within(label, cu, CUSTODY_CU_LIMIT),
            Err(error) if is_engine_non_progress_error(&error) => {
                assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
                assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
                return payout;
            }
            Err(error) => panic!("{label}: {error}"),
        }
        payout = payout
            .checked_add(env.token_amount(destination) as u128)
            .expect("resolved payout sum fits u128");
        if env.svm.get_account(&env.market).unwrap() == market_before
            && env.svm.get_account(&portfolio).unwrap() == portfolio_before
            && env.svm.get_account(&env.vault).unwrap() == vault_before
        {
            return payout;
        }
    }
    panic!("{label}: pre-debtor resolved work did not detach or stall");
}

fn close_resolved_until_asset_detaches(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    asset_index: usize,
    label: &str,
) -> u128 {
    let mut payout = 0u128;
    for _ in 0..16 {
        if !has_active_leg_for_asset(&env.portfolio_state(portfolio), asset_index) {
            return payout;
        }
        let (destination, result) = env.try_close_resolved_with_cu(owner, portfolio);
        let cu = result.unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_cu_within(label, cu, CUSTODY_CU_LIMIT);
        payout = payout
            .checked_add(env.token_amount(destination) as u128)
            .expect("resolved payout sum fits u128");
    }
    panic!("{label}: target leg did not detach in bounded public resolved calls");
}

fn run_recovery_resolved_pending_obligation_order(
    attacker_first: bool,
) -> RecoveryResolvedOrderOutcome {
    const OPEN_PRICE: u64 = 100;
    const TARGET_ASSET: u16 = 1;
    const AUXILIARY_ASSET: u16 = 0;
    const SIZE_Q: u128 = POS_SCALE / 50;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_bankrupt_close_lifetime_slots: 2,
        public_b_chunk_atoms: 1,
        ..V16CuMarketParams::default()
    });
    env.configure_permissionless_resolve_with_cu(100, 1);
    env.svm.warp_to_slot(1);
    env.activate_asset(TARGET_ASSET, 1, OPEN_PRICE);
    env.configure_auth_mark_for_asset_as_admin(AUXILIARY_ASSET, 1, OPEN_PRICE);
    env.configure_auth_mark_for_asset_as_admin(TARGET_ASSET, 1, OPEN_PRICE);

    let attacker_owner = Keypair::new();
    let victim_owner = Keypair::new();
    let debtor_owner = Keypair::new();
    let auxiliary_winner_owner = Keypair::new();
    let auxiliary_debtor_owner = Keypair::new();
    let attacker = env.create_portfolio(&attacker_owner);
    let victim = env.create_portfolio(&victim_owner);
    let debtor = env.create_portfolio(&debtor_owner);
    let auxiliary_winner = env.create_portfolio(&auxiliary_winner_owner);
    let auxiliary_debtor = env.create_portfolio(&auxiliary_debtor_owner);
    for (owner, portfolio, amount) in [
        (&attacker_owner, attacker, 10u128),
        (&victim_owner, victim, 10),
        (&debtor_owner, debtor, 4),
        (&auxiliary_winner_owner, auxiliary_winner, 10),
        (&auxiliary_debtor_owner, auxiliary_debtor, 2),
    ] {
        env.deposit(owner, portfolio, amount);
    }

    env.trade_asset_with_cu(
        TARGET_ASSET,
        &attacker_owner,
        attacker,
        &debtor_owner,
        debtor,
        SIZE_Q as i128,
        OPEN_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        TARGET_ASSET,
        &victim_owner,
        victim,
        &debtor_owner,
        debtor,
        SIZE_Q as i128,
        OPEN_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        AUXILIARY_ASSET,
        &auxiliary_winner_owner,
        auxiliary_winner,
        &auxiliary_debtor_owner,
        auxiliary_debtor,
        SIZE_Q as i128,
        OPEN_PRICE,
        0,
    );

    for (slot, mark) in [(2u64, 200u64), (3, 300)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(TARGET_ASSET, slot, mark);
        env.crank(
            attacker,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(TARGET_ASSET),
            },
        );
        env.push_auth_mark_for_asset_as_admin(AUXILIARY_ASSET, slot, mark);
        env.crank(
            auxiliary_winner,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(AUXILIARY_ASSET),
            },
        );
    }
    env.crank(
        debtor,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(TARGET_ASSET),
        },
    );
    env.crank(
        auxiliary_debtor,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(AUXILIARY_ASSET),
        },
    );

    env.svm.warp_to_slot(4);
    env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        TARGET_ASSET,
        4,
        0,
    );
    env.forfeit_recovery_leg_with_cu(&attacker_owner, attacker, TARGET_ASSET, u128::MAX);

    let retained_leg = active_leg_for_asset(&env.portfolio_state(attacker), TARGET_ASSET as usize);
    assert_eq!(retained_leg.basis_pos_q, 0);
    assert!(retained_leg.loss_weight > 0);
    assert!(has_active_leg_for_asset(
        &env.portfolio_state(debtor),
        TARGET_ASSET as usize
    ));
    let retained_target = env.market_state().1.assets[TARGET_ASSET as usize];
    assert_eq!(retained_target.pending_obligation_count_long, 1);
    assert_eq!(retained_target.pending_obligation_count_short, 0);
    assert_eq!(retained_target.stored_pos_count_long, 2);
    assert_eq!(retained_target.stored_pos_count_short, 1);
    assert_eq!(retained_leg.loss_weight, SIZE_Q);
    assert_eq!(retained_target.loss_weight_sum_long, 2 * SIZE_Q);
    assert_eq!(retained_target.loss_weight_sum_short, 2 * SIZE_Q);
    assert_eq!(retained_target.b_long_num, 0);
    assert_eq!(retained_target.b_short_num, 0);

    env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        AUXILIARY_ASSET,
        4,
        0,
    );
    env.forfeit_recovery_leg_with_cu(
        &auxiliary_debtor_owner,
        auxiliary_debtor,
        AUXILIARY_ASSET,
        1,
    );
    let auxiliary_close = close_progress(&env.portfolio_state(auxiliary_debtor));
    assert!(auxiliary_close.active && auxiliary_close.residual_remaining > 0);
    env.svm.warp_to_slot(
        auxiliary_close
            .max_close_slot
            .checked_add(1)
            .expect("auxiliary close expiry fits u64"),
    );
    env.crank(
        auxiliary_debtor,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![],
        },
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Recovery);

    env.crank(
        attacker,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![],
        },
    );
    let (resolved_config, resolved) = env.market_state();
    assert_eq!(resolved.mode, MarketModeV16::Resolved);
    let unresolved_target = resolved.assets[TARGET_ASSET as usize];
    assert_eq!(unresolved_target.pending_obligation_count_long, 1);
    assert_eq!(
        unresolved_target.loss_weight_sum_long,
        retained_target.loss_weight_sum_long
    );

    env.svm.warp_to_slot(
        resolved
            .resolved_slot
            .checked_add(resolved_config.force_close_delay_slots)
            .and_then(|slot| slot.checked_add(1))
            .expect("resolved force-close slot fits u64"),
    );
    let label = if attacker_first {
        "attacker-first Recovery-to-Resolved order"
    } else {
        "debtor-first Recovery-to-Resolved order"
    };
    let mut attacker_payout;
    let mut debtor_payout;
    let pending_long_before_debtor;
    let weight_long_before_debtor;
    if attacker_first {
        attacker_payout = try_close_resolved_until_stalled_or_asset_detaches(
            &mut env,
            &attacker_owner,
            attacker,
            TARGET_ASSET as usize,
            label,
        );
        let after_early_clear = env.market_state().1.assets[TARGET_ASSET as usize];
        pending_long_before_debtor = after_early_clear.pending_obligation_count_long;
        weight_long_before_debtor = after_early_clear.loss_weight_sum_long;
        assert_eq!(after_early_clear.b_long_num, 0);
        debtor_payout = close_resolved_until_asset_detaches(
            &mut env,
            &debtor_owner,
            debtor,
            TARGET_ASSET as usize,
            label,
        );
        attacker_payout += close_resolved_until_asset_detaches(
            &mut env,
            &attacker_owner,
            attacker,
            TARGET_ASSET as usize,
            label,
        );
    } else {
        pending_long_before_debtor = unresolved_target.pending_obligation_count_long;
        weight_long_before_debtor = unresolved_target.loss_weight_sum_long;
        debtor_payout = close_resolved_until_asset_detaches(
            &mut env,
            &debtor_owner,
            debtor,
            TARGET_ASSET as usize,
            label,
        );
        let after_debtor = env.market_state().1.assets[TARGET_ASSET as usize];
        assert_eq!(after_debtor.pending_obligation_count_long, 1);
        assert_eq!(
            after_debtor.loss_weight_sum_long,
            retained_target.loss_weight_sum_long
        );
        assert!(after_debtor.b_long_num > 0);
        attacker_payout = close_resolved_until_asset_detaches(
            &mut env,
            &attacker_owner,
            attacker,
            TARGET_ASSET as usize,
            label,
        );
    }

    let tail_actors = [
        (&attacker_owner, attacker),
        (&victim_owner, victim),
        (&debtor_owner, debtor),
        (&auxiliary_winner_owner, auxiliary_winner),
        (&auxiliary_debtor_owner, auxiliary_debtor),
    ];
    let tail = drain_resolved_cohort(&mut env, &tail_actors, label);
    attacker_payout += tail[0];
    debtor_payout += tail[2];

    let terminal = env.market_state().1;
    let target = terminal.assets[TARGET_ASSET as usize];
    assert_eq!(target.pending_obligation_count_long, 0);
    assert_eq!(target.pending_obligation_count_short, 0);
    assert_eq!(target.loss_weight_sum_long, 0);
    assert_eq!(target.loss_weight_sum_short, 0);
    assert_eq!(target.oi_eff_long_q, 0);
    assert_eq!(target.oi_eff_short_q, 0);
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));

    RecoveryResolvedOrderOutcome {
        attacker_payout,
        victim_payout: tail[1],
        debtor_payout,
        auxiliary_winner_payout: tail[3],
        auxiliary_debtor_payout: tail[4],
        pending_long_before_debtor,
        weight_long_before_debtor,
        target_b_long_num: target.b_long_num,
        target_b_short_num: target.b_short_num,
        target_pending_long: target.pending_obligation_count_long,
        target_pending_short: target.pending_obligation_count_short,
        target_weight_long: target.loss_weight_sum_long,
        target_weight_short: target.loss_weight_sum_short,
        vault_remaining: terminal.vault,
    }
}

#[test]
fn v16_attack_recovery_resolved_cannot_clear_unreleased_loss_weight_before_debtor() {
    let attacker_first = run_recovery_resolved_pending_obligation_order(true);
    let debtor_first = run_recovery_resolved_pending_obligation_order(false);

    eprintln!("attacker-first outcome: {attacker_first:#?}");
    eprintln!("debtor-first outcome: {debtor_first:#?}");
    let expected = RecoveryResolvedOrderOutcome {
        attacker_payout: 12,
        victim_payout: 12,
        debtor_payout: 0,
        auxiliary_winner_payout: 12,
        auxiliary_debtor_payout: 0,
        pending_long_before_debtor: 1,
        weight_long_before_debtor: 2 * (POS_SCALE / 50),
        target_b_long_num: 100_000_000_000_000_000,
        target_b_short_num: 0,
        target_pending_long: 0,
        target_pending_short: 0,
        target_weight_long: 0,
        target_weight_short: 0,
        vault_remaining: 0,
    };
    assert_eq!(debtor_first, expected, "control schedule changed");
    assert_eq!(
        attacker_first, expected,
        "attacker-first Resolved progress cleared retained loss weight before its debtor settled"
    );
}
