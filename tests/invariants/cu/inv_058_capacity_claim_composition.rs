//! INV-058/028: historical claim capacity composes with shared side-OI headroom.
//! Public, solvent, integral-mark histories; the independent owner/domain book
//! follows both reserved domains through materialization and terminal redemption.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use std::collections::BTreeSet;

const SLOTS: usize = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize;
const DOMAINS: usize = percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS;
const SHARED: usize = SLOTS - 1;
const LIMIT: u64 = 1_375_000;

#[derive(Default)]
struct Measurements {
    submissions: usize,
    rollbacks: usize,
    paying_rollbacks: usize,
    terminal: usize,
    peak: u64,
    packet: usize,
}

impl Measurements {
    fn submit(
        &mut self,
        w: &mut World,
        instructions: &[Instruction],
        changed: &[Pubkey],
        abort: bool,
    ) {
        let mut ixs = instructions.to_vec();
        if abort {
            ixs.push(system_instruction::transfer(
                &w.env.payer.pubkey(),
                &w.owners[0].pubkey(),
                u64::MAX,
            ));
        }
        let tx = w.transaction(&ixs);
        let bytes = bincode::serialize(&tx).unwrap().len();
        assert!(bytes <= 1232);
        self.packet = self.packet.max(bytes);
        if instructions
            .iter()
            .all(|ix| ix.accounts.iter().all(|a| !a.is_signer))
        {
            assert_eq!(tx.signatures.len(), 1, "payer-only economic continuation");
        }
        let before = frame(&w.env, &tx, &w.keys());
        let fee = FeeStructure::default().lamports_per_signature * tx.signatures.len() as u64;
        let result = w.env.svm.send_transaction(tx);
        let meta = if abort {
            let error = result.expect_err("ordinary System suffix aborts the completed prefix");
            assert_eq!(
                error.err,
                TransactionError::InstructionError(
                    instructions.len() as u8 + 2,
                    InstructionError::Custom(1)
                ),
                "submission {}: {error:?}",
                self.submissions
            );
            check_frame(&w.env, before, fee, &[]);
            self.rollbacks += 1;
            self.paying_rollbacks += usize::from(
                error
                    .meta
                    .logs
                    .iter()
                    .any(|line| line.contains("Instruction: Transfer")),
            );
            error.meta
        } else {
            let meta = result
                .unwrap_or_else(|e| panic!("capacity/OI submission {}: {e:?}", self.submissions));
            check_frame(&w.env, before, fee, changed);
            meta
        };
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {} success", w.env.program_id))
                .count(),
            instructions.len()
        );
        self.measure(meta.compute_units_consumed);
        self.submissions += 1;
    }

    fn measure(&mut self, cu: u64) {
        assert_cu_within("capacity/side-OI claim composition", cu, LIMIT);
        self.peak = self.peak.max(cu);
    }
}

struct Book {
    positions: [[i128; SLOTS]; ACTORS],
    claims: [[u128; DOMAINS]; ACTORS],
    losses: [u128; ACTORS],
    prices: [u64; SLOTS],
    generations: [u64; SLOTS],
    ids: [u64; ACTORS],
    epochs: [u64; ACTORS],
    slot: u64,
}

impl Book {
    fn new(w: &World) -> Self {
        assert_eq!(DOMAINS, 2 * SLOTS);
        Self {
            positions: [[0; SLOTS]; ACTORS],
            claims: [[0; DOMAINS]; ACTORS],
            losses: [0; ACTORS],
            prices: [PRICE; SLOTS],
            generations: std::array::from_fn(|a| w.env.asset_market_id(a as u16)),
            ids: w.portfolios.map(|p| w.env.portfolio_id(p)),
            epochs: w.portfolios.map(|p| w.env.portfolio_position_epoch(p)),
            slot: 0,
        }
    }

    fn entitlement(&self) -> [u128; ACTORS] {
        std::array::from_fn(|a| CAPITAL + self.claims[a].iter().sum::<u128>() - self.losses[a])
    }

    fn claims_match(&self, actor: usize, observed: [u128; DOMAINS], terminal: bool) -> bool {
        observed.into_iter().enumerate().all(|(domain, claim)| {
            claim == self.claims[actor][domain] * BOUND_SCALE || (terminal && claim == 0)
        })
    }

    fn record(&mut self, pair: usize, asset: usize, q: i128) {
        self.positions[pair][asset] += q;
        self.positions[pair + 1][asset] -= q;
        self.epochs[pair] += 1;
        self.epochs[pair + 1] += 1;
    }

    fn census(&self, w: &World) {
        let group = w.env.market_state().1;
        let accounts = w.portfolios.map(|p| w.env.portfolio_state(p));
        let market = w.env.svm.get_account(&w.env.market).unwrap();
        assert_market_stock_census(
            "capacity/OI",
            &group,
            &market.data,
            &accounts,
            w.env.token_amount(w.env.vault) as u128,
        )
        .unwrap();
        assert_reservation_encumbrance_census("capacity/OI", &group, &accounts).unwrap();
        assert_source_credit_rates("capacity/OI", &group).unwrap();
    }

    fn check(&self, w: &World, terminal: bool) {
        let group = w.env.market_state().1;
        let accounts = w.portfolios.map(|p| w.env.portfolio_state(p));
        let entitlement = self.entitlement();
        let mut remaining = [0; DOMAINS];
        let mut oi = [[0u128; 2]; SLOTS];
        let mut counts = [[0u64; 2]; SLOTS];
        let mut paid = 0;
        for actor in 0..ACTORS {
            let p = &accounts[actor];
            let data = w.env.svm.get_account(&w.portfolios[actor]).unwrap().data;
            let (provenance, owner) = state::read_portfolio_owner_preflight(&data).unwrap();
            assert_eq!(provenance.market_group_id, w.env.market.to_bytes());
            assert_eq!(
                provenance.portfolio_account_id,
                w.portfolios[actor].to_bytes()
            );
            assert_eq!(owner, w.owners[actor].pubkey().to_bytes());
            assert_eq!(w.env.portfolio_id(w.portfolios[actor]), self.ids[actor]);
            if !terminal {
                assert_eq!(
                    w.env.portfolio_position_epoch(w.portfolios[actor]),
                    self.epochs[actor]
                );
            }
            assert_eq!(resolved_receipt(p), ResolvedPayoutReceiptV16::EMPTY);
            assert_eq!((p.reserved_pnl.get(), p.fee_credits.get()), (0, 0));
            assert!(p.pnl.get() >= 0);
            let tokens = w.env.token_amount(w.tokens[actor]) as u128;
            assert_eq!(
                p.capital.get() + p.pnl.get() as u128 + tokens,
                entitlement[actor]
            );
            paid += tokens;
            let mut observed = [0; DOMAINS];
            let mut resources = BTreeSet::new();
            for source in p.source_domains.iter().filter(|s| s.is_occupied()) {
                let domain = source.domain.get() as usize;
                assert!(domain < DOMAINS && resources.insert(domain));
                assert_eq!(
                    source.source_claim_market_id.get(),
                    self.generations[domain / 2]
                );
                let claim = source.source_claim_bound_num.get();
                assert!(claim > 0);
                assert_eq!(claim, self.claims[actor][domain] * BOUND_SCALE);
                assert_eq!(source.source_claim_liened_num.get(), 0);
                assert_eq!(source.source_lien_counterparty_backing_num.get(), 0);
                assert_eq!(source.source_lien_insurance_backing_num.get(), 0);
                observed[domain] = claim;
                remaining[domain] += claim;
            }
            assert_eq!(
                p.pnl.get() as u128 * BOUND_SCALE,
                observed.iter().sum::<u128>()
            );
            assert!(self.claims_match(actor, observed, terminal));
            if !terminal {
                assert_eq!(tokens, 0);
                assert_eq!(p.capital.get(), CAPITAL - self.losses[actor]);
            }
            let mut legs = 0;
            for asset in 0..SLOTS {
                let expected = self.positions[actor][asset];
                if has_active_leg_for_asset(p, asset) {
                    let leg = active_leg_for_asset(p, asset);
                    assert_ne!(expected, 0);
                    assert_eq!(leg.basis_pos_q, expected);
                    assert_eq!(leg.market_id, self.generations[asset]);
                    assert_eq!(leg.a_basis, ADL_ONE);
                    assert!(expected.unsigned_abs() < percolator::MAX_POSITION_ABS_Q);
                    resources.extend([2 * asset, 2 * asset + 1]);
                    let side = usize::from(expected < 0);
                    oi[asset][side] += expected.unsigned_abs();
                    counts[asset][side] += 1;
                    legs += 1;
                } else if !terminal {
                    assert_eq!(expected, 0);
                }
            }
            assert_eq!(percolator::active_bitmap_count_ones(active_bitmap(p)), legs);
            assert!(resources.len() <= DOMAINS);
            if !terminal && self.positions[0][SHARED] != 0 && actor == 0 {
                assert_eq!(
                    resources.len(),
                    DOMAINS,
                    "all future source slots are reserved"
                );
            }
        }
        for asset in 0..SLOTS {
            let a = group.assets[asset];
            assert_eq!(a.market_id, self.generations[asset]);
            assert_eq!(a.effective_price, self.prices[asset]);
            assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi[asset]);
            assert_eq!(
                [a.stored_pos_count_long, a.stored_pos_count_short],
                counts[asset]
            );
            assert!(oi[asset].iter().all(|q| *q <= percolator::MAX_OI_SIDE_Q));
            assert_eq!([a.a_long, a.a_short], [ADL_ONE; 2]);
        }
        for (domain, bound) in remaining.into_iter().enumerate() {
            let source = group.source_credit[domain];
            let bucket = group.source_backing_buckets[domain];
            assert_eq!(source.positive_claim_bound_num, bound);
            assert_eq!(source.exact_positive_claim_num, bound);
            assert_eq!(source.fresh_reserved_backing_num, bound);
            assert_eq!(bucket.fresh_unliened_backing_num, bound);
            let original = self
                .claims
                .iter()
                .map(|claims| claims[domain])
                .sum::<u128>()
                * BOUND_SCALE;
            assert_eq!(bucket.consumed_liened_backing_num, original - bound);
            assert_eq!(source.provider_receivable_num, original - bound);
            assert_eq!(source.spent_backing_num, original - bound);
            assert_eq!(source.insurance_credit_reserved_num, 0);
        }
        assert_eq!(group.insurance, 0);
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert_eq!(group.vault, SUPPLY - paid);
        assert_eq!(w.env.token_amount(w.env.vault) as u128, group.vault);
        let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
        assert_eq!(
            (mint.supply as u128, mint.mint_authority),
            (SUPPLY, COption::None)
        );
        self.census(w);
    }

    fn mark(
        &mut self,
        w: &mut World,
        m: &mut Measurements,
        asset: usize,
        price: u64,
        order: [usize; ACTORS],
    ) {
        let delta = price as i128 - self.prices[asset] as i128;
        for actor in 0..ACTORS {
            let q = self.positions[actor][asset];
            assert_eq!(q % POS_SCALE as i128, 0);
            let gain = q / POS_SCALE as i128 * delta;
            if gain > 0 {
                self.claims[actor][2 * asset + usize::from(q > 0)] += gain as u128;
            } else {
                self.losses[actor] += gain.unsigned_abs();
            }
        }
        self.slot += 1;
        self.prices[asset] = price;
        w.env.svm.warp_to_slot(self.slot);
        m.measure(
            w.env
                .push_auth_mark_for_asset_as_admin(asset as u16, self.slot, price),
        );
        for actor in order {
            if self.positions[actor][asset] == 0 {
                continue;
            }
            let rank = |w: &World| {
                let p = w.env.portfolio_state(w.portfolios[actor]);
                u128::from(self.slot - w.env.market_state().1.assets[asset].slot_last)
                    + p.pnl
                        .get()
                        .abs_diff(self.claims[actor].iter().sum::<u128>() as i128)
                    + p.capital.get().abs_diff(CAPITAL - self.losses[actor])
            };
            for _ in 0..4 {
                let before = rank(w);
                if before == 0 {
                    break;
                }
                m.measure(w.env.crank(
                    w.portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: self.slot,
                        observations: crank_observations(asset as u16),
                    },
                ));
                assert!(
                    rank(w) < before,
                    "settlement decreases authenticated accrual or owner debt"
                );
                self.census(w);
            }
            assert_eq!(rank(w), 0, "four-call settlement bound");
        }
        self.check(w, false);
    }
}

fn trade(w: &World, pair: usize, asset: usize, q: i128, price: u64, batch: bool) -> Instruction {
    let a = w.portfolios[pair];
    let b = w.portfolios[pair + 1];
    let data = if batch {
        w.env.batch_trade_no_cpi_ix(
            a,
            b,
            vec![BatchTradeLeg {
                asset_index: asset as u16,
                market_id: w.env.asset_market_id(asset as u16),
                size_q: q,
                exec_price: price,
                fee_bps: 0,
            }],
        )
    } else {
        w.env.trade_no_cpi_ix(a, b, asset as u16, q, price, 0)
    };
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new(w.owners[pair].pubkey(), true),
            AccountMeta::new(w.owners[pair + 1].pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
        ],
        data: data.encode(),
    }
}

fn fill(
    w: &mut World,
    b: &mut Book,
    m: &mut Measurements,
    pair: usize,
    asset: usize,
    q: i128,
    batch: bool,
) {
    let ix = trade(w, pair, asset, q, b.prices[asset], batch);
    let changed = [w.env.market, w.portfolios[pair], w.portfolios[pair + 1]];
    m.submit(w, &[ix], &changed, false);
    b.record(pair, asset, q);
    b.check(w, false);
}

fn terminal(w: &mut World, b: &Book, m: &mut Measurements, order: [usize; ACTORS]) {
    let portfolios = w.portfolios.map(|p| w.env.svm.get_account(&p));
    m.measure(w.env.resolve());
    assert_eq!(w.portfolios.map(|p| w.env.svm.get_account(&p)), portfolios);
    assert_eq!(w.env.market_state().1.mode, MarketModeV16::Resolved);
    w.env.svm.warp_to_slot(b.slot + 5);
    let expected = b.entitlement();
    let rank = |w: &World, actor: usize| {
        let p = w.env.portfolio_state(w.portfolios[actor]);
        (
            percolator::active_bitmap_count_ones(active_bitmap(&p)),
            p.source_domains.iter().filter(|s| s.is_occupied()).count(),
            expected[actor] - w.env.token_amount(w.tokens[actor]) as u128,
        )
    };
    // A flat claimant waits for cohort detachment before retiring its sources.
    for _ in 0..DOMAINS + 3 {
        for actor in order {
            let before = rank(w, actor);
            if before == (0, 0, 0) {
                continue;
            }
            if before.0 == 0 && before.1 > 0 && (0..ACTORS).any(|peer| rank(w, peer).0 != 0) {
                continue;
            }
            let ix = pnl_terminal_handoff::payout(w, actor);
            let changed = [
                w.env.market,
                w.env.vault,
                w.portfolios[actor],
                w.tokens[actor],
            ];
            m.submit(w, &[ix.clone()], &[], true);
            b.check(w, true);
            m.submit(w, &[ix], &changed, false);
            assert!(
                rank(w, actor) < before,
                "terminal claim disposition decreases rank"
            );
            m.terminal += 1;
            b.check(w, true);
        }
    }
    for actor in 0..ACTORS {
        assert_eq!(rank(w, actor), (0, 0, 0), "finite cohort sweep bound");
        assert!(resolved_portfolio_is_terminal(&w.env, w.portfolios[actor]));
    }
    assert_eq!(w.tokens.map(|t| w.env.token_amount(t) as u128), expected);
    for actor in order {
        w.env.svm.expire_blockhash();
        m.measure(
            w.env
                .close_portfolio_with_cu(&w.owners[actor], w.portfolios[actor]),
        );
    }
    let group = w.env.market_state().1;
    assert_eq!(
        (
            group.vault,
            group.c_tot,
            group.source_claim_bound_total_num,
            group.materialized_portfolio_count
        ),
        (0, 0, 0, 0)
    );
    assert!(group
        .assets
        .iter()
        .all(|a| a.oi_eff_long_q == 0 && a.oi_eff_short_q == 0));
}

#[test]
fn v16_program_full_source_claims_compose_with_side_oi_handoff_and_terminal_exit() {
    assert_certified_engine_pin("INV-058/028 capacity and side-OI composition");
    let cap = percolator::MAX_OI_SIDE_Q;
    let sizes = [cap / 4, cap / 4 + POS_SCALE, cap / 2 - POS_SCALE];
    assert_eq!(sizes.iter().sum::<u128>(), cap);
    let mut m = Measurements::default();
    let mut worlds = 0;
    let mut full_tables = 0;
    for direction in [-1i128, 1] {
        for batch in [false, true] {
            for reverse in [false, true] {
                for receives in [false, true] {
                    let mut w = World::with_params(V16CuMarketParams {
                        max_portfolio_assets: SLOTS as u16,
                        initial_price: PRICE,
                        max_price_move_bps_per_slot: 150,
                        max_accrual_dt_slots: 64,
                        min_funding_lifetime_slots: 64,
                        ..V16CuMarketParams::default()
                    });
                    w.env.configure_permissionless_resolve_with_cu(1000, 5);
                    let mut b = Book::new(&w);
                    let order = if reverse {
                        [4, 2, 0, 5, 3, 1]
                    } else {
                        [1, 3, 5, 0, 2, 4]
                    };
                    for asset in 0..SHARED {
                        let q = (1 + asset % 3) as i128 * POS_SCALE as i128;
                        fill(&mut w, &mut b, &mut m, 0, asset, q, false);
                        b.mark(&mut w, &mut m, asset, PRICE + 1, order);
                        fill(&mut w, &mut b, &mut m, 0, asset, -2 * q, false);
                        b.mark(&mut w, &mut m, asset, PRICE, order);
                        fill(&mut w, &mut b, &mut m, 0, asset, q, false);
                    }
                    assert_eq!(b.claims[0].iter().filter(|c| **c > 0).count(), DOMAINS - 2);
                    let historical = b.claims[0];
                    for pair in [0, 2, 4] {
                        let q = direction * sizes[pair / 2] as i128;
                        let ix = trade(&w, pair, SHARED, q, PRICE, batch);
                        m.submit(&mut w, &[ix], &[], true);
                        b.check(&w, false);
                        fill(&mut w, &mut b, &mut m, pair, SHARED, q, batch);
                        let before_slot = w.env.market_state().1.assets[SHARED].slot_last;
                        if before_slot < b.slot {
                            m.measure(w.env.crank(
                                w.portfolios[pair],
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: b.slot,
                                    observations: crank_observations(SHARED as u16),
                                },
                            ));
                            assert_eq!(w.env.market_state().1.assets[SHARED].slot_last, b.slot);
                            b.check(&w, false);
                        }
                    }
                    assert_eq!(w.env.market_state().1.assets[SHARED].oi_eff_long_q, cap);
                    b.mark(
                        &mut w,
                        &mut m,
                        SHARED,
                        (PRICE as i128 + direction) as u64,
                        order,
                    );
                    let earned_before_handoff = b.claims;
                    let (donor, receiver) = if receives { (2, 0) } else { (0, 2) };
                    let release = direction * 2 * POS_SCALE as i128;
                    let release_ix = trade(&w, donor, SHARED, -release, b.prices[SHARED], batch);
                    let refill_ix = trade(&w, receiver, SHARED, release, b.prices[SHARED], !batch);
                    m.submit(&mut w, &[release_ix.clone(), refill_ix.clone()], &[], true);
                    b.check(&w, false);
                    for (pair, q, ix) in [
                        (donor, -release, release_ix),
                        (receiver, release, refill_ix),
                    ] {
                        let changed = [w.env.market, w.portfolios[pair], w.portfolios[pair + 1]];
                        m.submit(&mut w, &[ix], &changed, false);
                        b.record(pair, SHARED, q);
                        b.check(&w, false);
                        assert_eq!(b.claims, earned_before_handoff);
                        assert_eq!(
                            w.env.market_state().1.assets[SHARED].oi_eff_long_q,
                            cap - if pair == donor {
                                release.unsigned_abs()
                            } else {
                                0
                            }
                        );
                    }
                    for pair in [0, 2, 4] {
                        let q = -b.positions[pair][SHARED];
                        fill(&mut w, &mut b, &mut m, pair, SHARED, q, batch);
                        fill(&mut w, &mut b, &mut m, pair, SHARED, q, !batch);
                    }
                    b.mark(&mut w, &mut m, SHARED, PRICE, order);
                    assert_eq!(b.claims[0][..2 * SHARED], historical[..2 * SHARED]);
                    assert_eq!(
                        w.env
                            .portfolio_state(w.portfolios[0])
                            .source_domains
                            .iter()
                            .filter(|s| s.is_occupied())
                            .count(),
                        DOMAINS
                    );
                    assert_eq!(w.env.market_state().1.assets[SHARED].oi_eff_long_q, cap);
                    full_tables += 1;
                    let observed = [0, 2].map(|actor| {
                        let mut claims = [0; DOMAINS];
                        for source in w
                            .env
                            .portfolio_state(w.portfolios[actor])
                            .source_domains
                            .iter()
                            .filter(|s| s.is_occupied())
                        {
                            claims[source.domain.get() as usize] =
                                source.source_claim_bound_num.get();
                        }
                        assert!(b.claims_match(actor, claims, false));
                        claims
                    });
                    let mut wrong = observed;
                    wrong[0][2 * SHARED] += BOUND_SCALE;
                    wrong[1][2 * SHARED] -= BOUND_SCALE;
                    assert_eq!(
                        wrong.iter().flatten().sum::<u128>(),
                        observed.iter().flatten().sum::<u128>()
                    );
                    assert!(!b.claims_match(0, wrong[0], false));
                    assert!(!b.claims_match(2, wrong[1], false));
                    let transfer = if receives { 2i128 } else { -2 };
                    let historical_gain =
                        2 * (0..SHARED).map(|a| (1 + a % 3) as i128).sum::<i128>();
                    let gains = [
                        2 * (sizes[0] / POS_SCALE) as i128 + historical_gain + transfer,
                        2 * (sizes[1] / POS_SCALE) as i128 - transfer,
                        2 * (sizes[2] / POS_SCALE) as i128,
                    ];
                    let expected = std::array::from_fn(|a| {
                        (CAPITAL as i128 + gains[a / 2] * if a % 2 == 0 { 1 } else { -1 }) as u128
                    });
                    assert_eq!(b.entitlement(), expected);
                    terminal(&mut w, &b, &mut m, order);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!((worlds, full_tables), (16, 16));
    assert_eq!(m.paying_rollbacks, worlds * ACTORS);
    println!("INV-058/028 Scope K: worlds={worlds}, full_tables={full_tables}, checked_submissions={}, exact_rollbacks={}, paying_rollbacks={}, terminal_calls={}, peak_cu={}, max_packet={}",
        m.submissions, m.rollbacks, m.paying_rollbacks, m.terminal, m.peak, m.packet);
}
