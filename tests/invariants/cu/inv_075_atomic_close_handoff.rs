//! INV-075/076: a same-transaction close-barrier handoff commits all or nothing.
//! Public System/SPL/ATA/wrapper setup; no injected engine or token state.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const SIZE_Q: u128 = POS_SCALE / 50;
const DEPOSITS: [u64; 4] = [10, 10, 2, 2];
const SUPPLY: u64 = 24;
const GROSS_LOSS: u128 = SIZE_Q * (300 - 100) / POS_SCALE;
const RESIDUAL: u128 = GROSS_LOSS - 2;

fn snapshot(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<(Pubkey, Option<Account>)> {
    keys.iter()
        .map(|key| (*key, env.svm.get_account(key)))
        .collect()
}

fn assert_frame(
    env: &V16CuEnv,
    before: Vec<(Pubkey, Option<Account>)>,
    fee: u64,
    changed: &[Pubkey],
) {
    for (key, mut account) in before {
        if key == env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        if !changed.contains(&key) {
            assert_eq!(env.svm.get_account(&key), account, "account frame: {key}");
        }
    }
}

fn transaction_fee(tx: &Transaction) -> u64 {
    FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures)
}

#[test]
fn v16_program_atomic_close_handoff_rolls_back_stale_continuation() {
    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 1,
            max_bankrupt_close_lifetime_slots: 2,
            public_b_chunk_atoms: 1,
            ..V16CuMarketParams::default()
        },
    );
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_permissionless_resolve_with_cu(100, 5);
    let admin = env.admin.insecure_clone();
    let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
    let mut portfolios = [Pubkey::default(); 4];
    let mut tokens = [Pubkey::default(); 4];
    for index in 0..4 {
        let owner = &owners[index];
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let key = Keypair::new();
        portfolios[index] = key.pubkey();
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
        tokens[index] = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &tokens[index],
                &admin.pubkey(),
                &[],
                DEPOSITS[index],
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();
        let cu = env
            .send(
                env.deposit_ix(key.pubkey(), DEPOSITS[index].into()),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                    AccountMeta::new(tokens[index], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .unwrap();
        assert_cu_within("close handoff public deposit", cu, CUSTODY_CU_LIMIT);
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
    for winner in 0..2 {
        let cu = env.trade_asset_with_cu(
            0,
            &owners[winner],
            portfolios[winner],
            &owners[winner + 2],
            portfolios[winner + 2],
            SIZE_Q as i128,
            100,
            0,
        );
        assert_cu_within("close handoff public position", cu, TRADE_CU_LIMIT);
    }
    for (slot, mark) in [(2, 200), (3, 300)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, mark);
        for winner in &portfolios[..2] {
            env.crank(
                *winner,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
        }
    }
    for loss in &portfolios[2..] {
        env.crank(
            *loss,
            ProgInstruction::PermissionlessCrank {
                now_slot: 3,
                observations: crank_observations(0),
            },
        );
    }
    env.svm.warp_to_slot(4);
    env.try_shutdown_asset_with_authority(&admin, 0, 4)
        .expect("freeze the public loss domain before either close starts");

    let forfeit = |env: &V16CuEnv, index: usize| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owners[index].pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[index], false),
        ],
        data: ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: env.portfolio_id(portfolios[index]),
            position_epoch: env.portfolio_position_epoch(portfolios[index]),
            asset_index: 0,
            b_delta_budget: 1,
        }
        .encode(),
    };
    let crank = |env: &V16CuEnv, index: usize| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(env.payer.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[index], false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![],
        }
        .encode(),
    };
    let custody = snapshot(
        &env,
        &[
            env.mint, env.vault, tokens[0], tokens[1], tokens[2], tokens[3],
        ],
    );
    let portfolio_ids = portfolios.map(|key| env.portfolio_id(key));
    let census = |env: &V16CuEnv, booked: [u128; 2]| {
        let (_, group) = env.market_state();
        let asset = group.assets[0];
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(asset.lifecycle, AssetLifecycleV16::Recovery);
        assert_eq!(env.svm.get_sysvar::<Clock>().slot, 4);
        assert_eq!(group.vault, SUPPLY.into());
        assert_eq!(group.insurance, 0);
        assert_eq!(group.materialized_portfolio_count, 4);
        assert_eq!(
            group.negative_pnl_account_count,
            booked.iter().filter(|&&amount| amount < RESIDUAL).count() as u64
        );
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.supply, SUPPLY);
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(env.token_amount(env.vault), SUPPLY);
        for token in tokens {
            assert_eq!(env.token_amount(token), 0);
        }
        assert_frame(env, custody.clone(), 0, &[]);
        let mut capital = 0;
        let mut equity = 0;
        for (index, key) in portfolios.iter().enumerate() {
            let account = env.portfolio_state(*key);
            assert_eq!(account.owner, owners[index].pubkey().to_bytes());
            assert_eq!(env.portfolio_id(*key), portfolio_ids[index]);
            let leg = active_leg_for_asset(&account, 0);
            let sign = if index < 2 { 1 } else { -1 };
            assert_eq!(leg.basis_pos_q, sign * SIZE_Q as i128);
            assert_eq!(leg.a_basis, ADL_ONE);
            assert_eq!(leg.loss_weight, SIZE_Q);
            assert_eq!(leg.b_snap, 0, "B remains an explicit deferred winner debit");
            assert_eq!(account.cancel_deposit_escrow.get(), 0);
            assert_eq!(account.fee_credits.get(), 0);
            let value = account.capital.get() as i128 + account.pnl.get();
            if index < 2 {
                assert_eq!(value, DEPOSITS[index] as i128 + GROSS_LOSS as i128);
            } else {
                let amount = booked[index - 2];
                assert_eq!(value, amount as i128 - RESIDUAL as i128);
                let close = close_progress(&account);
                if amount == 0 {
                    assert_eq!(close, CloseProgressLedgerV16::EMPTY);
                } else {
                    assert!(close.active && !close.canceled);
                    assert_eq!(close.finalized, amount == RESIDUAL);
                    assert_eq!(
                        close.close_id, 1,
                        "IDs are portfolio-local, not domain-global"
                    );
                    assert_eq!(
                        (close.asset_index, close.market_id, close.domain_side),
                        (0, 1, SideV16::Long)
                    );
                    assert_eq!(close.gross_loss_at_close_start, RESIDUAL);
                    assert_eq!(close.b_loss_booked, amount);
                    assert_eq!(close.residual_remaining, RESIDUAL - amount);
                    assert_eq!(
                        close.support_consumed
                            + close.insurance_spent
                            + close.explicit_loss_assigned
                            + close.drift_consumed,
                        0
                    );
                    assert_eq!(close.junior_face_burned, 0);
                    assert_eq!(close.quantity_adl_applied_q, 0);
                    assert_eq!(close.max_close_slot, close.drift_reference_slot + 2);
                    assert!(4 <= close.max_close_slot, "handoff is before expiry");
                }
            }
            capital += account.capital.get();
            equity += value;
        }
        let total_booked = booked.iter().sum::<u128>();
        assert_eq!(
            equity - total_booked as i128,
            SUPPLY as i128,
            "booked B must offset exactly the relieved debtor PnL, not mint equity"
        );
        assert_eq!(group.c_tot, capital);
        assert_eq!((asset.a_long, asset.a_short), (ADL_ONE, ADL_ONE));
        assert_eq!(
            (asset.oi_eff_long_q, asset.oi_eff_short_q),
            (2 * SIZE_Q, 2 * SIZE_Q)
        );
        assert_eq!(
            (asset.stored_pos_count_long, asset.stored_pos_count_short),
            (2, 2)
        );
        assert_eq!(
            (
                asset.pending_obligation_count_long,
                asset.pending_obligation_count_short
            ),
            (0, 0)
        );
        assert_eq!(
            (asset.loss_weight_sum_long, asset.loss_weight_sum_short),
            (2 * SIZE_Q, 2 * SIZE_Q)
        );
        assert_eq!(
            asset.b_long_num * (2 * SIZE_Q) + asset.social_loss_remainder_long_num,
            total_booked * percolator::SOCIAL_LOSS_DEN,
            "exact accumulated social-loss numerator"
        );
        assert_eq!(asset.b_short_num, 0);
        assert_eq!(asset.social_loss_remainder_short_num, 0);
        assert_eq!(
            group.pending_domain_loss_barriers[0],
            booked.iter().filter(|&&n| n != 0 && n < RESIDUAL).count() as u64
        );
        assert!(group.pending_domain_loss_barriers[1..]
            .iter()
            .all(|&n| n == 0));
    };
    census(&env, [0, 0]);
    let start_first = forfeit(&env, 2);
    let cu = send_raw_tx(&mut env.svm, &env.payer, start_first, &[&owners[2]]).unwrap();
    assert_cu_within("close handoff first owner", cu, CUSTODY_CU_LIMIT);
    census(&env, [1, 0]);
    let first_close = close_progress(&env.portfolio_state(portfolios[2]));
    let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
    let finish_first = crank(&env, 2);
    let start_second = forfeit(&env, 3);
    let bundle = |env: &V16CuEnv, suffix: Vec<Instruction>| {
        let mut instructions = vec![heap_ix(), cu_ix()];
        instructions.extend(suffix);
        Transaction::new_signed_with_payer(
            &instructions,
            Some(&env.payer.pubkey()),
            &[&env.payer, &owners[3]],
            env.svm.latest_blockhash(),
        )
    };
    let keys_for = |tx: &Transaction| {
        let mut keys = tx.message.account_keys.clone();
        keys.extend([env.market, env.mint, env.vault, admin.pubkey()]);
        keys.extend(portfolios);
        keys.extend(tokens);
        keys.extend(owners.iter().map(Signer::pubkey));
        keys.sort_unstable();
        keys.dedup();
        keys
    };
    let reverse = bundle(&env, vec![start_second.clone(), finish_first.clone()]);
    let keys = keys_for(&reverse);
    let mut peak_cu = cu;
    for (tx, index, error, completed) in [
        (reverse, 2, PercolatorError::EngineLockActive, 0),
        (
            bundle(
                &env,
                vec![
                    finish_first.clone(),
                    start_second.clone(),
                    start_second.clone(),
                ],
            ),
            4,
            PercolatorError::EngineProvenanceMismatch,
            2,
        ),
    ] {
        let before = snapshot(&env, &keys_for(&tx));
        let fee = transaction_fee(&tx);
        let failure = env
            .svm
            .send_transaction(tx)
            .expect_err("invalid handoff order or stale continuation");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            completed,
            "the stale tail must fail only after finalization and replacement ownership succeeded"
        );
        assert_frame(&env, before, fee, &[]);
        assert_cu_within(
            "close handoff rejected bundle",
            failure.meta.compute_units_consumed,
            CRANK_CU_LIMIT + 2 * CUSTODY_CU_LIMIT,
        );
        peak_cu = peak_cu.max(failure.meta.compute_units_consumed);
        census(&env, [1, 0]);
        assert_eq!(
            close_progress(&env.portfolio_state(portfolios[2])),
            first_close
        );
        assert_eq!(
            portfolios.map(|key| env.portfolio_position_epoch(key)),
            epochs
        );
    }

    // Remove only the repeated stale instruction: both original prefix instructions land unchanged.
    let valid = bundle(&env, vec![finish_first, start_second]);
    let fee = transaction_fee(&valid);
    let before = snapshot(&env, &keys);
    let accepted = env
        .svm
        .send_transaction(valid)
        .expect("finalization hands off the barrier atomically");
    assert_cu_within(
        "close handoff accepted bundle",
        accepted.compute_units_consumed,
        CRANK_CU_LIMIT + CUSTODY_CU_LIMIT,
    );
    peak_cu = peak_cu.max(accepted.compute_units_consumed);
    assert_frame(
        &env,
        before,
        fee,
        &[env.market, portfolios[2], portfolios[3]],
    );
    census(&env, [2, 1]);
    let first_final = close_progress(&env.portfolio_state(portfolios[2]));
    let second_close = close_progress(&env.portfolio_state(portfolios[3]));
    assert_eq!(
        inv075_close_episode_key(first_final),
        inv075_close_episode_key(first_close)
    );
    assert_eq!(
        portfolios.map(|key| env.portfolio_position_epoch(key)),
        [epochs[0], epochs[1], epochs[2], epochs[3] + 1]
    );

    let finish_second = crank(&env, 3);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), finish_second],
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    let fee = transaction_fee(&tx);
    let before = snapshot(&env, &keys);
    let finalization = env
        .svm
        .send_transaction(tx)
        .expect("new barrier owner retains permissionless progress");
    assert_cu_within(
        "close handoff second finalization",
        finalization.compute_units_consumed,
        CRANK_CU_LIMIT,
    );
    peak_cu = peak_cu.max(finalization.compute_units_consumed);
    assert_frame(&env, before, fee, &[env.market, portfolios[3]]);
    census(&env, [2, 2]);
    assert_eq!(
        close_progress(&env.portfolio_state(portfolios[2])),
        first_final
    );
    assert_eq!(
        inv075_close_episode_key(close_progress(&env.portfolio_state(portfolios[3]))),
        inv075_close_episode_key(second_close)
    );
    assert_eq!(
        portfolios.map(|key| env.portfolio_position_epoch(key)),
        [epochs[0], epochs[1], epochs[2], epochs[3] + 1]
    );
    println!("INV-075/076 atomic handoff: 2 exact rollbacks, 4 loss atoms booked once, 0 residual/barriers, 24 SPL atoms, peak {peak_cu} CU");
}
