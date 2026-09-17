//! INV-073/077/080: public maximum-market native insurance completion.
//! Front and last-asset insurers have independent budgets and optional ledgers.
//! A funded user's permissionless payout precedes administrative portfolio deletion;
//! only then may keeper-only partial reserve payments cross both domain budgets.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: u64 = 1_009;
const BUDGETS: [[u64; 2]; 2] = [[37, 61], [43, 71]];
const RAW: u64 = 19;
const LIMIT: u64 = 300_000;

fn wire(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn execute(
    env: &mut V16CuEnv,
    ix: Instruction,
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    error: Option<PercolatorError>,
) -> u64 {
    env.svm.expire_blockhash();
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
            ix,
        ],
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        signing.len()
    );
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let fee = signing.len() as u64 * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let success = error.is_none();
    let result = env.svm.send_transaction(tx);
    let cu = match error {
        Some(error) => {
            let failed = result.expect_err("reserve payment must wait for portfolio disposal");
            assert_eq!(
                failed.err,
                TransactionError::InstructionError(2, InstructionError::Custom(error as u32)),
                "logs={:?}",
                failed.meta.logs
            );
            failed.meta.compute_units_consumed
        }
        None => {
            result
                .expect("bounded public terminal continuation")
                .compute_units_consumed
        }
    };
    for (key, mut expected) in keys.iter().zip(before) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        } else if success && changed.contains(key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete Account frame: {key}"
        );
    }
    assert_cu_within("public maximum-market native continuation", cu, LIMIT);
    cu
}

fn native_image(empty: &Account, amount: u64, raw: u64) -> Account {
    let mut expected = empty.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.amount, 0);
    assert_eq!(token.is_native, COption::Some(empty.lamports));
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected.lamports += amount + raw;
    expected
}

fn withdrawal(
    env: &V16CuEnv,
    asset: usize,
    beneficiary: Pubkey,
    destination: Pubkey,
    ledger: Pubkey,
    amount: u64,
) -> Instruction {
    wire(
        env,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: env.control_sequences(asset).authority_epoch,
            amount: amount.into(),
        },
        vec![
            AccountMeta::new_readonly(beneficiary, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
    )
}

#[test]
fn v16_program_public_max_market_native_partial_insurance_ledgers_complete_in_both_orders() {
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_capacity;

    let n = MAX_10M_MARKET_SLOTS;
    let assets = [0, n - 1];
    let account_len = state::market_account_len_for_capacity(n).unwrap();
    assert!(account_len <= 10 * 1024 * 1024);
    assert!(state::market_account_len_for_capacity(n + 1).unwrap() > 10 * 1024 * 1024);
    let total: u64 = BUDGETS.iter().flatten().sum();
    for order in [[0, 1], [1, 0]] {
        let mut env = inv081_public_native_market_with_capacity(n);
        let admin = env.admin.insecure_clone();
        let insurers = [Keypair::new(), Keypair::new()];
        let owner = Keypair::new();
        let mut peak = env.init_market_cu;
        let mut continuation_cu = [0u64; 3];
        let mut payout_cu = [[0u64; 3]; 2];
        for signer in [&insurers[0], &insurers[1], &owner] {
            env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
        }
        peak = peak.max(
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&insurers[0]),
                0,
                processor::ASSET_AUTH_INSURANCE,
                insurers[0].pubkey().to_bytes(),
            )
            .unwrap(),
        );
        for asset in 1..n {
            let cu = env.activate_asset_with_authorities(
                asset as u16,
                asset as u64 + 1,
                1_000_000,
                if asset == n - 1 {
                    insurers[1].pubkey()
                } else {
                    admin.pubkey()
                },
                admin.pubkey(),
                admin.pubkey(),
                admin.pubkey(),
            );
            assert_cu_within("public maximum-market activation", cu, LIMIT);
            peak = peak.max(cu);
        }
        assert_eq!(env.market_state().1.config.max_market_slots as usize, n);
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().data.len(),
            account_len
        );
        assert_eq!(env.market_state().1.next_market_id, n as u64 + 1);
        let tokens = insurers
            .each_ref()
            .map(|signer| create_ata_for_test(&mut env.svm, &env.payer, signer.pubkey(), env.mint));
        let user_token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
        let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        let custody = [env.vault, tokens[0], tokens[1], user_token, admin_token];
        let empty = custody.map(|key| env.svm.get_account(&key).unwrap());
        let ledger_keys = [Keypair::new(), Keypair::new()];
        for key in &ledger_keys {
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                key,
                state::insurance_ledger_account_len(),
                env.program_id,
            );
        }
        let ledgers = ledger_keys.each_ref().map(|key| key.pubkey());
        let ledger_frames = ledgers.map(|key| env.svm.get_account(&key).unwrap());
        for actor in 0..2 {
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    system_instruction::transfer(
                        &insurers[actor].pubkey(),
                        &tokens[actor],
                        BUDGETS[actor].iter().sum(),
                    ),
                    spl_token::instruction::sync_native(&spl_token::ID, &tokens[actor]).unwrap(),
                ],
                &[&insurers[actor]],
            )
            .unwrap();
            for side in 0..2 {
                let mut accounts = vec![
                    AccountMeta::new(insurers[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ];
                // The tail ledger records real principal; the front ledger is first
                // initialized by a keeper. The tail short-side deposit omits telemetry.
                if actor == 1 && side == 0 {
                    accounts.push(AccountMeta::new(ledgers[actor], false));
                }
                let cu = env
                    .send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: (2 * assets[actor] + side) as u16,
                            market_id: env.asset_market_id(assets[actor] as u16),
                            authority_epoch: env.control_sequences(assets[actor]).authority_epoch,
                            intent_id: env.control_sequences(assets[actor]).insurance_top_up + 1,
                            amount: BUDGETS[actor][side].into(),
                        },
                        accounts,
                        &[&insurers[actor]],
                    )
                    .unwrap();
                assert_cu_within("public maximum-market native funding", cu, LIMIT);
                peak = peak.max(cu);
            }
        }
        let portfolio_key = Keypair::new();
        let portfolio = portfolio_key.pubkey();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio_key,
            state::portfolio_account_len_for_market_slots(n).unwrap(),
            env.program_id,
        );
        peak = peak.max(
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&owner],
            )
            .unwrap(),
        );
        env.portfolios.push(portfolio);
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::transfer(&owner.pubkey(), &user_token, CAPITAL),
                spl_token::instruction::sync_native(&spl_token::ID, &user_token).unwrap(),
                system_instruction::transfer(&owner.pubkey(), &env.vault, RAW),
            ],
            &[&owner],
        )
        .unwrap();
        peak = peak.max(
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
        let beneficiaries = insurers.each_ref().map(|signer| signer.pubkey());
        let user = owner.pubkey();
        drop((owner, insurers, ledger_keys, portfolio_key));
        peak = peak.max(env.resolve());
        let tracked = [
            env.market,
            env.mint,
            env.vault,
            env.vault_authority,
            tokens[0],
            tokens[1],
            ledgers[0],
            ledgers[1],
            user,
            user_token,
            portfolio,
            beneficiaries[0],
            beneficiaries[1],
            admin.pubkey(),
            admin_token,
        ];
        let mut rejected = 0;
        for stage in 0..2 {
            for actor in order {
                let ix = withdrawal(
                    &env,
                    assets[actor],
                    beneficiaries[actor],
                    tokens[actor],
                    ledgers[actor],
                    1,
                );
                peak = peak.max(execute(
                    &mut env,
                    ix,
                    &[],
                    &tracked,
                    &[],
                    Some(PercolatorError::EngineLockActive),
                ));
                rejected += 1;
            }
            if stage == 0 {
                assert_eq!(env.portfolio_state(portfolio).capital.get(), CAPITAL.into());
                let before_market = env.svm.get_account(&env.market).unwrap();
                let ix = wire(
                    &env,
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    vec![
                        AccountMeta::new_readonly(user, false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(user_token, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                );
                let changed = [env.market, portfolio, user_token, env.vault];
                continuation_cu[0] = execute(&mut env, ix, &[], &tracked, &changed, None);
                peak = peak.max(continuation_cu[0]);
                assert!(resolved_portfolio_is_terminal(&env, portfolio));
                assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
                assert_eq!(env.market_state().1.c_tot, 0);
                assert_eq!(env.market_state().1.insurance, total.into());
                assert_eq!(env.market_state().1.materialized_portfolio_count, 1);
                assert_eq!(
                    env.svm.get_account(&env.market).unwrap().lamports,
                    before_market.lamports
                );
                assert_eq!(
                    env.svm.get_account(&user_token),
                    Some(native_image(&empty[3], CAPITAL, 0))
                );
                assert_eq!(
                    env.svm.get_account(&env.vault),
                    Some(native_image(&empty[0], total, RAW))
                );
            }
        }
        let mut expected_market = env.svm.get_account(&env.market).unwrap();
        let rent = env.svm.get_account(&portfolio).unwrap().lamports;
        let (cfg, mut group) = state::read_market(&expected_market.data).unwrap();
        group.materialized_portfolio_count -= 1;
        state::write_market(&mut expected_market.data, &cfg, &group).unwrap();
        expected_market.lamports += rent;
        let ix = wire(
            &env,
            env.close_portfolio_ix(portfolio),
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
        );
        let changed = [env.market, portfolio];
        continuation_cu[1] = execute(&mut env, ix, &[&admin], &tracked, &changed, None);
        peak = peak.max(continuation_cu[1]);
        assert_eq!(env.svm.get_account(&env.market), Some(expected_market));
        assert!(env
            .svm
            .get_account(&portfolio)
            .is_none_or(|account| account.lamports == 0));

        let mut paid = [0u64; 2];
        let mut payments = 0;
        for phase in 0..3 {
            for actor in order {
                let face: u64 = BUDGETS[actor].iter().sum();
                let amount = match phase {
                    0 => 1,
                    1 => BUDGETS[actor][0],
                    _ => face - paid[actor],
                };
                let mut expected_market = env.svm.get_account(&env.market).unwrap();
                let (cfg, mut group) = state::read_market(&expected_market.data).unwrap();
                let rank_before = group.insurance;
                let mut sequences = env.control_sequences(assets[actor]);
                paid[actor] += amount;
                group.insurance -= u128::from(amount);
                group.vault -= u128::from(amount);
                group.insurance_domain_budget_remaining_total -= u128::from(amount);
                group.insurance_domain_budget[2 * assets[actor]] =
                    BUDGETS[actor][0].saturating_sub(paid[actor]).into();
                group.insurance_domain_budget[2 * assets[actor] + 1] =
                    (BUDGETS[actor][1] - paid[actor].saturating_sub(BUDGETS[actor][0])).into();
                state::write_market(&mut expected_market.data, &cfg, &group).unwrap();
                sequences.authority_epoch += 1;
                state::write_asset_control_sequences(
                    &mut expected_market.data,
                    assets[actor],
                    &sequences,
                )
                .unwrap();
                let expected_record = state::InsuranceLedgerAccountV16 {
                    market_group: env.market.to_bytes(),
                    authority: beneficiaries[actor].to_bytes(),
                    total_principal_atoms: if actor == 1 {
                        BUDGETS[actor][0].saturating_sub(paid[actor]).into()
                    } else {
                        0
                    },
                    total_deposited_atoms: if actor == 1 {
                        BUDGETS[actor][0].into()
                    } else {
                        0
                    },
                    total_withdrawn_atoms: paid[actor].into(),
                    cumulative_profit_atoms: if actor == 1 {
                        BUDGETS[actor][1].into()
                    } else {
                        0
                    },
                    cumulative_loss_atoms: 0,
                    last_observed_insurance_atoms: (face - paid[actor]).into(),
                };
                let mut expected_ledger = ledger_frames[actor].clone();
                state::init_insurance_ledger(&mut expected_ledger.data, &expected_record).unwrap();
                let ix = withdrawal(
                    &env,
                    assets[actor],
                    beneficiaries[actor],
                    tokens[actor],
                    ledgers[actor],
                    amount,
                );
                assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
                let changed = [env.market, env.vault, tokens[actor], ledgers[actor]];
                payout_cu[actor][phase] = execute(&mut env, ix, &[], &tracked, &changed, None);
                peak = peak.max(payout_cu[actor][phase]);
                payments += 1;
                assert_eq!(
                    env.svm.get_account(&env.market),
                    Some(expected_market.clone())
                );
                assert_eq!(env.svm.get_account(&ledgers[actor]), Some(expected_ledger));
                assert_eq!(
                    env.svm.get_account(&tokens[actor]),
                    Some(native_image(&empty[actor + 1], paid[actor], 0))
                );
                let remaining = total - paid.iter().sum::<u64>();
                assert_eq!(
                    env.svm.get_account(&env.vault),
                    Some(native_image(&empty[0], remaining, RAW))
                );
                assert_eq!(rank_before - env.market_state().1.insurance, amount.into());
                assert!(env.market_state().1.insurance < rank_before);
                assert_market_stock_census(
                    "public native maximum insurance",
                    &group,
                    &expected_market.data,
                    &[],
                    remaining.into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census(
                    "public native maximum insurance",
                    &group,
                    &[],
                )
                .unwrap();
            }
        }
        assert_eq!(paid, BUDGETS.map(|pair| pair.iter().sum::<u64>()));
        let market = env.svm.get_account(&env.market).unwrap();
        let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        expected_admin.lamports += market.lamports - tombstone_rent + empty[0].lamports + RAW;
        let ix = wire(
            &env,
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
        );
        let changed = [env.market, env.vault, admin.pubkey(), admin_token];
        continuation_cu[2] = execute(&mut env, ix, &[&admin], &tracked, &changed, None);
        peak = peak.max(continuation_cu[2]);
        let tombstone = env.svm.get_account(&env.market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(tombstone.lamports, tombstone_rent);
        assert!(env
            .svm
            .get_account(&env.vault)
            .is_none_or(|account| account.lamports == 0));
        assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
        assert_eq!(env.svm.get_account(&admin_token), Some(empty[4].clone()));
        assert_cu_within("maximum-market native history peak", peak, LIMIT);
        println!("INV-073/077 public native insurance: order={order:?}, assets={n}, domains={}, account_bytes={account_len}, user_paid={CAPITAL}, reserve_paid={paid:?}, payments={payments}, exact_rejections={rejected}, slab_calls=1, payout_CU={payout_cu:?}, user_delete_slab_CU={continuation_cu:?}, peak_CU={peak}", 2 * n);
    }
}
