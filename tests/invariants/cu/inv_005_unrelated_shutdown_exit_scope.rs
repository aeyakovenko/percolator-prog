//! Row 416 / INV-005: a peer's shutdown insurance exit consumes the signing
//! authority's scope, even while a different asset's oracle coholds live backing.
//! Net-new versus terminal oracle ABA and shutdown reserve ABA: no authority
//! rotates before replay. The same peer payout preserves retained cold-oracle
//! consent through the local operator, but revokes it through the market fallback.
//! A real owner SPL prefix must roll back with the stale signed oracle envelope.

use super::*;

const PRINCIPAL: u128 = 41;
const RESERVE: u128 = 59;
const CAPITAL: u128 = 23;
const PEER_EXIT: u128 = 17;
const OWNER_PREFIX: u128 = 7;

#[test]
fn v16_program_peer_shutdown_exit_consumes_only_its_signing_authority_scope() {
    let mut peak = 0;
    for fallback in [false, true] {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                ..V16CuMarketParams::default()
            },
        );
        let admin = env.admin.insecure_clone();
        let actors: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
        let [incumbent, cold, incoming, peer, owner] = actors.each_ref();
        for actor in &actors {
            env.svm.airdrop(&actor.pubkey(), 1_000_000_000).unwrap();
        }
        for (asset, kind, holder) in [
            (0, processor::ASSET_AUTH_BACKING_BUCKET, incumbent),
            (0, processor::ASSET_AUTH_ORACLE, incumbent),
            (0, processor::ASSET_AUTH_ADMIN, cold),
            (1, processor::ASSET_AUTH_INSURANCE, peer),
            (1, processor::ASSET_AUTH_INSURANCE_OPERATOR, peer),
        ] {
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(holder),
                asset,
                kind,
                holder.pubkey().to_bytes(),
            )
            .unwrap();
        }
        for (asset, oracle) in [(0, incumbent), (1, &admin)] {
            env.configure_auth_mark_for_asset_with_authority(asset, oracle, 0, 100);
        }
        let wallets = actors
            .each_ref()
            .map(|actor| create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint));
        for (wallet, amount) in [
            (wallets[0], PRINCIPAL),
            (wallets[3], RESERVE),
            (wallets[4], CAPITAL),
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
        for (asset, holder, wallet, backing) in [
            (0, incumbent, wallets[0], true),
            (1, peer, wallets[3], false),
        ] {
            let sequence = env.control_sequences(asset);
            let data = if backing {
                ProgInstruction::TopUpBackingBucket {
                    domain: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch: sequence.authority_epoch,
                    intent_id: next_control_sequence(sequence.backing_top_up),
                    backing_fee_bps: 0,
                    insurance_share_bps: 0,
                    amount: PRINCIPAL,
                    expiry_slot: 10_000,
                }
            } else {
                ProgInstruction::TopUpInsuranceDomain {
                    domain: 2,
                    market_id: env.asset_market_id(1),
                    authority_epoch: sequence.authority_epoch,
                    intent_id: next_control_sequence(sequence.insurance_top_up),
                    amount: RESERVE,
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
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .unwrap();
        env.send(
            env.deposit_ix(portfolio, CAPITAL),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(wallets[4], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
        .unwrap();
        env.configure_permissionless_resolve_with_cu(100, 5);
        env.svm.warp_to_slot(2);
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 1, 2, 0);
        env.svm.warp_to_slot(7);
        env.configure_auth_mark_for_asset_with_authority(0, incumbent, 7, 100);

        let market = env.market;
        let vault = env.vault;
        let mut tracked = vec![market, vault, env.mint, portfolio, env.payer.pubkey()];
        tracked.extend(wallets);
        tracked.extend(actors.each_ref().map(Signer::pubkey));
        tracked.push(admin.pubkey());
        let mut profiles = [profile(&env, 0), profile(&env, 1)];
        let mut sequences = [env.control_sequences(0), env.control_sequences(1)];
        let generations = [env.asset_market_id(0), env.asset_market_id(1)];
        let book = |env: &V16CuEnv, paid: [u128; 3]| {
            let (cfg, group) = env.market_state();
            assert_eq!(cfg.marketauth, admin.pubkey().to_bytes());
            assert_eq!(group.mode, MarketModeV16::Live);
            assert_eq!(group.assets[0].lifecycle, AssetLifecycleV16::Active);
            assert_eq!(group.assets[1].lifecycle, AssetLifecycleV16::Recovery);
            assert_eq!(
                [env.asset_market_id(0), env.asset_market_id(1)],
                generations
            );
            assert_eq!(group.c_tot, CAPITAL - paid[2]);
            assert_eq!(
                env.portfolio_state(portfolio).capital.get(),
                CAPITAL - paid[2]
            );
            assert_eq!(env.portfolio_state(portfolio).pnl.get(), 0);
            assert_eq!(group.pnl_pos_tot, 0);
            assert_eq!(group.source_claim_bound_total_num, 0);
            assert_eq!(group.backing_provider_earnings_total, 0);
            assert_eq!(group.insurance, RESERVE - paid[1]);
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                RESERVE - paid[1]
            );
            for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
                assert_eq!(
                    bucket.fresh_unliened_backing_num,
                    if domain == 0 {
                        (PRINCIPAL - paid[0]) * BOUND_SCALE
                    } else {
                        0
                    }
                );
                assert_eq!(bucket.valid_liened_backing_num, 0);
                assert_eq!(bucket.impaired_liened_backing_num, 0);
                assert_eq!(bucket.consumed_liened_backing_num, 0);
                assert_eq!(bucket.utilization_fee_earnings, 0);
                assert_eq!(
                    group.insurance_domain_budget[domain],
                    if domain == 2 { RESERVE - paid[1] } else { 0 }
                );
                assert_eq!(group.insurance_domain_spent[domain], 0);
            }
            let balances = [paid[0], 0, 0, paid[1], paid[2]];
            for (wallet, amount) in wallets.iter().zip(balances) {
                assert_eq!(env.token_amount(*wallet) as u128, amount);
            }
            let remaining = PRINCIPAL + RESERVE + CAPITAL - paid.iter().sum::<u128>();
            assert_eq!(group.vault, remaining);
            assert_eq!(env.token_amount(vault) as u128, remaining);
            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(mint.supply as u128, PRINCIPAL + RESERVE + CAPITAL);
            assert_eq!(
                mint.supply as u128,
                remaining + balances.iter().sum::<u128>()
            );
        };
        let owner_exit = |env: &V16CuEnv, amount| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(wallets[4], false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env.withdraw_ix(portfolio, amount).encode(),
        };
        let prefix = owner_exit(&env, OWNER_PREFIX);
        let mut oracle = handoff(
            &env,
            0,
            processor::ASSET_AUTH_ORACLE,
            cold.pubkey(),
            Some(incoming.pubkey()),
            sequences[0].authority_epoch,
        );
        let retained = signed(
            &env,
            &[prefix.clone(), oracle.clone()],
            &[owner, cold, incoming],
        );
        let wire = bincode::serialize(&retained).unwrap();
        let signers = &retained.message.account_keys
            [..retained.message.header.num_required_signatures as usize];
        assert!(!signers.contains(&incumbent.pubkey()));
        assert!(!signers.contains(&admin.pubkey()));
        let before = frame(&env, &tracked);
        let simulation = env
            .svm
            .simulate_transaction(retained.clone().into())
            .expect("cold oracle replacement and owner exit are admissible before the peer debit");
        peak = peak.max(simulation.compute_units_consumed);
        assert_eq!(frame(&env, &tracked), before);
        book(&env, [0, 0, 0]);

        // Both routes pay the same shutdown-domain operator. Only the signer and
        // its selected epoch differ; no role handoff or generation change occurs.
        let debit_scope = if fallback { 0 } else { 1 };
        let signer = if fallback { &admin } else { peer };
        let mut peer_exit = payout(&env, signer.pubkey(), wallets[3], 2, false, PEER_EXIT);
        peer_exit.data = ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 1,
            market_id: generations[1],
            authority_epoch: sequences[debit_scope].authority_epoch,
            amount: PEER_EXIT,
        }
        .encode();
        let tx = signed(&env, &[peer_exit], &[signer]);
        peak = peak.max(land(
            &mut env,
            tx,
            &tracked,
            &[market, vault, wallets[3]],
            None,
            1,
        ));
        sequences[debit_scope].authority_epoch += 1;
        assert_eq!(
            [env.control_sequences(0), env.control_sequences(1)],
            sequences
        );
        assert_eq!([profile(&env, 0), profile(&env, 1)], profiles);
        book(&env, [0, PEER_EXIT, 0]);

        retained.verify().unwrap();
        assert_eq!(bincode::serialize(&retained).unwrap(), wire);
        assert_eq!(
            retained.message.recent_blockhash,
            env.svm.latest_blockhash()
        );
        if fallback {
            // A peer-domain debit through market authority revokes the pre-signed
            // asset-0 cold-admin request, although every role key is unchanged.
            peak = peak.max(land(
                &mut env,
                retained,
                &tracked,
                &[],
                Some((3, PercolatorError::EngineStale)),
                1,
            ));
            book(&env, [0, PEER_EXIT, 0]);
            let fresh = handoff(
                &env,
                0,
                processor::ASSET_AUTH_ORACLE,
                cold.pubkey(),
                Some(incoming.pubkey()),
                sequences[0].authority_epoch,
            );
            assert_eq!(fresh.accounts, oracle.accounts);
            oracle = fresh;
            let tx = signed(&env, &[prefix, oracle], &[owner, cold, incoming]);
            peak = peak.max(land(
                &mut env,
                tx,
                &tracked,
                &[market, vault, portfolio, wallets[4]],
                None,
                1,
            ));
        } else {
            // The exact same signed envelope survives the local peer exit.
            peak = peak.max(land(
                &mut env,
                retained,
                &tracked,
                &[market, vault, portfolio, wallets[4]],
                None,
                1,
            ));
        }
        sequences[0].authority_epoch += 1;
        profiles[0].oracle_authority = incoming.pubkey().to_bytes();
        assert_eq!(
            [env.control_sequences(0), env.control_sequences(1)],
            sequences
        );
        assert_eq!([profile(&env, 0), profile(&env, 1)], profiles);
        book(&env, [0, PEER_EXIT, OWNER_PREFIX]);

        // Observation power still conveys no right to the live funded backing.
        let seize = handoff(
            &env,
            0,
            processor::ASSET_AUTH_BACKING_BUCKET,
            cold.pubkey(),
            Some(incoming.pubkey()),
            sequences[0].authority_epoch,
        );
        let tx = signed(&env, &[seize], &[cold, incoming]);
        peak = peak.max(land(
            &mut env,
            tx,
            &tracked,
            &[],
            Some((2, PercolatorError::EngineLockActive)),
            0,
        ));
        let exits = [
            payout(&env, incumbent.pubkey(), wallets[0], 0, true, PRINCIPAL),
            owner_exit(&env, CAPITAL - OWNER_PREFIX),
            payout(
                &env,
                peer.pubkey(),
                wallets[3],
                2,
                false,
                RESERVE - PEER_EXIT,
            ),
        ];
        let tx = signed(&env, &exits, &[incumbent, owner, peer]);
        peak = peak.max(land(
            &mut env,
            tx,
            &tracked,
            &[market, vault, portfolio, wallets[0], wallets[3], wallets[4]],
            None,
            3,
        ));
        sequences[1].authority_epoch += 1;
        assert_eq!(
            [env.control_sequences(0), env.control_sequences(1)],
            sequences
        );
        assert_eq!([profile(&env, 0), profile(&env, 1)], profiles);
        book(&env, [PRINCIPAL, RESERVE, CAPITAL]);
    }
    assert_cu_within("peer shutdown debit and cold oracle scope", peak, 200_000);
    eprintln!("INV-005 peer shutdown scope: 2 worlds, 2 prevalidated envelopes, 1 unchanged retained success, 1 stale SPL-prefix rollback, 2 funded takeover rejections, exact final payouts; peak {peak} CU");
}
