//! INV-063 - Backing-expiry normalization.
//!
//! Normative obligation: Expired backing is normalized before every consumer and cannot remain economically fresh.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_program_retained_recovery_expiry_prerequisite_matrix_avoids_provider_capitalization`, `v16_program_post_snapshot_expiry_prerequisite_hits_live_stale_lock`, `v16_probe_post_expiry_trade_cannot_charge_backing_fee`, `v16_attack_lapsed_live_source_backing_expires_bounded_and_owner_can_reduce`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: the post-expiry trade test certifies the minimized PR367 trace on the fixed
//! program and checks exact rollback plus a surviving risk-reducing trade. The stateful INV-063
//! module supplies nonvacuous `expiry-1`/`expiry`/`expiry+1` matrices for all four trade routes and
//! released-PnL conversion; this CU module retains the finding-specific and bounded-crank evidence.
//! The lapsed-Live test is also whole-route SVM/CU evidence for INV-030, INV-071, and INV-073: it
//! proves one bounded expiry step, one bounded certification step, custody preservation, and a
//! subsequent owner reduction.

use super::*;

#[test]
fn v16_program_retained_recovery_expiry_prerequisite_matrix_avoids_provider_capitalization() {
    const PRICE: u64 = 1_000_000;
    const FIRST_MARK: u64 = 952_000;
    const SECOND_MARK: u64 = 932_000;
    const ASSET: u16 = 1;
    const SOURCE_DOMAIN: u16 = ASSET * 2;
    const INITIAL_CAPITAL: u128 = 1_000_000;
    const RETAINED_CAPITAL: u128 = 5_000;
    const BACKING: u128 = 100_000;
    const EXPIRY_SLOT: u64 = 41;

    let mut params = production_risk_params();
    params.max_portfolio_assets = 2;
    params.max_abs_funding_e9_per_slot = 0;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    env.configure_auth_mark_for_asset_as_admin(ASSET, 0, PRICE);

    let admin = env.admin.insecure_clone();
    let provider = Keypair::new();
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&provider),
        ASSET,
        processor::ASSET_AUTH_BACKING_BUCKET,
        provider.pubkey().to_bytes(),
    )
    .expect("install independent backing provider");
    let provider_source =
        env.top_up_backing_bucket_with_authority(&provider, SOURCE_DOMAIN, BACKING, EXPIRY_SLOT);
    assert_eq!(env.token_amount(provider_source), 0);

    let victim_owner = Keypair::new();
    let first_peer_owner = Keypair::new();
    let attacker_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let first_peer = env.create_portfolio(&first_peer_owner);
    let attacker = env.create_portfolio(&attacker_owner);
    for (owner, portfolio) in [
        (&victim_owner, victim),
        (&first_peer_owner, first_peer),
        (&attacker_owner, attacker),
    ] {
        env.deposit(owner, portfolio, INITIAL_CAPITAL);
    }

    env.trade_asset_with_cu(
        ASSET,
        &first_peer_owner,
        first_peer,
        &victim_owner,
        victim,
        POS_SCALE as i128,
        PRICE,
        0,
    );
    env.svm.warp_to_slot(20);
    env.push_auth_mark_for_asset_as_admin(ASSET, 20, FIRST_MARK);
    env.crank(
        victim,
        ProgInstruction::PermissionlessCrank {
            now_slot: 20,
            observations: crank_observations(ASSET),
        },
    );
    env.trade_asset_with_cu(
        ASSET,
        &first_peer_owner,
        first_peer,
        &victim_owner,
        victim,
        -(POS_SCALE as i128),
        FIRST_MARK,
        0,
    );
    let historical_claim =
        state::portfolio_source_domain(&env.portfolio_state(victim), SOURCE_DOMAIN as usize)
            .source_claim_bound_num
            .get();
    assert_eq!(historical_claim, 48_000 * BOUND_SCALE);

    env.withdraw(&victim_owner, victim, INITIAL_CAPITAL - RETAINED_CAPITAL);
    env.trade_asset_with_cu(
        ASSET,
        &attacker_owner,
        attacker,
        &victim_owner,
        victim,
        POS_SCALE as i128,
        FIRST_MARK,
        0,
    );
    env.svm.warp_to_slot(40);
    env.push_auth_mark_for_asset_as_admin(ASSET, 40, SECOND_MARK);
    for portfolio in [victim, attacker] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 40,
                observations: crank_observations(ASSET),
            },
        );
    }
    env.trade_asset_with_cu(
        ASSET,
        &attacker_owner,
        attacker,
        &victim_owner,
        victim,
        (POS_SCALE / 2) as i128,
        SECOND_MARK,
        0,
    );
    let source =
        state::portfolio_source_domain(&env.portfolio_state(victim), SOURCE_DOMAIN as usize);
    assert!(source.source_claim_bound_num.get() > historical_claim);
    assert!(source.source_claim_liened_num.get() > historical_claim);

    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, ASSET, 40, 0);
    env.forfeit_recovery_leg_with_cu(&attacker_owner, attacker, ASSET, u128::MAX);
    let before = env.market_state().1;
    assert_eq!(before.current_slot, 40);
    assert_eq!(
        before.source_backing_buckets[SOURCE_DOMAIN as usize].status,
        BackingBucketStatusV16::Fresh
    );
    assert!(
        before.source_backing_buckets[SOURCE_DOMAIN as usize].valid_liened_backing_num
            >= historical_claim
    );

    let retained_forfeit = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(victim_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(victim, false),
                ],
                data: ProgInstruction::ForfeitRecoveryLeg {
                    portfolio_id: env.portfolio_id(victim),
                    position_epoch: env.portfolio_position_epoch(victim),
                    asset_index: ASSET,
                    b_delta_budget: u128::MAX,
                }
                .encode(),
            },
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, &victim_owner],
        env.svm.latest_blockhash(),
    );
    env.svm.warp_to_slot(EXPIRY_SLOT + 1);
    env.svm
        .send_transaction(retained_forfeit)
        .expect("retained forfeit lands after authenticated backing expiry");
    let after_forfeit = env.portfolio_state(victim);
    assert!(!has_active_leg_for_asset(&after_forfeit, ASSET as usize));
    assert_eq!(after_forfeit.capital.get(), RETAINED_CAPITAL);
    assert_eq!(after_forfeit.pnl.get(), 68_000);
    assert!(
        state::portfolio_source_domain(&after_forfeit, SOURCE_DOMAIN as usize)
            .source_claim_bound_num
            .get()
            >= historical_claim
    );
    assert_eq!(
        env.market_state().1.source_backing_buckets[SOURCE_DOMAIN as usize]
            .consumed_liened_backing_num
            / BOUND_SCALE,
        0
    );
}

#[test]
fn v16_program_post_snapshot_expiry_prerequisite_hits_live_stale_lock() {
    const CAPITAL: u128 = 1_000;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_for_asset_as_admin(0, 0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 0, 100);

    let victim_owner = Keypair::new();
    let victim_loser_owner = Keypair::new();
    let attacker_owner = Keypair::new();
    let attacker_loser_owner = Keypair::new();
    let keeper_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let victim_loser = env.create_portfolio(&victim_loser_owner);
    let attacker = env.create_portfolio(&attacker_owner);
    let attacker_loser = env.create_portfolio(&attacker_loser_owner);
    let keeper = env.create_portfolio(&keeper_owner);
    for (owner, portfolio) in [
        (&victim_owner, victim),
        (&victim_loser_owner, victim_loser),
        (&attacker_owner, attacker),
        (&attacker_loser_owner, attacker_loser),
    ] {
        env.deposit(owner, portfolio, CAPITAL);
    }
    env.trade_asset_with_cu(
        0,
        &victim_owner,
        victim,
        &victim_loser_owner,
        victim_loser,
        POS_SCALE as i128,
        100,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &attacker_owner,
        attacker,
        &attacker_loser_owner,
        attacker_loser,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 140);
    env.push_auth_mark_for_asset_as_admin(1, 2, 140);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 1,
                    oracle_accounts: 0,
                },
            ],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(keeper, false),
        ],
        &[],
    )
    .expect("public keeper publishes both marks");
    for portfolio in [victim_loser, victim] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
    }
    for portfolio in [attacker_loser, attacker] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(1),
            },
        );
    }
    env.trade_asset_with_cu(
        0,
        &victim_owner,
        victim,
        &victim_loser_owner,
        victim_loser,
        -(POS_SCALE as i128),
        140,
        0,
    );
    assert!(env.portfolio_state(victim).pnl.get() > 0);
    assert!(env.portfolio_state(attacker).pnl.get() > 0);

    let flat = env.market_state().1;
    let lapse_slot = flat.source_backing_buckets[3]
        .expiry_slot
        .max(flat.source_backing_buckets[1].expiry_slot)
        .checked_add(1)
        .unwrap();
    env.svm.warp_to_slot(lapse_slot);
    env.push_auth_mark_for_asset_as_admin(1, lapse_slot, 130);
    env.crank(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: lapse_slot,
            observations: crank_observations(1),
        },
    );
    assert_eq!(
        env.market_state().1.source_backing_buckets[3].status,
        BackingBucketStatusV16::Fresh
    );
    assert!(env.market_state().1.source_backing_buckets[3].expiry_slot < lapse_slot);

    let close_slot = lapse_slot + 1;
    env.svm.warp_to_slot(close_slot);
    env.push_auth_mark_for_asset_as_admin(1, close_slot, 140);
    env.crank(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: close_slot,
            observations: crank_observations(1),
        },
    );
    let market_before = env.svm.get_account(&env.market).unwrap();
    let attacker_before = env.svm.get_account(&attacker).unwrap();
    let loser_before = env.svm.get_account(&attacker_loser).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let close = env.try_trade_asset_with_cu(
        1,
        &attacker_owner,
        attacker,
        &attacker_loser_owner,
        attacker_loser,
        -(POS_SCALE as i128),
        140,
        0,
    );
    assert!(
        close.is_err(),
        "the PR204 prerequisite unexpectedly progressed"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&attacker).unwrap(), attacker_before);
    assert_eq!(env.svm.get_account(&attacker_loser).unwrap(), loser_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert!(has_active_leg_for_asset(&env.portfolio_state(attacker), 1));
}

#[test]
fn v16_probe_post_expiry_trade_cannot_charge_backing_fee() {
    const PRICE: u64 = 100;
    const WINNING_MARK: u64 = 105;
    const OPEN_Q: i128 = 1_000 * POS_SCALE as i128;
    const INCREASE_Q: i128 = 50 * POS_SCALE as i128;
    const WINNING_DOMAIN: usize = 1;
    const EXPIRY_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 5_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    env.update_backing_fee_policy_with_cu(WINNING_DOMAIN as u16, 5_000, 0);
    env.top_up_backing_bucket(WINNING_DOMAIN as u16, 100_000, EXPIRY_SLOT);

    let trader_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let trader = env.create_portfolio(&trader_owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&trader_owner, trader, 52_501);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &counterparty_owner,
        counterparty,
        OPEN_Q,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(0, 1, WINNING_MARK);
    for portfolio in [counterparty, trader] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(env.portfolio_state(trader).pnl.get(), 5_000);
    let (_, before) = env.market_state();
    assert_eq!(before.current_slot, 1);
    assert_eq!(
        before.source_backing_buckets[WINNING_DOMAIN].expiry_slot,
        EXPIRY_SLOT
    );
    let trader_capital_before = env.portfolio_state(trader).capital.get();
    let provider_earnings_before =
        before.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings;
    let market_account_before = env.svm.get_account(&env.market).unwrap();
    let trader_account_before = env.svm.get_account(&trader).unwrap();
    let counterparty_account_before = env.svm.get_account(&counterparty).unwrap();

    env.svm.expire_blockhash();
    let retained_trade = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(trader_owner.pubkey(), true),
                    AccountMeta::new(counterparty_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(trader, false),
                    AccountMeta::new(counterparty, false),
                ],
                data: env
                    .trade_no_cpi_ix(trader, counterparty, 0, INCREASE_Q, WINNING_MARK, 0)
                    .encode(),
            },
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, &trader_owner, &counterparty_owner],
        env.svm.latest_blockhash(),
    );

    env.svm.warp_to_slot(EXPIRY_SLOT + 1);
    let trade = env.svm.send_transaction(retained_trade);

    let trader_after = env.portfolio_state(trader);
    let (_, after) = env.market_state();
    let provider_earnings_after =
        after.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings;
    let charged = provider_earnings_after - provider_earnings_before;
    let mut extracted = 0;
    if charged != 0 {
        let ledger = env.backing_domain_ledger_account();
        let provider_dest = env.token_account(env.admin.pubkey(), 0);
        env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(
            ledger,
            provider_dest,
            WINNING_DOMAIN as u16,
            charged,
        );
        extracted = env.token_amount(provider_dest);
    }
    assert!(
        trade.is_err(),
        "retained post-expiry trade charged {charged} backing-fee atoms, extracted {extracted} real SPL atoms, and reduced trader capital {} -> {} while authenticated slot {} exceeded expiry {} and engine slot stayed {}",
        trader_capital_before,
        trader_after.capital.get(),
        EXPIRY_SLOT + 1,
        EXPIRY_SLOT,
        after.current_slot,
    );
    assert_eq!(extracted, 0, "expired support may not earn a trade fee");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_account_before,
        "the rejected stale trade must roll back the market"
    );
    assert_eq!(
        env.svm.get_account(&trader).unwrap(),
        trader_account_before,
        "the rejected stale trade must roll back the trader"
    );
    assert_eq!(
        env.svm.get_account(&counterparty).unwrap(),
        counterparty_account_before,
        "the rejected stale trade must roll back the counterparty"
    );

    env.trade_asset_with_cu(
        0,
        &trader_owner,
        trader,
        &counterparty_owner,
        counterparty,
        -INCREASE_Q,
        WINNING_MARK,
        0,
    );
}

#[test]
fn v16_attack_lapsed_live_source_backing_expires_bounded_and_owner_can_reduce() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_permissionless_resolve_with_cu(10_000, 1);
    env.configure_auth_mark_with_cu(0, 100);
    let long_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short_owner = Keypair::new();
    let short = env.create_portfolio(&short_owner);
    let keeper_owner = Keypair::new();
    let keeper = env.create_portfolio(&keeper_owner);
    env.deposit(&long_owner, long, 40);
    env.deposit(&short_owner, short, 410);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        (2 * POS_SCALE) as i128,
        100,
        0,
    );

    for slot in 2..=30 {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 300);
        env.crank(
            keeper,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    for portfolio in [short, long, short] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 30,
                observations: crank_observations(0),
            },
        );
    }
    let after_adl = env.market_state().1;
    let long_leg = active_leg_for_asset(&env.portfolio_state(long), 0);
    let short_leg = active_leg_for_asset(&env.portfolio_state(short), 0);
    assert_eq!(after_adl.assets[0].b_long_num, 0);
    assert_eq!(after_adl.assets[0].b_short_num, 0);
    assert!(!long_leg.b_stale && !short_leg.b_stale);
    let generated_backing = after_adl.source_backing_buckets[1];
    assert_eq!(generated_backing.status, BackingBucketStatusV16::Fresh);
    assert!(generated_backing.fresh_unliened_backing_num > 0);

    for slot in 31..=160 {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 300);
        env.crank(
            keeper,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    let before_expiry = env.market_state().1;
    let lapsed = before_expiry.source_backing_buckets[1];
    assert_eq!(lapsed.status, BackingBucketStatusV16::Fresh);
    assert!(lapsed.expiry_slot < 160);
    assert!(env.portfolio_state(long).pnl.get() > 0);
    let vault_before = before_expiry.vault;
    let vault_tokens_before = env.token_amount(env.vault);

    env.svm.expire_blockhash();
    let expiry_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 160,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
            ],
            &[],
        )
        .expect("permissionless refresh must commit one lapsed-backing expiry step");
    assert_cu_within("lapsed source-backing expiry", expiry_cu, CRANK_CU_LIMIT);

    let after_expiry = env.market_state().1;
    let expired = after_expiry.source_backing_buckets[1];
    assert_eq!(expired.status, BackingBucketStatusV16::Expired);
    assert_eq!(expired.fresh_unliened_backing_num, 0);
    assert_ne!(
        health_cert(&env.portfolio_state(long)).cert_risk_epoch,
        after_expiry.risk_epoch,
        "expiry advances risk state and leaves one bounded certification step"
    );
    assert_eq!(after_expiry.vault, vault_before);
    assert_eq!(env.token_amount(env.vault), vault_tokens_before);

    env.svm.expire_blockhash();
    let certify_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 160,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
            ],
            &[],
        )
        .expect("the next bounded auto-crank must certify after expiry");
    assert_cu_within(
        "post-expiry account certification",
        certify_cu,
        CRANK_CU_LIMIT,
    );
    assert!(health_cert(&env.portfolio_state(long)).valid);

    let position_before = active_leg_for_asset(&env.portfolio_state(long), 0)
        .basis_pos_q
        .unsigned_abs();
    let exit_cu = env.rebalance_reduce_with_cu(&long_owner, long, 0, POS_SCALE / 4);
    assert_cu_within("post-expiry owner reduction", exit_cu, CUSTODY_CU_LIMIT);
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(long), 0)
            .basis_pos_q
            .unsigned_abs(),
        position_before - POS_SCALE / 4
    );
    let done = env.market_state().1;
    assert_eq!(done.vault, vault_before);
    assert_eq!(done.vault as u64, env.token_amount(env.vault));
}

// security.md sweep — backing-bucket expiry (#33): a winner's positive pnl backed by a backing bucket
// with an expiry. After the expiry passes, the payout must still be CONSERVING — the winner is paid at
// most the (still-available) backing, never more, and the system never over-pays expired backing.
// (Engine e833b7f / commit 6433346 "Expire lapsed backing at resolved close" makes the realize step
// expire past-expiry backing instead of stranding the winner with Stale — see the prior finding.)
#[test]
fn v16_attack_backing_expiry_no_overpay() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 40, 5); // backing expires at slot 5
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    env.add_source_positive_pnl(p, 1, 40);
    // advance PAST the backing expiry.
    env.svm.warp_to_slot(20);
    let _ = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 20,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[],
    );
    let vault_before = env.market_state().1.vault;
    env.resolve();
    // two close passes to converge.
    let mut out = 0u128;
    for _ in 0..2 {
        let d = env.close_resolved(&owner, p);
        out += env.token_amount(d) as u128;
    }
    let (_, g) = env.market_state();
    // winner gets at least its senior capital (1000) and at most capital + backing (1040) -- no over-pay.
    assert!(out >= 1_000, "winner recovers at least its senior capital");
    assert!(
        out <= 1_040,
        "winner paid at most capital + backing (no over-pay against expired backing): out={}",
        out
    );
    assert!(g.vault <= vault_before, "vault not over-credited");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
}
