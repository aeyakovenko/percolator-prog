//! Row 417: new lifetimes of funded canonical vaults and payout ATAs cannot
//! revive settled receipt requests after the containing market is tombstoned.

use super::*;

const SECONDARY_REFILL: u64 = 4_096;
const PRIMARY_REFILL: u64 = 1_116 + 1_217;

fn create_custody(world: &World, wallet: Pubkey, mint: Pubkey, prefund: bool) -> Vec<Instruction> {
    let sponsor = world.actors[1].owner.pubkey();
    let ata = canonical_vault_ata(wallet, mint);
    let mut instructions = Vec::new();
    if prefund {
        instructions.push(system_instruction::transfer(&sponsor, &ata, 17));
    }
    instructions.push(Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(sponsor, true),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(wallet, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
        ],
        data: vec![],
    });
    instructions
}

fn recreate_and_fund(
    world: &World,
    rail: &Rail,
    actor: usize,
    secondary: bool,
    prefund: bool,
) -> Vec<Instruction> {
    let (mint, vault) = if secondary {
        (rail.mint, rail.vault)
    } else {
        (world.env.mint, world.env.vault)
    };
    let mut instructions = create_custody(world, world.env.vault_authority, mint, prefund);
    instructions.extend(create_custody(
        world,
        world.actors[actor].owner.pubkey(),
        mint,
        prefund,
    ));
    if secondary && rail.native {
        instructions.push(system_instruction::transfer(
            &world.actors[1].owner.pubkey(),
            &vault,
            SECONDARY_REFILL,
        ));
        instructions.push(spl_token::instruction::sync_native(&spl_token::ID, &vault).unwrap());
    } else if secondary {
        instructions.push(
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &mint,
                &vault,
                &world.env.admin.pubkey(),
                &[],
                SECONDARY_REFILL,
            )
            .unwrap(),
        );
    } else {
        instructions.push(
            spl_token::instruction::transfer(
                &spl_token::ID,
                &world.actors[1].token,
                &vault,
                &world.actors[1].owner.pubkey(),
                &[],
                PRIMARY_REFILL,
            )
            .unwrap(),
        );
    }
    instructions
}

fn check_custody(world: &World, rail: &Rail, tombstone: &Account) {
    assert_eq!(
        world.env.svm.get_account(&world.env.market),
        Some(tombstone.clone())
    );
    for actor in &world.actors {
        let account = world.env.svm.get_account(&actor.portfolio).unwrap();
        assert_eq!(account.lamports, 0);
        assert!(
            account.data.is_empty(),
            "custody cannot reconstruct a portfolio"
        );
    }
    for (mint, vault, source, destinations, expected_supply, refill) in [
        (
            world.env.mint,
            world.env.vault,
            world.provider_token,
            world
                .actors
                .iter()
                .map(|actor| actor.token)
                .collect::<Vec<_>>(),
            PRIMARY_SUPPLY as u64 - 2,
            PRIMARY_REFILL,
        ),
        (
            rail.mint,
            rail.vault,
            rail.source,
            rail.destinations.to_vec(),
            if rail.native {
                0
            } else {
                SECONDARY_SUPPLY + SECONDARY_REFILL
            },
            SECONDARY_REFILL,
        ),
    ] {
        let native = mint == spl_token::native_mint::ID;
        assert_eq!(world.env.token_amount(vault), refill);
        assert!(
            refill >= 1_368,
            "each rail can fund either old claimant in full"
        );
        let mut total = 0;
        for key in [vault, source].into_iter().chain(destinations) {
            let account = world.env.svm.get_account(&key).unwrap();
            let token = TokenAccount::unpack(&account.data).unwrap();
            assert_eq!(account.owner, spl_token::ID);
            assert_eq!(token.mint, mint);
            assert_eq!(token.state, AccountState::Initialized);
            assert_eq!(token.delegate, COption::None);
            assert_eq!(token.close_authority, COption::None);
            if key == vault {
                assert_eq!(token.owner, world.env.vault_authority);
            }
            if native {
                let COption::Some(rent) = token.is_native else {
                    panic!("native reserve")
                };
                assert_eq!(account.lamports, rent + token.amount);
            } else {
                assert_eq!(token.is_native, COption::None);
            }
            total += token.amount;
        }
        assert_eq!(
            total,
            if native {
                SECONDARY_SUPPLY + SECONDARY_REFILL
            } else {
                expected_supply
            }
        );
        assert_eq!(
            Mint::unpack(&world.env.svm.get_account(&mint).unwrap().data)
                .unwrap()
                .supply,
            expected_supply
        );
        for actor in [0, 4] {
            let key = canonical_vault_ata(world.actors[actor].owner.pubkey(), mint);
            let account = world.env.svm.get_account(&key).unwrap();
            let token = TokenAccount::unpack(&account.data).unwrap();
            assert_eq!(token.owner, world.actors[actor].owner.pubkey());
            assert_eq!(
                token.amount, 0,
                "replacement ATA acquired no receipt entitlement"
            );
        }
    }
    assert_eq!(world.env.token_amount(world.actors[2].token), 1_283);
    assert_eq!(world.env.token_amount(world.provider_token), 234);
    assert_eq!(world.env.token_amount(world.actors[1].token), 0);
    assert_eq!(
        world.env.token_amount(rail.destinations[1]),
        SECONDARY_SUPPLY
    );
}

#[test]
fn v16_program_funded_post_close_custody_cannot_revive_retained_receipts() {
    let mut evidence = Evidence::default();
    let mut peaks = [0; 2];
    let mut recreation_rollbacks = 0;
    for native in [false, true] {
        for prefund in [false, true] {
            let mut world = World::before_receipts_with_staggered_quote_rails(
                if native {
                    install_native_secondary
                } else {
                    install_secondary
                },
                if native {
                    spl_token::native_mint::DECIMALS
                } else {
                    0
                },
            );
            for actor in [0, 4] {
                for _ in 0..8 {
                    if world.receipt(actor).present {
                        break;
                    }
                    world.land(&[world.payout(actor, false)], false).unwrap();
                }
                assert!(world.receipt(actor).present);
            }
            let original = [world.receipt(0), world.receipt(4)];
            let mut rail = Rail::new(&mut world);
            rail.fund(&mut world, SECONDARY_SUPPLY);
            let retained = [false, true].map(|secondary| {
                [0, 4].map(|actor| {
                    [true, false].map(|claim| {
                        if secondary {
                            rail.payout(&world, actor, claim)
                        } else {
                            world.payout(actor, claim)
                        }
                    })
                })
            });
            let slab = close_slab(&world, &rail);
            let normalize = world.payout(2, false);
            let mut paid = [1_116, 0, 0, 0, 1_217];
            check_receipts(&world, &rail, &original, paid, 0);
            for (stage, slot) in [(1, 14), (2, 17)] {
                world.env.svm.warp_to_slot(slot);
                commit_payments(&mut world, &rail, &[normalize.clone()], [0; 5], false);
                check_receipts(&world, &rail, &original, paid, stage);
                for actor in [0, 4] {
                    let mut delta = [0; 5];
                    delta[actor] = entitlement(actor, RESIDUALS[stage])
                        - entitlement(actor, RESIDUALS[stage - 1]);
                    commit_payments(
                        &mut world,
                        &rail,
                        &[retained[1][actor / 4][0].clone()],
                        delta,
                        true,
                    );
                    paid[actor] += delta[actor];
                    check_receipts(&world, &rail, &original, paid, stage);
                }
            }
            commit_payments(&mut world, &rail, &[normalize.clone()], [0; 5], false);
            commit_payments(&mut world, &rail, &[normalize], [0, 0, 1_283, 0, 0], false);
            paid[2] = 1_283;
            for actor in [0, 4] {
                assert_eq!(
                    world.receipt(actor).paid_effective,
                    entitlement(actor, FINAL_RESIDUAL)
                );
                commit_payments(
                    &mut world,
                    &rail,
                    &[retained[0][actor / 4][0].clone()],
                    [0; 5],
                    false,
                );
            }
            assert_eq!(paid, [1_198, 0, 1_283, 0, 1_368]);
            for actor in 0..6 {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                assert!(!world.receipt(actor).present);
                let close = close_portfolio(&world, actor);
                submit(&mut world, &mut evidence, &[close], false, &[actor]).unwrap();
            }
            assert_eq!(world.env.market_state().1.vault, 2);
            assert_eq!(world.env.token_amount(world.env.vault), 235);
            assert_eq!(world.env.token_amount(rail.vault), 0);
            rail.check(&world);
            let meta = submit(&mut world, &mut evidence, &[slab.clone()], true, &[]).unwrap();
            successes(&meta, spl_token::ID, 4);
            let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            for vault in [world.env.vault, rail.vault] {
                assert!(world
                    .env
                    .svm
                    .get_account(&vault)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            }

            // Both owners spend their complete payout, then close both payout ATAs.
            // The already-paid value stays visible in the independent sponsor's custody.
            for actor in [0, 4] {
                assert_eq!(rail.paid(&world, actor), paid[actor]);
                for (token, sink, amount) in [
                    (
                        world.actors[actor].token,
                        world.actors[1].token,
                        CAPITAL[actor] as u64 + entitlement(actor, INITIAL_RESIDUAL) as u64,
                    ),
                    (
                        rail.destinations[actor],
                        rail.destinations[1],
                        (entitlement(actor, FINAL_RESIDUAL) - entitlement(actor, INITIAL_RESIDUAL))
                            as u64,
                    ),
                ] {
                    assert_eq!(world.env.token_amount(token), amount);
                    let before = rail.frame(&world);
                    let owner = world.actors[actor].owner.pubkey();
                    let rent = world.env.svm.get_account(&token).unwrap().lamports
                        - if native && token == rail.destinations[actor] {
                            amount
                        } else {
                            0
                        };
                    let owner_lamports = world.env.svm.get_account(&owner).unwrap().lamports;
                    let sink_amount = world.env.token_amount(sink);
                    submit(
                        &mut world,
                        &mut evidence,
                        &[
                            spl_token::instruction::transfer(
                                &spl_token::ID,
                                &token,
                                &sink,
                                &owner,
                                &[],
                                amount,
                            )
                            .unwrap(),
                            spl_token::instruction::close_account(
                                &spl_token::ID,
                                &token,
                                &owner,
                                &owner,
                                &[],
                            )
                            .unwrap(),
                        ],
                        false,
                        &[actor],
                    )
                    .unwrap();
                    assert_eq!(
                        world.env.svm.get_account(&owner).unwrap().lamports,
                        owner_lamports + rent
                    );
                    assert_eq!(world.env.token_amount(sink), sink_amount + amount);
                    assert!(world
                        .env
                        .svm
                        .get_account(&token)
                        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                    world.assert_frame_except(&before, &[token, sink, owner]);
                }
            }
            assert_eq!(
                world.env.token_amount(world.actors[1].token),
                PRIMARY_REFILL
            );
            assert_eq!(
                world.env.token_amount(rail.destinations[1]),
                SECONDARY_SUPPLY
            );
            world.peak_cu = 0;

            // Fresh ATA initialization and actual funding succeed before each retained
            // handler rejects. A failed suffix must undo even new mint/native backing.
            for secondary in [false, true] {
                for actor in [0, 4] {
                    let prefix = recreate_and_fund(&world, &rail, actor, secondary, prefund);
                    for old in &retained[usize::from(secondary)][actor / 4] {
                        let before = rail.frame(&world);
                        let mut batch = prefix.clone();
                        batch.push(old.clone());
                        let failure = submit(
                            &mut world,
                            &mut evidence,
                            &batch,
                            secondary && !native,
                            &[1],
                        )
                        .unwrap_err();
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(
                                (batch.len() + 1) as u8,
                                tombstone_error(old)
                            )
                        );
                        successes(&failure.meta, associated_token_program_id(), 2);
                        successes(&failure.meta, spl_token::ID, 7);
                        successes(&failure.meta, world.env.program_id, 0);
                        assert_eq!(
                            rail.frame(&world),
                            before,
                            "recreation, funding, rent and mint rollback"
                        );
                        evidence.rollbacks += 1;
                        recreation_rollbacks += 1;
                    }
                }
            }

            // Commit the unchanged creation/funding prefixes, then recreate the peer.
            // Neither receipt owner signs any transaction from this point onward.
            let sponsor = world.actors[1].owner.pubkey();
            let sponsor_before = world.env.svm.get_account(&sponsor).unwrap().lamports;
            let ata_rent = world
                .env
                .svm
                .minimum_balance_for_rent_exemption(TokenAccount::LEN);
            for secondary in [false, true] {
                let mint = if secondary { rail.mint } else { world.env.mint };
                let prefix = recreate_and_fund(&world, &rail, 0, secondary, prefund);
                let meta = submit(
                    &mut world,
                    &mut evidence,
                    &prefix,
                    secondary && !native,
                    &[1],
                )
                .unwrap();
                successes(&meta, associated_token_program_id(), 2);
                successes(&meta, spl_token::ID, 7);
                let peer = create_custody(&world, world.actors[4].owner.pubkey(), mint, prefund);
                submit(&mut world, &mut evidence, &peer, false, &[1]).unwrap();
            }
            assert_eq!(
                world.env.svm.get_account(&sponsor).unwrap().lamports,
                sponsor_before - 6 * ata_rent - if native { SECONDARY_REFILL } else { 0 },
                "prefunding contributes to rent exactly once"
            );
            check_custody(&world, &rail, &tombstone);
            for slot in [17, 100] {
                world.env.svm.warp_to_slot(slot);
                for old in retained.iter().flatten().flatten() {
                    reject(
                        &mut world,
                        &rail,
                        &mut evidence,
                        &[old.clone()],
                        false,
                        &[],
                        tombstone_error(old),
                        0,
                    );
                    check_custody(&world, &rail, &tombstone);
                }
                reject(
                    &mut world,
                    &rail,
                    &mut evidence,
                    &[slab.clone()],
                    true,
                    &[],
                    tombstone_error(&slab),
                    0,
                );
                check_custody(&world, &rail, &tombstone);
            }
            peaks[usize::from(native)] = peaks[usize::from(native)].max(world.peak_cu);
        }
    }
    assert_eq!(recreation_rollbacks, 32);
    assert_eq!(evidence.rollbacks, 104);
    println!("INV-067 post-close custody: 4 histories, 24 recreated ATAs, {} exact rollbacks ({} creation/funding, 72 funded replays); peak CU classic={}, native={}; peak bytes={}", evidence.rollbacks, recreation_rollbacks, peaks[0], peaks[1], evidence.peak_bytes);
}
