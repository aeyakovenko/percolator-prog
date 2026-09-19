//! INV-064 / row 421: one same-slot budget follows distinct live and terminal recipients.
//! All setup and economic transitions use public instructions; absent insurance roles never sign
//! terminal payouts. No authority rotation, ledger attachment or clock advance changes allowance.

use super::*;
use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const BUDGETS: [[u64; 2]; 2] = [[19, 28], [11, 13]];
const LIVE: [u64; 2] = [23, 7];
const SUPPLY: u64 = 71;
const LIMIT: u32 = 200_000;

fn withdraw(
    env: &V16CuEnv,
    asset: usize,
    identity: Pubkey,
    destination: Pubkey,
    amount: u64,
    signed: bool,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(identity, signed),
            AccountMeta::new(env.market, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: env.control_sequences(asset).authority_epoch,
            amount: amount.into(),
        }
        .encode(),
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
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT),
            ix,
        ],
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        signing.len()
    );
    let fee = FeeStructure::default().lamports_per_signature * signing.len() as u64;
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let rejected = error.is_some();
    let result = env.svm.send_transaction(tx);
    let cu = if let Some(error) = error {
        let failure = result.expect_err("recipient or budget violation must reject");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(2, InstructionError::Custom(error as u32))
        );
        failure.meta.compute_units_consumed
    } else {
        result
            .expect("public insurance continuation")
            .compute_units_consumed
    };
    for (key, mut expected) in keys.iter().zip(before) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        } else if !rejected && changed.contains(key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete Account frame {key}"
        );
    }
    assert_cu_within("same-slot insurance recipient handoff", cu, LIMIT.into());
    cu
}

#[test]
fn v16_program_same_slot_operator_departure_preserves_terminal_recipient_and_budget() {
    let mut peak = 0;
    for order in [[0, 1], [1, 0]] {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                ..V16CuMarketParams::default()
            },
        );
        let admin = env.admin.insecure_clone();
        let beneficiaries = [Keypair::new(), Keypair::new()];
        let operators = [Keypair::new(), Keypair::new()];
        let wallets = beneficiaries.each_ref().map(Signer::pubkey);
        let operator_keys = operators.each_ref().map(Signer::pubkey);
        let recipients =
            wallets.map(|key| create_ata_for_test(&mut env.svm, &env.payer, key, env.mint));
        let live_recipients =
            operator_keys.map(|key| create_ata_for_test(&mut env.svm, &env.payer, key, env.mint));
        for asset in 0..2 {
            for (kind, incoming) in [
                (processor::ASSET_AUTH_INSURANCE, &beneficiaries[asset]),
                (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operators[asset]),
            ] {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    system_instruction::transfer(
                        &env.payer.pubkey(),
                        &incoming.pubkey(),
                        1_000_000,
                    ),
                    &[],
                )
                .unwrap();
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(incoming),
                    asset as u16,
                    kind,
                    incoming.pubkey().to_bytes(),
                )
                .unwrap();
            }
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &recipients[asset],
                    &admin.pubkey(),
                    &[],
                    BUDGETS[asset].iter().sum(),
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            for side in 0..2 {
                env.send(
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: (2 * asset + side) as u16,
                        market_id: env.asset_market_id(asset as u16),
                        authority_epoch: env.control_sequences(asset).authority_epoch,
                        intent_id: 0,
                        amount: BUDGETS[asset][side].into(),
                    },
                    vec![
                        AccountMeta::new(wallets[asset], true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(recipients[asset], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&beneficiaries[asset]],
                )
                .unwrap();
            }
        }
        let mint_frame = env.svm.get_account(&env.mint).unwrap();
        let slot = env.svm.get_sysvar::<Clock>().slot;
        let sequences = [0, 1].map(|asset| env.control_sequences(asset));
        let custody = [
            env.vault,
            live_recipients[0],
            live_recipients[1],
            recipients[0],
            recipients[1],
        ];
        let custody_frames = custody.map(|key| env.svm.get_account(&key).unwrap());
        let tracked: Vec<_> = [env.market, env.mint]
            .into_iter()
            .chain(custody)
            .chain(wallets)
            .chain(operator_keys)
            .collect();
        let oracle = |env: &V16CuEnv,
                      live: [u64; 2],
                      terminal: [u64; 2],
                      debits: [u64; 2],
                      resolved: bool| {
            let (cfg, group) = env.market_state();
            let paid = [0, 1].map(|asset| live[asset] + terminal[asset]);
            let remaining = SUPPLY - paid.iter().sum::<u64>();
            let expected: Vec<u128> = (0..2)
                .flat_map(|asset| {
                    [
                        BUDGETS[asset][0].saturating_sub(paid[asset]).into(),
                        (BUDGETS[asset][1] - paid[asset].saturating_sub(BUDGETS[asset][0])).into(),
                    ]
                })
                .collect();
            assert_eq!(group.insurance_domain_budget, expected);
            assert_eq!(group.insurance_domain_spent, vec![0; 4]);
            assert_eq!(
                (
                    group.insurance,
                    group.vault,
                    group.insurance_domain_budget_remaining_total
                ),
                (remaining.into(), remaining.into(), remaining.into())
            );
            assert_eq!(
                (
                    group.c_tot,
                    group.materialized_portfolio_count,
                    group.pnl_pos_tot
                ),
                (0, 0, 0)
            );
            assert_eq!(
                group.mode,
                if resolved {
                    MarketModeV16::Resolved
                } else {
                    MarketModeV16::Live
                }
            );
            // Current policy has no time-based reset or percentage cap: these wire fields are reserved.
            assert_eq!(
                (
                    cfg._reserved_insurance_withdraw_max_bps,
                    cfg._reserved_insurance_withdraw_deposits_only,
                    cfg._reserved_insurance_withdraw_cooldown_slots,
                    cfg._reserved_last_insurance_withdraw_slot
                ),
                (0, 0, 0, 0)
            );
            assert_eq!(env.svm.get_sysvar::<Clock>().slot, slot);
            for asset in 0..2 {
                let mut expected = sequences[asset];
                expected.authority_epoch += debits[asset];
                assert_eq!(env.control_sequences(asset), expected);
                let profile = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    asset,
                )
                .unwrap();
                assert_eq!(profile.insurance_authority, wallets[asset].to_bytes());
                assert_eq!(profile.insurance_operator, operator_keys[asset].to_bytes());
            }
            for ((key, frame), amount) in custody.iter().zip(&custody_frames).zip([
                remaining,
                live[0],
                live[1],
                terminal[0],
                terminal[1],
            ]) {
                let mut expected = frame.clone();
                let mut token = TokenAccount::unpack(&expected.data).unwrap();
                token.amount = amount;
                TokenAccount::pack(token, &mut expected.data).unwrap();
                assert_eq!(env.svm.get_account(key), Some(expected));
            }
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            assert_eq!(Mint::unpack(&mint_frame.data).unwrap().supply, SUPPLY);
            assert_eq!(
                custody
                    .iter()
                    .map(|key| env.token_amount(*key))
                    .sum::<u64>(),
                SUPPLY
            );
        };
        let mut live = [0; 2];
        let mut terminal = [0; 2];
        let mut debits = [0; 2];
        oracle(&env, live, terminal, debits, false);
        for asset in order {
            for (identity, destination, signed, error) in [
                (
                    wallets[asset],
                    recipients[asset],
                    false,
                    PercolatorError::ExpectedSigner,
                ),
                (
                    operator_keys[asset],
                    recipients[asset],
                    true,
                    PercolatorError::InvalidTokenAccount,
                ),
            ] {
                let ix = withdraw(&env, asset, identity, destination, 1, signed);
                let signers = if signed {
                    vec![&operators[asset]]
                } else {
                    vec![]
                };
                peak = peak.max(execute(&mut env, ix, &signers, &tracked, &[], Some(error)));
                oracle(&env, live, terminal, debits, false);
            }
            let ix = withdraw(
                &env,
                asset,
                operator_keys[asset],
                live_recipients[asset],
                LIVE[asset],
                true,
            );
            let changed = [env.market, env.vault, live_recipients[asset]];
            peak = peak.max(execute(
                &mut env,
                ix,
                &[&operators[asset]],
                &tracked,
                &changed,
                None,
            ));
            live[asset] += LIVE[asset];
            debits[asset] += 1;
            oracle(&env, live, terminal, debits, false);
        }
        for role in beneficiaries.iter().chain(&operators) {
            let balance = env.svm.get_account(&role.pubkey()).unwrap().lamports;
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(&role.pubkey(), &env.payer.pubkey(), balance),
                &[role],
            )
            .unwrap();
            assert!(env
                .svm
                .get_account(&role.pubkey())
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
        }
        drop((beneficiaries, operators));
        peak = peak.max(env.resolve());
        oracle(&env, live, terminal, debits, true);
        for asset in order {
            for (identity, destination, amount, error) in [
                (
                    operator_keys[asset],
                    recipients[asset],
                    1,
                    PercolatorError::Unauthorized,
                ),
                (
                    wallets[asset],
                    live_recipients[asset],
                    1,
                    PercolatorError::InvalidTokenAccount,
                ),
                (
                    wallets[asset],
                    recipients[asset],
                    BUDGETS[asset].iter().sum::<u64>() - LIVE[asset] + 1,
                    PercolatorError::EngineLockActive,
                ),
            ] {
                let ix = withdraw(&env, asset, identity, destination, amount, false);
                peak = peak.max(execute(&mut env, ix, &[], &tracked, &[], Some(error)));
                oracle(&env, live, terminal, debits, true);
            }
        }
        for asset in order {
            for amount in [1, BUDGETS[asset].iter().sum::<u64>() - LIVE[asset] - 1] {
                let ix = withdraw(
                    &env,
                    asset,
                    wallets[asset],
                    recipients[asset],
                    amount,
                    false,
                );
                let changed = [env.market, env.vault, recipients[asset]];
                peak = peak.max(execute(&mut env, ix, &[], &tracked, &changed, None));
                terminal[asset] += amount;
                debits[asset] += 1;
                oracle(&env, live, terminal, debits, true);
            }
            if asset == order[0] {
                let ix = withdraw(&env, asset, wallets[asset], recipients[asset], 1, false);
                peak = peak.max(execute(
                    &mut env,
                    ix,
                    &[],
                    &tracked,
                    &[],
                    Some(PercolatorError::EngineLockActive),
                ));
                oracle(&env, live, terminal, debits, true);
            }
        }
        assert_eq!(live, [23, 7]);
        assert_eq!(terminal, [24, 17]);
        assert_eq!(env.token_amount(env.vault), 0);
    }
    println!(
        "INV-064 / row 421: 2 same-slot orders, 12 payouts, 22 exact rejections, peak {peak} CU"
    );
}
