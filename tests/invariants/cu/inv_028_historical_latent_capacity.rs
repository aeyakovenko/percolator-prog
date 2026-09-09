//! INV-028, reopening 423: retained historical claims and still-latent position
//! domains share one bounded settlement budget. This is a positive boundary
//! product, not an over-capacity admission or arbitrary-history closure claim.
//! System/SPL/ATA/matcher/wrapper instructions construct all economic state.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use std::collections::BTreeSet;

#[path = "inv_028_concurrent_latent_capacity.rs"]
mod concurrent_latent_capacity;

const ASSETS: usize = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize;
const DOMAINS: usize = percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS;
const CAPITAL: u128 = 1_000_000;
const PRICE: u64 = 100;
const CU_LIMIT: u64 = 1_375_000;

struct History {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    matcher: (Pubkey, Pubkey, Pubkey),
    positions: [i128; ASSETS],
    prices: [u64; ASSETS],
    previous_prices: [u64; ASSETS],
    claims: [u128; DOMAINS],
    previous_claims: [u128; DOMAINS],
    slot: u64,
    calls: usize,
    max_trade: u64,
    max_crank: u64,
    max_convert: u64,
    max_withdraw: u64,
    max_close: u64,
}

impl History {
    fn new() -> Self {
        assert_eq!(DOMAINS, ASSETS * 2);
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: ASSETS as u16,
                initial_price: PRICE,
                max_price_move_bps_per_slot: 150,
                max_accrual_dt_slots: 64,
                min_funding_lifetime_slots: 64,
                ..V16CuMarketParams::default()
            },
        );
        for asset in 0..ASSETS {
            env.configure_auth_mark_for_asset_as_admin(asset as u16, 0, PRICE);
        }
        let owners = [Keypair::new(), Keypair::new()];
        let mut portfolios = [Pubkey::default(); 2];
        let mut tokens = [Pubkey::default(); 2];
        for i in 0..2 {
            env.svm.airdrop(&owners[i].pubkey(), 1_000_000_000).unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            portfolios[i] = portfolio.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                ],
                &[&owners[i]],
            )
            .expect("initialize public portfolio");
            env.portfolios.push(portfolios[i]);
            tokens[i] = create_ata_for_test(&mut env.svm, &env.payer, owners[i].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[i],
                    &env.admin.pubkey(),
                    &[],
                    CAPITAL as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .expect("mint public collateral");
            env.send(
                env.deposit_ix(portfolios[i], CAPITAL),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .expect("deposit public collateral");
        }
        let matcher = auth_matcher_for_lp_via_system_create(&mut env, &owners[1], portfolios[1]);
        let history = Self {
            env,
            owners,
            portfolios,
            tokens,
            matcher,
            positions: [0; ASSETS],
            prices: [PRICE; ASSETS],
            previous_prices: [PRICE; ASSETS],
            claims: [0; DOMAINS],
            previous_claims: [0; DOMAINS],
            slot: 0,
            calls: 0,
            max_trade: 0,
            max_crank: 0,
            max_convert: 0,
            max_withdraw: 0,
            max_close: 0,
        };
        history.assert_accounting();
        history
    }

    fn source_claims(&self, actor: usize) -> [u128; DOMAINS] {
        let mut claims = [0; DOMAINS];
        let mut seen = BTreeSet::new();
        let account = self.env.portfolio_state(self.portfolios[actor]);
        for source in account.source_domains.iter().filter(|s| s.is_occupied()) {
            let domain = source.domain.get() as usize;
            assert!(domain < DOMAINS);
            assert!(seen.insert(domain), "source attribution is unique");
            claims[domain] = source.source_claim_bound_num.get();
            assert_eq!(source.source_claim_liened_num.get(), 0);
            assert_eq!(source.source_lien_counterparty_backing_num.get(), 0);
            assert_eq!(source.source_lien_insurance_backing_num.get(), 0);
        }
        claims
    }

    fn assert_accounting(&self) {
        let group = self.env.market_state().1;
        let accounts = self.portfolios.map(|p| self.env.portfolio_state(p));
        let actual_claims = self.source_claims(0);
        assert_eq!(self.source_claims(1), [0; DOMAINS]);
        let mut backing = 0;
        for domain in 0..DOMAINS {
            let old = self.previous_claims[domain] * BOUND_SCALE;
            let next = self.claims[domain] * BOUND_SCALE;
            let claim = actual_claims[domain];
            assert!(
                claim == old || claim == next,
                "claim must be an input-derived prefix"
            );
            let source = &group.source_credit[domain];
            let bucket = &group.source_backing_buckets[domain];
            assert_eq!(source.exact_positive_claim_num, claim);
            assert_eq!(source.positive_claim_bound_num, claim);
            assert_eq!(source.valid_liened_backing_num, 0);
            assert_eq!(source.impaired_liened_backing_num, 0);
            assert_eq!(source.insurance_credit_reserved_num, 0);
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(bucket.impaired_liened_backing_num, 0);
            assert_eq!(
                source.fresh_reserved_backing_num,
                bucket.fresh_unliened_backing_num
            );
            let available = bucket.fresh_unliened_backing_num;
            assert!(
                available == old || available == next,
                "debit must be an input-derived prefix"
            );
            let usable = claim * source.credit_rate_num / percolator::CREDIT_RATE_SCALE;
            assert!(
                usable <= available,
                "usable credit stays within this domain's backing"
            );
            backing += available / BOUND_SCALE;
        }
        assert_eq!(accounts[0].capital.get(), CAPITAL);
        assert_eq!(
            accounts[0].pnl.get(),
            (actual_claims.iter().sum::<u128>() / BOUND_SCALE) as i128
        );
        assert_eq!(accounts[1].capital.get(), CAPITAL - backing);
        assert_eq!(accounts[1].pnl.get(), 0);
        assert_eq!(group.c_tot, 2 * CAPITAL - backing);
        assert_eq!(group.insurance, 0);
        assert_eq!(group.vault, 2 * CAPITAL);
        assert_eq!(self.env.token_amount(self.env.vault) as u128, 2 * CAPITAL);
        assert!(self.tokens.iter().all(|t| self.env.token_amount(*t) == 0));
        assert_eq!(
            Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data)
                .unwrap()
                .supply as u128,
            2 * CAPITAL,
        );

        for (actor, account) in accounts.iter().enumerate() {
            let mut resources: BTreeSet<_> = account
                .source_domains
                .iter()
                .filter(|s| s.is_occupied())
                .map(|s| s.domain.get())
                .collect();
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(account)) as usize,
                self.positions.iter().filter(|q| **q != 0).count()
            );
            for (asset, &quantity) in self.positions.iter().enumerate() {
                assert_eq!(has_active_leg_for_asset(account, asset), quantity != 0);
                if quantity != 0 {
                    assert_eq!(
                        active_leg_for_asset(account, asset).basis_pos_q,
                        if actor == 0 { quantity } else { -quantity }
                    );
                    resources.extend([2 * asset as u32, 2 * asset as u32 + 1]);
                }
                assert_eq!(group.assets[asset].oi_eff_long_q, quantity.unsigned_abs());
                assert_eq!(group.assets[asset].oi_eff_short_q, quantity.unsigned_abs());
                assert!(
                    group.assets[asset].effective_price == self.previous_prices[asset]
                        || group.assets[asset].effective_price == self.prices[asset],
                    "effective price follows the submitted one-atom history"
                );
            }
            assert!(
                resources.len() <= DOMAINS,
                "historical and future active-leg domains share the supported budget"
            );
        }
    }

    fn trade(&mut self, route: AccountResidualCounterTradePath, legs: &[(u16, i128, u64)]) {
        let (program, context, delegate) = self.matcher;
        if matches!(
            route,
            AccountResidualCounterTradePath::TradeCpi
                | AccountResidualCounterTradePath::BatchTradeCpi
        ) && self
            .env
            .portfolio_matcher_config(self.portfolios[1])
            .enabled()
            == 0
        {
            self.env.set_matcher_config(
                program,
                &self.owners[1],
                self.portfolios[1],
                context,
                delegate,
                1,
            );
            self.calls += 1;
            self.assert_accounting();
        }
        self.env.svm.expire_blockhash();
        let cu = match route {
            AccountResidualCounterTradePath::TradeNoCpi
            | AccountResidualCounterTradePath::TradeCpi => {
                let mut max_cu = 0;
                for &(asset, quantity, price) in legs {
                    self.env.svm.expire_blockhash();
                    let cu = if matches!(route, AccountResidualCounterTradePath::TradeNoCpi) {
                        self.env.trade_asset_with_cu(
                            asset,
                            &self.owners[0],
                            self.portfolios[0],
                            &self.owners[1],
                            self.portfolios[1],
                            quantity,
                            price,
                            0,
                        )
                    } else {
                        self.env.trade_cpi_with_cu_on_asset(
                            &self.owners[0],
                            self.portfolios[0],
                            &self.owners[1],
                            self.portfolios[1],
                            program,
                            context,
                            delegate,
                            asset,
                            quantity,
                            0,
                        )
                    };
                    self.positions[asset as usize] += quantity;
                    self.calls += 1;
                    self.assert_accounting();
                    max_cu = max_cu.max(cu);
                }
                max_cu
            }
            AccountResidualCounterTradePath::BatchTradeNoCpi
            | AccountResidualCounterTradePath::BatchTradeCpi => {
                let (instruction, metas, signers) =
                    if matches!(route, AccountResidualCounterTradePath::BatchTradeNoCpi) {
                        (
                            self.env.batch_trade_no_cpi_ix(
                                self.portfolios[0],
                                self.portfolios[1],
                                legs.iter()
                                    .map(|&(asset_index, size_q, exec_price)| BatchTradeLeg {
                                        asset_index,
                                        market_id: self.env.asset_market_id(asset_index),
                                        size_q,
                                        exec_price,
                                        fee_bps: 0,
                                    })
                                    .collect(),
                            ),
                            vec![
                                AccountMeta::new(self.owners[0].pubkey(), true),
                                AccountMeta::new(self.owners[1].pubkey(), true),
                                AccountMeta::new(self.env.market, false),
                                AccountMeta::new(self.portfolios[0], false),
                                AccountMeta::new(self.portfolios[1], false),
                            ],
                            vec![&self.owners[0], &self.owners[1]],
                        )
                    } else {
                        (
                            self.env.batch_trade_cpi_ix(
                                self.portfolios[0],
                                self.portfolios[1],
                                legs.iter()
                                    .map(|&(asset_index, size_q, _)| BatchTradeCpiLeg {
                                        asset_index,
                                        market_id: self.env.asset_market_id(asset_index),
                                        size_q,
                                        fee_bps: 0,
                                        limit_price: 0,
                                    })
                                    .collect(),
                            ),
                            vec![
                                AccountMeta::new(self.owners[0].pubkey(), true),
                                AccountMeta::new(self.env.market, false),
                                AccountMeta::new(self.portfolios[0], false),
                                AccountMeta::new(self.portfolios[1], false),
                                AccountMeta::new_readonly(program, false),
                                AccountMeta::new(context, false),
                                AccountMeta::new_readonly(delegate, false),
                            ],
                            vec![&self.owners[0]],
                        )
                    };
                let cu = self
                    .env
                    .send(instruction, metas, &signers)
                    .expect("bounded batch trade");
                for &(asset, quantity, _) in legs {
                    self.positions[asset as usize] += quantity;
                }
                self.calls += 1;
                self.assert_accounting();
                cu
            }
        };
        assert_cu_within("historical/latent capacity trade", cu, CU_LIMIT);
        self.max_trade = self.max_trade.max(cu);
    }

    fn mark_and_settle(&mut self, moves: &[(u16, u64)], order: [usize; 2]) {
        self.previous_claims = self.claims;
        self.previous_prices = self.prices;
        self.slot += 1;
        self.env.svm.warp_to_slot(self.slot);
        for &(asset, price) in moves {
            let before = self.prices[asset as usize];
            let quantity = self.positions[asset as usize] / POS_SCALE as i128;
            let gain = quantity * (price as i128 - before as i128);
            assert!(gain > 0);
            let domain = 2 * asset as usize + usize::from(quantity > 0);
            self.claims[domain] += gain as u128;
            self.prices[asset as usize] = price;
            self.env
                .push_auth_mark_for_asset_as_admin(asset, self.slot, price);
            self.calls += 1;
            self.assert_accounting();
        }
        let observations =
            crank_observations_for_assets(&moves.iter().map(|m| m.0).collect::<Vec<_>>());
        let gain = self.claims.iter().sum::<u128>();
        for actor in order {
            // Only input-derived economic debt and bounded authenticated accrual count as progress.
            let rank = |history: &Self| {
                let group = history.env.market_state().1;
                let account = history.env.portfolio_state(history.portfolios[actor]);
                let pending_slots = moves
                    .iter()
                    .map(|m| history.slot - group.assets[m.0 as usize].slot_last)
                    .sum::<u64>();
                u128::from(pending_slots)
                    + if actor == 0 {
                        account.pnl.get().abs_diff(gain as i128)
                    } else {
                        account.capital.get().abs_diff(CAPITAL - gain)
                    }
            };
            for _ in 0..4 {
                let before_rank = rank(self);
                if before_rank == 0 {
                    break;
                }
                self.env.svm.expire_blockhash();
                let cu = self
                    .env
                    .send(
                        ProgInstruction::PermissionlessCrank {
                            now_slot: self.slot,
                            observations: observations.clone(),
                        },
                        vec![
                            AccountMeta::new(self.env.payer.pubkey(), true),
                            AccountMeta::new(self.env.market, false),
                            AccountMeta::new(self.portfolios[actor], false),
                        ],
                        &[],
                    )
                    .expect("admitted risk retains a bounded public settlement step");
                self.calls += 1;
                assert_cu_within("historical/latent capacity settlement", cu, CU_LIMIT);
                self.max_crank = self.max_crank.max(cu);
                self.assert_accounting();
                assert!(
                    rank(self) < before_rank,
                    "each economic crank strictly reduces pending work"
                );
            }
            assert_eq!(
                rank(self),
                0,
                "economic endpoint reached in at most four calls"
            );
        }
        assert_eq!(self.source_claims(0), self.claims.map(|c| c * BOUND_SCALE));
        assert_eq!(
            self.env.portfolio_state(self.portfolios[1]).capital.get(),
            CAPITAL - gain
        );
        self.previous_claims = self.claims;
        self.previous_prices = self.prices;
        self.assert_accounting();
    }

    fn payout(&mut self, order: [usize; 2]) -> [u128; 2] {
        assert_eq!(self.positions, [0; ASSETS]);
        let gain = self.claims.iter().sum::<u128>();
        let mint = self.env.svm.get_account(&self.env.mint).unwrap();
        let vault = self.env.svm.get_account(&self.env.vault).unwrap();
        self.env.svm.expire_blockhash();
        self.max_convert =
            self.env
                .convert_released_pnl_with_cu(&self.owners[0], self.portfolios[0], gain);
        self.calls += 1;
        assert_cu_within(
            "historical/latent capacity conversion",
            self.max_convert,
            CU_LIMIT,
        );
        assert_eq!(self.env.svm.get_account(&self.env.vault).unwrap(), vault);
        let payouts = [CAPITAL + gain, CAPITAL - gain];
        let mut remaining = 2 * CAPITAL;
        for actor in order {
            let portfolio = self.env.portfolio_state(self.portfolios[actor]);
            assert_eq!(portfolio.capital.get(), payouts[actor]);
            assert_eq!((portfolio.pnl.get(), portfolio.reserved_pnl.get()), (0, 0));
            assert!(portfolio.source_domains.iter().all(|s| !s.is_occupied()));
            let other_before = self.env.svm.get_account(&self.portfolios[1 - actor]);
            self.env.svm.expire_blockhash();
            let cu = self
                .env
                .send(
                    self.env.withdraw_ix(self.portfolios[actor], payouts[actor]),
                    vec![
                        AccountMeta::new(self.owners[actor].pubkey(), true),
                        AccountMeta::new(self.env.market, false),
                        AccountMeta::new(self.portfolios[actor], false),
                        AccountMeta::new(self.tokens[actor], false),
                        AccountMeta::new(self.env.vault, false),
                        AccountMeta::new_readonly(self.env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&self.owners[actor]],
                )
                .expect("withdraw complete principal and converted claim to the public ATA");
            self.calls += 1;
            self.max_withdraw = self.max_withdraw.max(cu);
            assert_cu_within(
                "historical/latent capacity withdrawal",
                cu,
                CUSTODY_CU_LIMIT,
            );
            remaining -= payouts[actor];
            assert_eq!(
                self.env.token_amount(self.tokens[actor]) as u128,
                payouts[actor]
            );
            assert_eq!(self.env.token_amount(self.env.vault) as u128, remaining);
            assert_eq!(self.env.market_state().1.c_tot, remaining);
            self.env.svm.expire_blockhash();
            let cu = self
                .env
                .close_portfolio_with_cu(&self.owners[actor], self.portfolios[actor]);
            self.calls += 1;
            self.max_close = self.max_close.max(cu);
            assert_cu_within("historical/latent capacity close", cu, CUSTODY_CU_LIMIT);
            assert_eq!(
                self.env.svm.get_account(&self.portfolios[1 - actor]),
                other_before
            );
        }
        let group = self.env.market_state().1;
        assert_eq!((group.c_tot, group.vault, group.insurance), (0, 0, 0));
        assert_eq!(group.materialized_portfolio_count, 0);
        assert_eq!(self.env.svm.get_account(&self.env.mint).unwrap(), mint);
        for domain in 0..DOMAINS {
            assert_eq!(group.source_credit[domain].positive_claim_bound_num, 0);
            assert_eq!(group.source_credit[domain].fresh_reserved_backing_num, 0);
            assert_eq!(
                group.source_backing_buckets[domain].fresh_unliened_backing_num,
                0
            );
        }
        payouts
    }
}

#[test]
fn v16_program_historical_and_latent_domains_share_bounded_settlement_capacity() {
    let routes = [
        AccountResidualCounterTradePath::TradeNoCpi,
        AccountResidualCounterTradePath::TradeCpi,
        AccountResidualCounterTradePath::BatchTradeNoCpi,
        AccountResidualCounterTradePath::BatchTradeCpi,
    ];
    let mut worlds = 0;
    let mut calls = 0;
    let mut maxima = [0; 5];
    for historical_assets in [ASSETS - 2, ASSETS - 1] {
        for direction in [-1i128, 1] {
            for reverse in [false, true] {
                let order = if reverse { [1, 0] } else { [0, 1] };
                let mut endpoint = None;
                for rotation in 0..routes.len() {
                    let mut h = History::new();
                    let mut historical: Vec<_> = (0..historical_assets as u16).collect();
                    if reverse {
                        historical.reverse();
                    }
                    for asset in historical {
                        let q = (1 + i128::from(asset % 3)) * POS_SCALE as i128;
                        h.trade(routes[0], &[(asset, q, PRICE)]);
                        h.mark_and_settle(&[(asset, PRICE + 1)], order);
                        h.trade(routes[0], &[(asset, -2 * q, PRICE + 1)]);
                        h.mark_and_settle(&[(asset, PRICE)], order);
                        h.trade(routes[0], &[(asset, q, PRICE)]);
                    }
                    let historical_claims = h.source_claims(0);
                    assert_eq!(
                        historical_claims.iter().filter(|c| **c != 0).count(),
                        2 * historical_assets
                    );
                    let mut latent: Vec<_> = (historical_assets..ASSETS)
                        .map(|asset| {
                            let units = 7 + 4 * (asset - historical_assets) as i128;
                            let sign = if asset % 2 == 0 {
                                direction
                            } else {
                                -direction
                            };
                            (asset as u16, sign * units * POS_SCALE as i128, PRICE)
                        })
                        .collect();
                    if reverse {
                        latent.reverse();
                    }
                    h.trade(routes[rotation], &latent);
                    assert_eq!(
                        h.source_claims(0),
                        historical_claims,
                        "admitted positions are still latent"
                    );
                    assert_eq!(2 * historical_assets + 2 * latent.len(), DOMAINS);
                    let marks: Vec<_> = latent
                        .iter()
                        .map(|&(asset, q, _)| (asset, (PRICE as i128 + q.signum()) as u64))
                        .collect();
                    h.mark_and_settle(&marks, order);
                    let reversals: Vec<_> = latent
                        .iter()
                        .zip(&marks)
                        .map(|(&(asset, q, _), &(_, mark))| (asset, -2 * q, mark))
                        .collect();
                    h.trade(routes[(rotation + 1) % routes.len()], &reversals);
                    h.mark_and_settle(
                        &latent.iter().map(|l| (l.0, PRICE)).collect::<Vec<_>>(),
                        order,
                    );
                    let full = h.source_claims(0);
                    assert!(
                        full.iter().all(|c| *c > 0),
                        "both sides of every future domain materialize"
                    );
                    assert_eq!(
                        &full[..2 * historical_assets],
                        &historical_claims[..2 * historical_assets],
                        "new settlements cannot consume or replace historical attribution"
                    );
                    h.trade(routes[(rotation + 2) % routes.len()], &latent);
                    let payouts = h.payout(order);
                    if let Some(expected) = endpoint {
                        assert_eq!(payouts, expected);
                    }
                    endpoint = Some(payouts);
                    worlds += 1;
                    calls += h.calls;
                    for (max, cu) in maxima.iter_mut().zip([
                        h.max_trade,
                        h.max_crank,
                        h.max_convert,
                        h.max_withdraw,
                        h.max_close,
                    ]) {
                        *max = (*max).max(cu);
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    println!("INV-028 historical/latent boundary: worlds={worlds}, post-funding calls={calls}, max CU trade/crank/convert/withdraw/close={maxima:?}");
}
