//! INV-073/077/078: last-asset recovery past a publicly retired maximum-market prefix.

use super::*;
use crate::support::fuzz_model::assert_market_stock_census;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_max_market_retired_siblings_preserve_oversized_force_close_and_owner_exit() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;

    const N: usize = MAX_10M_MARKET_SLOTS;
    const LAST: u16 = (N - 1) as u16;
    const SHUTDOWN_SLOT: u64 = N as u64;
    const CAPITAL: u64 = 1_009;
    const PRICE: u64 = 100;
    const CU_BOUND: u64 = 400_000;
    let mut peak = 0;
    let mut measured = |cu| {
        assert_cu_within("maximum retired-prefix recovery", cu, CU_BOUND);
        peak = peak.max(cu);
    };
    let len = state::market_account_len_for_capacity(N).unwrap();
    assert!(len <= 10 * 1024 * 1024);
    assert!(state::market_account_len_for_capacity(N + 1).unwrap() > 10 * 1024 * 1024);
    let mut env = inv018_public_spl_market_with_capacity(0, V16CuMarketParams::default(), N);
    measured(env.init_market_cu);
    env.svm.warp_to_slot(1);
    // Append before retiring: the public admission rule requires reuse of free slots.
    for asset in 1..=LAST {
        measured(env.activate_asset(asset, u64::from(asset) + 1, PRICE));
    }
    for asset in 1..LAST {
        measured(env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_RETIRE,
            asset,
            SHUTDOWN_SLOT,
            0,
        ));
    }
    let (cfg, group) = env.market_state();
    assert_eq!(env.svm.get_account(&env.market).unwrap().data.len(), len);
    assert_eq!(group.config.max_market_slots as usize, N);
    assert_eq!(cfg.free_market_slot_count as usize, N - 2);
    assert!(group.assets[1..N - 1]
        .iter()
        .all(|asset| asset.lifecycle == AssetLifecycleV16::Retired));
    measured(env.configure_auth_mark_for_asset_as_admin(0, SHUTDOWN_SLOT, PRICE));
    measured(env.configure_permissionless_resolve_with_cu(1_000, 4));
    measured(env.configure_auth_mark_for_asset_as_admin(LAST, SHUTDOWN_SLOT, PRICE));

    let owners = [Keypair::new(), Keypair::new()];
    let keys = [Keypair::new(), Keypair::new()];
    let portfolios = keys.each_ref().map(|key| key.pubkey());
    let tokens = owners
        .each_ref()
        .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
    for i in 0..2 {
        env.svm.airdrop(&owners[i].pubkey(), 1_000_000_000).unwrap();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &keys[i],
            env.portfolio_account_len,
            env.program_id,
        );
        measured(
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                ],
                &[&owners[i]],
            )
            .unwrap(),
        );
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &tokens[i],
                &env.admin.pubkey(),
                &[],
                CAPITAL,
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        measured(
            env.send(
                env.deposit_ix(portfolios[i], CAPITAL.into()),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .unwrap(),
        );
    }
    measured(env.trade_asset_with_cu(
        LAST,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        (2 * POS_SCALE) as i128,
        PRICE,
        0,
    ));
    measured(env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        LAST,
        SHUTDOWN_SLOT,
        0,
    ));
    let frozen = env.svm.get_account(&env.market).unwrap();
    let rank = |env: &V16CuEnv| {
        let market = env.svm.get_account(&env.market).unwrap();
        let (cfg, group) = state::read_market(&market.data).unwrap();
        let accounts = portfolios.map(|key| env.portfolio_state(key));
        assert_market_stock_census(
            "retired-prefix recovery",
            &group,
            &market.data,
            &accounts,
            env.token_amount(env.vault).into(),
        )
        .unwrap();
        assert_eq!(cfg.free_market_slot_count as usize, N - 2);
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(group.assets[N - 1].lifecycle, AssetLifecycleV16::Recovery);
        for asset in 0..N - 1 {
            assert_eq!(
                market_engine_slot_bytes(&market.data, asset),
                market_engine_slot_bytes(&frozen.data, asset),
                "sibling {asset}"
            );
        }
        let exposure: u128 = accounts
            .iter()
            .flat_map(|a| a.legs.iter())
            .filter(|leg| leg.active != 0)
            .map(|leg| leg.basis_pos_q.get().unsigned_abs())
            .sum();
        let capital = accounts.iter().map(|a| a.capital.get()).sum::<u128>();
        assert_eq!(
            exposure,
            group
                .assets
                .iter()
                .map(|asset| asset.oi_eff_long_q + asset.oi_eff_short_q)
                .sum::<u128>()
        );
        assert!(accounts.iter().all(|a| a.pnl.get() == 0));
        [exposure, capital]
    };
    assert_eq!(rank(&env), [4 * POS_SCALE, 2 * u128::from(CAPITAL)]);

    let force = |env: &mut V16CuEnv, close_q, error: Option<PercolatorError>| {
        env.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                cu_ix(),
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                        AccountMeta::new(portfolios[1], false),
                    ],
                    data: ProgInstruction::ForceCloseAbandonedAsset {
                        asset_index: LAST,
                        now_slot: env.svm.get_sysvar::<Clock>().slot,
                        close_q,
                    }
                    .encode(),
                },
            ],
            Some(&env.payer.pubkey()),
            &[&env.payer],
            env.svm.latest_blockhash(),
        );
        assert_eq!(tx.message.header.num_required_signatures, 1);
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        let mut tracked = tx.message.account_keys.clone();
        tracked.extend([env.vault, env.mint, tokens[0], tokens[1]]);
        let before: Vec<_> = tracked.iter().map(|key| env.svm.get_account(key)).collect();
        let result = env.svm.send_transaction(tx);
        if let Some(error) = error {
            let failed = result.expect_err("force close must wait for the authenticated deadline");
            assert_eq!(
                failed.err,
                TransactionError::InstructionError(2, InstructionError::Custom(error as u32),)
            );
            for (key, mut expected) in tracked.iter().zip(before) {
                if *key == env.payer.pubkey() {
                    expected.as_mut().unwrap().lamports -=
                        FeeStructure::default().lamports_per_signature;
                }
                assert_eq!(env.svm.get_account(key), expected, "rollback {key}");
            }
            failed.meta.compute_units_consumed
        } else {
            result
                .expect("keeper-only last-asset recovery")
                .compute_units_consumed
        }
    };
    env.svm.warp_to_slot(SHUTDOWN_SLOT + 3);
    measured(force(
        &mut env,
        u128::MAX,
        Some(PercolatorError::EngineLockActive),
    ));
    env.svm.warp_to_slot(SHUTDOWN_SLOT + 4);
    for (request, remaining) in [(POS_SCALE, 2 * POS_SCALE), (u128::MAX, 0)] {
        let before = rank(&env);
        measured(force(&mut env, request, None));
        let after = rank(&env);
        assert!(
            after < before,
            "each accepted keeper call consumes real exposure"
        );
        assert_eq!(after, [remaining, 2 * u128::from(CAPITAL)]);
    }
    for i in 0..2 {
        let before = rank(&env);
        measured(
            env.send(
                env.withdraw_ix(portfolios[i], CAPITAL.into()),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .unwrap(),
        );
        assert!(rank(&env) < before);
        assert_eq!(env.token_amount(tokens[i]), CAPITAL);
    }
    assert_eq!(rank(&env), [0, 0]);
    assert_eq!(env.token_amount(env.vault), 0);
    println!("INV-077 public retired-prefix recovery: assets={N}, retired={}, keeper_steps=2, rollbacks=1, peak_cu={peak}, bound={CU_BOUND}, tx_ceiling=1400000", N - 2);
}
