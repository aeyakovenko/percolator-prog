//! Rows 419/435: independently priced funding debt crosses two nonzero close
//! residuals, B booking, retained-weight release and resolved SPL payout.
//! The solvent funding ledger and funding-free bankruptcy matrix do not compose
//! these transitions. No arbitrary-history or INV-086 equivalence claim is made.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use percolator::SOCIAL_LOSS_DEN;

const ENTRY: i128 = 1_000_000;
const STEP: i128 = 10_000;
const SLOTS: u64 = 20;
const RATE: i128 = 1_000;

struct Inputs {
    prices: [i128; 2],
    funding: [i128; 2],
    gains: [u128; 2],
    deposits: [u128; 5],
}

impl Inputs {
    fn new(reverse: bool) -> Self {
        let sign = if reverse { -1 } else { 1 };
        let directions = [sign, -sign];
        let prices = directions.map(|d| ENTRY + d * STEP * i128::from(SLOTS));
        // A target published before the first crank activates all twenty
        // capped intervals. Signed funding floors per interval, before scaling.
        let funding = directions.map(|d| {
            (1..=SLOTS)
                .map(|slot| {
                    (d * RATE * (ENTRY + d * STEP * i128::from(slot))).div_euclid(1_000_000_000)
                })
                .sum::<i128>()
        });
        assert_eq!(funding, directions.map(|d| d * i128::from(SLOTS)));
        let gains = std::array::from_fn(|pair| {
            ((pair as i128 + 1) * directions[pair] * (prices[pair] - ENTRY - funding[pair])) as u128
        });
        assert_eq!(gains, [199_980, 399_960]);
        // Exact 9/10 and 3/4 source rates keep conversion rounding out of this
        // funding/residual join. Both debtors start solvent and end bankrupt.
        let deposits = [200_000, gains[0] * 9 / 10, 300_000, gains[1] * 3 / 4, 777];
        Self {
            prices,
            funding,
            gains,
            deposits,
        }
    }

    fn check(&self, world: &AttributionWorld, model: &DebtModel) {
        model.check_with_gains(world, self.gains);
        let group = world.env.market_state().1;
        if group.mode == MarketModeV16::Resolved {
            assert_eq!(group.resolved_slot, SLOTS);
        }
        for pair in 0..2 {
            let a = group.assets[pair + 1];
            assert_eq!(a.effective_price as i128, self.prices[pair]);
            assert_eq!(a.slot_last, SLOTS);
            assert_eq!(
                [a.f_long_num, a.f_short_num],
                [-self.funding[pair], self.funding[pair]].map(|f| f * ADL_ONE as i128)
            );
            let side = usize::from(world.quantities[2 * pair] < 0);
            let mut b = [0; 2];
            if model.booked[pair] {
                let residual = self.gains[pair] - self.deposits[2 * pair + 1];
                let weight = world.quantities[2 * pair].unsigned_abs();
                b[side] = residual * SOCIAL_LOSS_DEN / weight;
                assert_eq!(b[side] * weight, residual * SOCIAL_LOSS_DEN);
            }
            assert_eq!([a.b_long_num, a.b_short_num], b, "domain-local B debit");
        }
        let accounts: Vec<_> = world
            .actors
            .iter()
            .map(|a| world.env.portfolio_state(a.portfolio))
            .collect();
        assert_market_stock_census(
            "funded pending bankruptcy",
            &group,
            &world.env.svm.get_account(&world.env.market).unwrap().data,
            &accounts,
            world.env.token_amount(world.env.vault) as u128,
        )
        .unwrap();
        assert_reservation_encumbrance_census("funded pending bankruptcy", &group, &accounts)
            .unwrap();
        assert_eq!(group.materialized_portfolio_count, 5);
        let mint = Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        for actor in &world.actors {
            let token =
                TokenAccount::unpack(&world.env.svm.get_account(&actor.token).unwrap().data)
                    .unwrap();
            assert_eq!(
                (token.owner, token.mint),
                (actor.owner.pubkey(), world.env.mint)
            );
        }
    }

    fn setup(&self, reverse: bool, peak: &mut u64) -> AttributionWorld {
        let mut world = AttributionWorld::new_with_deposits(
            reverse,
            V16CuMarketParams {
                max_portfolio_assets: 3,
                initial_price: ENTRY as u64,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 100,
                max_abs_funding_e9_per_slot: RATE as u64,
                max_bankrupt_close_lifetime_slots: 1_000,
                ..V16CuMarketParams::default()
            },
            self.deposits,
        );
        let admin = world.env.admin.insecure_clone();
        send_raw_tx(
            &mut world.env.svm,
            &world.env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &world.env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();
        let mut basis = [0; 4];
        let mut pending = [false; 4];
        for pair in 0..2 {
            let holder = &world.actors[2 * pair];
            let debtor = &world.actors[2 * pair + 1];
            let q = world.quantities[2 * pair];
            let cu = world.env.trade_asset_with_cu(
                (pair + 1) as u16,
                &holder.owner,
                holder.portfolio,
                &debtor.owner,
                debtor.portfolio,
                q,
                ENTRY as u64,
                0,
            );
            assert_cu_within("funded bankruptcy opening", cu, TRADE_CU_LIMIT);
            *peak = (*peak).max(cu);
            basis[2 * pair] = q;
            basis[2 * pair + 1] = -q;
            world.check(basis, pending);
        }
        for pair in 0..2 {
            let direction = world.quantities[2 * pair].signum();
            world.env.push_auth_mark_for_asset_as_admin(
                (pair + 1) as u16,
                1,
                (ENTRY + direction * 400_000) as u64,
            );
        }
        for slot in 1..=SLOTS {
            world.env.svm.warp_to_slot(slot);
            let cu = world.env.crank(
                world.actors[4].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations_for_assets(&[1, 2]),
                },
            );
            assert_cu_within("funded bankruptcy accrual", cu, CRANK_CU_LIMIT);
            *peak = (*peak).max(cu);
            world.check(basis, pending);
        }
        for pair in 0..2 {
            let holder = &world.actors[2 * pair];
            let debtor = &world.actors[2 * pair + 1];
            let cu = world.env.trade_asset_with_cu(
                (pair + 1) as u16,
                &holder.owner,
                holder.portfolio,
                &debtor.owner,
                debtor.portfolio,
                -world.quantities[2 * pair],
                self.prices[pair] as u64,
                0,
            );
            assert_cu_within("funded bankruptcy reduction", cu, TRADE_CU_LIMIT);
            *peak = (*peak).max(cu);
            basis[2 * pair] = 0;
            basis[2 * pair + 1] = 0;
            pending[2 * pair] = true;
            world.check(basis, pending);
        }
        world
    }
}

#[test]
fn v16_program_funded_bankrupt_domains_preserve_input_derived_pending_debt() {
    let mut peak_setup = 0;
    let mut peak_terminal = 0;
    let mut worlds = 0;
    let mut outcomes = Vec::new();
    for reverse in [false, true] {
        let inputs = Inputs::new(reverse);
        for early in 0..2 {
            let late = 1 - early;
            let mut world = inputs.setup(reverse, &mut peak_setup);
            let mut model = DebtModel {
                initial: std::array::from_fn(|pair| {
                    close_progress(
                        &world
                            .env
                            .portfolio_state(world.actors[2 * pair + 1].portfolio),
                    )
                }),
                booked: [false; 2],
                released: [false; 2],
            };
            for close in model.initial {
                assert!(close.active && !close.finalized && !close.canceled);
            }
            inputs.check(&world, &model);
            let cu = world.env.crank(
                world.actors[2 * early + 1].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: SLOTS,
                    observations: crank_observations((early + 1) as u16),
                },
            );
            assert_cu_within("funded bankruptcy first B", cu, CRANK_CU_LIMIT);
            peak_terminal = peak_terminal.max(cu);
            model.booked[early] = true;
            inputs.check(&world, &model);
            let before = world.frame();
            let cu = world.env.resolve();
            assert_cu_within("funded bankruptcy resolution", cu, CRANK_CU_LIMIT);
            peak_terminal = peak_terminal.max(cu);
            assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
            for (key, account) in before {
                if key != world.env.market {
                    assert_eq!(world.env.svm.get_account(&key), account);
                }
            }
            world.env.svm.warp_to_slot(SLOTS + 5 + 31 * early as u64);
            inputs.check(&world, &model);
            let retained = world.env.svm.get_account(&world.actors[2 * late].portfolio);
            let mut suffix = close_instruction(&world, 4);
            suffix.accounts[0].is_signer = false;
            let bundle = [
                terminal_instruction(&world, 2 * early),
                terminal_instruction(&world, 2 * late + 1),
                suffix.clone(),
            ];
            peak_terminal = peak_terminal.max(reject(
                &mut world,
                &bundle,
                4,
                PercolatorError::ExpectedSigner,
            ));
            inputs.check(&world, &model);

            model.close(&mut world, 2 * early, &mut peak_terminal);
            model.released[early] = true;
            inputs.check(&world, &model);
            assert_eq!(
                world.env.svm.get_account(&world.actors[2 * late].portfolio),
                retained
            );
            model.close(&mut world, 2 * late + 1, &mut peak_terminal);
            model.booked[late] = true;
            inputs.check(&world, &model);
            assert_eq!(
                world.env.svm.get_account(&world.actors[2 * late].portfolio),
                retained
            );
            for actor in [2 * early + 1, 4] {
                model.close(&mut world, actor, &mut peak_terminal);
                inputs.check(&world, &model);
            }
            let waiting = terminal_instruction(&world, 2 * early);
            peak_terminal = peak_terminal.max(reject(
                &mut world,
                &[waiting],
                2,
                PercolatorError::EngineNonProgress,
            ));
            inputs.check(&world, &model);
            for actor in [2 * late, 2 * early] {
                let bundle = [terminal_instruction(&world, actor), suffix.clone()];
                peak_terminal = peak_terminal.max(reject(
                    &mut world,
                    &bundle,
                    3,
                    PercolatorError::ExpectedSigner,
                ));
                inputs.check(&world, &model);
                model.close(&mut world, actor, &mut peak_terminal);
                model.released[actor / 2] = true;
                inputs.check(&world, &model);
            }
            let paid: [u128; 5] = std::array::from_fn(|actor| {
                world.env.token_amount(world.actors[actor].token) as u128
            });
            assert_eq!(paid, [379_982, 0, 599_970, 0, 777]);
            assert_eq!(world.env.market_state().1.vault, 0);
            for actor in 0..5 {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                let retry = terminal_instruction(&world, actor);
                peak_terminal = peak_terminal.max(reject(
                    &mut world,
                    &[retry],
                    2,
                    PercolatorError::EngineNonProgress,
                ));
                inputs.check(&world, &model);
            }
            for (index, actor) in world.actors.iter().enumerate() {
                let before = world.frame();
                let rent = world
                    .env
                    .svm
                    .get_account(&actor.portfolio)
                    .unwrap()
                    .lamports;
                let market_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let cu = world
                    .env
                    .close_portfolio_with_cu(&actor.owner, actor.portfolio);
                assert_cu_within("funded bankruptcy deletion", cu, CUSTODY_CU_LIMIT);
                peak_terminal = peak_terminal.max(cu);
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    market_lamports + rent
                );
                assert!(world
                    .env
                    .svm
                    .get_account(&actor.portfolio)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                for (key, account) in before {
                    if ![world.env.market, actor.portfolio].contains(&key) {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                assert_eq!(world.env.token_amount(actor.token) as u128, paid[index]);
            }
            let group = world.env.market_state().1;
            assert_eq!(
                (
                    group.materialized_portfolio_count,
                    group.vault,
                    group.c_tot,
                    group.pnl_pos_tot
                ),
                (0, 0, 0, 0)
            );
            outcomes.push(paid);
            worlds += 1;
        }
    }
    assert_eq!(worlds, 4);
    assert!(outcomes.windows(2).all(|pair| pair[0] == pair[1]));
    println!("INV-039 funded bankrupt domains: {worlds} worlds, 16 prefix/wait rollbacks, 20 terminal retries/deletions; peak CU setup={peak_setup}, terminal={peak_terminal}");
}
