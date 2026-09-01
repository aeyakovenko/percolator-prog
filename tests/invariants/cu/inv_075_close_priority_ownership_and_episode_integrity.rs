//! INV-075 - Close priority, ownership, and episode integrity.
//!
//! Normative obligation: close-ledger continuations are scoped to the proper
//! owner and episode, and inert/canceled ledgers cannot be replayed to double
//! book deposits or resurrect stale progress.
//!
//! Evidence in this file (I/C/bounded R): public LiteSVM tests cover non-owner
//! CureAndCancelClose rejection with exact rollback plus successful owner cure,
//! double-cure replay rejection, and both landing orders of two publicly
//! created equal-domain close contenders through permissionless expiry,
//! terminal finalization, and exact terminal settlement of all six economically
//! involved portfolios, including an unrelated live-asset pair. The two landing orders must produce identical per-role
//! payouts, internal/SPL custody, insurance, aggregate capital, OI, and claim
//! counts. The pinned engine implements first-landed exclusive domain
//! ownership, not the charter's strict ClosePriority preemption order;
//! resolving that specification/implementation divergence remains broader
//! model and design work.

use super::*;

fn inv075_close_episode_key(
    ledger: CloseProgressLedgerV16,
) -> (u64, u32, u64, SideV16, u128, u64, u64) {
    (
        ledger.close_id,
        ledger.asset_index,
        ledger.market_id,
        ledger.domain_side,
        ledger.gross_loss_at_close_start,
        ledger.drift_reference_slot,
        ledger.max_close_slot,
    )
}

struct Inv075CompetingCloseFixture {
    env: V16CuEnv,
    base_owners: Vec<Keypair>,
    base_portfolios: Vec<Pubkey>,
    winner_owners: Vec<Keypair>,
    winners: Vec<Pubkey>,
    loss_owners: Vec<Keypair>,
    losses: Vec<Pubkey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Inv075CloseOrderTerminalOutcome {
    payouts: [u128; 6],
    receipts: [(bool, u128, u128, u128, u128, bool); 6],
    engine_vault: u128,
    spl_vault: u64,
    insurance: u128,
    aggregate_capital: u128,
    oi: [(u128, u128); 2],
    negative_pnl_accounts: u64,
}

fn inv075_settle_resolved_to_fixed_point(
    env: &mut V16CuEnv,
    actors: &[(&Keypair, Pubkey); 6],
) -> [u128; 6] {
    let mut payouts = [0u128; 6];
    for round in 0..64 {
        let mut progressed = false;
        for (index, (owner, portfolio)) in actors.iter().enumerate() {
            if resolved_portfolio_is_terminal(env, *portfolio) {
                continue;
            }
            let market_before = env.svm.get_account(&env.market).unwrap();
            let portfolio_before = env.svm.get_account(portfolio).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            let (destination, result) = env.try_close_resolved_with_cu(owner, *portfolio);
            match result {
                Ok(cu) => {
                    assert_cu_within(
                        "INV-041/075 competing-close terminal settlement",
                        cu,
                        CUSTODY_CU_LIMIT,
                    );
                    let paid = u128::from(env.token_amount(destination));
                    payouts[index] = payouts[index]
                        .checked_add(paid)
                        .expect("competing-close payout overflow");
                    assert!(
                        env.svm.get_account(&env.market).unwrap() != market_before
                            || env.svm.get_account(portfolio).unwrap() != portfolio_before
                            || env.svm.get_account(&env.vault).unwrap() != vault_before
                            || paid != 0,
                        "accepted resolved settlement was a no-op in round {round}"
                    );
                    progressed = true;
                }
                Err(error) if is_engine_non_progress_error(&error) => {
                    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                    assert_eq!(env.svm.get_account(portfolio).unwrap(), portfolio_before);
                    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
                    assert_eq!(env.token_amount(destination), 0);
                }
                Err(error) => {
                    panic!("INV-041/075 competing-close terminal settlement failed: {error}")
                }
            }
        }
        if !progressed
            || actors
                .iter()
                .all(|(_, portfolio)| resolved_portfolio_is_terminal(env, *portfolio))
        {
            return payouts;
        }
    }
    panic!("competing-close cohort did not reach a bounded resolved fixed point")
}

fn inv075_competing_public_closes_fixture() -> Inv075CompetingCloseFixture {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_bankrupt_close_lifetime_slots: 2,
        public_b_chunk_atoms: 1,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.update_market_init_fee_policy_with_cu(1);

    // Keep an unrelated base market live while asset 1 enters Recovery.
    let base_long_owner = Keypair::new();
    let base_short_owner = Keypair::new();
    let base_long = env.create_portfolio(&base_long_owner);
    let base_short = env.create_portfolio(&base_short_owner);
    env.deposit(&base_long_owner, base_long, 1_000);
    env.deposit(&base_short_owner, base_short, 1_000);
    env.trade_asset_with_cu(
        0,
        &base_long_owner,
        base_long,
        &base_short_owner,
        base_short,
        POS_SCALE as i128,
        100,
        0,
    );

    let creator = Keypair::new();
    let creator_key = creator.pubkey();
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        100,
        creator_key,
        creator_key,
        creator_key,
        creator_key,
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(1, &creator, 1, 100);

    let mut winner_owners = Vec::new();
    let mut winners = Vec::new();
    let mut loss_owners = Vec::new();
    let mut losses = Vec::new();
    for _ in 0..2 {
        let winner_owner = Keypair::new();
        let loss_owner = Keypair::new();
        let winner = env.create_portfolio(&winner_owner);
        let loss = env.create_portfolio(&loss_owner);
        env.deposit(&winner_owner, winner, 10);
        env.deposit(&loss_owner, loss, 2);
        env.trade_asset_with_cu(
            1,
            &winner_owner,
            winner,
            &loss_owner,
            loss,
            (POS_SCALE / 50) as i128,
            100,
            0,
        );
        winner_owners.push(winner_owner);
        winners.push(winner);
        loss_owners.push(loss_owner);
        losses.push(loss);
    }

    for (slot, mark) in [(2u64, 200u64), (3, 300)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_with_authority(1, &creator, slot, mark);
        for &winner in &winners {
            env.crank(
                winner,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(1),
                },
            );
        }
    }
    for &loss in &losses {
        env.crank(
            loss,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: crank_observations(1),
            },
        );
    }

    env.svm.warp_to_slot(4);
    env.try_shutdown_asset_with_authority(&creator, 1, 4)
        .expect("asset creator shuts down asset 1");
    Inv075CompetingCloseFixture {
        env,
        base_owners: vec![base_long_owner, base_short_owner],
        base_portfolios: vec![base_long, base_short],
        winner_owners,
        winners,
        loss_owners,
        losses,
    }
}

fn inv075_try_forfeit(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
) -> Result<u64, String> {
    let portfolio_id = env.portfolio_id(portfolio);
    let position_epoch = env.portfolio_position_epoch(portfolio);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id,
            position_epoch,
            asset_index: 1,
            b_delta_budget: 1,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[owner],
    )
}

#[test]
fn v16_program_cure_and_cancel_close_owner_gated() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let victim = env.create_portfolio(&owner);
    env.deposit(&owner, victim, 100);
    env.seed_cancellable_close_progress(victim);

    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let attacker_source = env.token_account_for_mint(env.mint, attacker.pubkey(), 50);
    let before_market = env.svm.get_account(&env.market).unwrap().data;
    let before_victim = env.svm.get_account(&victim).unwrap().data;
    let before_source = env.token_amount(attacker_source);
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CureAndCancelClose {
            portfolio_id: env.portfolio_id(victim),
            position_epoch: env.portfolio_position_epoch(victim),
            optional_deposit: 50,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(attacker_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        rejected.is_err(),
        "non-owner must not cancel a victim close-progress ledger"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market
    );
    assert_eq!(env.svm.get_account(&victim).unwrap().data, before_victim);
    assert_eq!(env.token_amount(attacker_source), before_source);
    assert!(!close_progress(&env.portfolio_state(victim)).canceled);

    let owner_source = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    env.cure_and_cancel_close_with_cu(&owner, victim, owner_source, 0);
    let cured = env.portfolio_state(victim);
    assert!(close_progress(&cured).canceled);
    assert_eq!(cured.capital.get(), 100);
}

#[test]
fn v16_program_cure_cannot_be_replayed_on_canceled_close_ledger() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100);
    env.seed_cancellable_close_progress(portfolio);

    let first_source = env.token_account(owner.pubkey(), 0);
    env.cure_and_cancel_close_with_cu(&owner, portfolio, first_source, 0);
    assert!(close_progress(&env.portfolio_state(portfolio)).canceled);

    let replay_source = env.token_account(owner.pubkey(), 50);
    env.svm.expire_blockhash();
    let replay = env.send(
        ProgInstruction::CureAndCancelClose {
            portfolio_id: env.portfolio_id(portfolio),
            position_epoch: env.portfolio_position_epoch(portfolio),
            optional_deposit: 50,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(replay_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    let err = replay.expect_err("second cure of a canceled ledger must reject");
    assert!(err.contains("Custom(21)"));
    assert_eq!(
        env.token_amount(replay_source),
        50,
        "rejected replay must not pull the second deposit"
    );
}

// INV-075 public-route witness: the active close ledger is reached through real
// trade, auth-mark accrual, asset shutdown, and ForfeitRecoveryLeg. Competing
// public actions must not preempt ownership or rewrite the immutable episode
// identity, and the permissionless expiry continuation must remain available.
#[test]
fn v16_program_public_close_episode_competing_actions_preserve_priority_and_identity() {
    let PublicActiveCloseFixture {
        mut env,
        loss_owner,
        loss,
        live_counterparty_owner: other_owner,
        live_counterparty: other_portfolio,
        ..
    } = public_asset1_bankrupt_close_fixture();
    let ledger_before = close_progress(&env.portfolio_state(loss));
    let episode_key = inv075_close_episode_key(ledger_before);

    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let attacker_source = env.token_account_for_mint(env.mint, attacker.pubkey(), 50);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let loss_before = env.svm.get_account(&loss).unwrap();
    let source_before = env.svm.get_account(&attacker_source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let rejected_cure = env.send(
        ProgInstruction::CureAndCancelClose {
            portfolio_id: env.portfolio_id(loss),
            position_epoch: env.portfolio_position_epoch(loss),
            optional_deposit: 50,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
            AccountMeta::new(attacker_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        rejected_cure.is_err(),
        "non-owner must not preempt a public close episode"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&loss).unwrap(), loss_before);
    assert_eq!(
        env.svm.get_account(&attacker_source).unwrap(),
        source_before
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        inv075_close_episode_key(close_progress(&env.portfolio_state(loss))),
        episode_key
    );

    let owner_source = env.token_account_for_mint(env.mint, loss_owner.pubkey(), 25);
    let market_before_deposit = env.svm.get_account(&env.market).unwrap();
    let loss_before_deposit = env.svm.get_account(&loss).unwrap();
    let source_before_deposit = env.svm.get_account(&owner_source).unwrap();
    let vault_before_deposit = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let deposit = env.send(
        env.deposit_ix(loss, 25),
        vec![
            AccountMeta::new(loss_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
            AccountMeta::new(owner_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&loss_owner],
    );
    if deposit.is_err() {
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before_deposit
        );
        assert_eq!(env.svm.get_account(&loss).unwrap(), loss_before_deposit);
        assert_eq!(
            env.svm.get_account(&owner_source).unwrap(),
            source_before_deposit
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before_deposit
        );
    } else {
        assert_eq!(env.token_amount(owner_source), 0);
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap().data.len(),
            vault_before_deposit.data.len()
        );
    }
    let ledger_after_deposit = close_progress(&env.portfolio_state(loss));
    assert!(ledger_after_deposit.active);
    assert_eq!(inv075_close_episode_key(ledger_after_deposit), episode_key);

    let market_before_trade = env.svm.get_account(&env.market).unwrap();
    let loss_before_trade = env.svm.get_account(&loss).unwrap();
    let other_before_trade = env.svm.get_account(&other_portfolio).unwrap();
    let vault_before_trade = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let rejected_trade = env.try_trade_asset_with_cu(
        0,
        &loss_owner,
        loss,
        &other_owner,
        other_portfolio,
        (POS_SCALE / 10) as i128,
        100,
        0,
    );
    assert!(
        rejected_trade.is_err(),
        "active close account must not trade on an unrelated live asset"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_trade
    );
    assert_eq!(env.svm.get_account(&loss).unwrap(), loss_before_trade);
    assert_eq!(
        env.svm.get_account(&other_portfolio).unwrap(),
        other_before_trade
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before_trade);
    assert_eq!(
        inv075_close_episode_key(close_progress(&env.portfolio_state(loss))),
        episode_key
    );

    env.svm.warp_to_slot(ledger_before.max_close_slot + 1);
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("expired public close episode has a permissionless continuation");
    assert_cu_within(
        "INV-075 expired public close continuation",
        cu,
        CRANK_CU_LIMIT,
    );
    assert!(
        matches!(
            env.market_state().1.mode,
            MarketModeV16::Recovery | MarketModeV16::Resolved
        ),
        "expired close continuation must enter a terminal progress mode"
    );
}

// The pinned engine does not expose the charter's ClosePriority tuple. It
// serializes close starts with a first-landed per-domain barrier instead. This
// bounded public model exhausts both 2! landing orders for equal close
// contenders and pins that actual mechanism: the first start owns the barrier,
// the second rejects with an exact frame, the accepted episode retains its
// immutable identity, and both contenders can be terminally cleared after the
// configured delays without a signature from the first owner.
#[test]
fn v16_program_competing_close_starts_exhaust_both_landing_orders() {
    let mut outcomes = Vec::new();
    for first in [0usize, 1usize] {
        let second = 1 - first;
        let Inv075CompetingCloseFixture {
            mut env,
            base_owners,
            base_portfolios,
            winner_owners,
            winners,
            loss_owners,
            losses,
        } = inv075_competing_public_closes_fixture();

        inv075_try_forfeit(&mut env, &loss_owners[first], losses[first])
            .expect("first landed close takes the domain barrier");
        let accepted_before = close_progress(&env.portfolio_state(losses[first]));
        assert!(accepted_before.active && accepted_before.residual_remaining > 0);
        let accepted_episode = inv075_close_episode_key(accepted_before);

        let market_before = env.svm.get_account(&env.market).unwrap();
        let first_before = env.svm.get_account(&losses[first]).unwrap();
        let second_before = env.svm.get_account(&losses[second]).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        let rejected = inv075_try_forfeit(&mut env, &loss_owners[second], losses[second])
            .expect_err("second close in the occupied domain must reject");
        assert!(
            rejected.contains("Custom(21)"),
            "occupied-domain contender must reject LockActive: {rejected}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&losses[first]).unwrap(), first_before);
        assert_eq!(env.svm.get_account(&losses[second]).unwrap(), second_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
        assert_eq!(
            inv075_close_episode_key(close_progress(&env.portfolio_state(losses[first]))),
            accepted_episode
        );
        assert!(!close_progress(&env.portfolio_state(losses[second])).active);
        assert!(has_active_leg_for_asset(
            &env.portfolio_state(losses[second]),
            1
        ));

        env.svm.warp_to_slot(accepted_before.max_close_slot + 1);
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(losses[first], false),
            ],
            &[],
        )
        .expect("accepted close retains a permissionless expiry continuation");
        assert!(matches!(
            env.market_state().1.mode,
            MarketModeV16::Recovery | MarketModeV16::Resolved
        ));
        env.svm.warp_to_slot(accepted_before.max_close_slot + 10);
        for _ in 0..16 {
            let close = close_progress(&env.portfolio_state(losses[first]));
            if close.finalized && close.residual_remaining == 0 {
                break;
            }
            let (cfg, group) = env.market_state();
            if group.mode == MarketModeV16::Resolved {
                env.svm.warp_to_slot(
                    group
                        .resolved_slot
                        .saturating_add(cfg.force_close_delay_slots)
                        .saturating_add(1),
                );
            }
            env.svm.expire_blockhash();
            env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new_readonly(loss_owners[first].pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(losses[first], false),
                ],
                &[],
            )
            .expect("accepted close retains a permissionless terminal continuation");
        }
        let accepted_after = close_progress(&env.portfolio_state(losses[first]));
        assert!(
            accepted_after.finalized && accepted_after.residual_remaining == 0,
            "permissionless terminal continuation must finalize the accepted close: {accepted_after:?}"
        );
        if env.market_state().1.mode == MarketModeV16::Recovery {
            env.resolve_stale_permissionless_with_cu(200);
        }
        assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);

        let spl_vault_before = env.token_amount(env.vault) as u128;
        let actors = [
            (&base_owners[0], base_portfolios[0]),
            (&base_owners[1], base_portfolios[1]),
            (&winner_owners[0], winners[0]),
            (&winner_owners[1], winners[1]),
            (&loss_owners[0], losses[0]),
            (&loss_owners[1], losses[1]),
        ];
        let payouts = inv075_settle_resolved_to_fixed_point(&mut env, &actors);
        let receipts = std::array::from_fn(|index| {
            let account = env.portfolio_state(actors[index].1);
            let receipt = resolved_receipt(&account);
            assert_eq!(account.capital.get(), 0);
            assert_eq!(account.pnl.get(), 0);
            assert_eq!(account.reserved_pnl.get(), 0);
            assert_eq!(account.fee_credits.get(), 0);
            assert_eq!(account.cancel_deposit_escrow.get(), 0);
            assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
            assert!(account
                .source_domains
                .iter()
                .all(|source| !source.is_occupied()));
            assert!(
                resolved_portfolio_is_terminal(&env, actors[index].1) || receipt.present,
                "a fixed-point account must be empty or retain an explicit payout receipt"
            );
            (
                receipt.present,
                receipt.prior_bound_contribution_num,
                receipt.live_released_face_at_receipt,
                receipt.terminal_positive_claim_face,
                receipt.paid_effective,
                receipt.finalized,
            )
        });
        assert_eq!(
            payouts.iter().sum::<u128>() + u128::from(env.token_amount(env.vault)),
            spl_vault_before,
            "resolved settlement must account for every pre-settlement SPL atom"
        );
        let (_, terminal) = env.market_state();
        assert_eq!(terminal.assets[0].oi_eff_long_q, 0);
        assert_eq!(terminal.assets[0].oi_eff_short_q, 0);
        assert_eq!(terminal.assets[1].oi_eff_long_q, 0);
        assert_eq!(terminal.assets[1].oi_eff_short_q, 0);
        assert_eq!(terminal.negative_pnl_account_count, 0);
        assert_eq!(terminal.vault, u128::from(env.token_amount(env.vault)));
        outcomes.push(Inv075CloseOrderTerminalOutcome {
            payouts,
            receipts,
            engine_vault: terminal.vault,
            spl_vault: env.token_amount(env.vault),
            insurance: terminal.insurance,
            aggregate_capital: terminal.c_tot,
            oi: [
                (
                    terminal.assets[0].oi_eff_long_q,
                    terminal.assets[0].oi_eff_short_q,
                ),
                (
                    terminal.assets[1].oi_eff_long_q,
                    terminal.assets[1].oi_eff_short_q,
                ),
            ],
            negative_pnl_accounts: terminal.negative_pnl_account_count,
        });
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "same-domain close-start landing order changed terminal user value or market custody"
    );
}

// security.md sweep — withdraw blocked during active close (#22/#48): an account with an active/in-
// progress forced close must NOT be able to withdraw (withdraw_not_atomic rejects a non-inert close
// ledger). Prevents withdrawing funds out from under a forced close.
#[test]
fn v16_attack_withdraw_blocked_during_active_close() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    // seed an ACTIVE (cancellable) forced-close ledger.
    env.seed_cancellable_close_progress(p);
    // withdraw must reject while the close is active.
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let r = env.send(
        env.withdraw_ix(p, 500_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw during an active forced-close must reject"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no funds withdrawn during active close"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "capital intact"
    );
    // after curing+cancelling the close (Finding E), withdraw works again.
    let src = env.token_account(owner.pubkey(), 0);
    env.cure_and_cancel_close_with_cu(&owner, p, src, 0);
    let (d, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(
        env.token_amount(d),
        500_000,
        "withdraw works after curing the close"
    );
}

// security.md sweep — trade blocked during active close (#22): an account with an active forced-close
// must not be able to open/modify positions (it's being wound down). Trading on it must reject.
#[test]
fn v16_attack_trade_blocked_during_active_close() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.seed_cancellable_close_progress(pa); // la has an active forced-close
    let (_, g0) = env.market_state();
    // trading on la (with an active close) must reject.
    env.svm.expire_blockhash();
    let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    assert!(
        r.is_err(),
        "trade on an account with an active forced-close must reject"
    );
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        0,
        "no position opened during active close"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI created");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
}

// security.md sweep — deposit during active close (#22): a plain deposit to an account with an active
// forced-close must be handled safely — whether allowed (adds capital toward curing) or rejected, it
// must conserve and never corrupt the close ledger or accounting.
#[test]
fn v16_attack_deposit_during_active_close_safe() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.seed_cancellable_close_progress(p);
    let cap0 = env.portfolio_state(p).capital.get();
    let (_, g0) = env.market_state();
    // attempt a plain deposit during the active close.
    let src = env.token_account_for_mint(env.mint, owner.pubkey(), 500);
    env.svm.expire_blockhash();
    let r = env.send(
        env.deposit_ix(p, 500),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    let cap1 = env.portfolio_state(p).capital.get();
    let (_, g1) = env.market_state();
    // either outcome must conserve: capital change == vault change == source debit, accounting intact.
    if r.is_ok() {
        assert_eq!(cap1, cap0 + 500, "deposit credited exactly during close");
        assert_eq!(
            g1.vault,
            g0.vault + 500,
            "vault grew by exactly the deposit"
        );
        assert_eq!(env.token_amount(src), 0, "source fully transferred");
    } else {
        assert_eq!(cap1, cap0, "rejected deposit: capital unchanged");
        assert_eq!(g1.vault, g0.vault, "rejected deposit: vault unchanged");
        assert_eq!(
            env.token_amount(src),
            500,
            "rejected deposit: source untouched"
        );
    }
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// Coverage probe (audit, Finding candidate): after a user defensively cures and
// cancels a forced close (CureAndCancelClose), their `close_progress` ledger is
// left in the `canceled` state, never reset to EMPTY. `withdraw_not_atomic`
// requires `close_progress == EMPTY`, so the user can never withdraw their flat,
// solvent capital again in Live mode. This test asserts the CORRECT outcome (the
// user can withdraw after curing).
// GREEN regression: Finding E was fixed in engine f9af174 (withdraw now allows an
// inert `canceled` close ledger).
#[test]
fn v16_audit_withdraw_after_cure_and_cancel_close() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100);
    env.seed_cancellable_close_progress(portfolio);

    // Owner cures + cancels the forced close (no position -> IM is 0, so no extra
    // deposit needed).
    let source = env.token_account(owner.pubkey(), 0);
    env.cure_and_cancel_close_with_cu(&owner, portfolio, source, 0);

    // The account is now flat and solvent (capital 100, no positions). The user
    // must be able to withdraw their own capital.
    env.withdraw_with_cu(&owner, portfolio, 100);
    let account = state::read_portfolio(&env.svm.get_account(&portfolio).unwrap().data).unwrap();
    assert_eq!(
        account.capital.get(), 0,
        "a flat, solvent user must be able to withdraw their capital after curing a cancelled close",
    );
}
