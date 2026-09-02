//! INV-037 - Exact residual partition.
//!
//! A bankruptcy or close residual must be partitioned exactly once across the
//! close ledger categories. This test reaches a real public liquidation path
//! where insurance covers the bankrupt short, then checks the deployed
//! `CloseProgressLedger` equation before allowing the emptied portfolio to
//! close. The same independent oracle is mutation-tested by the stateful
//! owner and applied before and after continuation in INV-076's four public
//! route/drift worlds:
//!
//! ```text
//! gross_loss_at_close_start + drift_consumed =
//!   support_consumed
//! + insurance_spent
//! + b_loss_booked
//! + explicit_loss_assigned
//! + residual_remaining
//! ```
//!
//! `junior_face_burned` records retired claim face and is intentionally not a
//! second payment term; only the realizable `support_consumed` atoms cover
//! loss. The stateful INV-037 owner makes this distinction public and
//! nonvacuous on all four trade routes: 1,000 retired face atoms coexist with
//! exactly 251 realizable support atoms. Pending-obligation legs and weights are
//! governed separately by INV-039 and cannot directly credit this ledger. The
//! exhaustive engine struct literal in the stateful owner source-locks the
//! deployed category set, while the pinned engine contract proves its equation.

use super::*;
use crate::support::fuzz_model::verify_close_residual_partition;

#[test]
fn v16_program_insurance_covered_liquidation_close_ledger_partitions_exactly() {
    const SHORT_CAP: u128 = 55_000;
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(0);
    env.top_up_insurance(1_000_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let long_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short_owner = Keypair::new();
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&short_owner, short, SHORT_CAP);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        1_000_000,
        0,
    );

    for slot in 1..=40u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 1_070_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        );
    }

    let short_after = env.portfolio_state(short);
    let ledger = close_progress(&short_after);
    assert_eq!(short_after.capital.get(), 0);
    assert_eq!(short_after.pnl.get(), 0);
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&short_after)),
        "full liquidation leaves the loser flat",
    );
    assert!(
        ledger.finalized && ledger.residual_remaining == 0,
        "setup must exercise a finalized insurance-covered close ledger",
    );
    assert!(
        ledger.gross_loss_at_close_start > 0,
        "ledger must record a nonzero bankrupt close loss",
    );
    assert!(
        ledger.insurance_spent > 0,
        "insurance must be one non-vacuous partition component",
    );
    verify_close_residual_partition("insurance-covered liquidation", &ledger)
        .expect("close ledger partition must account for every loss atom exactly once");

    let close_cu = env.close_portfolio_with_cu(&short_owner, short);
    assert_cu_within(
        "ClosePortfolio after insurance-covered liquidation",
        close_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        1,
        "only the long counterparty remains materialized",
    );
}

#[test]
fn v16_bpf_account_residual_reward_counter_covers_all_trade_paths() {
    for path in [
        AccountResidualCounterTradePath::TradeNoCpi,
        AccountResidualCounterTradePath::TradeCpi,
        AccountResidualCounterTradePath::BatchTradeNoCpi,
        AccountResidualCounterTradePath::BatchTradeCpi,
    ] {
        run_account_residual_counter_credit_case(path, POS_SCALE as i128, 10_000, 50);
        run_account_residual_counter_credit_case(path, -(POS_SCALE as i128), 10_000, 50);
    }
}

#[test]
fn v16_bpf_account_residual_reward_counter_caps_available_crystallized_loss() {
    for path in [
        AccountResidualCounterTradePath::TradeNoCpi,
        AccountResidualCounterTradePath::TradeCpi,
        AccountResidualCounterTradePath::BatchTradeNoCpi,
        AccountResidualCounterTradePath::BatchTradeCpi,
    ] {
        run_account_residual_counter_credit_case(path, POS_SCALE as i128, 30, 30);
    }
}

#[test]
fn v16_bpf_account_residual_reward_counter_accumulates_across_batch_legs() {
    const PRICE: u64 = 1_000;
    for cpi in [false, true] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 500, 500, 24);
        env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
        env.configure_auth_mark_for_asset_as_admin(1, 1, PRICE);
        let taker_owner = Keypair::new();
        let lp_owner = Keypair::new();
        let taker_account = env.create_portfolio(&taker_owner);
        let lp_account = env.create_portfolio(&lp_owner);
        env.deposit(&taker_owner, taker_account, 10_000);
        env.deposit(&lp_owner, lp_account, 10_000);
        env.set_residual_reward_counters_for_test(taker_account, 10_000, 0, 0);
        env.svm.expire_blockhash();
        let cu = if cpi {
            let (matcher_program, ctx, delegate) =
                auth_matcher_for_lp_via_system_create(&mut env, &lp_owner, lp_account);
            env.send(
                env.batch_trade_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![
                        BatchTradeCpiLeg {
                            asset_index: 0,
                            market_id: first_generation_market_id((0) as u16),
                            size_q: POS_SCALE as i128,
                            fee_bps: 0,
                            limit_price: 0,
                        },
                        BatchTradeCpiLeg {
                            asset_index: 1,
                            market_id: first_generation_market_id((1) as u16),
                            size_q: -(POS_SCALE as i128),
                            fee_bps: 0,
                            limit_price: 0,
                        },
                    ],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                    AccountMeta::new_readonly(matcher_program, false),
                    AccountMeta::new(ctx, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[&taker_owner],
            )
            .expect("BatchTradeCpi residual counters must accumulate across legs")
        } else {
            env.send(
                env.batch_trade_no_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![
                        BatchTradeLeg {
                            asset_index: 0,
                            market_id: first_generation_market_id((0) as u16),
                            size_q: POS_SCALE as i128,
                            exec_price: PRICE,
                            fee_bps: 0,
                        },
                        BatchTradeLeg {
                            asset_index: 1,
                            market_id: first_generation_market_id((1) as u16),
                            size_q: -(POS_SCALE as i128),
                            exec_price: PRICE,
                            fee_bps: 0,
                        },
                    ],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(lp_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                ],
                &[&taker_owner, &lp_owner],
            )
            .expect("BatchTradeNoCpi residual counters must accumulate across legs")
        };
        assert_cu_within(
            "two-leg residual-counter batch",
            cu,
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        );
        let taker_after = env.portfolio_state(taker_account);
        let lp_after = env.portfolio_state(lp_account);
        assert_eq!(
            taker_after.residual_spent_principal_atoms_total.get(),
            100,
            "two 50-atom principal increases spend exactly 100 atoms"
        );
        assert_eq!(
            lp_after.residual_received_atoms_total.get(),
            100,
            "batch LP receives the sum of per-leg real-principal credits"
        );
    }
}

#[test]
fn v16_bpf_backing_residual_reward_counter_covers_all_trade_paths() {
    for path in [
        BackingResidualCounterTradePath::TradeNoCpi,
        BackingResidualCounterTradePath::TradeCpi,
        BackingResidualCounterTradePath::BatchTradeNoCpi,
        BackingResidualCounterTradePath::BatchTradeCpi,
    ] {
        run_backing_residual_counter_trade_path_case(path);
    }
}

#[test]
fn v16_bpf_backing_residual_reward_counter_is_snapshot_deterministic() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10);

    let read_ledger = |env: &V16CuEnv| {
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap()
    };
    let start = read_ledger(&env).residual_received_atoms();
    assert_eq!(start, 0, "farm starts from an explicit zero snapshot");

    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].consumed_liened_backing_num = 40 * BOUND_SCALE;
    });
    env.svm.expire_blockhash();
    env.sync_backing_domain_ledger_with_cu(ledger, 1);
    let first_loss = read_ledger(&env);
    assert_eq!(first_loss.cumulative_loss_atoms, 40);
    assert_eq!(first_loss.residual_received_atoms(), 40);
    assert_eq!(first_loss.residual_recovered_atoms(), 0);
    assert_eq!(first_loss.residual_received_delta_since(start).unwrap(), 40);

    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].consumed_liened_backing_num = 10 * BOUND_SCALE;
    });
    env.svm.expire_blockhash();
    env.sync_backing_domain_ledger_with_cu(ledger, 1);
    let after_recovery = read_ledger(&env);
    assert_eq!(
        after_recovery.residual_received_atoms(),
        40,
        "recovery must not decrement the farm reward counter"
    );
    assert_eq!(after_recovery.residual_recovered_atoms(), 30);
    assert_eq!(
        after_recovery.residual_received_delta_since(start).unwrap(),
        40,
        "same start/end reward delta after recovery remains deterministic"
    );

    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].consumed_liened_backing_num = 60 * BOUND_SCALE;
    });
    env.svm.expire_blockhash();
    env.sync_backing_domain_ledger_with_cu(ledger, 1);
    let second_loss = read_ledger(&env);
    assert_eq!(
        second_loss.residual_received_atoms(),
        90,
        "new realized loss after recovery adds only the new unavailable-principal delta"
    );
    assert_eq!(second_loss.residual_recovered_atoms(), 30);
    assert_eq!(
        second_loss.residual_received_delta_since(start).unwrap(),
        90
    );
    assert_eq!(
        second_loss
            .residual_received_delta_since(first_loss.residual_received_atoms())
            .unwrap(),
        50,
        "later farm windows get exactly their own monotonic delta"
    );
    assert!(
        second_loss.residual_received_delta_since(91).is_err(),
        "snapshots above the current counter are invalid, never underflowed"
    );
}

#[test]
fn v16_bpf_backing_residual_reward_counter_is_domain_isolated_and_sync_gated() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    let ledger_domain_1 = env.backing_domain_ledger_account();
    let ledger_domain_2 = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger_domain_1, 1, 100, 10);
    env.top_up_backing_bucket_with_ledger_with_cu(ledger_domain_2, 2, 100, 10);

    let read_ledger = |env: &V16CuEnv, ledger: Pubkey| {
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap()
    };

    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].consumed_liened_backing_num = 25 * BOUND_SCALE;
        group.source_backing_buckets[2].consumed_liened_backing_num = 70 * BOUND_SCALE;
    });
    assert_eq!(
        read_ledger(&env, ledger_domain_1).residual_received_atoms(),
        0,
        "counter only changes when its ledger is synced"
    );
    assert_eq!(
        read_ledger(&env, ledger_domain_2).residual_received_atoms(),
        0,
        "other domain also remains unchanged before sync"
    );

    env.svm.expire_blockhash();
    env.sync_backing_domain_ledger_with_cu(ledger_domain_1, 1);
    let d1_after = read_ledger(&env, ledger_domain_1);
    let d2_unsynced = read_ledger(&env, ledger_domain_2);
    assert_eq!(d1_after.residual_received_atoms(), 25);
    assert_eq!(
        d2_unsynced.residual_received_atoms(),
        0,
        "syncing domain 1 must not credit domain 2 rewards"
    );

    env.svm.expire_blockhash();
    env.sync_backing_domain_ledger_with_cu(ledger_domain_1, 1);
    assert_eq!(
        read_ledger(&env, ledger_domain_1).residual_received_atoms(),
        25,
        "idempotent re-sync without a new loss delta cannot double count"
    );

    env.svm.expire_blockhash();
    env.sync_backing_domain_ledger_with_cu(ledger_domain_2, 2);
    assert_eq!(
        read_ledger(&env, ledger_domain_2).residual_received_atoms(),
        70,
        "domain 2 receives exactly its own realized-loss delta once synced"
    );

    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].utilization_fee_earnings += 33;
        group.source_backing_buckets[1].consumed_liened_backing_num = 25 * BOUND_SCALE;
        group.vault += 33;
    });
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 233);
    env.svm.expire_blockhash();
    env.sync_backing_domain_ledger_with_cu(ledger_domain_1, 1);
    let d1_after_earnings = read_ledger(&env, ledger_domain_1);
    assert_eq!(
        d1_after_earnings.residual_received_atoms(),
        25,
        "earnings sync cannot inflate residual rewards"
    );
    assert_eq!(
        d1_after_earnings.total_earnings_atoms, 33,
        "control: the same sync did observe non-residual earnings"
    );
}
