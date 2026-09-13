//! INV-039/024/037/041/048/073/081: domain insurance can pay opposing debt,
//! but funding alone cannot discharge a pending cohort. An input-derived book
//! separates insurance consumption, B booking and the holder's later B debit.
//! This bounded product adds insured bankruptcy through resolution to the
//! uninsured two-domain owner. It leaves row419 OPEN: no fractional, ADL,
//! backing, funding/fee, restart or arbitrary-history oracle is claimed.

use super::*;
use solana_sdk::fee::FeeStructure;

const DONOR_DEPOSIT: u128 = 20_001;

struct InsuredDebtBook {
    initial: [CloseProgressLedgerV16; 2],
    insurance: [u128; 2],
    booked: [bool; 2],
    released: [bool; 2],
    source: Pubkey,
}

impl InsuredDebtBook {
    fn payout(&self, actor: usize) -> u128 {
        match actor {
            0 | 2 => PAYOUTS[actor] + self.insurance[actor / 2],
            4 => DONOR_DEPOSIT - self.insurance.iter().sum::<u128>(),
            _ => 0,
        }
    }

    fn check(&self, world: &AttributionWorld) {
        // This complete portfolio census also checks matched OI, exact stored
        // counts/weights, ADL factors, capital/PnL totals and fixed SPL supply.
        world.check([0; 4], [!self.released[0], false, !self.released[1], false]);
        assert_eq!(world.env.token_amount(self.source), 0);
        let group = world.env.market_state().1;
        let mut budgets = vec![0; group.insurance_domain_budget.len()];
        let mut spent = budgets.clone();
        for pair in 0..2 {
            let domain = 2 * (pair + 1) + usize::from(world.quantities[2 * pair] < 0);
            budgets[domain] = self.insurance[pair];
            spent[domain] = if self.booked[pair] {
                self.insurance[pair]
            } else {
                0
            };
            let mut expected = self.initial[pair];
            assert!(expected.active && !expected.finalized && !expected.canceled);
            assert_eq!(expected.gross_loss_at_close_start, RESIDUALS[pair]);
            assert_eq!(expected.residual_remaining, RESIDUALS[pair]);
            assert_eq!(
                (
                    expected.support_consumed,
                    expected.insurance_spent,
                    expected.b_loss_booked,
                    expected.explicit_loss_assigned,
                    expected.drift_consumed,
                    expected.junior_face_burned
                ),
                (0, 0, 0, 0, 0, 0)
            );
            if self.booked[pair] {
                expected.finalized = true;
                expected.residual_remaining = 0;
                expected.insurance_spent = self.insurance[pair];
                expected.b_loss_booked = RESIDUALS[pair] - self.insurance[pair];
            }
            let actual = close_progress(
                &world
                    .env
                    .portfolio_state(world.actors[2 * pair + 1].portfolio),
            );
            assert_eq!(
                actual, expected,
                "domain {pair}: exact independent close partition"
            );
            crate::support::fuzz_model::verify_close_residual_partition(
                "insured pending cohort",
                &actual,
            )
            .unwrap();
            assert!(!self.released[pair] || self.booked[pair]);
        }
        assert_eq!(group.insurance_domain_budget, budgets);
        assert_eq!(group.insurance_domain_spent, spent);
        assert_eq!(
            group.insurance,
            budgets.iter().sum::<u128>() - spent.iter().sum::<u128>()
        );
        for (actor, a) in world.actors.iter().enumerate() {
            let account = world.env.portfolio_state(a.portfolio);
            let receipt = resolved_receipt(&account);
            let due = if receipt.present {
                assert!(actor == 0 || actor == 2);
                assert!(self.booked.iter().all(|x| *x));
                assert!(self.released[actor / 2]);
                // Realized source credit can convert part of the claim into
                // capital. The owner equation below counts both forms once.
                assert!(receipt.terminal_positive_claim_face > 0);
                assert!(
                    receipt.terminal_positive_claim_face
                        <= ATTRIBUTION_DEPOSITS[actor + 1] + self.insurance[actor / 2]
                );
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .unwrap()
            } else {
                0
            };
            let expected = match actor {
                0 | 2 if !self.released[actor / 2] => {
                    (ATTRIBUTION_DEPOSITS[actor] + GAINS[actor / 2]) as i128
                }
                1 | 3 if !self.booked[actor / 2] => -(RESIDUALS[actor / 2] as i128),
                _ => self.payout(actor) as i128,
            };
            assert_eq!(
                account.capital.get() as i128
                    + account.pnl.get()
                    + due as i128
                    + world.env.token_amount(a.token) as i128,
                expected,
                "actor {actor}: booked={:?}, released={:?}, insurance={:?}, capital={}, pnl={}, receipt={receipt:?}, wallet={}",
                self.booked, self.released, self.insurance, account.capital.get(), account.pnl.get(), world.env.token_amount(a.token)
            );
            assert!(world.env.token_amount(a.token) as u128 <= self.payout(actor));
            if (actor == 0 || actor == 2)
                && (!self.released[actor / 2] || self.booked.contains(&false))
            {
                assert_eq!(world.env.token_amount(a.token), 0);
                assert!(!receipt.present);
                assert!(!group.payout_snapshot_captured);
            }
        }
    }

    fn step(&mut self, world: &mut AttributionWorld, actor: usize, peak: &mut u64) {
        let before = world.frame();
        let source_before = world.env.svm.get_account(&self.source);
        let old_leg = if (actor == 0 || actor == 2) && !self.released[actor / 2] {
            Some(active_leg_for_asset(
                &world.env.portfolio_state(world.actors[actor].portfolio),
                actor / 2 + 1,
            ))
        } else {
            None
        };
        let cu = world
            .payout(actor, false)
            .expect("prescribed insured terminal step progresses");
        *peak = (*peak).max(cu);
        assert_cu_within("INV-039 insured resolved step", cu, CUSTODY_CU_LIMIT);
        assert_ne!(
            world.frame(),
            before,
            "a successful continuation must progress"
        );
        if actor == 1 || actor == 3 {
            self.booked[actor / 2] = true;
        } else if actor == 0 || actor == 2 {
            if self.booked[actor / 2] {
                self.released[actor / 2] = true;
            } else {
                assert_eq!(
                    Some(active_leg_for_asset(
                        &world.env.portfolio_state(world.actors[actor].portfolio),
                        actor / 2 + 1
                    )),
                    old_leg,
                    "funded but unconsumed insurance cannot erase the old obligation"
                );
            }
        }
        for (key, account) in before {
            if ![
                world.env.market,
                world.env.vault,
                world.actors[actor].portfolio,
                world.actors[actor].token,
            ]
            .contains(&key)
            {
                assert_eq!(
                    world.env.svm.get_account(&key),
                    account,
                    "foreign Account {key}"
                );
            }
        }
        assert_eq!(world.env.svm.get_account(&self.source), source_before);
        self.check(world);
    }
}

fn fund(world: &mut AttributionWorld, insurance: [u128; 2]) -> Pubkey {
    let amount = insurance.iter().sum::<u128>();
    let env = &mut world.env;
    let source = create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
    let donor = &world.actors[4];
    let before_legs: Vec<_> = world.actors[..4]
        .iter()
        .map(|a| env.svm.get_account(&a.portfolio))
        .collect();
    env.send(
        env.withdraw_ix(donor.portfolio, amount),
        vec![
            AccountMeta::new(donor.owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(donor.portfolio, false),
            AccountMeta::new(donor.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&donor.owner],
    )
    .expect("independent principal funds the insured close");
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::transfer(
            &spl_token::ID,
            &donor.token,
            &source,
            &donor.owner.pubkey(),
            &[],
            amount as u64,
        )
        .unwrap(),
        &[&donor.owner],
    )
    .unwrap();
    for (pair, amount) in insurance.into_iter().enumerate() {
        let asset = pair + 1;
        let seq = env.control_sequences(asset);
        let market_id = env.asset_market_id(asset as u16);
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::TopUpInsuranceDomain {
                domain: (2 * asset + usize::from(world.quantities[2 * pair] < 0)) as u16,
                market_id,
                authority_epoch: seq.authority_epoch,
                intent_id: next_control_sequence(seq.insurance_top_up),
                amount,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&env.admin],
        )
        .unwrap();
    }
    for (a, before) in world.actors[..4].iter().zip(before_legs) {
        assert_eq!(
            env.svm.get_account(&a.portfolio),
            before,
            "funding cannot settle an opposing account"
        );
    }
    assert_eq!(env.token_amount(source), 0);
    source
}

fn reject_pending_deletion(world: &mut AttributionWorld, actor: usize) -> u64 {
    world.env.svm.expire_blockhash();
    let a = &world.actors[actor];
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), close_instruction(world, actor)],
        Some(&world.env.payer.pubkey()),
        &[&world.env.payer, &a.owner],
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend(world.frame().into_iter().map(|(key, _)| key));
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .iter()
        .map(|key| world.env.svm.get_account(key))
        .collect();
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    let failure = world
        .env
        .svm
        .send_transaction(tx)
        .expect_err("pending debt blocks mechanical deletion");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(PercolatorError::EngineLockActive as u32)
        )
    );
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == world.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            world.env.svm.get_account(&key),
            expected,
            "complete Account {key}"
        );
    }
    assert_cu_within(
        "INV-039 pending deletion rejection",
        failure.meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    failure.meta.compute_units_consumed
}

fn reject_unbooked_claims(world: &mut AttributionWorld) -> u64 {
    [0, 2]
        .into_iter()
        .map(|actor| {
            let mut claim = terminal_instruction(world, actor);
            claim.data = ProgInstruction::ClaimResolvedPayoutTopup.encode();
            reject(world, &[claim], 2, PercolatorError::EngineLockActive)
        })
        .max()
        .unwrap()
}

#[test]
fn v16_program_insured_pending_domains_preserve_exact_debt_through_resolution_orders() {
    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    // Counterparty-credit ratios are exactly 24/25 or 15/16, and 125/128.
    // This isolates insurance/B attribution from fractional source conversion.
    for insurance in [[7_500u128, 6_000], [12_000, 6_000]] {
        for reverse in [false, true] {
            for early in 0..2 {
                for live_booking in [false, true] {
                    for holders in [[0usize, 2], [2, 0]] {
                        let mut deposits = ATTRIBUTION_DEPOSITS;
                        deposits[4] = DONOR_DEPOSIT;
                        let mut world = setup_with_deposits(reverse, &mut peak, deposits);
                        let source = fund(&mut world, insurance);
                        let mut book = InsuredDebtBook {
                            initial: std::array::from_fn(|pair| {
                                close_progress(
                                    &world
                                        .env
                                        .portfolio_state(world.actors[2 * pair + 1].portfolio),
                                )
                            }),
                            insurance,
                            booked: [false; 2],
                            released: [false; 2],
                            source,
                        };
                        book.check(&world);
                        for actor in [0, 1, 2, 3] {
                            peak = peak.max(reject_pending_deletion(&mut world, actor));
                            rollbacks += 1;
                            book.check(&world);
                        }
                        if live_booking {
                            let crank = Instruction {
                                program_id: world.env.program_id,
                                data: ProgInstruction::PermissionlessCrank {
                                    now_slot: 5,
                                    observations: crank_observations((early + 1) as u16),
                                }
                                .encode(),
                                accounts: vec![
                                    AccountMeta::new(world.env.payer.pubkey(), true),
                                    AccountMeta::new(world.env.market, false),
                                    AccountMeta::new(world.actors[2 * early + 1].portfolio, false),
                                ],
                            };
                            let mut suffix = close_instruction(&world, 4);
                            suffix.accounts[0].is_signer = false;
                            peak = peak.max(reject(
                                &mut world,
                                &[crank, suffix],
                                3,
                                PercolatorError::ExpectedSigner,
                            ));
                            rollbacks += 1;
                            book.check(&world);
                            let before = world.frame();
                            let cu = world.env.crank(
                                world.actors[2 * early + 1].portfolio,
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: 5,
                                    observations: crank_observations((early + 1) as u16),
                                },
                            );
                            assert_cu_within("INV-039 insured live booking", cu, CRANK_CU_LIMIT);
                            peak = peak.max(cu);
                            book.booked[early] = true;
                            book.check(&world);
                            for (key, account) in before {
                                if ![world.env.market, world.actors[2 * early + 1].portfolio]
                                    .contains(&key)
                                {
                                    assert_eq!(world.env.svm.get_account(&key), account);
                                }
                            }
                        }
                        let before = world.frame();
                        peak = peak.max(world.env.resolve());
                        for (key, account) in before {
                            if key != world.env.market {
                                assert_eq!(world.env.svm.get_account(&key), account);
                            }
                        }
                        world.env.svm.warp_to_slot(10);
                        book.check(&world);
                        peak = peak.max(reject_unbooked_claims(&mut world));
                        rollbacks += 2;
                        book.check(&world);
                        let late = 1 - early;
                        let mut suffix = close_instruction(&world, 4);
                        suffix.accounts[0].is_signer = false;
                        if !live_booking {
                            let bundle =
                                [terminal_instruction(&world, 2 * early + 1), suffix.clone()];
                            peak = peak.max(reject(
                                &mut world,
                                &bundle,
                                3,
                                PercolatorError::ExpectedSigner,
                            ));
                            rollbacks += 1;
                            book.check(&world);
                            book.step(&mut world, 2 * early + 1, &mut peak);
                        }
                        book.step(&mut world, 2 * early, &mut peak);
                        book.step(&mut world, 2 * late, &mut peak);
                        peak = peak.max(reject_pending_deletion(&mut world, 2 * late));
                        rollbacks += 1;
                        peak = peak.max(reject_unbooked_claims(&mut world));
                        rollbacks += 2;
                        book.check(&world);
                        let bundle = [
                            terminal_instruction(&world, 2 * late + 1),
                            terminal_instruction(&world, 2 * late),
                            suffix.clone(),
                        ];
                        peak = peak.max(reject(
                            &mut world,
                            &bundle,
                            4,
                            PercolatorError::ExpectedSigner,
                        ));
                        rollbacks += 1;
                        book.check(&world);
                        book.step(&mut world, 2 * late + 1, &mut peak);
                        for actor in [2 * early + 1, 4] {
                            if !resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio,
                            ) {
                                book.step(&mut world, actor, &mut peak);
                            }
                        }
                        for _ in 0..2 {
                            for actor in holders {
                                if resolved_portfolio_is_terminal(
                                    &world.env,
                                    world.actors[actor].portfolio,
                                ) {
                                    continue;
                                }
                                let ix = terminal_instruction(&world, actor);
                                if actor == 2 * early && !book.released[late] {
                                    peak = peak.max(reject(
                                        &mut world,
                                        &[ix],
                                        2,
                                        PercolatorError::EngineNonProgress,
                                    ));
                                } else {
                                    peak = peak.max(reject(
                                        &mut world,
                                        &[ix, suffix.clone()],
                                        3,
                                        PercolatorError::ExpectedSigner,
                                    ));
                                    book.check(&world);
                                    book.step(&mut world, actor, &mut peak);
                                }
                                rollbacks += 1;
                                book.check(&world);
                            }
                        }
                        for actor in 0..5 {
                            assert_eq!(
                                world.env.token_amount(world.actors[actor].token) as u128,
                                book.payout(actor)
                            );
                            assert!(resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio
                            ));
                            let ix = terminal_instruction(&world, actor);
                            peak = peak.max(reject(
                                &mut world,
                                &[ix],
                                2,
                                PercolatorError::EngineNonProgress,
                            ));
                            rollbacks += 1;
                        }
                        book.check(&world);
                        let group = world.env.market_state().1;
                        assert_eq!(
                            (group.vault, group.insurance, group.c_tot, group.pnl_pos_tot),
                            (0, 0, 0, 0)
                        );
                        for actor in holders.into_iter().chain([2 * early + 1, 2 * late + 1, 4]) {
                            let before = world.frame();
                            let count = world.env.market_state().1.materialized_portfolio_count;
                            let a = &world.actors[actor];
                            let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                            assert_cu_within(
                                "INV-039 insured terminal deletion",
                                cu,
                                CUSTODY_CU_LIMIT,
                            );
                            assert_eq!(
                                world.env.market_state().1.materialized_portfolio_count,
                                count - 1
                            );
                            assert!(world
                                .env
                                .svm
                                .get_account(&a.portfolio)
                                .map_or(true, |account| account.lamports == 0
                                    && account.data.is_empty()));
                            for (key, account) in before {
                                if ![world.env.market, a.portfolio].contains(&key) {
                                    assert_eq!(world.env.svm.get_account(&key), account);
                                }
                            }
                        }
                        assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    println!("INV-039 insured pending domains: {worlds} worlds, {rollbacks} rollback checks, 160 exact payouts and terminal deletions; peak CU={peak}");
}
