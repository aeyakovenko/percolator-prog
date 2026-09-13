//! INV-008/010/024/031/064/080/081, coverage reopening 428:
//! a-value-withdrawal-intent-cannot-spend-insurance-stock-after-its-bound-epoch-changes.
//!
//! Public routes: System/SPL creation and minting, TopUpInsuranceDomain,
//! WithdrawInsuranceAsset, SPL SetAuthority(AccountOwner), UpdateAssetAuthority
//! (ORACLE), and ResolveMarket. Eight histories cross target asset 0/1, execution
//! in Live/Resolved after signing in Live, and absent/lazy insurance ledgers.
//! No program-owned bytes are edited; the established public SPL fixture owns setup.
//!
//! A late SPL failure restores payout, destination ownership, oracle-role epoch,
//! and ledger initialization together. Retained signed consent then pays. A later
//! destination repair / peer payout / oracle handoff prefix is restored exactly
//! when its old-epoch withdrawal suffix rejects. Committing repair and handoff
//! cannot revive the old consent even though the same insurer/operator owns the
//! same destination, enough target stock remains, and the peer's epoch is unchanged.
//! Fresh target consent and retained peer consent spend only their own domain stock.
//!
//! Limits: this is bounded authority-epoch and destination-role composition, not
//! an intrinsic insurance-stock sequence oracle. WithdrawInsuranceAsset has no
//! independent stock-sequence field. Successful withdrawal consumption at a fixed
//! authority epoch, new-stock replenishment, fee/policy histories, native/secondary
//! rails, liabilities, arbitrary histories, and durable nonces remain unproven.
//! Row 428 stays OPEN. SVM rollback and the bundled classic SPL program are assumed.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[derive(Default)]
struct Evidence {
    rollbacks: usize,
    successes: usize,
    peak_cu: u64,
}

impl Evidence {
    fn deliver(
        &mut self,
        env: &mut V16CuEnv,
        tx: Transaction,
        watched: &[Pubkey],
        error: Option<(u8, u32)>,
        successful_programs: [usize; 2],
    ) {
        tx.verify().unwrap();
        let before: Vec<_> = watched
            .iter()
            .chain(&tx.message.account_keys)
            .map(|key| (*key, env.svm.get_account(key)))
            .collect();
        let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
        payer.lamports -= FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let result = env.svm.send_transaction(tx);
        let meta = match error {
            Some((index, code)) => {
                let failure = result.expect_err("retained request must reject at the named gate");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(index, InstructionError::Custom(code))
                );
                for (key, account) in before {
                    if key != env.payer.pubkey() {
                        assert_eq!(env.svm.get_account(&key), account, "rollback at {key}");
                    }
                }
                self.rollbacks += 1;
                failure.meta
            }
            None => {
                self.successes += 1;
                result.expect("valid public continuation")
            }
        };
        assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
        for (program, count) in [env.program_id, spl_token::ID]
            .into_iter()
            .zip(successful_programs)
        {
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                count,
                "completed public prefixes: {:?}",
                meta.logs
            );
        }
        assert!(meta.compute_units_consumed > 0);
        assert_cu_within(
            "row428 destination/epoch retry",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        self.peak_cu = self.peak_cu.max(meta.compute_units_consumed);
    }
}

#[test]
fn v16_retained_insurance_destination_repair_cannot_cross_oracle_role_epoch() {
    const TARGET: [u128; 2] = [73, 41];
    const PEER: [u128; 2] = [59, 23];
    const SUPPLY: u64 = 196;
    let mut evidence = Evidence::default();
    for target in 0..2 {
        for resolved in [false, true] {
            for telemetry in [false, true] {
                let peer = 1 - target;
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let successor = Keypair::new();
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    system_instruction::transfer(
                        &env.payer.pubkey(),
                        &successor.pubkey(),
                        1_000_000,
                    ),
                    &[],
                )
                .unwrap();
                let source =
                    create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
                let destinations: [Keypair; 2] = std::array::from_fn(|_| Keypair::new());
                let ledgers: [Keypair; 2] = std::array::from_fn(|_| Keypair::new());
                for asset in 0..2 {
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &destinations[asset],
                        TokenAccount::LEN,
                        spl_token::ID,
                    );
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::initialize_account3(
                            &spl_token::ID,
                            &destinations[asset].pubkey(),
                            &env.mint,
                            &env.admin.pubkey(),
                        )
                        .unwrap(),
                        &[],
                    )
                    .unwrap();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &ledgers[asset],
                        state::insurance_ledger_account_len(),
                        env.program_id,
                    );
                }
                send_raw_ixs(
                    &mut env.svm,
                    &env.payer,
                    vec![
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &source,
                            &env.admin.pubkey(),
                            &[],
                            SUPPLY,
                        )
                        .unwrap(),
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &env.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &env.admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                    ],
                    &[&env.admin],
                )
                .unwrap();
                for (asset, amounts) in [(target, TARGET), (peer, PEER)] {
                    for (side, amount) in amounts.into_iter().enumerate() {
                        env.send(
                            ProgInstruction::TopUpInsuranceDomain {
                                domain: (asset * 2 + side) as u16,
                                market_id: env.asset_market_id(asset as u16),
                                authority_epoch: env.control_sequences(asset).authority_epoch,
                                intent_id: side as u64 + 1,
                                amount,
                            },
                            vec![
                                AccountMeta::new(env.admin.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(source, false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&env.admin.insecure_clone()],
                        )
                        .unwrap();
                    }
                }
                let controls = [0, 1].map(|asset| env.control_sequences(asset));
                let market_ids = [0, 1].map(|asset| env.asset_market_id(asset));
                let profiles = [0, 1].map(|asset| {
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        asset,
                    )
                    .unwrap()
                });
                let empty_ledgers = ledgers
                    .each_ref()
                    .map(|key| env.svm.get_account(&key.pubkey()).unwrap());
                assert!(empty_ledgers.iter().all(|a| a.data.iter().all(|b| *b == 0)));
                let tokens = [
                    env.vault,
                    source,
                    destinations[0].pubkey(),
                    destinations[1].pubkey(),
                ];
                let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
                let mint_frame = env.svm.get_account(&env.mint).unwrap();
                let mint = Mint::unpack(&mint_frame.data).unwrap();
                assert_eq!((mint.supply, mint.mint_authority), (SUPPLY, COption::None));
                let watched = [
                    env.market,
                    env.mint,
                    env.vault,
                    source,
                    destinations[0].pubkey(),
                    destinations[1].pubkey(),
                    ledgers[0].pubkey(),
                    ledgers[1].pubkey(),
                    env.admin.pubkey(),
                    successor.pubkey(),
                    env.vault_authority,
                ];
                let program_id = env.program_id;
                let market = env.market;
                let admin = env.admin.pubkey();
                let vault = env.vault;
                let vault_authority = env.vault_authority;
                let withdraw = |asset: usize, epoch, amount, signed| {
                    let mut accounts = vec![
                        AccountMeta::new_readonly(admin, signed),
                        AccountMeta::new(market, false),
                        AccountMeta::new(destinations[asset].pubkey(), false),
                        AccountMeta::new(vault, false),
                        AccountMeta::new_readonly(vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ];
                    if telemetry {
                        accounts.push(AccountMeta::new(ledgers[asset].pubkey(), false));
                    }
                    Instruction {
                        program_id,
                        accounts,
                        data: ProgInstruction::WithdrawInsuranceAsset {
                            asset_index: asset as u16,
                            market_id: market_ids[asset],
                            authority_epoch: epoch,
                            amount,
                        }
                        .encode(),
                    }
                };
                let transaction = |env: &V16CuEnv,
                                   instructions: Vec<Instruction>,
                                   nonce: u32,
                                   signers: &[&Keypair]| {
                    let mut message = vec![
                        heap_ix(),
                        ComputeBudgetInstruction::set_compute_unit_limit(300_000 - nonce),
                    ];
                    message.extend(instructions);
                    let mut all_signers = vec![&env.payer];
                    all_signers.extend_from_slice(signers);
                    Transaction::new_signed_with_payer(
                        &message,
                        Some(&env.payer.pubkey()),
                        &all_signers,
                        env.svm.latest_blockhash(),
                    )
                };
                let original = withdraw(target, controls[target].authority_epoch, 37, true);
                // Distinct envelopes are all signed before any transition. No failed signature
                // is resubmitted to LiteSVM's duplicate cache, and none is rebound to a new epoch.
                let retained = [10, 11, 12]
                    .map(|nonce| transaction(&env, vec![original.clone()], nonce, &[&env.admin]));
                let retained_bytes = retained
                    .each_ref()
                    .map(|tx| bincode::serialize(tx).unwrap());
                let peer_withdraw = withdraw(peer, controls[peer].authority_epoch, 11, true);
                let retained_peer =
                    transaction(&env, vec![peer_withdraw.clone()], 13, &[&env.admin]);
                let retained_peer_bytes = bincode::serialize(&retained_peer).unwrap();
                env.svm
                    .simulate_transaction(retained[0].clone().into())
                    .unwrap();
                if resolved {
                    env.send(
                        ProgInstruction::ResolveMarket {
                            asset_generation_frontier: env.market_state().1.next_market_id,
                            authority_epoch: controls[0].authority_epoch,
                        },
                        vec![
                            AccountMeta::new(admin, true),
                            AccountMeta::new(market, false),
                        ],
                        &[&env.admin.insecure_clone()],
                    )
                    .unwrap();
                }
                let check = |env: &V16CuEnv,
                             target_stock: [u128; 2],
                             peer_stock: [u128; 2],
                             changed_epoch: bool,
                             foreign_destination: bool| {
                    let group = env.market_state().1;
                    let mut budgets = [0; 4];
                    budgets[target * 2..target * 2 + 2].copy_from_slice(&target_stock);
                    budgets[peer * 2..peer * 2 + 2].copy_from_slice(&peer_stock);
                    let remaining = budgets.iter().sum::<u128>();
                    assert_eq!(group.insurance_domain_budget, budgets);
                    assert_eq!(group.insurance_domain_spent, [0; 4]);
                    assert_eq!(group.insurance_domain_budget_remaining_total, remaining);
                    assert_eq!(
                        (group.insurance, group.vault, group.c_tot),
                        (remaining, remaining, 0)
                    );
                    assert_eq!(
                        group.mode,
                        if resolved {
                            MarketModeV16::Resolved
                        } else {
                            MarketModeV16::Live
                        }
                    );
                    assert_eq!(group.materialized_portfolio_count, 0);
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(group.source_claim_bound_total_num, 0);
                    assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
                    let mut paid = [0; 2];
                    paid[target] = TARGET.iter().sum::<u128>() - target_stock.iter().sum::<u128>();
                    paid[peer] = PEER.iter().sum::<u128>() - peer_stock.iter().sum::<u128>();
                    for asset in 0..2 {
                        let mut expected_controls = controls[asset];
                        let mut expected_profile = profiles[asset];
                        if asset == target && changed_epoch {
                            expected_controls.authority_epoch += 1;
                            expected_profile.oracle_authority = successor.pubkey().to_bytes();
                        }
                        assert_eq!(env.control_sequences(asset), expected_controls);
                        assert_eq!(group.assets[asset].market_id, market_ids[asset]);
                        assert_eq!(
                            state::read_asset_oracle_profile(
                                &env.svm.get_account(&market).unwrap().data,
                                asset,
                            )
                            .unwrap(),
                            expected_profile
                        );
                        let record = env.svm.get_account(&ledgers[asset].pubkey()).unwrap();
                        if telemetry && paid[asset] != 0 {
                            let stock = budgets[asset * 2] + budgets[asset * 2 + 1];
                            assert_eq!(
                                state::read_insurance_ledger(&record.data).unwrap(),
                                state::InsuranceLedgerAccountV16 {
                                    market_group: market.to_bytes(),
                                    authority: admin.to_bytes(),
                                    // Lazy telemetry does not backfill unattached deposits.
                                    total_principal_atoms: 0,
                                    total_deposited_atoms: 0,
                                    total_withdrawn_atoms: paid[asset],
                                    cumulative_profit_atoms: 0,
                                    cumulative_loss_atoms: 0,
                                    last_observed_insurance_atoms: stock,
                                }
                            );
                        } else {
                            assert_eq!(record, empty_ledgers[asset]);
                        }
                    }
                    for (index, key) in tokens.into_iter().enumerate() {
                        let mut expected = token_frames[index].clone();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        token.amount = match index {
                            0 => remaining as u64,
                            1 => 0,
                            _ => paid[index - 2] as u64,
                        };
                        if index == target + 2 && foreign_destination {
                            token.owner = successor.pubkey();
                        }
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(env.svm.get_account(&key).unwrap(), expected);
                    }
                    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_frame);
                };
                let change_owner = |current, next: &Pubkey| {
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &destinations[target].pubkey(),
                        Some(next),
                        spl_token::instruction::AuthorityType::AccountOwner,
                        &current,
                        &[],
                    )
                    .unwrap()
                };
                let handoff = Instruction {
                    program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(admin, true),
                        AccountMeta::new_readonly(successor.pubkey(), true),
                        AccountMeta::new(market, false),
                    ],
                    data: ProgInstruction::UpdateAssetAuthority {
                        asset_index: target as u16,
                        market_id: market_ids[target],
                        authority_epoch: controls[target].authority_epoch,
                        kind: processor::ASSET_AUTH_ORACLE,
                        new_pubkey: successor.pubkey().to_bytes(),
                    }
                    .encode(),
                };
                check(&env, TARGET, PEER, false, false);
                let abort = transaction(
                    &env,
                    vec![
                        original.clone(),
                        change_owner(admin, &successor.pubkey()),
                        handoff.clone(),
                        spl_token::instruction::transfer(
                            &spl_token::ID,
                            &source,
                            &destinations[peer].pubkey(),
                            &admin,
                            &[],
                            1,
                        )
                        .unwrap(),
                    ],
                    20,
                    &[&env.admin, &successor],
                );
                evidence.deliver(
                    &mut env,
                    abort,
                    &watched,
                    Some((5, spl_token::error::TokenError::InsufficientFunds as u32)),
                    [2, 2],
                );
                check(&env, TARGET, PEER, false, false);
                assert_eq!(bincode::serialize(&retained[0]).unwrap(), retained_bytes[0]);
                evidence.deliver(&mut env, retained[0].clone(), &watched, None, [1, 1]);
                check(&env, [36, 41], PEER, false, false);

                let change = transaction(
                    &env,
                    vec![change_owner(admin, &successor.pubkey())],
                    21,
                    &[&env.admin],
                );
                evidence.deliver(&mut env, change, &watched, None, [0, 1]);
                check(&env, [36, 41], PEER, false, true);
                assert_eq!(bincode::serialize(&retained[1]).unwrap(), retained_bytes[1]);
                evidence.deliver(
                    &mut env,
                    retained[1].clone(),
                    &watched,
                    Some((2, PercolatorError::InvalidTokenAccount as u32)),
                    [0, 0],
                );
                check(&env, [36, 41], PEER, false, true);

                let repair = change_owner(successor.pubkey(), &admin);
                let abort_repair = transaction(
                    &env,
                    vec![
                        repair.clone(),
                        peer_withdraw.clone(),
                        handoff.clone(),
                        original.clone(),
                    ],
                    22,
                    &[&env.admin, &successor],
                );
                evidence.deliver(
                    &mut env,
                    abort_repair,
                    &watched,
                    Some((5, PercolatorError::EngineStale as u32)),
                    [2, 2],
                );
                check(&env, [36, 41], PEER, false, true);
                let commit_repair =
                    transaction(&env, vec![repair, handoff], 23, &[&env.admin, &successor]);
                evidence.deliver(&mut env, commit_repair, &watched, None, [1, 1]);
                check(&env, [36, 41], PEER, true, false);
                assert_eq!(bincode::serialize(&retained[2]).unwrap(), retained_bytes[2]);
                evidence.deliver(
                    &mut env,
                    retained[2].clone(),
                    &watched,
                    Some((2, PercolatorError::EngineStale as u32)),
                    [0, 0],
                );
                check(&env, [36, 41], PEER, true, false);
                let abort_peer =
                    transaction(&env, vec![peer_withdraw, original], 24, &[&env.admin]);
                evidence.deliver(
                    &mut env,
                    abort_peer,
                    &watched,
                    Some((3, PercolatorError::EngineStale as u32)),
                    [1, 1],
                );
                check(&env, [36, 41], PEER, true, false);

                assert_eq!(
                    bincode::serialize(&retained_peer).unwrap(),
                    retained_peer_bytes
                );
                evidence.deliver(&mut env, retained_peer, &watched, None, [1, 1]);
                check(&env, [36, 41], [48, 23], true, false);
                let fresh = transaction(
                    &env,
                    vec![withdraw(
                        target,
                        controls[target].authority_epoch + 1,
                        37,
                        true,
                    )],
                    25,
                    &[&env.admin],
                );
                evidence.deliver(&mut env, fresh, &watched, None, [1, 1]);
                check(&env, [0, 40], [48, 23], true, false);
                let final_instructions = vec![
                    withdraw(target, controls[target].authority_epoch + 1, 40, !resolved),
                    withdraw(peer, controls[peer].authority_epoch, 71, !resolved),
                ];
                let final_signers = if resolved { vec![] } else { vec![&env.admin] };
                let finish = transaction(&env, final_instructions, 26, &final_signers);
                evidence.deliver(&mut env, finish, &watched, None, [2, 2]);
                check(&env, [0; 2], [0; 2], true, false);
            }
        }
    }
    assert_eq!((evidence.rollbacks, evidence.successes), (40, 48));
    println!("row428 destination/epoch: 8 histories, 40 exact rollbacks, 48 continuations, peak {} CU; OPEN", evidence.peak_cu);
}
