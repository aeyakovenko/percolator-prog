//! INV-039/037/067/076: cancellation removes a close barrier, not the debtor's debt.
//! Existing cure witnesses stop at live obligation release. This fixed-supply public
//! matrix carries the canceled residual through immediate resolution or live release,
//! then reconciles both terminal orders against input-derived owner entitlements.

use super::*;
use close_preemption::terminal_instruction;
use solana_sdk::fee::FeeStructure;

const CURE: u128 = 100_000;

fn land(
    world: &mut AttributionWorld,
    instructions: &[Instruction],
    signers: &[&Keypair],
    allowed: &[Pubkey],
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    let mut ixs = vec![heap_ix(), cu_ix()];
    ixs.extend_from_slice(instructions);
    world.env.svm.expire_blockhash();
    let mut all_signers = vec![&world.env.payer];
    all_signers.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&world.env.payer.pubkey()),
        &all_signers,
        world.env.svm.latest_blockhash(),
    );
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
    let result = world.env.svm.send_transaction(tx);
    let rejected = rejection.is_some();
    let meta = if let Some((index, error)) = rejection {
        let failure = result.expect_err("the rejected suffix restores its economic prefix");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| { **line == format!("Program {} success", world.env.program_id) })
                .count(),
            usize::from(index - 2),
            "every preceding wrapper instruction completed"
        );
        if index > 2 {
            assert!(failure
                .meta
                .logs
                .contains(&format!("Program {} success", spl_token::ID)));
        }
        failure.meta
    } else {
        result.expect("valid cure or terminal continuation")
    };
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        if rejected || !allowed.contains(&key) {
            assert_eq!(
                world.env.svm.get_account(&key),
                expected,
                "complete Account {key}"
            );
        }
    }
    assert_cu_within(
        "INV-039 cure/resolution transaction",
        meta.compute_units_consumed,
        600_000,
    );
    meta.compute_units_consumed
}

fn setup(reverse: bool, peak: &mut u64) -> (AttributionWorld, u128) {
    let mut world = AttributionWorld::new_with_params(
        reverse,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: 1_000_000,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_bankrupt_close_lifetime_slots: 1_000,
            ..V16CuMarketParams::default()
        },
    );
    // A consenting third owner supplies the cure from existing principal before the
    // close starts. Revoked mint authority makes every later stock check fixed-supply.
    let donor = &world.actors[2];
    let debtor = &world.actors[1];
    let withdraw = Instruction {
        program_id: world.env.program_id,
        data: world.env.withdraw_ix(donor.portfolio, CURE).encode(),
        accounts: vec![
            AccountMeta::new(donor.owner.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(donor.portfolio, false),
            AccountMeta::new(donor.token, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    };
    let transfer = spl_token::instruction::transfer(
        &spl_token::ID,
        &donor.token,
        &debtor.token,
        &donor.owner.pubkey(),
        &[],
        CURE as u64,
    )
    .unwrap();
    let owner = donor.owner.insecure_clone();
    let allowed = [
        world.env.market,
        world.env.vault,
        donor.portfolio,
        donor.token,
        debtor.token,
    ];
    *peak = (*peak).max(land(
        &mut world,
        &[withdraw, transfer],
        &[&owner],
        &allowed,
        None,
    ));
    send_raw_tx(
        &mut world.env.svm,
        &world.env.payer,
        spl_token::instruction::set_authority(
            &spl_token::ID,
            &world.env.mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &world.env.admin.pubkey(),
            &[],
        )
        .unwrap(),
        &[&world.env.admin],
    )
    .unwrap();
    world.check([0; 4], [false; 4]);
    let sign = if reverse { -1i64 } else { 1 };
    let marks = (1..=5).map(|slot| (1_000_000 + sign * 40_000 * slot) as u64);
    enter_pending_bankruptcy(world, marks, peak)
}

fn assert_entitlements(world: &AttributionWorld, expected: [u128; 5]) {
    let mut pending = [false; 4];
    for (i, actor) in world.actors.iter().enumerate() {
        let account = world.env.portfolio_state(actor.portfolio);
        let receipt = resolved_receipt(&account);
        let due = if receipt.present {
            receipt
                .terminal_positive_claim_face
                .checked_sub(receipt.paid_effective)
                .unwrap()
        } else {
            0
        };
        let equity = account.capital.get() as i128
            + account.pnl.get()
            + world.env.token_amount(actor.token) as i128
            + due as i128;
        assert_eq!(
            equity, expected[i] as i128,
            "actor {i}: exact original entitlement; capital={}, pnl={}, token={}, receipt={receipt:?}",
            account.capital.get(), account.pnl.get(), world.env.token_amount(actor.token)
        );
        assert!(world.env.token_amount(actor.token) as u128 <= expected[i]);
        if i < 4 {
            pending[i] = has_active_leg_for_asset(&account, 1);
        }
    }
    assert!(!pending[1..].iter().any(|value| *value));
    world.check([0; 4], pending);
    let group = world.env.market_state().1;
    assert_eq!(group.insurance, 0);
    assert_eq!(
        [group.assets[1].b_long_num, group.assets[1].b_short_num],
        [0; 2]
    );
}

#[test]
fn v16_program_canceled_close_keeps_debt_through_pending_release_and_resolution() {
    let mut peak_setup = 0;
    let mut peak_cure = 0;
    let mut peak_terminal = 0;
    let mut prefix_rollbacks = 0;
    let mut payouts = 0;
    for reverse in [false, true] {
        for live_release in [false, true] {
            for order in [[0usize, 1, 2, 3, 4], [1, 0, 4, 3, 2]] {
                let (mut world, gain) = setup(reverse, &mut peak_setup);
                let residual = gain - ATTRIBUTION_DEPOSITS[1];
                assert_eq!((gain, residual), (200_000, 20_000));
                let expected = [
                    ATTRIBUTION_DEPOSITS[0] + gain,
                    CURE - residual,
                    ATTRIBUTION_DEPOSITS[2] - CURE,
                    ATTRIBUTION_DEPOSITS[3],
                    ATTRIBUTION_DEPOSITS[4],
                ];
                let debtor = world.actors[1].portfolio;
                let holder = world.actors[0].portfolio;
                let initial_close = close_progress(&world.env.portfolio_state(debtor));
                assert!(
                    initial_close.active && !initial_close.canceled && !initial_close.finalized
                );
                assert_close_partition(initial_close, residual);
                world.check([0; 4], [true, false, false, false]);
                let holder_before = world.env.svm.get_account(&holder);
                let retained_leg = active_leg_for_asset(&world.env.portfolio_state(holder), 1);
                let cure = Instruction {
                    program_id: world.env.program_id,
                    data: ProgInstruction::CureAndCancelClose {
                        portfolio_id: world.env.portfolio_id(debtor),
                        position_epoch: world.env.portfolio_position_epoch(debtor),
                        optional_deposit: CURE,
                    }
                    .encode(),
                    accounts: vec![
                        AccountMeta::new(world.actors[1].owner.pubkey(), true),
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(debtor, false),
                        AccountMeta::new(world.actors[1].token, false),
                        AccountMeta::new(world.env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                };
                let owner = world.actors[1].owner.insecure_clone();
                let mut suffix = close_instruction(&world, 0);
                suffix.accounts[0].is_signer = false;
                peak_cure = peak_cure.max(land(
                    &mut world,
                    &[cure.clone(), suffix.clone()],
                    &[&owner],
                    &[],
                    Some((3, PercolatorError::ExpectedSigner)),
                ));
                prefix_rollbacks += 1;
                assert_eq!(
                    close_progress(&world.env.portfolio_state(debtor)),
                    initial_close
                );
                let allowed = [
                    world.env.market,
                    world.env.vault,
                    debtor,
                    world.actors[1].token,
                ];
                peak_cure = peak_cure.max(land(&mut world, &[cure], &[&owner], &allowed, None));
                let mut canceled = initial_close;
                canceled.active = false;
                canceled.canceled = true;
                let cured = world.env.portfolio_state(debtor);
                assert_eq!(close_progress(&cured), canceled);
                assert_close_partition(canceled, residual);
                assert_eq!(
                    (cured.capital.get(), cured.pnl.get()),
                    (CURE, -(residual as i128))
                );
                assert_eq!(world.env.token_amount(world.actors[1].token), 0);
                assert_eq!(world.env.svm.get_account(&holder), holder_before);
                assert_eq!(
                    active_leg_for_asset(&world.env.portfolio_state(holder), 1),
                    retained_leg
                );
                world.check([0; 4], [true, false, false, false]);
                assert_entitlements(&world, expected);

                if live_release {
                    let cu = world.env.crank(
                        holder,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 5,
                            observations: crank_observations(1),
                        },
                    );
                    peak_terminal = peak_terminal.max(cu);
                    assert_cu_within("INV-039 canceled-close live release", cu, CRANK_CU_LIMIT);
                    world.check([0; 4], [false; 4]);
                    assert_entitlements(&world, expected);
                    assert_eq!(
                        world.env.portfolio_state(debtor).pnl.get(),
                        -(residual as i128)
                    );
                }
                let before_resolve = world.frame();
                peak_terminal = peak_terminal.max(world.env.resolve());
                for (key, account) in before_resolve {
                    if key != world.env.market {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                assert_eq!(close_progress(&world.env.portfolio_state(debtor)), canceled);
                assert_entitlements(&world, expected);
                world.env.svm.warp_to_slot(10);

                for _ in 0..2 {
                    for actor in order {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
                        {
                            continue;
                        }
                        let payout = terminal_instruction(&world, actor);
                        let allowed = [
                            world.env.market,
                            world.env.vault,
                            world.actors[actor].portfolio,
                            world.actors[actor].token,
                        ];
                        let before = world.frame();
                        let debtor_paid = resolved_portfolio_is_terminal(&world.env, debtor);
                        if actor == 1 || (actor == 0 && debtor_paid) {
                            peak_terminal = peak_terminal.max(land(
                                &mut world,
                                &[payout.clone(), suffix.clone()],
                                &[],
                                &[],
                                Some((3, PercolatorError::ExpectedSigner)),
                            ));
                            prefix_rollbacks += 1;
                        }
                        let paid_before = world.env.token_amount(world.actors[actor].token);
                        peak_terminal =
                            peak_terminal.max(land(&mut world, &[payout], &[], &allowed, None));
                        payouts += usize::from(
                            world.env.token_amount(world.actors[actor].token) > paid_before,
                        );
                        assert_ne!(
                            world.frame(),
                            before,
                            "successful terminal call must progress"
                        );
                        if actor == 0 && !debtor_paid {
                            assert_eq!(world.env.token_amount(world.actors[actor].token), 0);
                            assert!(!resolved_portfolio_is_terminal(&world.env, holder));
                        }
                        assert_eq!(close_progress(&world.env.portfolio_state(debtor)), canceled);
                        assert_entitlements(&world, expected);
                    }
                    if world
                        .actors
                        .iter()
                        .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                    {
                        break;
                    }
                }
                for (i, entitlement) in expected.into_iter().enumerate() {
                    assert_eq!(
                        world.env.token_amount(world.actors[i].token) as u128,
                        entitlement
                    );
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[i].portfolio
                    ));
                }
                world.check([0; 4], [false; 4]);
                assert_eq!(world.env.market_state().1.vault, 0);
                for actor in &world.actors {
                    let cu = world
                        .env
                        .close_portfolio_with_cu(&actor.owner, actor.portfolio);
                    peak_terminal = peak_terminal.max(cu);
                    assert_cu_within("INV-039 cured cohort deletion", cu, CUSTODY_CU_LIMIT);
                }
                assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
            }
        }
    }
    assert_eq!((prefix_rollbacks, payouts), (24, 40));
    println!("INV-039 cure/resolution: 8 worlds, {prefix_rollbacks} prefix rollbacks, {payouts} exact payouts, 40 portfolio deletions; peak CU setup={peak_setup}, cure/rollback={peak_cure}, terminal/rollback={peak_terminal}");
}
