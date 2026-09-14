//! Primary INV-014: retained reductions cross funded provider and fee-authority
//! returns, with route-normalized fees and earned owner/class terminal attribution.
//! Bounded INV-005/010/011/024/036/047/070/081; classic SPL, one exact-fill leg.

use super::*;
use crate::support::fuzz_model::TradeRoute;

const TEMPORARY_FEES: u64 = 11;

#[derive(Default)]
struct Book {
    paid: [u64; 5],
    fee: u64,
    closed: bool,
    principal: u64,
    earnings: u64,
    insurance: u64,
}

impl Book {
    fn check(&self, h: &History) {
        let w = &h.world;
        let group = w.env.market_state().1;
        let custody = SUPPLY - self.paid.iter().sum::<u64>();
        let long = INSURANCE + self.fee;
        let short = SHARED + self.fee;
        assert_eq!(group.vault, u128::from(custody));
        assert_eq!(group.insurance, u128::from(long + short - self.insurance));
        assert_eq!(
            group.insurance_domain_budget,
            [
                long - self.insurance.min(long),
                short - self.insurance.saturating_sub(long)
            ]
            .map(u128::from)
        );
        assert_eq!(group.insurance_domain_spent, [0; 2]);
        assert_eq!(
            group.backing_provider_earnings_total,
            u128::from(PROVIDER - self.earnings)
        );
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            u128::from(PROVIDER - self.earnings)
        );
        assert_eq!(group.source_backing_buckets[0].utilization_fee_earnings, 0);
        assert_eq!(
            w.env.svm.get_account(&w.env.mint),
            Some(w.mint_frame.clone())
        );
        for ((key, frame), amount) in w
            .tokens
            .into_iter()
            .zip(&h.token_frames)
            .zip(self.paid)
            .chain(std::iter::once(((w.env.vault, &h.vault_frame), custody)))
        {
            let mut expected = frame.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(w.env.svm.get_account(&key), Some(expected));
        }
        let accounts = w
            .portfolios
            .into_iter()
            .filter(|key| {
                w.env
                    .svm
                    .get_account(key)
                    .is_some_and(|a| !a.data.is_empty())
            })
            .map(|key| w.env.portfolio_state(key))
            .collect::<Vec<_>>();
        if group.mode == MarketModeV16::Live {
            for (i, account) in accounts.iter().enumerate() {
                assert_eq!(
                    account.capital.get() as i128 + account.pnl.get(),
                    i128::from(PAYOUTS[i] - self.fee)
                );
                assert_eq!(has_active_leg_for_asset(account, 0), !self.closed);
            }
            assert_eq!(
                group.assets[0].oi_eff_long_q,
                if self.closed { 0 } else { 1_050 * POS_SCALE }
            );
            assert_eq!(
                group.assets[0].oi_eff_short_q,
                group.assets[0].oi_eff_long_q
            );
        }
        if accounts.is_empty() {
            assert_eq!(
                group.source_backing_buckets[1].fresh_unliened_backing_num,
                u128::from(BACKING - self.principal) * BOUND_SCALE
            );
            assert_eq!((group.c_tot, group.pnl_pos_tot), (0, 0));
        }
        let raw = w.env.svm.get_account(&w.env.market).unwrap();
        assert_market_stock_census(
            "policy reserve routes",
            &group,
            &raw.data,
            &accounts,
            custody.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("policy reserve routes", &group, &accounts).unwrap();
    }
}

#[test]
fn v16_generated_retained_policy_routes_preserve_paid_earnings_across_funded_returns() {
    let routes = [TradeRoute::NoCpi, TradeRoute::Cpi];
    let mut outcomes = Vec::new();
    let mut peak = 0;
    let mut worlds = 0;
    for route in routes {
        for restored_bps in [19, CAP] {
            for (provider_first, terminal_provider_first) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
                let mut h = History::new();
                let temporary_ledger = Keypair::new();
                system_create_account_for_test(
                    &mut h.world.env.svm,
                    &h.world.env.payer,
                    &temporary_ledger,
                    state::backing_domain_ledger_account_len(),
                    h.world.env.program_id,
                );
                let mut book = Book::default();
                let mut controls = h.world.env.control_sequences(0);
                let profile = state::read_asset_oracle_profile(
                    &h.world
                        .env
                        .svm
                        .get_account(&h.world.env.market)
                        .unwrap()
                        .data,
                    0,
                )
                .unwrap();
                let mut expected_profile = profile;
                let epochs = h
                    .world
                    .portfolios
                    .map(|p| h.world.env.portfolio_position_epoch(p));
                book.check(&h);
                h.call(
                    &[payout(
                        &h.world,
                        FEES,
                        2,
                        PAID_PREFIX,
                        controls.authority_epoch,
                        h.ledger,
                    )],
                    [1, 1, 0],
                );
                book.paid[2] += PAID_PREFIX;
                book.earnings += PAID_PREFIX;
                book.check(&h);
                controls.trade_fee += 1;
                h.call(
                    &[h.policy(4, CAP, controls.trade_fee, controls.authority_epoch)],
                    [1, 0, 0],
                );
                let close = h.close(cpi);
                let gap = controls.trade_fee + 100;
                let old_policy = h.policy(4, CAP, gap, controls.authority_epoch);
                let prefix = h.sign(&[old_policy.clone(), close.clone()]);
                let suffix = h.sign(&[close.clone(), old_policy]);
                let denied = h.sign(&[close.clone()]);
                let retained = h.sign(&[close]);
                let wire = bincode::serialize(&retained).unwrap();
                for tx in [&prefix, &suffix, &denied, &retained] {
                    h.preview(tx);
                }
                let portfolios = h.world.portfolios.map(|p| h.world.env.svm.get_account(&p));
                let order = if provider_first {
                    [FEES, INSURER]
                } else {
                    [INSURER, FEES]
                };
                for role in order {
                    let from = if role == FEES { 2 } else { 4 };
                    let economy = h.world.env.market_state().1;
                    h.call(
                        &[rotate(&h.world, role, from, 3, controls.authority_epoch)],
                        [1, 0, 0],
                    );
                    controls.authority_epoch += 1;
                    if role == FEES {
                        expected_profile.backing_bucket_authority = h.world.wallets[3].to_bytes();
                    } else {
                        expected_profile.insurance_authority = h.world.wallets[3].to_bytes();
                    }
                    assert_eq!(h.world.env.market_state().1, economy);
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &h.world
                                .env
                                .svm
                                .get_account(&h.world.env.market)
                                .unwrap()
                                .data,
                            0
                        )
                        .unwrap(),
                        expected_profile
                    );
                    book.check(&h);
                }
                // The coholder receives only its consented earned-fee slice; principal stays encumbered.
                h.call(
                    &[payout(
                        &h.world,
                        FEES,
                        3,
                        TEMPORARY_FEES,
                        controls.authority_epoch,
                        temporary_ledger.pubkey(),
                    )],
                    [1, 1, 0],
                );
                book.paid[3] = TEMPORARY_FEES;
                book.earnings += TEMPORARY_FEES;
                book.check(&h);
                let temporary_frame = h.world.env.svm.get_account(&temporary_ledger.pubkey());
                controls.trade_fee += 1;
                h.call(
                    &[h.policy(3, CAP + 4, controls.trade_fee, controls.authority_epoch)],
                    [1, 0, 0],
                );
                for role in order.into_iter().rev() {
                    let to = if role == FEES { 2 } else { 4 };
                    let economy = h.world.env.market_state().1;
                    h.call(
                        &[rotate(&h.world, role, 3, to, controls.authority_epoch)],
                        [1, 0, 0],
                    );
                    controls.authority_epoch += 1;
                    assert_eq!(h.world.env.market_state().1, economy);
                    book.check(&h);
                }
                assert_eq!(
                    state::read_asset_oracle_profile(
                        &h.world
                            .env
                            .svm
                            .get_account(&h.world.env.market)
                            .unwrap()
                            .data,
                        0
                    )
                    .unwrap(),
                    profile
                );
                assert_eq!(h.world.env.control_sequences(0), controls);
                assert!(gap > controls.trade_fee);
                h.deliver(prefix, Some((2, PercolatorError::EngineStale)), [0, 0, 0]);
                book.check(&h);
                h.deliver(
                    denied,
                    Some((2, PercolatorError::InvalidInstruction)),
                    [0, 0, 0],
                );
                book.check(&h);
                controls.trade_fee += 1;
                h.call(
                    &[h.policy(
                        4,
                        restored_bps,
                        controls.trade_fee,
                        controls.authority_epoch,
                    )],
                    [1, 0, 0],
                );
                h.deliver(
                    suffix,
                    Some((3, PercolatorError::EngineStale)),
                    [1, 0, usize::from(cpi)],
                );
                book.check(&h);
                assert_eq!(
                    h.world.portfolios.map(|p| h.world.env.svm.get_account(&p)),
                    portfolios
                );
                assert_eq!(bincode::serialize(&retained).unwrap(), wire);
                h.deliver(retained, None, [1, 0, usize::from(cpi)]);
                book.fee = (1_050 * 105 * if cpi { restored_bps } else { CAP }).div_ceil(10_000);
                book.closed = true;
                book.check(&h);
                assert_eq!(h.world.env.control_sequences(0), controls);
                for (key, epoch) in h.world.portfolios.into_iter().zip(epochs) {
                    assert_eq!(h.world.env.portfolio_position_epoch(key), epoch + 1);
                }
                h.peak = h.peak.max(h.world.env.resolve());
                h.world.env.svm.warp_to_slot(7);
                for actor in if provider_first { [0, 1] } else { [1, 0] } {
                    let ix = wrap(
                        &h.world.env,
                        ProgInstruction::CloseResolved {
                            fee_rate_per_slot: 0,
                        },
                        vec![
                            AccountMeta::new_readonly(h.world.wallets[actor], false),
                            AccountMeta::new(h.world.env.market, false),
                            AccountMeta::new(h.world.portfolios[actor], false),
                            AccountMeta::new(h.world.tokens[actor], false),
                            AccountMeta::new(h.world.env.vault, false),
                            AccountMeta::new_readonly(h.world.env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    );
                    h.call(&[ix], [1, 1, 0]);
                    book.paid[actor] = PAYOUTS[actor] - book.fee;
                    book.check(&h);
                    assert!(resolved_portfolio_is_terminal(
                        &h.world.env,
                        h.world.portfolios[actor]
                    ));
                    h.peak = h.peak.max(
                        h.world
                            .env
                            .close_portfolio_with_cu(&h.users[actor], h.world.portfolios[actor]),
                    );
                    book.check(&h);
                }
                let insurance = INSURANCE + SHARED + 2 * book.fee;
                let mut tail = payout(
                    &h.world,
                    FEES,
                    2,
                    PROVIDER - book.earnings,
                    controls.authority_epoch,
                    h.ledger,
                );
                tail.accounts[0].is_signer = false;
                let mut former =
                    payout(&h.world, INSURER, 3, 1, controls.authority_epoch, h.ledger);
                former.accounts[0].is_signer = false;
                let tx = h.sign(&[tail.clone(), former]);
                h.deliver(tx, Some((3, PercolatorError::ExpectedSigner)), [1, 1, 0]);
                book.check(&h);
                h.call(&[tail], [1, 1, 0]);
                book.paid[2] += PROVIDER - book.earnings;
                book.earnings = PROVIDER;
                book.check(&h);
                let payouts = if terminal_provider_first {
                    [(PRINCIPAL, 2, BACKING), (INSURER, 4, insurance)]
                } else {
                    [(INSURER, 4, insurance), (PRINCIPAL, 2, BACKING)]
                };
                for (role, actor, amount) in payouts {
                    let mut ix = payout(
                        &h.world,
                        role,
                        actor,
                        amount,
                        controls.authority_epoch,
                        h.ledger,
                    );
                    if role != INSURER {
                        ix.accounts[0].is_signer = false;
                    }
                    h.call(&[ix], [1, 1, 0]);
                    book.paid[actor] += amount;
                    if role == PRINCIPAL {
                        book.principal = amount;
                    } else {
                        book.insurance = amount;
                    }
                    book.check(&h);
                }
                let expected = [
                    PAYOUTS[0] - book.fee,
                    PAYOUTS[1] - book.fee,
                    BACKING + PROVIDER - TEMPORARY_FEES,
                    TEMPORARY_FEES,
                    insurance,
                ];
                let attributed = |observed: [u64; 5]| observed == expected;
                assert!(attributed(
                    h.world.tokens.map(|key| h.world.env.token_amount(key))
                ));
                let mut wrong_owner = book.paid;
                wrong_owner[2] -= 1;
                wrong_owner[3] += 1;
                assert_eq!(wrong_owner.iter().sum::<u64>(), SUPPLY);
                assert!(!attributed(wrong_owner));
                assert_eq!(
                    h.world.env.svm.get_account(&temporary_ledger.pubkey()),
                    temporary_frame
                );
                let tokens = h.world.tokens.map(|p| h.world.env.svm.get_account(&p));
                let ledgers =
                    [h.ledger, temporary_ledger.pubkey()].map(|p| h.world.env.svm.get_account(&p));
                for ((raw, actor), paid) in ledgers
                    .iter()
                    .zip([2, 3])
                    .zip([PROVIDER - TEMPORARY_FEES, TEMPORARY_FEES])
                {
                    let ledger =
                        state::read_backing_domain_ledger(&raw.as_ref().unwrap().data).unwrap();
                    assert_eq!(ledger.authority, h.world.wallets[actor].to_bytes());
                    assert_eq!(ledger.total_earnings_withdrawn_atoms, u128::from(paid));
                }
                let rent = h
                    .world
                    .env
                    .svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                let mut admin = h
                    .world
                    .env
                    .svm
                    .get_account(&h.world.admin.pubkey())
                    .unwrap();
                admin.lamports += h
                    .world
                    .env
                    .svm
                    .get_account(&h.world.env.market)
                    .unwrap()
                    .lamports
                    + h.world
                        .env
                        .svm
                        .get_account(&h.world.env.vault)
                        .unwrap()
                        .lamports
                    - rent;
                let ix = wrap(
                    &h.world.env,
                    ProgInstruction::CloseSlab {
                        authority_epoch: controls.authority_epoch,
                    },
                    vec![
                        AccountMeta::new(h.world.admin.pubkey(), true),
                        AccountMeta::new(h.world.env.market, false),
                        AccountMeta::new(h.world.env.vault, false),
                        AccountMeta::new_readonly(h.world.env.vault_authority, false),
                        AccountMeta::new(h.world.tokens[4], false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(h.world.env.mint, false),
                    ],
                );
                h.call(&[ix], [1, 1, 0]);
                let slab = h.world.env.svm.get_account(&h.world.env.market).unwrap();
                assert_closed_market_tombstone(&slab);
                assert_eq!(slab.lamports, rent);
                assert_eq!(
                    h.world.env.svm.get_account(&h.world.admin.pubkey()),
                    Some(admin)
                );
                assert_eq!(
                    h.world.tokens.map(|p| h.world.env.svm.get_account(&p)),
                    tokens
                );
                assert_eq!(
                    [h.ledger, temporary_ledger.pubkey()].map(|p| h.world.env.svm.get_account(&p)),
                    ledgers
                );
                assert_eq!(
                    h.world.env.svm.get_account(&h.world.env.mint),
                    Some(h.world.mint_frame.clone())
                );
                assert!(h
                    .world
                    .env
                    .svm
                    .get_account(&h.world.env.vault)
                    .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
                // Remove disclosed transport fees before comparing owner entitlements.
                let mut normalized = book.paid;
                normalized[0] += book.fee;
                normalized[1] += book.fee;
                normalized[4] -= 2 * book.fee;
                outcomes.push(normalized);
                peak = peak.max(h.peak);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    assert!(outcomes.windows(2).all(|p| p[0] == p[1]));
    eprintln!("INV-014 generated policy reserves: {worlds} worlds, 64 simulations, 64 exact rollbacks, 64 funded handoffs, 32 owner payouts, 48 terminal reserve payouts, 16 slab closures; peak CU={peak}");
}
