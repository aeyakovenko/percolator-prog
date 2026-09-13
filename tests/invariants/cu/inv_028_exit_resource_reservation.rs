//! Row 423: many historical liens must coexist with future settlement domains.
//! Public funding/trades build the frontier. Provider withdrawal, latent settlement,
//! owner-only reduction and permissionless cleanup must preserve complete payouts.
//! This finite family uses 18 of the supported 28 domains; it is not maximum-shape closure.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const LIVE: usize = 8;
const USED_DOMAINS: usize = 2 * (LIVE + 1);
const RETAINED_CAPITAL: u128 = 0;
const PROVIDER_PER_DOMAIN: u128 = 400;
const PROVIDER_TOTAL: u128 = 2 * LIVE as u128 * PROVIDER_PER_DOMAIN;
const UNITS: i128 = 30;
const TOTAL_GAIN: u128 = 3_060;
const CU_LIMIT: u64 = 1_400_000;

struct ReservedExit {
    h: History,
    provider: Pubkey,
    provider_remaining: [u128; DOMAINS],
    mint: Account,
    calls: usize,
    rejected: usize,
    max_cu: u64,
    max_packet: usize,
}

impl ReservedExit {
    fn instruction(&self, data: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
        Instruction {
            program_id: self.h.env.program_id,
            accounts,
            data: data.encode(),
        }
    }

    fn trade(&self, quantity: i128, price: u64, batch: bool) -> Instruction {
        let h = &self.h;
        let data = if batch {
            h.env.batch_trade_no_cpi_ix(
                h.portfolios[0],
                h.portfolios[1],
                vec![BatchTradeLeg {
                    asset_index: LIVE as u16,
                    market_id: h.env.asset_market_id(LIVE as u16),
                    size_q: quantity,
                    exec_price: price,
                    fee_bps: 0,
                }],
            )
        } else {
            h.env.trade_no_cpi_ix(
                h.portfolios[0],
                h.portfolios[1],
                LIVE as u16,
                quantity,
                price,
                0,
            )
        };
        self.instruction(
            data,
            vec![
                AccountMeta::new(h.owners[0].pubkey(), true),
                AccountMeta::new(h.owners[1].pubkey(), true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(h.portfolios[0], false),
                AccountMeta::new(h.portfolios[1], false),
            ],
        )
    }

    fn withdraw(&self, actor: usize, amount: u128) -> Instruction {
        let h = &self.h;
        self.instruction(
            h.env.withdraw_ix(h.portfolios[actor], amount),
            vec![
                AccountMeta::new(h.owners[actor].pubkey(), true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(h.portfolios[actor], false),
                AccountMeta::new(h.tokens[actor], false),
                AccountMeta::new(h.env.vault, false),
                AccountMeta::new_readonly(h.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        )
    }

    fn crank(&self, actor: usize) -> Instruction {
        self.instruction(
            ProgInstruction::PermissionlessCrank {
                now_slot: self.h.slot,
                observations: crank_observations(LIVE as u16),
            },
            vec![
                AccountMeta::new(self.h.env.payer.pubkey(), true),
                AccountMeta::new(self.h.env.market, false),
                AccountMeta::new(self.h.portfolios[actor], false),
            ],
        )
    }

    #[track_caller]
    fn submit(
        &mut self,
        instructions: &[Instruction],
        actors: &[usize],
        rejection: Option<(u8, PercolatorError)>,
    ) {
        let h = &mut self.h;
        h.env.svm.expire_blockhash();
        let mut signers = vec![&h.env.payer];
        signers.extend(actors.iter().map(|&a| &h.owners[a]));
        if instructions.iter().any(|ix| {
            ix.accounts
                .iter()
                .any(|meta| meta.pubkey == h.env.admin.pubkey() && meta.is_signer)
        }) {
            signers.push(&h.env.admin);
        }
        let tx = Transaction::new_signed_with_payer(
            &[
                &[
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
                ][..],
                instructions,
            ]
            .concat(),
            Some(&h.env.payer.pubkey()),
            &signers,
            h.env.svm.latest_blockhash(),
        );
        let bytes = bincode::serialize(&tx).unwrap().len();
        assert!(bytes <= 1232);
        self.max_packet = self.max_packet.max(bytes);
        let mut keys = tx.message.account_keys.clone();
        keys.extend([
            h.env.market,
            h.env.mint,
            h.env.vault,
            h.portfolios[0],
            h.portfolios[1],
            h.tokens[0],
            h.tokens[1],
            self.provider,
            h.matcher.1,
            h.env.admin.pubkey(),
            h.owners[0].pubkey(),
            h.owners[1].pubkey(),
        ]);
        keys.sort_unstable();
        keys.dedup();
        keys.retain(|key| *key != h.env.payer.pubkey());
        let before: Vec<_> = keys.iter().map(|key| h.env.svm.get_account(key)).collect();
        let mut payer = h.env.svm.get_account(&h.env.payer.pubkey()).unwrap();
        payer.lamports -=
            tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
        let result = h.env.svm.send_transaction(tx);
        let cu = if let Some((index, error)) = rejection {
            let failure = result.expect_err("unfunded exit-resource request must reject");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
                "{failure:?}"
            );
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", h.env.program_id))
                    .count(),
                usize::from(index - 2),
                "the reservation-producing prefix must execute"
            );
            for (key, account) in keys.iter().zip(before) {
                assert_eq!(
                    h.env.svm.get_account(key),
                    account,
                    "complete Account rollback {key}"
                );
            }
            self.rejected += 1;
            failure.meta.compute_units_consumed
        } else {
            result
                .expect("admitted risk retains a public continuation")
                .compute_units_consumed
        };
        assert_eq!(h.env.svm.get_account(&h.env.payer.pubkey()).unwrap(), payer);
        assert_cu_within("historical lien/future exit resources", cu, CU_LIMIT);
        self.max_cu = self.max_cu.max(cu);
        self.calls += 1;
        self.audit();
    }

    fn audit(&self) {
        let h = &self.h;
        let group = h.env.market_state().1;
        let market = h.env.svm.get_account(&h.env.market).unwrap();
        let accounts: Vec<_> = h
            .portfolios
            .iter()
            .filter(|p| {
                h.env.svm.get_account(p).is_some_and(|a| {
                    a.owner == h.env.program_id && a.data.len() == h.env.portfolio_account_len
                })
            })
            .map(|p| h.env.portfolio_state(*p))
            .collect();
        assert_market_stock_census(
            "reserved exit",
            &group,
            &market.data,
            &accounts,
            h.env.token_amount(h.env.vault) as u128,
        )
        .unwrap();
        assert_reservation_encumbrance_census("reserved exit", &group, &accounts).unwrap();
        assert_source_credit_rates("reserved exit", &group).unwrap();
        assert_eq!(group.insurance, 0);
        assert_eq!(h.env.svm.get_account(&h.env.mint).unwrap(), self.mint);
        assert_eq!(
            group.vault
                + [h.tokens[0], h.tokens[1], self.provider]
                    .iter()
                    .map(|t| h.env.token_amount(*t) as u128)
                    .sum::<u128>(),
            2 * CAPITAL + PROVIDER_TOTAL
        );
        for account in &accounts {
            let mut resources = BTreeSet::new();
            for source in account.source_domains.iter().filter(|s| s.is_occupied()) {
                assert!(resources.insert(source.domain.get()));
                assert_eq!(source.source_lien_insurance_backing_num.get(), 0);
            }
            for leg in account.legs.iter().filter(|leg| leg.active != 0) {
                resources.extend([
                    2 * u32::from(leg.asset_index.get()),
                    2 * u32::from(leg.asset_index.get()) + 1,
                ]);
            }
            assert!(
                resources.len() <= USED_DOMAINS,
                "historical and prospective domains share capacity"
            );
        }
    }

    fn claims(&self) -> [u128; DOMAINS] {
        let mut claims = [0; DOMAINS];
        for source in self
            .h
            .env
            .portfolio_state(self.h.portfolios[0])
            .source_domains
            .iter()
            .filter(|s| s.is_occupied())
        {
            claims[source.domain.get() as usize] = source.source_claim_bound_num.get();
        }
        claims
    }

    fn lien(&self) -> u128 {
        self.h
            .env
            .portfolio_state(self.h.portfolios[0])
            .source_domains
            .iter()
            .map(|s| s.source_lien_counterparty_backing_num.get())
            .sum()
    }

    fn frontier(&self, quantity: i128) {
        assert_eq!(self.claims(), self.h.claims.map(|c| c * BOUND_SCALE));
        let group = self.h.env.market_state().1;
        for actor in 0..2 {
            let account = self.h.env.portfolio_state(self.h.portfolios[actor]);
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&account)),
                1
            );
            assert_eq!(
                active_leg_for_asset(&account, LIVE).basis_pos_q,
                quantity * if actor == 0 { 1 } else { -1 }
            );
            assert_eq!(
                account.capital.get(),
                if actor == 0 {
                    RETAINED_CAPITAL
                } else {
                    CAPITAL - self.h.claims.iter().sum::<u128>()
                }
            );
            let mut resources: BTreeSet<_> = account
                .source_domains
                .iter()
                .filter(|s| s.is_occupied())
                .map(|s| s.domain.get())
                .collect();
            resources.extend([2 * LIVE as u32, 2 * LIVE as u32 + 1]);
            assert_eq!(resources.len(), if actor == 0 { USED_DOMAINS } else { 2 });
        }
        for (asset, state) in group.assets.iter().enumerate() {
            let expected = if asset == LIVE {
                quantity.unsigned_abs()
            } else {
                0
            };
            assert_eq!(
                (state.oi_eff_long_q, state.oi_eff_short_q),
                (expected, expected)
            );
        }
        self.backing();
    }

    fn backing(&self) {
        let group = self.h.env.market_state().1;
        for domain in 0..DOMAINS {
            let bucket = group.source_backing_buckets[domain];
            assert_eq!(
                bucket.fresh_unliened_backing_num + bucket.valid_liened_backing_num,
                (self.h.claims[domain] + self.provider_remaining[domain]) * BOUND_SCALE,
                "every domain retains exactly its funded claim and unreturned provider atoms"
            );
        }
    }

    fn settle(&mut self, price: u64, direction: i128, order: [usize; 2]) {
        let domain = 2 * LIVE + usize::from(direction > 0);
        self.h.claims[domain] += UNITS as u128;
        self.h.slot += 1;
        self.h.env.svm.warp_to_slot(self.h.slot);
        let cu = self
            .h
            .env
            .push_auth_mark_for_asset_as_admin(LIVE as u16, self.h.slot, price);
        assert_cu_within("reserved exit mark", cu, CU_LIMIT);
        self.max_cu = self.max_cu.max(cu);
        self.calls += 1;
        self.audit();
        let retained = self
            .h
            .env
            .portfolio_state(self.h.portfolios[0])
            .source_domains;
        for actor in order {
            let target = self.h.claims.iter().sum::<u128>();
            let rank = |s: &Self| {
                let group = s.h.env.market_state().1;
                let account = s.h.env.portfolio_state(s.h.portfolios[actor]);
                u128::from(s.h.slot - group.assets[LIVE].slot_last)
                    + if actor == 0 {
                        account.pnl.get().abs_diff(target as i128)
                    } else {
                        account.capital.get().abs_diff(CAPITAL - target)
                    }
            };
            for _ in 0..4 {
                let before = rank(self);
                if before == 0 {
                    break;
                }
                let peer = self.h.env.svm.get_account(&self.h.portfolios[1 - actor]);
                self.submit(&[self.crank(actor)], &[], None);
                assert!(
                    rank(self) < before,
                    "bounded settlement consumes economic/accrual debt"
                );
                assert_eq!(
                    self.h.env.svm.get_account(&self.h.portfolios[1 - actor]),
                    peer
                );
                if actor == 0 {
                    assert_eq!(self.claims(), self.h.claims.map(|v| v * BOUND_SCALE));
                }
            }
            assert_eq!(rank(self), 0);
        }
        let after = self.h.env.portfolio_state(self.h.portfolios[0]);
        for source in retained
            .iter()
            .filter(|s| s.is_occupied() && s.domain.get() < (2 * LIVE) as u32)
        {
            assert_eq!(
                after
                    .source_domains
                    .iter()
                    .find(|s| s.is_occupied() && s.domain == source.domain),
                Some(source),
                "latent settlement preserves each historical reservation"
            );
        }
        assert_eq!(self.claims(), self.h.claims.map(|c| c * BOUND_SCALE));
        assert_eq!(after.capital.get(), RETAINED_CAPITAL);
    }

    fn provider_withdraw(&self, domain: usize, amount: u128) -> Instruction {
        let h = &self.h;
        self.instruction(
            ProgInstruction::WithdrawBackingBucket {
                domain: domain as u16,
                market_id: h.env.asset_market_id(domain as u16 / 2),
                authority_epoch: h.env.control_sequences(domain / 2).authority_epoch,
                amount,
            },
            vec![
                AccountMeta::new(h.env.admin.pubkey(), true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(self.provider, false),
                AccountMeta::new(h.env.vault, false),
                AccountMeta::new_readonly(h.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        )
    }

    fn return_provider(&mut self, final_release: bool) {
        let portfolios = self.h.portfolios.map(|p| self.h.env.svm.get_account(&p));
        let lien = self.lien();
        let mut checked_boundary = false;
        for domain in 0..2 * LIVE {
            let reserved =
                self.h.env.market_state().1.source_backing_buckets[domain].valid_liened_backing_num;
            assert_eq!(reserved % BOUND_SCALE, 0);
            let amount = self.provider_remaining[domain] - reserved / BOUND_SCALE;
            if reserved > 0 && !checked_boundary {
                self.submit(
                    &[self.provider_withdraw(domain, amount + 1)],
                    &[],
                    Some((2, PercolatorError::EngineLockActive)),
                );
                checked_boundary = true;
            }
            if amount > 0 {
                self.submit(&[self.provider_withdraw(domain, amount)], &[], None);
                self.provider_remaining[domain] -= amount;
            }
            assert_eq!(
                self.lien(),
                lien,
                "provider withdrawal cannot consume admitted risk's backing"
            );
            assert_eq!(
                self.h.portfolios.map(|p| self.h.env.svm.get_account(&p)),
                portfolios
            );
        }
        assert_eq!(checked_boundary, !final_release);
        assert_eq!(
            self.provider_remaining.iter().sum::<u128>() * BOUND_SCALE,
            self.h.env.market_state().1.source_backing_buckets[..2 * LIVE]
                .iter()
                .map(|b| b.valid_liened_backing_num)
                .sum::<u128>()
        );
        assert_eq!(
            self.h.env.token_amount(self.provider) as u128,
            PROVIDER_TOTAL - self.provider_remaining.iter().sum::<u128>()
        );
        self.backing();
    }
}

#[test]
fn v16_program_historical_liens_preserve_future_domains_and_owner_exit() {
    let mut worlds = 0;
    let mut calls = 0;
    let mut rejections = 0;
    let mut max_cu = 0;
    let mut max_packet = 0;
    let mut max_cleanup_calls = 0;
    for direction in [-1i128, 1] {
        for batch in [false, true] {
            for withdraw_early in [false, true] {
                let order = if batch { [0, 1] } else { [1, 0] };
                let mut h = History::new();
                for asset in 0..LIVE as u16 {
                    let q = 100 * (1 + i128::from(asset % 3)) * POS_SCALE as i128;
                    let route = AccountResidualCounterTradePath::TradeNoCpi;
                    h.trade(route, &[(asset, q, PRICE)]);
                    h.mark_and_settle(&[(asset, PRICE + 1)], order);
                    h.trade(route, &[(asset, -2 * q, PRICE + 1)]);
                    h.mark_and_settle(&[(asset, PRICE)], order);
                    h.trade(route, &[(asset, q, PRICE)]);
                }
                let historical = h.claims;
                assert_eq!(
                    historical.iter().filter(|c| **c != 0).count(),
                    USED_DOMAINS - 2
                );
                assert_eq!(historical.iter().sum::<u128>(), 3_000);
                let provider = create_ata_for_test(
                    &mut h.env.svm,
                    &h.env.payer,
                    h.env.admin.pubkey(),
                    h.env.mint,
                );
                send_raw_tx(
                    &mut h.env.svm,
                    &h.env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &h.env.mint,
                        &provider,
                        &h.env.admin.pubkey(),
                        &[],
                        PROVIDER_TOTAL as u64,
                    )
                    .unwrap(),
                    &[&h.env.admin],
                )
                .unwrap();
                for domain in 0..2 * LIVE {
                    h.env.svm.expire_blockhash();
                    let expiry = h.env.market_state().1.source_backing_buckets[domain].expiry_slot;
                    assert!(expiry > h.slot);
                    h.env.top_up_backing_bucket_from_admin_token_with_cu(
                        provider,
                        domain as u16,
                        PROVIDER_PER_DOMAIN,
                        expiry,
                    );
                }
                let mint = h.env.svm.get_account(&h.env.mint).unwrap();
                assert_eq!(
                    Mint::unpack(&mint.data).unwrap().supply as u128,
                    2 * CAPITAL + PROVIDER_TOTAL
                );
                let mut s = ReservedExit {
                    h,
                    provider,
                    provider_remaining: std::array::from_fn(|d| {
                        if d < 2 * LIVE {
                            PROVIDER_PER_DOMAIN
                        } else {
                            0
                        }
                    }),
                    mint,
                    calls: 0,
                    rejected: 0,
                    max_cu: 0,
                    max_packet: 0,
                };
                s.submit(&[s.withdraw(0, CAPITAL - RETAINED_CAPITAL)], &[0], None);
                for actor in [0, 1] {
                    s.submit(&[s.crank(actor)], &[], None);
                }
                let q = direction * UNITS * POS_SCALE as i128;
                let admission = s.trade(q, PRICE, batch);
                // Active portfolios cannot withdraw. The late guard must restore the
                // successful admission's liens, positions and custody before exact retry.
                s.submit(
                    &[admission.clone(), s.withdraw(0, 1)],
                    &[0, 1],
                    Some((3, PercolatorError::EngineStale)),
                );
                s.submit(&[admission], &[0, 1], None);
                s.frontier(q);
                assert_eq!(s.claims(), historical.map(|c| c * BOUND_SCALE));
                assert_eq!(
                    s.lien(),
                    (UNITS as u128 * u128::from(PRICE) - RETAINED_CAPITAL) * BOUND_SCALE
                );
                let reserved_sources =
                    s.h.env
                        .portfolio_state(s.h.portfolios[0])
                        .source_domains
                        .iter()
                        .filter(|source| source.source_lien_counterparty_backing_num.get() > 0)
                        .count();
                assert_eq!(reserved_sources, USED_DOMAINS - 2);
                s.submit(
                    &[s.trade(q, PRICE, !batch)],
                    &[0, 1],
                    Some((2, PercolatorError::EngineLockActive)),
                );
                if withdraw_early {
                    s.return_provider(false);
                }
                let first_price = (PRICE as i128 + direction) as u64;
                s.settle(first_price, direction, order);
                s.frontier(q);
                assert_eq!(
                    s.claims().iter().filter(|c| **c != 0).count(),
                    USED_DOMAINS - 1
                );
                s.submit(&[s.trade(-2 * q, first_price, batch)], &[0, 1], None);
                s.frontier(-q);
                assert!(s.lien() > 0);
                s.settle(PRICE, -direction, [order[1], order[0]]);
                s.frontier(-q);
                assert!(s.claims()[..USED_DOMAINS].iter().all(|c| *c > 0));
                if !withdraw_early {
                    s.return_provider(false);
                }

                // Only the risk owner's signature is needed; the other owner stays absent
                // through reduction and public prior-epoch/obsolete-reservation cleanup.
                let peer = s.h.env.svm.get_account(&s.h.portfolios[1]);
                let reduce = s.instruction(
                    ProgInstruction::RebalanceReduce {
                        portfolio_id: s.h.env.portfolio_id(s.h.portfolios[0]),
                        position_epoch: s.h.env.portfolio_position_epoch(s.h.portfolios[0]),
                        asset_index: LIVE as u16,
                        reduce_q: UNITS as u128 * POS_SCALE,
                    },
                    vec![
                        AccountMeta::new(s.h.owners[0].pubkey(), true),
                        AccountMeta::new(s.h.env.market, false),
                        AccountMeta::new(s.h.portfolios[0], false),
                    ],
                );
                s.submit(&[reduce], &[0], None);
                assert_eq!(s.h.env.svm.get_account(&s.h.portfolios[1]), peer);
                let mut cleanup_calls = 0;
                for actor in [0, 1] {
                    let rank = |s: &ReservedExit| {
                        let a = s.h.env.portfolio_state(s.h.portfolios[actor]);
                        (
                            percolator::active_bitmap_count_ones(active_bitmap(&a)),
                            a.source_domains
                                .iter()
                                .map(|d| d.source_lien_counterparty_backing_num.get())
                                .sum::<u128>(),
                        )
                    };
                    for _ in 0..DOMAINS + 2 {
                        let before = rank(&s);
                        if before == (0, 0) {
                            break;
                        }
                        s.submit(&[s.crank(actor)], &[], None);
                        cleanup_calls += 1;
                        assert!(
                            rank(&s) < before,
                            "cleanup strictly reduces leg/reservation debt"
                        );
                    }
                    assert_eq!(rank(&s), (0, 0));
                }
                assert!(cleanup_calls <= USED_DOMAINS + 2);
                max_cleanup_calls = max_cleanup_calls.max(cleanup_calls);
                assert_eq!(s.claims(), s.h.claims.map(|c| c * BOUND_SCALE));
                assert_eq!(s.h.claims.iter().sum::<u128>(), TOTAL_GAIN);
                s.return_provider(true);
                let convert = s.instruction(
                    s.h.env
                        .convert_released_pnl_ix(s.h.portfolios[0], TOTAL_GAIN),
                    vec![
                        AccountMeta::new(s.h.owners[0].pubkey(), true),
                        AccountMeta::new(s.h.env.market, false),
                        AccountMeta::new(s.h.portfolios[0], false),
                    ],
                );
                s.submit(
                    &[convert.clone()],
                    &[0],
                    Some((2, PercolatorError::EngineStale)),
                );
                s.submit(&[s.crank(0)], &[], None);
                s.submit(&[convert], &[0], None);
                for actor in order {
                    let payout = if actor == 0 {
                        RETAINED_CAPITAL + TOTAL_GAIN
                    } else {
                        CAPITAL - TOTAL_GAIN
                    };
                    assert_eq!(
                        s.h.env.portfolio_state(s.h.portfolios[actor]).capital.get(),
                        payout
                    );
                    s.submit(&[s.withdraw(actor, payout)], &[actor], None);
                    let close = s.instruction(
                        s.h.env.close_portfolio_ix(s.h.portfolios[actor]),
                        vec![
                            AccountMeta::new(s.h.owners[actor].pubkey(), true),
                            AccountMeta::new(s.h.env.market, false),
                            AccountMeta::new(s.h.portfolios[actor], false),
                        ],
                    );
                    s.submit(&[close], &[actor], None);
                    assert!(s
                        .h
                        .env
                        .svm
                        .get_account(&s.h.portfolios[actor])
                        .is_none_or(|a| a.data.is_empty() && a.lamports == 0));
                }
                let reset_side = u8::from(direction < 0);
                let cu = s.h.env.finalize_reset_side_with_cu(LIVE as u16, reset_side);
                assert_cu_within("reserved exit side finalization", cu, CU_LIMIT);
                s.calls += 1;
                s.max_cu = s.max_cu.max(cu);
                s.audit();
                assert_eq!(
                    s.h.tokens.map(|t| s.h.env.token_amount(t) as u128),
                    [CAPITAL + TOTAL_GAIN, CAPITAL - TOTAL_GAIN]
                );
                let group = s.h.env.market_state().1;
                assert_eq!(
                    (
                        group.vault,
                        group.c_tot,
                        group.insurance,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0, 0)
                );
                assert_eq!(
                    (group.assets[LIVE].mode_long, group.assets[LIVE].mode_short),
                    (SideModeV16::Normal, SideModeV16::Normal)
                );
                for (domain, source) in group.source_credit.iter().enumerate() {
                    assert_eq!(
                        (
                            source.valid_liened_backing_num,
                            source.impaired_liened_backing_num,
                            source.fresh_reserved_backing_num,
                            source.positive_claim_bound_num
                        ),
                        (0, 0, 0, 0)
                    );
                    assert_eq!(
                        source.provider_receivable_num,
                        s.h.claims[domain] * BOUND_SCALE
                    );
                    assert_eq!(
                        group.source_backing_buckets[domain].consumed_liened_backing_num,
                        s.h.claims[domain] * BOUND_SCALE
                    );
                }
                worlds += 1;
                calls += s.calls;
                rejections += s.rejected;
                max_cu = max_cu.max(s.max_cu);
                max_packet = max_packet.max(s.max_packet);
            }
        }
    }
    assert_eq!((worlds, rejections), (8, 32));
    println!("INV-028 reserved exit: worlds={worlds}, suffix_calls={calls}, exact_rollbacks={rejections}, max_suffix_cu={max_cu}, max_packet={max_packet}, max_cleanup_calls={max_cleanup_calls}");
}
