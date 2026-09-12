//! INV-014: retained permissionless activation under a successor's fee policy.
//!
//! Unlike the existing activation fee-increase refusal cell, this witness retains
//! an entire signed, previously executable Deposit + UpdateAssetLifecycle bundle.
//! Append and retired-slot reuse cross an authority handoff and lower, exact-cap,
//! or one-atom-over-cap policies. Both unauthorized controls and an authorized
//! successor's out-of-bounds charge roll back a successful SPL deposit prefix.
//! Successful activation reconciles creator value, custody, and market-0 insurance
//! independently; the fee must not become the newly installed asset's budget.
//!
//! This is six sampled histories on asset 1, not closure of counterexample 411 or
//! INV-014. It does not cover other fee tiers, arbitrary authority/lifecycle words,
//! every delayed fee route, or whole-market recreation. No program bytes are seeded.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_retained_activation_fee_cap_survives_policy_handoff_and_reuse() {
    const SUPPLY: u64 = 1_000;
    const PREFIX: u64 = 17;
    const OLD_FEE: u64 = 37;
    const SIGNED_CAP: u64 = 53;
    const ASSET: u16 = 1;
    const SLOT: u64 = 3;
    const PRICE: u64 = 101;
    const BUNDLE_CU_LIMIT: u64 = 400_000;

    let mut histories = 0;
    let mut rejections = 0;
    let mut peak_rejection_cu = 0;
    let mut peak_success_cu = 0;
    for reuse in [false, true] {
        for current_fee in [19, SIGNED_CAP, SIGNED_CAP + 1] {
            let label = format!("reuse={reuse}, fee={OLD_FEE}->{current_fee}, cap={SIGNED_CAP}");
            let mut env =
                inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market(6);
            let creator = Keypair::new();
            let successor = Keypair::new();
            for signer in [&creator, &successor] {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    system_instruction::transfer(&env.payer.pubkey(), &signer.pubkey(), 1_000_000),
                    &[],
                )
                .unwrap();
            }
            let portfolio_key = Keypair::new();
            let portfolio = portfolio_key.pubkey();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(creator.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&creator],
            )
            .unwrap();
            let source = create_ata_for_test(&mut env.svm, &env.payer, creator.pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &source,
                    &env.admin.pubkey(),
                    &[],
                    SUPPLY,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            if reuse {
                env.activate_asset(ASSET, 1, 100);
                env.svm.warp_to_slot(2);
                env.update_asset_lifecycle_as_admin_with_cu(
                    processor::ASSET_ACTION_RETIRE,
                    ASSET,
                    2,
                    0,
                );
            }
            set_test_clock(&mut env, SLOT, 100);
            env.update_market_init_fee_policy_with_cu(OLD_FEE.into());
            let (cfg_before, group_before) = env.market_state();
            assert_eq!(cfg_before.free_market_slot_count, u16::from(reuse));
            assert_eq!(group_before.vault, 0);
            assert_eq!(group_before.insurance, 0);
            assert_eq!(group_before.c_tot, 0);
            if reuse {
                assert_eq!(
                    group_before.assets[ASSET as usize].lifecycle,
                    AssetLifecycleV16::Retired
                );
            }
            let frontier = group_before.next_market_id;
            let sequence = env.portfolio_matcher_sequence(portfolio);
            let controls = env.control_sequences(0);
            let creator_bytes = creator.pubkey().to_bytes();
            let activation = ProgInstruction::UpdateAssetLifecycle {
                action: processor::ASSET_ACTION_ACTIVATE,
                asset_index: ASSET,
                market_id: frontier,
                // Permissionless activation binds generation and fees, not market authority.
                authority_epoch: 0,
                now_slot: SLOT,
                initial_price: PRICE,
                max_init_fee: SIGNED_CAP.into(),
                insurance_authority: creator_bytes,
                insurance_operator: creator_bytes,
                backing_bucket_authority: creator_bytes,
                oracle_authority: creator_bytes,
            };
            let activation_accounts = vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];
            let prefix = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(creator.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(source, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.deposit_ix(portfolio, PREFIX.into()).encode(),
            };
            // Raw transactions deliberately bypass send_tx's automatic generation rebinding.
            let bundle =
                |env: &V16CuEnv, instruction: &ProgInstruction, accounts: Vec<AccountMeta>| {
                    Transaction::new_signed_with_payer(
                        &[
                            heap_ix(),
                            cu_ix(),
                            prefix.clone(),
                            Instruction {
                                program_id: env.program_id,
                                accounts,
                                data: instruction.encode(),
                            },
                        ],
                        Some(&env.payer.pubkey()),
                        &[&env.payer, &creator],
                        env.svm.latest_blockhash(),
                    )
                };
            let retained = bundle(&env, &activation, activation_accounts.clone());
            retained.verify().unwrap();
            let signed_bytes = bincode::serialize(&retained).unwrap();
            assert!(signed_bytes.len() <= solana_sdk::packet::PACKET_DATA_SIZE);
            let frame_keys = [
                env.market,
                portfolio,
                source,
                env.vault,
                env.mint,
                creator.pubkey(),
                env.admin.pubkey(),
                successor.pubkey(),
                env.program_id,
                spl_token::ID,
                solana_sdk::compute_budget::id(),
            ];
            let frame = |env: &V16CuEnv| frame_keys.map(|key| env.svm.get_account(&key));
            let before = frame(&env);
            env.svm
                .simulate_transaction(retained.clone().into())
                .unwrap_or_else(|error| panic!("{label}: original bundle must execute: {error:?}"));
            assert_eq!(frame(&env), before, "simulation cannot consume consent");

            env.update_asset_authority_with_cu(&successor);
            let current_epoch = env.control_sequences(0).authority_epoch;
            assert_eq!(current_epoch, controls.authority_epoch + 1);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(successor.pubkey(), true),
                        AccountMeta::new(env.market, false),
                    ],
                    data: ProgInstruction::UpdateMarketInitFeePolicy {
                        min_init_fee: current_fee.into(),
                        policy_sequence: controls.market_init_fee + 1,
                        authority_epoch: current_epoch,
                    }
                    .encode(),
                },
                &[&successor],
            )
            .expect("the correctly authorized successor can change the current fee policy");
            assert_eq!(
                env.market_state().0.marketauth,
                successor.pubkey().to_bytes()
            );
            assert_eq!(
                env.market_state().0.permissionless_market_init_fee,
                current_fee.into()
            );
            assert_eq!(
                env.control_sequences(0).market_init_fee,
                controls.market_init_fee + 1
            );
            assert_eq!(env.market_state().1.next_market_id, frontier);
            assert_eq!(
                bundle(&env, &activation, activation_accounts.clone()),
                retained
            );
            assert_eq!(bincode::serialize(&retained).unwrap(), signed_bytes);

            let reject = |env: &mut V16CuEnv, transaction: Transaction, reason: &str| {
                let before = frame(env);
                let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                payer.lamports -= FeeStructure::default().lamports_per_signature
                    * u64::from(transaction.message.header.num_required_signatures);
                let error = env.svm.send_transaction(transaction).expect_err(reason);
                assert_eq!(
                    error.err,
                    TransactionError::InstructionError(
                        3,
                        InstructionError::Custom(PercolatorError::Unauthorized as u32),
                    ),
                    "{label}: {reason} must reach the suffix's authorization/fee guard"
                );
                assert!(error
                    .meta
                    .logs
                    .contains(&format!("Program {} success", spl_token::ID)));
                assert!(error
                    .meta
                    .logs
                    .contains(&format!("Program {} success", env.program_id)));
                assert_eq!(frame(env), before, "{label}: {reason}, complete rollback");
                assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                assert_eq!(env.portfolio_matcher_sequence(portfolio), sequence);
                assert_cu_within(reason, error.meta.compute_units_consumed, BUNDLE_CU_LIMIT);
                error.meta.compute_units_consumed
            };
            let unauthorized = bundle(
                &env,
                &ProgInstruction::UpdateMarketInitFeePolicy {
                    min_init_fee: 1,
                    policy_sequence: controls.market_init_fee + 2,
                    authority_epoch: current_epoch,
                },
                vec![
                    AccountMeta::new(creator.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
            );
            peak_rejection_cu = peak_rejection_cu.max(reject(
                &mut env,
                unauthorized,
                "an unauthorized caller cannot replace the successor's fee",
            ));
            rejections += 1;

            let accepted_limit = if current_fee > SIGNED_CAP {
                peak_rejection_cu = peak_rejection_cu.max(reject(
                    &mut env,
                    retained.clone(),
                    "a current policy one atom above signed consent cannot charge the creator",
                ));
                rejections += 1;
                current_fee
            } else {
                SIGNED_CAP
            };
            let accepted = if current_fee > SIGNED_CAP {
                let mut fresh = activation.clone();
                let ProgInstruction::UpdateAssetLifecycle { max_init_fee, .. } = &mut fresh else {
                    unreachable!()
                };
                *max_init_fee = accepted_limit.into();
                bundle(&env, &fresh, activation_accounts)
            } else {
                retained
            };
            accepted.verify().unwrap();
            let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
            payer.lamports -= FeeStructure::default().lamports_per_signature
                * u64::from(accepted.message.header.num_required_signatures);
            let unchanged_keys = [
                env.mint,
                creator.pubkey(),
                env.admin.pubkey(),
                successor.pubkey(),
            ];
            let unchanged_before = unchanged_keys.map(|key| env.svm.get_account(&key));
            let success = env.svm.send_transaction(accepted).unwrap_or_else(|error| {
                panic!("{label}: bounded consent must execute without refreshing the prefix: {error:?}")
            });
            assert_cu_within(
                "bounded activation fee bundle",
                success.compute_units_consumed,
                BUNDLE_CU_LIMIT,
            );
            peak_success_cu = peak_success_cu.max(success.compute_units_consumed);
            assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
            assert_eq!(
                unchanged_keys.map(|key| env.svm.get_account(&key)),
                unchanged_before
            );

            let (cfg, group) = env.market_state();
            let account = env.portfolio_state(portfolio);
            assert_eq!(env.portfolio_matcher_sequence(portfolio), sequence + 1);
            assert_eq!(account.capital.get(), u128::from(PREFIX));
            assert_eq!(account.pnl.get(), 0);
            assert_eq!(env.token_amount(source), SUPPLY - PREFIX - current_fee);
            assert_eq!(env.token_amount(env.vault), PREFIX + current_fee);
            assert_eq!(group.c_tot, PREFIX.into());
            assert_eq!(group.insurance, current_fee.into());
            assert_eq!(group.vault, u128::from(PREFIX + current_fee));
            assert_eq!(group.vault, group.c_tot + group.insurance);
            assert_eq!(
                group.insurance_domain_budget[0],
                u128::from(current_fee / 2)
            );
            assert_eq!(
                group.insurance_domain_budget[1],
                u128::from(current_fee - current_fee / 2)
            );
            assert!(group.insurance_domain_budget[2..]
                .iter()
                .all(|budget| *budget == 0));
            let creator_value = u128::from(env.token_amount(source)) + account.capital.get();
            assert_eq!(u128::from(SUPPLY) - creator_value, current_fee.into());
            assert!(u128::from(SUPPLY) - creator_value <= accepted_limit.into());
            assert_eq!(
                Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                    .unwrap()
                    .supply,
                SUPPLY
            );
            assert_eq!(cfg.permissionless_market_init_fee, current_fee.into());
            assert_eq!(cfg.marketauth, successor.pubkey().to_bytes());
            assert_eq!(cfg.free_market_slot_count, 0);
            assert_eq!(group.next_market_id, frontier + 1);
            assert_eq!(
                group.asset_activation_count,
                group_before.asset_activation_count + 1
            );
            assert_eq!(group.config.max_market_slots, 2);
            let asset = &group.assets[ASSET as usize];
            assert_eq!(asset.market_id, frontier);
            assert_eq!(asset.lifecycle, AssetLifecycleV16::Active);
            assert_eq!(asset.effective_price, PRICE);
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                ASSET as usize,
            )
            .unwrap();
            assert_eq!(profile.asset_admin, creator_bytes);
            assert_eq!(profile.insurance_authority, creator_bytes);
            assert_eq!(profile.insurance_operator, creator_bytes);
            assert_eq!(profile.backing_bucket_authority, creator_bytes);
            assert_eq!(profile.oracle_authority, creator_bytes);
            histories += 1;
            eprintln!(
                "INV-014 {label}: fee {current_fee} atoms, bundle CU {}",
                success.compute_units_consumed
            );
        }
    }
    assert_eq!((histories, rejections), (6, 8));
    eprintln!(
        "INV-014 retained activation fee: {histories} histories, {rejections} exact rollbacks, \
         peak rejected/successful bundle CU {peak_rejection_cu}/{peak_success_cu}"
    );
}
