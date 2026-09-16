//! Row 416 / INV-005: retained signed terminal envelopes across oracle/insurer
//! and market-authority ABA, with funded coholders and exact closure rollback.

use super::*;
use percolator_prog::constants;
use solana_sdk::system_program;

const PRINCIPAL: u128 = 41;
const RESERVE: u128 = 59;
const PEER: u128 = 23;
const DUST: u64 = 7;

fn exit(
    env: &V16CuEnv,
    holder: Pubkey,
    wallet: Pubkey,
    ledger: Option<Pubkey>,
    asset: usize,
    epoch: u64,
    insurance: bool,
    amount: u128,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(holder, true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(wallet, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    if let Some(ledger) = ledger {
        accounts.push(AccountMeta::new(ledger, false));
    }
    Instruction {
        program_id: env.program_id,
        accounts,
        data: if insurance {
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index: asset as u16,
                market_id: env.asset_market_id(asset as u16),
                authority_epoch: epoch,
                amount,
            }
        } else {
            ProgInstruction::WithdrawBackingBucket {
                domain: asset as u16 * 2,
                market_id: env.asset_market_id(asset as u16),
                authority_epoch: epoch,
                amount,
            }
        }
        .encode(),
    }
}

fn close(env: &V16CuEnv, destination: Pubkey, epoch: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: epoch,
        }
        .encode(),
    }
}

fn token_image(account: &Account, amount: u64) -> Account {
    let mut result = account.clone();
    let mut token = TokenAccount::unpack(&result.data).unwrap();
    token.amount = amount;
    TokenAccount::pack(token, &mut result.data).unwrap();
    result
}

fn prevalidate(env: &mut V16CuEnv, tx: &Transaction, tracked: &[Pubkey]) -> u64 {
    tx.verify().unwrap();
    assert_eq!(tx.message.recent_blockhash, env.svm.latest_blockhash());
    let before = frame(env, tracked);
    let result = env
        .svm
        .simulate_transaction(tx.clone().into())
        .expect("the exact retained signed envelope was admissible before succession");
    assert_eq!(frame(env, tracked), before);
    result.compute_units_consumed
}

#[test]
fn v16_program_retained_terminal_envelopes_bind_funded_oracle_roles_and_slab_closure() {
    let mut peak = 0;
    let mut rollbacks = 0;
    for asset in [0usize, 1] {
        for kind in [
            processor::ASSET_AUTH_ORACLE,
            processor::ASSET_AUTH_INSURANCE,
        ] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            // A coholds oracle, backing and both insurance roles. Cold and peer
            // remain separate, including when asset 0's market authority rotates.
            let actors: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
            let [a, b, c, cold, peer] = actors.each_ref();
            for actor in &actors {
                env.svm.airdrop(&actor.pubkey(), 1_000_000_000).unwrap();
            }
            for index in [0u16, 1] {
                env.configure_auth_mark_for_asset_as_admin(index, 0, 100);
                for (role, holder) in [
                    (processor::ASSET_AUTH_ORACLE, a),
                    (
                        processor::ASSET_AUTH_BACKING_BUCKET,
                        if index as usize == asset { a } else { peer },
                    ),
                    (processor::ASSET_AUTH_INSURANCE, a),
                    (processor::ASSET_AUTH_INSURANCE_OPERATOR, a),
                    (processor::ASSET_AUTH_ADMIN, cold),
                ] {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(holder),
                        index,
                        role,
                        holder.pubkey().to_bytes(),
                    )
                    .unwrap();
                }
            }
            let wallets = [
                a.pubkey(),
                b.pubkey(),
                c.pubkey(),
                peer.pubkey(),
                admin.pubkey(),
            ]
            .map(|key| create_ata_for_test(&mut env.svm, &env.payer, key, env.mint));
            let empty_wallets = wallets.map(|key| env.svm.get_account(&key).unwrap());
            for (destination, amount) in [
                (wallets[0], (PRINCIPAL + RESERVE) as u64),
                (wallets[3], PEER as u64),
                (env.vault, DUST),
            ] {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &destination,
                        &admin.pubkey(),
                        &[],
                        amount,
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
            let ledgers = [Keypair::new(), Keypair::new()];
            for (ledger, len) in ledgers.iter().zip([
                state::backing_domain_ledger_account_len(),
                state::insurance_ledger_account_len(),
            ]) {
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    ledger,
                    len,
                    env.program_id,
                );
            }
            let ledgers = ledgers.each_ref().map(Signer::pubkey);
            for (index, holder, wallet, amount, insurance, ledger) in [
                (asset, a, wallets[0], PRINCIPAL, false, Some(ledgers[0])),
                (asset, a, wallets[0], RESERVE, true, Some(ledgers[1])),
                (1 - asset, peer, wallets[3], PEER, false, None),
            ] {
                let seq = env.control_sequences(index);
                let data = if insurance {
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: index as u16 * 2,
                        market_id: env.asset_market_id(index as u16),
                        authority_epoch: seq.authority_epoch,
                        intent_id: next_control_sequence(seq.insurance_top_up),
                        amount,
                    }
                } else {
                    ProgInstruction::TopUpBackingBucket {
                        domain: index as u16 * 2,
                        market_id: env.asset_market_id(index as u16),
                        authority_epoch: seq.authority_epoch,
                        intent_id: next_control_sequence(seq.backing_top_up),
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
                    AccountMeta::new_readonly(spl_token::ID, false),
                ];
                if let Some(ledger) = ledger {
                    accounts.push(AccountMeta::new(ledger, false));
                }
                env.send(data, accounts, &[holder]).unwrap();
            }
            env.send(
                ProgInstruction::ResolveMarket {
                    asset_generation_frontier: env.market_state().1.next_market_id,
                    authority_epoch: env.control_sequences(0).authority_epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                &[&admin],
            )
            .unwrap();

            let market = env.market;
            let vault = env.vault;
            let mut tracked = vec![
                market,
                vault,
                env.mint,
                env.vault_authority,
                env.payer.pubkey(),
                admin.pubkey(),
            ];
            tracked.extend(wallets);
            tracked.extend(ledgers);
            tracked.extend(actors.each_ref().map(Signer::pubkey));
            let initial_profiles = [profile(&env, 0), profile(&env, 1)];
            let initial_sequences = [env.control_sequences(0), env.control_sequences(1)];
            let ledger_frames = ledgers.map(|key| env.svm.get_account(&key));
            let mint_frame = env.svm.get_account(&env.mint);
            let vault_frame = env.svm.get_account(&vault).unwrap();
            let assert_funded = |env: &V16CuEnv| {
                let group = env.market_state().1;
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
                assert_eq!(group.insurance, RESERVE);
                assert_eq!(group.insurance_domain_budget_remaining_total, RESERVE);
                assert_eq!(group.vault, PRINCIPAL + RESERVE + PEER);
                for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
                    let expected = if domain == asset * 2 {
                        PRINCIPAL
                    } else if domain == (1 - asset) * 2 {
                        PEER
                    } else {
                        0
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
                        if domain == asset * 2 { RESERVE } else { 0 }
                    );
                    assert_eq!(group.insurance_domain_spent[domain], 0);
                }
                assert_eq!(env.svm.get_account(&vault), Some(vault_frame.clone()));
                assert_eq!(env.svm.get_account(&env.mint), mint_frame);
                assert_eq!(ledgers.map(|key| env.svm.get_account(&key)), ledger_frames);
                for (key, empty) in wallets.iter().zip(&empty_wallets) {
                    assert_eq!(env.svm.get_account(key), Some(empty.clone()));
                }
            };
            let exits = |env: &V16CuEnv| {
                let e = env.control_sequences(asset).authority_epoch;
                vec![
                    exit(
                        env,
                        peer.pubkey(),
                        wallets[3],
                        None,
                        1 - asset,
                        env.control_sequences(1 - asset).authority_epoch,
                        false,
                        PEER,
                    ),
                    exit(
                        env,
                        a.pubkey(),
                        wallets[0],
                        Some(ledgers[0]),
                        asset,
                        e,
                        false,
                        PRINCIPAL,
                    ),
                    exit(
                        env,
                        a.pubkey(),
                        wallets[0],
                        Some(ledgers[1]),
                        asset,
                        e,
                        true,
                        RESERVE,
                    ),
                ]
            };
            let bundle = |env: &V16CuEnv| {
                let mut ixs = exits(env);
                ixs.push(close(
                    env,
                    wallets[4],
                    env.control_sequences(0).authority_epoch + u64::from(asset == 0),
                ));
                ixs
            };
            assert_funded(&env);
            let old_bundle = bundle(&env);
            let retained_exit = signed(&env, &old_bundle, &[peer, a, &admin]);
            let retained_close = signed(&env, &old_bundle[3..], &[&admin]);
            let retained_peer = signed(&env, &old_bundle[..1], &[peer]);
            let old_handoff = handoff(
                &env,
                asset,
                kind,
                a.pubkey(),
                Some(c.pubkey()),
                initial_sequences[asset].authority_epoch,
            );
            let retained_handoff =
                signed(&env, &[old_bundle[0].clone(), old_handoff], &[peer, a, c]);
            for tx in [&retained_exit, &retained_peer, &retained_handoff] {
                peak = peak.max(prevalidate(&mut env, tx, &tracked));
            }
            let retained_bytes = bincode::serialize(&retained_exit).unwrap();

            for (step, from, to) in [(1, a, b), (2, b, a)] {
                // Cold-admin oracle replacement does not require the funded oracle's signature.
                let authorizer = if kind == processor::ASSET_AUTH_ORACLE {
                    cold
                } else {
                    from
                };
                let ix = handoff(
                    &env,
                    asset,
                    kind,
                    authorizer.pubkey(),
                    Some(to.pubkey()),
                    env.control_sequences(asset).authority_epoch,
                );
                let tx = signed(&env, &[ix], &[authorizer, to]);
                peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
                let mut expected = initial_profiles;
                if kind == processor::ASSET_AUTH_ORACLE {
                    expected[asset].oracle_authority = to.pubkey().to_bytes();
                } else {
                    expected[asset].insurance_authority = to.pubkey().to_bytes();
                }
                assert_eq!([profile(&env, 0), profile(&env, 1)], expected);
                let mut sequences = initial_sequences;
                sequences[asset].authority_epoch += step;
                assert_eq!(
                    [env.control_sequences(0), env.control_sequences(1)],
                    sequences
                );
                assert_funded(&env);
                if step == 1 {
                    let current_backing = exits(&env)[1].clone();
                    let tx = signed(&env, &[current_backing], &[a]);
                    peak = peak.max(prevalidate(&mut env, &tx, &tracked));
                    let revoked = handoff(
                        &env,
                        asset,
                        kind,
                        a.pubkey(),
                        Some(c.pubkey()),
                        env.control_sequences(asset).authority_epoch,
                    );
                    let tx = signed(&env, &[old_bundle[0].clone(), revoked], &[peer, a, c]);
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
            }
            assert_eq!(bincode::serialize(&retained_exit).unwrap(), retained_bytes);
            for tx in [retained_exit, retained_handoff] {
                assert_eq!(tx.message.recent_blockhash, env.svm.latest_blockhash());
                tx.verify().unwrap();
                peak = peak.max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[],
                    Some((3, PercolatorError::EngineStale)),
                    1,
                ));
                rollbacks += 1;
            }
            // The identical sibling envelope remains admissible. Its consent is
            // still needed in the full terminal bundle, so simulation does not spend it.
            peak = peak.max(prevalidate(&mut env, &retained_peer, &tracked));
            let fresh_bundle = bundle(&env);
            let renewed_handoff = handoff(
                &env,
                asset,
                kind,
                a.pubkey(),
                Some(c.pubkey()),
                env.control_sequences(asset).authority_epoch,
            );
            let renewed = signed(&env, &[renewed_handoff], &[a, c]);
            peak = peak.max(prevalidate(&mut env, &renewed, &tracked));
            let current = signed(&env, &fresh_bundle, &[peer, a, &admin]);
            peak = peak.max(prevalidate(&mut env, &current, &tracked));
            let mut stale_close = fresh_bundle.clone();
            stale_close[3] = old_bundle[3].clone();
            let tx = signed(&env, &stale_close, &[peer, a, &admin]);
            if asset == 0 {
                peak = peak.max(land(
                    &mut env,
                    tx,
                    &tracked,
                    &[],
                    Some((5, PercolatorError::EngineStale)),
                    3,
                ));
                rollbacks += 1;
            } else {
                peak = peak.max(prevalidate(&mut env, &tx, &tracked));
            }
            assert_funded(&env);

            // A distinct market-authority ABA invalidates retained CloseSlab
            // consent even after all funded roles returned to the original keys.
            let role_sequences = [env.control_sequences(0), env.control_sequences(1)];
            for (step, from, to) in [(1, &admin, c), (2, c, &admin)] {
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(from.pubkey(), true),
                        AccountMeta::new(to.pubkey(), true),
                        AccountMeta::new(market, false),
                    ],
                    data: ProgInstruction::UpdateAuthority {
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        new_pubkey: to.pubkey().to_bytes(),
                    }
                    .encode(),
                };
                let tx = signed(&env, &[ix], &[from, to]);
                peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
                assert_eq!(env.market_state().0.marketauth, to.pubkey().to_bytes());
                let mut expected_sequences = role_sequences;
                expected_sequences[0].authority_epoch += step;
                assert_eq!(
                    [env.control_sequences(0), env.control_sequences(1)],
                    expected_sequences
                );
                assert_eq!([profile(&env, 0), profile(&env, 1)], initial_profiles);
                assert_funded(&env);
            }
            // These CloseSlab bytes were also prevalidated as the tail of the
            // original funded bundle; the standalone envelope was signed then.
            retained_close.verify().unwrap();
            assert_eq!(
                retained_close.message.recent_blockhash,
                env.svm.latest_blockhash()
            );
            peak = peak.max(land(
                &mut env,
                retained_close,
                &tracked,
                &[],
                Some((2, PercolatorError::EngineStale)),
                0,
            ));
            rollbacks += 1;
            let mut stale_close = bundle(&env);
            stale_close[3] = fresh_bundle[3].clone();
            let tx = signed(&env, &stale_close, &[peer, a, &admin]);
            peak = peak.max(land(
                &mut env,
                tx,
                &tracked,
                &[],
                Some((5, PercolatorError::EngineStale)),
                3,
            ));
            rollbacks += 1;
            // The pre-signed current envelope also becomes stale at asset 0's payout.
            peak = peak.max(land(
                &mut env,
                current,
                &tracked,
                &[],
                Some((if asset == 0 { 3 } else { 2 }, PercolatorError::EngineStale)),
                usize::from(asset == 0),
            ));
            rollbacks += 1;
            assert_funded(&env);

            let final_ixs = bundle(&env);
            let final_tx = signed(&env, &final_ixs, &[peer, a, &admin]);
            peak = peak.max(prevalidate(&mut env, &final_tx, &tracked));
            let mut failed_ixs = final_ixs.clone();
            failed_ixs.push(Instruction {
                program_id: system_program::ID,
                accounts: vec![],
                data: vec![255],
            });
            let failed_tx = signed(&env, &failed_ixs, &[peer, a, &admin]);
            let mut keys = tracked.clone();
            keys.extend(&failed_tx.message.account_keys);
            keys.sort_unstable();
            keys.dedup();
            let mut before = frame(&env, &keys);
            let fee = FeeStructure::default().lamports_per_signature
                * u64::from(failed_tx.message.header.num_required_signatures);
            before[keys
                .iter()
                .position(|key| *key == env.payer.pubkey())
                .unwrap()]
            .as_mut()
            .unwrap()
            .lamports -= fee;
            let failure = env
                .svm
                .send_transaction(failed_tx)
                .expect_err("rejected suffix restores the completed terminal closure");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(6, InstructionError::InvalidInstructionData)
            );
            assert_eq!(
                frame(&env, &keys),
                before,
                "rent, ledger, token, vault closure and tombstone all roll back"
            );
            for (program, count) in [(env.program_id, 4), (spl_token::ID, 5)] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    count,
                    "{:?}",
                    failure.meta
                );
            }
            peak = peak.max(failure.meta.compute_units_consumed);
            rollbacks += 1;
            assert_funded(&env);

            let admin_before = env.svm.get_account(&admin.pubkey()).unwrap();
            let market_rent = env.svm.get_account(&market).unwrap().lamports;
            let changed = [
                market,
                vault,
                wallets[0],
                wallets[3],
                wallets[4],
                ledgers[0],
                ledgers[1],
                admin.pubkey(),
            ];
            peak = peak.max(land(&mut env, final_tx, &tracked, &changed, None, 5));
            let tombstone = env.svm.get_account(&market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                env.svm
                    .minimum_balance_for_rent_exemption(constants::HEADER_LEN)
            );
            let mut expected_admin = admin_before;
            expected_admin.lamports += market_rent - tombstone.lamports + vault_frame.lamports;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert!(env.svm.get_account(&vault).is_none_or(
                |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
            ));
            for ((wallet, empty), amount) in wallets.iter().zip(&empty_wallets).zip([
                (PRINCIPAL + RESERVE) as u64,
                0,
                0,
                PEER as u64,
                DUST,
            ]) {
                assert_eq!(
                    env.svm.get_account(wallet),
                    Some(token_image(empty, amount))
                );
            }
            assert_eq!(env.svm.get_account(&env.mint), mint_frame);
            let mint = Mint::unpack(&mint_frame.as_ref().unwrap().data).unwrap();
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(mint.supply, (PRINCIPAL + RESERVE + PEER) as u64 + DUST);
            let backing =
                state::read_backing_domain_ledger(&env.svm.get_account(&ledgers[0]).unwrap().data)
                    .unwrap();
            assert_eq!(backing.authority, a.pubkey().to_bytes());
            assert_eq!(
                (
                    backing.total_deposited_atoms,
                    backing.total_principal_atoms,
                    backing.total_principal_withdrawn_atoms
                ),
                (PRINCIPAL, 0, PRINCIPAL)
            );
            assert_eq!(
                (
                    backing.cumulative_loss_atoms,
                    backing.cumulative_recovery_atoms
                ),
                (0, 0)
            );
            let insurance =
                state::read_insurance_ledger(&env.svm.get_account(&ledgers[1]).unwrap().data)
                    .unwrap();
            assert_eq!(insurance.authority, a.pubkey().to_bytes());
            assert_eq!(
                (
                    insurance.total_deposited_atoms,
                    insurance.total_principal_atoms,
                    insurance.total_withdrawn_atoms
                ),
                (RESERVE, 0, RESERVE)
            );
            assert_eq!(
                (
                    insurance.cumulative_loss_atoms,
                    insurance.cumulative_profit_atoms,
                    insurance.last_observed_insurance_atoms
                ),
                (0, 0, 0)
            );
        }
    }
    assert_eq!(rollbacks, 30);
    assert_cu_within("retained terminal oracle/role envelopes", peak, 300_000);
    eprintln!(
        "INV-005 terminal oracle/role: worlds=4, exact_rollbacks={rollbacks}, peak_cu={peak}"
    );
}
