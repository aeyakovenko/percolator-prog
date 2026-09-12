//! INV-067: receipt completion composes with final booked-residue burn and raw-stock sweep.
//! Existing receipt words end at portfolio deletion; this public suffix reaches CloseSlab.

use super::{late_expiry::World, *};
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
const RESIDUAL: u128 = 501 + 100 + 250;
const SUPPLY: u128 = 1_000 + 500 + 1_000 + 250 + 1_000 + 102;

fn entitlements() -> [u128; 5] {
    let total_face: u128 = FACES.iter().sum();
    std::array::from_fn(|actor| CAPITAL[actor] + FACES[actor] * RESIDUAL / total_face)
}

fn check_claims(world: &World, previous: [u64; 5]) -> [u64; 5] {
    world.custody();
    let limits = entitlements();
    std::array::from_fn(|actor| {
        let tokens = world.env.token_amount(world.actors[actor].token);
        let portfolio = world.env.portfolio_state(world.actors[actor].portfolio);
        assert!(tokens >= previous[actor], "owner payout must be monotone");
        assert!(u128::from(tokens) + portfolio.capital.get() <= limits[actor]);
        let receipt = world.receipt(actor);
        if receipt.present {
            assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
            assert_eq!(
                receipt.prior_bound_contribution_num,
                FACES[actor] * BOUND_SCALE
            );
            assert_eq!(receipt.live_released_face_at_receipt, 0);
            assert_eq!(u128::from(tokens), CAPITAL[actor] + receipt.paid_effective);
        }
        tokens
    })
}

#[test]
fn v16_program_receipt_terminal_suffix_partitions_rounding_burn_surplus_and_rent() {
    let expected = entitlements();
    let total_paid: u128 = expected.iter().sum();
    let rounding = RESIDUAL - (total_paid - CAPITAL.iter().sum::<u128>());
    assert_eq!(
        rounding, 2,
        "three independent receipt floors leave two atoms"
    );
    assert_eq!(SUPPLY, total_paid + rounding + 1);
    let mut peak_cu = 0;
    let mut close_peak_cu = 0;
    let mut slab_calls = 0;
    for eager in [false, true] {
        for reverse in [false, true] {
            for surplus in [0u64, 1] {
                let mut world = World::before_receipts();
                for actor in [0, 4] {
                    for _ in 0..8 {
                        if world.receipt(actor).present {
                            break;
                        }
                        world.land(&[world.payout(actor, false)], false).unwrap();
                    }
                    let receipt = world.receipt(actor);
                    assert!(receipt.present && !receipt.finalized);
                    assert_eq!(receipt.paid_effective, FACES[actor] * 501 / 3_000);
                }
                world.peak_cu = 0;
                let initial_frame = world.frame();
                let economic_lamports = |world: &World| -> u64 {
                    world
                        .frame()
                        .iter()
                        .filter_map(|(_, account)| account.as_ref())
                        .map(|account| account.lamports)
                        .sum()
                };
                let initial_lamports = economic_lamports(&world);
                let mut tokens = check_claims(&world, [0; 5]);
                world.env.svm.warp_to_slot(13);
                world.land(&[world.payout(2, false)], false).unwrap();
                let ledger = world.env.market_state().1.resolved_payout_ledger;
                assert_eq!(ledger.snapshot_slot, 12);
                assert_eq!(ledger.snapshot_residual, RESIDUAL);
                assert_eq!(ledger.current_payout_rate_num, RESIDUAL * BOUND_SCALE);
                assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
                tokens = check_claims(&world, tokens);
                let mut order = [0, 4, 2, 1, 3];
                if reverse {
                    order.reverse();
                }
                if eager {
                    for actor in order.into_iter().filter(|actor| *actor == 0 || *actor == 4) {
                        world.land(&[world.payout(actor, true)], false).unwrap();
                        tokens = check_claims(&world, tokens);
                        assert_eq!(u128::from(tokens[actor]), expected[actor]);
                    }
                }

                // Guaranteed public continuations complete deferred receipts and source cleanup.
                for sweep in 0..16 {
                    for actor in order {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
                        {
                            continue;
                        }
                        let before = world.frame();
                        let mut payout = world.payout(actor, false);
                        if sweep % 2 == 1 {
                            payout.data = ProgInstruction::PermissionlessCrank {
                                now_slot: 13,
                                observations: vec![],
                            }
                            .encode();
                        }
                        match world.land(&[payout], false) {
                            Ok(meta) => {
                                if world.frame() == before {
                                    // CloseResolved may replay a paid receipt while peers still
                                    // need cleanup; the bounded drain cannot stop at this no-op.
                                    assert_eq!(sweep % 2, 0);
                                    assert!(world.receipt(actor).present);
                                    assert_eq!(u128::from(tokens[actor]), expected[actor]);
                                    assert!(!meta.logs.iter().any(|line| *line
                                        == format!("Program {} success", spl_token::ID)));
                                }
                                world.assert_frame_except(
                                    &before,
                                    &[
                                        world.env.market,
                                        world.env.vault,
                                        world.actors[actor].portfolio,
                                        world.actors[actor].token,
                                    ],
                                );
                            }
                            Err(failure) => {
                                assert_eq!(
                                    failure.err,
                                    TransactionError::InstructionError(
                                        2,
                                        InstructionError::Custom(
                                            PercolatorError::EngineNonProgress as u32
                                        ),
                                    )
                                );
                                assert_eq!(world.frame(), before);
                            }
                        }
                        tokens = check_claims(&world, tokens);
                    }
                    if world
                        .actors
                        .iter()
                        .all(|actor| resolved_portfolio_is_terminal(&world.env, actor.portfolio))
                    {
                        break;
                    }
                }
                assert_eq!(tokens.map(u128::from), expected);
                for actor in order {
                    let portfolio = world.actors[actor].portfolio;
                    assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
                    let before = world.frame();
                    world.land(&[world.payout(actor, true)], false).unwrap();
                    assert_eq!(
                        world.frame(),
                        before,
                        "terminal receipt retry is exactly inert"
                    );
                    let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                    let market_before = world.env.svm.get_account(&world.env.market).unwrap();
                    let mut group = world.env.market_state().1;
                    let cu = world
                        .env
                        .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                    peak_cu = peak_cu.max(cu);
                    assert_cu_within("receipt suffix portfolio close", cu, CUSTODY_CU_LIMIT);
                    group.materialized_portfolio_count -= 1;
                    assert_eq!(world.env.market_state().1, group);
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports,
                        market_before.lamports + rent
                    );
                    assert!(world
                        .env
                        .svm
                        .get_account(&portfolio)
                        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                    world.assert_frame_except(&before, &[world.env.market, portfolio]);
                    world.custody();
                }
                let group = world.env.market_state().1;
                assert_eq!(
                    [
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.source_claim_bound_total_num,
                        group.insurance
                    ],
                    [0; 4]
                );
                assert_eq!(group.materialized_portfolio_count, 0);
                assert_eq!(group.vault, rounding);
                assert_eq!(world.env.token_amount(world.provider_token), 1);
                assert_eq!(economic_lamports(&world), initial_lamports);

                // The provider's one never-deposited token is raw surplus, not receipt backing.
                if surplus != 0 {
                    let before = world.frame();
                    let donation = spl_token::instruction::transfer(
                        &spl_token::ID,
                        &world.provider_token,
                        &world.env.vault,
                        &world.env.admin.pubkey(),
                        &[],
                        surplus,
                    )
                    .unwrap();
                    world.land(&[donation], true).unwrap();
                    world.assert_frame_except(&before, &[world.provider_token, world.env.vault]);
                    assert_eq!(world.env.market_state().1, group);
                }
                assert_eq!(
                    world.env.token_amount(world.env.vault) as u128,
                    rounding + u128::from(surplus)
                );
                assert_eq!(world.env.token_amount(world.provider_token), 1 - surplus);
                let before_close = world.frame();
                let market_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let vault_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.vault)
                    .unwrap()
                    .lamports;
                let admin_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.admin.pubkey())
                    .unwrap()
                    .lamports;
                let rent = world
                    .env
                    .svm
                    .get_sysvar::<solana_sdk::rent::Rent>()
                    .minimum_balance(percolator_prog::constants::HEADER_LEN);
                let close = Instruction {
                    program_id: world.env.program_id,
                    accounts: vec![
                        AccountMeta::new(world.env.admin.pubkey(), true),
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(world.env.vault, false),
                        AccountMeta::new_readonly(world.env.vault_authority, false),
                        AccountMeta::new(world.provider_token, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(world.env.mint, false),
                    ],
                    data: ProgInstruction::CloseSlab {
                        authority_epoch: world.env.control_sequences(0).authority_epoch,
                    }
                    .encode(),
                };
                let mut closed = false;
                for _ in 0..8 {
                    let before = world.frame();
                    let meta = world
                        .land(&[close.clone()], true)
                        .expect("receipt-complete terminal disposition");
                    slab_calls += 1;
                    close_peak_cu = close_peak_cu.max(meta.compute_units_consumed);
                    assert_ne!(
                        world.frame(),
                        before,
                        "bounded slab continuation must progress"
                    );
                    let market = world.env.svm.get_account(&world.env.market).unwrap();
                    if market.data.len() == percolator_prog::constants::HEADER_LEN {
                        assert_closed_market_tombstone(&market);
                        closed = true;
                        break;
                    }
                    world.assert_frame_except(&before, &[world.env.market]);
                    assert_eq!(world.env.market_state().1.vault, rounding);
                }
                assert!(
                    closed,
                    "receipt drain must reach its tombstone within eight calls"
                );
                world.assert_frame_except(
                    &before_close,
                    &[
                        world.env.market,
                        world.env.vault,
                        world.env.mint,
                        world.provider_token,
                        world.env.admin.pubkey(),
                    ],
                );
                assert!(world
                    .env
                    .svm
                    .get_account(&world.env.vault)
                    .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    rent
                );
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.admin.pubkey())
                        .unwrap()
                        .lamports,
                    admin_lamports + market_lamports + vault_lamports - rent
                );
                assert_eq!(economic_lamports(&world), initial_lamports);
                assert_eq!(
                    world.env.token_amount(world.provider_token),
                    1,
                    "only raw surplus returns to the authority, never receipt rounding"
                );
                let mut expected_mint = initial_frame
                    .iter()
                    .find(|(key, _)| *key == world.env.mint)
                    .unwrap()
                    .1
                    .clone()
                    .unwrap();
                let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                assert_eq!(u128::from(mint.supply), SUPPLY);
                mint.supply -= rounding as u64;
                Mint::pack(mint, &mut expected_mint.data).unwrap();
                assert_eq!(
                    world.env.svm.get_account(&world.env.mint).unwrap(),
                    expected_mint
                );
                assert_eq!(u128::from(mint.supply), total_paid + 1);
                assert_eq!(
                    std::array::from_fn::<_, 5, _>(|actor| u128::from(
                        world.env.token_amount(world.actors[actor].token)
                    )),
                    expected
                );
                peak_cu = peak_cu.max(world.peak_cu);
                println!("INV-067 terminal suffix eager={eager} reverse={reverse} surplus={surplus}: {SUPPLY} = {total_paid} user + {rounding} burn + 1 provider; exact rent");
            }
        }
    }
    assert_cu_within("receipt terminal suffix", peak_cu, 200_000);
    println!("INV-067 terminal disposition: 8 worlds, 40 portfolio closes, {slab_calls} slab calls; peak suffix CU {peak_cu}, peak CloseSlab CU {close_peak_cu}");
}
