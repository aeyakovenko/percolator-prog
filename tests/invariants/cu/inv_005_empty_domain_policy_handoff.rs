//! INV-005/020/024/027: an empty side's policy cannot unlock its funded sibling's role.

use super::super::payout as domain_payout;
use super::*;

#[test]
fn v16_program_empty_domain_policy_preserves_funded_sibling_handoff_and_oracle_scope() {
    let mut peak = 0;
    for asset in 0..2 {
        for empty_side in 0..2 {
            let empty = asset * 2 + empty_side;
            let funded = asset * 2 + 1 - empty_side;
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let holders = [Keypair::new(), Keypair::new(), Keypair::new()];
            let successor = Keypair::new();
            let cold = env.admin.insecure_clone();
            let user = Keypair::new();
            let actors = [
                &holders[0],
                &holders[1],
                &holders[2],
                &successor,
                &cold,
                &user,
            ];
            let (wallets, portfolio) = fund_fixture(&mut env, &actors);
            env.try_update_per_asset_authority_with_cu(
                &cold,
                Some(&holders[2]),
                asset as u16,
                processor::ASSET_AUTH_ORACLE,
                holders[2].pubkey().to_bytes(),
            )
            .unwrap();
            env.svm.warp_to_slot(1);
            env.configure_auth_mark_for_asset_with_authority(asset as u16, &holders[2], 1, 100);

            let market = env.market;
            let vault = env.vault;
            let user_before = env.svm.get_account(&portfolio);
            let mut tracked = vec![market, vault, env.mint, env.vault_authority, portfolio];
            tracked.extend(wallets);
            tracked.extend(actors.map(Signer::pubkey));
            let mut book = Book {
                backing: BACKING,
                insurance: INSURANCE,
                wallets: [0; 6],
            };
            let mut profiles = [profile(&env, 0), profile(&env, 1)];
            let mut sequences = [env.control_sequences(0), env.control_sequences(1)];
            let mut cfg = env.market_state().0;
            let check =
                |env: &V16CuEnv, book: &Book, profiles: &[_; 2], sequences: &[_; 2], cfg: &_| {
                    book.check(env, &wallets);
                    assert_eq!([profile(env, 0), profile(env, 1)], *profiles);
                    assert_eq!(
                        [env.control_sequences(0), env.control_sequences(1)],
                        *sequences
                    );
                    assert_eq!(env.market_state().0, *cfg);
                    assert_eq!(env.svm.get_account(&portfolio), user_before);
                };
            check(&env, &book, &profiles, &sequences, &cfg);
            let mut run = |env: &mut V16CuEnv, tx, changed: &[Pubkey], error, spl| {
                peak = peak.max(land(env, tx, &tracked, changed, error, spl));
            };

            let ix = domain_payout(
                &env,
                holders[2].pubkey(),
                wallets[2],
                empty,
                true,
                BACKING[empty],
            );
            let tx = signed(&env, &[ix], &[&holders[2]]);
            run(&mut env, tx, &[market, vault, wallets[2]], None, 1);
            book.backing[empty] = 0;
            book.wallets[2] = BACKING[empty];
            check(&env, &book, &profiles, &sequences, &cfg);

            let policy = |env: &V16CuEnv, domain: usize, epoch, sequence, rate| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(holders[0].pubkey(), true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::UpdateBackingFeePolicy {
                    domain: domain as u16,
                    market_id: env.asset_market_id(asset as u16),
                    authority_epoch: epoch,
                    policy_sequence: sequence,
                    fee_bps: rate,
                    insurance_share_bps: 2_500,
                }
                .encode(),
            };
            let set_policy = |profiles: &mut [state::AssetOracleProfileV16; 2],
                              cfg: &mut state::WrapperConfigV16,
                              domain: usize,
                              rate| {
                if domain % 2 == 0 {
                    profiles[asset].backing_trade_fee_bps_long = rate;
                    profiles[asset].backing_trade_fee_insurance_share_bps_long = 2_500;
                    if asset == 0 {
                        cfg.backing_trade_fee_bps_long = rate;
                        cfg.backing_trade_fee_insurance_share_bps_long = 2_500;
                    }
                } else {
                    profiles[asset].backing_trade_fee_bps_short = rate;
                    profiles[asset].backing_trade_fee_insurance_share_bps_short = 2_500;
                    if asset == 0 {
                        cfg.backing_trade_fee_bps_short = rate;
                        cfg.backing_trade_fee_insurance_share_bps_short = 2_500;
                    }
                }
                cfg.backing_trade_fee_policy_count += 1;
            };
            let epoch = sequences[asset].authority_epoch;
            let consent = handoff(
                &env,
                asset,
                processor::ASSET_AUTH_BACKING_BUCKET,
                holders[2].pubkey(),
                Some(successor.pubkey()),
                epoch,
            );
            env.svm.expire_blockhash();
            let retained = signed(&env, &[consent], &[&holders[2], &successor]);
            let before = super::super::frame(&env, &tracked);
            env.svm
                .simulate_transaction(retained.clone().into())
                .unwrap();
            assert_eq!(super::super::frame(&env, &tracked), before);

            // The policy is admissible on the empty side. The role spans both sides.
            let sequence = next_control_sequence(sequences[asset].backing_fee.max(epoch));
            let prefix = [
                policy(&env, empty, epoch, sequence, 77),
                domain_payout(&env, holders[2].pubkey(), wallets[2], funded, true, 3),
            ];
            let retry = signed(&env, &prefix, &[&holders[0], &holders[2]]);
            let mut attack = prefix.to_vec();
            attack.push(handoff(
                &env,
                asset,
                processor::ASSET_AUTH_BACKING_BUCKET,
                cold.pubkey(),
                Some(cold.pubkey()),
                epoch,
            ));
            let tx = signed(&env, &attack, &[&holders[0], &holders[2], &cold]);
            run(
                &mut env,
                tx,
                &[],
                Some((4, PercolatorError::EngineLockActive)),
                1,
            );
            check(&env, &book, &profiles, &sequences, &cfg);

            run(&mut env, retry, &[market, vault, wallets[2]], None, 1);
            book.backing[funded] -= 3;
            book.wallets[2] += 3;
            sequences[asset].backing_fee = sequence;
            set_policy(&mut profiles, &mut cfg, empty, 77);
            check(&env, &book, &profiles, &sequences, &cfg);

            // The exact earlier incumbent consent survives the independent policy lane.
            run(&mut env, retained, &[market], None, 0);
            profiles[asset].backing_bucket_authority = successor.pubkey().to_bytes();
            sequences[asset].authority_epoch += 1;
            check(&env, &book, &profiles, &sequences, &cfg);
            let epoch = sequences[asset].authority_epoch;
            let redirect = policy(&env, funded, epoch, sequence + 1, 88);
            let tx = signed(&env, &[redirect.clone()], &[&holders[0]]);
            run(
                &mut env,
                tx,
                &[],
                Some((2, PercolatorError::EngineLockActive)),
                0,
            );
            let seize = handoff(
                &env,
                asset,
                processor::ASSET_AUTH_BACKING_BUCKET,
                cold.pubkey(),
                Some(cold.pubkey()),
                epoch,
            );
            let tx = signed(&env, &[seize.clone()], &[&cold]);
            run(
                &mut env,
                tx,
                &[],
                Some((2, PercolatorError::EngineLockActive)),
                0,
            );
            check(&env, &book, &profiles, &sequences, &cfg);

            env.svm.warp_to_slot(2);
            let observe = |holder| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(holder, true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::PushAuthMark {
                    asset_index: asset as u16,
                    market_id: env.asset_market_id(asset as u16),
                    authority_epoch: epoch,
                    observation_sequence: next_control_sequence(
                        sequences[asset].oracle_observation,
                    ),
                    now_slot: u64::MAX,
                    mark_e6: 100,
                }
                .encode(),
            };
            let wrong_mark = observe(successor.pubkey());
            let honest_mark = observe(holders[2].pubkey());
            let tx = signed(&env, &[wrong_mark], &[&successor]);
            run(
                &mut env,
                tx,
                &[],
                Some((2, PercolatorError::Unauthorized)),
                0,
            );
            check(&env, &book, &profiles, &sequences, &cfg);
            let tx = signed(&env, &[honest_mark], &[&holders[2]]);
            run(&mut env, tx, &[market], None, 0);
            sequences[asset].oracle_observation += 1;
            profiles[asset].last_good_oracle_slot = 2;
            if asset == 0 {
                cfg.last_good_oracle_slot = 2;
            }
            check(&env, &book, &profiles, &sequences, &cfg);

            let remaining = BACKING[funded] - 3;
            for (actor, recipient, error) in [
                (2, 3, PercolatorError::Unauthorized),
                (4, 3, PercolatorError::Unauthorized),
                (3, 2, PercolatorError::InvalidTokenAccount),
                (3, 4, PercolatorError::InvalidTokenAccount),
            ] {
                let ix = domain_payout(
                    &env,
                    actors[actor].pubkey(),
                    wallets[recipient],
                    funded,
                    true,
                    remaining,
                );
                let tx = signed(&env, &[ix], &[actors[actor]]);
                run(&mut env, tx, &[], Some((2, error)), 0);
                check(&env, &book, &profiles, &sequences, &cfg);
            }
            let ix = domain_payout(
                &env,
                successor.pubkey(),
                wallets[3],
                funded,
                true,
                remaining,
            );
            let tx = signed(&env, &[ix], &[&successor]);
            run(&mut env, tx, &[market, vault, wallets[3]], None, 1);
            book.backing[funded] = 0;
            book.wallets[3] = remaining;
            check(&env, &book, &profiles, &sequences, &cfg);

            // Both previously rejected operations become admissible only after full repayment.
            env.svm.expire_blockhash();
            let tx = signed(&env, &[redirect, seize], &[&holders[0], &cold]);
            run(&mut env, tx, &[market], None, 0);
            sequences[asset].backing_fee += 1;
            sequences[asset].authority_epoch += 1;
            profiles[asset].backing_bucket_authority = cold.pubkey().to_bytes();
            set_policy(&mut profiles, &mut cfg, funded, 88);
            check(&env, &book, &profiles, &sequences, &cfg);
            assert_eq!(
                book.wallets[2] + book.wallets[3],
                BACKING[empty] + BACKING[funded]
            );
            assert_eq!(book.wallets[4], 0);
        }
    }
    eprintln!("INV-005 empty-domain policy: 4 worlds, 32 exact rejections, 4 policy/SPL prefix rollbacks, 4 retained funded handoffs, 4 exact provider exits; peak CU={peak}");
}
