//! INV-076/018/080: failed cure custody must preserve the close and its retry epoch.
//! Reuse the public fixed-supply close fixture and carry every claim through payout.

use super::*;

#[test]
fn v16_program_cure_token_cpi_failure_preserves_close_epoch_and_terminal_payouts() {
    let mut peak_setup = 0;
    let mut peak_cure = 0;
    let mut peak_terminal = 0;
    let mut rollbacks = 0;
    let mut payouts = 0;
    for reverse in [false, true] {
        for allowance in [0, CURE as u64 - 1] {
            let (mut world, gain) = setup(reverse, &mut peak_setup);
            let debtor = world.actors[1].portfolio;
            let source = world.actors[1].token;
            let owner = world.actors[1].owner.insecure_clone();
            let residual = gain - ATTRIBUTION_DEPOSITS[1];
            assert_eq!((gain, residual), (200_000, 20_000));
            let expected = [
                ATTRIBUTION_DEPOSITS[0] + gain,
                CURE - residual,
                ATTRIBUTION_DEPOSITS[2] - CURE,
                ATTRIBUTION_DEPOSITS[3],
                ATTRIBUTION_DEPOSITS[4],
            ];
            let close = close_progress(&world.env.portfolio_state(debtor));
            assert!(close.active && !close.canceled && !close.finalized);
            assert_eq!(close.residual_remaining, residual);
            assert_close_partition(close, residual);
            let epoch = world.env.portfolio_position_epoch(debtor);
            let cure = Instruction {
                program_id: world.env.program_id,
                accounts: vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(debtor, false),
                    AccountMeta::new(source, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::CureAndCancelClose {
                    portfolio_id: world.env.portfolio_id(debtor),
                    position_epoch: epoch,
                    optional_deposit: CURE,
                }
                .encode(),
            };
            let sign = |env: &V16CuEnv| {
                Transaction::new_signed_with_payer(
                    &[heap_ix(), cu_ix(), cure.clone()],
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &owner],
                    env.svm.latest_blockhash(),
                )
            };
            let before = world.frame();
            world
                .env
                .svm
                .simulate_transaction(sign(&world.env).into())
                .expect("the captured cure is solvent and executable before SPL approval");
            assert_eq!(world.frame(), before);

            // Balance validation still passes. SPL selects the self-delegate allowance
            // after the engine has canceled the close and advanced the position epoch.
            send_raw_tx(
                &mut world.env.svm,
                &world.env.payer,
                spl_token::instruction::approve(
                    &spl_token::ID,
                    &source,
                    &owner.pubkey(),
                    &owner.pubkey(),
                    &[],
                    allowance,
                )
                .unwrap(),
                &[&owner],
            )
            .unwrap();
            let token =
                TokenAccount::unpack(&world.env.svm.get_account(&source).unwrap().data).unwrap();
            assert_eq!(token.amount, CURE as u64);
            assert_eq!(token.delegate, COption::Some(owner.pubkey()));
            assert_eq!(token.delegated_amount, allowance);

            world.env.svm.expire_blockhash();
            let tx = sign(&world.env);
            tx.verify().unwrap();
            assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
            let fee = u64::from(tx.message.header.num_required_signatures)
                * FeeStructure::default().lamports_per_signature;
            let mut keys = tx.message.account_keys.clone();
            keys.extend(world.frame().into_iter().map(|(key, _)| key));
            keys.sort_unstable();
            keys.dedup();
            let before: Vec<_> = keys
                .iter()
                .map(|key| world.env.svm.get_account(key))
                .collect();
            let failure = world
                .env
                .svm
                .send_transaction(tx)
                .expect_err("cure must propagate its token CPI failure");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32
                    ),
                )
            );
            assert!(failure
                .meta
                .logs
                .contains(&format!("Program {} invoke [2]", spl_token::ID)));
            assert!(failure
                .meta
                .logs
                .iter()
                .any(|line| line == "Program log: Instruction: Transfer"));
            assert_cu_within(
                "INV-076 failed cure token CPI",
                failure.meta.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
            peak_cure = peak_cure.max(failure.meta.compute_units_consumed);
            for (key, mut account) in keys.into_iter().zip(before) {
                if key == world.env.payer.pubkey() {
                    account.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(world.env.svm.get_account(&key), account, "rollback {key}");
            }
            rollbacks += 1;
            assert_eq!(world.env.portfolio_position_epoch(debtor), epoch);
            assert_eq!(close_progress(&world.env.portfolio_state(debtor)), close);
            world.check([0; 4], [true, false, false, false]);

            send_raw_tx(
                &mut world.env.svm,
                &world.env.payer,
                spl_token::instruction::revoke(&spl_token::ID, &source, &owner.pubkey(), &[])
                    .unwrap(),
                &[&owner],
            )
            .unwrap();
            let vault_before = world.env.token_amount(world.env.vault);
            let allowed = [world.env.market, world.env.vault, debtor, source];
            peak_cure = peak_cure.max(land(
                &mut world,
                std::slice::from_ref(&cure),
                &[&owner],
                &allowed,
                None,
            ));
            let mut canceled = close;
            canceled.active = false;
            canceled.canceled = true;
            let cured = world.env.portfolio_state(debtor);
            assert_eq!(close_progress(&cured), canceled);
            assert_eq!(
                (cured.capital.get(), cured.pnl.get()),
                (CURE, -(residual as i128))
            );
            assert_eq!(world.env.portfolio_position_epoch(debtor), epoch + 1);
            assert_eq!(world.env.token_amount(source), 0);
            assert_eq!(
                world.env.token_amount(world.env.vault),
                vault_before + CURE as u64
            );
            assert_entitlements(&world, expected);

            // Reusing the captured episode fails even before the now-empty source is checked.
            peak_cure = peak_cure.max(land(
                &mut world,
                &[cure],
                &[&owner],
                &[],
                Some((2, PercolatorError::EngineProvenanceMismatch)),
            ));
            rollbacks += 1;
            assert_eq!(world.env.portfolio_position_epoch(debtor), epoch + 1);

            peak_terminal = peak_terminal.max(world.env.resolve());
            world.env.svm.warp_to_slot(10);
            for actor in [1, 0, 2, 3, 4] {
                let payout = terminal_instruction(&world, actor);
                let allowed = [
                    world.env.market,
                    world.env.vault,
                    world.actors[actor].portfolio,
                    world.actors[actor].token,
                ];
                peak_terminal = peak_terminal.max(land(&mut world, &[payout], &[], &allowed, None));
                assert_eq!(
                    world.env.token_amount(world.actors[actor].token) as u128,
                    expected[actor]
                );
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                assert_eq!(close_progress(&world.env.portfolio_state(debtor)), canceled);
                assert_entitlements(&world, expected);
                payouts += 1;
            }
            assert_eq!(world.env.market_state().1.vault, 0);
            assert_eq!(world.env.token_amount(world.env.vault), 0);
        }
    }
    assert_eq!((rollbacks, payouts), (8, 20));
    assert_cu_within("INV-076 cure and retry", peak_cure, CUSTODY_CU_LIMIT);
    assert_cu_within("INV-076 terminal payouts", peak_terminal, CUSTODY_CU_LIMIT);
    println!("INV-076 cure CPI retry: 4 worlds, 4 SPL failures, 4 unchanged-request cures, 4 stale replays, {payouts} exact unsigned payouts; peak CU setup={peak_setup}, cure={peak_cure}, terminal={peak_terminal}");
}
