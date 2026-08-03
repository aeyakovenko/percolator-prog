//! INV-073 - No permanent user lock.
//!
//! Normative obligation: Every publicly reachable funded state has a finite public path to capital or terminal disposition.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_source_backed_force_close_preserves_bounded_resolved_exits`, `v16_probe_liquidation_then_shutdown_preserves_bounded_owner_exit`, `v16_attack_permissionless_close_resolved_survives_drained_owner_system_account`, `v16_attack_permissionless_asset_epoch_grief_has_atomic_max_leg_exit`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_source_backed_force_close_preserves_bounded_resolved_exits() {
    const PRICE: u64 = 100;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = POS_SCALE as i128;
    const TOO_LARGE_INCREASE_Q: i128 = 30 * POS_SCALE as i128;
    const SHUTDOWN_SLOT: u64 = 4;
    const FORCE_CLOSE_DELAY: u64 = 5;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, PRICE);

    let winner_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&winner_owner, winner, 313);
    env.deposit(&counterparty_owner, counterparty, 1_000);
    env.top_up_backing_bucket(1, 150, 10);
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &counterparty_owner,
        counterparty,
        ASSET0_SIZE_Q,
        PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &winner_owner,
        winner,
        &counterparty_owner,
        counterparty,
        ASSET1_SIZE_Q,
        PRICE,
        0,
    );

    // Create source-backed positive PnL on asset 0, then use it as initial
    // margin for a public risk increase on the losing asset-1 leg.
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for (portfolio, asset_index) in [(counterparty, 0), (winner, 0), (counterparty, 1)] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(asset_index),
            },
        );
    }

    let watermark_dest = env.token_account(env.admin.pubkey(), 0);
    env.withdraw_backing_bucket_to_admin_token_with_cu(watermark_dest, 1, 50);
    assert!(
        env.try_trade_asset_with_cu(
            1,
            &winner_owner,
            winner,
            &counterparty_owner,
            counterparty,
            TOO_LARGE_INCREASE_Q,
            95,
            0,
        )
        .is_err(),
        "source-credit watermark control must reject the oversized increase"
    );
    env.svm.warp_to_slot(3);
    env.trade_asset_with_cu(
        1,
        &winner_owner,
        winner,
        &counterparty_owner,
        counterparty,
        SAFE_INCREASE_Q,
        95,
        0,
    );

    let winner_before_shutdown = env.portfolio_state(winner);
    let lien_before = winner_before_shutdown
        .source_domains
        .iter()
        .find(|source| source.source_lien_counterparty_backing_num.get() != 0);
    assert!(
        lien_before.is_some(),
        "winner must hold a counterparty-backed source lien; pnl={} capital={}",
        winner_before_shutdown.pnl.get(),
        winner_before_shutdown.capital.get(),
    );
    let lien_before = lien_before.unwrap();
    assert!(winner_before_shutdown.pnl.get() > 0);
    assert!(lien_before.source_claim_liened_num.get() > 0);
    assert!(lien_before.source_lien_counterparty_backing_num.get() > 0);
    assert_eq!(
        env.market_state().1.source_backing_buckets[1].status,
        BackingBucketStatusV16::Fresh
    );
    env.configure_permissionless_resolve_with_cu(100, FORCE_CLOSE_DELAY);

    // A post-init asset admin may shut down the market, but that action must
    // preserve bounded owner/keeper exits for users already holding claims.
    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        0,
        SHUTDOWN_SLOT,
        0,
    );
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        SHUTDOWN_SLOT,
        0,
    );
    let (_, recovery) = env.market_state();
    assert_eq!(recovery.assets[0].lifecycle, AssetLifecycleV16::Recovery);
    assert_eq!(recovery.assets[1].lifecycle, AssetLifecycleV16::Recovery);
    assert_eq!(
        recovery.source_backing_buckets[1].status,
        BackingBucketStatusV16::Fresh
    );

    let cranker = Keypair::new();
    let force_slot = SHUTDOWN_SLOT + FORCE_CLOSE_DELAY + 1;
    env.svm.warp_to_slot(force_slot);
    env.force_close_abandoned_asset_with_cu(
        &cranker,
        winner,
        counterparty,
        0,
        force_slot,
        ASSET0_SIZE_Q.unsigned_abs(),
    );
    env.force_close_abandoned_asset_with_cu(
        &cranker,
        winner,
        counterparty,
        1,
        force_slot,
        (ASSET1_SIZE_Q + SAFE_INCREASE_Q).unsigned_abs(),
    );
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(winner)
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(counterparty)
    )));
    let winner_after_force = env.portfolio_state(winner);
    let forced_lien = winner_after_force
        .source_domains
        .iter()
        .find(|source| source.source_lien_counterparty_backing_num.get() != 0)
        .expect("force-close must preserve the winner's account-local lien until settlement");
    let (_, forced_market) = env.market_state();
    let forced_domain = forced_lien.domain.get() as usize;
    assert_eq!(
        forced_market.source_backing_buckets[forced_domain].valid_liened_backing_num,
        forced_lien.source_lien_counterparty_backing_num.get(),
        "the public force-close setup must retain a real aggregate-backed source lien",
    );

    env.resolve();
    let winner_dest = env.token_account(winner_owner.pubkey(), 0);
    let counterparty_dest = env.token_account(counterparty_owner.pubkey(), 0);
    let winner_tokens_before = env.token_amount(winner_dest);
    let try_close = |env: &mut V16CuEnv, owner: &Keypair, portfolio: Pubkey, dest: Pubkey| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    };

    let is_terminal = |state: &PortfolioAccountV16| {
        state.capital.get() == 0
            && state.pnl.get() == 0
            && percolator::active_bitmap_is_empty(active_bitmap(state))
            && state
                .source_domains
                .iter()
                .all(|source| source.source_claim_bound_num.get() == 0)
            && state.stale_state == 0
            && state.b_stale_state == 0
    };
    let mut max_close_cu = 0;
    let mut close_rounds = 0;
    for round in 0..8 {
        let counterparty_cu = try_close(
            &mut env,
            &counterparty_owner,
            counterparty,
            counterparty_dest,
        )
        .expect("counterparty resolved close must make bounded progress");
        let winner_cu = try_close(&mut env, &winner_owner, winner, winner_dest)
            .expect("source-backed winner resolved close must make bounded progress");
        max_close_cu = max_close_cu.max(counterparty_cu).max(winner_cu);
        close_rounds = round + 1;
        if is_terminal(&env.portfolio_state(winner))
            && is_terminal(&env.portfolio_state(counterparty))
        {
            break;
        }
    }

    let winner_after = env.portfolio_state(winner);
    let counterparty_after = env.portfolio_state(counterparty);
    let winner_tokens_after = env.token_amount(winner_dest);
    assert!(
        is_terminal(&winner_after),
        "source-backed shutdown must not strand the independent winner; \
         capital={} pnl={} active={:?} rounds={close_rounds}",
        winner_after.capital.get(),
        winner_after.pnl.get(),
        active_bitmap(&winner_after),
    );
    assert!(
        is_terminal(&counterparty_after),
        "source-backed shutdown must not strand the counterparty; \
         capital={} pnl={} active={:?} rounds={close_rounds}",
        counterparty_after.capital.get(),
        counterparty_after.pnl.get(),
        active_bitmap(&counterparty_after),
    );
    assert!(
        winner_tokens_after > winner_tokens_before,
        "resolved close must transfer the winner's withdrawable value"
    );
    assert_cu_within(
        "source-backed force-close resolved wind-down",
        max_close_cu,
        CUSTODY_CU_LIMIT,
    );
}

#[test]
fn v16_probe_liquidation_then_shutdown_preserves_bounded_owner_exit() {
    let mut env = V16CuEnv::new();
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.configure_permissionless_resolve_with_cu(100, 2);

    let winner = Keypair::new();
    let winner_portfolio = env.create_portfolio(&winner);
    let loser = Keypair::new();
    let loser_portfolio = env.create_portfolio(&loser);
    let idle = Keypair::new();
    let idle_portfolio = env.create_portfolio(&idle);
    env.deposit(&winner, winner_portfolio, 5_000_000);
    env.deposit(&loser, loser_portfolio, 250);
    env.deposit(&idle, idle_portfolio, 5_000_000);
    env.trade_with_cu(
        &winner,
        winner_portfolio,
        &loser,
        loser_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );

    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loser_portfolio, false),
            ],
            &[],
        );
    }
    env.crank(
        loser_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );

    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        0,
        3,
        0,
    );

    for (owner, portfolio) in [
        (&winner, winner_portfolio),
        (&loser, loser_portfolio),
        (&idle, idle_portfolio),
    ] {
        if has_active_leg_for_asset(&env.portfolio_state(portfolio), 0) {
            env.svm.expire_blockhash();
            env.send(
                ProgInstruction::ForfeitRecoveryLeg {
                    asset_index: 0,
                    b_delta_budget: u128::MAX,
                },
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[owner],
            )
            .expect("every Recovery survivor has a bounded owner forfeit path");
        }
    }

    for portfolio in [winner_portfolio, loser_portfolio, idle_portfolio] {
        assert!(
            percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(portfolio))),
            "shutdown exit must clear every remaining leg"
        );
    }

    env.resolve();
    env.svm.warp_to_slot(6);
    let mut total_payout = 0u128;
    for (owner, portfolio) in [
        (&winner, winner_portfolio),
        (&loser, loser_portfolio),
        (&idle, idle_portfolio),
    ] {
        for _ in 0..8 {
            let state = env.portfolio_state(portfolio);
            let receipt = resolved_receipt(&state);
            if state.capital.get() == 0
                && state.pnl.get() == 0
                && state.reserved_pnl.get() == 0
                && (!receipt.present || receipt.finalized)
            {
                break;
            }
            let (dest, _) = env.close_resolved_with_cu(owner, portfolio);
            total_payout += env.token_amount(dest) as u128;
        }
        let state = env.portfolio_state(portfolio);
        let receipt = resolved_receipt(&state);
        assert_eq!(state.capital.get(), 0, "resolved capital must be payable");
        assert_eq!(state.pnl.get(), 0, "resolved PnL must settle");
        assert_eq!(state.reserved_pnl.get(), 0, "no payout may remain reserved");
        assert!(
            !receipt.present || receipt.finalized,
            "resolved payout receipt must finish in bounded calls"
        );
    }
    let (_, group) = env.market_state();
    assert_eq!(
        total_payout + group.vault,
        10_000_250,
        "all deposited collateral must remain in payouts or the engine vault"
    );
    assert_eq!(
        group.vault as u64,
        env.token_amount(env.vault),
        "engine and SPL custody must remain synchronized"
    );
}

#[test]
fn v16_attack_permissionless_close_resolved_survives_drained_owner_system_account() {
    let mut env = V16CuEnv::new();
    const EXIT_DELAY: u64 = 5;
    env.configure_permissionless_resolve_with_cu(100, EXIT_DELAY);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    let owner_dest = env.token_account(owner.pubkey(), 0);
    env.resolve();
    env.svm.warp_to_slot(EXIT_DELAY + 1);

    let owner_lamports = env.svm.get_account(&owner.pubkey()).unwrap().lamports;
    env.svm.expire_blockhash();
    send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![system_instruction::transfer(
            &owner.pubkey(),
            &env.payer.pubkey(),
            owner_lamports,
        )],
        &[&owner],
    )
    .expect("owner can publicly drain its system-account lamports");
    assert_eq!(
        env.svm
            .get_account(&owner.pubkey())
            .map(|account| account.lamports)
            .unwrap_or(0),
        0,
        "probe starts after the owner system account is no longer funded"
    );

    env.svm.expire_blockhash();
    let permissionless = env
        .send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(owner_dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("post-timeout CloseResolved should not depend on owner lamports");
    assert_cu_within(
        "post-timeout CloseResolved drained owner account",
        permissionless,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(owner_dest),
        1_000,
        "permissionless close still pays the portfolio owner's token account"
    );
    assert_eq!(env.token_amount(env.vault), 0);
}

#[test]
fn v16_attack_permissionless_asset_epoch_grief_has_atomic_max_leg_exit() {
    const LEGS: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const ATTACK_ASSET: u16 = LEGS;
    const PRICE: u64 = 100;

    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: LEGS,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
        LEGS as usize + 1,
    );
    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(1);
    for asset_index in 0..LEGS {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, PRICE);
    }

    let attacker = Keypair::new();
    env.svm.warp_to_slot(2);
    env.activate_permissionless_asset_with_fee(
        &attacker,
        ATTACK_ASSET,
        2,
        PRICE,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(ATTACK_ASSET, &attacker, 2, PRICE);

    let attack_long_owner = Keypair::new();
    let attack_short_owner = Keypair::new();
    let attack_long = env.create_portfolio(&attack_long_owner);
    let attack_short = env.create_portfolio(&attack_short_owner);
    env.deposit(&attack_long_owner, attack_long, 10_000);
    env.deposit(&attack_short_owner, attack_short, 10_000);
    env.trade_asset_with_cu(
        ATTACK_ASSET,
        &attack_long_owner,
        attack_long,
        &attack_short_owner,
        attack_short,
        POS_SCALE as i128,
        PRICE,
        0,
    );

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000_000);
    env.deposit(&short_owner, short, 10_000_000);
    let open_legs: Vec<BatchTradeLeg> = (0..LEGS)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            size_q: POS_SCALE as i128,
            exec_price: PRICE,
            fee_bps: 0,
        })
        .collect();
    env.send(
        ProgInstruction::BatchTradeNoCpi { legs: open_legs },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(short_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
            AccountMeta::new(short, false),
        ],
        &[&long_owner, &short_owner],
    )
    .expect("public 14-leg setup trade");
    let cert_epoch = health_cert(&env.portfolio_state(long)).cert_oracle_epoch;
    assert_eq!(cert_epoch, env.market_state().1.oracle_epoch);

    env.svm.warp_to_slot(3);
    env.send(
        ProgInstruction::PushAuthMark {
            asset_index: ATTACK_ASSET,
            now_slot: 3,
            mark_e6: 200,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&attacker],
    )
    .expect("attacker updates its permissionless mark");
    env.crank(
        attack_long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(ATTACK_ASSET),
        },
    );
    assert!(env.market_state().1.oracle_epoch > cert_epoch);
    let custody_before_exit = env.token_amount(env.vault);

    env.svm.expire_blockhash();
    let stale_exit = env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        PRICE,
        0,
    );
    let stale_err = stale_exit.expect_err("unrelated target update invalidates max-leg exit certs");
    assert!(
        stale_err.contains("Custom(19)") || stale_err.contains("custom program error: 0x13"),
        "expected EngineStale, got {stale_err}"
    );

    let program_id = env.program_id;
    let market = env.market;
    let payer = env.payer.pubkey();
    let crank_ix = |portfolio| Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(market, false),
            AccountMeta::new(portfolio, false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: vec![],
        }
        .encode(),
    };
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (matcher_context, matcher_delegate, _) =
        env.init_matcher_context_authorized(matcher_program, &short_owner, short);
    let exit_ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
            AccountMeta::new(short, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(matcher_context, false),
            AccountMeta::new_readonly(matcher_delegate, false),
        ],
        data: ProgInstruction::TradeCpi {
            asset_index: 0,
            size_q: -(POS_SCALE as i128),
            fee_bps: 0,
            limit_price: PRICE,
        }
        .encode(),
    };
    env.svm.expire_blockhash();
    let atomic_cu = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), crank_ix(long), crank_ix(short), exit_ix],
        &[&long_owner],
    )
    .expect("atomic recovery path must fit one transaction");
    assert!(
        atomic_cu < 1_400_000,
        "max-leg atomic refresh+exit consumed {atomic_cu} CU"
    );

    let after = env.market_state().1;
    assert_eq!(after.assets[0].oi_eff_long_q, 0);
    assert_eq!(after.assets[0].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
    assert_eq!(env.token_amount(env.vault), custody_before_exit);
    assert_eq!(after.vault as u64, custody_before_exit);
}
