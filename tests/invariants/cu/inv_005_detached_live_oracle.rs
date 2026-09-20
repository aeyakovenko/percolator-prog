//! Row 416: market-authority ABA cannot reacquire an oracle delegated over live exposure.
//! Unlike the detached backing/operator test, the protected role has no reserve stock:
//! its funding is the independent traders' exposure to its authenticated mark.

use super::*;

#[test]
fn v16_program_market_authority_aba_preserves_detached_live_oracle_and_owner_exits() {
    const MARKET_B: usize = 2;
    const ORACLE_C: usize = 3;
    const PREFIX_OWNER: usize = 4;
    const PREFIX: u128 = 7;
    let mut peak_cu = 0;
    for direction in [-1i128, 1] {
        let config = MarketConfig::default();
        let deposits = config.actor_deposits;
        let mut env = V16Svm::new([0x5c; 32], config);
        env.begin_public_trace();
        let size = direction * 10 * percolator::POS_SCALE as i128;
        let peer_size = -direction * 6 * percolator::POS_SCALE as i128;
        env.trade_no_cpi(0, 1, 0, size, INITIAL_PRICE, 0).unwrap();
        env.trade_no_cpi(MARKET_B, ORACLE_C, 1, peer_size, INITIAL_PRICE, 0)
            .unwrap();
        let (original_cfg, economy) = env.primary_market_state();
        let profiles = [0, 1].map(|asset| env.primary_profile(asset));
        let sequences = [0, 1].map(|asset| env.primary_control_sequences(asset));
        let portfolios: Vec<_> = (0..5)
            .map(|owner| env.primary_portfolio_data(owner))
            .collect();
        let a = original_cfg.marketauth;
        let b = env.actors[MARKET_B].signer.pubkey().to_bytes();
        let c = env.actors[ORACLE_C].signer.pubkey().to_bytes();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
        assert_eq!(economy.insurance, 0);
        assert_eq!(economy.source_claim_bound_total_num, 0);
        for asset in [0, 1] {
            assert!(economy.assets[asset].oi_eff_long_q > 0);
            assert!(economy.assets[asset].oi_eff_short_q > 0);
            assert_eq!(profiles[asset].oracle_authority, a);
        }
        let mut expected_profile = profiles[0];
        let mut expected_sequence = sequences[0];
        let check_management = |env: &V16Svm,
                                market_key,
                                expected_profile: state::AssetOracleProfileV16,
                                expected_sequence| {
            let mut cfg = original_cfg;
            cfg.marketauth = market_key;
            cfg.last_good_oracle_slot = expected_profile.last_good_oracle_slot;
            let (actual_cfg, actual_economy) = env.primary_market_state();
            assert_eq!(actual_cfg, cfg);
            assert_eq!(actual_economy, economy);
            assert_eq!(env.primary_profile(0), expected_profile);
            assert_eq!(env.primary_control_sequences(0), expected_sequence);
            assert_eq!(env.primary_profile(1), profiles[1]);
            assert_eq!(env.primary_control_sequences(1), sequences[1]);
            assert_eq!(
                (0..5)
                    .map(|owner| env.primary_portfolio_data(owner))
                    .collect::<Vec<_>>(),
                portfolios
            );
            assert_eq!(
                env.token_amount(env.vault) as u128,
                deposits.iter().sum::<u128>()
            );
            for owner in 0..5 {
                assert_eq!(env.token_amount(env.actors[owner].destination_token), 0);
            }
            assert_eq!(env.token_supply_observed(), env.initial_token_supply);
        };

        let withdrawal = env.build_retained_withdrawal(PREFIX_OWNER, PREFIX);
        let initial_handoff = env.build_retained_market_authority_handoff_from_admin(MARKET_B);
        let retained =
            env.bundle_retained_transactions(&[withdrawal.clone(), initial_handoff.clone()]);
        retained.verify().unwrap();
        assert!(bincode::serialized_size(&retained).unwrap() <= 1_232);
        env.svm
            .simulate_transaction(retained.clone().into())
            .expect("A's coheld oracle can follow its consented market handoff over live exposure");
        check_management(&env, a, expected_profile, expected_sequence);

        env.land_retained(initial_handoff).unwrap();
        expected_profile.asset_admin = b;
        expected_profile.insurance_authority = b;
        expected_profile.insurance_operator = b;
        expected_profile.backing_bucket_authority = b;
        expected_profile.oracle_authority = b;
        expected_sequence.authority_epoch += 1;
        check_management(&env, b, expected_profile, expected_sequence);

        env.update_asset_authority_between_actors(
            0,
            processor::ASSET_AUTH_ORACLE,
            MARKET_B,
            ORACLE_C,
        )
        .expect("B consents to detach only its funded oracle role to C");
        expected_profile.oracle_authority = c;
        expected_sequence.authority_epoch += 1;
        check_management(&env, b, expected_profile, expected_sequence);

        env.update_market_authority_to_admin(MARKET_B).unwrap();
        expected_profile.asset_admin = a;
        expected_profile.insurance_authority = a;
        expected_profile.insurance_operator = a;
        expected_profile.backing_bucket_authority = a;
        expected_sequence.authority_epoch += 1;
        check_management(&env, a, expected_profile, expected_sequence);

        // A is the current market/cold authority again, but neither its old epoch
        // nor newly signed cold-admin consent can reclaim C's live oracle.
        reject(&mut env, retained, 4, PercolatorError::EngineStale, 1, 1);
        let seizure = env.build_retained_asset_authority_handoff_from_admin(
            0,
            processor::ASSET_AUTH_ORACLE,
            MARKET_B,
        );
        let takeover = env.bundle_retained_transactions(&[withdrawal.clone(), seizure]);
        reject(
            &mut env,
            takeover,
            4,
            PercolatorError::EngineLockActive,
            1,
            1,
        );
        check_management(&env, a, expected_profile, expected_sequence);

        // Renew the identical market handoff at the current epoch. Automatic
        // rekeying follows A's remaining roles while C's exposure-funded role stays put.
        let renewed = env.build_retained_market_authority_handoff_from_admin(MARKET_B);
        env.land_retained(renewed).unwrap();
        expected_profile.asset_admin = b;
        expected_profile.insurance_authority = b;
        expected_profile.insurance_operator = b;
        expected_profile.backing_bucket_authority = b;
        expected_sequence.authority_epoch += 1;
        check_management(&env, b, expected_profile, expected_sequence);

        env.warp_to_slot(2);
        let price = INITIAL_PRICE + INITIAL_PRICE / 10;
        let old_a = env.build_retained_auth_mark(0, price);
        reject(&mut env, old_a, 3, PercolatorError::Unauthorized, 0, 0);
        let old_b = env.build_retained_market_control_for_actor(
            MARKET_B,
            ProgInstruction::PushAuthMark {
                asset_index: 0,
                market_id: economy.assets[0].market_id,
                now_slot: 2,
                mark_e6: price,
                observation_sequence: expected_sequence.oracle_observation + 1,
                authority_epoch: expected_sequence.authority_epoch,
            },
        );
        reject(&mut env, old_b, 3, PercolatorError::Unauthorized, 0, 0);
        check_management(&env, b, expected_profile, expected_sequence);
        env.push_auth_mark_for_actor(ORACLE_C, 0, u64::MAX, INITIAL_PRICE)
            .expect("the detached incumbent alone retains subject observation authority");
        expected_profile.last_good_oracle_slot = 2;
        expected_sequence.oracle_observation += 1;
        check_management(&env, b, expected_profile, expected_sequence);

        env.land_retained(withdrawal)
            .expect("the exact rolled-back SPL prefix remains payable");
        env.trade_no_cpi(0, 1, 0, -size, INITIAL_PRICE, 0).unwrap();
        assert_eq!(env.primary_market_state().1.assets[1], economy.assets[1]);
        env.trade_no_cpi(MARKET_B, ORACLE_C, 1, -peer_size, INITIAL_PRICE, 0)
            .unwrap();
        let mut remaining = deposits.iter().sum::<u128>() - PREFIX;
        for owner in [0, 1, MARKET_B, ORACLE_C, PREFIX_OWNER] {
            let owed = deposits[owner] - if owner == PREFIX_OWNER { PREFIX } else { 0 };
            env.withdraw_primary(owner, owed)
                .expect("each owner retains its exact live principal exit");
            remaining -= owed;
            assert_eq!(
                env.token_amount(env.actors[owner].destination_token) as u128,
                deposits[owner]
            );
            let group = env.primary_market_state().1;
            assert_eq!(
                (group.c_tot, group.pnl_pos_tot, group.vault),
                (remaining, 0, remaining)
            );
            assert_eq!(env.token_amount(env.vault) as u128, remaining);
            assert_eq!(env.token_supply_observed(), env.initial_token_supply);
        }
        assert_eq!(remaining, 0);
        assert_eq!(env.primary_profile(0).oracle_authority, c);
        assert_eq!(env.primary_profile(1), profiles[1]);
        assert_eq!(env.primary_control_sequences(1), sequences[1]);
        let trace = env.finish_public_trace();
        trace.validate_public_execution().unwrap();
        assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 4);
        peak_cu = peak_cu.max(
            trace
                .steps
                .iter()
                .filter_map(|step| step.compute_units)
                .max()
                .unwrap(),
        );
    }
    eprintln!("row416 detached live oracle: worlds=2, exact_rejections=8, SPL_rollbacks=4, owner_exits=10, peak_success_cu={peak_cu}");
}
