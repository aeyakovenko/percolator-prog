//! INV-088: a current market clock or recent reward credit is not a local fee proof.
//!
//! Unlike the position/domain summary matrices, this witness checks a local value
//! decision: Withdraw must price each flat account's own unpaid lifetime. Three
//! different creation slots and two explicit fee syncs leave unequal cursors;
//! receiving a real cranker reward touches capital but does not pay the recipient's
//! fees. All six withdrawal orders run with and without an unrelated asset crank.
//! Expected charges use only fixture slots, principal, and public fee policy, not
//! engine fee helpers or observed account deltas. Program state is initialized and
//! advanced only by public wrapper instructions through the standard CU fixture.
//!
//! Boundary: live, funded, flat portfolios with unclipped fees and a fixed policy;
//! this does not claim coverage of nonflat loss anchors or terminal fee rules.

use super::*;

#[test]
fn v16_program_withdraw_fee_cursor_is_account_local_after_reward_and_global_clock_touches() {
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    const PRINCIPALS: [u128; 3] = [20_003, 30_017, 40_029];
    const BIRTH_SLOTS: [u64; 3] = [0, 3, 7];
    const RATE: u128 = 17;
    const SHARE_BPS: u16 = 3_333;
    const REWARD_SLOT: u64 = 11;
    const SYNC_SLOT: u64 = 13;
    const EXIT_SLOT: u64 = 19;

    let mut max_sync_cu = 0;
    let mut max_crank_cu = 0;
    let mut max_withdraw_cu = 0;
    let mut worlds = 0;
    for advance_global_clock in [false, true] {
        for order in ORDERS {
            let context = format!("advance_global_clock={advance_global_clock}, order={order:?}");
            let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
                max_portfolio_assets: 2,
                max_accrual_dt_slots: 32,
                min_funding_lifetime_slots: 32,
                max_price_move_bps_per_slot: 100,
                maintenance_fee_per_slot: RATE,
                ..V16CuMarketParams::default()
            });
            env.configure_auth_mark_for_asset_as_admin(1, 0, 100);
            env.update_maintenance_fee_policy_with_cu(SHARE_BPS);
            let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
            let portfolios = [0, 1, 2].map(|actor| {
                env.svm.warp_to_slot(BIRTH_SLOTS[actor]);
                let portfolio = env.create_portfolio(&owners[actor]);
                let source = env.deposit(&owners[actor], portfolio, PRINCIPALS[actor]);
                assert_eq!(env.token_amount(source), 0, "{context}: funded principal");
                portfolio
            });
            let mint_before = env.svm.get_account(&env.mint).unwrap();
            let total_principal = PRINCIPALS.iter().sum::<u128>();
            let assert_census = |env: &V16CuEnv,
                                 capital: [u128; 3],
                                 fee_slots: [u64; 3],
                                 insurance: u128,
                                 paid_out: u128| {
                let states = portfolios.map(|key| env.portfolio_state(key));
                for actor in 0..3 {
                    assert_eq!(
                        states[actor].capital.get(),
                        capital[actor],
                        "{context}: account {actor} capital"
                    );
                    assert_eq!(
                        states[actor].last_fee_slot.get(),
                        fee_slots[actor],
                        "{context}: account {actor} local fee cursor"
                    );
                    assert_eq!(states[actor].pnl.get(), 0);
                    assert!(states[actor]
                        .active_bitmap
                        .iter()
                        .all(|word| word.get() == 0));
                }
                let group = env.market_state().1;
                let market = env.svm.get_account(&env.market).unwrap();
                let raw = market_group_header_bytes(&market.data);
                let independent_capital = states.iter().map(|p| p.capital.get()).sum::<u128>();
                assert_eq!(independent_capital, capital.iter().sum::<u128>());
                assert_eq!(
                    raw.c_tot.get(),
                    independent_capital,
                    "{context}: raw capital"
                );
                assert_eq!(
                    group.c_tot, independent_capital,
                    "{context}: capital census"
                );
                assert_eq!(group.insurance, insurance, "{context}: retained fees");
                assert_eq!(group.materialized_portfolio_count, 3);
                assert_eq!(group.vault, total_principal - paid_out);
                assert_eq!(group.vault, independent_capital + insurance);
                assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
                inv_088_assert_insurance_budget_summary_matches_domain_scan(env, &context);
            };
            let mut capital = PRINCIPALS;
            let mut fee_slots = BIRTH_SLOTS;
            let mut insurance = 0;
            assert_census(&env, capital, fee_slots, insurance, 0);

            env.svm.warp_to_slot(REWARD_SLOT);
            let untouched = env.svm.get_account(&portfolios[2]).unwrap();
            let charged = u128::from(REWARD_SLOT - BIRTH_SLOTS[0]) * RATE;
            let reward = charged * u128::from(SHARE_BPS) / 10_000;
            assert!(reward > 0 && charged < PRINCIPALS[0]);
            let cu =
                env.sync_maintenance_fee_with_cu(portfolios[0], Some(portfolios[1]), REWARD_SLOT);
            max_sync_cu = max_sync_cu.max(cu);
            assert_cu_within("INV-088 reward credit", cu, CUSTODY_CU_LIMIT);
            capital[0] -= charged;
            capital[1] += reward;
            fee_slots[0] = REWARD_SLOT;
            insurance += charged - reward;
            assert_census(&env, capital, fee_slots, insurance, 0);
            assert_eq!(env.svm.get_account(&portfolios[2]).unwrap(), untouched);
            assert_eq!(
                fee_slots[1], BIRTH_SLOTS[1],
                "the reward recipient still owes its own elapsed fees"
            );

            env.svm.warp_to_slot(SYNC_SLOT);
            let peers_before =
                [portfolios[0], portfolios[1]].map(|key| env.svm.get_account(&key).unwrap());
            let charged = u128::from(SYNC_SLOT - BIRTH_SLOTS[2]) * RATE;
            let cu = env.sync_maintenance_fee_with_cu(portfolios[2], None, SYNC_SLOT);
            max_sync_cu = max_sync_cu.max(cu);
            assert_cu_within("INV-088 unrelated account sync", cu, CUSTODY_CU_LIMIT);
            capital[2] -= charged;
            fee_slots[2] = SYNC_SLOT;
            insurance += charged;
            assert_census(&env, capital, fee_slots, insurance, 0);
            assert_eq!(
                [portfolios[0], portfolios[1]].map(|key| env.svm.get_account(&key).unwrap()),
                peers_before,
                "{context}: unrelated sync must not rewrite either peer"
            );

            env.svm.warp_to_slot(EXIT_SLOT);
            assert_eq!(env.market_state().1.current_slot, 0);
            if advance_global_clock {
                // Advance an asset with no account exposure while all three local fee cursors lag.
                let cu = env.crank(
                    portfolios[2],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: EXIT_SLOT,
                        observations: crank_observations(1),
                    },
                );
                max_crank_cu = max_crank_cu.max(cu);
                assert_cu_within("INV-088 unrelated asset clock", cu, CRANK_CU_LIMIT);
                let group = env.market_state().1;
                assert_eq!(
                    group.current_slot, EXIT_SLOT,
                    "{context}: clock must advance"
                );
                assert_eq!(group.assets[1].slot_last, EXIT_SLOT);
                assert_eq!(group.assets[0].slot_last, 0);
                assert_eq!(
                    [portfolios[0], portfolios[1]].map(|key| env.svm.get_account(&key).unwrap()),
                    peers_before,
                    "{context}: global progress cannot certify peer fee payment"
                );
            }
            assert_census(&env, capital, fee_slots, insurance, 0);
            assert_eq!(fee_slots, [REWARD_SLOT, BIRTH_SLOTS[1], SYNC_SLOT]);
            assert!(fee_slots.iter().all(|slot| *slot < EXIT_SLOT));

            let lifetime_fees = BIRTH_SLOTS.map(|birth| u128::from(EXIT_SLOT - birth) * RATE);
            let expected_payouts = [
                PRINCIPALS[0] - lifetime_fees[0],
                PRINCIPALS[1] + reward - lifetime_fees[1],
                PRINCIPALS[2] - lifetime_fees[2],
            ];
            let mut payouts = [0u128; 3];
            for actor in order {
                let before = portfolios.map(|key| env.svm.get_account(&key).unwrap());
                let remaining_fee = u128::from(EXIT_SLOT - fee_slots[actor]) * RATE;
                assert!(remaining_fee > 0 && remaining_fee < capital[actor]);
                assert_eq!(capital[actor] - remaining_fee, expected_payouts[actor]);
                // Submit the independently known pre-fee balance to exercise atomic withdraw-all.
                let (destination, cu) =
                    env.withdraw_with_cu(&owners[actor], portfolios[actor], capital[actor]);
                max_withdraw_cu = max_withdraw_cu.max(cu);
                assert_cu_within("INV-088 account-local withdraw-all", cu, CUSTODY_CU_LIMIT);
                assert_eq!(
                    u128::from(env.token_amount(destination)),
                    expected_payouts[actor],
                    "{context}: account {actor} payout must use its own unpaid interval"
                );
                capital[actor] = 0;
                fee_slots[actor] = EXIT_SLOT;
                insurance += remaining_fee;
                payouts[actor] = expected_payouts[actor];
                assert_census(&env, capital, fee_slots, insurance, payouts.iter().sum());
                for peer in (0..3).filter(|peer| *peer != actor) {
                    assert_eq!(
                        env.svm.get_account(&portfolios[peer]).unwrap(),
                        before[peer],
                        "{context}: withdrawing account {actor} must preserve peer {peer}"
                    );
                }
            }
            assert_eq!(
                payouts, expected_payouts,
                "{context}: order-independent exits"
            );
            assert_eq!(insurance, lifetime_fees.iter().sum::<u128>() - reward);
            assert_eq!(capital, [0; 3]);
            worlds += 1;
        }
    }
    assert_eq!(worlds, 12);
    eprintln!(
        "INV-088 account-local fee cursors: worlds={worlds}, max_sync_cu={max_sync_cu}, \
         max_crank_cu={max_crank_cu}, max_withdraw_cu={max_withdraw_cu}"
    );
}
