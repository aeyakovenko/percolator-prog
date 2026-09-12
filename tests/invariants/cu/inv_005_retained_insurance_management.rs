//! INV-005: empty-role consent is rechecked against current insurance stock, and
//! separately held funded insurance roles require ordered, atomic incumbent consent.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use litesvm::types::TransactionMetadata;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const PEER_INSURANCE: u128 = 19;
const ROLES: [u8; 2] = [
    processor::ASSET_AUTH_INSURANCE,
    processor::ASSET_AUTH_INSURANCE_OPERATOR,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Telemetry {
    Absent,
    StaleZero,
    Current,
}

fn wallet(env: &mut V16CuEnv, owner: Pubkey, amount: u128) -> Pubkey {
    let wallet = create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint);
    if amount != 0 {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &wallet,
                &env.admin.pubkey(),
                &[],
                amount as u64,
            )
            .unwrap(),
            &[&env.admin],
        )
        .expect("SPL mints the independently specified principal");
    }
    wallet
}

fn signed(env: &V16CuEnv, instructions: &[Instruction], signers: &[&Keypair]) -> Transaction {
    let mut all = vec![heap_ix(), cu_ix()];
    all.extend_from_slice(instructions);
    let mut signatures = vec![&env.payer];
    signatures.extend_from_slice(signers);
    Transaction::new_signed_with_payer(
        &all,
        Some(&env.payer.pubkey()),
        &signatures,
        env.svm.latest_blockhash(),
    )
}

fn handoff(env: &V16CuEnv, from: Pubkey, to: Pubkey, role: u8, epoch: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(from, true),
            AccountMeta::new(to, true),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: epoch,
            kind: role,
            new_pubkey: to.to_bytes(),
        }
        .encode(),
    }
}

fn land(
    env: &mut V16CuEnv,
    tx: Transaction,
    protected: &[Pubkey],
    changed: &[Pubkey],
    rejection: Option<(u8, PercolatorError)>,
) -> TransactionMetadata {
    let must_rollback = rejection.is_some();
    let before = protected
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, error)) = rejection {
        let failed = result.expect_err("role management must reject atomically");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
        );
        failed.meta
    } else {
        result.expect("authorized public continuation remains live")
    };
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    for (key, account) in protected.iter().zip(before) {
        if must_rollback || !changed.contains(key) {
            assert_eq!(env.svm.get_account(key), account, "account frame: {key}");
        }
    }
    assert_cu_within(
        "insurance management history",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    meta
}

fn fund(env: &mut V16CuEnv, authority: &Keypair, source: Pubkey, domain: u16, amount: u128) {
    let asset = domain / 2;
    let sequences = env.control_sequences(asset as usize);
    let instruction = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::TopUpInsuranceDomain {
            domain,
            market_id: env.asset_market_id(asset),
            authority_epoch: sequences.authority_epoch,
            intent_id: next_control_sequence(sequences.insurance_top_up),
            amount,
        }
        .encode(),
    };
    let tx = signed(env, &[instruction], &[authority]);
    env.svm
        .send_transaction(tx)
        .expect("public insurance funding without telemetry");
}

fn profiles(env: &V16CuEnv) -> [state::AssetOracleProfileV16; 2] {
    let market = env.svm.get_account(&env.market).unwrap();
    [0, 1].map(|asset| state::read_asset_oracle_profile(&market.data, asset).unwrap())
}

fn no_token_cpi(meta: &TransactionMetadata) {
    assert!(!meta
        .logs
        .iter()
        .any(|line| { line.contains(&format!("Program {} invoke", spl_token::ID)) }));
}

#[test]
fn v16_program_retained_empty_insurance_management_rechecks_stock_before_ordered_succession() {
    let mut max_rejection_cu = 0;
    let mut max_handoff_cu = 0;
    let mut max_withdrawal_cu = 0;
    for side in [0usize, 1] {
        for first in [0usize, 1] {
            for telemetry in [Telemetry::Absent, Telemetry::StaleZero, Telemetry::Current] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let principal = [37u128, 41][side];
                let admin = env.admin.insecure_clone();
                let holders = [Keypair::new(), Keypair::new()];
                let successor = Keypair::new();
                for key in [&holders[0], &holders[1], &successor] {
                    env.ensure_signer_account(key.pubkey());
                }
                let source = wallet(&mut env, holders[0].pubkey(), principal);
                let operator_wallet = wallet(&mut env, holders[1].pubkey(), 0);
                let successor_wallet = wallet(&mut env, successor.pubkey(), 0);
                let admin_wallet = wallet(&mut env, admin.pubkey(), PEER_INSURANCE);
                for role in 0..2 {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(&holders[role]),
                        0,
                        ROLES[role],
                        holders[role].pubkey().to_bytes(),
                    )
                    .expect("install distinct insurance authority and operator while empty");
                }
                fund(&mut env, &admin, admin_wallet, 3, PEER_INSURANCE);

                let mut protected = vec![
                    env.market,
                    env.vault,
                    env.mint,
                    env.vault_authority,
                    source,
                    operator_wallet,
                    successor_wallet,
                    admin_wallet,
                    admin.pubkey(),
                    holders[0].pubkey(),
                    holders[1].pubkey(),
                    successor.pubkey(),
                ];
                let ledger = if telemetry == Telemetry::Absent {
                    None
                } else {
                    let key = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &key,
                        state::insurance_ledger_account_len(),
                        env.program_id,
                    );
                    protected.push(key.pubkey());
                    Some(key.pubkey())
                };
                let sync = ledger.map(|ledger| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(holders[0].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(ledger, false),
                    ],
                    data: ProgInstruction::SyncInsuranceLedger.encode(),
                });
                if let Some(sync) = &sync {
                    let tx = signed(&env, &[sync.clone()], &[&holders[0]]);
                    land(&mut env, tx, &protected, &[ledger.unwrap()], None);
                    let state = state::read_insurance_ledger(
                        &env.svm.get_account(&ledger.unwrap()).unwrap().data,
                    )
                    .unwrap();
                    assert_eq!(state.last_observed_insurance_atoms, 0);
                    assert_eq!(state.total_principal_atoms, 0);
                }

                // No blockhash, account meta, instruction byte, or signature is replaced
                // between empty-state prevalidation and the funded-state rejection.
                env.svm.expire_blockhash();
                let initial_profiles = profiles(&env);
                let initial_sequences = [env.control_sequences(0), env.control_sequences(1)];
                let epoch = initial_sequences[0].authority_epoch;
                let order = [first, 1 - first];
                let cold = order
                    .iter()
                    .enumerate()
                    .map(|(step, &role)| {
                        handoff(
                            &env,
                            admin.pubkey(),
                            successor.pubkey(),
                            ROLES[role],
                            epoch + step as u64,
                        )
                    })
                    .collect::<Vec<_>>();
                let retained = signed(&env, &cold, &[&admin, &successor]);
                let before_simulation = protected
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>();
                env.svm
                    .simulate_transaction(retained.clone().into())
                    .expect("the exact signed two-role request is admissible while empty");
                assert_eq!(
                    protected
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>(),
                    before_simulation
                );

                fund(&mut env, &holders[0], source, side as u16, principal);
                let mut funded_sequences = initial_sequences;
                funded_sequences[0].insurance_top_up += 1;
                assert_eq!(
                    [env.control_sequences(0), env.control_sequences(1)],
                    funded_sequences
                );
                assert_eq!(profiles(&env), initial_profiles);
                if telemetry == Telemetry::Current {
                    let tx = signed(&env, &[sync.clone().unwrap()], &[&holders[0]]);
                    land(&mut env, tx, &protected, &[ledger.unwrap()], None);
                }
                if let Some(ledger) = ledger {
                    let state =
                        state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data)
                            .unwrap();
                    let observed = if telemetry == Telemetry::Current {
                        principal
                    } else {
                        0
                    };
                    assert_eq!(state.authority, holders[0].pubkey().to_bytes());
                    assert_eq!(state.total_principal_atoms, 0);
                    assert_eq!(state.total_deposited_atoms, 0);
                    assert_eq!(state.total_withdrawn_atoms, 0);
                    assert_eq!(state.last_observed_insurance_atoms, observed);
                    assert_eq!(state.cumulative_profit_atoms, observed);
                    assert_eq!(state.cumulative_loss_atoms, 0);
                }

                let assert_books = |env: &V16CuEnv, paid: u128| {
                    let (_, group) = env.market_state();
                    assert_eq!(group.mode, MarketModeV16::Live);
                    assert_eq!(group.c_tot, 0);
                    assert_eq!(group.vault, principal + PEER_INSURANCE - paid);
                    assert_eq!(group.insurance, principal + PEER_INSURANCE - paid);
                    for domain in 0..4 {
                        let expected = if domain == side {
                            principal - paid
                        } else if domain == 3 {
                            PEER_INSURANCE
                        } else {
                            0
                        };
                        assert_eq!(group.insurance_domain_budget[domain], expected);
                    }
                    assert_eq!(env.token_amount(env.vault) as u128, group.vault);
                    assert_eq!(env.token_amount(source), 0);
                    assert_eq!(env.token_amount(operator_wallet), 0);
                    assert_eq!(env.token_amount(admin_wallet), 0);
                    assert_eq!(env.token_amount(successor_wallet) as u128, paid);
                    let supply = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                        .unwrap()
                        .supply as u128;
                    assert_eq!(supply, principal + PEER_INSURANCE);
                    assert_eq!(supply, group.vault + paid);
                };
                assert_books(&env, 0);
                let before_roles = env.market_state();
                let meta = land(
                    &mut env,
                    retained,
                    &protected,
                    &[],
                    Some((2, PercolatorError::EngineLockActive)),
                );
                no_token_cpi(&meta);
                max_rejection_cu = max_rejection_cu.max(meta.compute_units_consumed);

                // The incumbents are different signers. Both consent, but their shared
                // asset epoch must advance between the two instructions in either order.
                let consent = |env: &V16CuEnv, advance_second: bool| {
                    order
                        .iter()
                        .enumerate()
                        .map(|(step, &role)| {
                            handoff(
                                env,
                                holders[role].pubkey(),
                                successor.pubkey(),
                                ROLES[role],
                                epoch + if advance_second { step as u64 } else { 0 },
                            )
                        })
                        .collect::<Vec<_>>()
                };
                let bad_order = signed(
                    &env,
                    &consent(&env, false),
                    &[&holders[0], &holders[1], &successor],
                );
                let meta = land(
                    &mut env,
                    bad_order,
                    &protected,
                    &[],
                    Some((3, PercolatorError::EngineStale)),
                );
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| **line == format!("Program {} success", env.program_id))
                        .count(),
                    1,
                    "the first role update executed before the second rejected"
                );
                no_token_cpi(&meta);
                max_rejection_cu = max_rejection_cu.max(meta.compute_units_consumed);
                assert_eq!(env.market_state(), before_roles);
                assert_eq!(profiles(&env), initial_profiles);
                assert_eq!(
                    [env.control_sequences(0), env.control_sequences(1)],
                    funded_sequences
                );

                let tx = signed(
                    &env,
                    &consent(&env, true),
                    &[&holders[0], &holders[1], &successor],
                );
                let market = env.market;
                let meta = land(&mut env, tx, &protected, &[market], None);
                no_token_cpi(&meta);
                max_handoff_cu = max_handoff_cu.max(meta.compute_units_consumed);
                assert_eq!(
                    env.market_state(),
                    before_roles,
                    "succession changes no economic state"
                );
                let mut expected_profiles = initial_profiles;
                expected_profiles[0].insurance_authority = successor.pubkey().to_bytes();
                expected_profiles[0].insurance_operator = successor.pubkey().to_bytes();
                assert_eq!(profiles(&env), expected_profiles);
                funded_sequences[0].authority_epoch += 2;
                assert_eq!(
                    [env.control_sequences(0), env.control_sequences(1)],
                    funded_sequences
                );
                assert_books(&env, 0);

                let withdrawal = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(successor.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(successor_wallet, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: epoch + 2,
                        amount: principal,
                    }
                    .encode(),
                };
                let tx = signed(&env, &[withdrawal], &[&successor]);
                let changed = [env.market, env.vault, successor_wallet];
                let meta = land(&mut env, tx, &protected, &changed, None);
                max_withdrawal_cu = max_withdrawal_cu.max(meta.compute_units_consumed);
                assert_books(&env, principal);
                assert_eq!(profiles(&env), expected_profiles);
                assert_eq!(
                    [env.control_sequences(0), env.control_sequences(1)],
                    funded_sequences
                );
            }
        }
    }
    eprintln!("INV-005 retained insurance management: worlds=12, empty_simulations=12, stock_rejections=12, atomic_epoch_rejections=12, consensual_pairs=12, withdrawals=12, rejection_cu={max_rejection_cu}, handoff_cu={max_handoff_cu}, withdrawal_cu={max_withdrawal_cu}");
}
