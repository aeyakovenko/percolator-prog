//! INV-070/088, row 424: public reuse cannot introduce an asset behind a terminal prefix.
//! Live simulations establish valid admin and permissionless reuse instructions. Once
//! resolved scanning has passed the retired slot, both routes must reject atomically,
//! including after a successful expiry prefix, and leave a bounded terminal exit.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_terminal_prefix_rejects_retired_slot_reuse_with_exact_rollback() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;

    const BOOKED: u64 = 17;
    const INIT_FEE: u64 = 1;
    const EXPIRY: u64 = 20;
    const CU_LIMIT: u64 = 300_000;
    let mut env = inv018_public_spl_market_with_capacity(0, V16CuMarketParams::default(), 3);
    let admin = env.admin.insecure_clone();
    let creator = Keypair::new();
    env.svm.airdrop(&creator.pubkey(), 1_000_000_000).unwrap();
    let destination = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    let creator_token = create_ata_for_test(&mut env.svm, &env.payer, creator.pubkey(), env.mint);
    for (token, amount) in [(destination, BOOKED), (creator_token, INIT_FEE)] {
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
    env.update_market_init_fee_policy_with_cu(INIT_FEE.into());
    env.activate_asset(1, 1, 100);
    env.activate_asset(2, 2, 100);
    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_RETIRE, 1, 3, 0);
    env.svm.warp_to_slot(4);
    env.top_up_backing_bucket_from_admin_token_with_cu(destination, 4, BOOKED.into(), EXPIRY);
    env.svm.warp_to_slot(5);

    let retired_id = env.asset_market_id(1);
    let next_id = env.market_state().1.next_market_id;
    let admin_epoch = env.control_sequences(0).authority_epoch;
    let reuse = [&admin, &creator].map(|signer| {
        let permissionless = signer.pubkey() == creator.pubkey();
        let mut accounts = vec![
            AccountMeta::new(signer.pubkey(), true),
            AccountMeta::new(env.market, false),
        ];
        if permissionless {
            accounts.extend([
                AccountMeta::new(creator_token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ]);
        }
        Instruction {
            program_id: env.program_id,
            accounts,
            data: ProgInstruction::UpdateAssetLifecycle {
                action: processor::ASSET_ACTION_ACTIVATE,
                asset_index: 1,
                market_id: next_id,
                authority_epoch: if permissionless { 0 } else { admin_epoch },
                now_slot: 5,
                initial_price: 100,
                max_init_fee: INIT_FEE.into(),
                insurance_authority: signer.pubkey().to_bytes(),
                insurance_operator: signer.pubkey().to_bytes(),
                backing_bucket_authority: signer.pubkey().to_bytes(),
                oracle_authority: signer.pubkey().to_bytes(),
            }
            .encode(),
        }
    });
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
            authority_epoch: admin_epoch,
        }
        .encode(),
    };
    let transaction = |env: &V16CuEnv, instructions: &[Instruction], signers: &[&Keypair]| {
        let mut ixs = vec![heap_ix(), cu_ix()];
        ixs.extend_from_slice(instructions);
        let mut all_signers = vec![&env.payer];
        all_signers.extend_from_slice(signers);
        let tx = Transaction::new_signed_with_payer(
            &ixs,
            Some(&env.payer.pubkey()),
            &all_signers,
            env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        tx
    };
    let tracked = [
        env.market,
        env.vault,
        env.mint,
        destination,
        creator_token,
        admin.pubkey(),
        creator.pubkey(),
    ];
    let keys = |tx: &Transaction| {
        let mut keys = tx.message.account_keys.clone();
        keys.extend(tracked);
        keys.sort_unstable();
        keys.dedup();
        keys
    };
    let frame = |env: &V16CuEnv, keys: &[Pubkey]| {
        keys.iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>()
    };
    let success_log = format!("Program {} success", env.program_id);
    let mut peak_cu = 0;
    let mut check_cu = |cu| {
        assert_cu_within("INV-070 terminal prefix reuse", cu, CU_LIMIT);
        peak_cu = peak_cu.max(cu);
    };

    // A current generation, complete authorities, and funded fee make both live routes actionable.
    for (ix, signer) in reuse.iter().zip([&admin, &creator]) {
        let tx = transaction(&env, std::slice::from_ref(ix), &[signer]);
        let keys = keys(&tx);
        let before = frame(&env, &keys);
        let preview = env
            .svm
            .simulate_transaction(tx.into())
            .expect("valid live retired-slot reuse");
        assert_eq!(
            preview
                .logs
                .iter()
                .filter(|log| **log == success_log)
                .count(),
            1
        );
        let token_success = format!("Program {} success", spl_token::ID);
        assert_eq!(
            preview
                .logs
                .iter()
                .filter(|log| **log == token_success)
                .count(),
            usize::from(signer.pubkey() == creator.pubkey()),
            "permissionless live reuse actually executes the funded init-fee transfer"
        );
        assert_eq!(frame(&env, &keys), before);
        check_cu(preview.compute_units_consumed);
    }
    check_cu(env.resolve());
    let stock = |env: &V16CuEnv, fresh: bool, cursor: u128| {
        let market = env.svm.get_account(&env.market).unwrap();
        let (cfg, group) = state::read_market(&market.data).unwrap();
        assert_eq!(cfg.terminal_slab_scan_progress, cursor);
        assert_eq!(cfg.free_market_slot_count, 1);
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.next_market_id, next_id);
        assert_eq!(group.assets[1].market_id, retired_id);
        assert_eq!(group.assets[1].lifecycle, AssetLifecycleV16::Retired);
        assert_eq!(group.materialized_portfolio_count, 0);
        assert_eq!((group.c_tot, group.pnl_pos_tot, group.insurance), (0, 0, 0));
        assert_eq!(group.vault, BOOKED.into());
        assert_eq!(env.token_amount(env.vault), BOOKED);
        assert_eq!(env.token_amount(destination), 0);
        assert_eq!(env.token_amount(creator_token), INIT_FEE);
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, BOOKED + INIT_FEE);
        assert_eq!(mint.mint_authority, COption::None);
        for (domain, (bucket, source)) in group
            .source_backing_buckets
            .iter()
            .zip(&group.source_credit)
            .enumerate()
        {
            let expected = if fresh && domain == 4 {
                u128::from(BOOKED) * BOUND_SCALE
            } else {
                0
            };
            assert_eq!(bucket.fresh_unliened_backing_num, expected);
            assert_eq!(source.fresh_reserved_backing_num, expected);
            assert_eq!(bucket.utilization_fee_earnings, 0);
            assert_eq!(source.positive_claim_bound_num, 0);
            assert_eq!(source.valid_liened_backing_num, 0);
        }
        assert_eq!(
            group.source_backing_buckets[4].status,
            if fresh {
                BackingBucketStatusV16::Fresh
            } else {
                BackingBucketStatusV16::Expired
            }
        );
        crate::support::fuzz_model::assert_market_stock_census(
            "INV-070 terminal prefix reuse",
            &group,
            &market.data,
            &[],
            BOOKED.into(),
        )
        .unwrap();
        crate::support::fuzz_model::assert_reservation_encumbrance_census(
            "INV-070 terminal prefix reuse",
            &group,
            &[],
        )
        .unwrap();
    };
    stock(&env, true, 0);
    let custody_keys = [
        env.vault,
        env.mint,
        destination,
        creator_token,
        admin.pubkey(),
        creator.pubkey(),
    ];
    let custody_before = frame(&env, &custody_keys);
    let send_close = |env: &mut V16CuEnv| {
        env.svm.expire_blockhash();
        let tx = transaction(env, std::slice::from_ref(&close), &[&admin]);
        env.svm
            .send_transaction(tx)
            .expect("bounded terminal continuation")
            .compute_units_consumed
    };
    check_cu(send_close(&mut env));
    stock(&env, true, 2);
    assert_eq!(frame(&env, &custody_keys), custody_before);
    let start = MARKET_GROUP_OFF + std::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
    let end = start + 2 * std::mem::size_of::<percolator::Market<state::AssetOracleStorageV16>>();
    let scanned_prefix = env.svm.get_account(&env.market).unwrap().data[start..end].to_vec();

    for expire_first in [false, true] {
        env.svm.warp_to_slot(if expire_first { EXPIRY } else { 5 });
        for (ix, signer) in reuse.iter().zip([&admin, &creator]) {
            env.svm.expire_blockhash();
            let instructions = if expire_first {
                vec![close.clone(), ix.clone()]
            } else {
                vec![ix.clone()]
            };
            let signers = if expire_first && signer.pubkey() != admin.pubkey() {
                vec![&admin, signer]
            } else {
                vec![signer]
            };
            let tx = transaction(&env, &instructions, &signers);
            let keys = keys(&tx);
            let mut expected = frame(&env, &keys);
            let payer_index = keys
                .iter()
                .position(|key| *key == env.payer.pubkey())
                .unwrap();
            expected[payer_index].as_mut().unwrap().lamports -=
                u64::from(tx.message.header.num_required_signatures)
                    * FeeStructure::default().lamports_per_signature;
            let error = env
                .svm
                .send_transaction(tx)
                .expect_err("resolved reuse behind the cursor must reject");
            assert_eq!(
                error.err,
                TransactionError::InstructionError(
                    2 + u8::from(expire_first),
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                )
            );
            assert_eq!(
                error
                    .meta
                    .logs
                    .iter()
                    .filter(|log| **log == success_log)
                    .count(),
                usize::from(expire_first)
            );
            assert_eq!(
                frame(&env, &keys),
                expected,
                "exact account rollback except signature fees"
            );
            stock(&env, true, 2);
            assert_eq!(
                env.market_state().1.current_slot,
                5,
                "staged expiry and engine time roll back together"
            );
            check_cu(error.meta.compute_units_consumed);
        }
    }

    // Two committed calls suffice after maturity: normalize the blocker, then close.
    check_cu(send_close(&mut env));
    stock(&env, false, 2);
    assert_eq!(env.market_state().1.current_slot, EXPIRY);
    assert_eq!(
        &env.svm.get_account(&env.market).unwrap().data[start..end],
        scanned_prefix.as_slice()
    );
    assert_eq!(frame(&env, &custody_keys), custody_before);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
    let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
    let mut mint = Mint::unpack(&expected_mint.data).unwrap();
    mint.supply = INIT_FEE;
    Mint::pack(mint, &mut expected_mint.data).unwrap();
    let final_frame_keys = [destination, creator_token, creator.pubkey()];
    let final_frame = frame(&env, &final_frame_keys);
    check_cu(send_close(&mut env));
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(
        tombstone.lamports,
        env.svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
    );
    expected_admin.lamports += market_before.lamports + vault_before.lamports - tombstone.lamports;
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
    assert_eq!(frame(&env, &final_frame_keys), final_frame);
    assert!(env
        .svm
        .get_account(&env.vault)
        .is_none_or(|account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)));
    eprintln!("INV-070 prefix reuse: 2 live previews, 4 exact rejections, 3 committed close calls, burn={BOOKED}, peak={peak_cu} CU");
}
