//! INV-024 owns generated owner/class attribution across terminal role returns
//! and principal expiry. INV-005/025/036/070/081 receive role, stock, fee, disposal
//! and Account-frame checks; INV-027 receives only a settled-user principal frame.
//! Finite public SPL histories, not generic invariant or coverage-row closure.

use super::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use solana_sdk::account::Account;

const OPERATOR: usize = 2;
const INITIAL: [usize; 3] = [2, 4, 3];
const OUTBOUND: [usize; 3] = [3, 3, 2];
const SEEDS: [u64; 2] = [0x5eed, 0xcafe];
const ORDERS: [[usize; 3]; 6] = [
    [FEES, INSURER, OPERATOR],
    [FEES, OPERATOR, INSURER],
    [INSURER, FEES, OPERATOR],
    [INSURER, OPERATOR, FEES],
    [OPERATOR, FEES, INSURER],
    [OPERATOR, INSURER, FEES],
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Claims {
    credited: [[u64; 3]; 5],
    transferred: [[u64; 3]; 5],
    paid: [[u64; 3]; 5],
    retired: [u64; 5],
    observed: [[Option<u64>; 2]; 5],
    insurance_observed_loss: [u64; 5],
    holders: [usize; 3],
    rotations: u64,
    expired: bool,
    normalized: bool,
}

impl Claims {
    fn new() -> Self {
        let mut credited = [[0; 3]; 5];
        credited[2] = [EARNINGS, 0, BACKING];
        credited[4][INSURER] = INSURANCE;
        Self {
            credited,
            transferred: [[0; 3]; 5],
            paid: [[0; 3]; 5],
            retired: [0; 5],
            observed: [[None; 2]; 5],
            insurance_observed_loss: [0; 5],
            holders: INITIAL,
            rotations: 0,
            expired: false,
            normalized: false,
        }
    }

    fn owner(&self, class: usize) -> usize {
        self.holders[if class == PRINCIPAL { FEES } else { class }]
    }

    fn claim(&self, actor: usize, class: usize) -> u64 {
        self.credited[actor][class]
            - self.transferred[actor][class]
            - self.paid[actor][class]
            - if class == PRINCIPAL {
                self.retired[actor]
            } else {
                0
            }
    }

    fn remaining(&self, class: usize) -> u64 {
        (0..5).map(|actor| self.claim(actor, class)).sum()
    }

    fn retire(&mut self) {
        assert!(!self.expired);
        let owner = self.owner(PRINCIPAL);
        let amount = self.remaining(PRINCIPAL);
        assert!(amount > 0);
        self.retired[owner] += amount;
        self.expired = true;
    }

    fn transfer(&mut self, role: usize, to: usize) {
        let from = self.holders[role];
        assert_ne!(from, to);
        for class in 0..3 {
            if role != OPERATOR && (class == role || (role == FEES && class == PRINCIPAL)) {
                let amount = self.claim(from, class);
                if class != PRINCIPAL {
                    assert!(amount > 0, "every beneficiary handoff has unpaid value");
                }
                self.transferred[from][class] += amount;
                self.credited[to][class] += amount;
            }
        }
        self.holders[role] = to;
        self.rotations += 1;
    }

    fn pay(&mut self, class: usize, amount: u64) {
        let actor = self.owner(class);
        assert!(amount > 0 && amount <= self.claim(actor, class));
        if class == INSURER {
            if let Some(last) = self.observed[actor][class] {
                // The optional aggregate ledger observes payouts to intervening
                // holders on return; that counter grants no additional claim.
                self.insurance_observed_loss[actor] += last - self.remaining(class);
            }
        }
        self.paid[actor][class] += amount;
        if class != PRINCIPAL {
            self.observed[actor][class] = Some(self.remaining(class));
        }
    }

    fn balances(&self) -> [u64; 5] {
        std::array::from_fn(|actor| {
            self.paid[actor].iter().sum::<u64>() + if actor < 2 { PAYOUTS[actor] } else { 0 }
        })
    }

    fn residue(&self) -> u64 {
        self.retired.iter().sum()
    }

    fn accepts(&self, balances: [u64; 5], stocks: [u64; 4], holders: [usize; 3]) -> bool {
        balances == self.balances()
            && stocks
                == [
                    self.remaining(FEES),
                    self.remaining(INSURER),
                    self.remaining(PRINCIPAL),
                    self.residue(),
                ]
            && holders == self.holders
    }
}

struct Frames {
    market: Account,
    tokens: [Account; 5],
    vault: Account,
    ledgers: [[Pubkey; 2]; 3],
    empty: [[Account; 2]; 3],
}

impl Frames {
    fn new(world: &mut TerminalEarningsWorld) -> Self {
        let env = &mut world.env;
        let ledgers = std::array::from_fn(|_| {
            std::array::from_fn(|role| {
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    if role == FEES {
                        state::backing_domain_ledger_account_len()
                    } else {
                        state::insurance_ledger_account_len()
                    },
                    env.program_id,
                );
                key.pubkey()
            })
        });
        Self {
            market: env.svm.get_account(&env.market).unwrap(),
            tokens: world.tokens.map(|key| env.svm.get_account(&key).unwrap()),
            vault: env.svm.get_account(&env.vault).unwrap(),
            empty: ledgers.map(|pair| pair.map(|key| env.svm.get_account(&key).unwrap())),
            ledgers,
        }
    }

    fn check(&self, world: &TerminalEarningsWorld, book: &Claims, epoch: u64) {
        let env = &world.env;
        let market = env.svm.get_account(&env.market).unwrap();
        let (cfg, group) = state::read_market(&market.data).unwrap();
        let (_, original) = state::read_market(&self.market.data).unwrap();
        assert_eq!(cfg.marketauth, world.admin.pubkey().to_bytes());
        assert_eq!(market.lamports, self.market.lamports);
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group.c_tot,
                group.pnl_pos_tot,
                group.materialized_portfolio_count
            ),
            (0, 0, 0)
        );
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
        assert!(group.insurance_domain_spent.iter().all(|v| *v == 0));
        assert_eq!(group.insurance_domain_budget[0], group.insurance);
        assert!(group.insurance_domain_budget[1..].iter().all(|v| *v == 0));
        let bucket = group.source_backing_buckets[1];
        let raw_principal = u64::try_from(bucket.fresh_unliened_backing_num / BOUND_SCALE).unwrap();
        // Clock expiry removes the economic claim before a route normalizes the
        // stale stock labels. Cleanup cannot transfer that value to a successor.
        let stale = if book.expired && !book.normalized {
            book.residue()
        } else {
            0
        };
        let fees = u64::try_from(bucket.utilization_fee_earnings).unwrap();
        let insurance = u64::try_from(group.insurance).unwrap();
        let observed_profile = state::read_asset_oracle_profile(&market.data, 0).unwrap();
        let holders = [
            observed_profile.backing_bucket_authority,
            observed_profile.insurance_authority,
            observed_profile.insurance_operator,
        ]
        .map(|key| {
            world
                .wallets
                .iter()
                .position(|wallet| wallet.to_bytes() == key)
                .unwrap()
        });
        assert!(book.accepts(
            world.tokens.map(|key| env.token_amount(key)),
            [
                fees,
                insurance,
                raw_principal - stale,
                u64::try_from(group.vault).unwrap() - fees - insurance - raw_principal + stale
            ],
            holders
        ));
        assert_eq!(
            bucket.fresh_unliened_backing_num,
            u128::from(book.remaining(PRINCIPAL) + stale) * BOUND_SCALE
        );
        assert_eq!(
            group.source_credit[1].fresh_reserved_backing_num,
            bucket.fresh_unliened_backing_num
        );
        assert_eq!(
            group.backing_provider_earnings_total,
            u128::from(book.remaining(FEES))
        );
        assert_eq!(
            bucket.consumed_liened_backing_num,
            u128::from(PROFIT) * BOUND_SCALE
        );
        assert_eq!(bucket.impaired_liened_backing_num, 0);
        assert_eq!(bucket.expiry_slot, 100);
        assert_eq!(
            bucket.status,
            if book.normalized {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        for domain in 0..group.source_credit.len() {
            if domain != 1 {
                assert_eq!(group.source_credit[domain], original.source_credit[domain]);
                assert_eq!(
                    group.source_backing_buckets[domain],
                    original.source_backing_buckets[domain]
                );
            }
        }
        let raw = (0..3).map(|class| book.remaining(class)).sum::<u64>() + book.residue();
        assert_eq!(group.vault, u128::from(raw));
        assert_eq!(raw + book.balances().iter().sum::<u64>(), SUPPLY);
        for class in 0..3 {
            assert_eq!(
                book.remaining(class)
                    + book.paid.iter().map(|p| p[class]).sum::<u64>()
                    + if class == PRINCIPAL {
                        book.residue()
                    } else {
                        0
                    },
                [EARNINGS, INSURANCE, BACKING][class]
            );
            for actor in 0..5 {
                if actor != book.owner(class) {
                    assert_eq!(book.claim(actor, class), 0);
                }
            }
        }
        for ((key, frame), amount) in world
            .tokens
            .into_iter()
            .zip(&self.tokens)
            .zip(book.balances())
            .chain(std::iter::once(((env.vault, &self.vault), raw)))
        {
            let mut expected = frame.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(env.svm.get_account(&key), Some(expected));
        }
        assert_eq!(
            env.svm.get_account(&env.mint),
            Some(world.mint_frame.clone())
        );
        let mut profile = state::read_asset_oracle_profile(&self.market.data, 0).unwrap();
        profile.backing_bucket_authority = world.wallets[book.holders[FEES]].to_bytes();
        profile.insurance_authority = world.wallets[book.holders[INSURER]].to_bytes();
        profile.insurance_operator = world.wallets[book.holders[OPERATOR]].to_bytes();
        assert_eq!(observed_profile, profile);
        assert_eq!(
            env.control_sequences(0).authority_epoch,
            epoch + book.rotations
        );
        for actor in 2..5 {
            for role in [FEES, INSURER] {
                let account = env.svm.get_account(&self.ledgers[actor - 2][role]).unwrap();
                let empty = &self.empty[actor - 2][role];
                let Some(remaining) = book.observed[actor][role] else {
                    assert_eq!(&account, empty);
                    continue;
                };
                assert_eq!(
                    (
                        account.owner,
                        account.lamports,
                        account.executable,
                        account.rent_epoch
                    ),
                    (
                        empty.owner,
                        empty.lamports,
                        empty.executable,
                        empty.rent_epoch
                    )
                );
                if role == FEES {
                    assert_eq!(
                        state::read_backing_domain_ledger(&account.data).unwrap(),
                        state::BackingDomainLedgerAccountV16 {
                            market_group: env.market.to_bytes(),
                            authority: world.wallets[actor].to_bytes(),
                            domain: 1,
                            total_earnings_withdrawn_atoms: book.paid[actor][role].into(),
                            last_observed_bucket_earnings_atoms: remaining.into(),
                            last_observed_unavailable_principal_atoms: PROFIT.into(),
                            ..Default::default()
                        }
                    );
                } else {
                    assert_eq!(
                        state::read_insurance_ledger(&account.data).unwrap(),
                        state::InsuranceLedgerAccountV16 {
                            market_group: env.market.to_bytes(),
                            authority: world.wallets[actor].to_bytes(),
                            total_withdrawn_atoms: book.paid[actor][role].into(),
                            cumulative_loss_atoms: book.insurance_observed_loss[actor].into(),
                            last_observed_insurance_atoms: remaining.into(),
                            ..Default::default()
                        }
                    );
                }
            }
        }
        assert_domain_budget_remaining_total_consistent(&group, "generated expiring roles");
        let mut data = market.data;
        state::market_view_mut(&mut data)
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
    }
}

fn payment(
    world: &TerminalEarningsWorld,
    frames: &Frames,
    book: &Claims,
    class: usize,
    amount: u64,
    epoch: u64,
) -> Instruction {
    let actor = book.owner(class);
    let ledger = frames.ledgers[actor - 2][if class == PRINCIPAL { FEES } else { class }];
    let mut ix = payout(world, class, actor, amount, epoch + book.rotations, ledger);
    ix.accounts[0].is_signer = false;
    if class == INSURER {
        ix.accounts.push(AccountMeta::new(ledger, false));
    }
    assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
    ix
}

fn cleanup(world: &TerminalEarningsWorld, book: &Claims, epoch: u64) -> Instruction {
    wrap(
        &world.env,
        ProgInstruction::CloseSlab {
            authority_epoch: epoch + book.rotations,
        },
        vec![
            AccountMeta::new(world.admin.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new(world.tokens[4], false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(world.env.mint, false),
        ],
    )
}

#[test]
fn v16_program_generated_expiring_role_returns_preserve_beneficiaries_through_cleanup() {
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut peak = 0;
    let mut checked = 0;
    let mut coalesced = 0;
    let mut operator_only = 0;
    let mut principal_owners = std::collections::BTreeSet::new();
    for seed in SEEDS {
        for order in ORDERS {
            for expire_after in [1, 3, 5] {
                let mut outcomes = Vec::new();
                for delayed_cleanup in [false, true] {
                    let mut rng = StdRng::seed_from_u64(seed);
                    let mut world = terminal_earnings_world();
                    let frames = Frames::new(&mut world);
                    let tracked = frames.ledgers.into_iter().flatten().collect::<Vec<_>>();
                    let epoch = world.env.control_sequences(0).authority_epoch;
                    let mut book = Claims::new();
                    let mut execute =
                        |world: &mut TerminalEarningsWorld,
                         ixs: &[Instruction],
                         submitter: Option<usize>,
                         rejection: Option<(u8, PercolatorError, usize)>| {
                            let payer = world.env.payer.insecure_clone();
                            if let Some(actor) = submitter {
                                world.env.payer = match actor {
                                    2 => world.incumbent.insecure_clone(),
                                    3 => world.successor.insecure_clone(),
                                    4 => world.admin.insecure_clone(),
                                    _ => unreachable!(),
                                };
                            }
                            rollbacks += usize::from(rejection.is_some());
                            peak = peak.max(land(world, &tracked, ixs, rejection));
                            checked += 1;
                            world.env.payer = payer;
                        };
                    frames.check(&world, &book, epoch);
                    let word = order.into_iter().chain(order.into_iter().rev());
                    for (step, role) in word.enumerate() {
                        if step == expire_after {
                            world
                                .env
                                .svm
                                .warp_to_slot(if delayed_cleanup { 103 } else { 100 });
                            book.retire();
                            frames.check(&world, &book, epoch);
                            if !delayed_cleanup {
                                let ix = cleanup(&world, &book, epoch);
                                execute(&mut world, &[ix], None, None);
                                book.normalized = true;
                                frames.check(&world, &book, epoch);
                            }
                        }
                        let from = book.holders[role];
                        let to = if step < 3 {
                            OUTBOUND[role]
                        } else {
                            INITIAL[role]
                        };
                        let kind = if role == OPERATOR {
                            processor::ASSET_AUTH_INSURANCE_OPERATOR
                        } else {
                            KINDS[role]
                        };
                        let handoff = wrap(
                            &world.env,
                            ProgInstruction::UpdateAssetAuthority {
                                asset_index: 0,
                                market_id: world.env.asset_market_id(0),
                                authority_epoch: epoch + book.rotations,
                                kind,
                                new_pubkey: world.wallets[to].to_bytes(),
                            },
                            vec![
                                AccountMeta::new(world.wallets[from], true),
                                AccountMeta::new_readonly(world.wallets[to], true),
                                AccountMeta::new(world.env.market, false),
                            ],
                        );
                        let mut next = book.clone();
                        next.transfer(role, to);
                        if step == expire_after {
                            let mut prefix = vec![handoff];
                            if delayed_cleanup {
                                prefix.push(cleanup(&world, &next, epoch));
                            }
                            prefix.push(payment(&world, &frames, &next, FEES, 1, epoch));
                            next.normalized = true;
                            next.pay(FEES, 1);
                            let mut rejected = prefix.clone();
                            let wrong = (2..5).find(|actor| *actor != next.owner(FEES)).unwrap();
                            let mut wrong_owner = payment(&world, &frames, &next, FEES, 1, epoch);
                            wrong_owner.accounts[0].pubkey = world.wallets[wrong];
                            rejected.push(wrong_owner);
                            execute(
                                &mut world,
                                &rejected,
                                delayed_cleanup.then_some(from),
                                Some((2 + prefix.len() as u8, PercolatorError::Unauthorized, 1)),
                            );
                            frames.check(&world, &book, epoch);
                            execute(&mut world, &prefix, delayed_cleanup.then_some(from), None);
                            book = next;
                            frames.check(&world, &book, epoch);
                            let insurance = payment(&world, &frames, &book, INSURER, 1, epoch);
                            let expired = payment(&world, &frames, &book, PRINCIPAL, 1, epoch);
                            execute(
                                &mut world,
                                &[insurance.clone(), expired],
                                None,
                                Some((3, PercolatorError::EngineStale, 1)),
                            );
                            frames.check(&world, &book, epoch);
                            execute(
                                &mut world,
                                &[insurance],
                                delayed_cleanup.then_some(book.holders[OPERATOR]),
                                None,
                            );
                            book.pay(INSURER, 1);
                        } else {
                            execute(&mut world, &[handoff], None, None);
                            book = next;
                        }
                        frames.check(&world, &book, epoch);
                        coalesced += usize::from(book.holders[FEES] == book.holders[INSURER]);
                        operator_only += usize::from(
                            book.holders[OPERATOR] != book.holders[FEES]
                                && book.holders[OPERATOR] != book.holders[INSURER],
                        );
                        let amounts: [u64; 3] = std::array::from_fn(|class| {
                            if class == PRINCIPAL && book.expired {
                                0
                            } else {
                                rng.gen_range(1..=(book.remaining(class) / 4).max(1))
                            }
                        });
                        let classes = if delayed_cleanup {
                            [PRINCIPAL, INSURER, FEES]
                        } else {
                            [FEES, INSURER, PRINCIPAL]
                        };
                        for class in classes {
                            if amounts[class] == 0 {
                                continue;
                            }
                            let actor = book.owner(class);
                            let ix = payment(&world, &frames, &book, class, amounts[class], epoch);
                            let submitter = delayed_cleanup.then_some(match step % 3 {
                                0 => from,
                                1 => book.holders[OPERATOR],
                                _ => actor,
                            });
                            execute(&mut world, &[ix], submitter, None);
                            book.pay(class, amounts[class]);
                            if class == PRINCIPAL {
                                principal_owners.insert(actor);
                            }
                            frames.check(&world, &book, epoch);
                        }
                    }
                    assert_eq!(book.holders, INITIAL);
                    let stocks = [
                        book.remaining(FEES),
                        book.remaining(INSURER),
                        0,
                        book.residue(),
                    ];
                    let mut wrong_owner = book.balances();
                    wrong_owner[2] -= 1;
                    wrong_owner[3] += 1;
                    assert_eq!(
                        wrong_owner.iter().sum::<u64>(),
                        book.balances().iter().sum::<u64>()
                    );
                    assert!(!book.accepts(wrong_owner, stocks, book.holders));
                    let mut wrong_class = stocks;
                    wrong_class[3] -= 1;
                    wrong_class[FEES] += 1;
                    assert_eq!(wrong_class.iter().sum::<u64>(), stocks.iter().sum::<u64>());
                    assert!(!book.accepts(book.balances(), wrong_class, book.holders));
                    let mut wrong_role = book.holders;
                    wrong_role.swap(INSURER, OPERATOR);
                    assert!(!book.accepts(book.balances(), stocks, wrong_role));
                    for class in if delayed_cleanup {
                        [INSURER, FEES]
                    } else {
                        [FEES, INSURER]
                    } {
                        let amount = book.remaining(class);
                        let ix = payment(&world, &frames, &book, class, amount, epoch);
                        execute(
                            &mut world,
                            &[ix],
                            delayed_cleanup.then_some(book.holders[OPERATOR]),
                            None,
                        );
                        book.pay(class, amount);
                        frames.check(&world, &book, epoch);
                    }
                    let tokens = world.tokens.map(|key| world.env.svm.get_account(&key));
                    let ledgers = frames
                        .ledgers
                        .map(|pair| pair.map(|key| world.env.svm.get_account(&key)));
                    let rent = world
                        .env
                        .svm
                        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                    let mut admin = world.env.svm.get_account(&world.admin.pubkey()).unwrap();
                    admin.lamports += frames.market.lamports + frames.vault.lamports - rent;
                    let ix = cleanup(&world, &book, epoch);
                    execute(&mut world, &[ix], None, None);
                    assert_eq!(
                        world.env.svm.get_account(&world.admin.pubkey()),
                        Some(admin)
                    );
                    let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
                    assert_closed_market_tombstone(&tombstone);
                    assert_eq!(tombstone.lamports, rent);
                    assert!(world
                        .env
                        .svm
                        .get_account(&world.env.vault)
                        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                    assert_eq!(
                        world.tokens.map(|key| world.env.svm.get_account(&key)),
                        tokens
                    );
                    assert_eq!(
                        frames
                            .ledgers
                            .map(|pair| pair.map(|key| world.env.svm.get_account(&key))),
                        ledgers
                    );
                    let mut mint_frame = world.mint_frame.clone();
                    let mut mint = Mint::unpack(&mint_frame.data).unwrap();
                    mint.supply = SUPPLY - book.residue();
                    Mint::pack(mint, &mut mint_frame.data).unwrap();
                    assert_eq!(world.env.svm.get_account(&world.env.mint), Some(mint_frame));
                    assert_eq!(book.balances().iter().sum::<u64>(), SUPPLY - book.residue());
                    outcomes.push(book);
                    worlds += 1;
                }
                assert_eq!(
                    outcomes[0], outcomes[1],
                    "seed={seed}, order={order:?}, expire_after={expire_after}"
                );
            }
        }
    }
    assert_eq!(worlds, 72);
    assert_eq!(rollbacks, 2 * worlds);
    assert_eq!(principal_owners, [2, 3].into_iter().collect());
    assert!(coalesced > 0 && operator_only > 0);
    eprintln!("INV-024 generated expiring roles: worlds={worlds}, handoffs={}, checked_transactions={checked}, exact_rollbacks={rollbacks}, coalesced_prefixes={coalesced}, operator_only_prefixes={operator_only}, peak_CU={peak}, limit={CU_LIMIT}", 6 * worlds);
}
