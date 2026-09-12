//! INV-012 / row 412: funded close cancellation revokes the closing portfolio's grant.
//!
//! Install consent after the public close-producing reduction, then retain CPI consumers.
//! Cure changes the close episode without changing positions; refreshing a request's episode
//! must not restore standing authority. Only an explicit new grant permits the same fill.
//! The active close initially prevents trading, so an untouched funded LP supplies the live
//! retained control. This is distinct from retained cure consent and reduction/forfeit revocation.

use super::*;

#[test]
fn v16_program_funded_close_cancellation_requires_fresh_matcher_capability() {
    const CURE: u128 = 2_000_000;
    const PRICE: u64 = 100;
    const EXPIRY: u64 = 100;
    const CAP: u16 = 37;
    const GRANT_SEQUENCE: u64 = 3; // One deposit, then two explicit grants.
    let mut peak_cu = 0;

    for batch in [false, true] {
        for sign in [-1i128, 1] {
            let label = format!("batch={batch}, sign={sign}");
            let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
                max_portfolio_assets: 2,
                initial_price: 1_000_000,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            });
            env.configure_auth_mark_with_cu(0, 1_000_000);
            env.configure_auth_mark_for_asset_as_admin(1, 0, PRICE);
            let winner_owner = Keypair::new();
            let lp_owner = Keypair::new();
            let cranker_owner = Keypair::new();
            let winner = env.create_portfolio(&winner_owner);
            let lp = env.create_portfolio(&lp_owner);
            let cranker = env.create_portfolio(&cranker_owner);
            let winner_source = env.deposit(&winner_owner, winner, 1_000_000);
            let lp_source = env.deposit(&lp_owner, lp, 161_600);
            let cranker_source = env.deposit(&cranker_owner, cranker, 1);
            let close_size = POS_SCALE as i128 * 3 / 4;
            env.trade_with_cu(
                &winner_owner,
                winner,
                &lp_owner,
                lp,
                close_size,
                1_000_000,
                0,
            );
            let mut mark = 1_000_000;
            for slot in 1..=20 {
                mark = mark * 10_500 / 10_000;
                env.svm.warp_to_slot(slot);
                env.push_auth_mark_with_cu(slot, mark);
                env.push_auth_mark_for_asset_as_admin(1, slot, PRICE);
                env.crank_if_actionable(
                    cranker,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations_for_assets(&[0, 1]),
                    },
                );
            }
            let close_price = env.market_state().1.assets[0].effective_price;
            env.trade_with_cu(
                &winner_owner,
                winner,
                &lp_owner,
                lp,
                -close_size,
                close_price,
                0,
            );
            let closing = env.portfolio_state(lp);
            let close = close_progress(&closing);
            assert!(close.active && !close.canceled && !close.finalized);
            assert_eq!(close.residual_remaining, close.gross_loss_at_close_start);
            assert!(close.residual_remaining > 0);
            assert_eq!(close.support_consumed, 0);
            assert_eq!(close.junior_face_burned, 0);
            assert_eq!(close.insurance_spent, 0);
            assert_eq!(close.b_loss_booked, 0);
            assert_eq!(close.explicit_loss_assigned, 0);
            assert_eq!(close.quantity_adl_applied_q, 0);
            assert_eq!(close.drift_consumed, 0);

            // A separate live asset excludes the close asset's pending-loss admission barrier.
            let taker_owner = Keypair::new();
            let peer_owner = Keypair::new();
            let taker = env.create_portfolio(&taker_owner);
            let peer = env.create_portfolio(&peer_owner);
            let taker_source = env.deposit(&taker_owner, taker, 1_000_000);
            let peer_source = env.deposit(&peer_owner, peer, 1_000_000);
            let cure_source = env.token_account_for_mint(env.mint, lp_owner.pubkey(), CURE as u64);
            let matcher_program = Pubkey::new_unique();
            env.svm.add_program(
                matcher_program,
                &std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF"),
            );
            let [(context, delegate), (peer_context, peer_delegate)] =
                [(&lp_owner, lp), (&peer_owner, peer)].map(|(owner, portfolio)| {
                    assert_eq!(env.portfolio_matcher_sequence(portfolio), 1);
                    // This helper publicly initializes the context and installs the first grant.
                    let (context, delegate, _) = env.init_auth_matcher_context_via_system_create(
                        matcher_program,
                        owner,
                        portfolio,
                    );
                    env.try_set_matcher_config_with_trade_fee_cap_and_expiry(
                        matcher_program,
                        owner,
                        portfolio,
                        context,
                        delegate,
                        1,
                        CAP,
                        EXPIRY,
                    )
                    .expect("owner installs bounded consent after close creation");
                    assert_eq!(env.portfolio_matcher_sequence(portfolio), GRANT_SEQUENCE);
                    (context, delegate)
                });
            let grant = env.portfolio_matcher_config(lp);
            assert_eq!(grant.enabled(), 1);
            assert_eq!(grant.trade_fee_cap_bps(), CAP);
            assert_eq!(env.portfolio_matcher_expiry(lp), EXPIRY);
            let epoch = env.portfolio_position_epoch(lp);
            let taker_epoch = env.portfolio_position_epoch(taker);
            let identities = [lp, taker, peer].map(|key| env.portfolio_id(key));
            let generations = [0, 1].map(|asset| env.asset_market_id(asset));
            assert_eq!(close_progress(&env.portfolio_state(lp)), close);
            let size = sign * POS_SCALE as i128;
            let trade = |env: &V16CuEnv, maker| {
                if batch {
                    env.batch_trade_cpi_ix_with_caps(
                        taker,
                        maker,
                        vec![BatchTradeCpiLeg {
                            asset_index: 1,
                            market_id: env.asset_market_id(1),
                            size_q: size,
                            fee_bps: 0,
                            limit_price: PRICE,
                        }],
                        0,
                        0,
                    )
                } else {
                    env.trade_cpi_ix(taker, maker, 1, size, 0, PRICE)
                }
            };
            let sign_trade = |env: &V16CuEnv, ix: &ProgInstruction, maker, ctx, delegate| {
                Transaction::new_signed_with_payer(
                    &[
                        heap_ix(),
                        cu_ix(),
                        Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(taker_owner.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(taker, false),
                                AccountMeta::new(maker, false),
                                AccountMeta::new_readonly(matcher_program, false),
                                AccountMeta::new(ctx, false),
                                AccountMeta::new_readonly(delegate, false),
                            ],
                            data: ix.encode(),
                        },
                    ],
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &taker_owner],
                    env.svm.latest_blockhash(),
                )
            };
            let retained_ix = trade(&env, lp);
            let retained = sign_trade(&env, &retained_ix, lp, context, delegate);
            let untouched = sign_trade(&env, &trade(&env, peer), peer, peer_context, peer_delegate);
            assert_eq!(retained.signatures.len(), 2, "no LP owner signature");
            let blockhash = retained.message.recent_blockhash;
            let slot = env.svm.get_sysvar::<Clock>().slot;
            assert!(slot < EXPIRY);
            let keys = [
                env.market,
                lp,
                taker,
                peer,
                winner,
                cranker,
                context,
                delegate,
                peer_context,
                peer_delegate,
                env.vault,
                env.mint,
                cure_source,
                lp_source,
                taker_source,
                peer_source,
                winner_source,
                cranker_source,
                lp_owner.pubkey(),
                taker_owner.pubkey(),
                peer_owner.pubkey(),
                winner_owner.pubkey(),
                cranker_owner.pubkey(),
            ];
            // Only the independent network fee payer is excluded from economic rollback.
            let snapshot = |env: &V16CuEnv| keys.map(|key| env.svm.get_account(&key));
            let before = snapshot(&env);
            let vault_before = env.token_amount(env.vault);
            let group_before = env.market_state().1;
            env.svm
                .simulate_transaction(untouched.clone().into())
                .expect("untouched retained grant is initially executable");
            assert_eq!(snapshot(&env), before);

            let cure = |env: &V16CuEnv, amount| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(lp_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(lp, false),
                    AccountMeta::new(cure_source, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::CureAndCancelClose {
                    portfolio_id: identities[0],
                    position_epoch: epoch,
                    optional_deposit: amount,
                }
                .encode(),
            };
            let failed_cure = Transaction::new_signed_with_payer(
                &[heap_ix(), cu_ix(), cure(&env, 0)],
                Some(&env.payer.pubkey()),
                &[&env.payer, &lp_owner],
                blockhash,
            );
            let rejected = env
                .svm
                .send_transaction(failed_cure)
                .expect_err("an unfunded cure cannot cancel the public residual");
            assert_eq!(
                rejected.err,
                solana_sdk::transaction::TransactionError::InstructionError(
                    2,
                    solana_sdk::instruction::InstructionError::Custom(
                        PercolatorError::EngineInvalidConfig as u32
                    ),
                )
            );
            assert_eq!(
                snapshot(&env),
                before,
                "{label}: failed cure preserves grant and episode"
            );
            peak_cu = peak_cu.max(rejected.meta.compute_units_consumed);

            let funded_cure = Transaction::new_signed_with_payer(
                &[heap_ix(), cu_ix(), cure(&env, CURE)],
                Some(&env.payer.pubkey()),
                &[&env.payer, &lp_owner],
                blockhash,
            );
            let cured = env
                .svm
                .send_transaction(funded_cure)
                .expect("funded public close cancellation");
            peak_cu = peak_cu.max(cured.compute_units_consumed);
            let after_cure = env.portfolio_state(lp);
            assert!(close_progress(&after_cure).canceled);
            assert_eq!(
                after_cure.legs, closing.legs,
                "cure revokes without a position mutation"
            );
            assert_eq!(after_cure.capital.get(), closing.capital.get() + CURE);
            assert_eq!(env.token_amount(cure_source), 0);
            assert_eq!(env.token_amount(env.vault), vault_before + CURE as u64);
            assert_eq!(env.market_state().1.vault, group_before.vault + CURE);
            assert_eq!(env.market_state().1.c_tot, group_before.c_tot + CURE);
            assert_eq!(env.portfolio_position_epoch(lp), epoch + 1);
            assert_eq!(env.portfolio_position_epoch(taker), taker_epoch);
            assert_eq!(
                [lp, taker, peer].map(|key| env.portfolio_id(key)),
                identities
            );
            assert_eq!([0, 1].map(|asset| env.asset_market_id(asset)), generations);
            let current_grant = env.portfolio_matcher_config(lp);
            assert_eq!(current_grant.enabled(), 0);
            assert_eq!(current_grant.trade_fee_cap_bps(), CAP);
            assert_eq!(current_grant.matcher_program, grant.matcher_program);
            assert_eq!(current_grant.matcher_context, grant.matcher_context);
            assert_eq!(current_grant.matcher_delegate, grant.matcher_delegate);
            assert_eq!(env.portfolio_matcher_sequence(lp), GRANT_SEQUENCE);
            assert_eq!(env.portfolio_matcher_expiry(lp), 0);
            for (key, old) in keys.iter().zip(&before) {
                if ![env.market, lp, cure_source, env.vault].contains(key) {
                    assert_eq!(&env.svm.get_account(key), old, "{label}: cure scope {key}");
                }
            }
            assert_eq!(env.svm.latest_blockhash(), blockhash);
            assert_eq!(env.svm.get_sysvar::<Clock>().slot, slot);

            let cured_frame = snapshot(&env);
            env.svm
                .simulate_transaction(untouched.clone().into())
                .expect("the exact untouched retained grant survives cure");
            assert_eq!(snapshot(&env), cured_frame);
            let mut current_ix = retained_ix.clone();
            match &mut current_ix {
                ProgInstruction::TradeCpi {
                    account_b_position_epoch,
                    ..
                }
                | ProgInstruction::BatchTradeCpi {
                    account_b_position_epoch,
                    ..
                } => {
                    *account_b_position_epoch = epoch + 1;
                }
                _ => unreachable!(),
            }
            let current = sign_trade(&env, &current_ix, lp, context, delegate);
            for (request, error) in [
                (retained, PercolatorError::EngineStale),
                (current, PercolatorError::Unauthorized),
            ] {
                let rejected = env
                    .svm
                    .send_transaction(request)
                    .expect_err("cure consumed old authority");
                assert_eq!(
                    rejected.err,
                    solana_sdk::transaction::TransactionError::InstructionError(
                        2,
                        solana_sdk::instruction::InstructionError::Custom(error as u32),
                    )
                );
                assert!(rejected
                    .meta
                    .logs
                    .iter()
                    .all(|line| !line.starts_with(&format!("Program {matcher_program} invoke"))));
                assert_eq!(
                    snapshot(&env),
                    cured_frame,
                    "{label}: exact consumer rollback"
                );
                peak_cu = peak_cu.max(rejected.meta.compute_units_consumed);
            }

            env.try_set_matcher_config_with_trade_fee_cap_and_expiry(
                matcher_program,
                &lp_owner,
                lp,
                context,
                delegate,
                1,
                CAP,
                EXPIRY,
            )
            .expect("explicit fresh owner authorization after cure");
            assert_eq!(env.portfolio_position_epoch(lp), epoch + 1);
            assert_eq!(env.portfolio_matcher_sequence(lp), GRANT_SEQUENCE + 1);
            match &mut current_ix {
                ProgInstruction::TradeCpi {
                    account_b_matcher_sequence,
                    ..
                }
                | ProgInstruction::BatchTradeCpi {
                    account_b_matcher_sequence,
                    ..
                } => {
                    *account_b_matcher_sequence = GRANT_SEQUENCE + 1;
                }
                _ => unreachable!(),
            }
            let fresh = sign_trade(&env, &current_ix, lp, context, delegate);
            let live = env
                .svm
                .send_transaction(fresh)
                .expect("same fill under the new grant must land");
            assert!(live
                .logs
                .iter()
                .any(|line| line == &format!("Program {matcher_program} success")));
            peak_cu = peak_cu.max(live.compute_units_consumed);
            assert_eq!(
                active_leg_for_asset(&env.portfolio_state(taker), 1).basis_pos_q,
                size
            );
            assert_eq!(
                active_leg_for_asset(&env.portfolio_state(lp), 1).basis_pos_q,
                -size
            );
            assert_eq!(env.portfolio_matcher_config(lp).enabled(), 1);
            assert_eq!(env.portfolio_matcher_expiry(lp), EXPIRY);
            assert_eq!(env.portfolio_matcher_sequence(lp), GRANT_SEQUENCE + 1);
            assert_eq!(env.portfolio_position_epoch(lp), epoch + 2);
            assert_eq!(env.market_state().1.assets[1].oi_eff_long_q, POS_SCALE);
            assert_eq!(env.market_state().1.assets[1].oi_eff_short_q, POS_SCALE);
            assert_eq!(env.svm.get_account(&peer), before[3]);
            assert_eq!(env.svm.get_account(&peer_context), before[8]);
            assert_cu_within("funded cure capability history", peak_cu, 500_000);
        }
    }
    println!("INV-012 funded cure revocation: 4 worlds, 8 consumer rollbacks, 4 fresh fills; peak {peak_cu} CU");
}
