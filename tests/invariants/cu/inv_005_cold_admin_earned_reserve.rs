//! INV-005/020/024/027/055: cold-admin succession cannot acquire an incumbent's
//! earned fees or the live source reserve after partial principal repayment.
//! Public System/SPL/ATA/wrapper construction; no terminal or funded-role transfer.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: [u128; 2] = [52_502, 2_000_000];
const PRINCIPAL: u128 = 100_000;
const RATE: u16 = 3_333;
const PROFIT: u128 = 1_000 * (105 - 100);
const LIEN: u128 = 1_050 * 105 / 2 - CAPITAL[0];
const PRINCIPAL_PAID: u128 = PRINCIPAL - LIEN;
const EARNINGS: u128 = (LIEN * RATE as u128).div_ceil(10_000);
const PREFIX: u128 = 137;
const SUPPLY: u128 = CAPITAL[0] + CAPITAL[1] + PRINCIPAL;

pub(super) fn wrap(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

pub(super) fn land(
    env: &mut V16CuEnv,
    instructions: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    changed: &[Pubkey],
    rejection: Option<(u8, PercolatorError, usize)>,
) -> u64 {
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
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before = keys
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let rejected = rejection.is_some();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, error, spl_successes)) = rejection {
        let failed = result.expect_err("current management cannot acquire an incumbent's value");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
            "{failed:?}"
        );
        for (program, count) in [
            (env.program_id, usize::from(index - 2)),
            (spl_token::ID, spl_successes),
        ] {
            assert_eq!(
                failed
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                count
            );
        }
        failed.meta
    } else {
        result.expect("current cold-admin management and incumbent payout remain live")
    };
    for (key, mut account) in keys.into_iter().zip(before) {
        if key == env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        if rejected || !changed.contains(&key) || key == env.payer.pubkey() {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "complete Account frame {key}"
            );
        }
    }
    assert_cu_within(
        "cold admin earned reserve",
        meta.compute_units_consumed,
        if instructions.len() == 1 {
            CUSTODY_CU_LIMIT
        } else {
            600_000
        },
    );
    meta.compute_units_consumed
}

#[test]
fn v16_program_cold_admin_rotation_preserves_earned_reserve_after_partial_principal_repayment() {
    assert_eq!((LIEN, EARNINGS), (2_623, 875));
    let mut peak_cu = [0; 3]; // fee trade/principal repayment, rejection, management/payout
    for asset in [0u16, 1] {
        for policy_first in [false, true] {
            let domain = asset * 2 + 1;
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    initial_margin_bps: 5_000,
                    maintenance_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let provider = Keypair::new();
            let cold = Keypair::new();
            let owners = [Keypair::new(), Keypair::new()];
            let actors = [&owners[0], &owners[1], &provider, &admin, &cold];
            for actor in [&owners[0], &owners[1], &provider, &cold] {
                env.svm.airdrop(&actor.pubkey(), 1_000_000_000).unwrap();
            }
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&provider),
                asset,
                processor::ASSET_AUTH_BACKING_BUCKET,
                provider.pubkey().to_bytes(),
            )
            .unwrap();
            env.svm.warp_to_slot(1);
            env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
            env.update_backing_fee_policy_with_cu(domain, RATE, 0);
            let tokens = actors.map(|owner| {
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
            });
            for (token, amount) in tokens.into_iter().zip([CAPITAL[0], CAPITAL[1], PRINCIPAL]) {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &token,
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
            let portfolios = owners.each_ref().map(|owner| {
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
            for i in 0..2 {
                env.send(
                    env.deposit_ix(portfolios[i], CAPITAL[i]),
                    vec![
                        AccountMeta::new(owners[i].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[i], false),
                        AccountMeta::new(tokens[i], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[i]],
                )
                .unwrap();
            }
            let ledger_key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &ledger_key,
                state::backing_domain_ledger_account_len(),
                env.program_id,
            );
            let ledger = ledger_key.pubkey();
            let sequences = env.control_sequences(asset as usize);
            env.send(
                ProgInstruction::TopUpBackingBucket {
                    domain,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: sequences.authority_epoch,
                    intent_id: next_control_sequence(sequences.backing_top_up),
                    backing_fee_bps: RATE,
                    insurance_share_bps: 0,
                    amount: PRINCIPAL,
                    expiry_slot: 100,
                },
                vec![
                    AccountMeta::new(provider.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[2], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(ledger, false),
                ],
                &[&provider],
            )
            .unwrap();
            env.trade_asset_with_cu(
                asset,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                1_000 * POS_SCALE as i128,
                100,
                0,
            );
            env.svm.warp_to_slot(2);
            env.push_auth_mark_for_asset_as_admin(asset, 2, 105);
            for i in [1, 0] {
                env.crank(
                    portfolios[i],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations(asset),
                    },
                );
            }
            peak_cu[0] = peak_cu[0].max(
                env.try_trade_asset_with_backing_fee_cap_with_cu(
                    asset,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    50 * POS_SCALE as i128,
                    105,
                    0,
                    RATE,
                )
                .unwrap(),
            );
            let earned = env.market_state().1;
            assert_eq!(
                earned.source_backing_buckets[domain as usize].utilization_fee_earnings,
                EARNINGS
            );
            assert_eq!(
                earned.source_backing_buckets[domain as usize].valid_liened_backing_num,
                LIEN * BOUND_SCALE
            );
            let mut tracked = vec![env.market, env.vault, env.mint, env.vault_authority, ledger];
            tracked.extend(tokens);
            tracked.extend(portfolios);
            tracked.extend(actors.map(Signer::pubkey));
            let principal = wrap(
                &env,
                ProgInstruction::WithdrawBackingBucket {
                    domain,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: env.control_sequences(asset as usize).authority_epoch,
                    amount: PRINCIPAL_PAID,
                },
                vec![
                    AccountMeta::new(provider.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[2], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(ledger, false),
                ],
            );
            let payout_changed = [env.market, env.vault, tokens[2], ledger];
            peak_cu[0] = peak_cu[0].max(land(
                &mut env,
                &[principal],
                &[&provider],
                &tracked,
                &payout_changed,
                None,
            ));

            let initial = env.market_state();
            let profiles = [0, 1].map(|i| {
                state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, i)
                    .unwrap()
            });
            let sequences = [env.control_sequences(0), env.control_sequences(1)];
            let epoch = sequences[asset as usize].authority_epoch;
            let policy_sequence =
                next_control_sequence(sequences[asset as usize].backing_fee.max(epoch + 1));
            let ledger_initial =
                state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data)
                    .unwrap();
            assert_eq!(ledger_initial.market_group, env.market.to_bytes());
            assert_eq!(ledger_initial.domain, domain);
            assert_eq!(ledger_initial.total_principal_atoms, LIEN);
            assert_eq!(ledger_initial.total_deposited_atoms, PRINCIPAL);
            assert_eq!(
                ledger_initial.total_principal_withdrawn_atoms,
                PRINCIPAL_PAID
            );
            assert_eq!(ledger_initial.total_earnings_atoms, EARNINGS);
            assert_eq!(ledger_initial.total_earnings_withdrawn_atoms, 0);
            assert_eq!(ledger_initial.last_observed_bucket_earnings_atoms, EARNINGS);
            let portfolio_frames = portfolios.map(|key| env.svm.get_account(&key));
            let mint_frame = env.svm.get_account(&env.mint);

            let rotate = |from, to, kind, epoch| {
                wrap(
                    &env,
                    ProgInstruction::UpdateAssetAuthority {
                        asset_index: asset,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: epoch,
                        kind,
                        new_pubkey: Pubkey::to_bytes(to),
                    },
                    vec![
                        AccountMeta::new(from, true),
                        AccountMeta::new_readonly(to, true),
                        AccountMeta::new(env.market, false),
                    ],
                )
            };
            let admin_rotation = rotate(
                admin.pubkey(),
                cold.pubkey(),
                processor::ASSET_AUTH_ADMIN,
                epoch,
            );
            let seize = rotate(
                cold.pubkey(),
                cold.pubkey(),
                processor::ASSET_AUTH_BACKING_BUCKET,
                epoch + 1,
            );
            let policy = |epoch, sequence, share| {
                wrap(
                    &env,
                    ProgInstruction::UpdateBackingFeePolicy {
                        domain,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: epoch,
                        policy_sequence: sequence,
                        fee_bps: RATE,
                        insurance_share_bps: share,
                    },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                    ],
                )
            };
            let reaffirm = policy(epoch + u64::from(!policy_first), policy_sequence, 0);
            let redirect_policy = policy(epoch + 1, policy_sequence + 1, 10_000);
            let earnings = |destination, amount| {
                wrap(
                    &env,
                    ProgInstruction::WithdrawBackingBucketEarnings {
                        domain,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: epoch + 1,
                        amount,
                    },
                    vec![
                        AccountMeta::new(provider.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(ledger, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                )
            };
            let prefix = if policy_first {
                vec![reaffirm, admin_rotation]
            } else {
                vec![admin_rotation, reaffirm]
            };
            let partial = earnings(tokens[2], PREFIX);
            let tail = earnings(tokens[2], EARNINGS - PREFIX);
            let wrong_destination = earnings(tokens[4], PREFIX);

            let check = |env: &V16CuEnv, managed: bool, paid: u128| {
                let (cfg, group) = env.market_state();
                assert_eq!(cfg, initial.0);
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(
                    group.assets[asset as usize].lifecycle,
                    AssetLifecycleV16::Active
                );
                let mut expected = initial.1.clone();
                expected.vault -= paid;
                expected.backing_provider_earnings_total -= paid;
                expected.source_backing_buckets[domain as usize].utilization_fee_earnings -= paid;
                assert_eq!(
                    group, expected,
                    "management and earnings payout preserve all other economics"
                );
                assert_eq!(
                    group.c_tot,
                    CAPITAL.iter().sum::<u128>() - PROFIT - EARNINGS
                );
                assert_eq!(
                    env.portfolio_state(portfolios[0]).capital.get(),
                    CAPITAL[0] - EARNINGS
                );
                assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), PROFIT as i128);
                assert_eq!(
                    env.portfolio_state(portfolios[1]).capital.get(),
                    CAPITAL[1] - PROFIT
                );
                assert_eq!(env.portfolio_state(portfolios[1]).pnl.get(), 0);
                assert_eq!(
                    portfolios.map(|key| env.svm.get_account(&key)),
                    portfolio_frames
                );
                let bucket = group.source_backing_buckets[domain as usize];
                assert_eq!(bucket.fresh_unliened_backing_num, PROFIT * BOUND_SCALE);
                assert_eq!(bucket.valid_liened_backing_num, LIEN * BOUND_SCALE);
                assert_eq!(bucket.consumed_liened_backing_num, 0);
                assert_eq!(bucket.impaired_liened_backing_num, 0);
                assert_eq!(bucket.utilization_fee_earnings, EARNINGS - paid);
                assert_eq!(
                    group.source_credit[domain as usize].fresh_reserved_backing_num,
                    (PROFIT + LIEN) * BOUND_SCALE
                );
                assert_eq!(group.insurance, 0);
                assert!(group
                    .insurance_domain_budget
                    .iter()
                    .all(|value| *value == 0));
                assert_eq!(group.vault, group.c_tot + PROFIT + LIEN + EARNINGS - paid);
                assert_eq!(env.token_amount(env.vault) as u128, group.vault);
                assert_eq!(
                    tokens.map(|key| env.token_amount(key) as u128),
                    [0, 0, PRINCIPAL_PAID + paid, 0, 0]
                );
                assert_eq!(group.vault + PRINCIPAL_PAID + paid, SUPPLY);
                assert_eq!(env.svm.get_account(&env.mint), mint_frame);
                let mint = Mint::unpack(&mint_frame.as_ref().unwrap().data).unwrap();
                assert_eq!(mint.supply as u128, SUPPLY);
                assert_eq!(mint.mint_authority, COption::None);
                let mut expected_ledger = ledger_initial;
                expected_ledger.last_observed_bucket_earnings_atoms -= paid;
                expected_ledger.total_earnings_withdrawn_atoms += paid;
                assert_eq!(
                    state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data)
                        .unwrap(),
                    expected_ledger
                );
                assert_eq!(expected_ledger.authority, provider.pubkey().to_bytes());
                for i in 0..2 {
                    let mut profile = profiles[i];
                    let mut sequence = sequences[i];
                    if managed && i == asset as usize {
                        profile.asset_admin = cold.pubkey().to_bytes();
                        sequence.authority_epoch += 1;
                        sequence.backing_fee = policy_sequence;
                    }
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            i
                        )
                        .unwrap(),
                        profile
                    );
                    assert_eq!(env.control_sequences(i), sequence);
                }
            };
            check(&env, false, 0);
            // Each suffix runs after both valid management instructions. A second
            // probe also executes real SPL payment before rejecting role substitution.
            for (suffix, signers, error) in [
                (
                    seize.clone(),
                    vec![&admin, &cold],
                    PercolatorError::EngineLockActive,
                ),
                (
                    redirect_policy.clone(),
                    vec![&admin, &cold],
                    PercolatorError::EngineLockActive,
                ),
                (
                    wrong_destination,
                    vec![&admin, &cold, &provider],
                    PercolatorError::InvalidTokenAccount,
                ),
            ] {
                let mut bundle = prefix.clone();
                bundle.push(suffix);
                peak_cu[1] = peak_cu[1].max(land(
                    &mut env,
                    &bundle,
                    &signers,
                    &tracked,
                    &[],
                    Some((4, error, 0)),
                ));
                check(&env, false, 0);
            }
            let mut bundle = prefix.clone();
            bundle.extend([partial.clone(), seize.clone()]);
            peak_cu[1] = peak_cu[1].max(land(
                &mut env,
                &bundle,
                &[&admin, &cold, &provider],
                &tracked,
                &[],
                Some((5, PercolatorError::EngineLockActive, 1)),
            ));
            check(&env, false, 0);
            bundle.pop();
            peak_cu[2] = peak_cu[2].max(land(
                &mut env,
                &bundle,
                &[&admin, &cold, &provider],
                &tracked,
                &payout_changed,
                None,
            ));
            check(&env, true, PREFIX);
            for (ix, signer) in [(seize, &cold), (redirect_policy, &admin)] {
                peak_cu[1] = peak_cu[1].max(land(
                    &mut env,
                    &[ix],
                    &[signer],
                    &tracked,
                    &[],
                    Some((2, PercolatorError::EngineLockActive, 0)),
                ));
                check(&env, true, PREFIX);
            }
            peak_cu[2] = peak_cu[2].max(land(
                &mut env,
                &[tail],
                &[&provider],
                &tracked,
                &payout_changed,
                None,
            ));
            check(&env, true, EARNINGS);
        }
    }
    eprintln!("INV-005 cold-admin earned reserve: worlds=4, rollbacks=24, SPL-prefix rollbacks=4, peak CU [fee trade/principal repayment, rejection, management/payout]={peak_cu:?}");
}
