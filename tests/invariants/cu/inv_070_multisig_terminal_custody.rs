//! INV-070 / row 418: funded owner conversion to SPL multisig preserves terminal
//! principal, while disposal of the receiving custody still requires its quorum.
//! Public System/SPL conversion and ATA reconstruction join unsigned wrapper
//! payout with exact transaction rollback and bounded slab/rent disposition.
//! This samples classic SPL, 2-of-3 custody, both payout aliases and all three
//! member pairs. It excludes multisig deposits, PnL, reserves, native/secondary
//! rails, unavailable quorums for subsequent spending, and general row closure.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use spl_token::state::Multisig;

const PRINCIPAL: u64 = 307;
const RESOLVE: u64 = 100;
const DELAY: u64 = 5;
const LIMIT: u64 = 300_000;

#[allow(clippy::too_many_arguments)]
fn step(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    rent_paid: u64,
    error: Option<(usize, InstructionError)>,
) -> u64 {
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
    let rejected = error.is_some();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, expected)) = error {
        let failure = result.expect_err("terminal composition rejects atomically");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError((index + 2) as u8, expected),
            "logs={:?}",
            failure.meta.logs
        );
        let completed = instructions[..index]
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
            "the expected wrapper prefix completed before rollback"
        );
        failure.meta
    } else {
        result.expect("bounded terminal continuation")
    };
    for (key, mut expected) in keys.iter().zip(before) {
        if *key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee + if rejected { 0 } else { rent_paid };
        } else if !rejected && changed.contains(key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(key),
            expected,
            "complete Account frame: {key}"
        );
    }
    assert_cu_within(
        "multisig terminal custody",
        meta.compute_units_consumed,
        LIMIT,
    );
    meta.compute_units_consumed
}

fn closed(env: &V16CuEnv, key: Pubkey) {
    assert!(env.svm.get_account(&key).is_none_or(|account| {
        account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
    }));
}

fn token_amount_frame(env: &V16CuEnv, key: Pubkey, empty: &Account, amount: u64) {
    let mut expected = empty.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.amount, 0);
    assert_eq!(token.delegate, COption::None);
    assert_eq!(token.close_authority, COption::None);
    assert_eq!(token.is_native, COption::None);
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    assert_eq!(env.svm.get_account(&key), Some(expected));
}

#[test]
fn v16_program_multisig_owner_conversion_preserves_unsigned_terminal_payout_and_quorum_disposal() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;

    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    for pair in [[0, 1], [0, 2], [1, 2]] {
        for crank in [false, true] {
            let mut env = inv018_public_spl_market(6);
            let admin = env.admin.insecure_clone();
            let owner = Keypair::new();
            let owner_key = owner.pubkey();
            let members = [Keypair::new(), Keypair::new(), Keypair::new()];
            let member_keys = members.each_ref().map(Signer::pubkey);
            for key in std::iter::once(owner_key).chain(member_keys) {
                env.svm.airdrop(&key, 1_000_000_000).unwrap();
            }
            let quorum = [&members[pair[0]], &members[pair[1]]];
            let recipient = member_keys[pair[0]];
            let destination = create_ata_for_test(&mut env.svm, &env.payer, owner_key, env.mint);
            let sink = create_ata_for_test(&mut env.svm, &env.payer, recipient, env.mint);
            let admin_token =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let empty_destination = env.svm.get_account(&destination).unwrap();
            let empty_sink = env.svm.get_account(&sink).unwrap();
            let empty_vault = env.svm.get_account(&env.vault).unwrap();
            assert_eq!(
                TokenAccount::unpack(&empty_destination.data).unwrap().owner,
                owner_key
            );
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            assert_eq!(empty_destination.lamports, rent);
            let portfolio_keypair = Keypair::new();
            let portfolio = portfolio_keypair.pubkey();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_keypair,
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner_key, true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&owner],
            )
            .unwrap();
            env.portfolios.push(portfolio);
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &destination,
                        &admin.pubkey(),
                        &[],
                        PRINCIPAL,
                    )
                    .unwrap(),
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &env.mint,
                        None,
                        spl_token::instruction::AuthorityType::MintTokens,
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                ],
                &[&admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolio, PRINCIPAL.into()),
                vec![
                    AccountMeta::new_readonly(owner_key, true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owner],
            )
            .unwrap();
            env.configure_permissionless_resolve_with_cu(RESOLVE, DELAY);

            let economic_keys = [
                env.market,
                portfolio,
                env.vault,
                env.mint,
                destination,
                sink,
            ];
            let economic_frame = economic_keys.map(|key| env.svm.get_account(&key));
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    system_instruction::allocate(&owner_key, Multisig::LEN as u64),
                    system_instruction::assign(&owner_key, &spl_token::ID),
                    spl_token::instruction::initialize_multisig2(
                        &spl_token::ID,
                        &owner_key,
                        &member_keys.iter().collect::<Vec<_>>(),
                        2,
                    )
                    .unwrap(),
                ],
                &[&owner],
            )
            .unwrap();
            assert_eq!(
                economic_keys.map(|key| env.svm.get_account(&key)),
                economic_frame
            );
            let multisig_frame = env.svm.get_account(&owner_key).unwrap();
            assert_eq!(multisig_frame.owner, spl_token::ID);
            let multisig = Multisig::unpack(&multisig_frame.data).unwrap();
            assert!(multisig.is_initialized);
            assert_eq!((multisig.m, multisig.n), (2, 3));
            assert_eq!(&multisig.signers[..3], &member_keys);
            drop(owner);

            let tracked = [
                env.market,
                portfolio,
                env.vault,
                env.vault_authority,
                env.mint,
                destination,
                sink,
                admin_token,
                admin.pubkey(),
                owner_key,
                member_keys[0],
                member_keys[1],
                member_keys[2],
            ];
            let quorum_keys = [member_keys[pair[0]], member_keys[pair[1]]];
            let dispose = spl_token::instruction::close_account(
                &spl_token::ID,
                &destination,
                &recipient,
                &owner_key,
                &quorum_keys.iter().collect::<Vec<_>>(),
            )
            .unwrap();
            let wallet_before = env.svm.get_account(&recipient).unwrap();
            peak = peak.max(step(
                &mut env,
                &[dispose.clone()],
                &quorum,
                &tracked,
                &[destination, recipient],
                0,
                None,
            ));
            closed(&env, destination);
            let mut wallet_after = wallet_before;
            wallet_after.lamports += rent;
            assert_eq!(env.svm.get_account(&recipient), Some(wallet_after));
            let create = Instruction {
                program_id: associated_token_program_id(),
                accounts: vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(destination, false),
                    AccountMeta::new_readonly(owner_key, false),
                    AccountMeta::new_readonly(env.mint, false),
                    AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
                ],
                data: vec![1],
            };
            let payout = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(owner_key, false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: if crank {
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
            };
            let transfer = |signers: &[Pubkey]| {
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &destination,
                    &sink,
                    &owner_key,
                    &signers.iter().collect::<Vec<_>>(),
                    PRINCIPAL,
                )
                .unwrap()
            };
            let resolve = Instruction {
                program_id: env.program_id,
                accounts: vec![AccountMeta::new(env.market, false)],
                data: ProgInstruction::ResolveStalePermissionless { now_slot: 0 }.encode(),
            };
            let market_key = env.market;
            let vault_key = env.vault;
            env.svm.warp_to_slot(RESOLVE);
            peak = peak.max(step(
                &mut env,
                &[resolve],
                &[],
                &tracked,
                &[market_key],
                0,
                None,
            ));
            let mint_frame = env.svm.get_account(&env.mint).unwrap();
            let mint = Mint::unpack(&mint_frame.data).unwrap();
            assert_eq!((mint.supply, mint.decimals), (PRINCIPAL, 6));
            assert_eq!(
                (mint.mint_authority, mint.freeze_authority),
                (COption::None, COption::None)
            );
            let identity = (
                env.portfolio_id(portfolio),
                env.portfolio_position_epoch(portfolio),
            );
            let funded_portfolio = env.svm.get_account(&portfolio).unwrap();
            let check_stock = |env: &V16CuEnv, paid: bool| {
                let account = env.svm.get_account(&env.market).unwrap();
                let (_, group) = env.market_state();
                let p = env.portfolio_state(portfolio);
                let remaining = if paid { 0 } else { PRINCIPAL };
                assert_eq!(
                    (group.mode, group.resolved_slot),
                    (MarketModeV16::Resolved, RESOLVE)
                );
                assert_eq!(
                    (group.c_tot, group.vault, group.insurance, group.pnl_pos_tot),
                    (remaining.into(), remaining.into(), 0, 0)
                );
                assert_eq!(group.materialized_portfolio_count, 1);
                assert_eq!(p.owner, owner_key.to_bytes());
                assert_eq!(p.capital.get(), remaining.into());
                assert_eq!(
                    (
                        p.pnl.get(),
                        p.reserved_pnl.get(),
                        p.cancel_deposit_escrow.get()
                    ),
                    (0, 0, 0)
                );
                assert_eq!(p.fee_credits.get(), 0);
                assert!(!resolved_receipt(&p).present);
                assert!(p.source_domains.iter().all(|source| !source.is_occupied()));
                assert!(percolator::active_bitmap_is_empty(active_bitmap(&p)));
                assert!(close_progress(&p).is_empty());
                assert_eq!(
                    (
                        env.portfolio_id(portfolio),
                        env.portfolio_position_epoch(portfolio)
                    ),
                    identity
                );
                assert_eq!(
                    env.svm.get_account(&portfolio).unwrap().lamports,
                    funded_portfolio.lamports
                );
                assert_eq!(resolved_portfolio_is_terminal(env, portfolio), paid);
                if !paid {
                    assert_eq!(
                        env.svm.get_account(&portfolio),
                        Some(funded_portfolio.clone())
                    );
                }
                assert_eq!(
                    env.svm.get_account(&owner_key),
                    Some(multisig_frame.clone())
                );
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                token_amount_frame(env, env.vault, &empty_vault, remaining);
                assert_market_stock_census(
                    "multisig terminal custody",
                    &group,
                    &account.data,
                    &[p],
                    remaining.into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census("multisig terminal custody", &group, &[p])
                    .unwrap();
            };
            check_stock(&env, false);
            env.svm.warp_to_slot(RESOLVE + DELAY - 1);
            peak = peak.max(step(
                &mut env,
                &[create.clone(), payout.clone()],
                &[],
                &tracked,
                &[],
                0,
                Some((
                    1,
                    InstructionError::Custom(PercolatorError::ExpectedSigner as u32),
                )),
            ));
            rollbacks += 1;
            check_stock(&env, false);
            env.svm.warp_to_slot(RESOLVE + DELAY);

            // The payout succeeds before SPL rejects the missing second member.
            // Rollback must restore the claim, vault and absent ATA, including rent.
            peak = peak.max(step(
                &mut env,
                &[create.clone(), payout.clone(), transfer(&quorum_keys[..1])],
                &quorum[..1],
                &tracked,
                &[],
                0,
                Some((2, InstructionError::MissingRequiredSignature)),
            ));
            rollbacks += 1;
            check_stock(&env, false);
            closed(&env, destination);
            token_amount_frame(&env, sink, &empty_sink, 0);

            peak = peak.max(step(
                &mut env,
                &[create, payout.clone()],
                &[],
                &tracked,
                &[market_key, portfolio, vault_key, destination],
                rent,
                None,
            ));
            check_stock(&env, true);
            token_amount_frame(&env, destination, &empty_destination, PRINCIPAL);
            token_amount_frame(&env, sink, &empty_sink, 0);
            peak = peak.max(step(
                &mut env,
                &[payout],
                &[],
                &tracked,
                &[],
                0,
                Some((
                    0,
                    InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                )),
            ));
            rollbacks += 1;

            let wallet_before = env.svm.get_account(&recipient).unwrap();
            peak = peak.max(step(
                &mut env,
                &[transfer(&quorum_keys), dispose],
                &quorum,
                &tracked,
                &[destination, sink, recipient],
                0,
                None,
            ));
            let mut wallet_after = wallet_before;
            wallet_after.lamports += rent;
            assert_eq!(env.svm.get_account(&recipient), Some(wallet_after));
            closed(&env, destination);
            token_amount_frame(&env, sink, &empty_sink, PRINCIPAL);
            check_stock(&env, true);

            let delete = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                data: env.close_portfolio_ix(portfolio).encode(),
            };
            let slab = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(admin_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            let insufficient = spl_token::instruction::transfer(
                &spl_token::ID,
                &sink,
                &admin_token,
                &recipient,
                &[],
                PRINCIPAL + 1,
            )
            .unwrap();
            peak = peak.max(step(
                &mut env,
                &[delete.clone(), slab.clone(), insufficient],
                &[&admin, quorum[0]],
                &tracked,
                &[],
                0,
                Some((
                    2,
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32,
                    ),
                )),
            ));
            rollbacks += 1;
            check_stock(&env, true);
            let market_before = env.svm.get_account(&env.market).unwrap();
            let mut admin_after = env.svm.get_account(&admin.pubkey()).unwrap();
            peak = peak.max(step(
                &mut env,
                &[delete, slab],
                &[&admin],
                &tracked,
                &[market_key, portfolio, vault_key, admin.pubkey()],
                0,
                None,
            ));
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                env.svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
            );
            admin_after.lamports +=
                market_before.lamports + funded_portfolio.lamports + rent - tombstone.lamports;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(admin_after));
            for key in [portfolio, env.vault, destination] {
                closed(&env, key);
            }
            token_amount_frame(&env, sink, &empty_sink, PRINCIPAL);
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
            assert_eq!(env.svm.get_account(&owner_key), Some(multisig_frame));
            worlds += 1;
        }
    }
    assert_eq!((worlds, rollbacks), (6, 24));
    println!("row418 multisig terminal custody: worlds={worlds}, rollbacks={rollbacks}, unsigned_payouts={worlds}, quorum_redemptions={worlds}, slab_closes={worlds}, peak_CU={peak}, limit={LIMIT}");
}
