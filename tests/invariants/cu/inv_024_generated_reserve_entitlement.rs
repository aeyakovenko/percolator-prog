//! INV-024/036/041: generated signed reserve succession preserves each owner's
//! unpaid entitlement across payout partitions, order and submitter changes.
//! Rows 410/416/429 receive bounded evidence; no terminal-actionability claim.

use super::*;
use rand::{rngs::StdRng, Rng, SeedableRng};

const STOCK: [u64; 3] = [EARNINGS, INSURANCE, BACKING];
const SEEDS: [u64; 8] = [0x24, 0x27, 0x36, 0x41, 0x410, 0x416, 0x429, 0x135];

#[derive(Clone, Debug, PartialEq, Eq)]
struct ClaimHistory {
    credited: [[u64; 3]; 5],
    transferred_out: [[u64; 3]; 5],
    paid: [[u64; 3]; 5],
    holders: [usize; 2],
    rotations: u64,
}

impl ClaimHistory {
    fn new() -> Self {
        let mut credited = [[0; 3]; 5];
        credited[2] = [EARNINGS, 0, BACKING];
        credited[4][INSURER] = INSURANCE;
        Self {
            credited,
            transferred_out: [[0; 3]; 5],
            paid: [[0; 3]; 5],
            holders: [2, 4],
            rotations: 0,
        }
    }

    fn owner(&self, class: usize) -> usize {
        self.holders[if class == PRINCIPAL { FEES } else { class }]
    }

    fn claim(&self, actor: usize, class: usize) -> u64 {
        self.credited[actor][class] - self.transferred_out[actor][class] - self.paid[actor][class]
    }

    fn remaining(&self) -> [u64; 3] {
        std::array::from_fn(|class| (0..5).map(|actor| self.claim(actor, class)).sum())
    }

    fn balances(&self) -> [u64; 5] {
        std::array::from_fn(|actor| {
            self.paid[actor].iter().sum::<u64>() + if actor < 2 { PAYOUTS[actor] } else { 0 }
        })
    }

    fn accepts(&self, balances: [u64; 5], reserves: [u64; 3]) -> bool {
        balances == self.balances() && reserves == self.remaining()
    }

    fn transfer(&mut self, role: usize, to: usize) {
        let from = self.holders[role];
        assert_ne!(from, to);
        for class in 0..3 {
            if class == role || (role == FEES && class == PRINCIPAL) {
                let unpaid = self.claim(from, class);
                assert!(unpaid > 0, "every handoff transfers real unpaid value");
                self.transferred_out[from][class] += unpaid;
                self.credited[to][class] += unpaid;
            }
        }
        self.holders[role] = to;
        self.rotations += 1;
    }

    fn pay(&mut self, class: usize, actor: usize, amount: u64) {
        assert_eq!(actor, self.owner(class));
        assert!(amount > 0 && amount <= self.claim(actor, class));
        self.paid[actor][class] += amount;
    }

    fn check(&self, world: &TerminalEarningsWorld, ledgers: [[Pubkey; 2]; 3], epoch: u64) {
        let env = &world.env;
        let remaining = self.remaining();
        for class in 0..3 {
            assert_eq!(
                remaining[class] + self.paid.iter().map(|paid| paid[class]).sum::<u64>(),
                STOCK[class]
            );
            for actor in 0..5 {
                if actor != self.owner(class) {
                    assert_eq!(self.claim(actor, class), 0);
                }
            }
        }
        let balances = world.tokens.map(|key| env.token_amount(key));
        let (_, group) = env.market_state();
        let bucket = group.source_backing_buckets[1];
        assert!(self.accepts(
            balances,
            [
                u64::try_from(bucket.utilization_fee_earnings).unwrap(),
                u64::try_from(group.insurance).unwrap(),
                u64::try_from(bucket.fresh_unliened_backing_num / BOUND_SCALE).unwrap(),
            ]
        ));
        Entitlements {
            remaining: [remaining[FEES], remaining[INSURER]],
            paid: self.balances(),
            holders: self.holders,
            principal: remaining[PRINCIPAL],
            rotations: self.rotations,
        }
        .check(world);
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap();
        assert_eq!(profile.asset_admin, world.wallets[4].to_bytes());
        assert_eq!(profile.insurance_operator, world.wallets[3].to_bytes());
        assert_eq!(profile.backing_trade_fee_bps_short, RATE);
        assert_eq!(profile.backing_trade_fee_insurance_share_bps_short, 0);
        assert_eq!(
            profile.backing_bucket_authority,
            world.wallets[self.holders[FEES]].to_bytes()
        );
        assert_eq!(
            profile.insurance_authority,
            world.wallets[self.holders[INSURER]].to_bytes()
        );
        assert_eq!(
            env.control_sequences(0).authority_epoch,
            epoch + self.rotations
        );
        for actor in 2..5 {
            for role in [FEES, INSURER] {
                let account = env.svm.get_account(&ledgers[actor - 2][role]).unwrap();
                if self.paid[actor][role] == 0 {
                    assert!(account.data.iter().all(|byte| *byte == 0));
                } else if role == FEES {
                    let ledger = state::read_backing_domain_ledger(&account.data).unwrap();
                    assert_eq!(ledger.market_group, env.market.to_bytes());
                    assert_eq!(ledger.authority, world.wallets[actor].to_bytes());
                    assert_eq!(ledger.domain, 1);
                    assert_eq!(
                        ledger.total_earnings_withdrawn_atoms,
                        self.paid[actor][role].into()
                    );
                } else {
                    let ledger = state::read_insurance_ledger(&account.data).unwrap();
                    assert_eq!(ledger.market_group, env.market.to_bytes());
                    assert_eq!(ledger.authority, world.wallets[actor].to_bytes());
                    assert_eq!(ledger.total_withdrawn_atoms, self.paid[actor][role].into());
                }
            }
        }
        let market = env.svm.get_account(&env.market).unwrap();
        crate::support::fuzz_model::assert_market_stock_census(
            "generated reserve entitlement",
            &group,
            &market.data,
            &[],
            remaining.iter().map(|v| u128::from(*v)).sum(),
        )
        .unwrap();
    }
}

fn payment(
    world: &TerminalEarningsWorld,
    book: &ClaimHistory,
    ledgers: [[Pubkey; 2]; 3],
    class: usize,
    amount: u64,
    epoch: u64,
) -> Instruction {
    let actor = book.owner(class);
    let ledger = ledgers[actor - 2][if class == PRINCIPAL { FEES } else { class }];
    let mut ix = payout(world, class, actor, amount, epoch + book.rotations, ledger);
    if class != INSURER {
        ix.accounts[0].is_signer = false;
    }
    if class == INSURER {
        ix.accounts.push(AccountMeta::new(ledger, false));
    }
    assert_eq!(
        ix.accounts.iter().any(|meta| meta.is_signer),
        class == INSURER
    );
    ix
}

fn submit_payments(
    world: &mut TerminalEarningsWorld,
    ledgers: &[Pubkey],
    instructions: &[Instruction],
    submitter: Option<usize>,
) -> u64 {
    let payer = world.env.payer.insecure_clone();
    if let Some(actor) = submitter {
        world.env.payer = match actor {
            2 => world.incumbent.insecure_clone(),
            3 => world.successor.insecure_clone(),
            4 => world.admin.insecure_clone(),
            _ => unreachable!(),
        };
    }
    let cu = land(world, ledgers, instructions, None);
    world.env.payer = payer;
    cu
}

#[test]
fn v16_program_generated_role_histories_preserve_owner_and_reserve_entitlement() {
    let mut peak = 0;
    let mut rollbacks = 0;
    let mut generated_handoffs = std::collections::BTreeSet::new();
    for seed in SEEDS {
        let mut outcomes = Vec::new();
        for split_reversed in [false, true] {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut world = terminal_earnings_world();
            let ledgers: [[Pubkey; 2]; 3] = std::array::from_fn(|_| {
                std::array::from_fn(|role| {
                    let key = Keypair::new();
                    system_create_account_for_test(
                        &mut world.env.svm,
                        &world.env.payer,
                        &key,
                        if role == FEES {
                            state::backing_domain_ledger_account_len()
                        } else {
                            state::insurance_ledger_account_len()
                        },
                        world.env.program_id,
                    );
                    key.pubkey()
                })
            });
            let tracked = ledgers.into_iter().flatten().collect::<Vec<_>>();
            let epoch = world.env.control_sequences(0).authority_epoch;
            let mut book = ClaimHistory::new();
            book.check(&world, ledgers, epoch);
            // The prefix guarantees a funded merge, split and return; subsequent
            // handoffs and every payout partition come from the input seed.
            for step in 0..9 {
                let (role, to) = match step {
                    0 => (INSURER, 2),
                    1 => (FEES, 3),
                    2 => (FEES, 2),
                    _ => {
                        let role = rng.gen_range(0..2);
                        let candidates = (2..5)
                            .filter(|actor| *actor != book.holders[role])
                            .collect::<Vec<_>>();
                        (role, candidates[rng.gen_range(0..candidates.len())])
                    }
                };
                let from = book.holders[role];
                generated_handoffs.insert((role, from, to));
                let ix = rotate(&world, role, from, to, epoch + book.rotations);
                peak = peak.max(land(&mut world, &tracked, &[ix], None));
                book.transfer(role, to);
                book.check(&world, ledgers, epoch);

                if step == 0 {
                    let prefix = payment(&world, &book, ledgers, INSURER, 1, epoch);
                    let cold_admin = rotate(&world, INSURER, 4, 3, epoch + book.rotations);
                    peak = peak.max(land(
                        &mut world,
                        &tracked,
                        &[prefix.clone(), cold_admin],
                        Some((3, PercolatorError::EngineLockActive, 1)),
                    ));
                    rollbacks += 1;
                    book.check(&world, ledgers, epoch);
                    peak = peak.max(submit_payments(&mut world, &tracked, &[prefix], Some(4)));
                    book.pay(INSURER, 2, 1);
                    book.check(&world, ledgers, epoch);

                    let remaining = book.remaining();
                    assert!(remaining[FEES] + 1 < remaining.iter().sum());
                    let over_budget =
                        payment(&world, &book, ledgers, FEES, remaining[FEES] + 1, epoch);
                    peak = peak.max(land(
                        &mut world,
                        &tracked,
                        &[over_budget],
                        Some((2, PercolatorError::EngineLockActive, 0)),
                    ));
                    rollbacks += 1;
                    book.check(&world, ledgers, epoch);
                }

                let amounts: [u64; 3] = book.remaining().map(|remaining| {
                    if step == 8 {
                        remaining
                    } else {
                        rng.gen_range(1..=std::cmp::max(1, remaining / 4))
                    }
                });
                let submitter = split_reversed.then_some(from);
                if split_reversed {
                    for class in [PRINCIPAL, INSURER, FEES] {
                        for amount in [amounts[class] / 2, amounts[class] - amounts[class] / 2] {
                            if amount == 0 {
                                continue;
                            }
                            let actor = book.owner(class);
                            let ix = payment(&world, &book, ledgers, class, amount, epoch);
                            peak =
                                peak.max(submit_payments(&mut world, &tracked, &[ix], submitter));
                            book.pay(class, actor, amount);
                            book.check(&world, ledgers, epoch);
                        }
                    }
                } else {
                    for classes in [&[FEES, INSURER][..], &[PRINCIPAL][..]] {
                        let ixs = classes
                            .iter()
                            .map(|&class| {
                                payment(&world, &book, ledgers, class, amounts[class], epoch)
                            })
                            .collect::<Vec<_>>();
                        peak = peak.max(submit_payments(&mut world, &tracked, &ixs, submitter));
                        for &class in classes {
                            book.pay(class, book.owner(class), amounts[class]);
                        }
                        book.check(&world, ledgers, epoch);
                    }
                }
                if step == 1 {
                    let balances = book.balances();
                    assert!(balances[2] > 0 && balances[3] > 0);
                    let mut wrong_owner = balances;
                    wrong_owner[2] -= 1;
                    wrong_owner[3] += 1;
                    assert_eq!(
                        wrong_owner.iter().sum::<u64>(),
                        balances.iter().sum::<u64>()
                    );
                    assert!(!book.accepts(wrong_owner, book.remaining()));
                    let mut wrong_reserve = book.remaining();
                    wrong_reserve[FEES] -= 1;
                    wrong_reserve[INSURER] += 1;
                    assert_eq!(
                        wrong_reserve.iter().sum::<u64>(),
                        book.remaining().iter().sum::<u64>()
                    );
                    assert!(!book.accepts(balances, wrong_reserve));
                }
            }
            assert_eq!(book.remaining(), [0; 3]);
            assert_eq!(book.balances().iter().sum::<u64>(), SUPPLY);
            outcomes.push(book);
        }
        assert_eq!(
            outcomes[0], outcomes[1],
            "seed {seed}: partition/order/submitter"
        );
    }
    assert_eq!(
        generated_handoffs.len(),
        12,
        "both roles traverse every distinct owner pair"
    );
    assert_eq!(rollbacks, 4 * SEEDS.len());
    eprintln!(
        "INV-024/036/041 generated reserves: seeds={} worlds={} handoffs={} exact_rollbacks={rollbacks} peak_CU={peak}",
        SEEDS.len(), 2 * SEEDS.len(), 18 * SEEDS.len()
    );
}
