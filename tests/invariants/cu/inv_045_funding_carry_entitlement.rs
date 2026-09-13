//! INV-045 / row425: fractional price carry across a funding-premium reversal.
//! Explicit market cranks leave owner K/F unsettled; signed reductions and account
//! cranks must preserve each owner's input-derived entitlement through SPL exit.
//! Integral lots, unit ADL and zero fees isolate funding from settlement residue.

use super::*;

#[path = "inv_045_retained_funding_retry.rs"]
mod retained_funding_retry;

const RATE: i128 = 10_000;
const REVERSAL_SLOT: u64 = 3;
const END_SLOT: u64 = 8;

struct Ledger {
    lots: [[i128; 2]; 4],
    value: [i128; 4],
    price: [u64; 2],
    funding: [i128; 2],
    carry: [u64; 2],
    slot: u64,
    latent_checks: usize,
}

impl Ledger {
    fn new() -> Self {
        Self {
            lots: OPEN_LOTS,
            value: PRINCIPAL.map(i128::from),
            price: ANCHORS,
            funding: [0; 2],
            carry: [0; 2],
            slot: 0,
            latent_checks: 0,
        }
    }

    fn advance(&mut self, direction: i128, slot: u64) {
        assert_eq!(slot, self.slot + 1);
        for asset in 0..2 {
            let sign = direction
                * if asset == 0 { 1 } else { -1 }
                * if slot <= REVERSAL_SLOT { 1 } else { -1 };
            let target = i128::from(ANCHORS[asset]) + sign * 20;
            let numerator = self.carry[asset] + ANCHORS[asset] * CAP_BPS;
            let price = self.price[asset] as i128 + sign * (numerator / 10_000) as i128;
            self.carry[asset] = numerator % 10_000;
            // Public premium, rate bound and one-slot signed floor. No wrapper
            // accrual helper or decoded F index supplies the expected result.
            let rate = ((target - price) * 1_000_000_000 / price).clamp(-RATE, RATE);
            let funding = -(rate * price).div_euclid(1_000_000_000);
            for actor in 0..4 {
                self.value[actor] +=
                    self.lots[actor][asset] * (price - self.price[asset] as i128 + funding);
            }
            self.funding[asset] += funding;
            self.price[asset] = price as u64;
        }
        self.slot = slot;
    }

    fn check(&mut self, world: &World) {
        let env = &world.env;
        let group = env.market_state().1;
        let market = env.svm.get_account(&env.market).unwrap();
        for asset in 0..2 {
            let state = group.assets[asset];
            let profile = state::read_asset_oracle_profile(&market.data, asset).unwrap();
            assert_eq!(state.slot_last, self.slot, "{:?}", world.trace);
            assert_eq!(state.effective_price, self.price[asset]);
            assert_eq!(state.fund_px_last, ANCHORS[asset]);
            assert_eq!(
                u64::from(profile.price_move_remainder_bps_num),
                self.carry[asset]
            );
            let k = (self.price[asset] as i128 - ANCHORS[asset] as i128) * ADL_ONE as i128;
            let f = self.funding[asset] * ADL_ONE as i128;
            assert_eq!((state.k_long, state.k_short), (k, -k));
            assert_eq!((state.f_long_num, state.f_short_num), (f, -f));
            assert_eq!((state.a_long, state.a_short), (ADL_ONE, ADL_ONE));
            assert_eq!((state.b_long_num, state.b_short_num), (0, 0));
            let oi = self
                .lots
                .iter()
                .map(|q| q[asset].max(0) as u128)
                .sum::<u128>()
                * POS_SCALE;
            assert_eq!((state.oi_eff_long_q, state.oi_eff_short_q), (oi, oi));
        }
        let mut capital = 0;
        let mut positive_pnl = 0;
        for actor in 0..4 {
            let account = env.portfolio_state(world.portfolios[actor]);
            let mut latent = 0;
            for asset in 0..2 {
                let leg = account
                    .legs
                    .iter()
                    .map(|leg| leg.try_to_runtime().unwrap())
                    .find(|leg| leg.active && leg.asset_index as usize == asset);
                let Some(leg) = leg else {
                    assert_eq!(self.lots[actor][asset], 0);
                    continue;
                };
                assert_eq!(leg.basis_pos_q, self.lots[actor][asset] * POS_SCALE as i128);
                assert_eq!(leg.a_basis, ADL_ONE);
                assert_eq!(leg.b_rem, 0);
                let sign = self.lots[actor][asset].signum();
                let k = sign * (self.price[asset] as i128 - ANCHORS[asset] as i128);
                let f = sign * self.funding[asset];
                assert_eq!(leg.k_snap % ADL_ONE as i128, 0);
                assert_eq!(leg.f_snap % ADL_ONE as i128, 0);
                latent += self.lots[actor][asset].abs()
                    * (k - leg.k_snap / ADL_ONE as i128 + f - leg.f_snap / ADL_ONE as i128);
            }
            self.latent_checks += usize::from(latent != 0);
            assert_eq!(
                account.capital.get() as i128
                    + account.pnl.get()
                    + latent
                    + i128::from(env.token_amount(world.tokens[actor])),
                self.value[actor],
                "owner={actor}: {:?}",
                world.trace
            );
            capital += account.capital.get();
            positive_pnl += account.pnl.get().max(0) as u128;
        }
        assert_eq!((group.c_tot, group.pnl_pos_tot), (capital, positive_pnl));
        assert_eq!(group.insurance, 0);
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert_eq!(
            self.value.iter().sum::<i128>(),
            PRINCIPAL.map(i128::from).iter().sum()
        );
        let paid = world
            .tokens
            .map(|key| u128::from(env.token_amount(key)))
            .iter()
            .sum::<u128>();
        assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(group.vault + paid, PRINCIPAL.map(u128::from).iter().sum());
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), group.vault + paid);
        assert_eq!(
            mint.mint_authority,
            solana_program::program_option::COption::None
        );
    }
}

fn settle(world: &mut World, ledger: &mut Ledger, actor: usize) {
    world
        .trace
        .push(format!("slot={}: crank owner={actor}", ledger.slot));
    let absent = (0..4).filter(|i| *i != actor).collect::<Vec<_>>();
    let before = absent
        .iter()
        .map(|i| world.env.svm.get_account(&world.portfolios[*i]))
        .collect::<Vec<_>>();
    world.env.svm.expire_blockhash();
    if let Some(cu) = world.env.crank_if_actionable(
        world.portfolios[actor],
        ProgInstruction::PermissionlessCrank {
            now_slot: ledger.slot,
            observations: crank_observations_for_assets(&[0, 1]),
        },
    ) {
        world.max_cu = world.max_cu.max(cu);
    }
    assert_eq!(
        absent
            .iter()
            .map(|i| world.env.svm.get_account(&world.portfolios[*i]))
            .collect::<Vec<_>>(),
        before
    );
    ledger.check(world);
}

fn reduce(
    world: &mut World,
    ledger: &mut Ledger,
    history: History,
    lots: i128,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
) {
    let profile_before = carry_transport_exit::profiles(world);
    if let Some((program, context, delegate)) = matcher {
        world.env.set_matcher_config_with_trade_fee_cap(
            program,
            &world.owners[1],
            world.portfolios[1],
            context,
            delegate,
            1,
            0,
        );
        ledger.check(world);
        assert_eq!(carry_transport_exit::profiles(world), profile_before);
    }
    let order = if history.reverse { [1, 0] } else { [0, 1] };
    let chunks = if history.batch {
        vec![order.to_vec()]
    } else {
        order.map(|asset| vec![asset]).to_vec()
    };
    for assets in chunks {
        let absent = [2, 3].map(|i| world.env.svm.get_account(&world.portfolios[i]));
        world.trace.push(format!(
            "slot={}: reduce={lots}, assets={assets:?}, cpi={}",
            ledger.slot,
            matcher.is_some()
        ));
        world.env.svm.expire_blockhash();
        let env = &mut world.env;
        let result = if let Some((program, context, delegate)) = matcher {
            if history.batch {
                let legs = assets
                    .iter()
                    .map(|asset| BatchTradeCpiLeg {
                        asset_index: *asset,
                        market_id: env.asset_market_id(*asset),
                        size_q: -lots * POS_SCALE as i128,
                        fee_bps: 0,
                        limit_price: ledger.price[*asset as usize],
                    })
                    .collect();
                env.send(
                    env.batch_trade_cpi_ix(world.portfolios[0], world.portfolios[1], legs),
                    vec![
                        AccountMeta::new(world.owners[0].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(world.portfolios[0], false),
                        AccountMeta::new(world.portfolios[1], false),
                        AccountMeta::new_readonly(program, false),
                        AccountMeta::new(context, false),
                        AccountMeta::new_readonly(delegate, false),
                    ],
                    &[&world.owners[0]],
                )
            } else {
                env.try_trade_cpi_with_cu_on_asset(
                    &world.owners[0],
                    world.portfolios[0],
                    &world.owners[1],
                    world.portfolios[1],
                    program,
                    context,
                    delegate,
                    assets[0],
                    -lots * POS_SCALE as i128,
                    0,
                )
            }
        } else if history.batch {
            let legs = assets
                .iter()
                .map(|asset| BatchTradeLeg {
                    asset_index: *asset,
                    market_id: env.asset_market_id(*asset),
                    size_q: -lots * POS_SCALE as i128,
                    exec_price: ledger.price[*asset as usize],
                    fee_bps: 0,
                })
                .collect();
            env.send(
                env.batch_trade_no_cpi_ix(world.portfolios[0], world.portfolios[1], legs),
                vec![
                    AccountMeta::new(world.owners[0].pubkey(), true),
                    AccountMeta::new(world.owners[1].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(world.portfolios[0], false),
                    AccountMeta::new(world.portfolios[1], false),
                ],
                &[&world.owners[0], &world.owners[1]],
            )
        } else {
            env.try_trade_asset_with_cu(
                assets[0],
                &world.owners[0],
                world.portfolios[0],
                &world.owners[1],
                world.portfolios[1],
                -lots * POS_SCALE as i128,
                ledger.price[assets[0] as usize],
                0,
            )
        };
        world.max_cu = world
            .max_cu
            .max(result.unwrap_or_else(|error| panic!("{error}: {:?}", world.trace)));
        for asset in assets {
            ledger.lots[0][asset as usize] -= lots;
            ledger.lots[1][asset as usize] += lots;
        }
        ledger.check(world);
        assert_eq!(carry_transport_exit::profiles(world), profile_before);
        assert_eq!(
            [2, 3].map(|i| world.env.svm.get_account(&world.portfolios[i])),
            absent
        );
    }
}

fn pay(world: &mut World, ledger: &mut Ledger, reverse: bool) {
    for actor in if reverse { [2, 0] } else { [0, 2] } {
        for asset in 0..2 {
            world.trace.push(format!(
                "slot={}: close pair={actor}/{}, asset={asset}",
                ledger.slot,
                actor + 1
            ));
            world.env.svm.expire_blockhash();
            world.max_cu = world.max_cu.max(world.env.trade_asset_with_cu(
                asset as u16,
                &world.owners[actor],
                world.portfolios[actor],
                &world.owners[actor + 1],
                world.portfolios[actor + 1],
                -ledger.lots[actor][asset] * POS_SCALE as i128,
                ledger.price[asset],
                0,
            ));
            ledger.lots[actor][asset] = 0;
            ledger.lots[actor + 1][asset] = 0;
            ledger.check(world);
        }
    }
    for actor in if reverse { [3, 2, 1, 0] } else { [0, 1, 2, 3] } {
        // Other owners' closes invalidate the flat account's conversion certificate.
        settle(world, ledger, actor);
        world.trace.push(format!(
            "slot={}: convert and withdraw owner={actor}, entitlement={}",
            ledger.slot, ledger.value[actor]
        ));
        let account = world.env.portfolio_state(world.portfolios[actor]);
        assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
        assert!(account.pnl.get() >= 0);
        if account.pnl.get() > 0 {
            world.max_cu = world.max_cu.max(world.env.convert_released_pnl_with_cu(
                &world.owners[actor],
                world.portfolios[actor],
                account.pnl.get() as u128,
            ));
        }
        ledger.check(world);
        assert_eq!(
            world
                .env
                .portfolio_state(world.portfolios[actor])
                .capital
                .get(),
            ledger.value[actor] as u128
        );
        let cu = world
            .env
            .send(
                world
                    .env
                    .withdraw_ix(world.portfolios[actor], ledger.value[actor] as u128),
                vec![
                    AccountMeta::new(world.owners[actor].pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.portfolios[actor], false),
                    AccountMeta::new(world.tokens[actor], false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&world.owners[actor]],
            )
            .unwrap();
        world.max_cu = world.max_cu.max(cu);
        ledger.check(world);
    }
    assert_eq!(
        world
            .tokens
            .map(|key| i128::from(world.env.token_amount(key))),
        ledger.value
    );
    let group = world.env.market_state().1;
    assert_eq!((group.vault, group.c_tot, group.pnl_pos_tot), (0, 0, 0));
}

#[test]
fn v16_program_funding_reversal_preserves_carry_and_unsettled_owner_entitlement() {
    let mut peak_cu = 0;
    let mut worlds = 0;
    for direction in [-1, 1] {
        let mut reference = None;
        for batch in [false, true] {
            for cpi_first in [false, true] {
                for settle_first in [false, true] {
                    let history = History {
                        direction,
                        batch,
                        split: false,
                        placement: usize::from(settle_first),
                        reverse: settle_first,
                    };
                    let mut world = World::with_funding(history, RATE as u64);
                    world.trace.push(format!(
                        "funding={RATE}, cpi_first={cpi_first}, reversal={REVERSAL_SLOT}"
                    ));
                    send_raw_tx(
                        &mut world.env.svm,
                        &world.env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &world.env.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &world.env.admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&world.env.admin],
                    )
                    .unwrap();
                    let matcher = auth_matcher_for_lp_via_system_create(
                        &mut world.env,
                        &world.owners[1],
                        world.portfolios[1],
                    );
                    let mut ledger = Ledger::new();
                    ledger.check(&world);
                    let passive = world.env.svm.get_account(&world.portfolios[3]);
                    for slot in 1..=END_SLOT {
                        world.env.svm.warp_to_slot(slot);
                        ledger.advance(direction, slot);
                        settle(&mut world, &mut ledger, 2);
                        if slot == 2 || slot == 5 {
                            assert!(ledger.carry.iter().all(|carry| *carry > 0));
                            if settle_first {
                                settle(&mut world, &mut ledger, 0);
                                settle(&mut world, &mut ledger, 1);
                            }
                            reduce(
                                &mut world,
                                &mut ledger,
                                history,
                                if slot == 2 { 2 } else { 1 },
                                (cpi_first == (slot == 2)).then_some(matcher),
                            );
                            if !settle_first {
                                settle(&mut world, &mut ledger, 1);
                                settle(&mut world, &mut ledger, 0);
                            }
                        }
                        if slot == REVERSAL_SLOT {
                            assert_eq!(ledger.carry, [7_200, 9_000]);
                            for asset in 0..2 {
                                let sign = direction * if asset == 0 { -1 } else { 1 };
                                let target = (ANCHORS[asset] as i128 + sign * 20) as u64;
                                world.env.push_auth_mark_for_asset_as_admin(
                                    asset as u16,
                                    slot,
                                    target,
                                );
                                let profile = carry_transport_exit::profiles(&world)[asset];
                                assert_eq!(profile.funding_mark_e6, target);
                                assert_eq!(
                                    (
                                        profile.funding_mark_pending_e6,
                                        profile.funding_mark_pending_slot
                                    ),
                                    (0, 0)
                                );
                            }
                            ledger.carry = [0; 2];
                            ledger.check(&world);
                        }
                        assert_eq!(world.env.svm.get_account(&world.portfolios[3]), passive);
                    }
                    assert_eq!(ledger.carry, [2_000, 5_000]);
                    assert_eq!(
                        ledger.price,
                        [(100 - direction) as u64, (125 + direction) as u64]
                    );
                    assert_eq!(
                        ledger.funding,
                        if direction == -1 { [3, 5] } else { [5, 3] }
                    );
                    assert!(ledger.latent_checks >= 16);
                    assert_eq!(ledger.value, [100_108, 199_904, 300_089, 399_965]);
                    pay(&mut world, &mut ledger, settle_first);
                    if let Some(expected) = reference {
                        assert_eq!(ledger.value, expected);
                    } else {
                        reference = Some(ledger.value);
                    }
                    peak_cu = peak_cu.max(world.max_cu);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    assert_cu_within("row425 funding/carry entitlement", peak_cu, 1_400_000);
    eprintln!(
        "row425 funding reversal: {worlds} worlds, 64 exact owner payouts, peak {peak_cu} CU"
    );
}
