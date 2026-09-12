//! INV-005: funded oracle succession after the cold admin renounces its own role.
//! Oracle consent transfers observation power, not the incumbent's backing principal.
//! This does not certify nonconsensual cold-admin replacement of a funded oracle.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[path = "inv_005_shutdown_reserve_aba.rs"]
mod shutdown_reserve_aba;

const PRINCIPAL: [u128; 2] = [17, 29];
const PEER_BACKING: u128 = 31;
const CAPITAL: u128 = 23;

fn profile(env: &V16CuEnv, asset: usize) -> state::AssetOracleProfileV16 {
    state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, asset)
        .unwrap()
}

fn withdrawal(
    env: &V16CuEnv,
    signer: Pubkey,
    wallet: Pubkey,
    domain: u16,
    amount: u128,
    epoch: u64,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(signer, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(wallet, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawBackingBucket {
            domain,
            market_id: env.asset_market_id(domain / 2),
            authority_epoch: epoch,
            amount,
        }
        .encode(),
    }
}

fn land(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    signers: &[&Keypair],
    protected: &[Pubkey],
    changed: &[Pubkey],
    error: Option<(u8, PercolatorError)>,
) -> litesvm::types::TransactionMetadata {
    env.svm.expire_blockhash();
    let mut ixs = vec![heap_ix(), cu_ix()];
    ixs.extend_from_slice(instructions);
    let mut signatures = vec![&env.payer];
    signatures.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &signatures,
        env.svm.latest_blockhash(),
    );
    tx.verify()
        .expect("all management and exit signatures verify");
    assert!(bincode::serialize(&tx).unwrap().len() <= 1232);
    let mut expected_signers = signatures.iter().map(|s| s.pubkey()).collect::<Vec<_>>();
    expected_signers.sort();
    let mut actual_signers =
        tx.message.account_keys[..usize::from(tx.message.header.num_required_signatures)].to_vec();
    actual_signers.sort();
    assert_eq!(actual_signers, expected_signers);
    let keys = protected
        .iter()
        .chain(&tx.message.account_keys)
        .copied()
        .filter(|key| *key != env.payer.pubkey())
        .collect::<std::collections::BTreeSet<_>>();
    let before = keys
        .iter()
        .map(|key| (*key, env.svm.get_account(key)))
        .collect::<Vec<_>>();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let rejected = error.is_some();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, error)) = error {
        let failure = result.expect_err("role or epoch boundary rejects exactly");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
        );
        failure.meta
    } else {
        result.expect("consented succession and independent exits remain live")
    };
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    for (key, mut account) in before {
        let after = env.svm.get_account(&key);
        if !rejected && changed.contains(&key) {
            if let (Some(before), Some(after)) = (&mut account, &after) {
                if before.owner == spl_token::ID && before.data.len() == TokenAccount::LEN {
                    let mut token = TokenAccount::unpack(&before.data).unwrap();
                    token.amount = TokenAccount::unpack(&after.data).unwrap().amount;
                    TokenAccount::pack(token, &mut before.data).unwrap();
                    assert_eq!(before, after, "only SPL amount may change: {key}");
                }
            }
        } else {
            assert_eq!(after, account, "complete Account frame: {key}");
        }
    }
    assert_cu_within(
        "funded oracle succession",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    meta
}

#[test]
fn v16_program_funded_oracle_succession_after_admin_burn_preserves_backing_exit() {
    let mut peak_cu = 0;
    for asset in [0u16, 1] {
        for first in [0usize, 1] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let incumbent = Keypair::new();
            let successor = Keypair::new();
            let user = Keypair::new();
            let actors = [&incumbent, &successor, &admin, &user];
            for actor in actors {
                env.ensure_signer_account(actor.pubkey());
            }
            for (index, kind, holder) in [
                (asset, processor::ASSET_AUTH_ORACLE, &incumbent),
                (asset, processor::ASSET_AUTH_BACKING_BUCKET, &incumbent),
                (1 - asset, processor::ASSET_AUTH_BACKING_BUCKET, &user),
            ] {
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(holder),
                    index,
                    kind,
                    holder.pubkey().to_bytes(),
                )
                .unwrap();
            }
            for index in [0u16, 1] {
                env.configure_auth_mark_for_asset_with_authority(
                    index,
                    if index == asset { &incumbent } else { &admin },
                    0,
                    100,
                );
            }
            let wallets = actors.map(|actor| {
                create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
            });
            for (wallet, amount) in [
                (wallets[0], PRINCIPAL.iter().sum::<u128>()),
                (wallets[3], CAPITAL + PEER_BACKING),
            ] {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &wallet,
                        &admin.pubkey(),
                        &[],
                        amount as u64,
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
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
            for (holder, wallet, domain, amount) in [
                (&incumbent, wallets[0], asset * 2, PRINCIPAL[0]),
                (&incumbent, wallets[0], asset * 2 + 1, PRINCIPAL[1]),
                (&user, wallets[3], (1 - asset) * 2, PEER_BACKING),
            ] {
                let sequences = env.control_sequences((domain / 2) as usize);
                env.send(
                    ProgInstruction::TopUpBackingBucket {
                        domain,
                        market_id: env.asset_market_id(domain / 2),
                        authority_epoch: sequences.authority_epoch,
                        intent_id: next_control_sequence(sequences.backing_top_up),
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount,
                        expiry_slot: 10_000,
                    },
                    vec![
                        AccountMeta::new(holder.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(wallet, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[holder],
                )
                .unwrap();
            }
            let portfolio_key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                env.portfolio_account_len,
                env.program_id,
            );
            let portfolio = portfolio_key.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(user.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&user],
            )
            .unwrap();
            env.portfolios.push(portfolio);
            env.send(
                env.deposit_ix(portfolio, CAPITAL),
                vec![
                    AccountMeta::new(user.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(wallets[3], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&user],
            )
            .unwrap();

            let mut protected = vec![env.market, env.mint, env.vault, portfolio];
            protected.extend(wallets);
            protected.extend(actors.map(Signer::pubkey));
            let peer = (1 - asset) as usize;
            let peer_profile = profile(&env, peer);
            let peer_sequences = env.control_sequences(peer);
            let peer_bucket = env.market_state().1.source_backing_buckets[peer * 2];
            let assert_stock = |env: &V16CuEnv, remaining: [u128; 2], user_paid: u128| {
                let (cfg, group) = env.market_state();
                let paid = PRINCIPAL.iter().sum::<u128>() - remaining.iter().sum::<u128>();
                assert_eq!(cfg.marketauth, admin.pubkey().to_bytes());
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(
                    group.assets[asset as usize].lifecycle,
                    AssetLifecycleV16::Active
                );
                assert_eq!(group.c_tot, CAPITAL - user_paid);
                assert_eq!(group.pnl_pos_tot, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.backing_provider_earnings_total, 0);
                assert_eq!(group.insurance, 0);
                assert!(group
                    .insurance_domain_budget
                    .iter()
                    .all(|amount| *amount == 0));
                assert_eq!(group.source_backing_buckets[peer * 2], peer_bucket);
                assert_eq!(profile(env, peer), peer_profile);
                assert_eq!(env.control_sequences(peer), peer_sequences);
                for state in &group.assets {
                    assert_eq!(state.raw_oracle_target_price, 100);
                    assert_eq!(state.effective_price, 100);
                    assert_eq!((state.oi_eff_long_q, state.oi_eff_short_q), (0, 0));
                    assert_eq!(
                        (state.stored_pos_count_long, state.stored_pos_count_short),
                        (0, 0)
                    );
                }
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
                let owner = env.portfolio_state(portfolio);
                assert_eq!(owner.owner, user.pubkey().to_bytes());
                assert_eq!(owner.capital.get(), CAPITAL - user_paid);
                assert_eq!(owner.pnl.get(), 0);
                let vault = remaining.iter().sum::<u128>() + PEER_BACKING + CAPITAL - user_paid;
                assert_eq!(group.vault, vault);
                assert_eq!(env.token_amount(env.vault) as u128, vault);
                assert_eq!(
                    wallets.map(|key| env.token_amount(key) as u128),
                    [paid, 0, 0, user_paid]
                );
                let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                assert_eq!(mint.mint_authority, COption::None);
                assert_eq!(
                    mint.supply as u128,
                    PRINCIPAL.iter().sum::<u128>() + PEER_BACKING + CAPITAL
                );
                assert_eq!(mint.supply as u128, vault + paid + user_paid);
            };
            assert_stock(&env, PRINCIPAL, 0);

            let mut expected_profile = profile(&env, asset as usize);
            let mut expected_sequences = env.control_sequences(asset as usize);
            let before_burn = env.market_state();
            let burn = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new_readonly(env.payer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                ],
                data: ProgInstruction::UpdateAssetAuthority {
                    asset_index: asset,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: expected_sequences.authority_epoch,
                    kind: processor::ASSET_AUTH_ADMIN,
                    new_pubkey: [0; 32],
                }
                .encode(),
            };
            let market = env.market;
            peak_cu = peak_cu.max(
                land(&mut env, &[burn], &[&admin], &protected, &[market], None)
                    .compute_units_consumed,
            );
            expected_profile.asset_admin = [0; 32];
            expected_sequences.authority_epoch += 1;
            assert_eq!(profile(&env, asset as usize), expected_profile);
            assert_eq!(env.control_sequences(asset as usize), expected_sequences);
            assert_eq!(env.market_state(), before_burn);
            assert_stock(&env, PRINCIPAL, 0);

            let epoch = expected_sequences.authority_epoch;
            let handoff = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(incumbent.pubkey(), true),
                    AccountMeta::new_readonly(successor.pubkey(), true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::UpdateAssetAuthority {
                    asset_index: asset,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: epoch,
                    kind: processor::ASSET_AUTH_ORACLE,
                    new_pubkey: successor.pubkey().to_bytes(),
                }
                .encode(),
            };
            let first_domain = asset * 2 + first as u16;
            let other_domain = asset * 2 + (1 - first) as u16;
            let prefix = withdrawal(&env, incumbent.pubkey(), wallets[0], first_domain, 7, epoch);
            let stale = withdrawal(&env, incumbent.pubkey(), wallets[0], other_domain, 1, epoch);
            // A real SPL payout and the oracle handoff both execute before the stale suffix.
            let meta = land(
                &mut env,
                &[prefix.clone(), handoff.clone(), stale],
                &[&incumbent, &successor],
                &protected,
                &[],
                Some((4, PercolatorError::EngineStale)),
            );
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", env.program_id))
                    .count(),
                2
            );
            assert!(meta
                .logs
                .contains(&format!("Program {} success", spl_token::ID)));
            assert_eq!(profile(&env, asset as usize), expected_profile);
            assert_eq!(env.control_sequences(asset as usize), expected_sequences);
            assert_stock(&env, PRINCIPAL, 0);

            let fresh = withdrawal(
                &env,
                incumbent.pubkey(),
                wallets[0],
                other_domain,
                1,
                epoch + 1,
            );
            let custody_changes = [market, env.vault, wallets[0]];
            let meta = land(
                &mut env,
                &[prefix, handoff, fresh],
                &[&incumbent, &successor],
                &protected,
                &custody_changes,
                None,
            );
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", env.program_id))
                    .count(),
                3
            );
            expected_profile.oracle_authority = successor.pubkey().to_bytes();
            expected_sequences.authority_epoch += 1;
            assert_eq!(profile(&env, asset as usize), expected_profile);
            assert_eq!(env.control_sequences(asset as usize), expected_sequences);
            let mut remaining = PRINCIPAL;
            remaining[first] -= 7;
            remaining[1 - first] -= 1;
            assert_stock(&env, remaining, 0);

            env.svm.warp_to_slot(2);
            let observation = next_control_sequence(expected_sequences.oracle_observation);
            let push = |env: &V16CuEnv, signer| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(signer, true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::PushAuthMark {
                    asset_index: asset,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: epoch + 1,
                    observation_sequence: observation,
                    now_slot: u64::MAX,
                    mark_e6: 100,
                }
                .encode(),
            };
            for (ix, signer) in [
                (push(&env, incumbent.pubkey()), &incumbent),
                (
                    withdrawal(
                        &env,
                        successor.pubkey(),
                        wallets[1],
                        first_domain,
                        1,
                        epoch + 1,
                    ),
                    &successor,
                ),
            ] {
                peak_cu = peak_cu.max(
                    land(
                        &mut env,
                        &[ix],
                        &[signer],
                        &protected,
                        &[],
                        Some((2, PercolatorError::Unauthorized)),
                    )
                    .compute_units_consumed,
                );
                assert_stock(&env, remaining, 0);
            }
            let before_observation = env.market_state().1;
            let ix = push(&env, successor.pubkey());
            peak_cu = peak_cu.max(
                land(&mut env, &[ix], &[&successor], &protected, &[market], None)
                    .compute_units_consumed,
            );
            expected_sequences.oracle_observation = observation;
            assert_eq!(env.control_sequences(asset as usize), expected_sequences);
            assert_eq!(
                env.market_state().1,
                before_observation,
                "a new observation at the same price changes no engine state"
            );
            expected_profile.last_good_oracle_slot = 2;
            assert_eq!(profile(&env, asset as usize), expected_profile);
            assert_stock(&env, remaining, 0);

            // The new oracle is absent from every remaining payout's compiled signer set.
            for side in [first, 1 - first] {
                let ix = withdrawal(
                    &env,
                    incumbent.pubkey(),
                    wallets[0],
                    asset * 2 + side as u16,
                    remaining[side],
                    epoch + 1,
                );
                peak_cu = peak_cu.max(
                    land(
                        &mut env,
                        &[ix],
                        &[&incumbent],
                        &protected,
                        &custody_changes,
                        None,
                    )
                    .compute_units_consumed,
                );
                remaining[side] = 0;
                assert_eq!(profile(&env, asset as usize), expected_profile);
                assert_eq!(env.control_sequences(asset as usize), expected_sequences);
                assert_stock(&env, remaining, 0);
            }
            let ix = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(user.pubkey(), true),
                    AccountMeta::new(market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(wallets[3], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.withdraw_ix(portfolio, CAPITAL).encode(),
            };
            let changed = [market, portfolio, wallets[3], env.vault];
            peak_cu = peak_cu.max(
                land(&mut env, &[ix], &[&user], &protected, &changed, None).compute_units_consumed,
            );
            assert_stock(&env, [0, 0], CAPITAL);
        }
    }
    eprintln!("INV-005 funded oracle succession: 4 worlds, 4 admin burns, 4 SPL/handoff rollbacks, 4 oracle successions, 8 role rejections, 12 final payouts, peak {peak_cu} CU");
}
