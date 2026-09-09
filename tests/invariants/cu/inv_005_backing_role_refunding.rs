//! INV-005: role containment through two-domain depletion, empty handoff, and refilling.
//! This non-oracle slice does not certify row 416's funded-oracle protection.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const PRINCIPAL: [u128; 2] = [17, 29];
const REFILL: u128 = 13;
const PEER_BACKING: u128 = 31;
const INSURANCE: u128 = 23;

fn mint_wallet(env: &mut V16CuEnv, owner: Pubkey, amount: u128) -> Pubkey {
    let wallet = create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint);
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
    .expect("public SPL funding");
    wallet
}

fn backing_transfer(
    env: &mut V16CuEnv,
    holder: &Keypair,
    wallet: Pubkey,
    domain: u16,
    amount: u128,
    withdraw: bool,
) -> u64 {
    let asset = domain / 2;
    let sequences = env.control_sequences(asset as usize);
    let market_id = env.asset_market_id(asset);
    let instruction = if withdraw {
        ProgInstruction::WithdrawBackingBucket {
            domain,
            market_id,
            authority_epoch: sequences.authority_epoch,
            amount,
        }
    } else {
        ProgInstruction::TopUpBackingBucket {
            domain,
            market_id,
            authority_epoch: sequences.authority_epoch,
            intent_id: next_control_sequence(sequences.backing_top_up),
            backing_fee_bps: 0,
            insurance_share_bps: 0,
            amount,
            expiry_slot: 10_000,
        }
    };
    let mut accounts = vec![
        AccountMeta::new(holder.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(wallet, false),
        AccountMeta::new(env.vault, false),
    ];
    if withdraw {
        accounts.push(AccountMeta::new_readonly(env.vault_authority, false));
    }
    accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
    env.svm.expire_blockhash();
    let cu = env
        .send(instruction, accounts, &[holder])
        .expect("incumbent's public backing transfer remains live");
    assert_cu_within("backing transfer", cu, CUSTODY_CU_LIMIT);
    cu
}

fn cold_handoff(
    env: &mut V16CuEnv,
    incoming: &Keypair,
    asset: u16,
    epoch: u64,
    protected: &[Pubkey],
    admitted: bool,
) -> u64 {
    assert_eq!(env.control_sequences(asset as usize).authority_epoch, epoch);
    let before = protected
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let before_market = env.market_state();
    let before_profiles = [0, 1].map(|index| {
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, index)
            .unwrap()
    });
    let before_sequences = [env.control_sequences(0), env.control_sequences(1)];
    assert_ne!(
        before_profiles[asset as usize].backing_bucket_authority,
        env.admin.pubkey().to_bytes(),
        "the signing cold admin is not the funded incumbent"
    );
    assert_ne!(
        before_profiles[asset as usize].backing_bucket_authority,
        incoming.pubkey().to_bytes(),
        "the requested handoff changes the holder"
    );
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(env.admin.pubkey(), true),
                    AccountMeta::new(incoming.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                data: ProgInstruction::UpdateAssetAuthority {
                    asset_index: asset,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: epoch,
                    kind: processor::ASSET_AUTH_BACKING_BUCKET,
                    new_pubkey: incoming.pubkey().to_bytes(),
                }
                .encode(),
            },
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, &env.admin, incoming],
        env.svm.latest_blockhash(),
    );
    let mut payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer_before.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let meta = if admitted {
        result.expect("cold admin may configure the fully depleted backing role")
    } else {
        let rejected = result.expect_err("any remaining role principal requires incumbent consent");
        assert_eq!(
            rejected.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::EngineLockActive as u32),
            )
        );
        rejected.meta
    };
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        payer_before
    );
    for (key, account) in protected.iter().zip(before) {
        if !admitted || *key != env.market {
            assert_eq!(env.svm.get_account(key), account, "handoff frame: {key}");
        }
    }
    assert_eq!(
        env.market_state(),
        before_market,
        "handoff cannot change economic state"
    );
    for index in 0..2 {
        let mut profile = before_profiles[index];
        let mut sequences = before_sequences[index];
        if admitted && index == asset as usize {
            profile.backing_bucket_authority = incoming.pubkey().to_bytes();
            sequences.authority_epoch += 1;
        }
        assert_eq!(
            state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                index
            )
            .unwrap(),
            profile
        );
        assert_eq!(env.control_sequences(index), sequences);
    }
    assert!(!meta
        .logs
        .iter()
        .any(|line| line.contains(&format!("Program {} invoke", spl_token::ID))));
    assert_cu_within(
        "cold backing handoff",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    meta.compute_units_consumed
}

#[test]
fn v16_program_backing_role_containment_tracks_both_domains_through_refunding() {
    let mut max_transfer_cu = 0;
    let mut max_rejection_cu = 0;
    let mut max_handoff_cu = 0;
    for asset in [0u16, 1] {
        for first in [0usize, 1] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            for index in 0..2 {
                assert_eq!(
                    env.market_state().1.assets[index].lifecycle,
                    AssetLifecycleV16::Active
                );
            }
            let admin = env.admin.insecure_clone();
            let incumbent = Keypair::new();
            let successor = Keypair::new();
            env.ensure_signer_account(incumbent.pubkey());
            env.ensure_signer_account(successor.pubkey());
            let original_total = PRINCIPAL.iter().sum::<u128>();
            let incumbent_wallet = mint_wallet(&mut env, incumbent.pubkey(), original_total);
            let successor_wallet = mint_wallet(&mut env, successor.pubkey(), REFILL);
            let admin_wallet = mint_wallet(&mut env, admin.pubkey(), PEER_BACKING + INSURANCE);
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&incumbent),
                asset,
                processor::ASSET_AUTH_BACKING_BUCKET,
                incumbent.pubkey().to_bytes(),
            )
            .expect("install independent incumbent while empty");
            let initial_epoch = env.control_sequences(asset as usize).authority_epoch;
            let peer = 1 - asset;
            let peer_domain = peer * 2;
            backing_transfer(
                &mut env,
                &admin,
                admin_wallet,
                peer_domain,
                PEER_BACKING,
                false,
            );
            let sequences = env.control_sequences(asset as usize);
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain: asset * 2,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: sequences.authority_epoch,
                    intent_id: next_control_sequence(sequences.insurance_top_up),
                    amount: INSURANCE,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(admin_wallet, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&admin],
            )
            .expect("unrelated insurance in the same asset remains funded");
            let peer_bucket = env.market_state().1.source_backing_buckets[peer_domain as usize];
            let protected = [
                env.market,
                env.vault,
                env.mint,
                incumbent_wallet,
                successor_wallet,
                admin_wallet,
                incumbent.pubkey(),
                successor.pubkey(),
                admin.pubkey(),
            ];
            let assert_books =
                |env: &V16CuEnv, remaining: [u128; 2], original_paid, refill_paid| {
                    let (_, group) = env.market_state();
                    assert_eq!(group.mode, MarketModeV16::Live);
                    assert_eq!(group.c_tot, 0);
                    assert_eq!(group.insurance, INSURANCE);
                    assert_eq!(
                        group.insurance_domain_budget[(asset * 2) as usize],
                        INSURANCE
                    );
                    assert_eq!(
                        group.source_backing_buckets[peer_domain as usize],
                        peer_bucket
                    );
                    for side in 0..2 {
                        let bucket = group.source_backing_buckets[asset as usize * 2 + side];
                        assert_eq!(
                            bucket.fresh_unliened_backing_num,
                            remaining[side] * BOUND_SCALE
                        );
                        assert_eq!(bucket.valid_liened_backing_num, 0);
                        assert_eq!(bucket.consumed_liened_backing_num, 0);
                        assert_eq!(bucket.impaired_liened_backing_num, 0);
                        assert_eq!(bucket.utilization_fee_earnings, 0);
                    }
                    let vault = remaining.iter().sum::<u128>() + PEER_BACKING + INSURANCE;
                    assert_eq!(group.vault, vault);
                    assert_eq!(env.token_amount(env.vault) as u128, vault);
                    assert_eq!(env.token_amount(incumbent_wallet) as u128, original_paid);
                    assert_eq!(env.token_amount(successor_wallet) as u128, refill_paid);
                    assert_eq!(env.token_amount(admin_wallet), 0);
                    let supply = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                        .unwrap()
                        .supply;
                    assert_eq!(
                        supply as u128,
                        original_total + REFILL + PEER_BACKING + INSURANCE
                    );
                    assert_eq!(supply as u128, vault + original_paid + refill_paid);
                };
            let mut remaining = [0; 2];
            for side in 0..2 {
                max_transfer_cu = max_transfer_cu.max(backing_transfer(
                    &mut env,
                    &incumbent,
                    incumbent_wallet,
                    asset * 2 + side as u16,
                    PRINCIPAL[side],
                    false,
                ));
                remaining[side] = PRINCIPAL[side];
                assert_books(
                    &env,
                    remaining,
                    original_total - remaining.iter().sum::<u128>(),
                    REFILL,
                );
            }
            for side in [first, 1 - first] {
                // No authority epoch changes during draining: rejection is the stock gate,
                // not stale consent, and the one-atom tail must still protect the entire role.
                for amount in [remaining[side] - 1, 1] {
                    max_rejection_cu = max_rejection_cu.max(cold_handoff(
                        &mut env,
                        &successor,
                        asset,
                        initial_epoch,
                        &protected,
                        false,
                    ));
                    max_transfer_cu = max_transfer_cu.max(backing_transfer(
                        &mut env,
                        &incumbent,
                        incumbent_wallet,
                        asset * 2 + side as u16,
                        amount,
                        true,
                    ));
                    remaining[side] -= amount;
                    assert_books(
                        &env,
                        remaining,
                        original_total - remaining.iter().sum::<u128>(),
                        REFILL,
                    );
                }
            }
            max_handoff_cu = max_handoff_cu.max(cold_handoff(
                &mut env,
                &successor,
                asset,
                initial_epoch,
                &protected,
                true,
            ));
            assert_books(&env, [0, 0], original_total, REFILL);

            let refill_side = 1 - first;
            max_transfer_cu = max_transfer_cu.max(backing_transfer(
                &mut env,
                &successor,
                successor_wallet,
                asset * 2 + refill_side as u16,
                REFILL,
                false,
            ));
            remaining[refill_side] = REFILL;
            assert_books(&env, remaining, original_total, 0);
            for amount in [REFILL - 1, 1] {
                max_rejection_cu = max_rejection_cu.max(cold_handoff(
                    &mut env,
                    &incumbent,
                    asset,
                    initial_epoch + 1,
                    &protected,
                    false,
                ));
                max_transfer_cu = max_transfer_cu.max(backing_transfer(
                    &mut env,
                    &successor,
                    successor_wallet,
                    asset * 2 + refill_side as u16,
                    amount,
                    true,
                ));
                remaining[refill_side] -= amount;
                assert_books(
                    &env,
                    remaining,
                    original_total,
                    REFILL - remaining[refill_side],
                );
            }
            max_handoff_cu = max_handoff_cu.max(cold_handoff(
                &mut env,
                &incumbent,
                asset,
                initial_epoch + 1,
                &protected,
                true,
            ));
            assert_books(&env, [0, 0], original_total, REFILL);
        }
    }
    eprintln!(
        "INV-005 backing refunding: worlds=4, rejected_handoffs=24, empty_handoffs=8, transfer_cu={max_transfer_cu}, rejection_cu={max_rejection_cu}, handoff_cu={max_handoff_cu}"
    );
}
