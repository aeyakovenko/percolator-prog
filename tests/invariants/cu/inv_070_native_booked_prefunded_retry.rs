//! Row 418: prefunded canonical custody beside nonzero booked native retirement.
//! Prior insurance redemption, external wrapping, residue and rent stay distinct.

use super::*;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market;

const PAID_INSURANCE: u64 = 37;
const BOOKED: u64 = 101;
const DONATION: u64 = 19;
const EXTRA: u64 = 23;

fn check_book(env: &V16CuEnv, paid: bool, expired: bool) {
    let market = env.svm.get_account(&env.market).unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let insurance = if paid { 0 } else { PAID_INSURANCE };
    assert_eq!(group.mode, MarketModeV16::Resolved);
    assert_eq!(
        (
            group.c_tot,
            group.pnl_pos_tot,
            group.materialized_portfolio_count
        ),
        (0, 0, 0)
    );
    assert_eq!(group.source_claim_bound_total_num, 0);
    assert_eq!(group.backing_provider_earnings_total, 0);
    assert_eq!(group.vault, u128::from(BOOKED + insurance));
    assert_eq!(group.insurance, u128::from(insurance));
    assert_eq!(
        group.insurance_domain_budget_remaining_total,
        u128::from(insurance)
    );
    assert_eq!(group.insurance_domain_budget[0], u128::from(insurance));
    assert!(group.insurance_domain_budget[1..]
        .iter()
        .all(|value| *value == 0));
    assert!(group.insurance_domain_spent.iter().all(|value| *value == 0));
    assert_eq!(
        group.source_backing_buckets[0].status,
        if expired {
            BackingBucketStatusV16::Expired
        } else {
            BackingBucketStatusV16::Fresh
        }
    );
    assert_eq!(
        group.source_backing_buckets[0].fresh_unliened_backing_num,
        if expired {
            0
        } else {
            u128::from(BOOKED) * BOUND_SCALE
        }
    );
    assert_market_stock_census(
        "prefunded native residue",
        &group,
        &market.data,
        &[],
        u128::from(BOOKED + insurance),
    )
    .unwrap();
    assert_reservation_encumbrance_census("prefunded native residue", &group, &[]).unwrap();
}

#[test]
fn v16_program_native_booked_residue_prefunded_custody_retry_preserves_value() {
    for above_rent in [false, true] {
        let mut env = inv081_public_native_market();
        let admin = env.admin.insecure_clone();
        let insurer = Keypair::new();
        let beneficiary = insurer.pubkey();
        env.svm.airdrop(&beneficiary, 1_000_000_000).unwrap();
        env.try_update_per_asset_authority_with_cu(
            &admin,
            Some(&insurer),
            0,
            processor::ASSET_AUTH_INSURANCE,
            beneficiary.to_bytes(),
        )
        .unwrap();
        let recipient = create_ata_for_test(&mut env.svm, &env.payer, beneficiary, env.mint);
        let destination = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
        let empty_recipient = env.svm.get_account(&recipient).unwrap();
        let empty_destination = env.svm.get_account(&destination).unwrap();
        let empty_vault = env.svm.get_account(&env.vault).unwrap();
        let mint_frame = env.svm.get_account(&env.mint);
        let rent = env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN);
        let system_rent = env.svm.minimum_balance_for_rent_exemption(0);
        assert!(system_rent + EXTRA < rent);
        let prefund = if above_rent {
            rent + EXTRA
        } else {
            system_rent + EXTRA
        };
        let external_wrapped = prefund.saturating_sub(rent);
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::transfer(&beneficiary, &recipient, PAID_INSURANCE),
                spl_token::instruction::sync_native(&spl_token::ID, &recipient).unwrap(),
                system_instruction::transfer(&admin.pubkey(), &destination, BOOKED),
                spl_token::instruction::sync_native(&spl_token::ID, &destination).unwrap(),
            ],
            &[&insurer, &admin],
        )
        .unwrap();
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                domain: 0,
                market_id: env.asset_market_id(0),
                authority_epoch: env.control_sequences(0).authority_epoch,
                intent_id: env.control_sequences(0).insurance_top_up + 1,
                amount: PAID_INSURANCE.into(),
            },
            vec![
                AccountMeta::new(beneficiary, true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(recipient, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&insurer],
        )
        .unwrap();
        env.svm.warp_to_slot(1);
        env.top_up_backing_bucket_from_admin_token_with_cu(destination, 0, BOOKED.into(), 100);
        env.resolve();
        check_book(&env, false, false);
        assert_eq!(
            env.svm.get_account(&recipient),
            Some(empty_recipient.clone())
        );
        assert_eq!(
            env.svm.get_account(&destination),
            Some(empty_destination.clone())
        );
        assert_eq!(
            env.svm.get_account(&env.vault),
            Some(token_image(&empty_vault, BOOKED + PAID_INSURANCE))
        );

        let market = env.market;
        let vault = env.vault;
        let departure = Pubkey::new_unique();
        let tracked = [
            market,
            vault,
            env.mint,
            recipient,
            destination,
            beneficiary,
            admin.pubkey(),
            departure,
        ];
        let mut peak = 0;
        let mut step = |env: &mut V16CuEnv,
                        ixs: &[Instruction],
                        signers: &[&Keypair],
                        allowed: &[Pubkey],
                        rent,
                        refund,
                        rejection| {
            peak = peak.max(checked_land(
                env, ixs, signers, &tracked, allowed, rent, refund, rejection,
            ));
        };
        let mut close = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
                AccountMeta::new(recipient, false),
            ],
            data: ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            }
            .encode(),
        };
        let withdrawal = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(beneficiary, false),
                AccountMeta::new(market, false),
                AccountMeta::new(recipient, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env
                .withdraw_insurance_asset_instruction(beneficiary, 0, PAID_INSURANCE.into())
                .encode(),
        };
        let sequences = env.control_sequences(0);
        step(
            &mut env,
            &[withdrawal.clone(), close.clone()],
            &[&admin],
            &[],
            0,
            None,
            Some((3, PercolatorError::EngineStale)),
        );
        assert_eq!(env.control_sequences(0), sequences);
        step(
            &mut env,
            &[withdrawal],
            &[],
            &[market, vault, recipient],
            0,
            None,
            None,
        );
        let mut paid_sequences = sequences;
        paid_sequences.authority_epoch += 1;
        assert_eq!(env.control_sequences(0), paid_sequences);
        check_book(&env, true, false);
        assert_eq!(
            env.svm.get_account(&recipient),
            Some(token_image(&empty_recipient, PAID_INSURANCE))
        );
        let redeem = spl_token::instruction::close_account(
            &spl_token::ID,
            &recipient,
            &beneficiary,
            &beneficiary,
            &[],
        )
        .unwrap();
        step(
            &mut env,
            &[redeem],
            &[&insurer],
            &[recipient],
            0,
            Some((beneficiary, rent + PAID_INSURANCE)),
            None,
        );
        assert_absent(&env, recipient);
        let departed = env.svm.get_account(&beneficiary).unwrap().lamports;
        step(
            &mut env,
            &[system_instruction::transfer(
                &beneficiary,
                &departure,
                departed,
            )],
            &[&insurer],
            &[beneficiary, departure],
            0,
            None,
            None,
        );
        assert_absent(&env, beneficiary);
        assert_eq!(env.svm.get_account(&departure).unwrap().lamports, departed);
        drop(insurer);

        // A system-owned canonical address can contain rent funding or extra native value.
        step(
            &mut env,
            &[system_instruction::transfer(
                &admin.pubkey(),
                &recipient,
                prefund,
            )],
            &[&admin],
            &[admin.pubkey(), recipient],
            0,
            None,
            None,
        );
        let prefunded = env.svm.get_account(&recipient).unwrap();
        assert_eq!(prefunded.owner, solana_sdk::system_program::ID);
        assert!(prefunded.data.is_empty());
        assert!(!prefunded.executable);
        assert_eq!(prefunded.lamports, prefund);
        close.data = ProgInstruction::CloseSlab {
            authority_epoch: paid_sequences.authority_epoch,
        }
        .encode();
        env.svm.warp_to_slot(100);
        step(
            &mut env,
            &[close.clone()],
            &[&admin],
            &[market],
            0,
            None,
            None,
        );
        check_book(&env, true, true);
        assert_eq!(
            env.svm.get_account(&vault),
            Some(token_image(&empty_vault, BOOKED))
        );
        step(
            &mut env,
            &[
                system_instruction::transfer(&admin.pubkey(), &vault, DONATION),
                spl_token::instruction::sync_native(&spl_token::ID, &vault).unwrap(),
            ],
            &[&admin],
            &[admin.pubkey(), vault],
            0,
            None,
            None,
        );
        check_book(&env, true, true);
        assert_eq!(
            env.svm.get_account(&vault),
            Some(token_image(&empty_vault, BOOKED + DONATION))
        );
        let before_close = env.svm.get_account(&market).unwrap();
        let repair = Instruction {
            program_id: associated_token_program_id(),
            accounts: vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(recipient, false),
                AccountMeta::new_readonly(beneficiary, false),
                AccountMeta::new_readonly(env.mint, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: vec![1],
        };
        let mut redirected = close.clone();
        redirected.accounts[7] = AccountMeta::new(destination, false);
        step(
            &mut env,
            &[repair.clone(), redirected],
            &[&admin],
            &[],
            0,
            None,
            Some((3, PercolatorError::InvalidTokenAccount)),
        );
        let mut denied = close.clone();
        denied.accounts[0] = AccountMeta::new(beneficiary, false);
        step(
            &mut env,
            &[repair.clone(), close.clone(), denied],
            &[&admin],
            &[],
            0,
            None,
            Some((4, PercolatorError::ExpectedSigner)),
        );
        assert_eq!(env.svm.get_account(&recipient), Some(prefunded));
        assert_eq!(env.svm.get_account(&market), Some(before_close.clone()));
        assert_eq!(env.control_sequences(0), paid_sequences);
        check_book(&env, true, true);

        let tombstone_rent = env
            .svm
            .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
        let refund = before_close.lamports + rent - tombstone_rent;
        step(
            &mut env,
            &[repair.clone(), close],
            &[&admin],
            &[market, vault, recipient, destination],
            rent.saturating_sub(prefund),
            Some((admin.pubkey(), refund)),
            None,
        );
        let tombstone = env.svm.get_account(&market).unwrap();
        assert_closed_market_tombstone(&tombstone);
        assert_eq!(tombstone.lamports, tombstone_rent);
        assert_absent(&env, vault);
        assert_absent(&env, beneficiary);
        assert_eq!(
            env.svm.get_account(&recipient),
            Some(token_image(&empty_recipient, BOOKED + external_wrapped))
        );
        assert_eq!(
            env.svm.get_account(&destination),
            Some(token_image(&empty_destination, DONATION))
        );
        assert_eq!(env.svm.get_account(&departure).unwrap().lamports, departed);
        assert_eq!(env.svm.get_account(&env.mint), mint_frame);
        // Idempotent repair after retirement cannot pay residue or rent a second time.
        step(&mut env, &[repair], &[], &[], 0, None, None);
        assert_eq!(env.svm.get_account(&market), Some(tombstone));
        println!("row418 prefund_above_rent={above_rent}: paid={PAID_INSURANCE}, residue={BOOKED}, external_wrapped={external_wrapped}, surplus={DONATION}, exact_rollbacks=3, repairs=1, retirements=1, peak={peak} CU");
    }
}
