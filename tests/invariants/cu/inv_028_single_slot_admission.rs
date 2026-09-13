//! INV-028 / row 423: one vacant source slot after history spans more than fourteen assets.
//!
//! Twenty-seven one-sided, detached claims leave room for the missing side of an old asset,
//! but not both domains of an unrelated asset. The admitted opposite-side episode must settle
//! new value into that last slot, preserve every historical claim, and pay both owners fully.
//! This is partial domain overlap, not paired history, reclamation, or a maximum-active-leg test.
//! Batch admission also must accumulate distinct future domains across individually fitting legs.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;
use std::collections::BTreeSet;

#[path = "inv_028_latent_capacity_reuse.rs"]
mod latent_capacity_reuse;

const CAPACITY: usize = percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS;
const ASSETS: usize = CAPACITY;
const HISTORY: usize = CAPACITY - 1;
const CAPITAL: u128 = 1_000_000;
const PRICE: u64 = 100;
const CU_LIMIT: u64 = 1_375_000;

struct SparseHistory {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    mint: Account,
    claims: [u128; 2 * ASSETS],
    previous_claims: [u128; 2 * ASSETS],
    prices: [u64; ASSETS],
    position: Option<(u16, i128)>,
    slot: u64,
    calls: usize,
    maxima: [u64; 5],
}

impl SparseHistory {
    fn new() -> Self {
        let active_cap = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
        assert_eq!((active_cap, CAPACITY), (14, 28));
        let mut env = inv018_public_spl_market_with_capacity(
            0,
            V16CuMarketParams {
                max_portfolio_assets: active_cap,
                initial_price: PRICE,
                max_price_move_bps_per_slot: 150,
                max_accrual_dt_slots: 64,
                min_funding_lifetime_slots: 64,
                ..V16CuMarketParams::default()
            },
            ASSETS,
        );
        for asset in active_cap..ASSETS as u16 {
            env.activate_asset(asset, u64::from(asset - active_cap + 1), PRICE);
        }
        let slot = u64::from(ASSETS as u16 - active_cap);
        for asset in 0..ASSETS as u16 {
            env.configure_auth_mark_for_asset_as_admin(asset, slot, PRICE);
        }
        env.portfolio_account_len = state::portfolio_account_len_for_market_slots(ASSETS).unwrap();
        let owners = [Keypair::new(), Keypair::new()];
        let mut portfolios = [Pubkey::default(); 2];
        let mut tokens = [Pubkey::default(); 2];
        for actor in 0..2 {
            env.svm
                .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                .unwrap();
            let key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                env.portfolio_account_len,
                env.program_id,
            );
            portfolios[actor] = key.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                ],
                &[&owners[actor]],
            )
            .expect("initialize System-created portfolio");
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
            .expect("SPL funds owner collateral");
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
            .expect("public collateral deposit");
        }
        let mint = env.svm.get_account(&env.mint).unwrap();
        let h = Self {
            env,
            owners,
            portfolios,
            tokens,
            mint,
            claims: [0; 2 * ASSETS],
            previous_claims: [0; 2 * ASSETS],
            prices: [PRICE; ASSETS],
            position: None,
            slot,
            calls: 0,
            maxima: [0; 5],
        };
        h.check();
        h
    }

    fn observe_cu(&mut self, route: usize, cu: u64) {
        self.calls += 1;
        self.maxima[route] = self.maxima[route].max(cu);
        assert_cu_within("single-slot admission history", cu, CU_LIMIT);
    }

    fn claims_for(&self, actor: usize) -> [u128; 2 * ASSETS] {
        let mut claims = [0; 2 * ASSETS];
        let mut seen = BTreeSet::new();
        let account = self.env.portfolio_state(self.portfolios[actor]);
        for source in account.source_domains.iter().filter(|s| s.is_occupied()) {
            let domain = source.domain.get() as usize;
            assert!(domain < claims.len() && seen.insert(domain));
            claims[domain] = source.source_claim_bound_num.get();
            assert!(claims[domain] > 0, "every occupied slot retains real value");
            assert_eq!(source.source_claim_liened_num.get(), 0);
            assert_eq!(source.source_lien_counterparty_backing_num.get(), 0);
            assert_eq!(source.source_lien_insurance_backing_num.get(), 0);
        }
        if let Some((asset, _)) = self.position {
            seen.extend([2 * asset as usize, 2 * asset as usize + 1]);
        }
        assert!(
            seen.len() <= CAPACITY,
            "history and future domains fit together"
        );
        claims
    }

    fn check(&self) {
        let claims = self.claims_for(0);
        assert_eq!(self.claims_for(1), [0; 2 * ASSETS]);
        let group = self.env.market_state().1;
        let mut backing = 0;
        for domain in 0..2 * ASSETS {
            let old = self.previous_claims[domain] * BOUND_SCALE;
            let next = self.claims[domain] * BOUND_SCALE;
            assert!(claims[domain] == old || claims[domain] == next);
            let source = &group.source_credit[domain];
            let bucket = &group.source_backing_buckets[domain];
            assert_eq!(source.exact_positive_claim_num, claims[domain]);
            assert_eq!(source.positive_claim_bound_num, claims[domain]);
            assert_eq!(source.valid_liened_backing_num, 0);
            assert_eq!(source.impaired_liened_backing_num, 0);
            assert_eq!(source.insurance_credit_reserved_num, 0);
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(bucket.impaired_liened_backing_num, 0);
            let available = bucket.fresh_unliened_backing_num;
            assert!(available == old || available == next);
            assert_eq!(source.fresh_reserved_backing_num, available);
            assert!(
                claims[domain] * source.credit_rate_num / percolator::CREDIT_RATE_SCALE
                    <= available
            );
            backing += available / BOUND_SCALE;
        }
        for actor in 0..2 {
            let account = self.env.portfolio_state(self.portfolios[actor]);
            assert_eq!(
                account.capital.get(),
                CAPITAL - if actor == 1 { backing } else { 0 }
            );
            assert_eq!(
                account.pnl.get(),
                if actor == 0 {
                    (claims.iter().sum::<u128>() / BOUND_SCALE) as i128
                } else {
                    0
                }
            );
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&account)),
                u32::from(self.position.is_some())
            );
            for asset in 0..ASSETS {
                let q = self
                    .position
                    .filter(|p| p.0 as usize == asset)
                    .map_or(0, |p| p.1);
                assert_eq!(has_active_leg_for_asset(&account, asset), q != 0);
                if q != 0 {
                    assert_eq!(
                        active_leg_for_asset(&account, asset).basis_pos_q,
                        q * if actor == 0 { 1 } else { -1 }
                    );
                }
                assert_eq!(group.assets[asset].oi_eff_long_q, q.unsigned_abs());
                assert_eq!(group.assets[asset].oi_eff_short_q, q.unsigned_abs());
            }
        }
        assert_eq!(
            (group.c_tot, group.vault, group.insurance),
            (2 * CAPITAL - backing, 2 * CAPITAL, 0)
        );
        assert_eq!(self.env.token_amount(self.env.vault) as u128, 2 * CAPITAL);
        assert!(self.tokens.iter().all(|t| self.env.token_amount(*t) == 0));
        assert_eq!(self.env.svm.get_account(&self.env.mint).unwrap(), self.mint);
        assert_eq!(
            Mint::unpack(&self.mint.data).unwrap().supply as u128,
            2 * CAPITAL
        );
    }

    fn trade(&mut self, asset: u16, q: i128) {
        let old = self.position.map_or(0, |p| {
            assert_eq!(p.0, asset);
            p.1
        });
        self.env.svm.expire_blockhash();
        let cu = self.env.trade_asset_with_cu(
            asset,
            &self.owners[0],
            self.portfolios[0],
            &self.owners[1],
            self.portfolios[1],
            q,
            self.prices[asset as usize],
            0,
        );
        self.position = (old + q != 0).then_some((asset, old + q));
        self.observe_cu(0, cu);
        self.check();
    }

    fn settle_mark(&mut self, price: u64, order: [usize; 2]) {
        let (asset, q) = self.position.unwrap();
        let gain = q / POS_SCALE as i128 * (price as i128 - self.prices[asset as usize] as i128);
        assert!(gain > 0);
        self.previous_claims = self.claims;
        self.claims[2 * asset as usize + usize::from(q > 0)] += gain as u128;
        self.prices[asset as usize] = price;
        self.slot += 1;
        self.env.svm.warp_to_slot(self.slot);
        let cu = self
            .env
            .push_auth_mark_for_asset_as_admin(asset, self.slot, price);
        self.observe_cu(1, cu);
        self.check();
        let total = self.claims.iter().sum::<u128>();
        for actor in order {
            let rank = |h: &Self| {
                let a = h.env.portfolio_state(h.portfolios[actor]);
                u128::from(h.slot - h.env.market_state().1.assets[asset as usize].slot_last)
                    + if actor == 0 {
                        a.pnl.get().abs_diff(total as i128)
                    } else {
                        a.capital.get().abs_diff(CAPITAL - total)
                    }
            };
            for _ in 0..4 {
                let before = rank(self);
                if before == 0 {
                    break;
                }
                let peer = self.env.svm.get_account(&self.portfolios[1 - actor]);
                self.env.svm.expire_blockhash();
                let cu = self.env.crank(
                    self.portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: self.slot,
                        observations: crank_observations(asset),
                    },
                );
                self.observe_cu(1, cu);
                self.check();
                assert_eq!(self.env.svm.get_account(&self.portfolios[1 - actor]), peer);
                assert!(
                    rank(self) < before,
                    "settlement must consume pending economic/accrual work"
                );
            }
            assert_eq!(
                rank(self),
                0,
                "admitted exposure settles in bounded public work"
            );
        }
        self.previous_claims = self.claims;
        self.check();
    }

    fn payout(&mut self, order: [usize; 2]) -> [u128; 2] {
        assert!(self.position.is_none());
        let gain = self.claims.iter().sum::<u128>();
        let vault = self.env.svm.get_account(&self.env.vault);
        let peer = self.env.svm.get_account(&self.portfolios[1]);
        self.env.svm.expire_blockhash();
        let cu = self
            .env
            .convert_released_pnl_with_cu(&self.owners[0], self.portfolios[0], gain);
        self.observe_cu(2, cu);
        assert_eq!(self.env.svm.get_account(&self.env.vault), vault);
        assert_eq!(self.env.svm.get_account(&self.portfolios[1]), peer);
        let payouts = [CAPITAL + gain, CAPITAL - gain];
        let mut remaining = 2 * CAPITAL;
        for actor in order {
            let account = self.env.portfolio_state(self.portfolios[actor]);
            assert_eq!(
                (
                    account.capital.get(),
                    account.pnl.get(),
                    account.reserved_pnl.get()
                ),
                (payouts[actor], 0, 0)
            );
            assert!(account.source_domains.iter().all(|s| !s.is_occupied()));
            let peer = self.env.svm.get_account(&self.portfolios[1 - actor]);
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
                .expect("complete funded owner payout");
            self.observe_cu(3, cu);
            assert_cu_within("single-slot owner withdrawal", cu, CUSTODY_CU_LIMIT);
            remaining -= payouts[actor];
            assert_eq!(
                self.env.token_amount(self.tokens[actor]) as u128,
                payouts[actor]
            );
            assert_eq!(self.env.token_amount(self.env.vault) as u128, remaining);
            let group = self.env.market_state().1;
            assert_eq!((group.c_tot, group.vault), (remaining, remaining));
            self.env.svm.expire_blockhash();
            let cu = self
                .env
                .close_portfolio_with_cu(&self.owners[actor], self.portfolios[actor]);
            self.observe_cu(4, cu);
            assert_cu_within("single-slot portfolio deletion", cu, CUSTODY_CU_LIMIT);
            assert_eq!(self.env.svm.get_account(&self.portfolios[1 - actor]), peer);
        }
        let group = self.env.market_state().1;
        assert_eq!(
            (
                group.c_tot,
                group.vault,
                group.insurance,
                group.materialized_portfolio_count
            ),
            (0, 0, 0, 0)
        );
        for domain in 0..2 * ASSETS {
            assert_eq!(group.source_credit[domain].positive_claim_bound_num, 0);
            assert_eq!(group.source_credit[domain].fresh_reserved_backing_num, 0);
            assert_eq!(
                group.source_backing_buckets[domain].fresh_unliened_backing_num,
                0
            );
        }
        assert_eq!(self.env.svm.get_account(&self.env.mint).unwrap(), self.mint);
        payouts
    }
}

#[test]
fn v16_program_single_vacant_domain_admission_preserves_historical_claims_and_exit() {
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    let mut calls = 0;
    let mut maxima = [0; 5];
    let mut worlds = 0;
    for direction in [-1i128, 1] {
        for reverse in [false, true] {
            let mut h = SparseHistory::new();
            let order = if reverse { [1, 0] } else { [0, 1] };
            let mut assets: Vec<_> = (0..HISTORY as u16).collect();
            if reverse {
                assets.reverse();
            }
            for &asset in &assets {
                let q = direction * (1 + i128::from(asset % 3)) * POS_SCALE as i128;
                h.trade(asset, q);
                h.settle_mark((PRICE as i128 + direction) as u64, order);
                h.trade(asset, -q);
            }
            let historical = h.claims_for(0);
            assert_eq!(historical.iter().filter(|c| **c > 0).count(), HISTORY);
            assert!(HISTORY > percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize);
            assert!(h.position.is_none());

            // One free slot cannot admit an unrelated domain pair. Use the real transaction
            // error and complete economic frames so a stale/compute failure cannot pass.
            let keys = [
                h.env.market,
                h.portfolios[0],
                h.portfolios[1],
                h.env.vault,
                h.env.mint,
                h.tokens[0],
                h.tokens[1],
                h.owners[0].pubkey(),
                h.owners[1].pubkey(),
                h.env.admin.pubkey(),
            ];
            let frame = keys.map(|key| h.env.svm.get_account(&key));
            h.env.svm.expire_blockhash();
            let ix = h.env.trade_no_cpi_ix(
                h.portfolios[0],
                h.portfolios[1],
                HISTORY as u16,
                POS_SCALE as i128,
                PRICE,
                0,
            );
            let tx = Transaction::new_signed_with_payer(
                &[
                    heap_ix(),
                    cu_ix(),
                    Instruction {
                        program_id: h.env.program_id,
                        accounts: vec![
                            AccountMeta::new(h.owners[0].pubkey(), true),
                            AccountMeta::new(h.owners[1].pubkey(), true),
                            AccountMeta::new(h.env.market, false),
                            AccountMeta::new(h.portfolios[0], false),
                            AccountMeta::new(h.portfolios[1], false),
                        ],
                        data: ix.encode(),
                    },
                ],
                Some(&h.env.payer.pubkey()),
                &[&h.env.payer, &h.owners[0], &h.owners[1]],
                h.env.svm.latest_blockhash(),
            );
            let failure = h
                .env
                .svm
                .send_transaction(tx)
                .expect_err("two missing domains cannot share one vacant slot");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
                )
            );
            assert_eq!(keys.map(|key| h.env.svm.get_account(&key)), frame);
            h.check();

            let asset = assets[0];
            let new_q = -direction * 7 * POS_SCALE as i128;
            let missing = 2 * asset as usize + usize::from(new_q > 0);
            assert_eq!(historical[missing], 0);
            assert!(historical[missing ^ 1] > 0);
            let ids = h.portfolios.map(|p| h.env.portfolio_id(p));
            h.trade(asset, new_q);
            assert_eq!(
                h.claims_for(0),
                historical,
                "admission preserves old claims and leaves the new domain latent"
            );
            assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), ids);
            h.settle_mark(PRICE, [order[1], order[0]]);
            let mut expected = historical;
            expected[missing] = 7 * BOUND_SCALE;
            assert_eq!(
                h.claims_for(0),
                expected,
                "only the absent opposite-side domain materializes"
            );
            assert_eq!(expected.iter().filter(|c| **c > 0).count(), CAPACITY);
            h.trade(asset, -new_q);
            assert_eq!(h.claims_for(0), expected);
            let historical_gain: u128 = (0..HISTORY).map(|asset| 1 + (asset % 3) as u128).sum();
            assert_eq!(
                h.payout(order),
                [CAPITAL + historical_gain + 7, CAPITAL - historical_gain - 7]
            );
            worlds += 1;
            calls += h.calls;
            for (max, cu) in maxima.iter_mut().zip(h.maxima) {
                *max = (*max).max(cu);
            }
        }
    }
    assert_eq!(worlds, 4);
    println!("INV-028 single vacant domain: worlds={worlds}, successful post-funding calls={calls}, max CU trade/mark-crank/convert/withdraw/close={maxima:?}");
}

#[test]
fn v16_program_batch_admission_cannot_share_last_future_domain_slot() {
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    let mut worlds = 0;
    let mut rejections = 0;
    let mut calls = 0;
    let mut maxima = [0; 5];
    let mut max_rejected_cu = 0;
    for direction in [-1i128, 1] {
        for constrained_is_b in [false, true] {
            for accepted in 0..2 {
                let mut h = SparseHistory::new();
                let order = if constrained_is_b { [1, 0] } else { [0, 1] };
                for asset in 0..HISTORY as u16 {
                    let q = direction * (1 + i128::from(asset % 3)) * POS_SCALE as i128;
                    h.trade(asset, q);
                    h.settle_mark((PRICE as i128 + direction) as u64, order);
                    h.trade(asset, -q);
                }
                let historical = h.claims_for(0);
                let occupied: BTreeSet<_> = historical
                    .iter()
                    .enumerate()
                    .filter_map(|(domain, claim)| (*claim > 0).then_some(domain))
                    .collect();
                assert_eq!(occupied.len(), CAPACITY - 1);
                assert!(h.position.is_none());
                let candidates = [
                    (0, -direction * 7 * POS_SCALE as i128),
                    ((HISTORY - 1) as u16, -direction * 11 * POS_SCALE as i128),
                ];
                let mut combined = occupied.clone();
                for &(asset, _) in &candidates {
                    let pair = [2 * asset as usize, 2 * asset as usize + 1];
                    let mut individually = occupied.clone();
                    individually.extend(pair);
                    assert_eq!(individually.len(), CAPACITY);
                    combined.extend(pair);
                }
                assert_eq!(combined.len(), CAPACITY + 1);
                assert!(
                    candidates.len()
                        < percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize
                );

                let send_batch = |h: &mut SparseHistory, legs: &[(u16, i128)]| {
                    let [a, b] = if constrained_is_b { [1, 0] } else { [0, 1] };
                    let instruction = h.env.batch_trade_no_cpi_ix(
                        h.portfolios[a],
                        h.portfolios[b],
                        legs.iter()
                            .map(|&(asset_index, quantity)| BatchTradeLeg {
                                asset_index,
                                market_id: h.env.asset_market_id(asset_index),
                                size_q: quantity * if a == 0 { 1 } else { -1 },
                                exec_price: h.prices[asset_index as usize],
                                fee_bps: 0,
                            })
                            .collect(),
                    );
                    h.env.svm.expire_blockhash();
                    let tx = Transaction::new_signed_with_payer(
                        &[
                            heap_ix(),
                            cu_ix(),
                            Instruction {
                                program_id: h.env.program_id,
                                accounts: vec![
                                    AccountMeta::new(h.owners[a].pubkey(), true),
                                    AccountMeta::new(h.owners[b].pubkey(), true),
                                    AccountMeta::new(h.env.market, false),
                                    AccountMeta::new(h.portfolios[a], false),
                                    AccountMeta::new(h.portfolios[b], false),
                                ],
                                data: instruction.encode(),
                            },
                        ],
                        Some(&h.env.payer.pubkey()),
                        &[&h.env.payer, &h.owners[a], &h.owners[b]],
                        h.env.svm.latest_blockhash(),
                    );
                    tx.verify().expect("public batch signatures");
                    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                    h.env.svm.send_transaction(tx)
                };

                let keys = [
                    h.env.market,
                    h.portfolios[0],
                    h.portfolios[1],
                    h.env.vault,
                    h.env.mint,
                    h.tokens[0],
                    h.tokens[1],
                    h.owners[0].pubkey(),
                    h.owners[1].pubkey(),
                    h.env.admin.pubkey(),
                ];
                for legs in [candidates, [candidates[1], candidates[0]]] {
                    let frame = keys.map(|key| h.env.svm.get_account(&key));
                    let failure = send_batch(&mut h, &legs)
                        .expect_err("distinct future domains cannot share the last vacant slot");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            2,
                            InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
                        )
                    );
                    let cu = failure.meta.compute_units_consumed;
                    assert_cu_within("joint future-domain admission rejection", cu, CU_LIMIT);
                    max_rejected_cu = max_rejected_cu.max(cu);
                    assert_eq!(keys.map(|key| h.env.svm.get_account(&key)), frame);
                    h.check();
                    rejections += 1;
                }

                // Each candidate gets its own public world: admission leaves its reserved
                // domain absent until favorable settlement, without reclaiming old claims.
                let (asset, q) = candidates[accepted];
                let missing = 2 * asset as usize + usize::from(q > 0);
                assert_eq!(historical[missing], 0);
                let ids = h.portfolios.map(|p| h.env.portfolio_id(p));
                let meta = send_batch(&mut h, &[(asset, q)])
                    .expect("either individual batch leg fits the remaining future-domain budget");
                h.position = Some((asset, q));
                h.observe_cu(0, meta.compute_units_consumed);
                h.check();
                assert_eq!(h.claims_for(0), historical);
                assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), ids);
                h.settle_mark(PRICE, [order[1], order[0]]);
                let gain = q.unsigned_abs() / POS_SCALE;
                let mut expected = historical;
                expected[missing] = gain * BOUND_SCALE;
                assert_eq!(h.claims_for(0), expected);
                assert_eq!(
                    expected.iter().filter(|claim| **claim > 0).count(),
                    CAPACITY
                );
                let meta = send_batch(&mut h, &[(asset, -q)])
                    .expect("full source occupancy preserves batch risk reduction");
                h.position = None;
                h.observe_cu(0, meta.compute_units_consumed);
                h.check();
                assert_eq!(h.claims_for(0), expected);
                let historical_gain: u128 = (0..HISTORY).map(|a| 1 + (a % 3) as u128).sum();
                assert_eq!(
                    h.payout(order),
                    [
                        CAPITAL + historical_gain + gain,
                        CAPITAL - historical_gain - gain
                    ]
                );
                worlds += 1;
                calls += h.calls;
                for (max, cu) in maxima.iter_mut().zip(h.maxima) {
                    *max = (*max).max(cu);
                }
            }
        }
    }
    assert_eq!((worlds, rejections), (8, 16));
    println!("INV-028 joint batch admission: worlds={worlds}, rejections={rejections}, successful post-funding calls={calls}, max rejected CU={max_rejected_cu}, max CU trade/mark-crank/convert/withdraw/close={maxima:?}");
}
