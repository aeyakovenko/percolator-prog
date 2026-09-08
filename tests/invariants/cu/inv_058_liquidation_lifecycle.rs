//! Lifecycle evidence for INV-058, not closure of row 427's disjoint-owner OI gate.
//! Full liquidation releases effective OI while retaining a prior-reset raw leg.
//! Close/reinitialize/deposit cannot skip that reset; afterwards a fresh trade
//! reuses the released capacity without losing a surviving second-asset notional.
//! No split-fill, cross-zero, fee-cap, or scalar max/max-plus-one probe is used.

use super::*;

const PRICE: u64 = 100;
const CAPITAL: u128 = 20_000_000_000;
const MM_FLOOR: u128 = 800_000_000;
const MAINTENANCE: u128 = 2;

struct Ledger {
    raw: [[i128; 2]; PRIMARY_ACTOR_COUNT],
    effective: [[i128; 2]; PRIMARY_ACTOR_COUNT],
    cached_notional: [u128; PRIMARY_ACTOR_COUNT],
    capital: [u128; PRIMARY_ACTOR_COUNT],
    insurance: u128,
}

fn notional(q: i128) -> u128 {
    let product = q.unsigned_abs().checked_mul(u128::from(PRICE)).unwrap();
    product / POS_SCALE + u128::from(product % POS_SCALE != 0)
}

impl Ledger {
    fn recertify(&mut self, actor: usize) {
        self.cached_notional[actor] = self.effective[actor]
            .into_iter()
            .map(notional)
            .try_fold(0u128, |total, n| total.checked_add(n))
            .unwrap();
    }

    fn check(&self, env: &V16Svm, label: &str) {
        let (_, group) = env.primary_market_state();
        let mut oi = [[0u128; 2]; 2];
        let mut stored = [[0u64; 2]; 2];
        for actor in 0..PRIMARY_ACTOR_COUNT {
            let account = env.primary_portfolio(actor);
            assert_eq!(
                account.capital.get(),
                self.capital[actor],
                "{label} actor {actor}"
            );
            assert_eq!(
                account.pnl.get(),
                0,
                "fixed mark, no funding or trading fees"
            );
            let mut decoded = [0i128; 2];
            for encoded in &account.legs {
                let leg = encoded.try_to_runtime().expect("public leg decode");
                if !leg.active {
                    continue;
                }
                let index = usize::try_from(leg.asset_index).unwrap();
                assert!(index < 2, "{label}: no unmodeled exposure");
                assert_eq!(decoded[index], 0, "one net leg per asset");
                decoded[index] = leg.basis_pos_q;
                assert_ne!(leg.basis_pos_q, 0);
                let side = usize::from(leg.basis_pos_q < 0);
                assert_eq!(
                    leg.side,
                    if side == 0 {
                        SideV16::Long
                    } else {
                        SideV16::Short
                    }
                );
                stored[index][side] += 1;
                let asset = group.assets[index];
                let (a, epoch, mode) = if side == 0 {
                    (asset.a_long, asset.epoch_long, asset.mode_long)
                } else {
                    (asset.a_short, asset.epoch_short, asset.mode_short)
                };
                assert_eq!(leg.market_id, asset.market_id);
                assert_eq!(leg.a_basis, ADL_ONE);
                assert_eq!(a, ADL_ONE, "full drain resets A, never partial ADL here");
                let effective = if leg.epoch_snap == epoch {
                    assert_eq!(mode, SideModeV16::Normal);
                    leg.basis_pos_q
                } else {
                    assert_eq!(leg.epoch_snap.checked_add(1), Some(epoch));
                    assert_eq!(mode, SideModeV16::ResetPending);
                    0
                };
                assert_eq!(
                    effective, self.effective[actor][index],
                    "{label} actor {actor}"
                );
                assert!(leg.basis_pos_q.unsigned_abs() <= percolator::MAX_POSITION_ABS_Q);
                oi[index][side] = oi[index][side]
                    .checked_add(effective.unsigned_abs())
                    .unwrap();
            }
            assert_eq!(
                decoded, self.raw[actor],
                "{label} actor {actor} retained basis"
            );
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&account)),
                self.raw[actor].into_iter().filter(|q| *q != 0).count() as u32,
                "{label} actor {actor} stored-leg count"
            );
            let cert = health_cert(&account);
            if cert.valid {
                assert_eq!(
                    cert.certified_worst_case_loss, self.cached_notional[actor],
                    "{label} actor {actor} cached ceil notional"
                );
            }
            assert!(self.cached_notional[actor] <= percolator::MAX_ACCOUNT_NOTIONAL);
        }
        for (index, asset) in group.assets.iter().enumerate() {
            let expected_oi = oi.get(index).copied().unwrap_or([0; 2]);
            let expected_stored = stored.get(index).copied().unwrap_or([0; 2]);
            assert_eq!(
                [asset.oi_eff_long_q, asset.oi_eff_short_q],
                expected_oi,
                "{label} asset {index}"
            );
            assert_eq!(
                [asset.stored_pos_count_long, asset.stored_pos_count_short],
                expected_stored
            );
            assert_eq!(expected_oi[0], expected_oi[1]);
            for q in expected_oi {
                assert!(q <= percolator::MAX_OI_SIDE_Q);
            }
            if index < 2 {
                assert_eq!(asset.effective_price, PRICE);
                assert_eq!(asset.raw_oracle_target_price, PRICE);
                assert_eq!(asset.lifecycle, AssetLifecycleV16::Active);
            }
        }
        let capital: u128 = self.capital.iter().sum();
        assert_eq!(group.c_tot, capital, "{label} capital census");
        assert_eq!(
            group.insurance, self.insurance,
            "{label} maintenance attribution"
        );
        assert_eq!(group.vault, capital + self.insurance, "{label} custody");
        assert_public_stock_census(label, env).unwrap();
        assert_public_encumbrance_census(label, env).unwrap();
    }
}

#[test]
fn v16_program_liquidation_reset_reopen_reuses_capacity_and_preserves_live_notional() {
    let cap = percolator::MAX_POSITION_ABS_Q;
    assert_eq!(cap, percolator::MAX_OI_SIDE_Q);
    let old_q = i128::try_from(3 * cap / 4 + POS_SCALE / 7).unwrap();
    let live_q = i128::try_from(cap / 4 + POS_SCALE / 3).unwrap();
    let new_q = i128::try_from(5 * cap / 8 + POS_SCALE / 7).unwrap();
    assert!(old_q.unsigned_abs() + new_q.unsigned_abs() > cap);
    assert!(old_q.unsigned_abs() < cap && new_q.unsigned_abs() < cap);
    assert!(notional(old_q) * 500 / 10_000 < MM_FLOOR);

    for direction in [-1i128, 1] {
        for route in INV_058_TRADE_ROUTES {
            let mut config = inv_058_max_position_config();
            config.min_nonzero_mm_req = MM_FLOOR;
            config.min_nonzero_im_req = MM_FLOOR + 1;
            config.maintenance_margin_bps = 500;
            config.initial_margin_bps = 500;
            config.max_price_move_bps_per_slot = 24;
            config.maintenance_fee_per_slot = MAINTENANCE;
            config.actor_deposits[1] = MM_FLOOR + 1;
            config.actor_token_balances[1] = (2 * CAPITAL) as u64;
            let mut env = V16Svm::new([0x58; 32], config);
            env.begin_public_trace();
            let mut ledger = Ledger {
                raw: [[0; 2]; PRIMARY_ACTOR_COUNT],
                effective: [[0; 2]; PRIMARY_ACTOR_COUNT],
                cached_notional: [0; PRIMARY_ACTOR_COUNT],
                capital: config.actor_deposits,
                insurance: 0,
            };
            ledger.check(&env, "initial");

            // The survivor carries a second live leg, so reset cleanup must recertify
            // its remaining risk rather than zeroing the whole account's notional.
            for (taker, maker, asset, q) in [(0, 1, 0, old_q), (0, 2, 1, live_q)] {
                env.trade_no_cpi(taker, maker, asset, direction * q, PRICE, 0)
                    .unwrap();
                ledger.raw[taker][asset as usize] = direction * q;
                ledger.raw[maker][asset as usize] = -direction * q;
                ledger.effective = ledger.raw;
                ledger.recertify(taker);
                ledger.recertify(maker);
                ledger.check(&env, "open");
            }
            env.warp_to_slot(2);
            env.crank(4, 2, crank_observations_for_assets(&[0, 1]))
                .expect("public keeper advances both loss-current asset clocks");
            ledger.check(&env, "loss-current clocks");
            let live_asset_before = env.primary_market_state().1.assets[1];
            let untouched_portfolio = env.primary_portfolio_data(2);
            let survivor_before = env.primary_portfolio_data(0);
            let old_id = env.primary_portfolio_id(1);
            let address = env.actors[1].portfolio;
            let custody_before = env.all_token_account_data();

            // One authenticated elapsed slot leaves positive equity below the MM
            // floor. No nonzero residual can be healthy: the selected close is full.
            env.sync_maintenance_fee(1, 2).unwrap();
            ledger.capital[1] -= MAINTENANCE;
            ledger.insurance += MAINTENANCE;
            assert!(ledger.capital[1] > 0 && ledger.capital[1] < MM_FLOOR);
            ledger.check(&env, "maintenance deficit");
            let mut liquidation_steps = 0;
            while has_active_leg_for_asset(&env.primary_portfolio(1), 0) && liquidation_steps < 3 {
                env.crank(1, 2, crank_observations(0)).unwrap();
                liquidation_steps += 1;
                if !has_active_leg_for_asset(&env.primary_portfolio(1), 0) {
                    ledger.raw[1][0] = 0;
                    ledger.effective[0][0] = 0;
                    ledger.effective[1][0] = 0;
                    ledger.recertify(1);
                }
                ledger.check(&env, "liquidation progress");
            }
            assert_eq!(ledger.raw[1], [0; 2], "bounded full liquidation");
            assert_eq!(
                env.primary_portfolio_data(0),
                survivor_before,
                "liquidation retains the winner's old basis and conservative cache"
            );
            assert_eq!(env.primary_portfolio_data(2), untouched_portfolio);
            assert_eq!(env.primary_market_state().1.assets[1], live_asset_before);
            assert_eq!(env.all_token_account_data(), custody_before);
            let reset_side = u8::from(direction < 0);
            let reset_asset = env.primary_market_state().1.assets[0];
            assert_eq!(
                if reset_side == 0 {
                    reset_asset.mode_long
                } else {
                    reset_asset.mode_short
                },
                SideModeV16::ResetPending
            );

            let destination_before = env.token_amount(env.actors[1].destination_token);
            let withdrawal = ledger.capital[1];
            env.withdraw_primary(1, withdrawal).unwrap();
            ledger.capital[1] = 0;
            ledger.check(&env, "liquidated owner withdrawal");
            assert_eq!(
                env.token_amount(env.actors[1].destination_token) - destination_before,
                u64::try_from(withdrawal).unwrap()
            );
            env.close_primary_portfolio(1).unwrap();
            assert_eq!(env.svm.get_account(&address).unwrap().lamports, 0);
            env.fund_closed_primary_portfolio(1, 1_000_000_000).unwrap();
            env.reinitialize_primary_portfolio(1).unwrap();
            assert_eq!(env.actors[1].portfolio, address);
            assert!(env.primary_portfolio_id(1) > old_id);
            ledger.check(&env, "reinitialized");
            env.deposit_primary(1, CAPITAL).unwrap();
            ledger.capital[1] = CAPITAL;
            ledger.check(&env, "redeposited");

            // Recreated actor is the taker; the surviving maker's public matcher
            // capability is current. This is a lifecycle gate, not stale consent.
            if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                env.ensure_primary_matcher_enabled(0).unwrap();
            }
            let before = inv_058_economic_snapshot(&env);
            let error = execute_trade_route(&mut env, route, 1, 0, 0, -direction * new_q, PRICE, 0)
                .expect_err("zero OI does not authorize new risk before reset cleanup");
            assert!(
                error.contains(&format!(
                    "Custom({})",
                    PercolatorError::EngineLockActive as u32
                )),
                "{route:?} direction={direction}: {error}"
            );
            assert_eq!(inv_058_economic_snapshot(&env), before);
            ledger.check(&env, "reset-gated retrade rollback");

            for _ in 0..3 {
                if !has_active_leg_for_asset(&env.primary_portfolio(0), 0) {
                    break;
                }
                env.crank(0, 2, vec![]).unwrap();
            }
            assert!(!has_active_leg_for_asset(&env.primary_portfolio(0), 0));
            ledger.raw[0][0] = 0;
            ledger.capital[0] -= MAINTENANCE;
            ledger.insurance += MAINTENANCE;
            ledger.recertify(0);
            ledger.check(&env, "prior-reset residue cleanup");
            let asset = env.primary_market_state().1.assets[0];
            if (if reset_side == 0 {
                asset.mode_long
            } else {
                asset.mode_short
            }) == SideModeV16::ResetPending
            {
                env.finalize_reset_side(0, reset_side).unwrap();
            }
            let asset = env.primary_market_state().1.assets[0];
            assert_eq!(
                [asset.mode_long, asset.mode_short],
                [SideModeV16::Normal; 2]
            );
            ledger.check(&env, "reset finalized");

            execute_trade_route(&mut env, route, 1, 0, 0, -direction * new_q, PRICE, 0)
                .unwrap_or_else(|error| {
                    panic!("{route:?} direction={direction} post-reset retrade: {error}")
                });
            ledger.raw[0][0] = direction * new_q;
            ledger.raw[1][0] = -direction * new_q;
            ledger.effective = ledger.raw;
            ledger.recertify(0);
            ledger.recertify(1);
            ledger.check(&env, "released capacity reused");
            assert!(health_cert(&env.primary_portfolio(0)).valid);
            assert!(health_cert(&env.primary_portfolio(1)).valid);
            assert_eq!(
                ledger.cached_notional[0],
                notional(new_q) + notional(live_q)
            );
            assert_eq!(
                env.primary_portfolio_data(2),
                untouched_portfolio,
                "unconsumed counterparty remains byte-exact throughout the lifecycle"
            );

            let before = inv_058_economic_snapshot(&env);
            let error = env
                .crank(0, 2, vec![])
                .expect_err("cleanup retry cannot release the new leg");
            assert!(
                error.contains(&format!(
                    "Custom({})",
                    PercolatorError::EngineNonProgress as u32
                )),
                "{error}"
            );
            assert_eq!(inv_058_economic_snapshot(&env), before);
            ledger.check(&env, "cleanup retry");
            let trace = env.finish_public_trace();
            trace
                .validate_public_execution()
                .expect("public instructions only, exact rejection rollback");
            let rejects = trace.steps.iter().filter(|step| !step.succeeded).count();
            assert_eq!(rejects, 2);
            let max_cu = trace
                .steps
                .iter()
                .filter_map(|step| step.compute_units)
                .max()
                .unwrap();
            assert!(max_cu < TX_CU_LIMIT);
            println!("INV-058 liquidation direction={direction} route={route:?}: steps={} rejects={rejects} liquidation_steps={liquidation_steps} max_cu={max_cu}", trace.steps.len());
        }
    }
}
