use super::*;

#[test]
fn v16_program_funded_restart_preserves_roles_but_revokes_retained_generation_consent() {
    // INV-005/020/024: restart carries both insurance budgets into a new generation,
    // but neither retained consent nor cold-admin ownership follows those atoms.
    const RESERVES: [u128; 2] = [23, 41];
    const PEER: u128 = 19;
    const TOTAL: u128 = RESERVES[0] + RESERVES[1];
    let mut peak = 0;
    let mut rollbacks = 0;
    for subject in 0..2usize {
        let peer = 1 - subject;
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
        for owner in [&incumbent, &successor] {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        }
        let wallets = [&incumbent, &admin, &successor]
            .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
        for (wallet, amount) in [(wallets[0], TOTAL), (wallets[1], PEER)] {
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
        env.configure_permissionless_resolve_with_cu(100, 1);
        for kind in [
            processor::ASSET_AUTH_INSURANCE,
            processor::ASSET_AUTH_INSURANCE_OPERATOR,
            processor::ASSET_AUTH_ORACLE,
        ] {
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&incumbent),
                subject as u16,
                kind,
                incumbent.pubkey().to_bytes(),
            )
            .unwrap();
        }
        for (domain, owner, wallet, amount) in [
            (2 * subject, &incumbent, wallets[0], RESERVES[0]),
            (2 * subject + 1, &incumbent, wallets[0], RESERVES[1]),
            (2 * peer, &admin, wallets[1], PEER),
        ] {
            let seq = env.control_sequences(domain / 2);
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain: domain as u16,
                    market_id: env.asset_market_id((domain / 2) as u16),
                    authority_epoch: seq.authority_epoch,
                    intent_id: seq.insurance_top_up + 1,
                    amount,
                },
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(wallet, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .unwrap();
        }
        set_test_clock(&mut env, 2, 100);
        let market = env.market;
        let vault = env.vault;
        let tracked = [
            market,
            vault,
            env.mint,
            env.vault_authority,
            incumbent.pubkey(),
            admin.pubkey(),
            successor.pubkey(),
            wallets[0],
            wallets[1],
            wallets[2],
        ];
        let initial_profile = profile(&env, subject);
        let initial_seq = env.control_sequences(subject);
        let peer_profile = profile(&env, peer);
        let peer_seq = env.control_sequences(peer);
        let generation = env.asset_market_id(subject as u16);
        let mint_image = env.svm.get_account(&env.mint);
        let check_book = |env: &V16CuEnv, subject_paid: bool, peer_paid: bool| {
            let group = env.market_state().1;
            let remaining = if subject_paid { 0 } else { TOTAL } + if peer_paid { 0 } else { PEER };
            assert_eq!(group.mode, MarketModeV16::Live);
            assert_eq!(group.c_tot, 0);
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(group.insurance, remaining);
            assert_eq!(group.insurance_domain_budget_remaining_total, remaining);
            assert_eq!(group.vault, remaining);
            assert_eq!(env.token_amount(vault) as u128, remaining);
            for domain in 0..group.insurance_domain_budget.len() {
                let expected = if domain / 2 == subject && !subject_paid {
                    RESERVES[domain % 2]
                } else if domain == 2 * peer && !peer_paid {
                    PEER
                } else {
                    0
                };
                assert_eq!(group.insurance_domain_budget[domain], expected);
                assert_eq!(group.insurance_domain_spent[domain], 0);
            }
            let tokens = [
                if subject_paid { TOTAL } else { 0 },
                if peer_paid { PEER } else { 0 },
                0,
            ];
            for (wallet, amount) in wallets.iter().zip(tokens) {
                assert_eq!(env.token_amount(*wallet) as u128, amount);
            }
            assert_eq!(env.svm.get_account(&env.mint), mint_image);
            let mint = Mint::unpack(&mint_image.as_ref().unwrap().data).unwrap();
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(mint.supply as u128, TOTAL + PEER);
            assert_eq!(remaining + tokens.iter().sum::<u128>(), TOTAL + PEER);
            crate::support::fuzz_model::assert_market_stock_census(
                "funded insurance restart",
                &group,
                &env.svm.get_account(&market).unwrap().data,
                &[],
                remaining,
            )
            .unwrap();
        };
        check_book(&env, false, false);
        let prefix = payout(&env, admin.pubkey(), wallets[1], peer * 2, false, 1);
        let requests = [
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(incumbent.pubkey(), true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::ConfigureAuthMark {
                    asset_index: subject as u16,
                    market_id: generation,
                    authority_epoch: initial_seq.authority_epoch,
                    observation_sequence: 17,
                    now_slot: 2,
                    initial_mark_e6: 120,
                }
                .encode(),
            },
            payout(
                &env,
                incumbent.pubkey(),
                wallets[0],
                subject * 2,
                false,
                TOTAL,
            ),
        ];
        let retained = requests.each_ref().map(|request| {
            signed(
                &env,
                &[prefix.clone(), request.clone()],
                &[&admin, &incumbent],
            )
        });
        let retained_bytes = retained
            .each_ref()
            .map(|tx| bincode::serialize(tx).unwrap());
        let peer_exit = signed(
            &env,
            &[payout(
                &env,
                admin.pubkey(),
                wallets[1],
                peer * 2,
                false,
                PEER,
            )],
            &[&admin],
        );
        let prevalidate = |env: &mut V16CuEnv, tx: &Transaction| {
            tx.verify().unwrap();
            assert_eq!(tx.message.recent_blockhash, env.svm.latest_blockhash());
            let mut keys = tx.message.account_keys.clone();
            keys.extend_from_slice(&tracked);
            let before = frame(env, &keys);
            let result = env.svm.simulate_transaction(tx.clone().into()).unwrap();
            assert_eq!(frame(env, &keys), before);
            result.compute_units_consumed
        };
        for tx in retained.iter().chain(std::iter::once(&peer_exit)) {
            peak = peak.max(prevalidate(&mut env, tx));
        }
        env.try_shutdown_asset_with_authority(&admin, subject as u16, 2)
            .unwrap();
        assert_eq!(
            env.market_state().1.assets[subject].lifecycle,
            AssetLifecycleV16::Recovery
        );
        assert_eq!(env.control_sequences(subject), initial_seq);
        check_book(&env, false, false);

        set_test_clock(&mut env, 3, 101);
        let restart = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
            ],
            data: ProgInstruction::RestartAssetOracle {
                asset_index: subject as u16,
                market_id: generation,
                authority_epoch: initial_seq.authority_epoch,
                observation_sequence: initial_seq.oracle_observation + 1,
                now_slot: 3,
                initial_price: 100,
            }
            .encode(),
        };
        let next_generation = env.market_state().1.next_market_id;
        let failed_restart = signed(
            &env,
            &[prefix.clone(), restart.clone(), requests[1].clone()],
            &[&admin, &incumbent],
        );
        peak = peak.max(land(
            &mut env,
            failed_restart,
            &tracked,
            &[],
            Some((4, PercolatorError::AssetGenerationMismatch)),
            1,
        ));
        rollbacks += 1;
        assert_eq!(env.market_state().1.next_market_id, next_generation);
        assert_eq!(env.asset_market_id(subject as u16), generation);
        peak = peak.max(prevalidate(&mut env, &retained[1]));
        let tx = signed(&env, &[restart], &[&admin]);
        peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
        assert_eq!(env.asset_market_id(subject as u16), next_generation);
        assert!(next_generation > generation);
        let mut restarted_seq = initial_seq;
        restarted_seq.oracle_observation += 1;
        assert_eq!(env.control_sequences(subject), restarted_seq);
        let restarted_profile = profile(&env, subject);
        assert_eq!(restarted_profile.asset_admin, initial_profile.asset_admin);
        assert_eq!(
            restarted_profile.insurance_authority,
            incumbent.pubkey().to_bytes()
        );
        assert_eq!(
            restarted_profile.insurance_operator,
            incumbent.pubkey().to_bytes()
        );
        assert_eq!(
            restarted_profile.oracle_authority,
            incumbent.pubkey().to_bytes()
        );
        assert_eq!(
            restarted_profile.backing_bucket_authority,
            initial_profile.backing_bucket_authority
        );
        assert_eq!(
            env.market_state().1.assets[subject].lifecycle,
            AssetLifecycleV16::Active
        );
        check_book(&env, false, false);

        for (tx, bytes) in retained.iter().zip(retained_bytes) {
            assert_eq!(bincode::serialize(tx).unwrap(), bytes);
            tx.verify().unwrap();
            assert_eq!(tx.message.recent_blockhash, env.svm.latest_blockhash());
            peak = peak.max(land(
                &mut env,
                tx.clone(),
                &tracked,
                &[],
                Some((3, PercolatorError::AssetGenerationMismatch)),
                1,
            ));
            rollbacks += 1;
        }
        for kind in [
            processor::ASSET_AUTH_INSURANCE,
            processor::ASSET_AUTH_INSURANCE_OPERATOR,
        ] {
            let attempt = handoff(
                &env,
                subject,
                kind,
                admin.pubkey(),
                Some(successor.pubkey()),
                restarted_seq.authority_epoch,
            );
            let tx = signed(&env, &[prefix.clone(), attempt], &[&admin, &successor]);
            peak = peak.max(land(
                &mut env,
                tx,
                &tracked,
                &[],
                Some((3, PercolatorError::EngineLockActive)),
                1,
            ));
            rollbacks += 1;
        }
        assert_eq!(profile(&env, subject), restarted_profile);
        assert_eq!(env.control_sequences(subject), restarted_seq);
        assert_eq!(profile(&env, peer), peer_profile);
        assert_eq!(env.control_sequences(peer), peer_seq);
        check_book(&env, false, false);

        // Only the generation changes: signer, epoch, sequence, amount and destination
        // are identical, so generation rejection cannot be masked by another stale bound.
        let renewed = requests.map(|mut request| {
            let mut ix = ProgInstruction::decode(&request.data).unwrap();
            match &mut ix {
                ProgInstruction::ConfigureAuthMark { market_id, .. }
                | ProgInstruction::WithdrawInsuranceAsset { market_id, .. } => {
                    assert_eq!(*market_id, generation);
                    *market_id = next_generation;
                }
                _ => unreachable!(),
            }
            request.data = ix.encode();
            request
        });
        for request in &renewed {
            let tx = signed(
                &env,
                &[prefix.clone(), request.clone()],
                &[&admin, &incumbent],
            );
            peak = peak.max(prevalidate(&mut env, &tx));
        }
        let tx = signed(&env, &[renewed[0].clone()], &[&incumbent]);
        peak = peak.max(land(&mut env, tx, &tracked, &[market], None, 0));
        assert_eq!(env.control_sequences(subject).oracle_observation, 17);
        assert_eq!(
            env.control_sequences(subject).authority_epoch,
            initial_seq.authority_epoch
        );
        let observed = profile(&env, subject);
        assert_eq!(
            observed.oracle_mode,
            percolator_prog::constants::ORACLE_MODE_AUTH_MARK
        );
        assert_eq!(
            (observed.mark_ewma_e6, observed.last_good_oracle_slot),
            (120, 3)
        );
        assert_eq!(env.market_state().1.assets[subject].effective_price, 120);
        check_book(&env, false, false);
        let tx = signed(&env, &[renewed[1].clone()], &[&incumbent]);
        peak = peak.max(land(
            &mut env,
            tx,
            &tracked,
            &[market, vault, wallets[0]],
            None,
            1,
        ));
        assert_eq!(
            env.control_sequences(subject).authority_epoch,
            initial_seq.authority_epoch + 1
        );
        assert_eq!(profile(&env, subject), observed);
        check_book(&env, true, false);
        peak = peak.max(land(
            &mut env,
            peer_exit,
            &tracked,
            &[market, vault, wallets[1]],
            None,
            1,
        ));
        check_book(&env, true, true);
    }
    assert_eq!(rollbacks, 10);
    eprintln!("funded insurance restart: 2 worlds, {rollbacks} exact rollbacks, peak {peak} CU");
}
