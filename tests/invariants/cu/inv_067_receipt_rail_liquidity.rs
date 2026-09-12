//! Row 417: two unequal receipt holders compete for one canonical secondary reserve.
//! A one-atom-short batch rolls back expiry and the first SPL payout; either public
//! replenishment or a different funded rail must preserve both original claim identities.

use super::{late_expiry::World, *};
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
const INITIAL_RESIDUAL: u128 = 501;
const FINAL_RESIDUAL: u128 = 851;
const PRIMARY_SUPPLY: u128 = 3_852;
const SECONDARY_SUPPLY: u64 = 233;

fn entitlement(actor: usize, residual: u128) -> u128 {
    FACES[actor] * residual / FACES.iter().sum::<u128>()
}

fn install_secondary(env: &mut V16CuEnv) {
    let mint = inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_create_public_spl_mint(
        &mut env.svm, &env.payer, env.admin.pubkey(), 0,
    );
    env.update_base_unit_mints_with_cu(env.mint, mint);
}

struct Rail {
    mint: Pubkey,
    vault: Pubkey,
    source: Pubkey,
    destinations: [Pubkey; 5],
    funded: u64,
}

impl Rail {
    fn new(world: &mut World) -> Self {
        let env = &mut world.env;
        let mint = Pubkey::new_from_array(env.market_state().0.secondary_collateral_mint);
        let vault = create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, mint);
        let source = create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), mint);
        let destinations = std::array::from_fn(|actor| {
            create_ata_for_test(
                &mut env.svm,
                &env.payer,
                world.actors[actor].owner.pubkey(),
                mint,
            )
        });
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &mint,
                &source,
                &env.admin.pubkey(),
                &[],
                SECONDARY_SUPPLY,
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        Self {
            mint,
            vault,
            source,
            destinations,
            funded: 0,
        }
    }

    fn frame(&self, world: &World) -> Vec<(Pubkey, Option<Account>)> {
        let mut frame = world.frame();
        frame.extend(
            [self.mint, self.vault, self.source]
                .into_iter()
                .chain(self.destinations)
                .map(|key| (key, world.env.svm.get_account(&key))),
        );
        frame
    }

    fn fund(&mut self, world: &mut World, amount: u64) {
        let before = self.frame(world);
        let instruction = spl_token::instruction::transfer(
            &spl_token::ID,
            &self.source,
            &self.vault,
            &world.env.admin.pubkey(),
            &[],
            amount,
        )
        .unwrap();
        world.land(&[instruction], true).unwrap();
        self.funded += amount;
        world.assert_frame_except(&before, &[self.source, self.vault]);
        self.check(world);
    }

    fn payout(&self, world: &World, actor: usize, claim: bool) -> Instruction {
        let mut instruction = world.payout(actor, claim);
        instruction.accounts[3] = AccountMeta::new(self.destinations[actor], false);
        instruction.accounts[4] = AccountMeta::new(self.vault, false);
        instruction
    }

    fn paid(&self, world: &World, actor: usize) -> u128 {
        world.env.token_amount(world.actors[actor].token) as u128
            + world.env.token_amount(self.destinations[actor]) as u128
    }

    fn check(&self, world: &World) {
        let env = &world.env;
        let primary_vault = env.token_amount(env.vault) as u128;
        let secondary_vault = env.token_amount(self.vault) as u128;
        assert_eq!(
            primary_vault + secondary_vault,
            env.market_state().1.vault + self.funded as u128,
            "unbooked secondary liquidity is not a new engine claim"
        );
        assert_eq!(
            primary_vault
                + env.token_amount(world.provider_token) as u128
                + world
                    .actors
                    .iter()
                    .map(|actor| env.token_amount(actor.token) as u128)
                    .sum::<u128>(),
            PRIMARY_SUPPLY
        );
        assert_eq!(
            secondary_vault
                + env.token_amount(self.source) as u128
                + self
                    .destinations
                    .iter()
                    .map(|key| env.token_amount(*key) as u128)
                    .sum::<u128>(),
            SECONDARY_SUPPLY as u128
        );
        for (mint, supply) in [
            (env.mint, PRIMARY_SUPPLY),
            (self.mint, SECONDARY_SUPPLY as u128),
        ] {
            assert_eq!(
                Mint::unpack(&env.svm.get_account(&mint).unwrap().data)
                    .unwrap()
                    .supply as u128,
                supply
            );
        }
        for actor in 0..5 {
            assert!(
                self.paid(world, actor) <= CAPITAL[actor] + entitlement(actor, FINAL_RESIDUAL),
                "cross-rail prefix overpaid actor {actor}"
            );
        }
    }
}

#[test]
fn v16_program_late_expiry_claimant_orders_share_secondary_liquidity_without_losing_receipts() {
    let mut peak_cu = 0;
    let mut worlds = 0;
    for landing in [13, 14] {
        for order in [[0, 4], [4, 0]] {
            for first_claim_route in [false, true] {
                for replenish in [false, true] {
                    let mut world = World::before_receipts_with_setup(install_secondary);
                    for actor in [0, 4] {
                        for _ in 0..8 {
                            if world.receipt(actor).present {
                                break;
                            }
                            world.land(&[world.payout(actor, false)], false).unwrap();
                        }
                        let receipt = world.receipt(actor);
                        assert!(receipt.present && !receipt.finalized);
                        assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                        assert_eq!(receipt.paid_effective, entitlement(actor, INITIAL_RESIDUAL));
                        assert_eq!(
                            receipt.prior_bound_contribution_num,
                            FACES[actor] * BOUND_SCALE
                        );
                    }
                    let original = [world.receipt(order[0]), world.receipt(order[1])];
                    let identity = order.map(|actor| {
                        state::read_portfolio_owner_preflight(
                            &world
                                .env
                                .svm
                                .get_account(&world.actors[actor].portfolio)
                                .unwrap()
                                .data,
                        )
                        .unwrap()
                    });
                    let mut rail = Rail::new(&mut world);
                    rail.fund(&mut world, SECONDARY_SUPPLY - 1);
                    let due = order.map(|actor| {
                        entitlement(actor, FINAL_RESIDUAL) - entitlement(actor, INITIAL_RESIDUAL)
                    });
                    assert!(due
                        .iter()
                        .all(|amount| *amount > 0 && *amount < (SECONDARY_SUPPLY - 1) as u128));
                    assert_eq!(due.iter().sum::<u128>(), SECONDARY_SUPPLY as u128);
                    let release = world.payout(2, false);
                    let retained = [
                        rail.payout(&world, order[0], first_claim_route),
                        rail.payout(&world, order[1], !first_claim_route),
                    ];
                    world.env.svm.warp_to_slot(landing);
                    let before = rail.frame(&world);
                    let group = world.env.market_state().1;
                    assert_eq!(
                        group.source_backing_buckets[3].status,
                        BackingBucketStatusV16::Fresh
                    );
                    assert_eq!(
                        group.source_credit[3].fresh_reserved_backing_num,
                        350 * BOUND_SCALE
                    );
                    assert_eq!(
                        group.resolved_payout_ledger.snapshot_residual,
                        INITIAL_RESIDUAL
                    );
                    let batch = [release.clone(), retained[0].clone(), retained[1].clone()];
                    let failure = world
                        .land(&batch, false)
                        .expect_err("shared secondary reserve is exactly one atom short");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            4,
                            InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32)
                        )
                    );
                    for (program, successes) in [(world.env.program_id, 2), (spl_token::ID, 1)] {
                        assert_eq!(failure.meta.logs.iter().filter(|line| **line == format!("Program {program} success")).count(), successes,
                            "expiry and the first SPL payout must execute before the second claimant fails");
                    }
                    assert_cu_within(
                        "shared-rail expiry/payout rollback",
                        failure.meta.compute_units_consumed,
                        500_000,
                    );
                    assert_eq!(
                        rail.frame(&world),
                        before,
                        "both receipts, late expiry, both mints, vaults and rent roll back"
                    );
                    rail.check(&world);

                    if replenish {
                        rail.fund(&mut world, 1);
                        let before = rail.frame(&world);
                        world.land(&batch, false).expect(
                            "unchanged expiry and claim bytes retry with exact shared liquidity",
                        );
                        world.assert_frame_except(
                            &before,
                            &[
                                world.env.market,
                                world.actors[2].portfolio,
                                world.actors[order[0]].portfolio,
                                world.actors[order[1]].portfolio,
                                rail.vault,
                                rail.destinations[order[0]],
                                rail.destinations[order[1]],
                            ],
                        );
                        assert_eq!(world.env.token_amount(rail.vault), 0);
                    } else {
                        // Reverse the failed batch's claimant order. The other owner can use
                        // the existing secondary reserve; the first still has a primary exit.
                        let before = rail.frame(&world);
                        world.land(&[release, retained[1].clone()], false).unwrap();
                        world.assert_frame_except(
                            &before,
                            &[
                                world.env.market,
                                world.actors[2].portfolio,
                                world.actors[order[1]].portfolio,
                                rail.vault,
                                rail.destinations[order[1]],
                            ],
                        );
                        assert_eq!(world.receipt(order[0]), original[0]);
                        assert_eq!(
                            world.env.token_amount(rail.destinations[order[1]]) as u128,
                            due[1]
                        );
                        rail.check(&world);
                        let before = rail.frame(&world);
                        let vault = world.env.token_amount(world.env.vault);
                        world
                            .land(&[world.payout(order[0], first_claim_route)], false)
                            .unwrap();
                        assert_eq!(
                            vault - world.env.token_amount(world.env.vault),
                            due[0] as u64
                        );
                        world.assert_frame_except(
                            &before,
                            &[
                                world.env.market,
                                world.env.vault,
                                world.actors[order[0]].portfolio,
                                world.actors[order[0]].token,
                            ],
                        );
                    }
                    rail.check(&world);
                    let after = world.env.market_state().1;
                    assert_eq!(after.source_credit[3].fresh_reserved_backing_num, 0);
                    assert_ne!(
                        after.source_backing_buckets[3].status,
                        BackingBucketStatusV16::Fresh
                    );
                    let ledger = after.resolved_payout_ledger;
                    assert_eq!(ledger.snapshot_slot, 12);
                    assert_eq!(ledger.snapshot_residual, FINAL_RESIDUAL);
                    assert_eq!(ledger.current_payout_rate_num, FINAL_RESIDUAL * BOUND_SCALE);
                    assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
                    assert_eq!(
                        ledger.terminal_claim_exact_receipts_num,
                        2_000 * BOUND_SCALE
                    );
                    assert_eq!(
                        ledger.terminal_claim_bound_unreceipted_num,
                        1_000 * BOUND_SCALE
                    );
                    for (index, actor) in order.into_iter().enumerate() {
                        let mut expected = original[index];
                        expected.paid_effective = entitlement(actor, FINAL_RESIDUAL);
                        assert_eq!(
                            world.receipt(actor),
                            expected,
                            "only paid_effective changes across rails"
                        );
                        assert_eq!(
                            state::read_portfolio_owner_preflight(
                                &world
                                    .env
                                    .svm
                                    .get_account(&world.actors[actor].portfolio)
                                    .unwrap()
                                    .data
                            )
                            .unwrap(),
                            identity[index]
                        );
                        assert_eq!(
                            rail.paid(&world, actor),
                            CAPITAL[actor] + expected.paid_effective
                        );
                        let secondary_paid = if replenish || index == 1 {
                            due[index]
                        } else {
                            0
                        };
                        assert_eq!(
                            world.env.token_amount(rail.destinations[actor]) as u128,
                            secondary_paid
                        );
                        for secondary in [false, true] {
                            let before = rail.frame(&world);
                            let instruction = if secondary {
                                rail.payout(&world, actor, true)
                            } else {
                                world.payout(actor, true)
                            };
                            world.land(&[instruction], false).unwrap();
                            assert_eq!(
                                rail.frame(&world),
                                before,
                                "a different funded rail cannot pay the same receipt twice"
                            );
                        }
                    }
                    for _ in 0..16 {
                        for actor in [2, order[0], order[1], 1, 3] {
                            if resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio,
                            ) {
                                continue;
                            }
                            let before = rail.frame(&world);
                            let vault = world.env.token_amount(world.env.vault);
                            let engine_vault = world.env.market_state().1.vault;
                            let tokens = world.env.token_amount(world.actors[actor].token);
                            match world.land(&[world.payout(actor, false)], false) {
                                Ok(_) => {
                                    assert_ne!(
                                        rail.frame(&world),
                                        before,
                                        "successful terminal continuation must progress"
                                    );
                                    let payout =
                                        world.env.token_amount(world.actors[actor].token) - tokens;
                                    assert_eq!(
                                        vault - world.env.token_amount(world.env.vault),
                                        payout
                                    );
                                    assert_eq!(
                                        engine_vault - world.env.market_state().1.vault,
                                        payout as u128
                                    );
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
                                            )
                                        )
                                    );
                                    assert_eq!(rail.frame(&world), before);
                                }
                            }
                            rail.check(&world);
                        }
                        if world.actors.iter().all(|actor| {
                            resolved_portfolio_is_terminal(&world.env, actor.portfolio)
                        }) {
                            break;
                        }
                    }
                    for actor in 0..5 {
                        assert!(resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio
                        ));
                        assert_eq!(
                            rail.paid(&world, actor),
                            CAPITAL[actor] + entitlement(actor, FINAL_RESIDUAL)
                        );
                        let before = rail.frame(&world);
                        world
                            .land(
                                &[rail.payout(&world, actor, true), world.payout(actor, true)],
                                false,
                            )
                            .unwrap();
                        assert_eq!(
                            rail.frame(&world),
                            before,
                            "terminal cross-rail receipt retries"
                        );
                        let portfolio = world.actors[actor].portfolio;
                        let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                        let market_rent = world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports;
                        let cu = world
                            .env
                            .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                        assert_cu_within(
                            "shared-rail terminal portfolio close",
                            cu,
                            CUSTODY_CU_LIMIT,
                        );
                        peak_cu = peak_cu.max(cu);
                        assert_eq!(
                            world
                                .env
                                .svm
                                .get_account(&portfolio)
                                .map_or(0, |account| account.lamports),
                            0
                        );
                        assert_eq!(
                            world
                                .env
                                .svm
                                .get_account(&world.env.market)
                                .unwrap()
                                .lamports,
                            market_rent + rent
                        );
                        world.assert_frame_except(&before, &[world.env.market, portfolio]);
                        rail.check(&world);
                    }
                    let group = world.env.market_state().1;
                    assert_eq!(group.materialized_portfolio_count, 0);
                    assert_eq!(
                        [
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.source_claim_bound_total_num,
                            group.insurance
                        ],
                        [0; 4]
                    );
                    let rounding = FINAL_RESIDUAL
                        - (0..5)
                            .map(|actor| entitlement(actor, FINAL_RESIDUAL))
                            .sum::<u128>();
                    assert_eq!(rounding, 2);
                    assert_eq!(group.vault, rounding);
                    assert_eq!(
                        world.env.token_amount(world.env.vault)
                            + world.env.token_amount(rail.vault),
                        rounding as u64 + rail.funded
                    );
                    assert_eq!(world.env.token_amount(world.provider_token), 1);
                    assert_cu_within("shared-rail bounded progress", world.peak_cu, 500_000);
                    peak_cu = peak_cu.max(world.peak_cu);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("INV-067 row 417: {worlds} shared-rail worlds, 16 late-expiry paid-prefix rollbacks, 8 exact-liquidity retries, 8 mixed-rail exits; peak suffix CU {peak_cu}");
}
