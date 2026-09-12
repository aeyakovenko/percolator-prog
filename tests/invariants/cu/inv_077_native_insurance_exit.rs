//! Row 418 / INV-077: a signed native-insurance exit survives partial redemption
//! and public custody recreation. Remaining insurance is the bounded payout rank.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const CU_LIMIT: u64 = 150_000;

fn run(env: &mut V16CuEnv, instructions: Vec<Instruction>, signers: &[&Keypair]) -> u64 {
    env.svm.expire_blockhash();
    let mut ixs = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
    ];
    ixs.extend(instructions);
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
    let meta = env
        .svm
        .send_transaction(tx)
        .expect("bounded native insurance step");
    assert_cu_within(
        "native insurance exit",
        meta.compute_units_consumed,
        CU_LIMIT,
    );
    meta.compute_units_consumed
}

fn recreate_custody(env: &mut V16CuEnv, owner: Pubkey, token: Pubkey) -> u64 {
    let ix = Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(token, false),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
        ],
        data: vec![],
    };
    run(env, vec![ix], &[])
}

fn assert_native(env: &V16CuEnv, key: Pubkey, initial: &Account, amount: u64, raw: u64) {
    let mut expected = initial.clone();
    let mut token = TokenAccount::unpack(&expected.data).unwrap();
    assert_eq!(token.amount, 0);
    assert_eq!(token.is_native, COption::Some(initial.lamports));
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected.lamports += amount + raw;
    assert_eq!(env.svm.get_account(&key), Some(expected));
}

#[test]
fn v16_program_native_insurance_partial_redemption_reaches_bounded_terminal_exit() {
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

    const LONG: u64 = 47;
    const SHORT: u64 = 59;
    const INSURANCE: u64 = LONG + SHORT;
    const SURPLUS: u64 = 17;
    const RAW: u64 = 19;

    for first in [13, 61] {
        for sync_surplus in [false, true] {
            let mut env = inv081_public_native_market();
            let admin = env.admin.insecure_clone();
            let beneficiary = Keypair::new();
            env.svm
                .airdrop(&beneficiary.pubkey(), 1_000_000_000)
                .unwrap();
            let mut peak = env.init_market_cu;
            peak = peak.max(
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(&beneficiary),
                    0,
                    processor::ASSET_AUTH_INSURANCE,
                    beneficiary.pubkey().to_bytes(),
                )
                .unwrap(),
            );
            let destination =
                create_ata_for_test(&mut env.svm, &env.payer, beneficiary.pubkey(), env.mint);
            let admin_token =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            let empty_destination = env.svm.get_account(&destination).unwrap();
            let empty_admin_token = env.svm.get_account(&admin_token).unwrap();
            let empty_vault = env.svm.get_account(&env.vault).unwrap();
            let rent = empty_destination.lamports;
            let mint = env.svm.get_account(&env.mint);
            let authority = env.svm.get_account(&env.vault_authority);
            let funding = vec![
                system_instruction::transfer(&beneficiary.pubkey(), &destination, INSURANCE),
                spl_token::instruction::sync_native(&spl_token::ID, &destination).unwrap(),
                system_instruction::transfer(&admin.pubkey(), &env.vault, SURPLUS),
                spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap(),
                system_instruction::transfer(&admin.pubkey(), &env.vault, RAW),
            ];
            peak = peak.max(run(&mut env, funding, &[&beneficiary, &admin]));
            for (domain, amount) in [(0, LONG), (1, SHORT)] {
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(beneficiary.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::TopUpInsuranceDomain {
                        domain,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        intent_id: env.control_sequences(0).insurance_top_up + 1,
                        amount: amount.into(),
                    }
                    .encode(),
                };
                peak = peak.max(run(&mut env, vec![ix], &[&beneficiary]));
            }
            peak = peak.max(env.resolve());
            let cfg = env.market_state().0;
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            assert_eq!(profile.insurance_authority, beneficiary.pubkey().to_bytes());
            assert_eq!(profile.insurance_operator, admin.pubkey().to_bytes());
            let sequences = env.control_sequences(0);
            let market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
            let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
            let beneficiary_before = env.svm.get_account(&beneficiary.pubkey()).unwrap();

            let check = |env: &V16CuEnv, paid: u64, custody_amount: u64| {
                let market = env.svm.get_account(&env.market).unwrap();
                let (current_cfg, group) = state::read_market(&market.data).unwrap();
                let remaining = INSURANCE - paid;
                assert_eq!(current_cfg, cfg);
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(group.materialized_portfolio_count, 0);
                assert_eq!(
                    (group.c_tot, group.vault, group.insurance),
                    (0, remaining.into(), remaining.into())
                );
                assert_eq!(
                    group.insurance_domain_budget[0],
                    LONG.saturating_sub(paid).into()
                );
                assert_eq!(
                    group.insurance_domain_budget[1],
                    (SHORT - paid.saturating_sub(LONG)).into()
                );
                assert!(group.insurance_domain_spent.iter().all(|spent| *spent == 0));
                assert_eq!(
                    state::read_asset_oracle_profile(&market.data, 0).unwrap(),
                    profile
                );
                assert_eq!(env.control_sequences(0), sequences);
                assert_eq!(market.lamports, market_lamports);
                assert_native(env, env.vault, &empty_vault, remaining + SURPLUS, RAW);
                assert_native(env, destination, &empty_destination, custody_amount, 0);
                assert_native(env, admin_token, &empty_admin_token, 0, 0);
                assert_eq!(env.svm.get_account(&env.mint), mint);
                assert_eq!(env.svm.get_account(&env.vault_authority), authority);
                assert_eq!(
                    env.svm.get_account(&admin.pubkey()),
                    Some(admin_before.clone())
                );
                assert_market_stock_census(
                    "native terminal insurance",
                    &group,
                    &market.data,
                    &[],
                    (env.token_amount(env.vault) - SURPLUS).into(),
                )
                .unwrap();
                assert_reservation_encumbrance_census("native terminal insurance", &group, &[])
                    .unwrap();
                let mut data = market.data.clone();
                state::market_view_mut(&mut data)
                    .unwrap()
                    .1
                    .validate_shape()
                    .unwrap();
            };
            check(&env, 0, 0);
            let mut paid = 0;
            for (index, amount) in [first, INSURANCE - first].into_iter().enumerate() {
                let rank_before = env.market_state().1.insurance;
                assert!(rank_before > 0);
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(beneficiary.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env
                        .withdraw_insurance_asset_instruction(
                            beneficiary.pubkey(),
                            0,
                            amount.into(),
                        )
                        .encode(),
                };
                peak = peak.max(run(&mut env, vec![ix], &[&beneficiary]));
                paid += amount;
                let rank_after = env.market_state().1.insurance;
                assert_eq!(rank_before - rank_after, amount.into());
                assert!(rank_after < rank_before);
                check(&env, paid, amount);

                let frame_keys = [
                    env.market,
                    env.vault,
                    env.mint,
                    env.vault_authority,
                    admin_token,
                    admin.pubkey(),
                ];
                let frame = frame_keys.map(|key| env.svm.get_account(&key));
                let redeem = spl_token::instruction::close_account(
                    &spl_token::ID,
                    &destination,
                    &beneficiary.pubkey(),
                    &beneficiary.pubkey(),
                    &[],
                )
                .unwrap();
                peak = peak.max(run(&mut env, vec![redeem], &[&beneficiary]));
                assert!(env
                    .svm
                    .get_account(&destination)
                    .is_none_or(|account| account.lamports == 0
                        && account.data.iter().all(|byte| *byte == 0)));
                let mut expected = beneficiary_before.clone();
                expected.lamports += paid + (index as u64 + 1) * rent;
                assert_eq!(env.svm.get_account(&beneficiary.pubkey()), Some(expected));
                assert_eq!(frame_keys.map(|key| env.svm.get_account(&key)), frame);
                if index == 0 {
                    peak = peak.max(recreate_custody(
                        &mut env,
                        beneficiary.pubkey(),
                        destination,
                    ));
                    check(&env, paid, 0);
                    assert_eq!(frame_keys.map(|key| env.svm.get_account(&key)), frame);
                }
            }
            assert_eq!(paid, INSURANCE);
            assert_eq!(env.market_state().1.insurance, 0);
            let (sweep, raw) = if sync_surplus {
                let frame = env.svm.get_account(&env.market);
                let sync = spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap();
                peak = peak.max(run(&mut env, vec![sync], &[]));
                assert_eq!(env.svm.get_account(&env.market), frame);
                (SURPLUS + RAW, 0)
            } else {
                (SURPLUS, RAW)
            };
            assert_native(&env, env.vault, &empty_vault, sweep, raw);
            let beneficiary_final = env.svm.get_account(&beneficiary.pubkey());
            let close = Instruction {
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
                    authority_epoch: sequences.authority_epoch,
                }
                .encode(),
            };
            peak = peak.max(run(&mut env, vec![close], &[&admin]));
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                env.svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN,)
            );
            assert!(env.svm.get_account(&env.vault).is_none_or(
                |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
            ));
            assert_native(&env, admin_token, &empty_admin_token, sweep, 0);
            let mut expected_admin = admin_before.clone();
            expected_admin.lamports +=
                market_lamports + empty_vault.lamports + raw - tombstone.lamports;
            assert_eq!(
                env.svm.get_account(&admin.pubkey()),
                Some(expected_admin.clone())
            );
            let redeem = spl_token::instruction::close_account(
                &spl_token::ID,
                &admin_token,
                &admin.pubkey(),
                &admin.pubkey(),
                &[],
            )
            .unwrap();
            peak = peak.max(run(&mut env, vec![redeem], &[&admin]));
            expected_admin.lamports += empty_admin_token.lamports + sweep;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert!(env.svm.get_account(&admin_token).is_none_or(
                |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
            ));
            assert_eq!(env.svm.get_account(&env.market), Some(tombstone));
            assert_eq!(
                env.svm.get_account(&beneficiary.pubkey()),
                beneficiary_final
            );
            assert_eq!(env.svm.get_account(&env.mint), mint);
            assert_eq!(env.svm.get_account(&env.vault_authority), authority);
            assert_cu_within("native insurance history peak", peak, CU_LIMIT);
            println!("INV-077 native insurance: first={first}, sync_surplus={sync_surplus}, paid={paid}, payouts=2, slab_calls=1, peak_CU={peak}");
        }
    }
}
