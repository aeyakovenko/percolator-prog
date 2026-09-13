//! INV-070 / row 418: the same sweep address crosses classic SPL, Token-2022,
//! uninitialized SPL, and repaired SPL custody around funded terminal progress.
//! Public instructions only; four finite mint-authority/repair-order histories.
//! Token-2022 is a rejected destination, not an admitted quote-token variant.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const PRINCIPAL: u64 = 101;
const BACKING: u64 = 307;
const SURPLUS: u64 = 17;
const EXPIRY: u64 = 5;
const LIMIT: u64 = 300_000;

fn submit(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    rejected_index: Option<usize>,
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
    assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(index) = rejected_index {
        let failure = result.expect_err("unsupported or uninitialized terminal custody");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                (index + 2) as u8,
                InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32),
            ),
            "logs={:?}",
            failure.meta.logs
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| { **line == format!("Program {} success", env.program_id) })
                .count(),
            instructions[..index]
                .iter()
                .filter(|ix| ix.program_id == env.program_id)
                .count(),
            "the payout/deletion prefix must execute before custody rejection"
        );
        for (key, mut account) in keys.iter().zip(before) {
            if *key == env.payer.pubkey() {
                account.as_mut().unwrap().lamports -= fee;
            }
            assert_eq!(
                env.svm.get_account(key),
                account,
                "complete Account rollback {key}"
            );
        }
        failure.meta
    } else {
        result.expect("bounded public continuation")
    };
    assert_cu_within(
        "terminal custody program recreation",
        meta.compute_units_consumed,
        LIMIT,
    );
    meta.compute_units_consumed
}

fn amount_frame(env: &V16CuEnv, key: Pubkey, empty: &Account, amount: u64) {
    let mut expected = empty.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.amount, 0);
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    assert_eq!(env.svm.get_account(&key), Some(expected));
}

fn closed(env: &V16CuEnv, key: Pubkey) {
    assert!(env
        .svm
        .get_account(&key)
        .is_none_or(|a| { a.lamports == 0 && a.data.iter().all(|byte| *byte == 0) }));
}

#[test]
fn v16_program_terminal_custody_program_recreation_preserves_funded_retirement() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;

    for fixed_supply in [false, true] {
        for bundled_repair in [false, true] {
            let mut env = inv018_public_spl_market(6);
            env.svm.add_program(
                spl_token_2022::ID,
                &std::fs::read(spl_token_2022_program_path()).unwrap(),
            );
            let admin = env.admin.insecure_clone();
            let owner = Keypair::new();
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            let portfolio_key = Keypair::new();
            let portfolio = portfolio_key.pubkey();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                env.portfolio_account_len,
                env.program_id,
            );
            let init_cu = env
                .send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                    ],
                    &[&owner],
                )
                .unwrap();
            env.portfolios.push(portfolio);
            let user_token =
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
            let funding = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let destination_key = Keypair::new();
            let destination = destination_key.pubkey();
            assert_ne!(
                destination, funding,
                "same-address recreation uses non-ATA custody"
            );
            let foreign_mint = Keypair::new();
            let tracked = [
                env.market,
                portfolio,
                env.vault,
                env.mint,
                user_token,
                funding,
                destination,
                foreign_mint.pubkey(),
                owner.pubkey(),
                admin.pubkey(),
            ];
            let empty_user = env.svm.get_account(&user_token).unwrap();
            let empty_funding = env.svm.get_account(&funding).unwrap();
            let empty_vault = env.svm.get_account(&env.vault).unwrap();
            let token_rent = env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            let payer_key = env.payer.pubkey();
            let create_destination = |program| {
                system_instruction::create_account(
                    &payer_key,
                    &destination,
                    token_rent,
                    TokenAccount::LEN as u64,
                    &program,
                )
            };
            let initialize = spl_token::instruction::initialize_account3(
                &spl_token::ID,
                &destination,
                &env.mint,
                &admin.pubkey(),
            )
            .unwrap();
            let mut peak = env.init_market_cu.max(init_cu);
            let mut run = |env: &mut V16CuEnv, ixs: &[Instruction], signers: &[&Keypair], error| {
                peak = peak.max(submit(env, ixs, signers, &tracked, error));
            };
            run(
                &mut env,
                &[create_destination(spl_token::ID), initialize.clone()],
                &[&destination_key],
                None,
            );
            let empty_destination = env.svm.get_account(&destination).unwrap();
            let mut minting = [(user_token, PRINCIPAL), (funding, BACKING + SURPLUS)]
                .map(|(destination, amount)| {
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &destination,
                        &admin.pubkey(),
                        &[],
                        amount,
                    )
                    .unwrap()
                })
                .to_vec();
            if fixed_supply {
                minting.push(
                    spl_token::instruction::set_authority(
                        &spl_token::ID,
                        &env.mint,
                        None,
                        spl_token::instruction::AuthorityType::MintTokens,
                        &admin.pubkey(),
                        &[],
                    )
                    .unwrap(),
                );
            }
            run(&mut env, &minting, &[&admin], None);
            let deposit = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(user_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.deposit_ix(portfolio, PRINCIPAL.into()).encode(),
            };
            run(&mut env, &[deposit], &[&owner], None);
            env.svm.warp_to_slot(1);
            let backing_cu = env.top_up_backing_bucket_from_admin_token_with_cu(
                funding,
                1,
                BACKING.into(),
                EXPIRY,
            );
            let donation = spl_token::instruction::transfer(
                &spl_token::ID,
                &funding,
                &env.vault,
                &admin.pubkey(),
                &[],
                SURPLUS,
            )
            .unwrap();
            run(&mut env, &[donation], &[&admin], None);
            let assert_stock = |env: &V16CuEnv, paid: bool| {
                let (_, group) = env.market_state();
                let portfolios = if paid {
                    vec![]
                } else {
                    vec![env.portfolio_state(portfolio)]
                };
                let capital = if paid { 0 } else { PRINCIPAL };
                assert_eq!(
                    (group.c_tot, group.insurance, group.vault),
                    (capital.into(), 0, (capital + BACKING).into())
                );
                assert_eq!(group.materialized_portfolio_count, u64::from(!paid));
                assert_eq!(
                    group.source_backing_buckets[1].status,
                    if paid {
                        BackingBucketStatusV16::Expired
                    } else {
                        BackingBucketStatusV16::Fresh
                    }
                );
                assert_market_stock_census(
                    "recreated terminal custody",
                    &group,
                    &env.svm.get_account(&env.market).unwrap().data,
                    &portfolios,
                    (capital + BACKING).into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census(
                    "recreated custody labels",
                    &group,
                    &portfolios,
                )
                .unwrap();
                amount_frame(env, env.vault, &empty_vault, capital + BACKING + SURPLUS);
                amount_frame(env, user_token, &empty_user, PRINCIPAL - capital);
                assert_eq!(env.svm.get_account(&funding), Some(empty_funding.clone()));
            };
            assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
            assert_stock(&env, false);
            let resolve_cu = env.resolve();
            env.svm.warp_to_slot(EXPIRY);
            assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
            let mint_before = env.svm.get_account(&env.mint).unwrap();
            let mint = Mint::unpack(&mint_before.data).unwrap();
            assert_eq!(mint.supply, PRINCIPAL + BACKING + SURPLUS);
            assert_eq!(
                mint.mint_authority,
                if fixed_supply {
                    COption::None
                } else {
                    COption::Some(admin.pubkey())
                }
            );
            let owner_before = env.svm.get_account(&owner.pubkey());
            let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
            let market_before = env.svm.get_account(&env.market).unwrap();
            let portfolio_rent = env.svm.get_account(&portfolio).unwrap().lamports;

            let close_token = |program| {
                spl_token_2022::instruction::close_account(
                    &program,
                    &destination,
                    &admin.pubkey(),
                    &admin.pubkey(),
                    &[],
                )
                .unwrap()
            };
            run(&mut env, &[close_token(spl_token::ID)], &[&admin], None);
            closed(&env, destination);
            expected_admin.lamports += token_rent;
            let foreign_mint_rent = env.svm.minimum_balance_for_rent_exemption(Mint::LEN);
            let foreign_setup = [
                system_instruction::create_account(
                    &payer_key,
                    &foreign_mint.pubkey(),
                    foreign_mint_rent,
                    Mint::LEN as u64,
                    &spl_token_2022::ID,
                ),
                spl_token_2022::instruction::initialize_mint2(
                    &spl_token_2022::ID,
                    &foreign_mint.pubkey(),
                    &admin.pubkey(),
                    None,
                    6,
                )
                .unwrap(),
                create_destination(spl_token_2022::ID),
                spl_token_2022::instruction::initialize_account3(
                    &spl_token_2022::ID,
                    &destination,
                    &foreign_mint.pubkey(),
                    &admin.pubkey(),
                )
                .unwrap(),
            ];
            run(
                &mut env,
                &foreign_setup,
                &[&foreign_mint, &destination_key],
                None,
            );
            let foreign_account = env.svm.get_account(&destination).unwrap();
            assert_eq!(foreign_account.owner, spl_token_2022::ID);
            assert_eq!(foreign_account.data.len(), TokenAccount::LEN);
            let token = spl_token_2022::state::Account::unpack(&foreign_account.data).unwrap();
            assert_eq!(
                (token.mint, token.owner, token.amount),
                (foreign_mint.pubkey(), admin.pubkey(), 0)
            );
            assert_eq!(
                token.state,
                spl_token_2022::state::AccountState::Initialized
            );
            let foreign_mint_frame = env.svm.get_account(&foreign_mint.pubkey());
            assert_stock(&env, false);

            let payout = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(user_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
                .encode(),
            };
            let delete = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                data: env.close_portfolio_ix(portfolio).encode(),
            };
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
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            run(
                &mut env,
                &[payout.clone(), delete.clone(), close.clone()],
                &[&admin],
                Some(2),
            );
            assert_stock(&env, false);

            // A valid alternate permits principal payment and expiry normalization
            // while the original address is still owned by the unsupported program.
            let mut normalize = close.clone();
            normalize.accounts[4].pubkey = funding;
            run(&mut env, &[payout, delete, normalize], &[&admin], None);
            closed(&env, portfolio);
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().lamports,
                market_before.lamports + portfolio_rent
            );
            assert_stock(&env, true);
            assert_eq!(env.svm.get_account(&destination), Some(foreign_account));
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_before.clone()));

            run(
                &mut env,
                &[close_token(spl_token_2022::ID)],
                &[&admin],
                None,
            );
            closed(&env, destination);
            expected_admin.lamports += token_rent;
            run(
                &mut env,
                &[create_destination(spl_token::ID)],
                &[&destination_key],
                None,
            );
            let uninitialized = env.svm.get_account(&destination).unwrap();
            assert_eq!(uninitialized.owner, spl_token::ID);
            assert_eq!(uninitialized.data, vec![0; TokenAccount::LEN]);
            assert_eq!(uninitialized.lamports, token_rent);
            run(&mut env, &[close.clone()], &[&admin], Some(0));
            assert_stock(&env, true);
            assert_eq!(
                env.svm.get_account(&admin.pubkey()),
                Some(expected_admin.clone())
            );

            if bundled_repair {
                run(&mut env, &[initialize, close], &[&admin], None);
            } else {
                run(&mut env, &[initialize], &[], None);
                assert_eq!(
                    env.svm.get_account(&destination),
                    Some(empty_destination.clone())
                );
                assert_stock(&env, true);
                run(&mut env, &[close], &[&admin], None);
            }
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            let tombstone_rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            assert_eq!(tombstone.lamports, tombstone_rent);
            expected_admin.lamports +=
                market_before.lamports + portfolio_rent - tombstone_rent + empty_vault.lamports;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert_eq!(env.svm.get_account(&owner.pubkey()), owner_before);
            closed(&env, env.vault);
            closed(&env, portfolio);
            amount_frame(&env, user_token, &empty_user, PRINCIPAL);
            amount_frame(&env, destination, &empty_destination, SURPLUS);
            assert_eq!(env.svm.get_account(&funding), Some(empty_funding));
            let mut retired_mint = mint_before;
            let mut mint = Mint::unpack(&retired_mint.data).unwrap();
            mint.supply = PRINCIPAL + SURPLUS;
            Mint::pack(mint, &mut retired_mint.data).unwrap();
            assert_eq!(env.svm.get_account(&env.mint), Some(retired_mint));
            assert_eq!(
                env.svm.get_account(&foreign_mint.pubkey()),
                foreign_mint_frame
            );
            peak = peak.max(backing_cu).max(resolve_cu);
            assert_cu_within("funded custody recreation history", peak, LIMIT);
            eprintln!("row418 custody program recreation: fixed_supply={fixed_supply}, bundled_repair={bundled_repair}, principal={PRINCIPAL}, retired={BACKING}, surplus={SURPLUS}, rollbacks=2, successful_CloseSlab=2, peak_CU={peak}, headroom={}", LIMIT - peak);
        }
    }
}
