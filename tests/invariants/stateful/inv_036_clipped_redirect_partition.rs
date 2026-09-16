//! INV-024/036/038/040: clip each payer's fees before redirecting each leg's actual
//! charge. Four transports, both payer roles and leg orders, and capital one atom
//! either side of the first fee exhaust 32 worlds through three distinct recipient
//! payouts and the funded trader's withdrawal. Expected values use only deposits,
//! signed quantities and policies, never observed fee deltas or production helpers.

use crate::support::{
    fuzz_model::{
        assert_market_stock_census, assert_public_encumbrance_census, assert_public_stock_census,
    },
    v16_svm::{MarketConfig, TxSuccess, V16Svm},
};
use percolator::POS_SCALE;
use percolator_prog::{
    error::PercolatorError,
    ix::{BatchTradeCpiLeg, BatchTradeLeg},
    processor::ASSET_AUTH_INSURANCE_OPERATOR,
    state,
};

const PRICE: u64 = 101;
const REDIRECT: u128 = 3_333;
const FUNDED: u128 = 1_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Partition {
    capital: [u128; 2],
    domains: [u128; 6],
}

impl Partition {
    fn read(group: &state::MarketGroupV16, accounts: &[state::PortfolioAccountV16]) -> Self {
        Self {
            capital: std::array::from_fn(|i| accounts[i].capital.get()),
            domains: group.insurance_domain_budget.as_slice().try_into().unwrap(),
        }
    }

    fn charge(&mut self, asset: usize, low_close_size: i128) {
        let requested = (low_close_size.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
        for actor in 0..2 {
            let paid = requested.min(self.capital[actor]);
            self.capital[actor] -= paid;
            let redirected = paid * REDIRECT / 10_000;
            let local = paid - redirected;
            let base_long = redirected / 2;
            let base_short = redirected - base_long;
            assert_eq!(paid, local + base_long + base_short);
            let side = usize::from((low_close_size > 0) == (actor == 1));
            self.domains[2 * asset + side] += local;
            self.domains[0] += base_long;
            self.domains[1] += base_short;
        }
    }
}

fn verify(env: &V16Svm, expected: &Partition, payouts: [u128; 5]) {
    let group = env.primary_market_state().1;
    let accounts: Vec<_> = (0..5).map(|i| env.primary_portfolio(i)).collect();
    assert_eq!(Partition::read(&group, &accounts), *expected);
    assert_eq!(group.c_tot, expected.capital.iter().sum::<u128>());
    assert_eq!(group.insurance, expected.domains.iter().sum::<u128>());
    assert_eq!(group.vault, group.c_tot + group.insurance);
    for actor in 0..5 {
        assert_eq!(accounts[actor].pnl.get(), 0);
        assert_eq!(
            u128::from(env.token_amount(env.actors[actor].destination_token)),
            payouts[actor]
        );
        if actor >= 2 {
            assert_eq!(accounts[actor].capital.get(), 0);
        }
    }
    assert_public_stock_census("clipped redirect", env).unwrap();
    assert_public_encumbrance_census("clipped redirect", env).unwrap();
}

fn reject(result: Result<TxSuccess, String>, expected: PercolatorError) {
    let error = result.expect_err("recipient-local fee capacity must reject");
    let expected = format!("InstructionError(3, Custom({}))", expected as u32);
    assert!(error.contains(&expected), "expected {expected}: {error}");
}

#[test]
fn v16_program_clipped_redirect_fees_preserve_rounding_and_recipient_payouts() {
    let mut worlds = 0;
    let mut transactions = 0;
    let mut rejections = 0;
    let mut controls = 0;
    let mut max_cu = 0;
    for reverse in [false, true] {
        let mut close_legs = [(1u16, -(POS_SCALE as i128)), (2, 2 * POS_SCALE as i128)];
        if reverse {
            close_legs.reverse();
        }
        let first_fee = close_legs[0].1.unsigned_abs() * u128::from(PRICE) / POS_SCALE;
        for low_deposit in [first_fee - 1, first_fee + 1] {
            let mut baseline = None;
            for cpi in [false, true] {
                for batch in [false, true] {
                    for swap_roles in [false, true] {
                        let mut env = V16Svm::new(
                            [0x40; 32],
                            MarketConfig {
                                initial_price: PRICE,
                                maintenance_margin_bps: 1_000,
                                initial_margin_bps: 1_000,
                                max_price_move_bps_per_slot: 500,
                                max_accrual_dt_slots: 1,
                                min_funding_lifetime_slots: 1,
                                actor_deposits: [low_deposit, FUNDED, 0, 0, 0],
                                ..MarketConfig::default()
                            },
                        );
                        let foreign = env.market_data(true);
                        let supply = env.token_supply_observed();
                        let recipients: Vec<_> =
                            (2..5).map(|i| env.primary_portfolio_data(i)).collect();
                        let (a, b, sign) = if swap_roles { (1, 0, -1) } else { (0, 1, 1) };
                        let mut expected = Partition {
                            capital: [low_deposit, FUNDED],
                            domains: [0; 6],
                        };
                        let mut payouts = [0; 5];
                        env.begin_public_trace();
                        for asset in 0..3 {
                            env.update_asset_authority_from_admin(
                                asset,
                                ASSET_AUTH_INSURANCE_OPERATOR,
                                asset as usize + 2,
                            )
                            .unwrap();
                        }
                        env.batch_trade_no_cpi(
                            0,
                            1,
                            close_legs
                                .iter()
                                .map(|&(asset, size)| BatchTradeLeg {
                                    asset_index: asset,
                                    market_id: env.primary_market_state().1.assets[asset as usize]
                                        .market_id,
                                    size_q: -size,
                                    exec_price: PRICE,
                                    fee_bps: 0,
                                })
                                .collect(),
                        )
                        .unwrap();
                        env.update_trade_fee_policy(10_000).unwrap();
                        env.update_fee_redirect_policy(REDIRECT as u16).unwrap();
                        verify(&env, &expected, payouts);
                        let vault_before = env.svm.get_account(&env.vault).unwrap();
                        let schedules = if batch {
                            vec![close_legs.to_vec()]
                        } else {
                            close_legs.iter().map(|leg| vec![*leg]).collect()
                        };
                        for legs in schedules {
                            if cpi {
                                // Each close binds the LP's current position episode.
                                env.set_matcher_config_with_trade_fee_cap(b, 1, 10_000)
                                    .unwrap();
                            }
                            let ids = env
                                .primary_market_state()
                                .1
                                .assets
                                .iter()
                                .map(|asset| asset.market_id)
                                .collect::<Vec<_>>();
                            match (cpi, batch) {
                                (false, false) => {
                                    let (asset, size) = legs[0];
                                    env.trade_no_cpi(a, b, asset, sign * size, PRICE, 10_000)
                                        .unwrap();
                                }
                                (true, false) => {
                                    let (asset, size) = legs[0];
                                    env.trade_cpi(a, b, asset, sign * size, 10_000, PRICE)
                                        .unwrap();
                                }
                                (false, true) => {
                                    env.batch_trade_no_cpi(
                                        a,
                                        b,
                                        legs.iter()
                                            .map(|&(asset, size)| BatchTradeLeg {
                                                asset_index: asset,
                                                market_id: ids[asset as usize],
                                                size_q: sign * size,
                                                exec_price: PRICE,
                                                fee_bps: 10_000,
                                            })
                                            .collect(),
                                    )
                                    .unwrap();
                                }
                                (true, true) => {
                                    env.batch_trade_cpi(
                                        a,
                                        b,
                                        legs.iter()
                                            .map(|&(asset, size)| BatchTradeCpiLeg {
                                                asset_index: asset,
                                                market_id: ids[asset as usize],
                                                size_q: sign * size,
                                                limit_price: PRICE,
                                                fee_bps: 10_000,
                                            })
                                            .collect(),
                                    )
                                    .unwrap();
                                }
                            }
                            for (asset, size) in legs {
                                expected.charge(asset as usize, size);
                            }
                            verify(&env, &expected, payouts);
                            assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
                        }
                        assert_eq!(expected.capital, [0, FUNDED - 303]);
                        assert_eq!(
                            expected.domains,
                            match (reverse, low_deposit > first_fee) {
                                (false, false) => [65, 68, 68, 67, 0, 135],
                                (false, true) => [65, 68, 68, 68, 1, 135],
                                (true, false) => [82, 84, 68, 0, 135, 135],
                                (true, true) => [82, 85, 68, 1, 135, 135],
                            }
                        );
                        let group = env.primary_market_state().1;
                        for asset in &group.assets {
                            assert_eq!(asset.oi_eff_long_q, 0);
                            assert_eq!(asset.oi_eff_short_q, 0);
                        }
                        // Balanced bad substitutions pass stock conservation but fail attribution.
                        let accounts: Vec<_> = (0..5).map(|i| env.primary_portfolio(i)).collect();
                        let raw = env.market_data(false);
                        let mut pooled_rounding = group.clone();
                        let total_redirect = expected.domains[0] + expected.domains[1];
                        pooled_rounding.insurance_domain_budget[0] = total_redirect / 2;
                        pooled_rounding.insurance_domain_budget[1] =
                            total_redirect - total_redirect / 2;
                        let mut wrong_recipient = group.clone();
                        wrong_recipient.insurance_domain_budget[2] += 1;
                        wrong_recipient.insurance_domain_budget[5] -= 1;
                        for wrong in [pooled_rounding, wrong_recipient] {
                            assert_market_stock_census(
                                "balanced wrong redirect",
                                &wrong,
                                &raw,
                                &accounts,
                                group.vault,
                            )
                            .unwrap();
                            assert_ne!(Partition::read(&wrong, &accounts), expected);
                            controls += 1;
                        }
                        let mut wrong_payer = accounts.clone();
                        wrong_payer[0].capital = percolator::V16PodU128::new(1);
                        wrong_payer[1].capital = percolator::V16PodU128::new(FUNDED - 304);
                        assert_market_stock_census(
                            "balanced wrong payer",
                            &group,
                            &raw,
                            &wrong_payer,
                            group.vault,
                        )
                        .unwrap();
                        assert_ne!(Partition::read(&group, &wrong_payer), expected);
                        controls += 1;

                        // Changing policy after collection cannot redirect already earned atoms.
                        env.update_fee_redirect_policy(10_000).unwrap();
                        verify(&env, &expected, payouts);
                        for asset in 0..3 {
                            let recipient = asset + 2;
                            let amount =
                                expected.domains[2 * asset] + expected.domains[2 * asset + 1];
                            assert!(amount > 0);
                            reject(
                                env.withdraw_insurance_asset(recipient, asset as u16, amount + 1),
                                PercolatorError::EngineLockActive,
                            );
                            reject(
                                env.withdraw_insurance_asset(2 + (asset + 1) % 3, asset as u16, 1),
                                PercolatorError::InvalidTokenAccount,
                            );
                            verify(&env, &expected, payouts);
                            env.withdraw_insurance_asset(recipient, asset as u16, amount)
                                .unwrap();
                            expected.domains[2 * asset..2 * asset + 2].fill(0);
                            payouts[recipient] = amount;
                            verify(&env, &expected, payouts);
                            reject(
                                env.withdraw_insurance_asset(recipient, asset as u16, 1),
                                PercolatorError::EngineLockActive,
                            );
                        }
                        env.withdraw_primary(1, FUNDED - 303).unwrap();
                        expected.capital[1] = 0;
                        payouts[1] = FUNDED - 303;
                        verify(&env, &expected, payouts);
                        assert_eq!(payouts.iter().sum::<u128>(), low_deposit + FUNDED);
                        assert_eq!(env.market_data(true), foreign);
                        assert_eq!(env.token_supply_observed(), supply);
                        for (i, bytes) in recipients.iter().enumerate() {
                            assert_eq!(env.primary_portfolio_data(i + 2), *bytes);
                        }
                        if let Some(previous) = baseline {
                            assert_eq!(
                                payouts, previous,
                                "transport and participant order preserve all five payouts"
                            );
                        } else {
                            baseline = Some(payouts);
                        }
                        let trace = env.finish_public_trace();
                        trace.validate_public_execution().unwrap();
                        assert_eq!(trace.out_of_band_economic_mutations, 0);
                        let rejected = trace.steps.iter().filter(|step| !step.succeeded).count();
                        assert_eq!(rejected, 9);
                        rejections += rejected;
                        transactions += trace.steps.len();
                        max_cu = max_cu.max(
                            trace
                                .steps
                                .iter()
                                .filter_map(|step| step.compute_units)
                                .max()
                                .unwrap(),
                        );
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    assert_eq!(transactions, 712);
    assert_eq!(rejections, 288);
    assert_eq!(controls, 96);
    eprintln!("clipped redirect: {worlds} worlds, {transactions} transactions, {rejections} exact-rollback rejections, {controls} balanced controls, max CU={max_cu}");
}
