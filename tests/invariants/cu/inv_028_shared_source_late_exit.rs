//! Row 423: two full source tables share backing through staggered last-domain
//! settlement, cohort readiness and complete public exit. Economic state is public.
//! The input ledger tracks unequal owner claims and each domain's remaining backing.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};

const DEBTOR: usize = 2;
const OWNER_WINDOW: u64 = 5;
type Claims = [[u128; DOMAINS]; 2];

struct SharedHistory {
    env: V16CuEnv,
    owners: [Keypair; 3],
    portfolios: [Pubkey; 3],
    tokens: [Pubkey; 3],
    positions: [[i128; ASSETS]; 3],
    prices: [u64; ASSETS],
    earned: Claims,
    floor: Claims,
    redeemed: Claims,
    funded: [u128; DOMAINS],
    funded_floor: [u128; DOMAINS],
    slot: u64,
    calls: usize,
    peak_cu: u64,
    peak_packet: usize,
}

impl SharedHistory {
    fn new() -> Self {
        assert_eq!(DOMAINS, 2 * ASSETS);
        let mut env = inv018_public_spl_market_with_capacity(
            0,
            V16CuMarketParams {
                max_portfolio_assets: ASSETS as u16,
                initial_price: PRICE,
                max_price_move_bps_per_slot: 150,
                max_accrual_dt_slots: 64,
                min_funding_lifetime_slots: 64,
                ..V16CuMarketParams::default()
            },
            ASSETS,
        );
        for asset in 0..ASSETS {
            env.configure_auth_mark_for_asset_as_admin(asset as u16, 0, PRICE);
        }
        env.configure_permissionless_resolve_with_cu(1_000, OWNER_WINDOW);
        let owners = std::array::from_fn(|_| Keypair::new());
        let mut portfolios = [Pubkey::default(); 3];
        let mut tokens = [Pubkey::default(); 3];
        for actor in 0..3 {
            env.svm
                .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                .unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            portfolios[actor] = portfolio.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            env.portfolios.push(portfolios[actor]);
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
                    CAPITAL as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[actor], CAPITAL),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
        }
        let h = Self {
            env,
            owners,
            portfolios,
            tokens,
            positions: [[0; ASSETS]; 3],
            prices: [PRICE; ASSETS],
            earned: [[0; DOMAINS]; 2],
            floor: [[0; DOMAINS]; 2],
            redeemed: [[0; DOMAINS]; 2],
            funded: [0; DOMAINS],
            funded_floor: [0; DOMAINS],
            slot: 0,
            calls: 0,
            peak_cu: 0,
            peak_packet: 0,
        };
        h.check();
        h
    }

    fn claims(&self, actor: usize) -> [u128; DOMAINS] {
        let mut claims = [0; DOMAINS];
        let mut domains = BTreeSet::new();
        for source in self
            .env
            .portfolio_state(self.portfolios[actor])
            .source_domains
        {
            if source.is_occupied() {
                let domain = source.domain.get() as usize;
                assert!(domain < DOMAINS && domains.insert(domain));
                claims[domain] = source.source_claim_bound_num.get() / BOUND_SCALE;
                assert_eq!(
                    source.source_claim_bound_num.get(),
                    claims[domain] * BOUND_SCALE
                );
                assert_eq!(source.source_claim_liened_num.get(), 0);
                assert_eq!(source.source_lien_counterparty_backing_num.get(), 0);
                assert_eq!(source.source_lien_insurance_backing_num.get(), 0);
            }
        }
        claims
    }

    fn valid_claim_prefix(&self, claims: Claims) -> bool {
        (0..2).all(|actor| {
            (0..DOMAINS).all(|d| {
                let face = claims[actor][d] + self.redeemed[actor][d];
                face == self.floor[actor][d] || face == self.earned[actor][d]
            })
        })
    }

    fn check(&self) {
        let market = self.env.svm.get_account(&self.env.market).unwrap();
        let group = self.env.market_state().1;
        let accounts = self.portfolios.map(|p| self.env.portfolio_state(p));
        let claims = std::array::from_fn(|actor| self.claims(actor));
        assert!(
            self.valid_claim_prefix(claims),
            "input-derived owner/domain claims"
        );
        assert_eq!(self.claims(DEBTOR), [0; DOMAINS]);
        let paid = self.tokens.map(|t| self.env.token_amount(t) as u128);
        assert_eq!(group.insurance, 0);
        assert_eq!(group.vault + paid.iter().sum::<u128>(), 3 * CAPITAL);
        assert_eq!(self.env.token_amount(self.env.vault) as u128, group.vault);
        let mut backing = 0;
        for d in 0..DOMAINS {
            let redeemed = self.redeemed[0][d] + self.redeemed[1][d];
            let bucket = &group.source_backing_buckets[d];
            let source = &group.source_credit[d];
            let stock = bucket.fresh_unliened_backing_num;
            assert!(
                stock == (self.funded_floor[d] - redeemed) * BOUND_SCALE
                    || stock == (self.funded[d] - redeemed) * BOUND_SCALE,
                "domain {d}: backing belongs to the unpaid input ledger"
            );
            backing += stock / BOUND_SCALE;
            let face = (claims[0][d] + claims[1][d]) * BOUND_SCALE;
            assert_eq!(source.exact_positive_claim_num, face);
            assert_eq!(source.positive_claim_bound_num, face);
            assert_eq!(source.fresh_reserved_backing_num, stock);
            assert_eq!(source.valid_liened_backing_num, 0);
            assert_eq!(source.impaired_liened_backing_num, 0);
            assert_eq!(source.insurance_credit_reserved_num, 0);
            assert!(face * source.credit_rate_num / percolator::CREDIT_RATE_SCALE <= stock);
        }
        let redeemed = self.redeemed.iter().flatten().sum::<u128>();
        assert_eq!(
            accounts[DEBTOR].capital.get() + paid[DEBTOR],
            CAPITAL - backing - redeemed
        );
        assert_eq!(accounts[DEBTOR].pnl.get(), 0);
        for actor in 0..2 {
            assert_eq!(
                accounts[actor].pnl.get(),
                claims[actor].iter().sum::<u128>() as i128
            );
            assert_eq!(
                accounts[actor].capital.get() + paid[actor],
                CAPITAL + self.redeemed[actor].iter().sum::<u128>()
            );
            let mut resources: BTreeSet<_> =
                (0..DOMAINS).filter(|&d| claims[actor][d] != 0).collect();
            for a in 0..ASSETS {
                if self.positions[actor][a] != 0 {
                    resources.extend([2 * a, 2 * a + 1]);
                }
            }
            assert!(resources.len() <= DOMAINS);
        }
        for a in 0..ASSETS {
            let positions = self.positions.map(|q| q[a]);
            for actor in 0..3 {
                assert_eq!(
                    has_active_leg_for_asset(&accounts[actor], a),
                    positions[actor] != 0
                );
                if positions[actor] != 0 {
                    assert_eq!(
                        active_leg_for_asset(&accounts[actor], a).basis_pos_q,
                        positions[actor]
                    );
                }
            }
            // Resolved sides retain aggregate OI until all account legs detach.
            if group.mode == MarketModeV16::Live {
                assert_eq!(
                    group.assets[a].oi_eff_long_q,
                    positions
                        .iter()
                        .filter(|q| **q > 0)
                        .map(|q| q.unsigned_abs())
                        .sum::<u128>()
                );
                assert_eq!(
                    group.assets[a].oi_eff_short_q,
                    positions
                        .iter()
                        .filter(|q| **q < 0)
                        .map(|q| q.unsigned_abs())
                        .sum::<u128>()
                );
            }
        }
        assert_market_stock_census(
            "shared late source",
            &group,
            &market.data,
            &accounts,
            group.vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("shared late source", &group, &accounts).unwrap();
        assert_source_credit_rates("shared late source", &group).unwrap();
    }

    fn record(&mut self, cu: u64) {
        assert_cu_within("shared historical/latent exit", cu, CU_LIMIT);
        self.peak_cu = self.peak_cu.max(cu);
        self.calls += 1;
    }

    fn trade(&mut self, actor: usize, asset: usize, delta: i128) {
        let peer = self.env.svm.get_account(&self.portfolios[1 - actor]);
        self.env.svm.expire_blockhash();
        let cu = self
            .env
            .try_trade_asset_with_cu(
                asset as u16,
                &self.owners[actor],
                self.portfolios[actor],
                &self.owners[DEBTOR],
                self.portfolios[DEBTOR],
                delta,
                self.prices[asset],
                0,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "actor={actor}, asset={asset}, delta={delta}, slot={}: {error}",
                    self.slot
                )
            });
        self.positions[actor][asset] += delta;
        self.positions[DEBTOR][asset] -= delta;
        self.record(cu);
        assert_eq!(self.env.svm.get_account(&self.portfolios[1 - actor]), peer);
        self.check();
    }

    fn mark(&mut self, asset: usize, price: u64) {
        self.floor = self.earned;
        self.funded_floor = self.funded;
        for actor in 0..2 {
            let q = self.positions[actor][asset];
            let gain = q / POS_SCALE as i128 * (price as i128 - self.prices[asset] as i128);
            assert!(gain > 0);
            let domain = 2 * asset + usize::from(q > 0);
            self.earned[actor][domain] += gain as u128;
            self.funded[domain] += gain as u128;
        }
        self.prices[asset] = price;
        self.slot += 1;
        self.env.svm.warp_to_slot(self.slot);
        let frames = self.portfolios.map(|p| self.env.svm.get_account(&p));
        let cu = self
            .env
            .push_auth_mark_for_asset_as_admin(asset as u16, self.slot, price);
        self.record(cu);
        assert_eq!(
            self.portfolios.map(|p| self.env.svm.get_account(&p)),
            frames
        );
        self.check();
    }

    fn settle(&mut self, actor: usize, asset: usize) {
        let target = if actor == DEBTOR {
            CAPITAL - self.funded.iter().sum::<u128>()
        } else {
            self.earned[actor].iter().sum::<u128>()
        };
        let rank = |h: &Self| {
            let account = h.env.portfolio_state(h.portfolios[actor]);
            let value = if actor == DEBTOR {
                account.capital.get()
            } else {
                account.pnl.get() as u128
            };
            value.abs_diff(target)
                + u128::from(h.slot - h.env.market_state().1.assets[asset].slot_last)
        };
        for _ in 0..4 {
            let before = rank(self);
            if before == 0 {
                break;
            }
            let frames = self.portfolios.map(|p| self.env.svm.get_account(&p));
            self.env.svm.expire_blockhash();
            let cu = self.env.crank(
                self.portfolios[actor],
                ProgInstruction::PermissionlessCrank {
                    now_slot: self.slot,
                    observations: crank_observations(asset as u16),
                },
            );
            self.record(cu);
            assert!(
                rank(self) < before,
                "input debt and authenticated accrual decrease"
            );
            for peer in 0..3 {
                if peer != actor {
                    assert_eq!(
                        self.env.svm.get_account(&self.portfolios[peer]),
                        frames[peer]
                    );
                }
            }
            self.check();
        }
        assert_eq!(rank(self), 0, "settlement bound is four calls per owner");
        if actor == DEBTOR {
            self.funded_floor = self.funded;
        } else {
            self.floor[actor] = self.earned[actor];
        }
        self.check();
    }

    fn advance_claimant(&mut self, actor: usize, automatic: bool, materialize_only: bool) -> usize {
        let latent = self.earned[actor]
            .iter()
            .zip(self.floor[actor])
            .filter(|(a, b)| **a != *b)
            .count();
        assert_eq!(
            latent,
            usize::from(materialize_only),
            "materialization and payout phases have distinct input frontiers"
        );
        let mut remaining = std::array::from_fn::<_, DOMAINS, _>(|d| {
            self.earned[actor][d] - self.redeemed[actor][d]
        });
        let mut calls = 0;
        let entitlement = CAPITAL + remaining.iter().sum::<u128>();
        let rank = |h: &Self| {
            let latent = h.earned[actor]
                .iter()
                .zip(h.floor[actor])
                .filter(|(a, b)| **a != *b)
                .count();
            2 * latent
                + h.positions[actor].iter().filter(|q| **q != 0).count()
                + h.claims(actor).iter().filter(|c| **c != 0).count()
                + usize::from((h.env.token_amount(h.tokens[actor]) as u128) < entitlement)
        };
        for _ in 0..if materialize_only { 1 } else { DOMAINS + 3 } {
            if resolved_portfolio_is_terminal(&self.env, self.portfolios[actor]) {
                break;
            }
            let before_rank = rank(self);
            let peers = self.portfolios.map(|p| self.env.svm.get_account(&p));
            let tokens = self.tokens.map(|p| self.env.svm.get_account(&p));
            let mint = self.env.svm.get_account(&self.env.mint);
            let ix = Instruction {
                program_id: self.env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(self.owners[actor].pubkey(), false),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(self.portfolios[actor], false),
                    AccountMeta::new(self.tokens[actor], false),
                    AccountMeta::new(self.env.vault, false),
                    AccountMeta::new_readonly(self.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: if automatic {
                    ProgInstruction::PermissionlessCrank {
                        now_slot: self.slot,
                        observations: vec![],
                    }
                } else {
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    }
                }
                .encode(),
            };
            self.env.svm.expire_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
                    ix,
                ],
                Some(&self.env.payer.pubkey()),
                &[&self.env.payer],
                self.env.svm.latest_blockhash(),
            );
            assert_eq!(tx.message.header.num_required_signatures, 1);
            let bytes = bincode::serialize(&tx).unwrap().len();
            assert!(bytes <= 1232);
            self.peak_packet = self.peak_packet.max(bytes);
            let cu = self
                .env
                .svm
                .send_transaction(tx)
                .expect("shared backing retains a public exit")
                .compute_units_consumed;
            self.record(cu);
            let after = self.claims(actor);
            let removed: Vec<_> = (0..DOMAINS).filter(|&d| remaining[d] != after[d]).collect();
            assert!(removed.len() <= 1, "one bounded input claim disposition");
            for d in removed {
                assert!(remaining[d] > 0);
                assert_eq!(after[d], 0);
                self.redeemed[actor][d] = remaining[d];
                remaining[d] = 0;
            }
            self.floor[actor] = self.earned[actor];
            if percolator::active_bitmap_is_empty(active_bitmap(
                &self.env.portfolio_state(self.portfolios[actor]),
            )) {
                self.positions[actor] = [0; ASSETS];
            }
            assert!(rank(self) < before_rank, "terminal work strictly decreases");
            for peer in 0..3 {
                if peer != actor {
                    assert_eq!(
                        self.env.svm.get_account(&self.portfolios[peer]),
                        peers[peer]
                    );
                    assert_eq!(self.env.svm.get_account(&self.tokens[peer]), tokens[peer]);
                }
            }
            assert_eq!(self.env.svm.get_account(&self.env.mint), mint);
            self.check();
            calls += 1;
        }
        if materialize_only {
            assert_eq!(calls, 1);
            assert!(
                self.claims(actor).iter().all(|c| *c > 0),
                "each claimant independently fills all 28 source records"
            );
            assert_eq!(self.env.token_amount(self.tokens[actor]), 0);
        } else {
            assert_eq!(rank(self), 0);
            assert_eq!(remaining, [0; DOMAINS]);
            assert!(resolved_portfolio_is_terminal(
                &self.env,
                self.portfolios[actor]
            ));
            assert_eq!(
                self.env.token_amount(self.tokens[actor]) as u128,
                entitlement
            );
        }
        calls
    }
}

#[test]
fn v16_program_shared_history_preserves_late_claimant_capacity_and_exact_exit() {
    assert_certified_engine_pin("INV-028 shared source late exit");
    let mut peak_cu = 0;
    let mut peak_packet = 0;
    let mut calls = 0;
    let mut terminal_calls = 0;
    for direction in [-1i128, 1] {
        for first in [0, 1] {
            let mut h = SharedHistory::new();
            for asset in 0..ASSETS {
                let cu = h
                    .env
                    .push_auth_mark_for_asset_as_admin(asset as u16, h.slot, PRICE);
                h.record(cu);
                h.check();
                h.settle(DEBTOR, asset);
                let quantities = [1 + asset as i128 % 3, 4 + asset as i128 % 5]
                    .map(|q| direction * q * POS_SCALE as i128);
                for actor in 0..2 {
                    h.trade(actor, asset, quantities[actor]);
                }
                h.mark(asset, (PRICE as i128 + direction) as u64);
                for actor in [DEBTOR, 1, 0] {
                    h.settle(actor, asset);
                }
                for actor in 0..2 {
                    h.trade(actor, asset, -2 * quantities[actor]);
                }
                if asset == ASSETS - 1 {
                    break;
                }
                h.mark(asset, PRICE);
                for actor in [0, DEBTOR, 1] {
                    h.settle(actor, asset);
                }
                for actor in 0..2 {
                    h.trade(actor, asset, quantities[actor]);
                }
            }
            let historical = [h.claims(0), h.claims(1)];
            for claims in historical {
                assert_eq!(claims.iter().filter(|c| **c > 0).count(), DOMAINS - 1);
            }
            let mut wrong_owner = historical;
            wrong_owner.swap(0, 1);
            assert!(
                !h.valid_claim_prefix(wrong_owner),
                "owner swap must fail despite unchanged domain totals"
            );
            let mut wrong_domain = historical;
            wrong_domain[0][0] += 1;
            wrong_domain[0][1] -= 1;
            assert!(
                !h.valid_claim_prefix(wrong_domain),
                "domain transfer must fail despite unchanged owner totals"
            );
            h.mark(ASSETS - 1, PRICE);
            h.settle(DEBTOR, ASSETS - 1);
            assert_eq!([h.claims(0), h.claims(1)], historical);
            let frames = h.portfolios.map(|p| h.env.svm.get_account(&p));
            let cu = h.env.resolve();
            h.record(cu);
            assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), frames);
            h.check();
            h.slot = h.env.market_state().1.resolved_slot + OWNER_WINDOW;
            h.env.svm.warp_to_slot(h.slot);
            terminal_calls += h.advance_claimant(first, false, true);
            assert_eq!(h.claims(1 - first), historical[1 - first]);
            assert_eq!(h.env.token_amount(h.tokens[1 - first]), 0);
            let late_domain = 2 * (ASSETS - 1) + usize::from(direction < 0);
            let group = h.env.market_state().1;
            assert_eq!(
                group.source_credit[late_domain].positive_claim_bound_num,
                h.earned[first][late_domain] * BOUND_SCALE
            );
            assert_eq!(
                group.source_backing_buckets[late_domain].fresh_unliened_backing_num,
                h.funded[late_domain] * BOUND_SCALE
            );
            terminal_calls += h.advance_claimant(1 - first, true, true);
            let claimant_frames = [h.portfolios[0], h.portfolios[1], h.tokens[0], h.tokens[1]]
                .map(|p| h.env.svm.get_account(&p));
            h.env.svm.expire_blockhash();
            let cu = h
                .env
                .send(
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    vec![
                        AccountMeta::new_readonly(h.owners[DEBTOR].pubkey(), false),
                        AccountMeta::new(h.env.market, false),
                        AccountMeta::new(h.portfolios[DEBTOR], false),
                        AccountMeta::new(h.tokens[DEBTOR], false),
                        AccountMeta::new(h.env.vault, false),
                        AccountMeta::new_readonly(h.env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[],
                )
                .expect("debtor retains its complete senior remainder");
            h.positions[DEBTOR] = [0; ASSETS];
            h.record(cu);
            terminal_calls += 1;
            assert_eq!(
                [h.portfolios[0], h.portfolios[1], h.tokens[0], h.tokens[1]]
                    .map(|p| h.env.svm.get_account(&p)),
                claimant_frames
            );
            h.check();
            assert_eq!(
                h.env.token_amount(h.tokens[DEBTOR]) as u128,
                CAPITAL - h.funded.iter().sum::<u128>()
            );
            terminal_calls += h.advance_claimant(first, false, false);
            assert_eq!(h.claims(1 - first), h.earned[1 - first]);
            assert_eq!(h.env.token_amount(h.tokens[1 - first]), 0);
            terminal_calls += h.advance_claimant(1 - first, true, false);
            for actor in 0..3 {
                assert!(resolved_portfolio_is_terminal(&h.env, h.portfolios[actor]));
                let custody = [
                    h.env.vault,
                    h.env.mint,
                    h.tokens[0],
                    h.tokens[1],
                    h.tokens[2],
                ]
                .map(|p| h.env.svm.get_account(&p));
                let portfolios = h.portfolios.map(|p| h.env.svm.get_account(&p));
                h.env.svm.expire_blockhash();
                let cu = h
                    .env
                    .close_portfolio_with_cu(&h.owners[actor], h.portfolios[actor]);
                assert_cu_within(
                    "shared source empty portfolio deletion",
                    cu,
                    CUSTODY_CU_LIMIT,
                );
                h.record(cu);
                assert_eq!(
                    [
                        h.env.vault,
                        h.env.mint,
                        h.tokens[0],
                        h.tokens[1],
                        h.tokens[2]
                    ]
                    .map(|p| h.env.svm.get_account(&p)),
                    custody
                );
                for peer in 0..3 {
                    if peer != actor {
                        assert_eq!(h.env.svm.get_account(&h.portfolios[peer]), portfolios[peer]);
                    }
                }
                let group = h.env.market_state().1;
                assert_eq!((group.vault, group.c_tot, group.insurance), (0, 0, 0));
                assert_eq!(group.materialized_portfolio_count, (2 - actor) as u64);
            }
            let group = h.env.market_state().1;
            assert_eq!(
                (
                    group.vault,
                    group.c_tot,
                    group.insurance,
                    group.materialized_portfolio_count
                ),
                (0, 0, 0, 0)
            );
            assert_eq!(h.env.token_amount(h.env.vault), 0);
            for asset in group.assets {
                assert_eq!((asset.oi_eff_long_q, asset.oi_eff_short_q), (0, 0));
            }
            peak_cu = peak_cu.max(h.peak_cu);
            peak_packet = peak_packet.max(h.peak_packet);
            calls += h.calls;
        }
    }
    println!("INV-028 shared late claimant: worlds=4, calls={calls}, terminal_calls={terminal_calls}, peak_cu={peak_cu}, peak_packet={peak_packet}");
}
