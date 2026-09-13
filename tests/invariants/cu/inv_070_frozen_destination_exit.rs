//! Row 418: nonzero resolved PnL exits through fresh SPL custody while original
//! destinations remain frozen, including after irrevocable freeze-authority removal.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: [u64; 2] = [1_000, 1_300];
const ENTRY_PRICE: u64 = 100;
const FINAL_PRICE: u64 = 110;
const PAYOUT: [u64; 2] = [
    CAPITAL[0] + FINAL_PRICE - ENTRY_PRICE,
    CAPITAL[1] - (FINAL_PRICE - ENTRY_PRICE),
];
const SURPLUS: u64 = 17;
const LIMIT: u64 = 500_000;

fn step(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    rent_paid: u64,
) -> Result<u64, (TransactionError, u64)> {
    env.svm.expire_blockhash();
    let mut ixs = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ];
    ixs.extend_from_slice(instructions);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        signing.len()
    );
    assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
    let fee = FeeStructure::default().lamports_per_signature * signing.len() as u64;
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let result = env.svm.send_transaction(tx);
    if let Err(failure) = &result {
        if let TransactionError::InstructionError(index, _) = failure.err {
            let completed = instructions[..usize::from(index) - 2]
                .iter()
                .filter(|ix| ix.program_id == env.program_id)
                .count();
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| { **line == format!("Program {} success", env.program_id) })
                    .count(),
                completed,
                "all preceding wrapper instructions completed before rollback"
            );
        }
    }
    for (key, mut expected) in keys.iter().zip(before) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee + if result.is_ok() { rent_paid } else { 0 };
        } else if result.is_ok() && changed.contains(key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete Account frame: {key}"
        );
    }
    let cu = match &result {
        Ok(meta) => meta.compute_units_consumed,
        Err(failure) => failure.meta.compute_units_consumed,
    };
    assert_cu_within("frozen destination terminal transaction", cu, LIMIT);
    result.map(|_| cu).map_err(|failure| (failure.err, cu))
}

fn invalid_destination(error: TransactionError, index: u8) {
    assert_eq!(
        error,
        TransactionError::InstructionError(
            index,
            InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32),
        )
    );
}

fn closed(env: &V16CuEnv, key: Pubkey) {
    assert!(env.svm.get_account(&key).is_none_or(|account| {
        account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
    }));
}

#[test]
fn v16_program_frozen_destinations_preserve_pnl_exit_without_freeze_authority() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;

    for revoke_freeze in [false, true] {
        for crank_first in [false, true] {
            let mut env = inv018_public_spl_market(6);
            let admin = env.admin.insecure_clone();
            let freezer = Keypair::new();
            let freezer_key = freezer.pubkey();
            env.svm.airdrop(&freezer_key, 1_000_000_000).unwrap();
            let secondary_mint = env.mint;
            let secondary_vault = env.vault;
            let primary = Keypair::new();
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    system_instruction::create_account(
                        &env.payer.pubkey(),
                        &primary.pubkey(),
                        1_000_000_000,
                        Mint::LEN as u64,
                        &spl_token::ID,
                    ),
                    spl_token::instruction::initialize_mint(
                        &spl_token::ID,
                        &primary.pubkey(),
                        &admin.pubkey(),
                        Some(&freezer_key),
                        6,
                    )
                    .unwrap(),
                ],
                &[&primary],
            )
            .unwrap();
            env.send(
                ProgInstruction::UpdateBaseUnitMints {
                    primary_mint: primary.pubkey().to_bytes(),
                    secondary_mint: secondary_mint.to_bytes(),
                    authority_epoch: env.control_sequences(0).authority_epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new_readonly(primary.pubkey(), false),
                    AccountMeta::new_readonly(secondary_mint, false),
                    AccountMeta::new_readonly(secondary_vault, false),
                ],
                &[&admin],
            )
            .unwrap();
            env.mint = primary.pubkey();
            env.vault =
                create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, env.mint);
            let owners = [Keypair::new(), Keypair::new()];
            let owner_keys = owners.each_ref().map(Signer::pubkey);
            let beneficiaries = [owner_keys[0], owner_keys[1], admin.pubkey()];
            let originals = beneficiaries
                .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
            let secondary_destination =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), secondary_mint);
            let portfolios = owners.each_ref().map(|owner| {
                env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
            for i in 0..3 {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &originals[i],
                        &admin.pubkey(),
                        &[],
                        if i < 2 { CAPITAL[i] } else { SURPLUS },
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
                if i < 2 {
                    env.send(
                        env.deposit_ix(portfolios[i], CAPITAL[i].into()),
                        vec![
                            AccountMeta::new(owner_keys[i], true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(originals[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[i]],
                    )
                    .unwrap();
                } else {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::transfer(
                            &spl_token::ID,
                            &originals[i],
                            &env.vault,
                            &admin.pubkey(),
                            &[],
                            SURPLUS,
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                }
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
            env.configure_auth_mark_for_asset_as_admin(0, 0, ENTRY_PRICE);
            env.configure_permissionless_resolve_with_cu(100, 3);
            let trade_cu = env.trade_with_cu(
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                POS_SCALE as i128,
                ENTRY_PRICE,
                0,
            );
            assert_cu_within("freezable quote funded trade", trade_cu, LIMIT);
            env.svm.warp_to_slot(1);
            env.push_auth_mark_with_cu(1, FINAL_PRICE);
            let observation_cu = env.crank(
                portfolios[1],
                ProgInstruction::PermissionlessCrank {
                    now_slot: 1,
                    observations: crank_observations(0),
                },
            );
            assert_cu_within("freezable quote observation", observation_cu, LIMIT);
            let resolve_cu = env.resolve();
            assert_cu_within("freezable quote resolution", resolve_cu, LIMIT);
            assert_eq!(env.market_state().1.assets[0].effective_price, FINAL_PRICE);
            assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, POS_SCALE);
            assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, POS_SCALE);
            let mut freeze: Vec<_> = originals
                .iter()
                .map(|token| {
                    spl_token::instruction::freeze_account(
                        &spl_token::ID,
                        token,
                        &env.mint,
                        &freezer_key,
                        &[],
                    )
                    .unwrap()
                })
                .collect();
            if revoke_freeze {
                freeze.push(
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &env.mint,
                        None,
                        spl_token::instruction::AuthorityType::FreezeAccount,
                        &freezer_key,
                        &[],
                    )
                    .unwrap(),
                );
            }
            send_raw_ixs(&mut env.svm, &env.payer, freeze, &[&freezer]).unwrap();
            drop(freezer);
            drop(owners);
            env.svm.warp_to_slot(4);

            let seeds = [
                "frozen-exit-long",
                "frozen-exit-short",
                "frozen-exit-surplus",
            ];
            let destinations = seeds.map(|seed| {
                Pubkey::create_with_seed(&env.payer.pubkey(), seed, &spl_token::ID).unwrap()
            });
            let mut tracked = vec![
                env.market,
                env.mint,
                env.vault,
                env.vault_authority,
                secondary_mint,
                secondary_vault,
                secondary_destination,
                freezer_key,
            ];
            tracked.extend(portfolios);
            tracked.extend(beneficiaries);
            tracked.extend(originals);
            tracked.extend(destinations);
            let frozen_frame = originals.map(|key| env.svm.get_account(&key).unwrap());
            let mint_frame = env.svm.get_account(&env.mint).unwrap();
            let mint = Mint::unpack(&mint_frame.data).unwrap();
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(
                mint.freeze_authority,
                if revoke_freeze {
                    COption::None
                } else {
                    COption::Some(freezer_key)
                }
            );
            assert_eq!(mint.supply, CAPITAL.iter().sum::<u64>() + SURPLUS);
            for account in &frozen_frame {
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(token.state, AccountState::Frozen);
                assert_eq!(token.amount, 0);
            }
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let create = |i: usize| {
                vec![
                    system_instruction::create_account_with_seed(
                        &env.payer.pubkey(),
                        &destinations[i],
                        &env.payer.pubkey(),
                        seeds[i],
                        rent,
                        TokenAccount::LEN as u64,
                        &spl_token::ID,
                    ),
                    spl_token::instruction::initialize_account3(
                        &spl_token::ID,
                        &destinations[i],
                        &env.mint,
                        &beneficiaries[i],
                    )
                    .unwrap(),
                ]
            };
            let creations: [Vec<Instruction>; 3] = std::array::from_fn(create);
            let payouts: [Instruction; 2] = std::array::from_fn(|i| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(owner_keys[i], false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(originals[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: if (i == 0) == crank_first {
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 0,
                        observations: vec![],
                    }
                } else {
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    }
                }
                .encode(),
            });
            let mut peak = [0; 3];
            let mut paid = [false; 2];
            // Each solvent leg detaches and pays in one call, losing leg first.
            for i in [1, 0] {
                let (error, cu) =
                    step(&mut env, &[payouts[i].clone()], &[], &tracked, &[], 0).unwrap_err();
                invalid_destination(error, 2);
                peak[1] = peak[1].max(cu);
                let mut payout = payouts[i].clone();
                payout.accounts[3] = AccountMeta::new(destinations[i], false);
                let mut completion = creations[i].clone();
                completion.push(payout);
                if i == 1 {
                    let mut aborted_payouts = completion.clone();
                    aborted_payouts.push(payouts[0].clone());
                    let (error, cu) =
                        step(&mut env, &aborted_payouts, &[], &tracked, &[], 0).unwrap_err();
                    invalid_destination(error, 5);
                    peak[1] = peak[1].max(cu);
                }
                let changed = [env.market, portfolios[i], env.vault, destinations[i]];
                peak[0] = peak[0]
                    .max(step(&mut env, &completion, &[], &tracked, &changed, rent).unwrap());
                paid[i] = true;
                assert_eq!(env.token_amount(destinations[i]), PAYOUT[i]);
                assert!(resolved_portfolio_is_terminal(&env, portfolios[i]));
                let market = env.svm.get_account(&env.market).unwrap();
                let group = env.market_state().1;
                let ps = portfolios.map(|key| env.portfolio_state(key));
                let unpaid = CAPITAL.iter().sum::<u64>()
                    - (0..2).filter(|j| paid[*j]).map(|j| PAYOUT[j]).sum::<u64>();
                assert_eq!(group.vault, unpaid.into());
                assert_eq!(group.insurance, 0);
                assert_eq!(env.token_amount(env.vault), unpaid + SURPLUS);
                assert_market_stock_census(
                    "frozen destination PnL",
                    &group,
                    &market.data,
                    &ps,
                    unpaid.into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census("frozen destination PnL", &group, &ps)
                    .unwrap();
                for j in 0..2 {
                    if paid[j] {
                        assert_eq!(env.token_amount(destinations[j]), PAYOUT[j]);
                    }
                }
                assert_eq!(
                    originals.map(|key| env.svm.get_account(&key).unwrap()),
                    frozen_frame
                );
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
            }
            let original_market = env.svm.get_account(&env.market).unwrap();
            let portfolio_rent: u64 = portfolios
                .iter()
                .map(|key| env.svm.get_account(key).unwrap().lamports)
                .sum();
            let group = env.market_state().1;
            assert_eq!((group.c_tot, group.vault, group.insurance), (0, 0, 0));
            assert_eq!(
                (
                    group.assets[0].oi_eff_long_q,
                    group.assets[0].oi_eff_short_q
                ),
                (0, 0)
            );
            assert_eq!(group.materialized_portfolio_count, 2);
            let deletion: Vec<_> = portfolios
                .iter()
                .map(|portfolio| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(*portfolio, false),
                    ],
                    data: env.close_portfolio_ix(*portfolio).encode(),
                })
                .collect();
            let mut slab = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(originals[2], false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(secondary_vault, false),
                    AccountMeta::new(secondary_destination, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            let mut rejected_close = deletion.clone();
            rejected_close.push(slab.clone());
            let (error, cu) =
                step(&mut env, &rejected_close, &[&admin], &tracked, &[], 0).unwrap_err();
            invalid_destination(error, 4);
            peak[1] = peak[1].max(cu);
            peak[2] = peak[2].max(
                step(
                    &mut env,
                    &creations[2],
                    &[],
                    &tracked,
                    &[destinations[2]],
                    rent,
                )
                .unwrap(),
            );
            slab.accounts[4] = AccountMeta::new(destinations[2], false);
            let mut completion = deletion;
            completion.push(slab);
            let mut late_abort = completion.clone();
            late_abort.push(
                spl_token::instruction::burn(
                    &spl_token::ID,
                    &destinations[2],
                    &env.mint,
                    &admin.pubkey(),
                    &[],
                    SURPLUS + 1,
                )
                .unwrap(),
            );
            let (error, cu) = step(&mut env, &late_abort, &[&admin], &tracked, &[], 0).unwrap_err();
            assert_eq!(
                error,
                TransactionError::InstructionError(
                    5,
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32
                    )
                )
            );
            peak[1] = peak[1].max(cu);
            let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            expected_admin.lamports +=
                original_market.lamports + portfolio_rent + 2 * rent - tombstone_rent;
            let changed = [
                env.market,
                portfolios[0],
                portfolios[1],
                env.vault,
                secondary_vault,
                destinations[2],
                admin.pubkey(),
            ];
            peak[2] =
                peak[2].max(step(&mut env, &completion, &[&admin], &tracked, &changed, 0).unwrap());
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(tombstone.lamports, tombstone_rent);
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            for key in [portfolios[0], portfolios[1], env.vault, secondary_vault] {
                closed(&env, key);
            }
            assert_eq!(env.token_amount(destinations[2]), SURPLUS);
            for i in 0..3 {
                assert_ne!(
                    destinations[i],
                    canonical_vault_ata(beneficiaries[i], env.mint)
                );
                let account = env.svm.get_account(&destinations[i]).unwrap();
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(account.owner, spl_token::ID);
                assert_eq!(account.lamports, rent);
                assert_eq!(token.mint, env.mint);
                assert_eq!(token.owner, beneficiaries[i]);
                assert_eq!(token.state, AccountState::Initialized);
                assert_eq!(
                    (token.delegate, token.close_authority, token.is_native),
                    (COption::None, COption::None, COption::None)
                );
                assert_eq!(token.amount, if i < 2 { PAYOUT[i] } else { SURPLUS });
            }
            assert_eq!(
                originals.map(|key| env.svm.get_account(&key).unwrap()),
                frozen_frame
            );
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
            println!("INV-070 frozen destination exit: revoke_freeze={revoke_freeze}, crank_first={crank_first}, payout_calls=2, paid={PAYOUT:?}, surplus={SURPLUS}, exact_rollbacks=5, CU[payout,reject,close]={peak:?}, trade_CU={trade_cu}, observation_CU={observation_cu}, resolve_CU={resolve_cu}");
        }
    }
}
