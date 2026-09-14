//! INV-039/024: fractional B allocation outlives the debtor's Account incarnation.
//! Recreating and funding that address before/after one holder settles must keep
//! old cohort charges, detached carry, fresh capital and owner payouts distinct.

use super::*;
use solana_sdk::{
    fee::FeeStructure, instruction::InstructionError, rent::Rent, transaction::TransactionError,
};

const FRESH_CAPITAL: u128 = 113;

#[track_caller]
fn land(
    world: &mut AttributionWorld,
    instructions: &[Instruction],
    signers: &[&Keypair],
    changed: &[Pubkey],
    rejection: Option<(usize, PercolatorError, usize)>,
) -> u64 {
    world.env.svm.expire_blockhash();
    let mut batch = vec![heap_ix(), cu_ix()];
    batch.extend_from_slice(instructions);
    let mut signing = vec![&world.env.payer];
    signing.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &batch,
        Some(&world.env.payer.pubkey()),
        &signing,
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    assert_eq!(
        tx.message.header.num_required_signatures as usize,
        1 + signers.len()
    );
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
    let meta = if let Some((index, error, prefixes)) = rejection {
        let failure = result.expect_err("bounded suffix rejection after SPL payment");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                (2 + index) as u8,
                InstructionError::Custom(error as u32)
            )
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", world.env.program_id))
                .count(),
            prefixes
        );
        assert!(
            failure
                .meta
                .logs
                .iter()
                .any(|line| *line == format!("Program {} success", spl_token::ID)),
            "real SPL prefix"
        );
        failure.meta
    } else {
        result.expect("bounded fractional cohort continuation")
    };
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        if rejected || !changed.contains(&key) {
            assert_eq!(
                world.env.svm.get_account(&key),
                expected,
                "complete Account {key}"
            );
        }
    }
    assert_cu_within(
        "fractional cohort recreation transaction",
        meta.compute_units_consumed,
        600_000,
    );
    meta.compute_units_consumed
}

fn deletion(world: &AttributionWorld, actor: usize, signed: bool) -> Instruction {
    let a = &world.actors[actor];
    Instruction {
        program_id: world.env.program_id,
        data: world.env.close_portfolio_ix(a.portfolio).encode(),
        accounts: vec![
            AccountMeta::new(a.owner.pubkey(), signed),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(a.portfolio, false),
        ],
    }
}

fn recreate(world: &AttributionWorld) -> Vec<Instruction> {
    let env = &world.env;
    let debtor = &world.actors[1];
    let donor = &world.actors[4];
    vec![
        Instruction {
            program_id: env.program_id,
            data: env.withdraw_ix(donor.portfolio, 777).encode(),
            accounts: vec![
                AccountMeta::new(donor.owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(donor.portfolio, false),
                AccountMeta::new(donor.token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        },
        deletion(world, 1, true),
        system_instruction::transfer(
            &debtor.owner.pubkey(),
            &debtor.portfolio,
            env.svm
                .get_sysvar::<Rent>()
                .minimum_balance(env.portfolio_account_len),
        ),
        Instruction {
            program_id: env.program_id,
            data: ProgInstruction::InitPortfolio.encode(),
            accounts: vec![
                AccountMeta::new(debtor.owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(debtor.portfolio, false),
            ],
        },
        spl_token::instruction::transfer(
            &spl_token::ID,
            &donor.token,
            &debtor.token,
            &donor.owner.pubkey(),
            &[],
            FRESH_CAPITAL as u64,
        )
        .unwrap(),
        Instruction {
            program_id: env.program_id,
            data: ProgInstruction::Deposit {
                portfolio_id: env.market_state().0.next_portfolio_id,
                expected_sequence: 0,
                amount: FRESH_CAPITAL,
            }
            .encode(),
            accounts: vec![
                AccountMeta::new(debtor.owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(debtor.portfolio, false),
                AccountMeta::new(debtor.token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        },
    ]
}

fn check_classes(world: &AttributionWorld, book: &Book) {
    book.check(world);
    let env = &world.env;
    for actor in &world.actors {
        let raw = env.svm.get_account(&actor.token).unwrap();
        assert_eq!(raw.owner, spl_token::ID);
        let token = TokenAccount::unpack(&raw.data).unwrap();
        assert_eq!(token.owner, actor.owner.pubkey());
        assert_eq!(token.mint, env.mint);
    }
    if book.recreated_capital > 0 && !book.deleted[1] {
        let a = env.portfolio_state(world.actors[1].portfolio);
        assert_eq!(
            a.capital.get() + env.token_amount(world.actors[1].token) as u128,
            FRESH_CAPITAL,
            "new incarnation's senior principal"
        );
        assert_eq!(a.pnl.get(), 0);
        assert!(!resolved_receipt(&a).present);
        assert!(a
            .source_domains
            .iter()
            .all(|s| s.source_claim_bound_num.get() == 0));
    }
}

fn settle_holder(world: &mut AttributionWorld, book: &mut Book, actor: usize, peak: &mut u64) {
    let before = world.frame();
    *peak = (*peak).max(world.env.crank(
        world.actors[actor].portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: crank_observations(1),
        },
    ));
    // This continuation settles B; the retained leg still owns its fractional carry.
    book.settled[actor / 2] = true;
    for (key, account) in before {
        if ![world.env.market, world.actors[actor].portfolio].contains(&key) {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }
    check_classes(world, book);
}

fn check_fresh_class_vector(world: &AttributionWorld, book: &Book) {
    let input = book.input;
    let expected: [[i128; 3]; 5] = std::array::from_fn(|actor| match actor {
        0 | 2 => [
            input.deposits()[actor] as i128,
            (input.gains()[actor / 2]
                - if book.settled[actor / 2] {
                    input.losses()[actor / 2]
                } else {
                    0
                }) as i128,
            0,
        ],
        1 => [FRESH_CAPITAL as i128, 0, 0],
        3 => [input.payouts()[3] as i128, 0, 0],
        4 => [0, 0, (777 - FRESH_CAPITAL) as i128],
        _ => unreachable!(),
    });
    let actual: [[i128; 3]; 5] = std::array::from_fn(|actor| {
        let a = &world.actors[actor];
        let state = world.env.portfolio_state(a.portfolio);
        [
            state.capital.get() as i128,
            state.pnl.get(),
            world.env.token_amount(a.token) as i128,
        ]
    });
    let matches = |observation: [[i128; 3]; 5]| observation == expected;
    assert!(
        matches(actual),
        "owner-local capital, PnL and paid SPL classes"
    );
    let mut wrong_owner = actual;
    wrong_owner[0][0] -= 1;
    wrong_owner[1][0] += 1;
    let mut wrong_class = actual;
    wrong_class[1][0] -= 1;
    wrong_class[1][1] += 1;
    for observation in [wrong_owner, wrong_class] {
        assert_eq!(
            observation.iter().flatten().sum::<i128>(),
            actual.iter().flatten().sum()
        );
        assert!(
            !matches(observation),
            "aggregate-preserving attribution mutation"
        );
    }
}

fn payout(world: &AttributionWorld, actor: usize) -> Instruction {
    let env = &world.env;
    let a = &world.actors[actor];
    Instruction {
        program_id: env.program_id,
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
        accounts: vec![
            AccountMeta::new_readonly(a.owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(a.portfolio, false),
            AccountMeta::new(a.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    }
}

#[test]
fn v16_program_fractional_cohort_debt_survives_funded_debtor_recreation_and_resolution() {
    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    for (weights, residual) in [([450_003, 600_004], 7), ([300_003, 700_007], 13)] {
        let input = Inputs { weights, residual };
        assert!(input.booking_remainder() > 0);
        assert!(input.carries().iter().all(|carry| *carry > 0));
        let mut baseline = None;
        for reverse in [false, true] {
            for first in [0, 2] {
                for recreate_first in [false, true] {
                    let mut world = setup(input, reverse, false, &mut peak);
                    let mut book = Book {
                        input,
                        recovery_peer: false,
                        recreated_capital: 0,
                        booked: false,
                        settled: [false; 2],
                        detached: [false; 2],
                        deleted: [false; 5],
                        converted: [None; 2],
                    };
                    check_classes(&world, &book);
                    let donor = world.actors[4].owner.insecure_clone();
                    let debtor = world.actors[1].owner.insecure_clone();
                    let instructions = recreate(&world);
                    peak = peak.max(land(
                        &mut world,
                        &instructions,
                        &[&donor, &debtor],
                        &[],
                        Some((1, PercolatorError::EngineLockActive, 1)),
                    ));
                    rollbacks += 1;
                    check_classes(&world, &book);

                    let before = world.frame();
                    peak = peak.max(world.env.crank(
                        world.actors[1].portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: 5,
                            observations: crank_observations(1),
                        },
                    ));
                    book.booked = true;
                    for (key, account) in before {
                        if ![world.env.market, world.actors[1].portfolio].contains(&key) {
                            assert_eq!(world.env.svm.get_account(&key), account);
                        }
                    }
                    check_classes(&world, &book);
                    if !recreate_first {
                        settle_holder(&mut world, &mut book, first, &mut peak);
                    }

                    let old_id = world.env.portfolio_id(world.actors[1].portfolio);
                    let next_id = world.env.market_state().0.next_portfolio_id;
                    let old_group = world.env.market_state().1;
                    let market_rent = world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports;
                    let old_rent = world
                        .env
                        .svm
                        .get_account(&world.actors[1].portfolio)
                        .unwrap()
                        .lamports;
                    let debtor_sol = world
                        .env
                        .svm
                        .get_account(&debtor.pubkey())
                        .unwrap()
                        .lamports;
                    let new_rent = world
                        .env
                        .svm
                        .get_sysvar::<Rent>()
                        .minimum_balance(world.env.portfolio_account_len);
                    let prefix = recreate(&world);
                    let mut rejected = prefix.clone();
                    rejected.push(deletion(&world, 3, false));
                    peak = peak.max(land(
                        &mut world,
                        &rejected,
                        &[&donor, &debtor],
                        &[],
                        Some((prefix.len(), PercolatorError::ExpectedSigner, 4)),
                    ));
                    rollbacks += 1;
                    check_classes(&world, &book);
                    let changed = [
                        world.env.market,
                        world.env.vault,
                        world.actors[1].portfolio,
                        debtor.pubkey(),
                        world.actors[1].token,
                        world.actors[4].portfolio,
                        world.actors[4].token,
                    ];
                    peak = peak.max(land(
                        &mut world,
                        &prefix,
                        &[&donor, &debtor],
                        &changed,
                        None,
                    ));
                    book.recreated_capital = FRESH_CAPITAL;
                    check_classes(&world, &book);
                    check_fresh_class_vector(&world, &book);
                    assert_ne!(old_id, next_id);
                    assert_eq!(world.env.portfolio_id(world.actors[1].portfolio), next_id);
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.actors[1].portfolio)
                            .unwrap()
                            .lamports,
                        new_rent
                    );
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&debtor.pubkey())
                            .unwrap()
                            .lamports,
                        debtor_sol - new_rent
                    );
                    assert_eq!(
                        world
                            .env
                            .svm
                            .get_account(&world.env.market)
                            .unwrap()
                            .lamports,
                        market_rent + old_rent
                    );
                    let recreated = world.env.market_state().1;
                    assert_eq!(recreated.assets, old_group.assets);
                    assert_eq!(recreated.source_credit, old_group.source_credit);
                    assert_eq!(recreated.materialized_portfolio_count, 5);
                    if recreate_first {
                        settle_holder(&mut world, &mut book, first, &mut peak);
                    }
                    assert_eq!(book.settled, [first == 0, first == 2]);
                    assert_eq!(book.converted, [None; 2]);
                    let before = world.frame();
                    peak = peak.max(world.env.resolve());
                    for (key, account) in before {
                        if key != world.env.market {
                            assert_eq!(world.env.svm.get_account(&key), account);
                        }
                    }
                    world.env.svm.warp_to_slot(10);
                    check_classes(&world, &book);
                    let paid_prefix = vec![payout(&world, 1), deletion(&world, 3, false)];
                    peak = peak.max(land(
                        &mut world,
                        &paid_prefix,
                        &[],
                        &[],
                        Some((1, PercolatorError::ExpectedSigner, 1)),
                    ));
                    rollbacks += 1;
                    book.close(&mut world, 1, &mut peak);
                    assert_eq!(
                        world.env.token_amount(world.actors[1].token) as u128,
                        FRESH_CAPITAL
                    );
                    check_classes(&world, &book);

                    for _ in 0..6 {
                        for actor in [1, first, 4, 2 - first, 3] {
                            if book.deleted[actor] {
                                continue;
                            }
                            if !resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio,
                            ) {
                                book.close(&mut world, actor, &mut peak);
                                check_classes(&world, &book);
                            }
                            if resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio,
                            ) {
                                let market_rent = world
                                    .env
                                    .svm
                                    .get_account(&world.env.market)
                                    .unwrap()
                                    .lamports;
                                let released_rent = world
                                    .env
                                    .svm
                                    .get_account(&world.actors[actor].portfolio)
                                    .unwrap()
                                    .lamports;
                                book.delete(&mut world, actor, &mut peak);
                                assert_eq!(
                                    world
                                        .env
                                        .svm
                                        .get_account(&world.env.market)
                                        .unwrap()
                                        .lamports,
                                    market_rent + released_rent
                                );
                                check_classes(&world, &book);
                            }
                        }
                    }
                    assert!(book.deleted.iter().all(|deleted| *deleted));
                    assert!(book.settled.iter().all(|settled| *settled));
                    assert!(book.detached.iter().all(|detached| *detached));
                    let paid: [u128; 5] = std::array::from_fn(|i| {
                        world.env.token_amount(world.actors[i].token) as u128
                    });
                    assert_eq!(paid, book.owner_entitlements());
                    let group = world.env.market_state().1;
                    assert_eq!(
                        (
                            group.c_tot,
                            group.pnl_pos_tot,
                            group.materialized_portfolio_count
                        ),
                        (0, 0, 0)
                    );
                    assert_eq!(group.vault, input.cash_residue());
                    assert_eq!(
                        *baseline.get_or_insert((paid, group.vault)),
                        (paid, group.vault)
                    );

                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    assert_eq!(rollbacks, 48);
    println!("INV-039 fractional cohort recreation: {worlds} worlds, {rollbacks} complete SPL-prefix rollbacks, 80 terminal deletions; peak CU={peak}");
}
