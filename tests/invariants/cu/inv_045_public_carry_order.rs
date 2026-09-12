//! Row425: account-settlement order and signed fill partition at a committed cap frontier.
//! INV-024/025/038/041/045/052/071/085/086/088, bounded public SBF evidence only.
//! Integral position lots isolate fractional price-cap carry from settlement rounding.
//! The primary selector constructs no pending cohorts, resolution, backing expiry,
//! or residual partition. The carry_transport_exit child adds resolved owner payouts.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

#[path = "inv_045_precrank_carry.rs"]
mod precrank_carry;

#[path = "inv_045_carry_transport_exit.rs"]
mod carry_transport_exit;

#[path = "inv_045_fractional_position_residue.rs"]
mod fractional_position_residue;

const ANCHORS: [u64; 2] = [100, 125];
const CAP_BPS: u64 = 24;
const PRINCIPAL: [u64; 4] = [100_003, 200_009, 300_017, 400_037];
const OPEN_LOTS: [[i128; 2]; 4] = [[13, 17], [-13, -17], [7, 11], [-7, -11]];

#[derive(Clone, Copy, Debug)]
struct History {
    direction: i128,
    batch: bool,
    split: bool,
    // Account cranks land before, between, or after the authorized fill parts.
    placement: usize,
    reverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Economics {
    price: [u64; 2],
    carry: [u64; 2],
    lots: [[i128; 2]; 4],
    entitlement: [i128; 4],
    vault: u128,
}

struct World {
    env: V16CuEnv,
    owners: [Keypair; 4],
    portfolios: [Pubkey; 4],
    tokens: [Pubkey; 4],
    expected: Economics,
    // Compact role-labelled public inputs, stopped at the first failing prefix.
    trace: Vec<String>,
    max_cu: u64,
    latent_checks: usize,
}

impl Drop for World {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "row425 first failing public prefix:\n{}",
                self.trace.join("\n")
            );
        }
    }
}

impl World {
    fn new(history: History) -> Self {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                initial_price: ANCHORS[0],
                max_price_move_bps_per_slot: CAP_BPS,
                max_abs_funding_e9_per_slot: 0,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(0);
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset as u16, 0, ANCHORS[asset]);
        }
        let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
        let mut portfolios = [Pubkey::default(); 4];
        let mut tokens = [Pubkey::default(); 4];
        for actor in 0..4 {
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            // Owner SOL is supplied through the System program, not account injection.
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(
                    &env.payer.pubkey(),
                    &owners[actor].pubkey(),
                    1_000_000,
                ),
                &[],
            )
            .unwrap();
            portfolios[actor] = portfolio.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            env.portfolios.push(portfolio.pubkey());
            tokens[actor] =
                create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    PRINCIPAL[actor],
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolio.pubkey(), PRINCIPAL[actor] as u128),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
        }
        for actor in [0, 2] {
            for asset in 0..2 {
                env.trade_asset_with_cu(
                    asset as u16,
                    &owners[actor],
                    portfolios[actor],
                    &owners[actor + 1],
                    portfolios[actor + 1],
                    OPEN_LOTS[actor][asset] * POS_SCALE as i128,
                    ANCHORS[asset],
                    0,
                );
            }
        }
        for asset in 0..2 {
            let sign = history.direction * if asset == 0 { 1 } else { -1 };
            env.push_auth_mark_for_asset_as_admin(
                asset as u16,
                0,
                (ANCHORS[asset] as i128 + sign * 20) as u64,
            );
        }
        Self {
            env,
            owners,
            portfolios,
            tokens,
            expected: Economics {
                price: ANCHORS,
                carry: [0; 2],
                lots: OPEN_LOTS,
                entitlement: PRINCIPAL.map(i128::from),
                vault: PRINCIPAL.into_iter().map(u128::from).sum(),
            },
            trace: vec![format!(
                "{history:?}; slot=0; cap_bps={CAP_BPS}; ConfigureAuthMark({ANCHORS:?}); SPL deposits={PRINCIPAL:?}; TradeNoCpi lots={OPEN_LOTS:?}, fees=0; PushAuthMark(anchor +/- 20)"
            )],
            max_cu: 0,
            latent_checks: 0,
        }
    }

    fn check(&mut self, history: History, elapsed: u64) -> Economics {
        let group = self.env.market_state().1;
        let market = self.env.svm.get_account(&self.env.market).unwrap();
        let profiles = [0, 1].map(|i| state::read_asset_oracle_profile(&market.data, i).unwrap());
        let mut actual = self.expected.clone();
        let mut capital = 0;
        let mut settled_pnl = 0;
        let mut positive_pnl = 0;
        let mut latent_pnl = 0;
        for asset in 0..2 {
            let state = group.assets[asset];
            assert_eq!(state.slot_last, elapsed, "{:?}", self.trace);
            assert_eq!(state.fund_px_last, ANCHORS[asset], "{:?}", self.trace);
            assert_eq!((state.f_long_num, state.f_short_num), (0, 0));
            assert_eq!((state.a_long, state.a_short), (ADL_ONE, ADL_ONE));
            assert_eq!((state.b_long_num, state.b_short_num), (0, 0));
            assert_eq!(
                (
                    state.pending_obligation_count_long,
                    state.pending_obligation_count_short
                ),
                (0, 0)
            );
            let sign = history.direction * if asset == 0 { 1 } else { -1 };
            assert_eq!(
                state.raw_oracle_target_price as i128,
                ANCHORS[asset] as i128 + sign * 20
            );
            // Recompute the canonical quotient/remainder from public inputs, never
            // the wrapper's price-step routine or its cached effective price.
            let numerator = ANCHORS[asset] * CAP_BPS * elapsed;
            let price = (ANCHORS[asset] as i128 + sign * (numerator / 10_000) as i128) as u64;
            for actor in 0..4 {
                self.expected.entitlement[actor] += self.expected.lots[actor][asset]
                    * (price as i128 - self.expected.price[asset] as i128);
            }
            self.expected.price[asset] = price;
            self.expected.carry[asset] = numerator % 10_000;
            actual.price[asset] = state.effective_price;
            actual.carry[asset] = profiles[asset].price_move_remainder_bps_num as u64;
            let long: i128 = self.expected.lots.iter().map(|q| q[asset].max(0)).sum();
            assert_eq!(
                state.oi_eff_long_q,
                long as u128 * POS_SCALE,
                "{:?}",
                self.trace
            );
            assert_eq!(state.oi_eff_short_q, state.oi_eff_long_q);
            let k = (price as i128 - ANCHORS[asset] as i128) * ADL_ONE as i128;
            assert_eq!((state.k_long, state.k_short), (k, -k), "{:?}", self.trace);
        }
        for actor in 0..4 {
            let account = self.env.portfolio_state(self.portfolios[actor]);
            let mut latent = 0;
            for asset in 0..2 {
                let leg = account
                    .legs
                    .iter()
                    .filter_map(|l| l.try_to_runtime().ok())
                    .find(|l| l.active && l.asset_index as usize == asset)
                    .unwrap();
                actual.lots[actor][asset] = leg.basis_pos_q / POS_SCALE as i128;
                assert_eq!(leg.basis_pos_q % POS_SCALE as i128, 0);
                assert_eq!(leg.a_basis, ADL_ONE);
                assert_eq!((leg.b_rem, leg.f_snap), (0, 0));
                let k = if leg.basis_pos_q > 0 {
                    group.assets[asset].k_long
                } else {
                    group.assets[asset].k_short
                };
                let numerator = leg.basis_pos_q.abs() * (k - leg.k_snap);
                let denominator = POS_SCALE as i128 * ADL_ONE as i128;
                assert_eq!(numerator % denominator, 0);
                latent += numerator / denominator;
            }
            self.latent_checks += usize::from(latent != 0);
            actual.entitlement[actor] = account.capital.get() as i128 + account.pnl.get() + latent;
            capital += account.capital.get();
            settled_pnl += account.pnl.get();
            positive_pnl += account.pnl.get().max(0) as u128;
            latent_pnl += latent;
            assert_eq!(self.env.token_amount(self.tokens[actor]), 0);
        }
        actual.vault = self.env.token_amount(self.env.vault) as u128;
        assert_eq!(group.c_tot, capital, "{:?}", self.trace);
        assert_eq!(group.pnl_pos_tot, positive_pnl, "{:?}", self.trace);
        assert_eq!(group.insurance, 0, "{:?}", self.trace);
        assert_eq!(group.vault, actual.vault);
        let mint = self.env.svm.get_account(&self.env.mint).unwrap();
        assert_eq!(
            Mint::unpack(&mint.data).unwrap().supply as u128,
            actual.vault
        );
        assert_eq!(
            actual.vault as i128,
            capital as i128 + settled_pnl + latent_pnl,
            "raw account stock plus unsettled signed obligations: {:?}",
            self.trace
        );
        assert_eq!(
            actual, self.expected,
            "first mismatching public prefix: {:?}",
            self.trace
        );
        actual
    }

    fn crank(&mut self, actor: usize, history: History, slot: u64) {
        let observations = if history.reverse { [1, 0] } else { [0, 1] };
        self.trace.push(format!(
            "slot={slot}: PermissionlessCrank(actor={actor}, hints={observations:?})"
        ));
        let frame_keys: Vec<_> = [self.env.market, self.env.mint, self.env.vault]
            .into_iter()
            .chain(self.portfolios)
            .chain(self.tokens)
            .collect();
        let before: Vec<_> = frame_keys
            .iter()
            .map(|key| self.env.svm.get_account(key))
            .collect();
        self.env.svm.expire_blockhash();
        match self.env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations_for_assets(&observations),
            },
            vec![
                AccountMeta::new(self.env.payer.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
            ],
            &[],
        ) {
            Ok(cu) => {
                self.max_cu = self.max_cu.max(cu);
                assert_ne!(
                    frame_keys
                        .iter()
                        .map(|key| self.env.svm.get_account(key))
                        .collect::<Vec<_>>(),
                    before,
                    "accepted crank must progress: {:?}",
                    self.trace
                );
            }
            Err(error) => {
                assert!(
                    is_engine_non_progress_error(&error),
                    "{error}: {:?}",
                    self.trace
                );
                assert_eq!(
                    frame_keys
                        .iter()
                        .map(|key| self.env.svm.get_account(key))
                        .collect::<Vec<_>>(),
                    before,
                    "non-progress rollback: {:?}",
                    self.trace
                );
            }
        }
        self.check(history, slot);
    }

    fn reduce(&mut self, history: History, slot: u64, lots: i128) {
        let order = if history.reverse { [1, 0] } else { [0, 1] };
        let legs: Vec<_> = order
            .into_iter()
            .map(|asset| BatchTradeLeg {
                asset_index: asset as u16,
                market_id: self.env.asset_market_id(asset),
                size_q: -lots * POS_SCALE as i128,
                exec_price: self.expected.price[asset as usize],
                fee_bps: 0,
            })
            .collect();
        let chunks: Vec<_> = if history.batch {
            vec![legs]
        } else {
            legs.into_iter().map(|leg| vec![leg]).collect()
        };
        for legs in chunks {
            let passive_before =
                [2, 3].map(|actor| self.env.svm.get_account(&self.portfolios[actor]));
            self.trace.push(format!(
                "Clock.slot={}; frontier={slot}: {}(actors=0/1, legs={legs:?})",
                self.env.svm.get_sysvar::<Clock>().slot,
                if history.batch {
                    "BatchTradeNoCpi"
                } else {
                    "TradeNoCpi"
                }
            ));
            self.env.svm.expire_blockhash();
            let result = if history.batch {
                self.env.send(
                    self.env.batch_trade_no_cpi_ix(
                        self.portfolios[0],
                        self.portfolios[1],
                        legs.clone(),
                    ),
                    vec![
                        AccountMeta::new(self.owners[0].pubkey(), true),
                        AccountMeta::new(self.owners[1].pubkey(), true),
                        AccountMeta::new(self.env.market, false),
                        AccountMeta::new(self.portfolios[0], false),
                        AccountMeta::new(self.portfolios[1], false),
                    ],
                    &[&self.owners[0], &self.owners[1]],
                )
            } else {
                let leg = &legs[0];
                self.env.try_trade_asset_with_cu(
                    leg.asset_index,
                    &self.owners[0],
                    self.portfolios[0],
                    &self.owners[1],
                    self.portfolios[1],
                    leg.size_q,
                    leg.exec_price,
                    0,
                )
            };
            let cu = result.unwrap_or_else(|error| panic!("{error}: {:?}", self.trace));
            self.max_cu = self.max_cu.max(cu);
            for leg in legs {
                self.expected.lots[0][leg.asset_index as usize] -= lots;
                self.expected.lots[1][leg.asset_index as usize] += lots;
            }
            self.check(history, slot);
            assert_eq!(
                [2, 3].map(|actor| self.env.svm.get_account(&self.portfolios[actor])),
                passive_before,
                "signed active-owner reductions cannot write passive portfolios: {:?}",
                self.trace
            );
        }
    }
}

#[test]
fn v16_program_row425_public_carry_entitlement_is_partition_and_order_equivalent() {
    let mut worlds = 0;
    let mut max_cu = 0;
    for direction in [-1, 1] {
        let mut baseline = None;
        for batch in [false, true] {
            for split in [false, true] {
                for placement in 0..3 {
                    for reverse in [false, true] {
                        let history = History {
                            direction,
                            batch,
                            split,
                            placement,
                            reverse,
                        };
                        let mut world = World::new(history);
                        world.check(history, 0);
                        for slot in 1..=5 {
                            world
                                .trace
                                .push(format!("Clock.slot={slot}; expire_blockhash"));
                            world.env.svm.warp_to_slot(slot);
                            // This public observer commits the market path. Active owners
                            // still have independent, potentially stale settlement cursors.
                            world.crank(2, history, slot);
                            for phase in 0..3 {
                                if phase == history.placement {
                                    for actor in if reverse { [1, 0] } else { [0, 1] } {
                                        world.crank(actor, history, slot);
                                    }
                                }
                                if phase < 2 && (history.split || phase == 0) {
                                    world.reduce(history, slot, if history.split { 1 } else { 2 });
                                }
                            }
                            world.crank(3, history, slot);
                        }
                        let endpoint = world.check(history, 5);
                        assert_eq!(endpoint.carry, [2_000, 5_000]);
                        assert_eq!(
                            endpoint.entitlement,
                            [
                                PRINCIPAL[0] as i128 - 6 * direction,
                                PRINCIPAL[1] as i128 + 6 * direction,
                                PRINCIPAL[2] as i128 - 4 * direction,
                                PRINCIPAL[3] as i128 + 4 * direction,
                            ]
                        );
                        assert!(world.latent_checks > 0, "local cursors must actually lag");
                        if let Some(expected) = &baseline {
                            assert_eq!(
                                &endpoint, expected,
                                "history={history:?}; {:?}",
                                world.trace
                            );
                        } else {
                            baseline = Some(endpoint.clone());
                        }
                        // Oracle sensitivity only: these host copies never enter LiteSVM.
                        let mut swapped = endpoint.clone();
                        swapped.entitlement[0] += 1;
                        swapped.entitlement[1] -= 1;
                        assert_eq!(
                            swapped.entitlement.iter().sum::<i128>(),
                            endpoint.entitlement.iter().sum()
                        );
                        assert_ne!(
                            swapped, world.expected,
                            "balanced wrong-owner credit must fail"
                        );
                        let mut dropped = endpoint;
                        dropped.carry[0] = 0;
                        assert_ne!(
                            dropped, world.expected,
                            "discarded fractional carry must fail"
                        );
                        max_cu = max_cu.max(world.max_cu);
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 48);
    assert_cu_within("row425 two-asset public order/partition", max_cu, 1_400_000);
    eprintln!(
        "row425: {worlds} histories, max transition CU={max_cu}, no entitlement/carry mismatch"
    );
}
