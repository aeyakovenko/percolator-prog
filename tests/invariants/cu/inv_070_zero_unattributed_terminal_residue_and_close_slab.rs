//! INV-070 - Zero unattributed terminal residue and CloseSlab.
//!
//! `CloseSlab` is the final market-account reclaim path. It may run only after
//! all user claimable value, live encumbrances, unresolved loss, pending
//! receipts, and unexplained accounting have been reduced to zero or explicitly
//! classified as final protocol dust. These tests exercise the deployed wrapper
//! through LiteSVM public instructions and assert the two security-relevant
//! terminal properties:
//!
//! * a live or resolved-but-funded market rejects atomically and remains
//!   recoverable by the user wind-down route; and
//! * after accounting is fully drained, final raw vault dust can be swept only
//!   to the current authority's correct quote-token destination, with wrong
//!   destinations rejected before the vault or market slab changes.
//! `v16_program_close_slab_refunds_exact_vault_and_market_excess_rent_after_normal_exit` adds the
//! corresponding lamport-side postcondition for a normal public deposit, withdrawal, portfolio
//! close, and market resolution: the typed market tombstone retains exactly its canonical rent,
//! the SPL vault is closed, and every other lamport reaches the current market authority.
//! `v16_program_recovery_force_close_reaches_zero_residue_and_close_slab` composes the final path
//! with a publicly reached Recovery episode and permissionless force-close. It proves terminal
//! normalization does not rely on a market that stayed Active throughout its lifetime.
//! `v16_program_terminal_stock_and_close_slab_composition_is_source_complete` closes the current
//! surface by binding the exact engine pin's terminal-claim, reservation, recredit, retirement,
//! and bounded-scan proofs to the existing public lifecycle and maximum-shape witnesses. It also
//! locks the wrapper ordering: canonical vault and destination validation precedes the engine
//! transition, and no SPL burn, transfer, close, or market tombstone write can occur before
//! `ReadyToClose`.

use super::*;

#[path = "inv_070_mixed_maturity_terminal_residue.rs"]
mod mixed_maturity;

#[path = "inv_070_terminal_prefix_reuse.rs"]
mod terminal_prefix_reuse;

#[path = "inv_070_prefunded_quote_custody.rs"]
mod prefunded_quote_custody;

#[path = "inv_070_frozen_destination_exit.rs"]
mod frozen_destination_exit;

#[path = "inv_070_native_pnl_sync_retry.rs"]
mod native_pnl_sync_retry;

#[path = "inv_070_terminal_native_reclassification.rs"]
mod terminal_native_reclassification;

#[path = "inv_070_generated_prefix_actionability.rs"]
mod generated_prefix_actionability;

#[test]
fn v16_program_terminal_scan_reconciles_external_surplus_arriving_after_cached_prefix() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;

    const SLOTS: usize = 257;
    const BOOKED: u64 = 17;
    const SURPLUS: u64 = 19;
    const EXPIRY: u64 = 400;
    const CU_LIMIT: u64 = 500_000;

    for transfer_after_scan in [false, true] {
        let mut env =
            inv018_public_spl_market_with_capacity(0, V16CuMarketParams::default(), SLOTS);
        let admin = env.admin.insecure_clone();
        let donor = Keypair::new();
        env.svm.airdrop(&donor.pubkey(), 1_000_000_000).unwrap();
        let destination = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        let source = create_ata_for_test(&mut env.svm, &env.payer, donor.pubkey(), env.mint);
        for asset in 1..SLOTS {
            env.activate_asset(asset as u16, asset as u64 + 1, 100);
        }
        for (token, amount) in [(destination, BOOKED), (source, SURPLUS)] {
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
        env.svm.warp_to_slot(300);
        env.top_up_backing_bucket_from_admin_token_with_cu(destination, 0, BOOKED.into(), EXPIRY);
        env.svm.warp_to_slot(EXPIRY - 1);
        env.resolve();

        let close = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
            data: ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        };
        let send_close = |env: &mut V16CuEnv| {
            env.svm.expire_blockhash();
            let cu = send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![heap_ix(), cu_ix(), close.clone()],
                &[&admin],
            )
            .expect("public bounded terminal scan");
            assert_cu_within("INV-070 external surplus scan", cu, CU_LIMIT);
            cu
        };
        let custody_keys = [
            env.vault,
            env.mint,
            destination,
            source,
            admin.pubkey(),
            donor.pubkey(),
        ];
        let custody = |env: &V16CuEnv| custody_keys.map(|key| env.svm.get_account(&key));
        let stock = |env: &V16CuEnv, transferred: bool, cursor: u128| {
            let (cfg, group) = env.market_state();
            assert_eq!(cfg.terminal_slab_scan_progress, cursor);
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!((group.c_tot, group.pnl_pos_tot, group.insurance), (0, 0, 0));
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(group.backing_provider_earnings_total, 0);
            assert_eq!(group.vault, BOOKED.into());
            assert_eq!(
                env.token_amount(env.vault),
                BOOKED + if transferred { SURPLUS } else { 0 }
            );
            assert_eq!(
                env.token_amount(source),
                if transferred { 0 } else { SURPLUS }
            );
            assert_eq!(env.token_amount(destination), 0);
            assert!(group.source_backing_buckets.iter().all(|bucket| {
                bucket.status != BackingBucketStatusV16::Fresh
                    && bucket.fresh_unliened_backing_num == 0
                    && bucket.valid_liened_backing_num == 0
            }));
            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
            assert_eq!(mint.supply, BOOKED + SURPLUS);
            assert_eq!(mint.mint_authority, COption::None);
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "INV-070 external surplus scan",
                &group,
                &[],
            )
            .unwrap();
        };
        let transfer = |env: &mut V16CuEnv| {
            let market = env.svm.get_account(&env.market);
            let mint = env.svm.get_account(&env.mint);
            let recipient = env.svm.get_account(&destination);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &source,
                    &env.vault,
                    &donor.pubkey(),
                    &[],
                    SURPLUS,
                )
                .unwrap(),
                &[&donor],
            )
            .unwrap();
            assert_eq!(
                env.svm.get_account(&env.market),
                market,
                "external transfer cannot invalidate the stored prefix by writing the market"
            );
            assert_eq!(env.svm.get_account(&env.mint), mint);
            assert_eq!(env.svm.get_account(&destination), recipient);
        };

        // Expiry creates booked residue without moving tokens; the next call scans 256 slots.
        env.svm.warp_to_slot(EXPIRY);
        let before_expiry = custody(&env);
        let mut peak_cu = send_close(&mut env);
        assert_eq!(custody(&env), before_expiry);
        stock(&env, false, 0);
        if !transfer_after_scan {
            transfer(&mut env);
        }
        let before_scan = custody(&env);
        peak_cu = peak_cu.max(send_close(&mut env));
        assert_eq!(custody(&env), before_scan);
        stock(&env, !transfer_after_scan, 256);

        env.svm.expire_blockhash();
        let retained = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), close.clone()],
            Some(&env.payer.pubkey()),
            &[&env.payer, &admin],
            env.svm.latest_blockhash(),
        );
        let retained_bytes = bincode::serialize(&retained).unwrap();
        let keys = retained.message.account_keys.clone();
        let frame = |env: &V16CuEnv| {
            keys.iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>()
        };
        let before_preview = frame(&env);
        let preview = env
            .svm
            .simulate_transaction(retained.clone().into())
            .unwrap();
        assert_eq!(frame(&env), before_preview);
        let token_success = format!("Program {} success", spl_token::ID);
        assert_eq!(
            preview
                .logs
                .iter()
                .filter(|log| **log == token_success)
                .count(),
            if transfer_after_scan { 2 } else { 3 },
            "preview burns and closes; only existing raw surplus enables a transfer"
        );

        let scanned_market = env.svm.get_account(&env.market).unwrap();
        if transfer_after_scan {
            transfer(&mut env);
        }
        env.svm.warp_to_slot(EXPIRY + 1);
        assert_eq!(env.svm.get_account(&env.market), Some(scanned_market));
        stock(&env, true, 256);
        assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);

        let market_before = env.svm.get_account(&env.market).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
        let mut expected_destination = env.svm.get_account(&destination).unwrap();
        let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
        let donor_before = env.svm.get_account(&donor.pubkey());
        let source_before = env.svm.get_account(&source);
        let mut expected_payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
        expected_payer.lamports -= u64::from(retained.message.header.num_required_signatures)
            * solana_sdk::fee::FeeStructure::default().lamports_per_signature;
        let result = env
            .svm
            .send_transaction(retained)
            .expect("final close must reconcile current external custody after the persisted scan");
        peak_cu = peak_cu
            .max(result.compute_units_consumed)
            .max(preview.compute_units_consumed);
        assert_eq!(result.logs.iter().filter(|log| **log == token_success).count(), 3, "final close must burn booked residue, sweep newly actionable surplus, and close the vault");

        let mut token = TokenAccount::unpack(&expected_destination.data).unwrap();
        token.amount = SURPLUS;
        TokenAccount::pack(token, &mut expected_destination.data).unwrap();
        let mut mint = Mint::unpack(&expected_mint.data).unwrap();
        mint.supply = SURPLUS;
        Mint::pack(mint, &mut expected_mint.data).unwrap();
        assert_eq!(
            env.svm.get_account(&destination),
            Some(expected_destination)
        );
        assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
        assert_eq!(env.svm.get_account(&source), source_before);
        assert_eq!(env.svm.get_account(&donor.pubkey()), donor_before);
        assert_eq!(
            env.svm.get_account(&env.payer.pubkey()),
            Some(expected_payer)
        );
        let tombstone = env.svm.get_account(&env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(
            tombstone.lamports,
            env.svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
        );
        expected_admin.lamports +=
            market_before.lamports + vault_before.lamports - tombstone.lamports;
        assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
        assert!(env.svm.get_account(&env.vault).is_none_or(
            |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
        ));
        assert_cu_within("INV-070 external surplus history peak", peak_cu, CU_LIMIT);
        eprintln!("INV-070 transfer_after_scan={transfer_after_scan}: cursor=0->256->tombstone, burn={BOOKED}, sweep={SURPLUS}, peak={peak_cu} CU");
    }
}

#[test]
fn v16_program_retained_terminal_withdrawal_revalidates_expiry_after_scan_and_partial_payout() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const EARLIER: u64 = 17;
    const LATER: u64 = 31;
    const PAID: u64 = 13;
    const EXPIRY: u64 = 20;
    const SUPPLY: u64 = EARLIER + LATER;
    const CU_LIMIT: u64 = 150_000;

    for landing_slot in [EXPIRY - 1, EXPIRY] {
        let mut env = inv018_public_spl_market_with_capacity(0, V16CuMarketParams::default(), 2);
        let admin = env.admin.insecure_clone();
        env.activate_asset(1, 1, 100);
        let provider = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &provider,
                &admin.pubkey(),
                &[],
                SUPPLY,
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();
        env.svm.warp_to_slot(1);
        env.top_up_backing_bucket_from_admin_token_with_cu(provider, 0, EARLIER.into(), 10);
        env.top_up_backing_bucket_from_admin_token_with_cu(provider, 3, LATER.into(), EXPIRY);
        env.svm.warp_to_slot(9);
        env.resolve();

        let close = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(provider, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
            data: ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        };
        let send_close = |env: &mut V16CuEnv| {
            env.svm.expire_blockhash();
            let cu = send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![heap_ix(), cu_ix(), close.clone()],
                &[&admin],
            )
            .unwrap();
            assert_cu_within("INV-070 retained terminal close", cu, CU_LIMIT);
            cu
        };
        let stock = |env: &V16CuEnv, fresh: [u64; 2], paid: u64, cursor: u128| {
            let (cfg, group) = env.market_state();
            let market = env.svm.get_account(&env.market).unwrap();
            assert_eq!(cfg.terminal_slab_scan_progress, cursor);
            assert_eq!(group.mode, MarketModeV16::Resolved);
            assert_eq!(
                (
                    group.materialized_portfolio_count,
                    group.c_tot,
                    group.insurance
                ),
                (0, 0, 0)
            );
            assert_eq!(group.vault, u128::from(SUPPLY - paid));
            assert_eq!(env.token_amount(env.vault), SUPPLY - paid);
            assert_eq!(env.token_amount(provider), paid);
            assert_eq!(
                Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                    .unwrap()
                    .supply,
                SUPPLY
            );
            for domain in 0..4 {
                let atoms = match domain {
                    0 => fresh[0],
                    3 => fresh[1],
                    _ => 0,
                };
                let bucket = group.source_backing_buckets[domain];
                let source = group.source_credit[domain];
                assert_eq!(
                    bucket.fresh_unliened_backing_num,
                    u128::from(atoms) * BOUND_SCALE
                );
                assert_eq!(
                    source.fresh_reserved_backing_num,
                    u128::from(atoms) * BOUND_SCALE
                );
                assert_eq!(
                    (
                        bucket.utilization_fee_earnings,
                        source.positive_claim_bound_num,
                        source.valid_liened_backing_num
                    ),
                    (0, 0, 0)
                );
            }
            crate::support::fuzz_model::assert_market_stock_census(
                "INV-070 retained terminal withdrawal",
                &group,
                &market.data,
                &[],
                u128::from(SUPPLY - paid),
            )
            .unwrap();
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "INV-070 retained terminal withdrawal",
                &group,
                &[],
            )
            .unwrap();
        };
        stock(&env, [EARLIER, LATER], 0, 0);

        // Normalize the earlier stock, commit a scan past it, then pay only part of the live bucket.
        env.svm.warp_to_slot(10);
        let mut peak_cu = send_close(&mut env);
        stock(&env, [0, LATER], 0, 0);
        assert_eq!(
            env.market_state().1.source_backing_buckets[0].status,
            BackingBucketStatusV16::Expired
        );
        peak_cu = peak_cu.max(send_close(&mut env));
        stock(&env, [0, LATER], 0, 1);
        peak_cu = peak_cu.max(env.withdraw_backing_bucket_to_admin_token_with_cu(
            provider,
            3,
            PAID.into(),
        ));
        stock(&env, [0, LATER - PAID], PAID, 1);
        let slot_start =
            MARKET_GROUP_OFF + std::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
        let slot_end =
            slot_start + std::mem::size_of::<percolator::Market<state::AssetOracleStorageV16>>();
        let earlier_slot =
            env.svm.get_account(&env.market).unwrap().data[slot_start..slot_end].to_vec();
        let prefix_market = env.svm.get_account(&env.market).unwrap();
        assert_eq!(env.market_state().1.current_slot, 10);

        let withdraw = Instruction {
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
                domain: 3,
                market_id: env.asset_market_id(1),
                authority_epoch: env.withdrawal_authority_epoch(admin.pubkey(), 1, false),
                amount: (LATER - PAID).into(),
            }
            .encode(),
        };
        env.svm.warp_to_slot(EXPIRY - 1);
        env.svm.expire_blockhash();
        let retained = Transaction::new_signed_with_payer(
            &[heap_ix(), cu_ix(), withdraw, close.clone()],
            Some(&env.payer.pubkey()),
            &[&env.payer, &admin],
            env.svm.latest_blockhash(),
        );
        let retained_bytes = bincode::serialize(&retained).unwrap();
        let keys = retained.message.account_keys.clone();
        let frame = |env: &V16CuEnv| {
            keys.iter()
                .map(|key| env.svm.get_account(key))
                .collect::<Vec<_>>()
        };
        let before = frame(&env);
        let preview = env
            .svm
            .simulate_transaction(retained.clone().into())
            .expect("both withdrawal and final slab close are actionable before expiry");
        assert_eq!(frame(&env), before);
        let success_log = format!("Program {} success", env.program_id);
        assert_eq!(
            preview
                .logs
                .iter()
                .filter(|log| **log == success_log)
                .count(),
            2
        );
        peak_cu = peak_cu.max(preview.compute_units_consumed);

        // Only authenticated Clock changes between preview and delivery; signatures stay intact.
        env.svm.warp_to_slot(landing_slot);
        assert_eq!(env.svm.get_account(&env.market), Some(prefix_market));
        assert_eq!(
            env.market_state().1.source_backing_buckets[3].status,
            BackingBucketStatusV16::Fresh
        );
        assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
        let market_before_close = env.svm.get_account(&env.market).unwrap();
        let vault_before_close = env.svm.get_account(&env.vault).unwrap();
        let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
        let mut expected_provider = env.svm.get_account(&provider).unwrap();
        let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
        let fee = u64::from(retained.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
        let payer_index = keys
            .iter()
            .position(|key| *key == env.payer.pubkey())
            .unwrap();
        let mut expected_frame = before;
        expected_frame[payer_index].as_mut().unwrap().lamports -= fee;
        if landing_slot == EXPIRY {
            let error = env
                .svm
                .send_transaction(retained)
                .expect_err("elapsed backing cannot pay a retained principal withdrawal");
            assert_eq!(
                error.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )
            );
            assert_eq!(
                error
                    .meta
                    .logs
                    .iter()
                    .filter(|log| **log == success_log)
                    .count(),
                0
            );
            assert_eq!(
                frame(&env),
                expected_frame,
                "exact rollback except the signature fee"
            );
            peak_cu = peak_cu.max(error.meta.compute_units_consumed);
            stock(&env, [0, LATER - PAID], PAID, 1);
            assert_eq!(env.market_state().1.current_slot, 10);

            let provider_before_expiry = env.svm.get_account(&provider);
            let vault_before_expiry = env.svm.get_account(&env.vault);
            let mint_before_expiry = env.svm.get_account(&env.mint);
            peak_cu = peak_cu.max(send_close(&mut env));
            stock(&env, [0, 0], PAID, 1);
            assert_eq!(env.market_state().1.current_slot, EXPIRY);
            assert_eq!(
                env.market_state().1.source_backing_buckets[3].status,
                BackingBucketStatusV16::Expired
            );
            assert_eq!(env.svm.get_account(&provider), provider_before_expiry);
            assert_eq!(env.svm.get_account(&env.vault), vault_before_expiry);
            assert_eq!(env.svm.get_account(&env.mint), mint_before_expiry);
            assert_eq!(
                &env.svm.get_account(&env.market).unwrap().data[slot_start..slot_end],
                earlier_slot.as_slice()
            );
            peak_cu = peak_cu.max(send_close(&mut env));
        } else {
            let result = env
                .svm
                .send_transaction(retained)
                .expect("unchanged pre-expiry delivery");
            peak_cu = peak_cu.max(result.compute_units_consumed);
            assert_eq!(
                result
                    .logs
                    .iter()
                    .filter(|log| **log == success_log)
                    .count(),
                2
            );
        }

        let paid = if landing_slot < EXPIRY { LATER } else { PAID };
        let burned = SUPPLY - paid;
        let mut provider_token = TokenAccount::unpack(&expected_provider.data).unwrap();
        provider_token.amount = paid;
        TokenAccount::pack(provider_token, &mut expected_provider.data).unwrap();
        let mut mint = Mint::unpack(&expected_mint.data).unwrap();
        mint.supply = SUPPLY - burned;
        Mint::pack(mint, &mut expected_mint.data).unwrap();
        assert_eq!(env.svm.get_account(&provider), Some(expected_provider));
        assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
        let tombstone = env.svm.get_account(&env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(
            tombstone.lamports,
            env.svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
        );
        expected_admin.lamports +=
            market_before_close.lamports + vault_before_close.lamports - tombstone.lamports;
        assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
        assert!(env.svm.get_account(&env.vault).is_none_or(
            |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
        ));
        assert_cu_within("INV-070 retained terminal history peak", peak_cu, CU_LIMIT);
        eprintln!("INV-070 landing={landing_slot}: prior payout={PAID}, final provider={paid}, burn={burned}, peak={peak_cu} CU");
    }
}

#[test]
fn v16_program_native_quote_terminal_surplus_sync_has_exact_token_and_lamport_disposition() {
    use super::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;
    use crate::support::fuzz_model::{
        assert_market_stock_census, assert_reservation_encumbrance_census,
    };

    const CAPITAL: u64 = 1_009;
    const SURPLUS: u64 = 17;
    const UNSYNCED: u64 = 19;
    const STEP_CU_LIMIT: u64 = 150_000;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Stage {
        Funded,
        Deposited,
        Resolved,
        Paid,
        Dematerialized,
        Synced,
        Closed,
    }

    for sync_before_close in [false, true] {
        let mut env = inv081_public_native_market();
        let admin = env.admin.insecure_clone();
        let owner = Keypair::new();
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let mut peak_cu = 0;
        let mut check_cu = |label, cu| {
            assert_cu_within(label, cu, STEP_CU_LIMIT);
            peak_cu = peak_cu.max(cu);
        };
        check_cu("native InitMarket", env.init_market_cu);
        let portfolio_key = Keypair::new();
        check_cu(
            "native portfolio System creation",
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                env.portfolio_account_len,
                env.program_id,
            ),
        );
        let portfolio = portfolio_key.pubkey();
        let portfolio_accounts = vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ];
        check_cu(
            "native InitPortfolio",
            env.send(
                ProgInstruction::InitPortfolio,
                portfolio_accounts,
                &[&owner],
            )
            .unwrap(),
        );
        env.portfolios.push(portfolio);
        let user_token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
        let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        let token_rent = env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN);
        let token_keys = [user_token, admin_token, env.vault];
        let empty_tokens = token_keys.map(|key| env.svm.get_account(&key).unwrap());
        for (account, wallet) in
            empty_tokens
                .iter()
                .zip([owner.pubkey(), admin.pubkey(), env.vault_authority])
        {
            let token = TokenAccount::unpack(&account.data).unwrap();
            assert_eq!(account.owner, spl_token::ID);
            assert_eq!(account.lamports, token_rent);
            assert_eq!(token.mint, spl_token::native_mint::ID);
            assert_eq!(token.owner, wallet);
            assert_eq!(token.amount, 0);
            assert_eq!(token.is_native, COption::Some(token_rent));
            assert_eq!(token.state, AccountState::Initialized);
            assert_eq!(token.delegate, COption::None);
            assert_eq!(token.close_authority, COption::None);
        }
        let market_before = env.svm.get_account(&env.market).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio).unwrap();
        check_cu(
            "native public funding and raw vault donation",
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    ComputeBudgetInstruction::set_compute_unit_limit(STEP_CU_LIMIT as u32),
                    system_instruction::transfer(&owner.pubkey(), &user_token, CAPITAL),
                    spl_token::instruction::sync_native(&spl_token::ID, &user_token).unwrap(),
                    system_instruction::transfer(&admin.pubkey(), &env.vault, SURPLUS),
                    spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap(),
                    system_instruction::transfer(&admin.pubkey(), &env.vault, UNSYNCED),
                ],
                &[&owner, &admin],
            )
            .unwrap(),
        );
        assert_eq!(
            env.svm.get_account(&env.market),
            Some(market_before.clone())
        );
        assert_eq!(
            env.svm.get_account(&portfolio),
            Some(portfolio_before.clone())
        );

        let cfg_before = env.market_state().0;
        assert_eq!(
            cfg_before.collateral_mint,
            spl_token::native_mint::ID.to_bytes()
        );
        assert_eq!(cfg_before.secondary_collateral_mint, [0; 32]);
        assert_eq!(
            env.vault,
            canonical_vault_ata(env.vault_authority, env.mint)
        );
        let portfolio_id = env.portfolio_id(portfolio);
        let mint_before = env.svm.get_account(&env.mint);
        let authority_before = env.svm.get_account(&env.vault_authority);
        let owner_before = env.svm.get_account(&owner.pubkey());
        let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let tracked = [
            env.market,
            portfolio,
            env.vault,
            user_token,
            admin_token,
            env.mint,
            env.vault_authority,
            owner.pubkey(),
            admin.pubkey(),
        ];
        let lamport_sum = |env: &V16CuEnv| {
            tracked
                .iter()
                .filter_map(|key| env.svm.get_account(key))
                .map(|account| account.lamports)
                .sum::<u64>()
        };
        let total_lamports = lamport_sum(&env);

        // Inputs and the completed public stage determine every stock. Native mint supply
        // does not count wrapped SOL; backing lamports and token amounts must be reconciled.
        let check = |env: &V16CuEnv, stage: Stage| {
            let capital = if matches!(stage, Stage::Deposited | Stage::Resolved) {
                CAPITAL
            } else {
                0
            };
            let closed = stage == Stage::Closed;
            let materialized = stage < Stage::Dematerialized;
            let synced = sync_before_close && stage >= Stage::Synced;
            let raw_surplus = SURPLUS + if synced { UNSYNCED } else { 0 };
            let raw_lamports = if synced { 0 } else { UNSYNCED };
            let amounts = [
                CAPITAL - capital,
                if closed { raw_surplus } else { 0 },
                capital + raw_surplus,
            ];
            for (index, key) in token_keys.iter().enumerate() {
                if closed && index == 2 {
                    assert!(env.svm.get_account(key).is_none_or(|account| {
                        account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
                    }));
                    continue;
                }
                let mut expected = empty_tokens[index].clone();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                token.amount = amounts[index];
                TokenAccount::pack(token, &mut expected.data).unwrap();
                expected.lamports += amounts[index] + if index == 2 { raw_lamports } else { 0 };
                assert_eq!(
                    env.svm.get_account(key),
                    Some(expected),
                    "{stage:?}: native token frame {key}"
                );
            }
            assert_eq!(env.svm.get_account(&env.mint), mint_before);
            assert_eq!(env.svm.get_account(&env.vault_authority), authority_before);
            assert_eq!(env.svm.get_account(&owner.pubkey()), owner_before);
            let mut expected_admin = admin_before.clone();
            if closed {
                expected_admin.lamports +=
                    market_before.lamports + portfolio_before.lamports + token_rent + raw_lamports
                        - tombstone_rent;
            }
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert_eq!(
                lamport_sum(env),
                total_lamports,
                "{stage:?}: exact lamport conservation excluding the separate fee payer"
            );
            let market = env.svm.get_account(&env.market).unwrap();
            assert_eq!(market.owner, market_before.owner);
            assert_eq!(market.executable, market_before.executable);
            assert_eq!(market.rent_epoch, market_before.rent_epoch);
            assert_eq!(
                market.lamports,
                if closed {
                    tombstone_rent
                } else {
                    market_before.lamports
                        + if materialized {
                            0
                        } else {
                            portfolio_before.lamports
                        }
                }
            );
            let portfolios = if materialized {
                let account = env.svm.get_account(&portfolio).unwrap();
                assert_eq!(account.lamports, portfolio_before.lamports);
                assert_eq!(account.owner, portfolio_before.owner);
                assert_eq!(account.executable, portfolio_before.executable);
                assert_eq!(account.rent_epoch, portfolio_before.rent_epoch);
                assert_eq!(env.portfolio_id(portfolio), portfolio_id);
                let state = env.portfolio_state(portfolio);
                assert_eq!(state.owner, owner.pubkey().to_bytes());
                assert_eq!(state.capital.get(), capital.into());
                vec![state]
            } else {
                assert!(env
                    .svm
                    .get_account(&portfolio)
                    .is_none_or(|account| { account.lamports == 0 && account.data.is_empty() }));
                Vec::new()
            };
            if closed {
                assert_eq!(capital, 0);
                assert_closed_market_tombstone(&market);
            } else {
                let (cfg, group) = state::read_market(&market.data).unwrap();
                assert_eq!(cfg, cfg_before);
                assert_eq!(
                    group.mode,
                    if stage >= Stage::Resolved {
                        MarketModeV16::Resolved
                    } else {
                        MarketModeV16::Live
                    }
                );
                assert_eq!(
                    (group.vault, group.c_tot, group.insurance),
                    (capital.into(), capital.into(), 0)
                );
                // The census owns booked stock. Remove only the independently fixed raw
                // donation from actual SPL custody, never a surplus inferred from the engine.
                let booked = env
                    .token_amount(env.vault)
                    .checked_sub(raw_surplus)
                    .unwrap();
                assert_eq!(booked, capital);
                assert_market_stock_census(
                    "INV-070 native terminal",
                    &group,
                    &market.data,
                    &portfolios,
                    booked.into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census(
                    "INV-070 native terminal",
                    &group,
                    &portfolios,
                )
                .unwrap();
                let mut data = market.data.clone();
                let (_, view) = state::market_view_mut(&mut data).unwrap();
                view.validate_shape().unwrap();
                if materialized {
                    let mut data = env.svm.get_account(&portfolio).unwrap().data;
                    state::portfolio_view_mut_for_market_slots(&mut data, 1)
                        .unwrap()
                        .validate_with_market(&view.as_view())
                        .unwrap();
                }
            }
        };
        check(&env, Stage::Funded);
        check_cu(
            "native Deposit",
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
            .unwrap(),
        );
        check(&env, Stage::Deposited);
        let funded_portfolio = env.svm.get_account(&portfolio);
        check_cu(
            "native ResolveMarket",
            env.send(
                ProgInstruction::ResolveMarket {
                    asset_generation_frontier: 0,
                    authority_epoch: env.control_sequences(0).authority_epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                &[&admin],
            )
            .unwrap(),
        );
        assert_eq!(env.svm.get_account(&portfolio), funded_portfolio);
        check(&env, Stage::Resolved);
        check_cu(
            "native permissionless CloseResolved",
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
            .unwrap(),
        );
        check(&env, Stage::Paid);
        check_cu(
            "native ClosePortfolio",
            env.close_portfolio_with_cu(&owner, portfolio),
        );
        check(&env, Stage::Dematerialized);
        if sync_before_close {
            let frame = [env.market, portfolio].map(|key| env.svm.get_account(&key));
            env.svm.expire_blockhash();
            check_cu(
                "terminal vault SyncNative",
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap(),
                    &[],
                )
                .unwrap(),
            );
            assert_eq!(
                [env.market, portfolio].map(|key| env.svm.get_account(&key)),
                frame
            );
            check(&env, Stage::Synced);
        }
        check_cu(
            "native CloseSlab",
            env.send(
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
                ],
                &[&admin],
            )
            .unwrap(),
        );
        check(&env, Stage::Closed);

        for (token, wallet, amount) in [
            (user_token, &owner, CAPITAL),
            (
                admin_token,
                &admin,
                SURPLUS + if sync_before_close { UNSYNCED } else { 0 },
            ),
        ] {
            let frame = tracked.map(|key| (key, env.svm.get_account(&key)));
            check_cu(
                "terminal native owner redemption",
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::close_account(
                        &spl_token::ID,
                        &token,
                        &wallet.pubkey(),
                        &wallet.pubkey(),
                        &[],
                    )
                    .unwrap(),
                    &[wallet],
                )
                .unwrap(),
            );
            for (key, mut expected) in frame {
                if key == token {
                    assert!(env.svm.get_account(&key).is_none_or(|account| {
                        account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
                    }));
                    continue;
                }
                if key == wallet.pubkey() {
                    expected.as_mut().unwrap().lamports += token_rent + amount;
                }
                assert_eq!(
                    env.svm.get_account(&key),
                    expected,
                    "redemption frame {key}"
                );
            }
            assert_eq!(lamport_sum(&env), total_lamports);
        }
        println!("INV-070 native terminal: sync={sync_before_close}, user={CAPITAL}, raw_tokens={SURPLUS}, raw_lamports={UNSYNCED}, peak_CU={peak_cu}; exact tombstone and both owner redemptions");
    }
}

#[test]
fn v16_program_dual_quote_terminal_history_classifies_stock_and_exact_tombstone_rent() {
    use super::inv_018_quote_mint_vault_token_program_and_authority_integrity::{
        inv018_create_public_spl_mint, inv018_public_spl_market,
    };
    use super::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const DEPOSIT: u64 = 1_200;
    const WITHDRAW: u64 = 137;
    const INSURANCE: u64 = 300;
    const SURPLUS: u64 = 17;
    const SECONDARY_RESERVE: u64 = 1_800;
    const STEP_CU_LIMIT: u64 = 150_000;
    const TERMINAL_CU_LIMIT: u64 = 300_000;

    struct Rail {
        mint: Pubkey,
        vault: Pubkey,
        user_token: Pubkey,
        admin_token: Pubkey,
        funded: u64,
    }

    // Keep the existing decimal product; mixed rails add native custody in either
    // position without repeating the single-native-vault sync/redemption witness.
    for (decimals, native_rail) in [
        (0, None),
        (6, None),
        (9, None),
        (u8::MAX, None),
        (spl_token::native_mint::DECIMALS, Some(0)),
        (spl_token::native_mint::DECIMALS, Some(1)),
    ] {
        for payout_rail in 0..2 {
            let mut env = if native_rail.is_some() {
                inv081_public_native_market()
            } else {
                inv018_public_spl_market(decimals)
            };
            let owner = Keypair::new();
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            let mut peak_step_cu = env.init_market_cu;
            let mut check_step = |label, cu| {
                assert_cu_within(label, cu, STEP_CU_LIMIT);
                peak_step_cu = peak_step_cu.max(cu);
            };
            check_step("public InitMarket", env.init_market_cu);

            let added_mint = inv018_create_public_spl_mint(
                &mut env.svm,
                &env.payer,
                env.admin.pubkey(),
                decimals,
            );
            let secondary_mint = if native_rail == Some(1) {
                // Publicly move the empty bootstrap native vault to the secondary
                // role; only the host's account handles change after this succeeds.
                check_step(
                    "public UpdateBaseUnitMints native secondary",
                    env.send(
                        ProgInstruction::UpdateBaseUnitMints {
                            primary_mint: added_mint.to_bytes(),
                            secondary_mint: env.mint.to_bytes(),
                            authority_epoch: env.control_sequences(0).authority_epoch,
                        },
                        vec![
                            AccountMeta::new(env.admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new_readonly(added_mint, false),
                            AccountMeta::new_readonly(env.mint, false),
                            AccountMeta::new_readonly(env.vault, false),
                        ],
                        &[&env.admin.insecure_clone()],
                    )
                    .unwrap(),
                );
                let native_mint = env.mint;
                env.mint = added_mint;
                env.vault =
                    create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, env.mint);
                native_mint
            } else {
                check_step(
                    "public UpdateBaseUnitMints",
                    env.update_base_unit_mints_with_cu(env.mint, added_mint),
                );
                added_mint
            };
            let portfolio_key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                env.portfolio_account_len,
                env.program_id,
            );
            let portfolio = portfolio_key.pubkey();
            let portfolio_accounts = vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ];
            check_step(
                "public InitPortfolio",
                env.send(
                    ProgInstruction::InitPortfolio,
                    portfolio_accounts.clone(),
                    &[&owner],
                )
                .unwrap(),
            );
            env.portfolios.push(portfolio);

            let mut rails = Vec::new();
            for (rail, mint) in [env.mint, secondary_mint].into_iter().enumerate() {
                let vault = if rail == 0 {
                    env.vault
                } else if native_rail == Some(1) {
                    canonical_vault_ata(env.vault_authority, mint)
                } else {
                    create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, mint)
                };
                let user_token =
                    create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), mint);
                let admin_token =
                    create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), mint);
                let native = native_rail == Some(rail);
                assert_eq!(mint == spl_token::native_mint::ID, native);
                let admin_funding = if rail == 0 {
                    INSURANCE + SURPLUS
                } else {
                    SECONDARY_RESERVE
                };
                let mut funding = Vec::new();
                for (destination, amount) in [
                    (admin_token, admin_funding),
                    (user_token, if rail == 0 { DEPOSIT } else { 0 }),
                ] {
                    if amount == 0 {
                        continue;
                    }
                    if native {
                        funding.extend([
                            system_instruction::transfer(&env.admin.pubkey(), &destination, amount),
                            spl_token::instruction::sync_native(&spl_token::ID, &destination)
                                .unwrap(),
                        ]);
                    } else {
                        funding.push(
                            spl_token::instruction::mint_to(
                                &spl_token::ID,
                                &mint,
                                &destination,
                                &env.admin.pubkey(),
                                &[],
                                amount,
                            )
                            .unwrap(),
                        );
                    }
                }
                if !native {
                    funding.push(
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &env.admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                    );
                }
                send_raw_ixs(&mut env.svm, &env.payer, funding, &[&env.admin]).unwrap();
                rails.push(Rail {
                    mint,
                    vault,
                    user_token,
                    admin_token,
                    funded: if rail == 0 {
                        DEPOSIT + INSURANCE + SURPLUS
                    } else {
                        SECONDARY_RESERVE
                    },
                });
            }

            let wrap = |ix: ProgInstruction, accounts| Instruction {
                program_id: percolator_prog::id(),
                accounts,
                data: ix.encode(),
            };
            let mut deposit_accounts = portfolio_accounts.clone();
            deposit_accounts.extend([
                AccountMeta::new(rails[0].user_token, false),
                AccountMeta::new(rails[0].vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ]);
            let top_up_accounts = vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(rails[0].admin_token, false),
                AccountMeta::new(rails[0].vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];
            let mut top_up = ProgInstruction::TopUpInsurance {
                market_id: 0,
                intent_id: 0,
                authority_epoch: 0,
                amount: INSURANCE.into(),
            };
            bind_current_generation_guards(&env.svm, &top_up_accounts, &mut top_up);
            let mut funding = vec![
                heap_ix(),
                ComputeBudgetInstruction::set_compute_unit_limit(STEP_CU_LIMIT as u32),
                wrap(env.deposit_ix(portfolio, DEPOSIT.into()), deposit_accounts),
                wrap(top_up, top_up_accounts),
            ];
            for (rail, accounts) in rails.iter().enumerate() {
                funding.push(
                    spl_token::instruction::transfer(
                        &spl_token::ID,
                        &accounts.admin_token,
                        &accounts.vault,
                        &env.admin.pubkey(),
                        &[],
                        if rail == 0 {
                            SURPLUS
                        } else {
                            SECONDARY_RESERVE
                        },
                    )
                    .unwrap(),
                );
            }
            check_step(
                "funded primary claims, insurance and both raw vault stocks",
                send_raw_ixs(&mut env.svm, &env.payer, funding, &[&owner, &env.admin]).unwrap(),
            );

            // Only predetermined payments drive this oracle. In particular, primary
            // backing discharged via the secondary rail becomes surplus, not a second claim.
            let token_rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let assert_stock = |env: &V16CuEnv,
                                user_paid: [u64; 2],
                                insurance_paid: [u64; 2],
                                materialized: bool,
                                closed: bool| {
                let claim = DEPOSIT - user_paid.iter().sum::<u64>();
                let insurance = INSURANCE - insurance_paid.iter().sum::<u64>();
                let secondary_paid = user_paid[1] + insurance_paid[1];
                let surplus = SURPLUS + secondary_paid;
                let remaining = [
                    claim + insurance + surplus,
                    SECONDARY_RESERVE - secondary_paid,
                ];
                for (rail, accounts) in rails.iter().enumerate() {
                    let native = native_rail == Some(rail);
                    let mint_account = env.svm.get_account(&accounts.mint).unwrap();
                    let mint = Mint::unpack(&mint_account.data).unwrap();
                    assert_eq!(mint_account.owner, spl_token::ID);
                    assert_eq!(mint.supply, if native { 0 } else { accounts.funded });
                    assert_eq!(mint.decimals, decimals);
                    assert_eq!(mint.mint_authority, COption::None);
                    assert_eq!(mint.freeze_authority, COption::None);
                    let mut tracked_tokens = 0;
                    for (key, wallet, expected) in [
                        (accounts.user_token, owner.pubkey(), user_paid[rail]),
                        (
                            accounts.admin_token,
                            env.admin.pubkey(),
                            insurance_paid[rail] + if closed { remaining[rail] } else { 0 },
                        ),
                        (accounts.vault, env.vault_authority, remaining[rail]),
                    ] {
                        if closed && key == accounts.vault {
                            if let Some(account) = env.svm.get_account(&key) {
                                assert_eq!(
                                    account.lamports, 0,
                                    "canonical SPL vault rent reclaimed"
                                );
                                assert!(
                                    account.data.iter().all(|byte| *byte == 0),
                                    "SPL vault state cleared"
                                );
                            }
                            continue;
                        }
                        let token_account = env.svm.get_account(&key).unwrap();
                        let token = TokenAccount::unpack(&token_account.data).unwrap();
                        assert_eq!(token_account.owner, spl_token::ID);
                        assert_eq!(token.mint, accounts.mint);
                        assert_eq!(token.owner, wallet);
                        assert_eq!(token.state, AccountState::Initialized);
                        assert_eq!(
                            token.is_native,
                            if native {
                                COption::Some(token_rent)
                            } else {
                                COption::None
                            }
                        );
                        assert_eq!(
                            token_account.lamports,
                            token_rent + if native { expected } else { 0 },
                            "exact rent and backing on rail {rail}, account {key}"
                        );
                        assert_eq!(token.delegate, COption::None);
                        assert_eq!(token.close_authority, COption::None);
                        assert_eq!(token.amount, expected, "rail {rail}, account {key}");
                        tracked_tokens += token.amount;
                    }
                    assert_eq!(
                        tracked_tokens, accounts.funded,
                        "every atom classified on rail {rail}"
                    );
                    assert_eq!(
                        accounts.vault,
                        canonical_vault_ata(env.vault_authority, accounts.mint)
                    );
                }
                if closed {
                    assert_eq!((claim, insurance), (0, 0));
                    assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
                } else {
                    let (cfg, group) = env.market_state();
                    assert_eq!(cfg.collateral_mint, rails[0].mint.to_bytes());
                    assert_eq!(cfg.secondary_collateral_mint, rails[1].mint.to_bytes());
                    assert_eq!(group.c_tot, claim.into());
                    assert_eq!(group.insurance, insurance.into());
                    assert_eq!(group.vault, u128::from(claim + insurance));
                    assert_eq!(group.materialized_portfolio_count, u64::from(materialized));
                }
                if materialized {
                    assert_eq!(env.portfolio_state(portfolio).capital.get(), claim.into());
                } else {
                    if let Some(account) = env.svm.get_account(&portfolio) {
                        assert_eq!(account.lamports, 0, "portfolio rent reclaimed");
                        assert!(account.data.is_empty(), "portfolio storage reclaimed");
                    }
                }
            };
            let mut user_paid = [0; 2];
            let mut insurance_paid = [0; 2];
            assert_stock(&env, user_paid, insurance_paid, true, false);

            let payout_accounts = |rail: usize, signed: bool| {
                vec![
                    AccountMeta::new_readonly(owner.pubkey(), signed),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(rails[rail].user_token, false),
                    AccountMeta::new(rails[rail].vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ]
            };
            let withdraw_rail = 1 - payout_rail;
            let withdraw_accounts = payout_accounts(withdraw_rail, true);
            let close_resolved_accounts = payout_accounts(payout_rail, false);
            check_step(
                "live cross-rail Withdraw",
                env.send(
                    env.withdraw_ix(portfolio, WITHDRAW.into()),
                    withdraw_accounts,
                    &[&owner],
                )
                .unwrap(),
            );
            user_paid[withdraw_rail] = WITHDRAW;
            assert_stock(&env, user_paid, insurance_paid, true, false);
            check_step("ResolveMarket", env.resolve());
            assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
            assert_stock(&env, user_paid, insurance_paid, true, false);

            let terminal = vec![
                wrap(
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    close_resolved_accounts,
                ),
                wrap(env.close_portfolio_ix(portfolio), portfolio_accounts),
                wrap(
                    env.withdraw_insurance_asset_instruction(
                        env.admin.pubkey(),
                        0,
                        INSURANCE.into(),
                    ),
                    vec![
                        AccountMeta::new(env.admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(rails[withdraw_rail].admin_token, false),
                        AccountMeta::new(rails[withdraw_rail].vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                ),
                wrap(
                    ProgInstruction::CloseSlab {
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    },
                    vec![
                        AccountMeta::new(env.admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(rails[0].vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new(rails[0].admin_token, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(rails[1].vault, false),
                        AccountMeta::new(rails[1].admin_token, false),
                    ],
                ),
            ];

            // Abort after both SPL vault closes and the typed market tombstone write.
            // The exact failing index distinguishes this from an earlier guard rejection.
            let mut aborted = vec![
                heap_ix(),
                ComputeBudgetInstruction::set_compute_unit_limit(TERMINAL_CU_LIMIT as u32),
            ];
            aborted.extend(terminal.clone());
            aborted.push(
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &rails[0].user_token,
                    &rails[0].admin_token,
                    &owner.pubkey(),
                    &[],
                    rails[0].funded + 1,
                )
                .unwrap(),
            );
            env.svm.expire_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &aborted,
                Some(&env.payer.pubkey()),
                &[&env.payer, &env.admin, &owner],
                env.svm.latest_blockhash(),
            );
            let fee = u64::from(tx.message.header.num_required_signatures)
                * FeeStructure::default().lamports_per_signature;
            let mut keys = tx.message.account_keys.clone();
            keys.extend(rails.iter().map(|rail| rail.mint));
            keys.sort_unstable();
            keys.dedup();
            let frame: Vec<_> = keys
                .into_iter()
                .map(|key| (key, env.svm.get_account(&key)))
                .collect();
            let failure = env
                .svm
                .send_transaction(tx)
                .expect_err("late SPL suffix must reject");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    6,
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32
                    ),
                )
            );
            assert_cu_within(
                "complete terminal rollback",
                failure.meta.compute_units_consumed,
                TERMINAL_CU_LIMIT,
            );
            for (key, mut before) in frame {
                if key == env.payer.pubkey() {
                    before.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(
                    env.svm.get_account(&key),
                    before,
                    "full account rollback for {key}"
                );
            }
            assert_stock(&env, user_paid, insurance_paid, true, false);

            let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
            let portfolio_rent = env.svm.get_account(&portfolio).unwrap().lamports;
            let vault_rent: u64 = rails
                .iter()
                .enumerate()
                .map(|(rail, accounts)| {
                    let account = env.svm.get_account(&accounts.vault).unwrap();
                    let token = TokenAccount::unpack(&account.data).unwrap();
                    // Wrapped principal leaves through SPL transfers, never as a rent refund.
                    account.lamports
                        - if native_rail == Some(rail) {
                            token.amount
                        } else {
                            0
                        }
                })
                .sum();
            let admin_lamports = env.svm.get_account(&env.admin.pubkey()).unwrap().lamports;
            let owner_lamports = env.svm.get_account(&owner.pubkey()).unwrap().lamports;
            let mut materialized = true;
            for (step, ix) in terminal.into_iter().enumerate() {
                env.svm.expire_blockhash();
                let signers: &[&Keypair] = match step {
                    0 => &[], // The separate fee payer alone lands the economic user payout.
                    1 => &[&owner],
                    _ => &[&env.admin],
                };
                check_step(
                    [
                        "CloseResolved",
                        "ClosePortfolio",
                        "WithdrawInsuranceAsset",
                        "CloseSlab",
                    ][step],
                    send_raw_tx(&mut env.svm, &env.payer, ix, signers).unwrap(),
                );
                match step {
                    0 => user_paid[payout_rail] = DEPOSIT - WITHDRAW,
                    1 => {
                        materialized = false;
                        assert_eq!(
                            env.svm.get_account(&env.market).unwrap().lamports,
                            market_rent + portfolio_rent
                        );
                    }
                    2 => insurance_paid[withdraw_rail] = INSURANCE,
                    3 => {}
                    _ => unreachable!(),
                }
                assert_stock(&env, user_paid, insurance_paid, materialized, step == 3);
            }
            let tombstone_rent = solana_sdk::rent::Rent::default()
                .minimum_balance(percolator_prog::constants::HEADER_LEN);
            assert_eq!(
                env.svm.get_account(&env.admin.pubkey()).unwrap().lamports,
                admin_lamports + market_rent + portfolio_rent + vault_rent - tombstone_rent
            );
            assert_eq!(
                env.svm.get_account(&owner.pubkey()).unwrap().lamports,
                owner_lamports
            );
            println!("INV-070 decimals={decimals}, native_rail={native_rail:?}, payout_rail={payout_rail}: peak step {peak_step_cu} CU, terminal rollback {} CU",
                failure.meta.compute_units_consumed);
        }
    }
}

#[test]
fn v16_program_recovery_force_close_reaches_zero_residue_and_close_slab() {
    const INITIAL_CAPITAL: u128 = 1_000_000;
    const OPEN_Q: u128 = 2 * POS_SCALE;
    const SHUTDOWN_SLOT: u64 = 2;
    const FORCE_CLOSE_SLOT: u64 = 7;

    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(100, 5);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let cranker = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, INITIAL_CAPITAL);
    env.deposit(&short_owner, short, INITIAL_CAPITAL);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        OPEN_Q as i128,
        100,
        0,
    );
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, OPEN_Q);
    assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, OPEN_Q);
    assert_eq!(env.token_amount(env.vault), 2 * INITIAL_CAPITAL as u64);

    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        0,
        SHUTDOWN_SLOT,
        0,
    );
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );

    env.svm.warp_to_slot(FORCE_CLOSE_SLOT);
    let force_close_cu =
        env.force_close_abandoned_asset_with_cu(&cranker, long, short, 0, FORCE_CLOSE_SLOT, OPEN_Q);
    assert_cu_within(
        "Recovery force-close before terminal slab",
        force_close_cu,
        CUSTODY_CU_LIMIT,
    );
    let recovered = env.market_state().1;
    assert_eq!(recovered.assets[0].oi_eff_long_q, 0);
    assert_eq!(recovered.assets[0].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
    assert_eq!(recovered.vault, 2 * INITIAL_CAPITAL);
    assert_eq!(recovered.c_tot, 2 * INITIAL_CAPITAL);
    assert_eq!(recovered.vault as u64, env.token_amount(env.vault));

    env.resolve();
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
    let resolved_slot = env.market_state().1.resolved_slot;
    let market_before_timeout = env.svm.get_account(&env.market).unwrap();
    let long_before_timeout = env.svm.get_account(&long).unwrap();
    let vault_before_timeout = env.svm.get_account(&env.vault).unwrap();
    let (_, early_close) = env.try_close_resolved_with_cu(&long_owner, long);
    assert!(
        early_close.is_err(),
        "unsigned CloseResolved must remain owner-gated before the configured timeout",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_timeout
    );
    assert_eq!(env.svm.get_account(&long).unwrap(), long_before_timeout);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_timeout
    );

    env.svm.warp_to_slot(resolved_slot + 5);
    let long_destination = env.close_resolved(&long_owner, long);
    let short_destination = env.close_resolved(&short_owner, short);
    assert_eq!(env.token_amount(long_destination), INITIAL_CAPITAL as u64);
    assert_eq!(env.token_amount(short_destination), INITIAL_CAPITAL as u64);
    env.close_portfolio_with_cu(&long_owner, long);
    env.close_portfolio_with_cu(&short_owner, short);

    let (_, terminal) = env.market_state();
    assert_eq!(terminal.materialized_portfolio_count, 0);
    assert_eq!(terminal.vault, 0);
    assert_eq!(terminal.c_tot, 0);
    assert_eq!(terminal.insurance, 0);
    assert_eq!(env.token_amount(env.vault), 0);

    let admin = env.admin.insecure_clone();
    let admin_destination = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(admin_destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .expect("Recovery-normalized market must reach CloseSlab");
    assert_cu_within("Recovery-normalized CloseSlab", close_cu, CUSTODY_CU_LIMIT);
    assert_eq!(env.token_amount(admin_destination), 0);
    assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
}

#[test]
fn v16_program_close_slab_rejects_until_market_has_zero_terminal_residue() {
    let mut env = V16CuEnv::new();
    let market = env.market;
    let vault = env.vault;
    let vault_authority = env.vault_authority;
    let admin = env.admin.insecure_clone();
    let admin_dest = env.token_account(admin.pubkey(), 0);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let try_close = |env: &mut V16CuEnv| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new(admin_dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };

    for label in ["live funded market", "resolved funded market"] {
        let before_market = env.svm.get_account(&market).unwrap();
        let before_vault = env.svm.get_account(&vault).unwrap();
        let before_portfolio = env.svm.get_account(&portfolio).unwrap();
        let before_dest = env.svm.get_account(&admin_dest).unwrap();
        assert!(
            try_close(&mut env).is_err(),
            "{label}: CloseSlab must reject before terminal accounting is zero",
        );
        assert_eq!(
            env.svm.get_account(&market).unwrap(),
            before_market,
            "{label}: rejected CloseSlab must not mutate market accounting",
        );
        assert_eq!(
            env.svm.get_account(&vault).unwrap(),
            before_vault,
            "{label}: rejected CloseSlab must not move or close vault custody",
        );
        assert_eq!(
            env.svm.get_account(&portfolio).unwrap(),
            before_portfolio,
            "{label}: rejected CloseSlab must not mutate user portfolio state",
        );
        assert_eq!(
            env.svm.get_account(&admin_dest).unwrap(),
            before_dest,
            "{label}: rejected CloseSlab must not pay final dust destination",
        );

        if label == "live funded market" {
            env.resolve();
            assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
        }
    }

    let user_dest = env.close_resolved(&owner, portfolio);
    assert_eq!(
        env.token_amount(user_dest),
        1_000_000,
        "resolved user wind-down remains available after rejected CloseSlab attempts",
    );
    env.close_portfolio_with_cu(&owner, portfolio);
    let (_, drained) = env.market_state();
    assert_eq!(
        (
            drained.vault,
            drained.insurance,
            drained.c_tot,
            drained.materialized_portfolio_count,
        ),
        (0, 0, 0, 0),
        "all terminal user value and accounting are drained before final slab reclaim",
    );

    assert!(
        try_close(&mut env).is_ok(),
        "fully drained market can be reclaimed by CloseSlab",
    );
    let closed_market = env.svm.get_account(&market).unwrap();
    assert_closed_market_tombstone(&closed_market);
}

#[test]
fn v16_program_close_slab_refunds_exact_vault_and_market_excess_rent_after_normal_exit() {
    const PRINCIPAL: u128 = 321;

    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, PRINCIPAL);
    let owner_destination = env.withdraw(&owner, portfolio, PRINCIPAL);
    assert_eq!(env.token_amount(owner_destination), PRINCIPAL as u64);
    env.close_portfolio_with_cu(&owner, portfolio);
    env.resolve();

    let terminal = env.market_state().1;
    assert_eq!(
        (
            terminal.vault,
            terminal.insurance,
            terminal.c_tot,
            terminal.materialized_portfolio_count,
            env.token_amount(env.vault),
        ),
        (0, 0, 0, 0, 0),
        "normal owner exit must leave no token or accounting stock before CloseSlab",
    );

    let admin = env.admin.insecure_clone();
    let destination = env.token_account(admin.pubkey(), 0);
    let authority_epoch = env.control_sequences(0).authority_epoch;
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
    let retained_market_rent = env
        .svm
        .get_sysvar::<solana_sdk::rent::Rent>()
        .minimum_balance(percolator_prog::constants::HEADER_LEN);
    let expected_refund = market_before
        .lamports
        .checked_sub(retained_market_rent)
        .and_then(|excess| excess.checked_add(vault_before.lamports))
        .expect("bounded fixture lamport refund");
    let tracked_lamports_before = admin_before
        .lamports
        .checked_add(market_before.lamports)
        .and_then(|amount| amount.checked_add(vault_before.lamports))
        .expect("bounded fixture lamport stock");

    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            ProgInstruction::CloseSlab { authority_epoch },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .expect("normal-exit CloseSlab");
    assert_cu_within(
        "normal-exit CloseSlab exact rent refund",
        close_cu,
        CUSTODY_CU_LIMIT,
    );

    let market_after = env.svm.get_account(&env.market).unwrap();
    let admin_after = env.svm.get_account(&admin.pubkey()).unwrap();
    let vault_after_lamports = env
        .svm
        .get_account(&env.vault)
        .map_or(0, |account| account.lamports);
    let actual_refund = admin_after
        .lamports
        .checked_sub(admin_before.lamports)
        .expect("CloseSlab must not debit the current market authority");
    assert_closed_market_tombstone(&market_after);
    assert_eq!(market_after.lamports, retained_market_rent);
    assert_eq!(env.token_amount(destination), 0);
    assert_eq!(
        vault_after_lamports, 0,
        "the closed SPL vault retains no rent"
    );
    assert_eq!(
        actual_refund, expected_refund,
        "CloseSlab refunds exactly the primary vault rent plus market excess rent",
    );
    assert_eq!(
        admin_after.lamports + market_after.lamports + vault_after_lamports,
        tracked_lamports_before,
        "successful CloseSlab neither creates nor strands tracked lamports",
    );
    println!(
        "INV-070 normal CloseSlab: CU={close_cu}, refund={actual_refund}, retained={retained_market_rent}"
    );
}

#[test]
fn v16_program_close_slab_final_dust_destination_validation_is_atomic() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.resolve();
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 7);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    let close_slab_to = |env: &mut V16CuEnv, dest: Pubkey| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };
    let assert_rejected_close_unchanged = |env: &V16CuEnv, label: &str| {
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label}: market slab must not be zeroed",
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "{label}: primary vault must not be transferred or closed",
        );
    };

    let wrong_mint = Pubkey::new_unique();
    let wrong_mint_dest = env.token_account_for_mint(wrong_mint, admin.pubkey(), 0);
    let wrong_mint_close = close_slab_to(&mut env, wrong_mint_dest);
    assert!(
        wrong_mint_close.is_err(),
        "CloseSlab must reject a wrong-mint primary destination",
    );
    assert_eq!(
        env.token_amount(wrong_mint_dest),
        0,
        "wrong-mint destination receives nothing",
    );
    assert_rejected_close_unchanged(&env, "wrong-mint primary destination rejection");

    let foreign_dest = env.token_account_for_mint(env.mint, Pubkey::new_unique(), 0);
    let foreign_close = close_slab_to(&mut env, foreign_dest);
    assert!(
        foreign_close.is_err(),
        "CloseSlab must reject a third-party primary destination",
    );
    assert_eq!(
        env.token_amount(foreign_dest),
        0,
        "foreign destination receives nothing",
    );
    assert_rejected_close_unchanged(&env, "foreign primary destination rejection");

    let good_dest = env.token_account(admin.pubkey(), 0);
    let good_close = close_slab_to(&mut env, good_dest);
    assert!(
        good_close.is_ok(),
        "valid CloseSlab still recovers final vault dust: {good_close:?}",
    );
    assert_eq!(
        env.token_amount(good_dest),
        7,
        "primary vault dust is recovered to current market authority",
    );
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&closed_market);
}

// security.md sweep — CloseSlab with secondary collateral (#44/#48): if a secondary collateral mint is
// configured, closing the slab must not zero the market after closing only the primary vault. Any
// secondary reserve must be recovered atomically in the same close, or the PDA-held reserve is stranded.
#[test]
fn v16_attack_close_slab_requires_secondary_vault_recovery() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 50),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.resolve();
    let market_before = env.svm.get_account(&env.market).unwrap();

    let primary_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let primary_only = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        primary_only.is_err(),
        "CloseSlab must reject when a configured secondary vault is omitted"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "omitted-secondary close leaves market intact"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        50,
        "secondary reserve remains recoverable after rejected close"
    );

    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 7),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let primary_dest = env.token_account(admin.pubkey(), 0);
    let wrong_secondary_dest = env.token_account_for_mint(secondary_mint, Pubkey::new_unique(), 0);
    env.svm.expire_blockhash();
    let wrong_secondary_dest_close = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new(wrong_secondary_dest, false),
        ],
        &[&admin],
    );
    assert!(
        wrong_secondary_dest_close.is_err(),
        "CloseSlab must reject before closing any vault when the secondary destination is wrong"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "bad secondary destination leaves market intact"
    );
    assert_eq!(
        env.token_amount(env.vault),
        7,
        "primary vault dust remains recoverable"
    );
    assert_eq!(
        env.token_amount(primary_dest),
        0,
        "primary dust not paid before bad secondary validation"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        50,
        "secondary vault remains recoverable"
    );
    assert_eq!(
        env.token_amount(wrong_secondary_dest),
        0,
        "wrong secondary destination receives nothing"
    );

    let primary_dest = env.token_account(admin.pubkey(), 0);
    let secondary_dest = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let close_both = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new(secondary_dest, false),
        ],
        &[&admin],
    );
    assert!(
        close_both.is_ok(),
        "CloseSlab closes both configured vaults: {:?}",
        close_both
    );
    assert_eq!(
        env.token_amount(primary_dest),
        7,
        "primary vault dust recovered to admin"
    );
    assert_eq!(
        env.token_amount(secondary_dest),
        50,
        "secondary reserve recovered to admin"
    );
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&closed_market);
}

// SOL-010 / account-kind confusion: InitPortfolio is permissionless and targets a program-owned
// writable account. Passing the market slab itself as the portfolio target must reject atomically;
// SOL-010 (reinitialization): InitPortfolio targets a program-owned account and SETS its owner. An
// attacker could try to re-init a VICTIM's already-funded portfolio -- which would reset its capital
// and reassign ownership, a severe LOF (victim's vaulted tokens orphaned). The is_initialized guard
// SOL-010 / DoS: an initialized-but-empty portfolio still increments materialized_portfolio_count.
// Reinitializing it must not register the same account twice, or one legitimate ClosePortfolio would
// LOF (fund-stranding): ClosePortfolio zeroes the account and reclaims its rent. If it allowed closing
// a portfolio with non-zero capital, those vaulted tokens would be orphaned -- and this would NOT trip
// conservation (vault still >= c_tot + insurance; the tokens just become unwithdrawable). The closable
// Regression for the marketauth terminal-cleanup privilege: it is only a liveness tool for already
// closable empty portfolios. It must not let marketauth skip CloseResolved and burn a user's pending
// payout/capital during market wind-down.
#[test]
fn v16_attack_marketauth_terminal_close_cannot_skip_resolved_payout() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let terminal_close = env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        terminal_close.is_err(),
        "marketauth terminal cleanup must reject a portfolio with unresolved payout/capital"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected terminal ClosePortfolio must not mutate resolved market accounting"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected terminal ClosePortfolio must not dematerialize the user's payout state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected terminal ClosePortfolio must not move custody"
    );

    let (dest, _) = env.close_resolved_with_cu(&owner, portfolio);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "owner still recovers through CloseResolved"
    );
    let (_, group) = env.market_state();
    assert_eq!(
        group.vault, 0,
        "resolved payout drains accounted vault value"
    );
    assert_eq!(
        group.materialized_portfolio_count, 1,
        "CloseResolved pays value; ClosePortfolio performs the separate dematerialization step"
    );

    env.svm.expire_blockhash();
    env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    )
    .expect("owner can dematerialize the empty resolved portfolio after payout");
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "owner ClosePortfolio completes the wind-down after payout"
    );
}

// Same terminal-cleanup boundary, but for the deferred top-up receipt lane: after a partial resolved
// payout the portfolio can have zero capital while still carrying unpaid user value. Marketauth must
// not be able to dematerialize that receipt before ClaimResolvedPayoutTopup finishes it.
#[test]
fn v16_attack_marketauth_terminal_close_cannot_burn_pending_payout_topup() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    {
        let mut market_account = env.svm.get_account(&env.market).expect("market account");
        let mut portfolio_account = env.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        env.svm.set_account(env.market, market_account).unwrap();
        env.svm.set_account(portfolio, portfolio_account).unwrap();
    }
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 60);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let terminal_close = env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        terminal_close.is_err(),
        "marketauth terminal cleanup must reject a portfolio with pending payout top-up"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "pending receipt must not be dematerialized"
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let good_dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, good_dest);
    assert_eq!(
        env.token_amount(good_dest),
        60,
        "owner receives the pending top-up"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);

    env.svm.expire_blockhash();
    env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    )
    .expect("owner can close after pending payout top-up is finalized");
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "receipt-finalized account is closable"
    );
}

#[derive(Clone, Copy)]
struct Inv070TerminalCompositionClass {
    class: &'static str,
    engine_proofs: &'static [&'static str],
    public_witnesses: &'static [&'static str],
}

fn inv070_source_defines_test(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

fn inv070_braced_body_after<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing production marker {marker}"));
    let open = start
        + source[start..]
            .find('{')
            .unwrap_or_else(|| panic!("missing body after {marker}"));
    let mut depth = 0i32;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[(open + 1)..(open + offset)];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated body after {marker}");
}

#[test]
fn v16_program_terminal_stock_and_close_slab_composition_is_source_complete() {
    const ENGINE_PIN: &str = "394fd0bf2cb7d73df425eb3754dc3be1a0c44336";
    const CLASSES: &[Inv070TerminalCompositionClass] = &[
        Inv070TerminalCompositionClass {
            class: "unsettled accounts, capital, positive claims, and payout receipts",
            engine_proofs: &[
                "proof_v16_public_terminal_insurance_retirement_requires_resolved_ready_accounts",
                "proof_v16_public_terminal_insurance_retirement_rejects_account_capital",
                "proof_v16_public_terminal_insurance_retirement_rejects_positive_source_claim",
            ],
            public_witnesses: &[
                "v16_program_close_slab_rejects_until_market_has_zero_terminal_residue",
                "v16_attack_marketauth_terminal_close_cannot_skip_resolved_payout",
                "v16_attack_marketauth_terminal_close_cannot_burn_pending_payout_topup",
                "v16_program_full_terminal_lifecycle_is_claimant_order_independent",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "fresh backing principal, provider earnings, expiry, and withdrawal",
            engine_proofs: &[
                "proof_v16_public_terminal_insurance_retirement_rejects_provider_earnings",
                "proof_v16_public_terminal_insurance_retirement_rejects_backing_principal",
            ],
            public_witnesses: &[
                "v16_program_resolved_close_normalizes_backing_at_expiry",
                "insurance_spend_composes_through_liquidation_partial_receipt_and_terminal_payout",
                "expired_backing_composes_through_insurance_recredit_and_terminal_slab_cleanup",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "domain insurance budgets, reservations, spent overlap, and recredit",
            engine_proofs: &[
                "proof_v16_public_terminal_insurance_retirement_rejects_every_live_reservation_class",
                "proof_v16_terminal_claim_free_overlap_recredit_is_exactly_bounded",
                "proof_v16_terminal_claim_free_overlap_recredit_updates_only_paired_insurance_domain",
            ],
            public_witnesses: &[
                "v16_program_prior_insurance_frames_all_partial_receipt_orders",
                "expired_backing_composes_through_insurance_recredit_and_terminal_slab_cleanup",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "claim-free protocol surplus and final internal stock retirement",
            engine_proofs: &[
                "proof_v16_terminal_unbudgeted_insurance_retirement_is_exact_and_claim_safe",
                "proof_v16_public_terminal_insurance_retirement_is_exact_and_fully_framed",
            ],
            public_witnesses: &[
                "v16_program_recovery_force_close_reaches_zero_residue_and_close_slab",
                "expired_backing_composes_through_insurance_recredit_and_terminal_slab_cleanup",
                "v16_primary_quote_routes_match_actual_spl_and_internal_accounting_deltas",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "bounded asset scan and persisted strict-progress cursor",
            engine_proofs: &[
                "proof_v16_terminal_slab_asset_step_is_total_and_priority_ordered",
                "proof_v16_terminal_slab_wait_is_error_or_strict_cursor_progress",
            ],
            public_witnesses: &[
                "v16_bpf_terminal_claim_free_surplus_close_stays_bounded_on_10m_market",
                "v16_bpf_terminal_insurance_last_domain_withdraw_stays_bounded_on_10m_market",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "authority, canonical vaults, optional secondary reserve, aliases, and tombstone",
            engine_proofs: &[],
            public_witnesses: &[
                "v16_attack_close_slab_rejects_stale_marketauth_after_rotation",
                "v16_program_close_slab_account_roles_are_exhaustive",
                "v16_attack_close_slab_rejects_foreign_market_vaults",
                "v16_attack_close_slab_requires_secondary_vault_recovery",
                "v16_program_close_slab_refunds_exact_vault_and_market_excess_rent_after_normal_exit",
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-070 composes exact engine proofs and must reopen on a pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the same certified engine revision",
    );

    let witness_sources = [
        include_str!("inv_005_authority_incarnation_binding.rs"),
        include_str!("inv_017_signer_writable_role_and_account_alias_safety.rs"),
        include_str!("inv_018_quote_mint_vault_token_program_and_authority_integrity.rs"),
        include_str!("inv_034_domain_and_instance_isolation.rs"),
        include_str!("inv_063_backing_expiry_normalization.rs"),
        include_str!("inv_070_zero_unattributed_terminal_residue_and_close_slab.rs"),
        include_str!("inv_077_bounded_work_and_maximum_shape_compute.rs"),
        include_str!("../stateful/inv_063_backing_expiry_normalization.rs"),
        include_str!("../stateful/inv_066_resolved_payout_fairness_and_order_independence.rs"),
        include_str!("../stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs"),
    ];
    let mut classes = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    for row in CLASSES {
        assert!(classes.insert(row.class), "duplicate terminal stock class");
        assert!(!row.public_witnesses.is_empty());
        for proof in row.engine_proofs {
            assert!(proofs.insert(*proof), "duplicate engine proof {proof}");
            assert!(proof.starts_with("proof_v16_"));
        }
        for witness in row.public_witnesses {
            assert!(
                witness_sources
                    .iter()
                    .any(|source| inv070_source_defines_test(source, witness)),
                "terminal class '{}' lacks executable public witness {witness}",
                row.class,
            );
        }
    }
    assert_eq!(classes.len(), 6, "terminal stock class roster drift");
    assert_eq!(proofs.len(), 12, "terminal engine proof roster drift");

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    let body = inv070_braced_body_after(production, "fn handle_close_slab<'a>");
    for required in [
        "expect_live_authority(&cfg.marketauth, admin_dest.key)",
        "require_authority_epoch_view(&group, 0, expected_authority_epoch)",
        "group.header.mode != 1",
        "group.header.c_tot.get() != 0",
        "group.header.materialized_portfolio_count.get() != 0",
        "verify_vault_token_account(vault_token, &vault_authority, &primary_mint)",
        "verify_user_token_account(dest_token, admin_dest.key, &primary_mint)",
        ".advance_terminal_slab_not_atomic(authenticated_slot, scan_start)",
        "TerminalSlabOutcomeV16::ScanProgress",
        "TerminalSlabOutcomeV16::BackingExpired",
        "TerminalSlabOutcomeV16::InsuranceRecredited",
        "TerminalSlabOutcomeV16::ReadyToClose",
        ".checked_sub(retired_u64)",
        "burn_tokens_signed(",
        "transfer_tokens_signed(",
        "spl_token::instruction::close_account(",
        "market_ai.realloc(constants::HEADER_LEN, false)",
        "state::write_closed_market_tombstone",
    ] {
        assert!(
            body.contains(required),
            "CloseSlab lost boundary {required}"
        );
    }

    let engine = body
        .find(".advance_terminal_slab_not_atomic(authenticated_slot, scan_start)")
        .expect("terminal engine transition");
    for validation in [
        "verify_vault_token_account(vault_token, &vault_authority, &primary_mint)",
        "verify_user_token_account(dest_token, admin_dest.key, &primary_mint)",
    ] {
        assert!(
            body.find(validation).expect("custody validation") < engine,
            "{validation} must precede terminal engine mutation",
        );
    }
    for effect in [
        "burn_tokens_signed(",
        "transfer_tokens_signed(",
        "spl_token::instruction::close_account(",
        "market_ai.realloc(constants::HEADER_LEN, false)",
        "state::write_closed_market_tombstone",
    ] {
        assert!(
            engine < body.find(effect).expect("terminal external effect"),
            "{effect} must remain after the engine reaches ReadyToClose",
        );
    }
    assert_eq!(
        body.matches(".advance_terminal_slab_not_atomic(").count(),
        1,
        "CloseSlab must have one canonical engine transition",
    );
}
