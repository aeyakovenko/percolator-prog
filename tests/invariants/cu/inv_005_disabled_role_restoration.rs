//! Row 416: burned asset-admin restoration across public retirement/reactivation.
//! Required oracle/funded roles cannot be zeroed; their claims exit separately.

use super::*;

const PRINCIPAL: u128 = 41;
const RESERVE: u128 = 59;
const PEER: u128 = 23;

fn lifecycle(env: &V16CuEnv, action: u8, a: Pubkey) -> Instruction {
    let activate = action == processor::ASSET_ACTION_ACTIVATE;
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetLifecycle {
            action,
            asset_index: 1,
            market_id: if activate {
                env.market_state().1.next_market_id
            } else {
                env.asset_market_id(1)
            },
            authority_epoch: env.control_sequences(0).authority_epoch,
            now_slot: 2,
            initial_price: if activate { 100 } else { 0 },
            max_init_fee: 0,
            insurance_authority: a.to_bytes(),
            insurance_operator: a.to_bytes(),
            backing_bucket_authority: a.to_bytes(),
            oracle_authority: a.to_bytes(),
        }
        .encode(),
    }
}

fn mark_request(
    env: &V16CuEnv,
    a: Pubkey,
    ewma: bool,
    configure: bool,
    epoch: u64,
    observation: u64,
) -> Instruction {
    let market_id = env.asset_market_id(1);
    let data = match (ewma, configure) {
        (false, false) => ProgInstruction::PushAuthMark {
            asset_index: 1,
            market_id,
            authority_epoch: epoch,
            observation_sequence: observation,
            now_slot: 2,
            mark_e6: 120,
        },
        (true, false) => ProgInstruction::PushEwmaMark {
            asset_index: 1,
            market_id,
            authority_epoch: epoch,
            observation_sequence: observation,
            now_slot: 2,
            mark_e6: 120,
        },
        (false, true) => ProgInstruction::ConfigureAuthMark {
            asset_index: 1,
            market_id,
            authority_epoch: epoch,
            observation_sequence: observation,
            now_slot: 2,
            initial_mark_e6: 100,
        },
        (true, true) => ProgInstruction::ConfigureEwmaMark {
            asset_index: 1,
            market_id,
            authority_epoch: epoch,
            observation_sequence: observation,
            now_slot: 2,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
    };
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(a, true),
            AccountMeta::new(env.market, false),
        ],
        data: data.encode(),
    }
}

fn requests(env: &V16CuEnv, a: Pubkey, b: Pubkey, wallet: Pubkey, ewma: bool) -> Vec<Instruction> {
    let seq = env.control_sequences(1);
    vec![
        mark_request(
            env,
            a,
            ewma,
            false,
            seq.authority_epoch,
            seq.oracle_observation + 1,
        ),
        mark_request(
            env,
            a,
            ewma,
            true,
            seq.authority_epoch,
            seq.oracle_observation + 1,
        ),
        handoff(
            env,
            1,
            processor::ASSET_AUTH_ADMIN,
            a,
            Some(b),
            seq.authority_epoch,
        ),
        handoff(
            env,
            1,
            processor::ASSET_AUTH_ORACLE,
            a,
            Some(b),
            seq.authority_epoch,
        ),
        payout(env, a, wallet, 2, true, PRINCIPAL),
        payout(env, a, wallet, 2, false, RESERVE),
    ]
}

fn prevalidate(env: &mut V16CuEnv, tx: &Transaction, tracked: &[Pubkey]) -> u64 {
    tx.verify().unwrap();
    assert_eq!(tx.message.recent_blockhash, env.svm.latest_blockhash());
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    let before = frame(env, &keys);
    let result = env.svm.simulate_transaction(tx.clone().into()).unwrap();
    assert_eq!(
        frame(env, &keys),
        before,
        "simulation is not a committed exit"
    );
    result.compute_units_consumed
}

fn fund(
    env: &mut V16CuEnv,
    holder: &Keypair,
    wallet: Pubkey,
    asset: usize,
    insurance: bool,
    amount: u128,
) {
    let seq = env.control_sequences(asset);
    let data = if insurance {
        ProgInstruction::TopUpInsuranceDomain {
            domain: asset as u16 * 2,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: seq.authority_epoch,
            intent_id: seq.insurance_top_up + 1,
            amount,
        }
    } else {
        ProgInstruction::TopUpBackingBucket {
            domain: asset as u16 * 2,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: seq.authority_epoch,
            intent_id: seq.backing_top_up + 1,
            amount,
            backing_fee_bps: 0,
            insurance_share_bps: 0,
            expiry_slot: 10_000,
        }
    };
    env.send(
        data,
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

fn token_image(account: &Account, amount: u64) -> Account {
    let mut result = account.clone();
    let mut token = TokenAccount::unpack(&result.data).unwrap();
    token.amount = amount;
    TokenAccount::pack(token, &mut result.data).unwrap();
    result
}

#[test]
fn v16_program_burned_admin_restoration_keeps_signed_oracle_and_funded_requests_stale() {
    let mut peak = 0;
    let mut rollbacks = 0;
    for detour in [false, true] {
        for ewma in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let a = Keypair::new();
            let b = Keypair::new();
            let peer = Keypair::new();
            for actor in [&a, &b, &peer] {
                env.svm.airdrop(&actor.pubkey(), 1_000_000_000).unwrap();
            }
            let wallets = [&a, &b, &peer].map(|actor| {
                create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
            });
            let empty_wallets = wallets.map(|key| env.svm.get_account(&key).unwrap());
            let empty_vault = env.svm.get_account(&env.vault).unwrap();
            for (wallet, amount) in [(wallets[0], PRINCIPAL + RESERVE), (wallets[2], PEER)] {
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
            for kind in [
                processor::ASSET_AUTH_ORACLE,
                processor::ASSET_AUTH_BACKING_BUCKET,
                processor::ASSET_AUTH_INSURANCE,
                processor::ASSET_AUTH_INSURANCE_OPERATOR,
                processor::ASSET_AUTH_ADMIN,
            ] {
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(&a),
                    1,
                    kind,
                    a.pubkey().to_bytes(),
                )
                .unwrap();
            }
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&peer),
                0,
                processor::ASSET_AUTH_BACKING_BUCKET,
                peer.pubkey().to_bytes(),
            )
            .unwrap();
            env.svm.warp_to_slot(2);
            let configuration = mark_request(
                &env,
                a.pubkey(),
                ewma,
                true,
                env.control_sequences(1).authority_epoch,
                1,
            );
            let market = env.market;
            let vault = env.vault;
            let tracked = [
                market,
                vault,
                env.mint,
                env.vault_authority,
                a.pubkey(),
                b.pubkey(),
                peer.pubkey(),
                admin.pubkey(),
                wallets[0],
                wallets[1],
                wallets[2],
            ];
            let tx = signed(&env, &[configuration], &[&a]);
            peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
            fund(&mut env, &a, wallets[0], 1, false, PRINCIPAL);
            fund(&mut env, &a, wallets[0], 1, true, RESERVE);
            fund(&mut env, &peer, wallets[2], 0, false, PEER);
            let mint_frame = env.svm.get_account(&env.mint);
            let mint = Mint::unpack(&mint_frame.as_ref().unwrap().data).unwrap();
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(mint.supply as u128, PRINCIPAL + RESERVE + PEER);
            let initial_profile = profile(&env, 1);
            let initial_seq = env.control_sequences(1);
            let peer_profile = profile(&env, 0);
            let peer_seq = env.control_sequences(0);
            let old_generation = env.asset_market_id(1);
            let book = |env: &V16CuEnv, backing: u128, insurance: u128, peer_backing: u128| {
                let group = env.market_state().1;
                let total = backing + insurance + peer_backing;
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert_eq!(
                    (
                        group.source_claim_bound_total_num,
                        group.backing_provider_earnings_total
                    ),
                    (0, 0)
                );
                assert_eq!(group.vault, total);
                assert_eq!(group.insurance, insurance);
                assert_eq!(group.insurance_domain_budget_remaining_total, insurance);
                for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
                    let expected = match domain {
                        0 => peer_backing,
                        2 => backing,
                        _ => 0,
                    };
                    assert_eq!(bucket.fresh_unliened_backing_num, expected * BOUND_SCALE);
                    assert_eq!(
                        (
                            bucket.valid_liened_backing_num,
                            bucket.consumed_liened_backing_num,
                            bucket.impaired_liened_backing_num,
                            bucket.utilization_fee_earnings
                        ),
                        (0, 0, 0, 0)
                    );
                    assert_eq!(
                        group.insurance_domain_budget[domain],
                        if domain == 2 { insurance } else { 0 }
                    );
                    assert_eq!(group.insurance_domain_spent[domain], 0);
                }
                for (index, amount) in [
                    PRINCIPAL + RESERVE - backing - insurance,
                    0,
                    PEER - peer_backing,
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(
                        env.svm.get_account(&wallets[index]),
                        Some(token_image(&empty_wallets[index], amount as u64))
                    );
                }
                assert_eq!(
                    env.svm.get_account(&vault),
                    Some(token_image(&empty_vault, total as u64))
                );
                assert_eq!(env.svm.get_account(&env.mint), mint_frame);
                assert_eq!(profile(env, 0), peer_profile);
                assert_eq!(env.control_sequences(0), peer_seq);
            };
            book(&env, PRINCIPAL, RESERVE, PEER);
            let old_requests = requests(&env, a.pubkey(), b.pubkey(), wallets[0], ewma);
            let prefix =
                |env: &V16CuEnv, amount| payout(env, peer.pubkey(), wallets[2], 0, true, amount);
            let retained: Vec<Vec<Transaction>> = [1, 2, 3]
                .into_iter()
                .map(|amount| {
                    old_requests
                        .iter()
                        .enumerate()
                        .map(|(index, request)| {
                            let mut signers = vec![&peer, &a];
                            if matches!(index, 2 | 3) {
                                signers.push(&b);
                            }
                            let tx =
                                signed(&env, &[prefix(&env, amount), request.clone()], &signers);
                            peak = peak.max(prevalidate(&mut env, &tx, &tracked));
                            tx
                        })
                        .collect()
                })
                .collect();
            let retained_bytes: Vec<Vec<Vec<u8>>> = retained
                .iter()
                .map(|phase| {
                    phase
                        .iter()
                        .map(|tx| bincode::serialize(tx).unwrap())
                        .collect()
                })
                .collect();
            let retained_peer = signed(&env, &[prefix(&env, PEER)], &[&peer]);
            peak = peak.max(prevalidate(&mut env, &retained_peer, &tracked));

            if detour {
                let tx = signed(&env, &[old_requests[2].clone()], &[&a, &b]);
                peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
            }
            let holder = if detour { &b } else { &a };
            for kind in [
                processor::ASSET_AUTH_ORACLE,
                processor::ASSET_AUTH_BACKING_BUCKET,
                processor::ASSET_AUTH_INSURANCE,
                processor::ASSET_AUTH_INSURANCE_OPERATOR,
            ] {
                let zero = handoff(
                    &env,
                    1,
                    kind,
                    holder.pubkey(),
                    None,
                    env.control_sequences(1).authority_epoch,
                );
                let tx = signed(&env, &[prefix(&env, 1), zero], &[&peer, holder]);
                peak = peak.max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[],
                    Some((3, PercolatorError::InvalidInstruction)),
                    1,
                ));
                rollbacks += 1;
            }
            let burn = handoff(
                &env,
                1,
                processor::ASSET_AUTH_ADMIN,
                holder.pubkey(),
                None,
                env.control_sequences(1).authority_epoch,
            );
            let tx = signed(&env, &[burn], &[holder]);
            peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
            let mut burned = initial_profile;
            burned.asset_admin = [0; 32];
            assert_eq!(profile(&env, 1), burned);
            let mut burned_seq = initial_seq;
            burned_seq.authority_epoch += 1 + u64::from(detour);
            assert_eq!(env.control_sequences(1), burned_seq);
            book(&env, PRINCIPAL, RESERVE, PEER);

            // The burn is permanent within this generation, even with a fresh epoch.
            let restore = handoff(
                &env,
                1,
                processor::ASSET_AUTH_ADMIN,
                a.pubkey(),
                Some(a.pubkey()),
                burned_seq.authority_epoch,
            );
            let tx = signed(&env, &[prefix(&env, 1), restore], &[&peer, &a]);
            peak = peak.max(land(
                &mut env,
                tx,
                &tracked,
                &[],
                Some((3, PercolatorError::Unauthorized)),
                1,
            ));
            rollbacks += 1;
            for (index, tx) in retained[2].iter().enumerate() {
                assert_eq!(bincode::serialize(tx).unwrap(), retained_bytes[2][index]);
                let error = if index == 2 {
                    PercolatorError::Unauthorized
                } else {
                    PercolatorError::EngineStale
                };
                peak = peak.max(land(
                    &mut env,
                    tx.clone(),
                    &tracked,
                    &[],
                    Some((3, error)),
                    1,
                ));
                rollbacks += 1;
            }
            for backing in [true, false] {
                // Keep the actual beneficiary's wallet valid so this reaches
                // the caller-authority check even on the insurance route.
                let unauthorized = payout(&env, b.pubkey(), wallets[0], 2, backing, 1);
                let tx = signed(&env, &[prefix(&env, 1), unauthorized], &[&peer, &b]);
                peak = peak.max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[],
                    Some((3, PercolatorError::Unauthorized)),
                    1,
                ));
                rollbacks += 1;
            }
            for (backing, amount) in [(true, PRINCIPAL), (false, RESERVE)] {
                let tx = signed(
                    &env,
                    &[payout(&env, a.pubkey(), wallets[0], 2, backing, amount)],
                    &[&a],
                );
                peak = peak.max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[market, vault, wallets[0]],
                    None,
                    1,
                ));
                book(&env, 0, if backing { RESERVE } else { 0 }, PEER);
            }
            let tx = signed(
                &env,
                &[lifecycle(&env, processor::ASSET_ACTION_RETIRE, a.pubkey())],
                &[&admin],
            );
            peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
            assert_eq!(
                env.market_state().1.assets[1].lifecycle,
                AssetLifecycleV16::Retired
            );
            assert_eq!(profile(&env, 1).asset_admin, [0; 32]);
            book(&env, 0, 0, PEER);
            env.svm.warp_to_slot(3);
            let tx = signed(
                &env,
                &[
                    prefix(&env, 1),
                    lifecycle(&env, processor::ASSET_ACTION_ACTIVATE, a.pubkey()),
                    old_requests[0].clone(),
                ],
                &[&peer, &admin, &a],
            );
            peak = peak.max(land(
                &mut env,
                tx,
                &tracked,
                &[],
                Some((4, PercolatorError::AssetGenerationMismatch)),
                1,
            ));
            rollbacks += 1;
            assert_eq!(env.asset_market_id(1), old_generation);
            assert_eq!(
                env.market_state().1.assets[1].lifecycle,
                AssetLifecycleV16::Retired
            );
            book(&env, 0, 0, PEER);
            let generation = env.market_state().1.next_market_id;
            let tx = signed(
                &env,
                &[lifecycle(
                    &env,
                    processor::ASSET_ACTION_ACTIVATE,
                    a.pubkey(),
                )],
                &[&admin],
            );
            peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
            assert_eq!(env.asset_market_id(1), generation);
            assert!(generation > old_generation);
            assert_eq!(
                env.control_sequences(1),
                state::AssetControlSequencesV16::default()
            );
            let restoration = handoff(
                &env,
                1,
                processor::ASSET_AUTH_ADMIN,
                admin.pubkey(),
                Some(a.pubkey()),
                0,
            );
            let tx = signed(&env, &[restoration], &[&admin, &a]);
            peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
            // Public self-handoffs recreate the old scalar epoch. Generation is
            // then the only changed binding in the retained request payloads.
            while env.control_sequences(1).authority_epoch < initial_seq.authority_epoch {
                let ix = handoff(
                    &env,
                    1,
                    processor::ASSET_AUTH_ORACLE,
                    a.pubkey(),
                    Some(a.pubkey()),
                    env.control_sequences(1).authority_epoch,
                );
                let tx = signed(&env, &[ix], &[&a]);
                peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
            }
            let configuration =
                mark_request(&env, a.pubkey(), ewma, true, initial_seq.authority_epoch, 1);
            let tx = signed(&env, &[configuration], &[&a]);
            peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
            fund(&mut env, &a, wallets[0], 1, false, PRINCIPAL);
            fund(&mut env, &a, wallets[0], 1, true, RESERVE);
            let mut restored_profile = initial_profile;
            restored_profile.mark_ewma_last_slot = 3;
            restored_profile.last_good_oracle_slot = 3;
            assert_eq!(profile(&env, 1), restored_profile);
            assert_eq!(env.control_sequences(1), initial_seq);
            book(&env, PRINCIPAL, RESERVE, PEER);
            let fresh_requests = requests(&env, a.pubkey(), b.pubkey(), wallets[0], ewma);
            for (index, request) in fresh_requests.iter().enumerate() {
                let mut signers = vec![&peer, &a];
                if matches!(index, 2 | 3) {
                    signers.push(&b);
                }
                let tx = signed(&env, &[prefix(&env, 1), request.clone()], &signers);
                peak = peak.max(prevalidate(&mut env, &tx, &tracked));
            }
            for phase in 0..2 {
                if phase == 1 {
                    let ix = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(market, false),
                        ],
                        data: ProgInstruction::ResolveMarket {
                            asset_generation_frontier: env.market_state().1.next_market_id,
                            authority_epoch: env.control_sequences(0).authority_epoch,
                        }
                        .encode(),
                    };
                    let tx = signed(&env, &[ix], &[&admin]);
                    peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
                    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
                }
                for (index, tx) in retained[phase].iter().enumerate() {
                    tx.verify().unwrap();
                    assert_eq!(tx.message.recent_blockhash, env.svm.latest_blockhash());
                    assert_eq!(
                        bincode::serialize(tx).unwrap(),
                        retained_bytes[phase][index]
                    );
                    peak = peak.max(land(
                        &mut env,
                        tx.clone(),
                        &tracked,
                        &[],
                        Some((3, PercolatorError::AssetGenerationMismatch)),
                        1,
                    ));
                    rollbacks += 1;
                    book(&env, PRINCIPAL, RESERVE, PEER);
                }
            }
            peak = peak.max(land(
                &mut env,
                retained_peer,
                &tracked,
                &[market, vault, wallets[2]],
                None,
                1,
            ));
            book(&env, PRINCIPAL, RESERVE, 0);
            for (backing, amount) in [(true, PRINCIPAL), (false, RESERVE)] {
                let tx = signed(
                    &env,
                    &[payout(&env, a.pubkey(), wallets[0], 2, backing, amount)],
                    &[&a],
                );
                peak = peak.max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[market, vault, wallets[0]],
                    None,
                    1,
                ));
                book(&env, 0, if backing { RESERVE } else { 0 }, 0);
            }
        }
    }
    assert_eq!(rollbacks, 104);
    assert_cu_within("burned admin restoration", peak, 300_000);
    eprintln!("burned admin restoration: worlds=4 rollbacks={rollbacks} peak_cu={peak}");
}
