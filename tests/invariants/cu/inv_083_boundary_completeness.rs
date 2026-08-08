//! INV-083 - Boundary completeness.
//!
//! Normative obligation: public routes cover zero, one, maximum, maximum-plus
//! and shape-boundary cases. Any value excluded by a handler must reject before
//! allocation, mutation, panic, or custody movement.
//!
//! Evidence in this file (I/C): oversized batch leg vectors at the public decode
//! boundary reject as instruction data errors rather than allocating a large
//! vector or panicking the SBF program. Other boundary cases are distributed in
//! INV-011, INV-058, INV-077, and the instruction-decoder Kani owner.

use super::*;

#[test]
fn v16_program_batch_decode_oversized_vectors_reject_before_allocation() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    let taker = Keypair::new();
    let maker = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let maker_account = env.create_portfolio(&maker);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let maker_before = env.svm.get_account(&maker_account).unwrap();

    let no_cpi_legs: Vec<BatchTradeLeg> = (0..u16::from(u8::MAX))
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let no_cpi = env.send(
        env.batch_trade_no_cpi_ix(taker_account, maker_account, no_cpi_legs),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(maker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(maker_account, false),
        ],
        &[&taker, &maker],
    );
    let no_cpi_err = no_cpi.expect_err("oversized BatchTradeNoCpi must reject");
    assert!(no_cpi_err.contains("InvalidInstructionData"));
    assert!(
        !no_cpi_err.contains("ProgramFailedToComplete")
            && !no_cpi_err.contains("memory allocation failed"),
        "oversized BatchTradeNoCpi must not panic the program: {no_cpi_err}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&maker_account).unwrap(), maker_before);

    let cpi_legs: Vec<BatchTradeCpiLeg> = (0..u16::from(u8::MAX))
        .map(|asset_index| BatchTradeCpiLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let cpi = env.send(
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: env.portfolio_id(taker_account),
            account_b_portfolio_id: env.portfolio_id(maker_account),
            legs: cpi_legs,
        },
        vec![],
        &[],
    );
    let cpi_err = cpi.expect_err("oversized BatchTradeCpi must reject");
    assert!(cpi_err.contains("InvalidInstructionData"));
    assert!(
        !cpi_err.contains("ProgramFailedToComplete")
            && !cpi_err.contains("memory allocation failed"),
        "oversized BatchTradeCpi must not panic the program: {cpi_err}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&maker_account).unwrap(), maker_before);
}

// security.md sweep — numerical boundary (#37 i128::MIN negation / #38 wide overflow):
// extreme trade sizes must be rejected cleanly (no panic, no OI, no value movement).
#[test]
fn v16_attack_extreme_size_trade_rejected_no_panic() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    for sz in [i128::MIN, i128::MAX, i128::MIN + 1] {
        env.svm.expire_blockhash();
        let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, sz, 100, 0);
        assert!(r.is_err(), "extreme size {} must be rejected cleanly", sz);
    }
    let (_, g) = env.market_state();
    assert_eq!(
        g.assets[0].oi_eff_long_q, 0,
        "no OI from rejected extreme-size trades"
    );
    assert_eq!(g.c_tot, 2_000_000, "no capital moved");
}

// security.md sweep — asset_index bounds (#37/#39): an out-of-range asset_index on any instruction
// must reject cleanly (no OOB access / panic / state corruption).
#[test]
fn v16_attack_out_of_range_asset_index_rejected() {
    let mut env = V16CuEnv::new(); // 1 asset (index 0 valid)
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();
    for bad in [1u16, 7, 255, 9999, u16::MAX] {
        // trade on a bad asset index
        env.svm.expire_blockhash();
        let rt = env.try_trade_asset_with_cu(bad, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
        assert!(
            rt.is_err(),
            "trade on out-of-range asset_index {} must reject",
            bad
        );
        // crank on a bad asset index
        env.svm.expire_blockhash();
        let rc = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(bad),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(pa, false),
            ],
            &[],
        );
        assert!(
            rc.is_err(),
            "crank on out-of-range asset_index {} must reject",
            bad
        );
        // push auth mark on a bad asset index (admin)
        env.svm.expire_blockhash();
        let rm = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::PushAuthMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: bad,
                now_slot: 1,
                mark_e6: 100,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        );
        assert!(
            rm.is_err(),
            "push auth mark on out-of-range asset_index {} must reject",
            bad
        );
    }
    // no corruption from any rejected OOB attempt.
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI created");
}

// security.md sweep — domain-indexed public calls must reject domains outside the configured market
// slots before touching accounting, ledgers, or SPL custody. On a one-asset market, domains 0/1 are
// valid and domain 2 is out of range; a real market authority with valid token accounts still cannot
// write or move funds through that phantom domain.
#[test]
fn v16_attack_domain_indexed_calls_reject_out_of_range_atomically() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    const BAD_DOMAIN: u16 = 2;

    env.top_up_insurance(1_000);
    env.top_up_backing_bucket_with_cu(0, 1_000, 10_000);
    let (_, funded) = env.market_state();
    assert_eq!(
        funded.insurance_domain_budget.len(),
        2,
        "one-asset market has exactly domains 0 and 1"
    );
    assert!(
        funded.vault >= 2_000,
        "setup leaves real withdrawable vault balance"
    );

    let insurance_src = env.token_account_for_mint(env.mint, admin.pubkey(), 123);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let source_before = env.svm.get_account(&insurance_src).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let topup_ins = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            market_id: 0,
            domain: BAD_DOMAIN,
            amount: 123,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(insurance_src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        topup_ins.is_err(),
        "phantom insurance domain top-up must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&insurance_src).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let backing_src = env.token_account_for_mint(env.mint, admin.pubkey(), 456);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let source_before = env.svm.get_account(&backing_src).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let topup_backing = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            market_id: 0,
            domain: BAD_DOMAIN,
            amount: 456,
            expiry_slot: 10_000,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        topup_backing.is_err(),
        "phantom backing bucket top-up must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&backing_src).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let insurance_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let dest_before = env.svm.get_account(&insurance_dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let withdraw_ins = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: BAD_DOMAIN as u16,
            amount: 1,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(insurance_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_ins.is_err(),
        "phantom insurance asset withdraw must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&insurance_dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let backing_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let dest_before = env.svm.get_account(&backing_dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let withdraw_backing = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: BAD_DOMAIN,
            amount: 1,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_backing.is_err(),
        "phantom backing bucket withdraw must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&backing_dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let earnings_ledger = env.backing_domain_ledger_account();
    let earnings_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&earnings_ledger).unwrap();
    let dest_before = env.svm.get_account(&earnings_dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let withdraw_earnings = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: BAD_DOMAIN,
            amount: 1,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(earnings_ledger, false),
            AccountMeta::new(earnings_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_earnings.is_err(),
        "phantom backing earnings withdraw must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&earnings_ledger).unwrap(),
        ledger_before
    );
    assert_eq!(env.svm.get_account(&earnings_dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let sync_ledger = env.backing_domain_ledger_account();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&sync_ledger).unwrap();
    env.svm.expire_blockhash();
    let sync = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SyncBackingDomainLedger { domain: BAD_DOMAIN },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(sync_ledger, false),
        ],
        &[&admin],
    );
    assert!(sync.is_err(), "phantom backing ledger sync must reject");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&sync_ledger).unwrap(), ledger_before);

    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let fee_policy = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBackingFeePolicy {
            market_id: 0,
            policy_sequence: u64::MAX,
            domain: BAD_DOMAIN,
            fee_bps: 77,
            insurance_share_bps: 5_000,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        fee_policy.is_err(),
        "phantom backing fee policy update must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);

    let (_, g) = env.market_state();
    assert_eq!(
        g.insurance_domain_budget.len(),
        2,
        "no phantom domain was appended"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == canonical vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — large-amount deposit boundary + TVL cap (#37): the vault is capped at
// MAX_VAULT_TVL (overflow prevention). A deposit above the cap must reject; a large deposit just below
// it must credit exactly (no truncation/wraparound in the u128 aggregates) and round-trip exactly.
#[test]
fn v16_attack_large_amount_deposit_withdraw_exact() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    const MAX_TVL: u128 = 10_000_000_000_000_000;
    // over-cap deposit -> reject.
    let over = MAX_TVL + 1;
    let src_over = env.token_account_for_mint(env.mint, owner.pubkey(), over as u64);
    env.svm.expire_blockhash();
    let r = env.send(
        env.deposit_ix(p, over),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(src_over, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "deposit above MAX_VAULT_TVL must reject (overflow/abuse cap)"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        0,
        "no capital credited on over-cap deposit"
    );

    // large below-cap deposit -> exact credit, no overflow.
    let big: u128 = MAX_TVL - 7;
    env.deposit(&owner, p, big);
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        big,
        "capital credited exactly (no overflow/truncation)"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.c_tot, big, "c_tot == the large deposit");
    assert_eq!(g1.vault, big, "vault == the large deposit");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    // withdraw it all back -> exact.
    let (dest, _) = env.withdraw_with_cu(&owner, p, big);
    assert_eq!(
        env.token_amount(dest) as u128,
        big,
        "withdrew exactly the large amount"
    );
    let (_, g2) = env.market_state();
    assert_eq!(g2.c_tot, 0, "c_tot back to 0");
    assert_eq!(g2.vault, 0, "vault drained exactly");
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation");
}

// security.md sweep - mark input bounds (#37/#39): the mark authority controls settlement input, but
// invalid marks or an EWMA halflife of zero must not even transiently rewrite the oracle profile. Existing
// conservation tests cover "no panic"; this asserts exact market rollback for every public mark entrypoint.
#[test]
fn v16_attack_mark_input_bounds_reject_atomically() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let over_max = percolator::MAX_ORACLE_PRICE + 1;
    env.svm.warp_to_slot(1);

    let reject_unchanged = |env: &mut V16CuEnv, ix: ProgInstruction, label: &str| {
        let before = env.svm.get_account(&env.market).unwrap();
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&admin],
        );
        assert!(rejected.is_err(), "{label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before,
            "{label} must leave the market byte-identical"
        );
    };

    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 0,
        },
        "ConfigureAuthMark zero initial mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: over_max,
        },
        "ConfigureAuthMark over-max initial mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 0,
            mark_ewma_halflife_slots: 4,
            mark_min_fee: 0,
        },
        "ConfigureEwmaMark zero initial mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: over_max,
            mark_ewma_halflife_slots: 4,
            mark_min_fee: 0,
        },
        "ConfigureEwmaMark over-max initial mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 0,
            mark_min_fee: 0,
        },
        "ConfigureEwmaMark zero halflife",
    );

    env.svm.expire_blockhash();
    let ewma = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 4,
            mark_min_fee: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        ewma.is_ok(),
        "valid EWMA configuration should succeed: {ewma:?}"
    );
    let (cfg_ewma, _) = env.market_state();
    assert_eq!(
        cfg_ewma.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_EWMA_MARK
    );
    assert_eq!(cfg_ewma.mark_ewma_e6, 100);
    assert_eq!(cfg_ewma.mark_ewma_halflife_slots, 4);

    env.svm.warp_to_slot(2);
    reject_unchanged(
        &mut env,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 0,
        },
        "PushEwmaMark zero mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: over_max,
        },
        "PushEwmaMark over-max mark",
    );

    env.svm.expire_blockhash();
    let valid_ewma_push = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 120,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        valid_ewma_push.is_ok(),
        "valid EWMA push remains live after rejected bounds probes: {valid_ewma_push:?}"
    );
    assert_eq!(env.market_state().0.mark_ewma_last_slot, 2);

    env.svm.warp_to_slot(3);
    env.svm.expire_blockhash();
    let auth = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: 3,
            asset_index: 0,
            now_slot: 3,
            initial_mark_e6: 200,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        auth.is_ok(),
        "valid AuthMark configuration should succeed: {auth:?}"
    );

    env.svm.warp_to_slot(4);
    reject_unchanged(
        &mut env,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 4,
            mark_e6: 0,
        },
        "PushAuthMark zero mark",
    );
    reject_unchanged(
        &mut env,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 4,
            mark_e6: over_max,
        },
        "PushAuthMark over-max mark",
    );

    env.svm.expire_blockhash();
    let valid_auth_push = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: 4,
            asset_index: 0,
            now_slot: 4,
            mark_e6: 220,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        valid_auth_push.is_ok(),
        "valid AuthMark push remains live after rejected bounds probes: {valid_auth_push:?}"
    );
    let (cfg_auth, _) = env.market_state();
    assert_eq!(
        cfg_auth.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_AUTH_MARK
    );
    assert_eq!(cfg_auth.mark_ewma_e6, 220);
    assert_eq!(cfg_auth.oracle_target_price_e6, 220);
    assert_eq!(cfg_auth.mark_ewma_last_slot, 4);
}

// security.md sweep - sparse append DoS: permissionless activation may append exactly the next
// configured slot, or reuse a retired slot, but it must not accept sparse jumps. Otherwise a stranger
// could force large market-account growth or create holes in the asset table.
#[test]
fn v16_attack_permissionless_sparse_append_indices_rejected_without_realloc_or_fee() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);
    env.svm.warp_to_slot(1);

    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let (cfg_before, group_before) = env.market_state();
    assert_eq!(
        group_before.config.max_market_slots, 1,
        "starts as a one-asset market"
    );
    assert_eq!(
        cfg_before.free_market_slot_count, 0,
        "no retired slots are reusable"
    );

    for bad_index in [2u16, 7, u16::MAX] {
        let source = env.token_account(creator.pubkey(), FEE as u64);
        let source_before = env.svm.get_account(&source).unwrap();
        env.svm.expire_blockhash();
        let rejected = env.send(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index: bad_index,
                now_slot: 1,
                initial_price: 100,
                max_init_fee: u128::MAX,
                insurance_authority: creator.pubkey().to_bytes(),
                insurance_operator: creator.pubkey().to_bytes(),
                backing_bucket_authority: creator.pubkey().to_bytes(),
                oracle_authority: creator.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&creator],
        );
        assert!(
            rejected.is_err(),
            "permissionless sparse append at index {bad_index} must reject"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "rejected sparse append at index {bad_index} did not realloc or mutate the market"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "rejected sparse append at index {bad_index} did not move vault tokens"
        );
        assert_eq!(
            env.svm.get_account(&source).unwrap(),
            source_before,
            "rejected sparse append at index {bad_index} did not debit the creator"
        );
        let (_, rejected_group) = env.market_state();
        assert_eq!(
            rejected_group.config.max_market_slots, group_before.config.max_market_slots,
            "rejected sparse append at index {bad_index} did not advance configured slots"
        );
        assert_eq!(
            rejected_group.insurance_domain_budget, group_before.insurance_domain_budget,
            "rejected sparse append at index {bad_index} did not credit any domain budget"
        );
    }

    let valid_source = env.token_account(creator.pubkey(), FEE as u64);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(valid_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    )
    .expect("contiguous permissionless append still succeeds after sparse attempts");
    let (_, valid_group) = env.market_state();
    assert_eq!(
        valid_group.config.max_market_slots, 2,
        "valid append advances exactly one slot"
    );
    assert_eq!(valid_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(
        env.token_amount(valid_source),
        0,
        "valid append pulls only the real fee"
    );
}

// Fresh InitMarket is a public bootstrap boundary: an attacker or misconfigured launcher should not be
// able to burn a newly allocated market account into a half-written, unusable slab with grief params.
#[test]
fn v16_attack_init_market_rejects_grief_config_without_burning_market_account() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let valid = V16CuMarketParams::default();
    let market_len = state::market_account_len_for_capacity(valid.max_portfolio_assets as usize)
        .expect("market len");
    let portfolio_len =
        state::portfolio_account_len_for_market_slots(valid.max_portfolio_assets as usize)
            .expect("portfolio len");

    let mut zero_portfolios = V16CuMarketParams::default();
    zero_portfolios.max_portfolio_assets = 0;
    let mut over_portfolio_cap = V16CuMarketParams::default();
    over_portfolio_cap.max_portfolio_assets =
        percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS + 1;
    let mut impossible_bound = V16CuMarketParams::default();
    impossible_bound.h_max = (BOUND_SCALE + 1) as u64;
    let mut zero_oracle = V16CuMarketParams::default();
    zero_oracle.initial_price = 0;
    let mut over_oracle = V16CuMarketParams::default();
    over_oracle.initial_price = percolator::MAX_ORACLE_PRICE + 1;
    let mut fee_floor_dos = V16CuMarketParams::default();
    fee_floor_dos.max_trading_fee_bps = 99;
    fee_floor_dos.trade_fee_base_bps = 100;
    let mut maintenance_drain = V16CuMarketParams::default();
    maintenance_drain.maintenance_fee_per_slot = percolator::MAX_PROTOCOL_FEE_ABS + 1;

    for (label, params) in [
        ("zero portfolio cap", zero_portfolios),
        ("portfolio cap above wrapper limit", over_portfolio_cap),
        ("h_max above bound scale", impossible_bound),
        ("zero initial oracle price", zero_oracle),
        ("initial oracle price above max", over_oracle),
        ("trade fee floor above max fee", fee_floor_dos),
        ("maintenance fee above protocol cap", maintenance_drain),
    ] {
        let market = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &market,
            market_len,
            env.program_id,
        );
        let market_before = env
            .svm
            .get_account(&market.pubkey())
            .expect("market account");

        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            init_market_instruction(&params),
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market.pubkey(), false),
                AccountMeta::new_readonly(env.mint, false),
            ],
            &[&admin],
        );
        assert!(
            rejected.is_err(),
            "{label}: hostile InitMarket config must reject"
        );
        assert_eq!(
            env.svm
                .get_account(&market.pubkey())
                .expect("market account"),
            market_before,
            "{label}: rejected InitMarket must not dirty the freshly allocated market account"
        );

        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            init_market_instruction(&valid),
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market.pubkey(), false),
                AccountMeta::new_readonly(env.mint, false),
            ],
            &[&admin],
        )
        .expect("valid InitMarket after rejected grief config");
        let market_after_valid = env
            .svm
            .get_account(&market.pubkey())
            .expect("market account");
        let (cfg, group) = state::read_market(&market_after_valid.data).expect("valid market");
        assert_eq!(
            cfg.marketauth,
            admin.pubkey().to_bytes(),
            "{label}: valid retry keeps the real initializer as market authority"
        );
        assert_eq!(
            cfg.collateral_mint,
            env.mint.to_bytes(),
            "{label}: valid retry pins the intended collateral mint"
        );
        assert_eq!(
            group.assets[0].effective_price, valid.initial_price,
            "{label}: valid retry initializes a sane base oracle"
        );

        let owner = Keypair::new();
        env.ensure_signer_account(owner.pubkey());
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio,
            portfolio_len,
            env.program_id,
        );
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(market.pubkey(), false),
                AccountMeta::new(portfolio.pubkey(), false),
            ],
            &[&owner],
        )
        .expect("portfolio init after valid market retry");
        let portfolio_account = env
            .svm
            .get_account(&portfolio.pubkey())
            .expect("portfolio account");
        state::read_portfolio(&portfolio_account.data).expect("valid portfolio after retry");
    }
}
