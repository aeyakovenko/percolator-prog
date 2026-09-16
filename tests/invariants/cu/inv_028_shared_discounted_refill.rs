//! INV-028: unequal co-claimants consume two discounted domains across a late refill.
//! Unlike full-table mixed availability and single-domain deferred registration, both
//! sources have fractional rates when the first owner converts. A matching refill
//! then changes only one remaining rate and repays part of its consumed-backing
//! receivable. Expired stock stays in custody but cannot fund either live conversion.
//! Both claimant orders use public trades, settlement, expiry, top-ups and withdrawal.

use super::*;

const DOMAINS: [usize; 2] = [1, 3];
const CAPITAL: u128 = 2_000;
const FACES: [[u128; 2]; 2] = [[50, 150], [150, 50]];

struct SourceBook {
    claims: [[u128; 2]; 2],
    fresh: [u128; 2],
    spent: [u128; 2],
    receivable: [u128; 2],
}

impl SourceBook {
    fn check(&self, env: &V16CuEnv, portfolios: &[Pubkey; 4]) -> [[u128; 2]; 2] {
        let group = env.market_state().1;
        let now = env.svm.get_sysvar::<Clock>().slot;
        let mut support = [[0; 2]; 2];
        for (index, domain) in DOMAINS.into_iter().enumerate() {
            let bucket = group.source_backing_buckets[domain];
            let source = group.source_credit[domain];
            let reservation = group.insurance_credit_reservations[domain];
            let available =
                if bucket.status == BackingBucketStatusV16::Fresh && bucket.expiry_slot > now {
                    bucket.fresh_unliened_backing_num
                } else {
                    0
                };
            assert_eq!(available, self.fresh[index] * BOUND_SCALE);
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(bucket.impaired_liened_backing_num, 0);
            assert_eq!(reservation.insurance_credit_reserved_num, 0);
            assert_eq!(reservation.valid_liened_insurance_num, 0);
            assert_eq!(reservation.impaired_liened_insurance_num, 0);
            assert_eq!(source.fresh_reserved_backing_num, available);
            assert_eq!(source.valid_liened_backing_num, 0);
            assert_eq!(source.impaired_liened_backing_num, 0);
            assert_eq!(source.spent_backing_num, self.spent[index] * BOUND_SCALE);
            assert_eq!(
                source.provider_receivable_num,
                self.receivable[index] * BOUND_SCALE
            );
            assert_eq!(
                bucket.consumed_liened_backing_num,
                source.provider_receivable_num
            );

            let total = self.claims.iter().map(|c| c[index]).sum::<u128>() * BOUND_SCALE;
            let rate = if total == 0 {
                percolator::CREDIT_RATE_SCALE
            } else {
                (available
                    .checked_mul(percolator::CREDIT_RATE_SCALE)
                    .unwrap()
                    / total)
                    .min(percolator::CREDIT_RATE_SCALE)
            };
            let mut local_total = 0;
            let mut usable_num = 0;
            for (actor, portfolio) in portfolios.iter().enumerate() {
                let account = env.portfolio_state(*portfolio);
                let mut face = 0;
                for local in account.source_domains.iter().filter(|s| s.is_occupied()) {
                    assert!(DOMAINS.contains(&(local.domain.get() as usize)));
                    assert_eq!(local.source_claim_liened_num.get(), 0);
                    assert_eq!(local.source_claim_impaired_num.get(), 0);
                    if local.domain.get() as usize == domain {
                        face += local.source_claim_bound_num.get();
                    }
                }
                let expected = if actor < 2 {
                    self.claims[actor][index]
                } else {
                    0
                };
                assert_eq!(
                    face,
                    expected * BOUND_SCALE,
                    "actor={actor}, domain={domain}"
                );
                local_total += face;
                let usable = face.checked_mul(rate).unwrap() / percolator::CREDIT_RATE_SCALE;
                usable_num += usable;
                if actor < 2 {
                    support[actor][index] = usable / BOUND_SCALE;
                }
            }
            assert_eq!(local_total, total);
            assert_eq!(source.exact_positive_claim_num, total);
            assert_eq!(source.positive_claim_bound_num, total);
            assert_eq!(source.credit_rate_num, rate);
            assert!(
                usable_num <= available,
                "domain {domain}: credit exceeds available backing"
            );
        }
        let capital = portfolios
            .iter()
            .map(|p| env.portfolio_state(*p).capital.get())
            .sum::<u128>();
        assert_eq!(group.c_tot, capital);
        assert_eq!(group.insurance, 0);
        assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
        support
    }
}

#[test]
fn v16_program_shared_discounted_domains_recompute_capacity_after_late_refill() {
    const EXPIRY: u64 = 4;
    const REFILLED_EXPIRY: u64 = 100;
    let mut peak_cu = 0;
    for first in [0usize, 1] {
        let second = 1 - first;
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            max_portfolio_assets: 2,
            maintenance_margin_bps: 3_000,
            initial_margin_bps: 3_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: EXPIRY,
            min_funding_lifetime_slots: EXPIRY,
            ..V16CuMarketParams::default()
        });
        env.svm.warp_to_slot(1);
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
            env.top_up_backing_bucket(DOMAINS[asset as usize] as u16, 1, EXPIRY);
        }
        let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
        let portfolios = std::array::from_fn(|actor| env.create_portfolio(&owners[actor]));
        for actor in 0..4 {
            env.deposit(&owners[actor], portfolios[actor], CAPITAL);
        }
        for asset in 0..2 {
            for actor in 0..2 {
                env.svm.expire_blockhash();
                peak_cu = peak_cu.max(env.trade_asset_with_cu(
                    asset as u16,
                    &owners[actor],
                    portfolios[actor],
                    &owners[actor + 2],
                    portfolios[actor + 2],
                    (FACES[actor][asset] / 5 * POS_SCALE) as i128,
                    100,
                    0,
                ));
            }
        }
        env.svm.warp_to_slot(2);
        for asset in 0..2 {
            env.push_auth_mark_for_asset_as_admin(asset, 2, 105);
        }
        let crank = |slot| ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: [crank_observations(0), crank_observations(1)].concat(),
        };
        for actor in [2, 3, 0, 1] {
            peak_cu = peak_cu.max(env.crank(portfolios[actor], crank(2)));
        }
        for asset in 0..2 {
            for actor in 0..2 {
                env.svm.expire_blockhash();
                peak_cu = peak_cu.max(env.trade_asset_with_cu(
                    asset as u16,
                    &owners[actor],
                    portfolios[actor],
                    &owners[actor + 2],
                    portfolios[actor + 2],
                    -((FACES[actor][asset] / 5 * POS_SCALE) as i128),
                    105,
                    0,
                ));
            }
        }
        let mut book = SourceBook {
            claims: FACES,
            fresh: [201; 2],
            spent: [0; 2],
            receivable: [0; 2],
        };
        assert_eq!(book.check(&env, &portfolios), FACES);
        for (actor, portfolio) in portfolios.iter().enumerate() {
            let account = env.portfolio_state(*portfolio);
            assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
            assert_eq!(account.reserved_pnl.get(), 0);
            assert_eq!(account.pnl.get(), if actor < 2 { 200 } else { 0 });
            assert_eq!(
                account.capital.get(),
                CAPITAL - if actor < 2 { 0 } else { 200 }
            );
        }

        let vault_before_expiry = env.svm.get_account(&env.vault).unwrap();
        env.svm.warp_to_slot(EXPIRY);
        for asset in 0..2 {
            env.push_auth_mark_for_asset_as_admin(asset, EXPIRY, 105);
        }
        let unexpired = |env: &V16CuEnv| {
            let group = env.market_state().1;
            DOMAINS
                .iter()
                .filter(|&&domain| {
                    group.source_backing_buckets[domain].status == BackingBucketStatusV16::Fresh
                })
                .count()
        };
        for _ in 0..2 {
            let before = unexpired(&env);
            if before == 0 {
                break;
            }
            peak_cu = peak_cu.max(env.crank(portfolios[first], crank(EXPIRY)));
            assert!(
                unexpired(&env) < before,
                "expiry normalization must progress"
            );
        }
        for domain in DOMAINS {
            assert_eq!(
                env.market_state().1.source_backing_buckets[domain].status,
                BackingBucketStatusV16::Expired
            );
        }
        book.fresh = [0; 2];
        assert_eq!(book.check(&env, &portfolios), [[0; 2]; 2]);
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before_expiry
        );

        for (index, amount) in [80, 120].into_iter().enumerate() {
            env.top_up_backing_bucket(DOMAINS[index] as u16, amount, REFILLED_EXPIRY);
            book.fresh[index] += amount;
            book.check(&env, &portfolios);
        }
        assert_eq!(book.check(&env, &portfolios), [[20, 90], [60, 30]]);
        let mut gains = [0; 2];
        for actor in [first, second] {
            if actor == second {
                // The refill clears receivable bookkeeping and adds fresh capacity once.
                // The untouched sibling retains its 3/5 rate despite spare vault custody.
                let refill = FACES[second][0] / 10;
                let peer_before = env.svm.get_account(&portfolios[second]).unwrap();
                let sibling_before = env.market_state().1.source_backing_buckets[DOMAINS[1]];
                env.top_up_backing_bucket(DOMAINS[0] as u16, refill, REFILLED_EXPIRY);
                book.fresh[0] += refill;
                book.receivable[0] -= refill;
                assert_eq!(
                    env.svm.get_account(&portfolios[second]).unwrap(),
                    peer_before
                );
                assert_eq!(
                    env.market_state().1.source_backing_buckets[DOMAINS[1]],
                    sibling_before
                );
                let support = book.check(&env, &portfolios);
                assert_eq!(
                    support[second],
                    [FACES[second][0] / 2, FACES[second][1] * 3 / 5]
                );
                assert_eq!(
                    support[first], [0; 2],
                    "refill cannot resurrect burned claims"
                );
            }
            if let Some(cu) = env.crank_if_actionable(portfolios[actor], crank(EXPIRY)) {
                peak_cu = peak_cu.max(cu);
            }
            let per_domain = book.check(&env, &portfolios)[actor];
            let expected = per_domain.iter().sum::<u128>();
            assert!(expected > 0 && expected < FACES[actor].iter().sum());
            let group = env.market_state().1;
            assert!(
                group.vault - group.c_tot > expected,
                "expired custody must not be mistaken for available source capacity"
            );
            let tracked = [
                env.market,
                env.vault,
                env.mint,
                portfolios[0],
                portfolios[1],
                portfolios[2],
                portfolios[3],
            ];
            let before = tracked.map(|key| env.svm.get_account(&key));
            env.svm.expire_blockhash();
            let error = env
                .send(
                    env.convert_released_pnl_ix(portfolios[actor], expected - 1),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                    ],
                    &[&owners[actor]],
                )
                .expect_err("one-atom short caller cap must roll back both domain consumptions");
            assert!(error.contains("Custom(21)"), "unexpected error: {error}");
            assert_eq!(tracked.map(|key| env.svm.get_account(&key)), before);
            book.check(&env, &portfolios);

            env.svm.expire_blockhash();
            peak_cu = peak_cu.max(env.convert_released_pnl_with_cu(
                &owners[actor],
                portfolios[actor],
                u128::MAX,
            ));
            let after = env.portfolio_state(portfolios[actor]);
            assert_eq!(after.capital.get() - CAPITAL, expected);
            assert_eq!(after.pnl.get(), 0);
            assert_eq!(env.svm.get_account(&env.vault), before[1]);
            assert_eq!(
                env.svm.get_account(&portfolios[1 - actor]),
                before[3 + 1 - actor]
            );
            for index in 0..2 {
                assert!(per_domain[index] <= book.fresh[index]);
                book.fresh[index] -= per_domain[index];
                book.spent[index] += per_domain[index];
                book.receivable[index] += per_domain[index];
            }
            book.claims[actor] = [0; 2];
            gains[actor] = expected;
            book.check(&env, &portfolios);
        }
        assert_eq!(book.fresh, [0; 2]);
        assert_eq!(book.receivable, [80, 120]);
        assert_eq!(gains, if first == 0 { [110, 105] } else { [115, 90] });
        for actor in 0..4 {
            let amount = if actor < 2 {
                CAPITAL + gains[actor]
            } else {
                CAPITAL - 200
            };
            let destination = env.withdraw(&owners[actor], portfolios[actor], amount);
            assert_eq!(env.token_amount(destination) as u128, amount);
            book.check(&env, &portfolios);
        }
        assert_eq!(
            env.token_amount(env.vault),
            402,
            "only expired stock remains"
        );
    }
    assert_cu_within("INV-028 shared discounted refill", peak_cu, 1_000_000);
    eprintln!("INV-028 shared discounted refill: both claimant orders passed; peak CU={peak_cu}");
}
