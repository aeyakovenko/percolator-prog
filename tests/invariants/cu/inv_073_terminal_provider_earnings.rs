//! Publicly earned provider fees keep their beneficiary through unsigned terminal payout.

use super::*;

#[test]
fn v16_program_terminal_earned_fees_have_unsigned_exact_disposition() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    const CAPITAL: [u64; 2] = [3_130, 10_000];
    const BACKING: u64 = 51_500;
    const SUPPLY: u64 = CAPITAL[0] + CAPITAL[1] + 1_000 + BACKING;
    const DOMAIN: u16 = 1;
    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 4,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            maintenance_fee_per_slot: 530,
            ..V16CuMarketParams::default()
        },
    );
    let admin = env.admin.insecure_clone();
    let provider = Keypair::new();
    let owners = [Keypair::new(), Keypair::new()];
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&provider),
        0,
        processor::ASSET_AUTH_BACKING_BUCKET,
        provider.pubkey().to_bytes(),
    )
    .unwrap();
    env.svm.warp_to_slot(1);
    env.configure_permissionless_resolve_with_cu(100, 5);
    for asset in [0, 1] {
        env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
    }
    env.update_backing_fee_policy_with_cu(DOMAIN, 5_000, 2_500);
    env.svm.expire_blockhash();
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);

    let tokens = [&owners[0], &owners[1], &provider].map(|owner| {
        env.ensure_signer_account(owner.pubkey());
        create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
    });
    for (token, amount) in tokens
        .into_iter()
        .zip([CAPITAL[0] + 500, CAPITAL[1] + 500, BACKING])
    {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &admin.pubkey(),
                &[],
                amount,
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();
    }
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::set_authority(
            &spl_token::ID,
            &env.mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &admin.pubkey(),
            &[],
        )
        .unwrap(),
        &[&admin],
    )
    .unwrap();
    let portfolios = owners.each_ref().map(|owner| {
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
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(key.pubkey(), false),
            ],
            &[owner],
        )
        .unwrap();
        env.portfolios.push(key.pubkey());
        key.pubkey()
    });
    let ledger_key = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger_key,
        state::backing_domain_ledger_account_len(),
        env.program_id,
    );
    let ledger = ledger_key.pubkey();
    let deposit = |env: &mut V16CuEnv, index: usize, amount: u64| {
        env.send(
            env.deposit_ix(portfolios[index], amount.into()),
            vec![
                AccountMeta::new(owners[index].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[index], false),
                AccountMeta::new(tokens[index], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[index]],
        )
        .unwrap();
    };
    let top_up = |env: &mut V16CuEnv, amount: u128| {
        env.send(
            ProgInstruction::TopUpBackingBucket {
                domain: DOMAIN,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: 0,
                backing_fee_bps: 5_000,
                insurance_share_bps: 2_500,
                amount,
                expiry_slot: 100,
            },
            vec![
                AccountMeta::new(provider.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&provider],
        )
        .unwrap();
    };
    for index in 0..2 {
        deposit(&mut env, index, CAPITAL[index]);
    }
    top_up(&mut env, 1_500);
    for (asset, quantity) in [(0, 200), (1, 100)] {
        env.trade_asset_with_cu(
            asset,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            quantity * POS_SCALE as i128,
            100,
            0,
        );
    }
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for (index, asset) in [(1, 0), (0, 0), (1, 1)] {
        env.crank(
            portfolios[index],
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset, 1 - asset]),
            },
        );
    }
    assert_eq!(env.portfolio_state(portfolios[0]).capital.get(), 2_100);
    assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), 1_000);
    top_up(&mut env, 50_000);
    for index in 0..2 {
        deposit(&mut env, index, 500);
    }
    env.try_trade_asset_with_backing_fee_cap_with_cu(
        1,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        20 * POS_SCALE as i128,
        95,
        0,
        5_000,
    )
    .unwrap();
    let earnings =
        env.market_state().1.source_backing_buckets[DOMAIN as usize].utilization_fee_earnings;
    assert!(
        earnings > 1,
        "real utilization creates a divisible provider claim"
    );
    env.resolve();
    env.svm.warp_to_slot(7);
    for _ in 0..16 {
        for index in [1, 0] {
            if resolved_portfolio_is_terminal(&env, portfolios[index]) {
                continue;
            }
            env.svm.expire_blockhash();
            env.send(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(owners[index].pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[index], false),
                    AccountMeta::new(tokens[index], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            )
            .expect("funded user disposition progresses without reserve signers");
        }
        if portfolios
            .iter()
            .all(|p| resolved_portfolio_is_terminal(&env, *p))
        {
            break;
        }
    }
    for index in 0..2 {
        assert!(resolved_portfolio_is_terminal(&env, portfolios[index]));
        env.close_portfolio_with_cu(&owners[index], portfolios[index]);
    }
    let beneficiary = provider.pubkey();
    drop(provider);
    assert_ne!(env.payer.pubkey(), beneficiary);
    assert_ne!(env.payer.pubkey(), admin.pubkey());
    let keys = [
        env.market,
        env.vault,
        env.mint,
        ledger,
        tokens[0],
        tokens[1],
        tokens[2],
        portfolios[0],
        portfolios[1],
        beneficiary,
        admin.pubkey(),
    ];
    let mut paid = 0;
    let mut max_cu = 0;
    for amount in [earnings / 2, earnings - earnings / 2] {
        let before = env.market_state().1;
        let before_ledger =
            state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
        let frame = keys.map(|key| env.svm.get_account(&key));
        let ix = Instruction {
            program_id: env.program_id,
            data: ProgInstruction::WithdrawBackingBucketEarnings {
                domain: DOMAIN,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                amount,
            }
            .encode(),
            accounts: vec![
                AccountMeta::new_readonly(beneficiary, false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ledger, false),
                AccountMeta::new(tokens[2], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        };
        env.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), ix],
            Some(&env.payer.pubkey()),
            &[&env.payer],
            env.svm.latest_blockhash(),
        );
        assert_eq!(tx.message.header.num_required_signatures, 1);
        assert_eq!(tx.message.account_keys[0], env.payer.pubkey());
        let meta = env
            .svm
            .send_transaction(tx)
            .expect("earned terminal claim pays its recorded provider without a signature");
        max_cu = max_cu.max(meta.compute_units_consumed);
        assert_cu_within(
            "INV-073 public terminal earned fees",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        paid += amount;
        let after = env.market_state().1;
        assert_eq!(after.vault, before.vault - amount);
        assert_eq!(after.insurance, before.insurance);
        assert_eq!(
            after.insurance_domain_budget,
            before.insurance_domain_budget
        );
        assert_eq!(after.source_credit, before.source_credit);
        for (domain, (old, new)) in before
            .source_backing_buckets
            .iter()
            .zip(&after.source_backing_buckets)
            .enumerate()
        {
            let mut expected = *old;
            if domain == DOMAIN as usize {
                expected.utilization_fee_earnings -= amount;
            }
            assert_eq!(*new, expected);
        }
        let after_ledger =
            state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
        assert_eq!(after_ledger.authority, beneficiary.to_bytes());
        assert_eq!(
            after_ledger.total_principal_atoms,
            before_ledger.total_principal_atoms
        );
        assert_eq!(after_ledger.total_earnings_withdrawn_atoms, paid);
        assert_eq!(
            after_ledger.last_observed_bucket_earnings_atoms,
            earnings - paid
        );
        assert_eq!(u128::from(env.token_amount(tokens[2])), paid);
        assert_eq!(after.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(
            tokens
                .map(|token| env.token_amount(token))
                .iter()
                .sum::<u64>()
                + env.token_amount(env.vault),
            SUPPLY
        );
        for (key, account) in keys.into_iter().zip(frame) {
            if ![env.market, env.vault, ledger, tokens[2]].contains(&key) {
                assert_eq!(env.svm.get_account(&key), account);
            }
        }
    }
    assert_eq!(paid, earnings);
    assert_eq!(
        env.market_state().1.source_backing_buckets[DOMAIN as usize].utilization_fee_earnings,
        0
    );
    println!("INV-073 public terminal provider earnings: paid={paid} max_cu={max_cu}; principal and insurance framed");
}
