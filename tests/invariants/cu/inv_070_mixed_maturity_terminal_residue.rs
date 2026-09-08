//! Mixed terminal stock: live provider principal, lapsed principal, funded insurance, and
//! unbooked SPL surplus have different dispositions even after the last user has been paid.

use super::*;

#[test]
fn v16_program_mixed_maturity_terminal_residue_preserves_partition_and_close_retry() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const CAPITAL: u64 = 1_009;
    const LIVE: u64 = 401;
    const LAPSED: u64 = 307;
    const INSURANCE: u64 = 203;
    const SURPLUS: u64 = 17;
    const SUPPLY: u64 = CAPITAL + LIVE + LAPSED + INSURANCE + SURPLUS;
    const EXPIRY: u64 = 5;

    for insurance_first in [false, true] {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                ..V16CuMarketParams::default()
            },
        );
        let admin = env.admin.insecure_clone();
        let owner = Keypair::new();
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let portfolio_key = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio_key,
            env.portfolio_account_len,
            env.program_id,
        );
        let portfolio = portfolio_key.pubkey();
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[&owner],
        )
        .unwrap();
        env.portfolios.push(portfolio);
        let user_token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
        let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        for (token, amount) in [(user_token, CAPITAL), (admin_token, SUPPLY - CAPITAL)] {
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
        env.send(
            env.deposit_ix(portfolio, CAPITAL.into()),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(user_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .unwrap();
        env.svm.warp_to_slot(1);
        for (domain, amount, expiry_slot) in [(0, LIVE, 100), (3, LAPSED, EXPIRY)] {
            env.top_up_backing_bucket_from_admin_token_with_cu(
                admin_token,
                domain,
                amount.into(),
                expiry_slot,
            );
        }
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                domain: 2,
                market_id: env.asset_market_id(1),
                authority_epoch: env.control_sequences(1).authority_epoch,
                intent_id: 0,
                amount: INSURANCE.into(),
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(admin_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .unwrap();
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::transfer(
                &spl_token::ID,
                &admin_token,
                &env.vault,
                &admin.pubkey(),
                &[],
                SURPLUS,
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();

        let lamports = |env: &V16CuEnv, key| env.svm.get_account(&key).map_or(0, |a| a.lamports);
        let market_rent = lamports(&env, env.market);
        let portfolio_rent = lamports(&env, portfolio);
        let vault_rent = lamports(&env, env.vault);
        let owner_lamports = lamports(&env, owner.pubkey());
        let admin_lamports = lamports(&env, admin.pubkey());
        let token_rents = [lamports(&env, user_token), lamports(&env, admin_token)];
        let tombstone_rent = env
            .svm
            .get_sysvar::<solana_sdk::rent::Rent>()
            .minimum_balance(percolator_prog::constants::HEADER_LEN);
        let mint_before = env.svm.get_account(&env.mint).unwrap();

        // The oracle is driven by fixed inputs and completed public actions, not engine deltas.
        let stock = |env: &V16CuEnv,
                     user_paid: bool,
                     materialized: bool,
                     paid: [bool; 2],
                     expired: bool,
                     closed: bool| {
            let capital = if user_paid { 0 } else { CAPITAL };
            let live = if paid[0] { 0 } else { LIVE };
            let insurance = if paid[1] { 0 } else { INSURANCE };
            let paid_admin = LIVE - live + INSURANCE - insurance;
            let vault = if closed {
                0
            } else {
                capital + live + insurance + LAPSED + SURPLUS
            };
            let amounts = [
                CAPITAL - capital,
                paid_admin + if closed { SURPLUS } else { 0 },
                vault,
            ];
            for (index, (key, wallet)) in [
                (user_token, owner.pubkey()),
                (admin_token, admin.pubkey()),
                (env.vault, env.vault_authority),
            ]
            .into_iter()
            .enumerate()
            {
                if closed && index == 2 {
                    if let Some(account) = env.svm.get_account(&key) {
                        assert_eq!(account.lamports, 0);
                        assert!(account.data.iter().all(|byte| *byte == 0));
                    }
                    continue;
                }
                let account = env.svm.get_account(&key).unwrap();
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(account.owner, spl_token::ID);
                assert_eq!(token.mint, env.mint);
                assert_eq!(token.owner, wallet);
                assert_eq!(token.state, AccountState::Initialized);
                assert_eq!(token.delegate, COption::None);
                assert_eq!(token.close_authority, COption::None);
                assert_eq!(token.is_native, COption::None);
                assert_eq!(token.amount, amounts[index]);
                assert_eq!(
                    account.lamports,
                    if index == 2 {
                        vault_rent
                    } else {
                        token_rents[index]
                    }
                );
            }
            let mut expected_mint = Mint::unpack(&mint_before.data).unwrap();
            assert_eq!(expected_mint.supply, SUPPLY);
            assert_eq!(expected_mint.mint_authority, COption::None);
            assert_eq!(expected_mint.freeze_authority, COption::None);
            expected_mint.supply -= if closed { LAPSED } else { 0 };
            let mint = env.svm.get_account(&env.mint).unwrap();
            assert_eq!(Mint::unpack(&mint.data).unwrap(), expected_mint);
            assert_eq!(mint.lamports, mint_before.lamports);
            assert_eq!(mint.owner, mint_before.owner);
            assert_eq!(amounts.iter().sum::<u64>(), expected_mint.supply);
            assert_eq!(
                env.vault,
                canonical_vault_ata(env.vault_authority, env.mint)
            );
            assert_eq!(lamports(env, owner.pubkey()), owner_lamports);
            assert_eq!(
                lamports(env, admin.pubkey()),
                admin_lamports
                    + if closed {
                        market_rent + portfolio_rent + vault_rent - tombstone_rent
                    } else {
                        0
                    }
            );
            assert_eq!(
                lamports(env, env.market),
                if closed {
                    tombstone_rent
                } else {
                    market_rent + if materialized { 0 } else { portfolio_rent }
                }
            );
            if materialized {
                assert_eq!(lamports(env, portfolio), portfolio_rent);
                assert_eq!(env.portfolio_state(portfolio).capital.get(), capital.into());
            } else if let Some(account) = env.svm.get_account(&portfolio) {
                assert_eq!(account.lamports, 0);
                assert!(account.data.is_empty());
            }
            if closed {
                assert_eq!((capital, live, insurance), (0, 0, 0));
                assert!(expired && !materialized);
                assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
            } else {
                let group = env.market_state().1;
                assert_eq!(group.c_tot, capital.into());
                assert_eq!(group.vault, u128::from(capital + live + insurance + LAPSED));
                assert_eq!(group.insurance, insurance.into());
                assert_eq!(group.materialized_portfolio_count, u64::from(materialized));
                assert_eq!(
                    &group.insurance_domain_budget[..4],
                    &[0, 0, insurance.into(), 0]
                );
                assert!(group.insurance_domain_spent.iter().all(|value| *value == 0));
                for (domain, amount) in [(0, live), (3, if expired { 0 } else { LAPSED })] {
                    assert_eq!(
                        group.source_backing_buckets[domain].fresh_unliened_backing_num,
                        u128::from(amount) * BOUND_SCALE
                    );
                    assert_eq!(
                        group.source_credit[domain].fresh_reserved_backing_num,
                        u128::from(amount) * BOUND_SCALE
                    );
                }
                assert!(group
                    .assets
                    .iter()
                    .all(|asset| asset.oi_eff_long_q == 0 && asset.oi_eff_short_q == 0));
            }
        };
        stock(&env, false, true, [false; 2], false, false);
        env.resolve();
        env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(user_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .unwrap();
        stock(&env, true, true, [false; 2], false, false);
        env.close_portfolio_with_cu(&owner, portfolio);
        stock(&env, true, false, [false; 2], false, false);
        env.svm.warp_to_slot(EXPIRY);
        let group = env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.source_backing_buckets[0].expiry_slot, 100);
        assert_eq!(group.source_backing_buckets[3].expiry_slot, EXPIRY);
        assert_eq!(
            group.source_backing_buckets[3].status,
            BackingBucketStatusV16::Fresh
        );

        let withdrawal_accounts = vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(admin_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ];
        let wrap = |ix: ProgInstruction, accounts| Instruction {
            program_id: env.program_id,
            accounts,
            data: ix.encode(),
        };
        let withdrawals = [
            wrap(
                ProgInstruction::WithdrawBackingBucket {
                    domain: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                    amount: LIVE.into(),
                },
                withdrawal_accounts.clone(),
            ),
            wrap(
                env.withdraw_insurance_asset_instruction(admin.pubkey(), 1, INSURANCE.into()),
                withdrawal_accounts,
            ),
        ];
        let close = wrap(
            ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(admin_token, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
        );
        let frame_keys = [
            env.market,
            env.vault,
            env.mint,
            portfolio,
            user_token,
            admin_token,
            admin.pubkey(),
            owner.pubkey(),
            env.payer.pubkey(),
        ];
        let land =
            |env: &mut V16CuEnv, instructions: &[Instruction], rejection: Option<(u8, usize)>| {
                env.svm.expire_blockhash();
                let mut batch = vec![
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(500_000),
                ];
                batch.extend_from_slice(instructions);
                let tx = Transaction::new_signed_with_payer(
                    &batch,
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &admin],
                    env.svm.latest_blockhash(),
                );
                let fee = u64::from(tx.message.header.num_required_signatures)
                    * FeeStructure::default().lamports_per_signature;
                let before = frame_keys.map(|key| env.svm.get_account(&key));
                let payer_before = lamports(env, env.payer.pubkey());
                let result = env.svm.send_transaction(tx);
                assert_eq!(lamports(env, env.payer.pubkey()), payer_before - fee);
                let meta = if let Some((index, transfers)) = rejection {
                    let failure = result.expect_err("live residue must block CloseSlab");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            index,
                            InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                        )
                    );
                    assert_eq!(
                        failure
                            .meta
                            .logs
                            .iter()
                            .filter(|line| **line == format!("Program {} success", spl_token::ID))
                            .count(),
                        transfers
                    );
                    for (key, mut account) in frame_keys.into_iter().zip(before) {
                        if key == env.payer.pubkey() {
                            account.as_mut().unwrap().lamports -= fee;
                        }
                        assert_eq!(
                            env.svm.get_account(&key),
                            account,
                            "exact rollback for {key}"
                        );
                    }
                    failure.meta
                } else {
                    result.expect("public terminal continuation")
                };
                assert_cu_within(
                    "mixed-maturity terminal step",
                    meta.compute_units_consumed,
                    500_000,
                );
                meta.compute_units_consumed
            };

        let early_cu = land(&mut env, &[close.clone()], Some((2, 0)));
        // A real insurance transfer precedes the live-backing close rejection; it cannot stick.
        let rollback_cu = land(
            &mut env,
            &[withdrawals[1].clone(), close.clone()],
            Some((3, 1)),
        );
        stock(&env, true, false, [false; 2], false, false);
        let mut paid = [false; 2];
        let mut expired = false;
        let mut expiry_cu = 0;
        let mut withdrawal_cu = [0; 2];
        for (step, role) in [usize::from(insurance_first), usize::from(!insurance_first)]
            .into_iter()
            .enumerate()
        {
            withdrawal_cu[role] = land(&mut env, &[withdrawals[role].clone()], None);
            paid[role] = true;
            stock(&env, true, false, paid, expired, false);
            if role == 0 {
                let custody_before = [env.vault, env.mint, user_token, admin_token, admin.pubkey()]
                    .map(|key| env.svm.get_account(&key));
                expiry_cu = land(&mut env, &[close.clone()], None);
                expired = true;
                assert_eq!(
                    [env.vault, env.mint, user_token, admin_token, admin.pubkey()]
                        .map(|key| env.svm.get_account(&key)),
                    custody_before,
                    "expiry reclassifies backing without burning, sweeping, or refunding custody"
                );
                assert_eq!(
                    env.market_state().1.source_backing_buckets[3].status,
                    BackingBucketStatusV16::Expired
                );
                stock(&env, true, false, paid, expired, false);
            }
            if step == 0 {
                land(&mut env, &[close.clone()], Some((2, 0)));
                stock(&env, true, false, paid, expired, false);
            }
        }
        let close_cu = land(&mut env, &[close], None);
        stock(&env, true, false, paid, true, true);
        println!("INV-070 mixed maturity insurance_first={insurance_first}: early={early_cu}, rollback={rollback_cu}, withdrawals={withdrawal_cu:?}, expiry={expiry_cu}, close={close_cu} CU; {SUPPLY} = {CAPITAL} user + {LIVE} provider + {INSURANCE} insurance + {LAPSED} burn + {SURPLUS} sweep");
    }
}
