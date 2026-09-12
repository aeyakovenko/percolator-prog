//! INV-028: a new public profit cannot reuse already-consumed source backing.
//!
//! Two public trade/mark rounds create distinct claims in the same domain. The first is closed
//! and converted, then its remaining fresh backing is withdrawn. The second claim cannot reuse
//! consumed backing or the still-stale loser's prospective payment. Sibling backing and unreserved
//! insurance are funded negative controls; a matching-domain refill creates only partial credit.
//! Once public loss settlement supplies the missing backing, only the independently recomputed
//! surplus is withdrawable. A one-atom withdrawal across the claim watermark rolls back exactly.
//! This is a consumed-backing cap witness, not an expiry, fee, terminal-payout, or summary census.
//! All accounts are System/SPL/ATA-created and all economic state comes from public instructions.

use super::*;

fn assert_current_source_cap(
    env: &V16CuEnv,
    expected_fresh: u128,
    expected_claim: u128,
    expected_consumed: u128,
) -> u128 {
    const DOMAIN: usize = 1;
    let group = env.market_state().1;
    let bucket = group.source_backing_buckets[DOMAIN];
    let reservation = group.insurance_credit_reservations[DOMAIN];
    let source = group.source_credit[DOMAIN];
    let now = env.svm.get_sysvar::<Clock>().slot;

    // Rebuild the denominator from owners, not the cached domain claim total. The fixture has
    // no liens, so every local claim competes for the same currently unencumbered backing.
    let mut claims_num = 0u128;
    for key in &env.portfolios {
        for local in &env.portfolio_state(*key).source_domains {
            if !local.is_occupied() {
                continue;
            }
            assert_eq!(local.domain.get() as usize, DOMAIN);
            assert_eq!(local.source_claim_liened_num.get(), 0);
            assert_eq!(local.source_claim_impaired_num.get(), 0);
            claims_num = claims_num
                .checked_add(local.source_claim_bound_num.get())
                .unwrap();
        }
    }
    assert_eq!(claims_num, expected_claim * BOUND_SCALE);
    assert_eq!(source.positive_claim_bound_num, claims_num);
    assert_eq!(bucket.valid_liened_backing_num, 0);
    assert_eq!(bucket.impaired_liened_backing_num, 0);
    assert_eq!(
        bucket.consumed_liened_backing_num,
        expected_consumed * BOUND_SCALE
    );
    assert_eq!(
        bucket.fresh_unliened_backing_num,
        expected_fresh * BOUND_SCALE
    );
    assert_eq!(reservation.insurance_credit_reserved_num, 0);
    assert_eq!(reservation.valid_liened_insurance_num, 0);
    assert_eq!(reservation.impaired_liened_insurance_num, 0);

    // Raw bucket freshness and reservation records are the numerator oracle. Neither consumed
    // principal, the sibling bucket, vault residual, nor an unreserved insurance budget qualifies.
    let available_num =
        if bucket.status == BackingBucketStatusV16::Fresh && bucket.expiry_slot > now {
            bucket.fresh_unliened_backing_num
        } else {
            0
        };
    assert_eq!(available_num, expected_fresh * BOUND_SCALE);
    let rate = if claims_num == 0 {
        percolator::CREDIT_RATE_SCALE
    } else {
        available_num
            .checked_mul(percolator::CREDIT_RATE_SCALE)
            .unwrap()
            / claims_num
    }
    .min(percolator::CREDIT_RATE_SCALE);
    assert_eq!(source.credit_rate_num, rate);
    let usable_num =
        claims_num.checked_mul(source.credit_rate_num).unwrap() / percolator::CREDIT_RATE_SCALE;
    assert!(usable_num <= available_num);
    assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
    available_num.saturating_sub(claims_num) / BOUND_SCALE
}

#[test]
fn v16_program_current_source_backing_caps_repeated_public_profit() {
    use super::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const DOMAIN: u16 = 1;
    const DEPOSIT: u128 = 1_000;
    const PROFIT: u128 = 100;
    const BACKING: u128 = 173;
    const SIBLING_BACKING: u128 = 997;
    const UNRESERVED_INSURANCE: u128 = 401;
    const REFILL: u128 = 73;
    const EXPIRY: u64 = 100;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
    );
    let admin = env.admin.insecure_clone();
    let owners = [Keypair::new(), Keypair::new()];
    let portfolios: [Pubkey; 2] = std::array::from_fn(|actor| {
        env.svm
            .airdrop(&owners[actor].pubkey(), 1_000_000_000)
            .unwrap();
        let key = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &key,
            env.portfolio_account_len,
            env.program_id,
        );
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(key.pubkey(), false),
            ],
            &[&owners[actor]],
        )
        .unwrap();
        env.portfolios.push(key.pubkey());
        key.pubkey()
    });
    let tokens: [Pubkey; 2] = std::array::from_fn(|actor| {
        create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint)
    });
    let provider = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    let provider_funding = BACKING + SIBLING_BACKING + UNRESERVED_INSURANCE + REFILL;
    for (token, amount) in [
        (tokens[0], DEPOSIT),
        (tokens[1], DEPOSIT),
        (provider, provider_funding),
    ] {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &admin.pubkey(),
                &[],
                amount as u64,
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();
    }
    for actor in 0..2 {
        env.send(
            env.deposit_ix(portfolios[actor], DEPOSIT),
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[actor], false),
                AccountMeta::new(tokens[actor], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[actor]],
        )
        .unwrap();
    }
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.top_up_backing_bucket_from_admin_token_with_cu(provider, DOMAIN, BACKING, EXPIRY);

    let mut max_trade_cu = 0;
    let mut max_crank_cu = 0;
    let mut conversion_cu = 0;
    let mut initial_withdrawal_cu = 0;
    for (round, open_price, close_price) in [(0u64, 100, 105), (1, 105, 110)] {
        max_trade_cu = max_trade_cu.max(env.trade_with_cu(
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            SIZE_Q,
            open_price,
            0,
        ));
        env.svm.warp_to_slot(round + 2);
        env.push_auth_mark_for_asset_as_admin(0, round + 2, close_price);
        for portfolio in [portfolios[1], portfolios[0]] {
            if round == 1 && portfolio == portfolios[1] {
                continue;
            }
            max_crank_cu = max_crank_cu.max(env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: round + 2,
                    observations: crank_observations(0),
                },
            ));
        }
        assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), PROFIT as i128);
        if round == 0 {
            max_trade_cu = max_trade_cu.max(env.trade_with_cu(
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                -SIZE_Q,
                close_price,
                0,
            ));
            assert!(percolator::active_bitmap_is_empty(active_bitmap(
                &env.portfolio_state(portfolios[0])
            )));
            assert_eq!(
                assert_current_source_cap(&env, BACKING + PROFIT, PROFIT, 0),
                BACKING
            );
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            conversion_cu = env.convert_released_pnl_with_cu(&owners[0], portfolios[0], PROFIT);
            assert_eq!(
                env.portfolio_state(portfolios[0]).capital.get(),
                DEPOSIT + PROFIT
            );
            assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), 0);
            assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
            assert_eq!(assert_current_source_cap(&env, BACKING, 0, PROFIT), BACKING);
            initial_withdrawal_cu =
                env.withdraw_backing_bucket_to_admin_token_with_cu(provider, DOMAIN, BACKING);
            assert_eq!(assert_current_source_cap(&env, 0, 0, PROFIT), 0);
        }
    }
    assert_eq!(assert_current_source_cap(&env, 0, PROFIT, PROFIT), 0);
    let capped = env.market_state().1.source_credit[DOMAIN as usize];
    assert_eq!(capped.credit_rate_num, 0);
    let stale_loser = env.portfolio_state(portfolios[1]);
    assert_eq!(stale_loser.capital.get(), DEPOSIT - PROFIT);
    assert_eq!(stale_loser.pnl.get(), 0);
    assert!(env.market_state().1.assets[0].k_short < active_leg_for_asset(&stale_loser, 0).k_snap);

    // These funded public controls increase custody but supply no currently usable source credit.
    env.top_up_backing_bucket_from_admin_token_with_cu(provider, 0, SIBLING_BACKING, EXPIRY);
    assert_eq!(assert_current_source_cap(&env, 0, PROFIT, PROFIT), 0);
    env.send(
        ProgInstruction::TopUpInsuranceDomain {
            domain: DOMAIN,
            market_id: 0,
            authority_epoch: 0,
            intent_id: 0,
            amount: UNRESERVED_INSURANCE,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(provider, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    )
    .unwrap();
    assert_eq!(assert_current_source_cap(&env, 0, PROFIT, PROFIT), 0);
    let controls = env.market_state().1;
    assert_eq!(
        controls.source_credit[DOMAIN as usize].credit_rate_num,
        capped.credit_rate_num
    );
    assert_eq!(
        controls.source_backing_buckets[0].fresh_unliened_backing_num,
        SIBLING_BACKING * BOUND_SCALE
    );
    assert_eq!(
        controls.insurance_domain_budget[DOMAIN as usize],
        UNRESERVED_INSURANCE
    );

    let refill_cu =
        env.top_up_backing_bucket_from_admin_token_with_cu(provider, DOMAIN, REFILL, EXPIRY);
    assert_eq!(
        assert_current_source_cap(&env, REFILL, PROFIT, PROFIT - REFILL),
        0
    );
    let partial_rate = env.market_state().1.source_credit[DOMAIN as usize].credit_rate_num;
    assert!(partial_rate > 0 && partial_rate < percolator::CREDIT_RATE_SCALE);
    assert_eq!(env.portfolio_state(portfolios[1]), stale_loser);

    let vault_before_settlement = env.svm.get_account(&env.vault).unwrap();
    max_crank_cu = max_crank_cu.max(env.crank(
        portfolios[1],
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    ));
    assert_eq!(
        env.portfolio_state(portfolios[1]).capital.get(),
        DEPOSIT - 2 * PROFIT
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_settlement
    );
    let surplus = assert_current_source_cap(&env, REFILL + PROFIT, PROFIT, 0);
    assert_eq!(
        surplus, REFILL,
        "only realized backing creates provider surplus"
    );

    let withdraw = |env: &mut V16CuEnv, amount: u128| {
        env.svm.expire_blockhash();
        let instruction = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(provider, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::WithdrawBackingBucket {
                domain: DOMAIN,
                market_id: env.asset_market_id(0),
                authority_epoch: env.withdrawal_authority_epoch(admin.pubkey(), 0, false),
                amount,
            }
            .encode(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), instruction],
            Some(&env.payer.pubkey()),
            &[&env.payer, &admin],
            env.svm.latest_blockhash(),
        );
        env.svm.send_transaction(tx)
    };
    let reject_at_cap = |env: &mut V16CuEnv, amount: u128| {
        let tracked = [
            env.market,
            portfolios[0],
            portfolios[1],
            env.vault,
            provider,
            env.mint,
            tokens[0],
            tokens[1],
            admin.pubkey(),
        ];
        let before: Vec<_> = tracked.iter().map(|key| env.svm.get_account(key)).collect();
        let error =
            withdraw(env, amount).expect_err("one atom would dilute existing source claims");
        assert_eq!(
            error.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::EngineLockActive as u32)
            )
        );
        for (key, account) in tracked.iter().zip(before) {
            assert_eq!(
                env.svm.get_account(key),
                account,
                "rejected withdrawal changed {key}"
            );
        }
        error.meta.compute_units_consumed
    };
    let first_reject_cu = reject_at_cap(&mut env, surplus + 1);
    let provider_before = env.token_amount(provider);
    let vault_before = env.token_amount(env.vault);
    let withdrawal_cu = withdraw(&mut env, surplus)
        .expect("exact recomputed surplus is withdrawable")
        .compute_units_consumed;
    assert_eq!(env.token_amount(provider), provider_before + surplus as u64);
    assert_eq!(env.token_amount(env.vault), vault_before - surplus as u64);
    assert_eq!(assert_current_source_cap(&env, PROFIT, PROFIT, 0), 0);
    let boundary_reject_cu = reject_at_cap(&mut env, 1);
    assert_eq!(assert_current_source_cap(&env, PROFIT, PROFIT, 0), 0);

    let final_group = env.market_state().1;
    assert_eq!(
        final_group.source_backing_buckets[0],
        controls.source_backing_buckets[0]
    );
    assert_eq!(
        final_group.insurance_domain_budget,
        controls.insurance_domain_budget
    );
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(u128::from(mint.supply), 2 * DEPOSIT + provider_funding);
    assert_eq!(
        env.token_amount(env.vault) + env.token_amount(provider),
        mint.supply
    );
    for (label, cu, limit) in [
        ("INV-028 public trade", max_trade_cu, TRADE_CU_LIMIT),
        ("INV-028 public crank", max_crank_cu, CRANK_CU_LIMIT),
        (
            "INV-028 consume old backing",
            conversion_cu,
            CUSTODY_CU_LIMIT,
        ),
        ("INV-028 eligible refill", refill_cu, CUSTODY_CU_LIMIT),
        (
            "INV-028 withdraw old fresh backing",
            initial_withdrawal_cu,
            CUSTODY_CU_LIMIT,
        ),
        (
            "INV-028 exact surplus withdrawal",
            withdrawal_cu,
            CUSTODY_CU_LIMIT,
        ),
        (
            "INV-028 diluted claim rejection",
            first_reject_cu,
            CUSTODY_CU_LIMIT,
        ),
        (
            "INV-028 exact watermark rejection",
            boundary_reject_cu,
            CUSTODY_CU_LIMIT,
        ),
    ] {
        assert_cu_within(label, cu, limit);
        eprintln!("{label}: {cu} CU");
    }
}
