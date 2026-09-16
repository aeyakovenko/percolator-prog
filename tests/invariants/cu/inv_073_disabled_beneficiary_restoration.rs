//! Row 421 / INV-073: disabled custody, rejected beneficiary burns and consented
//! A -> B -> A restoration across senior exit. All economic setup and transitions
//! use public instructions. No provider expiry or insurance recredit is involved.
//! Final residue here is external SPL/native surplus, not booked claim residue.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const CAPITAL: u64 = 137;
const INSURANCE: u64 = 47;
const PREFIX: u64 = 7;
const SURPLUS: u64 = 11;
const RAW_NATIVE: u64 = 13;

fn handoff(env: &V16CuEnv, from: Pubkey, to: Pubkey, epoch: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(from, true),
            AccountMeta::new_readonly(to, to != Pubkey::default()),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: epoch,
            kind: processor::ASSET_AUTH_INSURANCE,
            new_pubkey: to.to_bytes(),
        }
        .encode(),
    }
}

fn payout(env: &V16CuEnv, holder: Pubkey, token: Pubkey, amount: u64, epoch: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(holder, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: epoch,
            amount: amount.into(),
        }
        .encode(),
    }
}

fn close(env: &V16CuEnv, admin: Pubkey, token: Pubkey, signed: bool) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin, signed),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: env.control_sequences(0).authority_epoch,
        }
        .encode(),
    }
}

fn retire(env: &V16CuEnv, authority: Pubkey, signed: bool) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(authority, signed),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_RETIRE,
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            now_slot: 1,
            initial_price: 0,
            max_init_fee: 0,
            insurance_authority: [0; 32],
            insurance_operator: [0; 32],
            backing_bucket_authority: [0; 32],
            oracle_authority: [0; 32],
        }
        .encode(),
    }
}

fn token_image(empty: &Account, amount: u64, raw: u64) -> Account {
    let mut expected = empty.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.amount, 0);
    token.amount = amount;
    if token.is_native.is_some() {
        expected.lamports += amount + raw;
    } else {
        assert_eq!(raw, 0);
    }
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected
}

fn beneficiary_image(empty: &Account, amount: u64, delegate: Pubkey, disabled: bool) -> Account {
    let mut expected = token_image(empty, amount, 0);
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    token.delegate = COption::Some(delegate);
    token.delegated_amount = 1;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    if !disabled {
        // SPL revocation clears the COption tag but retains its inactive payload.
        token.delegate = COption::None;
        token.delegated_amount = 0;
        TokenAccount::pack(token, &mut expected.data).unwrap();
    }
    expected
}

// The shared runner checks exact errors, completed wrapper prefixes and full
// rollback. Add success-side Account frames and conservation including rent/fees.
fn land(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    failure: Option<(u8, u32, usize)>,
) -> u64 {
    let mut keys = tracked.to_vec();
    keys.push(env.payer.pubkey());
    keys.extend(
        ixs.iter()
            .flat_map(|ix| ix.accounts.iter().map(|a| a.pubkey)),
    );
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let fee = (1 + signers.len()) as u64 * FeeStructure::default().lamports_per_signature;
    let total_before: u128 = before
        .iter()
        .flatten()
        .map(|a| u128::from(a.lamports))
        .sum();
    let cu = insurance_succession_tx(env, ixs, signers, tracked, failure);
    for (key, mut expected) in keys.iter().zip(before) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        } else if failure.is_none() && changed.contains(key) {
            continue;
        }
        assert_eq!(env.svm.get_account(key), expected, "Account frame {key}");
    }
    assert_eq!(
        keys.iter()
            .filter_map(|key| env.svm.get_account(key))
            .map(|a| u128::from(a.lamports))
            .sum::<u128>(),
        total_before - u128::from(fee),
        "all touched lamports including rent and network fees"
    );
    cu
}

#[test]
fn v16_program_disabled_beneficiary_restoration_preserves_seniors_epochs_and_final_surplus() {
    let mut peaks = [0; 2];
    let mut rejections = 0;
    for native in [false, true] {
        for handoff_before_exit in [false, true] {
            let (peak, rejected) = run(native, handoff_before_exit);
            peaks[usize::from(native)] = peaks[usize::from(native)].max(peak);
            rejections += rejected;
        }
    }
    assert_eq!(rejections, 68);
    eprintln!("INV-073 disabled beneficiary restoration: worlds=4, exact_rollbacks={rejections}, classic_peak={}, native_peak={}", peaks[0], peaks[1]);
}

fn run(native: bool, handoff_before_exit: bool) -> (u64, usize) {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    let mut env = if native {
        inv081_public_native_market()
    } else {
        inv018_public_spl_market_with_params(0, V16CuMarketParams::default())
    };
    let admin = env.admin.insecure_clone();
    let a = Keypair::new();
    let b = Keypair::new();
    let owner = Keypair::new();
    let delegate = Keypair::new();
    for key in [&a, &b, &owner, &delegate] {
        env.svm.airdrop(&key.pubkey(), 1_000_000_000).unwrap();
    }
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&a),
        0,
        processor::ASSET_AUTH_INSURANCE,
        a.pubkey().to_bytes(),
    )
    .unwrap();
    let wallets = [a.pubkey(), b.pubkey(), owner.pubkey(), admin.pubkey()];
    let tokens = wallets.map(|key| create_ata_for_test(&mut env.svm, &env.payer, key, env.mint));
    let empty = [tokens[0], tokens[1], tokens[2], tokens[3], env.vault]
        .map(|key| env.svm.get_account(&key).unwrap());
    for (index, amount) in [(0, INSURANCE), (2, CAPITAL), (3, SURPLUS)] {
        if native {
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    system_instruction::transfer(&wallets[index], &tokens[index], amount),
                    spl_token::instruction::sync_native(&spl_token::ID, &tokens[index]).unwrap(),
                ],
                &[if index == 0 {
                    &a
                } else if index == 2 {
                    &owner
                } else {
                    &admin
                }],
            )
            .unwrap();
        } else {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[index],
                    &admin.pubkey(),
                    &[],
                    amount,
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
        }
    }
    if !native {
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
    }
    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio,
        env.portfolio_account_len,
        env.program_id,
    );
    let portfolio = portfolio.pubkey();
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
    env.send(
        env.deposit_ix(portfolio, CAPITAL.into()),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(tokens[2], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    )
    .unwrap();
    for (domain, amount) in [(0, 19u64), (1, 28)] {
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                domain,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: env.control_sequences(0).insurance_top_up + 1,
                amount: amount.into(),
            },
            vec![
                AccountMeta::new(a.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&a],
        )
        .unwrap();
    }
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::transfer(
            &spl_token::ID,
            &tokens[3],
            &env.vault,
            &admin.pubkey(),
            &[],
            SURPLUS,
        )
        .unwrap(),
        &[&admin],
    )
    .unwrap();
    if native {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            system_instruction::transfer(&admin.pubkey(), &env.vault, RAW_NATIVE),
            &[&admin],
        )
        .unwrap();
    }
    env.svm.warp_to_slot(1);
    env.resolve();
    let original_epoch = env.control_sequences(0).authority_epoch;
    let original_sequences = env.control_sequences(0);
    let original_config = env.market_state().0;
    let original_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let retained_payout = payout(
        &env,
        a.pubkey(),
        tokens[0],
        INSURANCE - PREFIX,
        original_epoch,
    );
    let retained_handoff = handoff(&env, a.pubkey(), b.pubkey(), original_epoch);
    let mint_frame = env.svm.get_account(&env.mint);
    if !native {
        let mint = Mint::unpack(&mint_frame.as_ref().unwrap().data).unwrap();
        assert_eq!(mint.supply, CAPITAL + INSURANCE + SURPLUS);
        assert_eq!(mint.mint_authority, COption::None);
    }
    let portfolio_rent = env.svm.get_account(&portfolio).unwrap().lamports;
    let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
    let tracked: Vec<_> = [
        env.market,
        env.vault,
        env.mint,
        env.vault_authority,
        portfolio,
        delegate.pubkey(),
    ]
    .into_iter()
    .chain(wallets)
    .chain(tokens)
    .collect();
    let mut peak = 0;
    let mut rejected = 0;
    let mut execute = |env: &mut V16CuEnv,
                       ixs: &[Instruction],
                       signers: &[&Keypair],
                       changed: &[Pubkey],
                       failure: Option<(u8, u32, usize)>| {
        peak = peak.max(land(env, ixs, signers, &tracked, changed, failure));
        rejected += usize::from(failure.is_some());
    };
    let market = env.market;
    let vault = env.vault;
    let fail = |index, error: PercolatorError, successes| Some((index, error as u32, successes));
    let user_exit = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(tokens[2], false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
    };
    let mut disabled = true;
    let mut user_paid = false;
    let mut deleted = false;
    let mut paid = [0u64; 2];
    let mut holder = a.pubkey();
    let mut epoch = original_epoch;
    let check = |env: &V16CuEnv,
                 disabled: bool,
                 user_paid: bool,
                 deleted: bool,
                 paid: [u64; 2],
                 holder: Pubkey,
                 epoch: u64| {
        let group = env.market_state().1;
        let capital = if user_paid { 0 } else { CAPITAL };
        let insurance = INSURANCE - paid.iter().sum::<u64>();
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (group.c_tot, group.insurance, group.vault),
            (
                capital.into(),
                insurance.into(),
                (capital + insurance).into()
            )
        );
        assert_eq!(group.materialized_portfolio_count, u64::from(!deleted));
        assert_eq!(group.pnl_pos_tot, 0);
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert!(group.insurance_domain_spent.iter().all(|n| *n == 0));
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            insurance.into()
        );
        let total_paid = paid.iter().sum::<u64>();
        assert_eq!(
            &group.insurance_domain_budget[..2],
            &[
                u128::from(19u64.saturating_sub(total_paid)),
                u128::from(28 - total_paid.saturating_sub(19)),
            ]
        );
        assert_eq!(env.control_sequences(0).authority_epoch, epoch);
        let mut expected_sequences = original_sequences;
        expected_sequences.authority_epoch = epoch;
        assert_eq!(env.control_sequences(0), expected_sequences);
        assert_eq!(env.market_state().0, original_config);
        let data = env.svm.get_account(&market).unwrap().data;
        let profile = state::read_asset_oracle_profile(&data, 0).unwrap();
        let mut expected_profile = original_profile;
        expected_profile.insurance_authority = holder.to_bytes();
        assert_eq!(profile, expected_profile);
        let ps = if deleted {
            vec![]
        } else {
            vec![env.portfolio_state(portfolio)]
        };
        assert_market_stock_census(
            "disabled beneficiary restoration",
            &group,
            &data,
            &ps,
            // External surplus is reconciled against real custody below; the
            // shared census requires the booked vault quantity as its input.
            u128::from(capital + insurance),
        )
        .unwrap();
        assert_reservation_encumbrance_census("disabled beneficiary restoration", &group, &ps)
            .unwrap();
        for (index, amount) in [
            paid[0],
            paid[1],
            if user_paid { CAPITAL } else { 0 },
            0,
            capital + insurance + SURPLUS,
        ]
        .into_iter()
        .enumerate()
        {
            let mut expected = token_image(
                &empty[index],
                amount,
                if native && index == 4 { RAW_NATIVE } else { 0 },
            );
            if index == 0 {
                expected = beneficiary_image(&empty[0], amount, delegate.pubkey(), disabled);
            }
            assert_eq!(
                env.svm
                    .get_account(&if index == 4 { vault } else { tokens[index] }),
                Some(expected),
                "custody index={index}, disabled={disabled}, paid={paid:?}"
            );
        }
        assert_eq!(env.svm.get_account(&env.mint), mint_frame);
        assert_eq!(
            env.svm.get_account(&market).unwrap().lamports,
            market_rent + if deleted { portfolio_rent } else { 0 }
        );
    };
    let approve = spl_token::instruction::approve(
        &spl_token::ID,
        &tokens[0],
        &delegate.pubkey(),
        &a.pubkey(),
        &[],
        1,
    )
    .unwrap();
    execute(&mut env, &[approve], &[&a], &[tokens[0]], None);
    check(&env, disabled, user_paid, deleted, paid, holder, epoch);
    execute(
        &mut env,
        &[retained_payout.clone()],
        &[],
        &[],
        fail(2, PercolatorError::InvalidTokenAccount, 0),
    );

    // A successful senior payout cannot make a zero beneficiary or unauthorized
    // mechanical cleanup commit; the owner retains its entire claim on failure.
    let zero = handoff(&env, a.pubkey(), Pubkey::default(), epoch);
    execute(
        &mut env,
        &[user_exit.clone(), zero],
        &[&a],
        &[],
        fail(3, PercolatorError::InvalidInstruction, 1),
    );
    let unsigned_retire = retire(&env, admin.pubkey(), false);
    execute(
        &mut env,
        &[user_exit.clone(), unsigned_retire],
        &[],
        &[],
        fail(3, PercolatorError::ExpectedSigner, 1),
    );
    let signed_close = close(&env, admin.pubkey(), tokens[3], true);
    execute(
        &mut env,
        &[signed_close],
        &[&admin],
        &[],
        fail(2, PercolatorError::EngineLockActive, 0),
    );
    check(&env, disabled, user_paid, deleted, paid, holder, epoch);

    for phase in 0..2 {
        if (phase == 0) == handoff_before_exit {
            // Incoming consent is mandatory even when the destination is disabled.
            let mut missing_consent = retained_handoff.clone();
            missing_consent.accounts[1].is_signer = false;
            execute(
                &mut env,
                &[missing_consent],
                &[&a],
                &[],
                fail(2, PercolatorError::ExpectedSigner, 0),
            );
            execute(
                &mut env,
                &[retained_handoff.clone()],
                &[&a, &b],
                &[market],
                None,
            );
            holder = b.pubkey();
            epoch += 1;
        } else {
            execute(
                &mut env,
                &[user_exit.clone()],
                &[],
                &[market, vault, portfolio, tokens[2]],
                None,
            );
            user_paid = true;
            check(&env, disabled, user_paid, deleted, paid, holder, epoch);
            let current_token = if holder == a.pubkey() {
                tokens[0]
            } else {
                tokens[1]
            };
            let blocked = payout(&env, holder, current_token, PREFIX, epoch);
            execute(
                &mut env,
                &[blocked],
                &[],
                &[],
                fail(
                    2,
                    if holder == a.pubkey() {
                        PercolatorError::InvalidTokenAccount
                    } else {
                        PercolatorError::EngineLockActive
                    },
                    0,
                ),
            );
            let deletion = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(owner.pubkey(), true),
                    AccountMeta::new(market, false),
                    AccountMeta::new(portfolio, false),
                ],
                data: env.close_portfolio_ix(portfolio).encode(),
            };
            let mut unsigned_deletion = deletion.clone();
            unsigned_deletion.accounts[0].is_signer = false;
            execute(
                &mut env,
                &[unsigned_deletion],
                &[],
                &[],
                fail(2, PercolatorError::ExpectedSigner, 0),
            );
            execute(&mut env, &[deletion], &[&owner], &[market, portfolio], None);
            deleted = true;
        }
        check(&env, disabled, user_paid, deleted, paid, holder, epoch);
    }
    assert_eq!(holder, b.pubkey());
    let partial = payout(&env, b.pubkey(), tokens[1], PREFIX, epoch);
    let zero = handoff(&env, b.pubkey(), Pubkey::default(), epoch + 1);
    execute(
        &mut env,
        &[partial.clone(), zero],
        &[&b],
        &[],
        fail(3, PercolatorError::InvalidInstruction, 1),
    );
    let cold_seizure = handoff(&env, admin.pubkey(), a.pubkey(), epoch);
    execute(
        &mut env,
        &[cold_seizure],
        &[&admin, &a],
        &[],
        fail(2, PercolatorError::EngineLockActive, 0),
    );
    execute(&mut env, &[partial], &[], &[market, vault, tokens[1]], None);
    paid[1] = PREFIX;
    epoch += 1;
    check(&env, disabled, user_paid, deleted, paid, holder, epoch);

    let restoration = handoff(&env, b.pubkey(), a.pubkey(), epoch);
    let revoke =
        spl_token::instruction::revoke(&spl_token::ID, &tokens[0], &a.pubkey(), &[]).unwrap();
    // Both the restored role and repaired SPL custody roll back on stale A consent.
    execute(
        &mut env,
        &[restoration.clone(), revoke.clone(), retained_payout.clone()],
        &[&b, &a],
        &[],
        fail(4, PercolatorError::EngineStale, 1),
    );
    check(&env, disabled, user_paid, deleted, paid, holder, epoch);
    execute(
        &mut env,
        &[restoration, revoke],
        &[&b, &a],
        &[market, tokens[0]],
        None,
    );
    disabled = false;
    holder = a.pubkey();
    epoch += 1;
    check(&env, disabled, user_paid, deleted, paid, holder, epoch);
    execute(
        &mut env,
        &[retained_payout],
        &[],
        &[],
        fail(2, PercolatorError::EngineStale, 0),
    );
    execute(
        &mut env,
        &[retained_handoff],
        &[&a, &b],
        &[],
        fail(2, PercolatorError::EngineStale, 0),
    );
    let former_to_restored = payout(&env, b.pubkey(), tokens[0], 1, epoch);
    execute(
        &mut env,
        &[former_to_restored],
        &[],
        &[],
        fail(2, PercolatorError::Unauthorized, 0),
    );
    let final_payment = payout(&env, a.pubkey(), tokens[0], INSURANCE - PREFIX, epoch);
    let unsigned_close = close(&env, admin.pubkey(), tokens[3], false);
    execute(
        &mut env,
        &[final_payment.clone(), unsigned_close],
        &[],
        &[],
        fail(3, PercolatorError::ExpectedSigner, 1),
    );
    let signed_close = close(&env, admin.pubkey(), tokens[3], true);
    execute(
        &mut env,
        &[signed_close],
        &[&admin],
        &[],
        fail(2, PercolatorError::EngineLockActive, 0),
    );
    execute(
        &mut env,
        &[final_payment],
        &[],
        &[market, vault, tokens[0]],
        None,
    );
    paid[0] = INSURANCE - PREFIX;
    epoch += 1;
    check(&env, disabled, user_paid, deleted, paid, holder, epoch);

    // Zero attributed stock does not grant the keeper retirement/close authority.
    let unsigned_close = close(&env, admin.pubkey(), tokens[3], false);
    execute(
        &mut env,
        &[unsigned_close],
        &[],
        &[],
        fail(2, PercolatorError::ExpectedSigner, 0),
    );
    let signed_retire = retire(&env, admin.pubkey(), true);
    execute(
        &mut env,
        &[signed_retire],
        &[&admin],
        &[],
        fail(2, PercolatorError::EngineLockActive, 0),
    );
    let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
    let final_close = close(&env, admin.pubkey(), tokens[3], true);
    execute(
        &mut env,
        &[final_close],
        &[&admin],
        &[market, vault, tokens[3], admin.pubkey()],
        None,
    );
    let tombstone = env.svm.get_account(&market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert!(env
        .svm
        .get_account(&vault)
        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
    let mut expected_admin = admin_before;
    expected_admin.lamports += market_rent + portfolio_rent - tombstone_rent
        + empty[4].lamports
        + if native { RAW_NATIVE } else { 0 };
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    for (index, amount) in [INSURANCE - PREFIX, PREFIX, CAPITAL, SURPLUS]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            env.svm.get_account(&tokens[index]),
            Some(if index == 0 {
                beneficiary_image(&empty[0], amount, delegate.pubkey(), false)
            } else {
                token_image(&empty[index], amount, 0)
            })
        );
    }
    assert_eq!(env.svm.get_account(&env.mint), mint_frame);
    assert_eq!(
        tokens.map(|key| env.token_amount(key)).iter().sum::<u64>(),
        CAPITAL + INSURANCE + SURPLUS
    );
    if native {
        for (index, signer) in [&a, &b, &owner, &admin].into_iter().enumerate() {
            let balance = env.svm.get_account(&wallets[index]).unwrap().lamports;
            let custody = env.svm.get_account(&tokens[index]).unwrap().lamports;
            let redeem = spl_token::instruction::close_account(
                &spl_token::ID,
                &tokens[index],
                &wallets[index],
                &wallets[index],
                &[],
            )
            .unwrap();
            execute(
                &mut env,
                &[redeem],
                &[signer],
                &[tokens[index], wallets[index]],
                None,
            );
            assert!(env
                .svm
                .get_account(&tokens[index])
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            assert_eq!(
                env.svm.get_account(&wallets[index]).unwrap().lamports,
                balance + custody
            );
            assert_eq!(env.svm.get_account(&market), Some(tombstone.clone()));
        }
    }
    (peak, rejected)
}
