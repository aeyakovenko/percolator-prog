//! INV-077: a refresh whose K/F loss burns liened claim face at the full source
//! cap. Construction uses only wrapper transitions.
//!
//! The account holds a claim in every one of the 28 source domains, the claims
//! are its margin (so they are liened), and a mark move takes all of it. Each
//! burned lien pays the domain its loss is owed to, so every domain must end
//! fully backed, and the refresh must fit in one transaction: an account that
//! cannot be refreshed cannot be liquidated.
//!
//! That refresh costs work for every leg that reaches liened face and for every
//! claim it burns through, and past a point it does not fit. The wrapper
//! therefore refuses the trade that would take a portfolio holding liened face
//! past `liened claims + 6 * legs <= 72`; the second half of this file measures
//! the costliest loss on that line and shows the trades past it are refused.

use super::*;

const SOURCES: usize = percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS;
const PRICE_LOW: u64 = 100;
const PRICE_HIGH: u64 = 101;
/// Twice the opening mark: a short opened at `PRICE_LOW` against 100% margin
/// loses exactly its margin.
const WIPE_OUT_PRICE: u64 = 200;
/// Each round of the fixture leaves the LP a claim of this size in one domain,
/// paid in by the taker, beside the same amount of provider principal.
const CLAIM_PER_SOURCE: u128 = 1_000;
const PROVIDER_PER_SOURCE: u128 = 1_000;
/// Mark for every leg but the first in the costliest loss: ten atoms a unit.
const SMALL_LOSS_PRICE: u64 = 110;
const LEG_WEIGHT: usize = percolator_prog::constants::WRAPPER_SOURCE_LIEN_REFRESH_LEG_WEIGHT;
const MAX_WORK: usize = percolator_prog::constants::WRAPPER_MAX_SOURCE_LIEN_REFRESH_WORK;
/// `PercolatorError::InvalidInstruction`, which the wrapper's admission bounds return.
const REFUSED: &str = "Custom(9)";

struct AllSourceLienPair {
    env: V16CuEnv,
    taker_owner: Keypair,
    lp_owner: Keypair,
    taker: Pubkey,
    lp: Pubkey,
    slot: u64,
    /// The LP's capital, withdrawn into `capital_source`.
    capital: u128,
    capital_source: Pubkey,
}

fn liened_sources(state: &PortfolioAccountV16) -> usize {
    state
        .source_domains
        .iter()
        .filter(|source| source.source_claim_liened_num.get() != 0)
        .count()
}

fn certificate_is_current(env: &V16CuEnv, portfolio: Pubkey) -> bool {
    let group = env.market_state().1;
    let state = env.portfolio_state(portfolio);
    let cert = health_cert(&state);
    cert.valid
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == active_bitmap(&state)
}

/// The pair of `v16_program_sequential_all_source_lien_mutation_shape_is_bounded`
/// up to its last source. The LP is flat, holds a `CLAIM_PER_SOURCE` claim in each
/// of the 28 source domains (27 of them still liened from the earlier seeds) and
/// no capital: it has just been withdrawn into `capital_source`.
fn all_source_lien_pair() -> AllSourceLienPair {
    const LIEN_PER_SOURCE_Q: i128 = 10 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: MAX_SOURCE_LIVE_ASSETS,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            ..V16CuMarketParams::default()
        },
        70,
    );
    let mut slot = 0u64;
    for asset_index in 0..MAX_SOURCE_LIVE_ASSETS {
        env.configure_auth_mark_for_asset_as_admin(asset_index, slot, PRICE_LOW);
    }

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 2_000_000);
    env.deposit(&lp_owner, lp, 2_000_000);

    let drive_both_current = |env: &mut V16CuEnv, now_slot: u64| {
        for _ in 0..8 {
            for portfolio in [taker, lp] {
                if !certificate_is_current(env, portfolio) {
                    env.crank(
                        portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot,
                            observations: vec![],
                        },
                    );
                }
            }
            if certificate_is_current(env, taker) && certificate_is_current(env, lp) {
                return;
            }
        }
        panic!("all-source lien pair did not reach a certificate fixed point");
    };
    let settle_both = |env: &mut V16CuEnv, asset_index: u16, now_slot: u64| {
        for portfolio in [taker, lp] {
            env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot,
                    observations: crank_observations(asset_index),
                },
            );
        }
        drive_both_current(env, now_slot);
    };
    let direct_fill = |env: &mut V16CuEnv, asset_index: u16, size_q: i128, exec_price: u64| {
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            asset_index,
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            size_q,
            exec_price,
            0,
        )
    };

    let mut source_count = 0u16;
    for asset_index in 0..MAX_SOURCE_LIVE_ASSETS {
        for domain in [asset_index * 2, asset_index * 2 + 1] {
            env.top_up_backing_bucket(domain, PROVIDER_PER_SOURCE, 1_000);
        }
        for (open_q, open_price, target_price, close_q) in [
            (
                -MAX_SOURCE_LIVE_SIZE_Q,
                PRICE_LOW,
                PRICE_HIGH,
                MAX_SOURCE_LIVE_SIZE_Q,
            ),
            (
                MAX_SOURCE_LIVE_SIZE_Q,
                PRICE_HIGH,
                PRICE_LOW,
                -MAX_SOURCE_LIVE_SIZE_Q,
            ),
        ] {
            direct_fill(&mut env, asset_index, open_q, open_price)
                .unwrap_or_else(|error| panic!("asset {asset_index} open failed: {error}"));
            slot += 1;
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(asset_index, slot, target_price);
            settle_both(&mut env, asset_index, slot);
            direct_fill(&mut env, asset_index, close_q, target_price)
                .unwrap_or_else(|error| panic!("asset {asset_index} close failed: {error}"));

            source_count += 1;
            if source_count % 2 != 0 && source_count < 27 {
                continue;
            }
            let capital = env.portfolio_state(lp).capital.get();
            let (capital_source, _) = env.withdraw_with_cu(&lp_owner, lp, capital);
            if usize::from(source_count) == SOURCES {
                let state = env.portfolio_state(lp);
                assert_eq!(state.capital.get(), 0);
                assert_eq!(
                    state.pnl.get(),
                    (SOURCES as u128 * CLAIM_PER_SOURCE) as i128
                );
                assert!(percolator::active_bitmap_is_empty(active_bitmap(&state)));
                assert_eq!(liened_sources(&state), SOURCES - 1);
                return AllSourceLienPair {
                    env,
                    taker_owner,
                    lp_owner,
                    taker,
                    lp,
                    slot,
                    capital,
                    capital_source,
                };
            }

            // Lien every claim so far against one position, then put the capital
            // back and flatten. The liens stay until a crank releases them.
            let seed_q = LIEN_PER_SOURCE_Q * i128::from(source_count);
            direct_fill(&mut env, 0, seed_q, PRICE_LOW)
                .unwrap_or_else(|error| panic!("source-lien seed {source_count} failed: {error}"));
            assert_eq!(
                liened_sources(&env.portfolio_state(lp)),
                usize::from(source_count)
            );
            env.svm.expire_blockhash();
            env.send(
                env.deposit_ix(lp, capital),
                vec![
                    AccountMeta::new(lp_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(lp, false),
                    AccountMeta::new(capital_source, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&lp_owner],
            )
            .expect("redeposit the withdrawn collateral");
            direct_fill(&mut env, 0, -seed_q, PRICE_LOW).unwrap_or_else(|error| {
                panic!("source-lien seed {source_count} flatten failed: {error}")
            });
            drive_both_current(&mut env, slot);
        }
    }
    unreachable!("the fixture returns at its last source");
}

impl AllSourceLienPair {
    /// Puts the LP's capital back.
    fn redeposit_capital(&mut self) {
        self.env.svm.expire_blockhash();
        self.env
            .send(
                self.env.deposit_ix(self.lp, self.capital),
                vec![
                    AccountMeta::new(self.lp_owner.pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(self.lp, false),
                    AccountMeta::new(self.capital_source, false),
                    AccountMeta::new(self.env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&self.lp_owner],
            )
            .expect("redeposit the withdrawn collateral");
    }

    /// Opens one LP short per entry of `units` (asset `i` gets `units[i]`) at
    /// `PRICE_LOW` in a single batch and returns its CU.
    fn open_shorts_in_one_batch(&mut self, units: &[i128]) -> u64 {
        let legs = units
            .iter()
            .enumerate()
            .map(|(asset_index, units)| BatchTradeLeg {
                asset_index: asset_index as u16,
                market_id: first_generation_market_id(asset_index as u16),
                size_q: units * POS_SCALE as i128,
                exec_price: PRICE_LOW,
                fee_bps: 0,
            })
            .collect();
        self.env.svm.expire_blockhash();
        self.env
            .send(
                self.env.batch_trade_no_cpi_ix(self.taker, self.lp, legs),
                vec![
                    AccountMeta::new(self.taker_owner.pubkey(), true),
                    AccountMeta::new(self.lp_owner.pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(self.taker, false),
                    AccountMeta::new(self.lp, false),
                ],
                &[&self.taker_owner, &self.lp_owner],
            )
            .expect("open the LP's shorts against its claims")
    }

    /// Opens the same shorts one trade at a time and returns the largest CU.
    fn open_shorts_one_by_one(&mut self, units: &[i128]) -> u64 {
        let mut max_cu = 0;
        for (asset_index, units) in units.iter().enumerate() {
            self.env.svm.expire_blockhash();
            let cu = self
                .env
                .try_trade_asset_with_cu(
                    asset_index as u16,
                    &self.taker_owner,
                    self.taker,
                    &self.lp_owner,
                    self.lp,
                    units * POS_SCALE as i128,
                    PRICE_LOW,
                    0,
                )
                .unwrap_or_else(|error| panic!("open short on asset {asset_index}: {error}"));
            max_cu = max_cu.max(cu);
        }
        max_cu
    }

    /// Moves the first `legs` assets to `WIPE_OUT_PRICE` in the next slot.
    fn move_marks_against_the_lp(&mut self, legs: usize) {
        self.move_marks(&vec![WIPE_OUT_PRICE; legs])
    }

    /// Moves asset `i` to `prices[i]` in the next slot. The marks are committed
    /// on a flat observer, so the LP's own crank carries no observation.
    fn move_marks(&mut self, prices: &[u64]) {
        let observer = self.env.create_portfolio(&Keypair::new());
        self.slot += 1;
        self.env.svm.warp_to_slot(self.slot);
        for (asset_index, price) in prices.iter().enumerate() {
            self.env
                .push_auth_mark_for_asset_as_admin(asset_index as u16, self.slot, *price);
        }
        for asset_index in 0..prices.len() as u16 {
            self.env.svm.expire_blockhash();
            self.env
                .send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: self.slot,
                        observations: crank_observations(asset_index),
                    },
                    vec![
                        AccountMeta::new(self.env.payer.pubkey(), true),
                        AccountMeta::new(self.env.market, false),
                        AccountMeta::new(observer, false),
                    ],
                    &[],
                )
                .expect("commit the moved mark");
        }
        let group = self.env.market_state().1;
        for (asset_index, price) in prices.iter().enumerate() {
            assert_eq!(group.assets[asset_index].effective_price, *price);
        }
    }

    /// One crank of the LP.
    fn try_crank_lp(&mut self, observations: Vec<CrankObservationHint>) -> Result<u64, String> {
        self.env.svm.expire_blockhash();
        self.env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: self.slot,
                observations,
            },
            vec![
                AccountMeta::new(self.env.payer.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.lp, false),
            ],
            &[],
        )
    }

    /// Cranks the flat, funded LP until only `liened` of its claims still carry
    /// a lien. Each crank returns the lien of one domain.
    fn release_liens_down_to(&mut self, liened: usize) {
        for _ in 0..4 * SOURCES {
            if liened_sources(&self.env.portfolio_state(self.lp)) <= liened {
                break;
            }
            // A flat account is recertified against an observation; the releases
            // that follow need none.
            let observations = if certificate_is_current(&self.env, self.lp) {
                vec![]
            } else {
                crank_observations(0)
            };
            self.try_crank_lp(observations)
                .expect("a flat funded account's liens are released one domain a crank");
        }
        assert_eq!(liened_sources(&self.env.portfolio_state(self.lp)), liened);
    }

    /// One LP short of `units` on `asset_index` at `PRICE_LOW`.
    fn try_open_short(&mut self, asset_index: u16, units: i128) -> Result<u64, String> {
        self.env.svm.expire_blockhash();
        self.env.try_trade_asset_with_cu(
            asset_index,
            &self.taker_owner,
            self.taker,
            &self.lp_owner,
            self.lp,
            units * POS_SCALE as i128,
            PRICE_LOW,
            0,
        )
    }

    /// The LP buys back `units` of its short on `asset_index`.
    fn try_reduce_short(&mut self, asset_index: u16, units: i128) -> Result<u64, String> {
        self.try_open_short(asset_index, -units)
    }

    /// One hint-free crank of the LP: the refresh that settles every leg's loss.
    fn refresh_lp(&mut self) -> u64 {
        self.env.svm.expire_blockhash();
        self.env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: self.slot,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(self.env.payer.pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(self.lp, false),
                ],
                &[],
            )
            .expect("the loss refresh must fit in one transaction")
    }

    /// Refreshes the taker, whose gains become claims on the LP's loss domains,
    /// and requires every source domain to hold backing for all of its claims.
    fn assert_every_domain_backs_its_claims(&mut self) {
        for _ in 0..4 {
            if certificate_is_current(&self.env, self.taker) {
                break;
            }
            self.env.crank(
                self.taker,
                ProgInstruction::PermissionlessCrank {
                    now_slot: self.slot,
                    observations: vec![],
                },
            );
        }
        assert!(certificate_is_current(&self.env, self.taker));
        let group = self.env.market_state().1;
        for domain in 0..SOURCES {
            let source = &group.source_credit[domain];
            assert!(
                source.fresh_reserved_backing_num >= source.positive_claim_bound_num,
                "domain {domain} holds {} for claims of {}",
                source.fresh_reserved_backing_num / BOUND_SCALE,
                source.positive_claim_bound_num / BOUND_SCALE
            );
        }
    }

    fn reserved_backing(&self) -> Vec<u128> {
        let group = self.env.market_state().1;
        (0..SOURCES)
            .map(|domain| group.source_credit[domain].fresh_reserved_backing_num / BOUND_SCALE)
            .collect()
    }
}

/// One short whose margin is all 28 claims. The move burns every lien in one
/// leg's settlement. The backing behind each burned lien follows the loss to
/// the short-loss domain of asset 0, which the taker's gain is then a claim on:
/// that domain ends with its own 2,000 and the 1,000 the taker paid into each
/// of the other 27, and every other domain keeps exactly its provider's 1,000.
#[test]
fn v16_program_one_leg_loss_through_every_source_lien_is_bounded_and_backs_the_gain() {
    let mut pair = all_source_lien_pair();
    let open_cu = pair.open_shorts_in_one_batch(&[280]);
    assert_cu_within("one-leg all-source lien open", open_cu, 1_400_000);
    assert_eq!(liened_sources(&pair.env.portfolio_state(pair.lp)), SOURCES);
    assert_eq!(
        pair.reserved_backing(),
        vec![CLAIM_PER_SOURCE + PROVIDER_PER_SOURCE; SOURCES]
    );

    pair.move_marks_against_the_lp(1);
    let refresh_cu = pair.refresh_lp();
    assert_cu_within(
        "one-leg all-source lien loss refresh",
        refresh_cu,
        1_375_000,
    );

    let lp = pair.env.portfolio_state(pair.lp);
    assert_eq!(lp.pnl.get(), 0);
    assert_eq!(lp.capital.get(), 0);
    assert_eq!(liened_sources(&lp), 0);
    assert!(lp.source_domains.iter().all(|source| !source.is_occupied()));

    // The LP is short asset 0: its loss is owed to that asset's short-loss domain.
    const LOSS_DOMAIN: usize = 1;
    let mut expected = vec![PROVIDER_PER_SOURCE; SOURCES];
    expected[LOSS_DOMAIN] =
        CLAIM_PER_SOURCE + PROVIDER_PER_SOURCE + (SOURCES as u128 - 1) * CLAIM_PER_SOURCE;
    assert_eq!(pair.reserved_backing(), expected);
    pair.assert_every_domain_backs_its_claims();
    let group = pair.env.market_state().1;
    assert_eq!(
        group.source_credit[LOSS_DOMAIN].positive_claim_bound_num / BOUND_SCALE,
        SOURCES as u128 * CLAIM_PER_SOURCE,
        "the taker's whole gain is a claim on the loss domain"
    );
    println!("INV-077 one-leg all-source lien loss CU: open={open_cu}, refresh={refresh_cu}");
}

/// Five shorts share the same 28 liened claims, and each leg's loss burns the
/// liens the legs before it left. Five losing legs is what the engine pinned
/// before the liened-face fix could refresh in this shape; the fix must not
/// cost that.
#[test]
fn v16_program_five_leg_loss_through_every_source_lien_is_bounded_and_backs_every_gain() {
    const UNITS: [i128; 5] = [15, 66, 66, 66, 66];
    let mut pair = all_source_lien_pair();
    let open_cu = pair.open_shorts_in_one_batch(&UNITS);
    assert_cu_within("five-leg all-source lien open", open_cu, 1_400_000);
    assert_eq!(liened_sources(&pair.env.portfolio_state(pair.lp)), SOURCES);

    pair.move_marks_against_the_lp(UNITS.len());
    let refresh_cu = pair.refresh_lp();
    assert_cu_within(
        "five-leg all-source lien loss refresh",
        refresh_cu,
        1_375_000,
    );

    let loss = UNITS.iter().sum::<i128>() * (WIPE_OUT_PRICE - PRICE_LOW) as i128;
    let lp = pair.env.portfolio_state(pair.lp);
    assert_eq!(
        lp.pnl.get(),
        (SOURCES as u128 * CLAIM_PER_SOURCE) as i128 - loss
    );
    assert_eq!(lp.capital.get(), 0);
    assert_eq!(
        pair.reserved_backing().iter().sum::<u128>(),
        SOURCES as u128 * (CLAIM_PER_SOURCE + PROVIDER_PER_SOURCE),
        "burned liens move backing between domains and never out of them"
    );
    pair.assert_every_domain_backs_its_claims();
    println!("INV-077 five-leg all-source lien loss CU: open={open_cu}, refresh={refresh_cu}");
}

/// The same loss for an LP that put its capital back: six shorts opened against
/// capital while 27 of its claims still carry liens from earlier positions. The
/// loss is still paid out of the claims first, liened or not.
#[test]
fn v16_program_six_leg_loss_through_leftover_source_liens_is_bounded_and_backs_every_gain() {
    const UNITS: [i128; 6] = [15, 53, 53, 53, 53, 53];
    let mut pair = all_source_lien_pair();
    pair.redeposit_capital();
    let open_cu = pair.open_shorts_one_by_one(&UNITS);
    assert_cu_within("six-leg leftover-lien open", open_cu, 1_375_000);
    assert_eq!(
        liened_sources(&pair.env.portfolio_state(pair.lp)),
        SOURCES - 1
    );

    pair.move_marks_against_the_lp(UNITS.len());
    let refresh_cu = pair.refresh_lp();
    assert_cu_within("six-leg leftover-lien loss refresh", refresh_cu, 1_375_000);

    let lp = pair.env.portfolio_state(pair.lp);
    assert_eq!(lp.pnl.get(), 0);
    assert_eq!(lp.capital.get(), pair.capital);
    assert_eq!(liened_sources(&lp), 0);
    assert_eq!(
        pair.reserved_backing().iter().sum::<u128>(),
        SOURCES as u128 * (CLAIM_PER_SOURCE + PROVIDER_PER_SOURCE),
        "burned liens move backing between domains and never out of them"
    );
    pair.assert_every_domain_backs_its_claims();
    println!("INV-077 six-leg leftover-lien loss CU: open={open_cu}, refresh={refresh_cu}");
}

// ---------------------------------------------------------------------------
// The refresh bound on locked claim face.
// ---------------------------------------------------------------------------

/// Position sizes for the costliest loss `legs` shorts can take over `liened`
/// liened claims, the other claims being unliened.
///
/// Asset 0 goes to `WIPE_OUT_PRICE` (100 atoms a unit): its loss takes every
/// unliened claim, which is netted at par, and half of the first liened one.
/// Every other asset goes to `SMALL_LOSS_PRICE` (ten atoms a unit) and loses
/// the same amount, sized so that each loss ends inside a liened claim and the
/// last one inside the last claim. Every leg then burns liened face, and the
/// liened burns number `liened + legs - 1`, the most there can be.
fn costliest_loss_units(legs: usize, liened: usize) -> Vec<i128> {
    assert!(legs >= 2 && (1..SOURCES).contains(&liened));
    let claim = CLAIM_PER_SOURCE as i128;
    let first_loss = (SOURCES - liened) as i128 * claim + claim / 2;
    let liened_left = liened as i128 * claim - claim / 2;
    let later_legs = legs as i128 - 1;
    let mut share = (liened_left - 10) / later_legs / 10 * 10;
    while (1..=later_legs).any(|leg| (claim / 2 + leg * share) % claim == 0) {
        share -= 10;
    }
    assert!(share >= 10);
    let mut units = vec![first_loss / (WIPE_OUT_PRICE - PRICE_LOW) as i128];
    units.extend(std::iter::repeat(share / (SMALL_LOSS_PRICE - PRICE_LOW) as i128).take(legs - 1));
    units
}

/// `legs` shorts opened against capital over `liened` leftover liens take the
/// costliest loss. The refresh must fit, pay every gain's domain, and one more
/// leg must be refused. Returns the refresh CU.
fn costliest_loss_within_the_bound(legs: usize, liened: usize) -> u64 {
    assert!(liened + LEG_WEIGHT * legs <= MAX_WORK);
    let units = costliest_loss_units(legs, liened);
    let mut pair = all_source_lien_pair();
    pair.redeposit_capital();
    pair.release_liens_down_to(liened);
    let open_cu = pair.open_shorts_one_by_one(&units);
    assert_cu_within(
        "open under the source-lien refresh bound",
        open_cu,
        1_375_000,
    );
    assert_eq!(liened_sources(&pair.env.portfolio_state(pair.lp)), liened);

    let refused = pair
        .try_open_short(legs as u16, 1)
        .expect_err("one more leg would pass the bound");
    assert!(refused.contains(REFUSED), "{refused}");

    let mut prices = vec![SMALL_LOSS_PRICE; legs];
    prices[0] = WIPE_OUT_PRICE;
    pair.move_marks(&prices);
    let refresh_cu = pair.refresh_lp();
    assert_cu_within(
        &format!("costliest {legs}-leg loss over {liened} liened claims"),
        refresh_cu,
        1_375_000,
    );

    let loss = units[0] * (WIPE_OUT_PRICE - PRICE_LOW) as i128
        + units[1..].iter().sum::<i128>() * (SMALL_LOSS_PRICE - PRICE_LOW) as i128;
    let lp = pair.env.portfolio_state(pair.lp);
    assert_eq!(
        lp.pnl.get(),
        (SOURCES as u128 * CLAIM_PER_SOURCE) as i128 - loss,
        "the whole loss is paid out of the claims"
    );
    assert_eq!(lp.capital.get(), pair.capital);
    assert_eq!(
        pair.reserved_backing().iter().sum::<u128>(),
        SOURCES as u128 * (CLAIM_PER_SOURCE + PROVIDER_PER_SOURCE),
        "netted and burned claims move backing between domains and never out of them"
    );
    pair.assert_every_domain_backs_its_claims();
    println!(
        "INV-077 costliest loss, {legs} legs over {liened} liened claims: open={open_cu}, refresh={refresh_cu}"
    );
    refresh_cu
}

/// Seven legs may sit on any number of liens. Capital is back in the account, so
/// one of the 28 claims carries none.
#[test]
fn v16_program_costliest_seven_leg_loss_over_27_liens_is_bounded_and_backs_every_gain() {
    costliest_loss_within_the_bound(7, 27);
}

#[test]
fn v16_program_costliest_eight_leg_loss_over_24_liens_is_bounded_and_backs_every_gain() {
    assert_eq!(24 + LEG_WEIGHT * 8, MAX_WORK);
    costliest_loss_within_the_bound(8, 24);
}

#[test]
fn v16_program_costliest_nine_leg_loss_over_18_liens_is_bounded_and_backs_every_gain() {
    assert_eq!(18 + LEG_WEIGHT * 9, MAX_WORK);
    costliest_loss_within_the_bound(9, 18);
}

#[test]
fn v16_program_costliest_ten_leg_loss_over_12_liens_is_bounded_and_backs_every_gain() {
    assert_eq!(12 + LEG_WEIGHT * 10, MAX_WORK);
    costliest_loss_within_the_bound(10, 12);
}

#[test]
fn v16_program_costliest_eleven_leg_loss_over_6_liens_is_bounded_and_backs_every_gain() {
    assert_eq!(6 + LEG_WEIGHT * 11, MAX_WORK);
    costliest_loss_within_the_bound(11, 6);
}

/// The shapes that could not be refreshed before the bound can no longer be
/// built: over 27 leftover liens the eighth leg is refused, whatever its size.
/// What does not raise the work past the bound is not refused: reducing a leg,
/// closing one, and opening it again.
#[test]
fn v16_program_source_lien_refresh_bound_refuses_the_leg_past_it() {
    let mut pair = all_source_lien_pair();
    pair.redeposit_capital();
    assert_eq!(
        liened_sources(&pair.env.portfolio_state(pair.lp)),
        SOURCES - 1
    );
    for asset_index in 0..7 {
        pair.try_open_short(asset_index, 20)
            .unwrap_or_else(|error| panic!("leg {asset_index} is within the bound: {error}"));
    }
    assert!(SOURCES - 1 + LEG_WEIGHT * 7 <= MAX_WORK && SOURCES - 1 + LEG_WEIGHT * 8 > MAX_WORK);
    let before = pair.env.portfolio_state(pair.lp);
    for units in [1, 20, 29] {
        let refused = pair
            .try_open_short(7, units)
            .expect_err("an eighth leg over 27 liens passes the bound");
        assert!(refused.contains(REFUSED), "{refused}");
    }
    assert_eq!(pair.env.portfolio_state(pair.lp), before);

    pair.try_reduce_short(0, 10)
        .expect("reducing a leg adds no work");
    pair.try_reduce_short(6, 20)
        .expect("closing a leg lowers the work");
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&pair.env.portfolio_state(pair.lp))),
        6
    );
    pair.try_open_short(6, 20)
        .expect("the seventh leg is within the bound again");
    let refused = pair
        .try_open_short(7, 20)
        .expect_err("and the eighth is still past it");
    assert!(refused.contains(REFUSED), "{refused}");
}

/// Unliened claims need no such bound. All fourteen legs lose through 28
/// unliened claims and a little capital: every claim is netted at par and the
/// refresh fits.
#[test]
fn v16_program_fourteen_leg_loss_through_unliened_claims_at_the_source_cap_is_bounded() {
    const UNITS: [i128; 14] = [15, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21];
    let mut pair = all_source_lien_pair();
    pair.redeposit_capital();
    pair.release_liens_down_to(0);
    let open_cu = pair.open_shorts_one_by_one(&UNITS);
    assert_cu_within("fourteen-leg unliened open", open_cu, 1_375_000);
    assert_eq!(liened_sources(&pair.env.portfolio_state(pair.lp)), 0);

    pair.move_marks_against_the_lp(UNITS.len());
    let refresh_cu = pair.refresh_lp();
    assert_cu_within("fourteen-leg unliened loss refresh", refresh_cu, 1_375_000);

    let loss = UNITS.iter().sum::<i128>() as u128 * (WIPE_OUT_PRICE - PRICE_LOW) as u128;
    let claims = SOURCES as u128 * CLAIM_PER_SOURCE;
    let lp = pair.env.portfolio_state(pair.lp);
    assert_eq!(lp.pnl.get(), 0);
    assert_eq!(lp.capital.get(), pair.capital - (loss - claims));
    assert!(lp.source_domains.iter().all(|source| !source.is_occupied()));
    pair.assert_every_domain_backs_its_claims();
    println!("INV-077 fourteen-leg unliened loss CU: open={open_cu}, refresh={refresh_cu}");
}
