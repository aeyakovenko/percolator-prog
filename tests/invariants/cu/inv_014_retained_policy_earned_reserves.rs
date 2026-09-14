//! INV-014 with bounded INV-005/010/011/024/036/047/070/081 evidence.
//! Retained close/policy envelopes cross a funded insurance-authority return
//! while incumbent provider earnings and both users' terminal claims persist.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

#[path = "inv_014_generated_policy_reserve_routes.rs"]
mod generated_policy_reserve_routes;

const SHARE: u16 = 2_500;
const SHARED: u64 = EARNINGS * SHARE as u64 / 10_000;
const PROVIDER: u64 = EARNINGS - SHARED;
const PAID_PREFIX: u64 = 17;
const CAP: u64 = 37;
const CLOSE_FEE: u64 = (1_050 * 105 * CAP).div_ceil(10_000);
const LIMIT: u32 = 700_000;

struct History {
    world: TerminalEarningsWorld,
    users: [Keypair; 2],
    matcher: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
    ledger: Pubkey,
    token_frames: [solana_sdk::account::Account; 5],
    vault_frame: solana_sdk::account::Account,
    nonce: u32,
    peak: u64,
}

impl History {
    fn new() -> Self {
        let (mut world, users) = terminal_earnings_world_with_fee_share(false, None, SHARE);
        let matcher = Pubkey::new_unique();
        world.env.svm.add_program(
            matcher,
            &std::fs::read(auth_matcher_program_path()).unwrap(),
        );
        let (context, delegate, _) = world.env.init_auth_matcher_context_via_system_create(
            matcher,
            &users[1],
            world.portfolios[1],
        );
        world.env.set_matcher_config_with_trade_fee_cap(
            matcher,
            &users[1],
            world.portfolios[1],
            context,
            delegate,
            1,
            137,
        );
        let ledger = Keypair::new();
        system_create_account_for_test(
            &mut world.env.svm,
            &world.env.payer,
            &ledger,
            state::backing_domain_ledger_account_len(),
            world.env.program_id,
        );
        Self {
            token_frames: world
                .tokens
                .map(|key| world.env.svm.get_account(&key).unwrap()),
            vault_frame: world.env.svm.get_account(&world.env.vault).unwrap(),
            world,
            users,
            matcher,
            context,
            delegate,
            ledger: ledger.pubkey(),
            nonce: 0,
            peak: 0,
        }
    }

    fn sign(&mut self, ixs: &[Instruction]) -> Transaction {
        self.nonce += 1;
        let mut signers = vec![&self.world.env.payer];
        for signer in self.users.iter().chain([
            &self.world.admin,
            &self.world.incumbent,
            &self.world.successor,
        ]) {
            if ixs
                .iter()
                .flat_map(|ix| &ix.accounts)
                .any(|a| a.is_signer && a.pubkey == signer.pubkey())
            {
                signers.push(signer);
            }
        }
        let instructions = [
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT - self.nonce),
        ]
        .into_iter()
        .chain(ixs.iter().cloned())
        .collect::<Vec<_>>();
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.world.env.payer.pubkey()),
            &signers,
            self.world.env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        tx
    }

    fn frame(&self, tx: &Transaction) -> Vec<(Pubkey, Option<solana_sdk::account::Account>)> {
        let w = &self.world;
        let mut keys = tx.message.account_keys.clone();
        keys.extend([
            w.env.market,
            w.env.vault,
            w.env.mint,
            self.context,
            self.delegate,
            self.ledger,
        ]);
        keys.extend(w.wallets);
        keys.extend(w.tokens);
        keys.extend(w.portfolios);
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter()
            .map(|key| (key, w.env.svm.get_account(&key)))
            .collect()
    }

    fn preview(&mut self, tx: &Transaction) {
        let before = self.frame(tx);
        let result = self
            .world
            .env
            .svm
            .simulate_transaction(tx.clone().into())
            .unwrap();
        self.peak = self.peak.max(result.compute_units_consumed);
        assert_eq!(self.frame(tx), before);
    }

    fn deliver(
        &mut self,
        tx: Transaction,
        error: Option<(u8, PercolatorError)>,
        successes: [usize; 3],
    ) {
        let before = self.frame(&tx);
        let writable = tx
            .message
            .account_keys
            .iter()
            .enumerate()
            .filter(|(i, _)| tx.message.is_writable(*i))
            .map(|(_, key)| *key)
            .collect::<Vec<_>>();
        let fee = FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let result = self.world.env.svm.send_transaction(tx);
        let rejected = error.is_some();
        let meta = if let Some((index, error)) = error {
            let failure = result.expect_err("retained policy/fee or terminal role boundary");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
                "{failure:?}"
            );
            failure.meta
        } else {
            result.expect("bounded retained consent or attributed payout")
        };
        for (key, mut old) in before {
            if key == self.world.env.payer.pubkey() {
                old.as_mut().unwrap().lamports -= fee;
            }
            if rejected || !writable.contains(&key) || key == self.world.env.payer.pubkey() {
                assert_eq!(
                    self.world.env.svm.get_account(&key),
                    old,
                    "complete Account {key}"
                );
            }
        }
        for (program, count) in [self.world.env.program_id, spl_token::ID, self.matcher]
            .into_iter()
            .zip(successes)
        {
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                count,
                "{meta:?}"
            );
        }
        assert!(meta.compute_units_consumed > 0 && meta.compute_units_consumed <= u64::from(LIMIT));
        self.peak = self.peak.max(meta.compute_units_consumed);
    }

    fn call(&mut self, ixs: &[Instruction], successes: [usize; 3]) {
        let tx = self.sign(ixs);
        self.deliver(tx, None, successes);
    }

    fn policy(&self, holder: usize, bps: u64, sequence: u64, epoch: u64) -> Instruction {
        wrap(
            &self.world.env,
            ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: bps,
                policy_sequence: sequence,
                authority_epoch: epoch,
            },
            vec![
                AccountMeta::new(self.world.wallets[holder], true),
                AccountMeta::new(self.world.env.market, false),
            ],
        )
    }

    fn close(&self, cpi: bool) -> Instruction {
        let w = &self.world;
        let [a, b] = w.portfolios;
        let qty = -1_050 * POS_SCALE as i128;
        let ix = if cpi {
            w.env.trade_cpi_ix(a, b, 0, qty, CAP, 105)
        } else {
            w.env.trade_no_cpi_ix(a, b, 0, qty, 105, CAP)
        };
        let mut accounts = vec![AccountMeta::new(w.wallets[0], true)];
        if !cpi {
            accounts.push(AccountMeta::new(w.wallets[1], true));
        }
        accounts.extend([
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
        ]);
        if cpi {
            accounts.extend([
                AccountMeta::new_readonly(self.matcher, false),
                AccountMeta::new(self.context, false),
                AccountMeta::new_readonly(self.delegate, false),
            ]);
        }
        wrap(&w.env, ix, accounts)
    }

    fn check(
        &self,
        paid: [u64; 5],
        closed: bool,
        principal_paid: u64,
        fees_paid: u64,
        insurance_paid: u64,
    ) {
        let w = &self.world;
        let (_, group) = w.env.market_state();
        let custody = SUPPLY - paid.iter().sum::<u64>();
        assert_eq!(group.vault, u128::from(custody));
        assert_eq!(w.env.token_amount(w.env.vault), custody);
        assert_eq!(
            group.backing_provider_earnings_total,
            u128::from(PROVIDER - fees_paid)
        );
        assert_eq!(
            group.source_backing_buckets[1].utilization_fee_earnings,
            u128::from(PROVIDER - fees_paid)
        );
        assert_eq!(group.source_backing_buckets[0].utilization_fee_earnings, 0);
        let charge = if closed { CLOSE_FEE } else { 0 };
        let long = INSURANCE + charge;
        let short = SHARED + charge;
        assert_eq!(
            group.insurance_domain_budget,
            [
                long - insurance_paid.min(long),
                short - insurance_paid.saturating_sub(long)
            ]
            .map(u128::from)
        );
        assert_eq!(group.insurance, u128::from(long + short - insurance_paid));
        assert_eq!(group.insurance_domain_spent, [0; 2]);
        assert_eq!(
            w.env.svm.get_account(&w.env.mint),
            Some(w.mint_frame.clone())
        );
        for ((key, owner), amount) in w.tokens.into_iter().zip(w.wallets).zip(paid) {
            let raw = w.env.svm.get_account(&key).unwrap();
            let token = TokenAccount::unpack(&raw.data).unwrap();
            assert_eq!(raw.owner, spl_token::ID);
            assert_eq!(
                (token.owner, token.mint, token.amount),
                (owner, w.env.mint, amount)
            );
        }
        for ((key, frame), amount) in w
            .tokens
            .into_iter()
            .zip(&self.token_frames)
            .zip(paid)
            .chain(std::iter::once(((w.env.vault, &self.vault_frame), custody)))
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
                let equity = CAPITAL[i] as i128
                    + if i == 0 {
                        PROFIT as i128 - EARNINGS as i128
                    } else {
                        -(PROFIT as i128)
                    }
                    - charge as i128;
                assert_eq!(account.capital.get() as i128 + account.pnl.get(), equity);
                assert_eq!(has_active_leg_for_asset(account, 0), !closed);
            }
            assert_eq!(
                group.assets[0].oi_eff_long_q,
                if closed { 0 } else { 1_050 * POS_SCALE }
            );
            assert_eq!(
                group.assets[0].oi_eff_short_q,
                group.assets[0].oi_eff_long_q
            );
        }
        if accounts.is_empty() {
            assert_eq!(
                group.source_backing_buckets[1].fresh_unliened_backing_num,
                u128::from(BACKING - principal_paid) * BOUND_SCALE
            );
            assert_eq!(group.c_tot, 0);
            assert_eq!(group.pnl_pos_tot, 0);
        }
        let raw = w.env.svm.get_account(&w.env.market).unwrap();
        if closed {
            let profile = state::read_asset_oracle_profile(&raw.data, 0).unwrap();
            assert_eq!(profile.backing_bucket_authority, w.wallets[2].to_bytes());
            assert_eq!(profile.insurance_operator, w.wallets[3].to_bytes());
            assert_eq!(profile.insurance_authority, w.wallets[4].to_bytes());
            assert_eq!(profile.oracle_authority, w.wallets[4].to_bytes());
            assert_eq!(profile.backing_trade_fee_bps_short, RATE);
            assert_eq!(profile.backing_trade_fee_insurance_share_bps_short, SHARE);
        }
        assert_market_stock_census(
            "retained policy earned reserves",
            &group,
            &raw.data,
            &accounts,
            custody.into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("retained policy earned reserves", &group, &accounts)
            .unwrap();
    }
}

#[test]
fn v16_retained_close_policy_return_preserves_earned_reserves_through_terminal_payout() {
    assert_eq!((SHARED, PROVIDER, CLOSE_FEE), (218, 657, 408));
    let mut outcomes = Vec::new();
    let mut peak = 0;
    for cpi in [false, true] {
        for renew_policy in [false, true] {
            let mut h = History::new();
            let initial = h.world.env.control_sequences(0);
            let mut controls = initial;
            let mut paid = [0; 5];
            h.check(paid, false, 0, 0, 0);
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
            paid[2] = PAID_PREFIX;
            h.check(paid, false, 0, PAID_PREFIX, 0);
            let ledger_prefix = h.world.env.svm.get_account(&h.ledger);
            controls.trade_fee += 1;
            h.call(
                &[h.policy(4, CAP, controls.trade_fee, controls.authority_epoch)],
                [1, 0, 0],
            );
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
            let portfolios = h
                .world
                .portfolios
                .map(|key| h.world.env.svm.get_account(&key));
            let positions = h
                .world
                .portfolios
                .map(|key| h.world.env.portfolio_position_epoch(key));
            let close = h.close(cpi);
            let gap = controls.trade_fee + 7;
            let policy = h.policy(4, CAP, gap, controls.authority_epoch);
            let prefix = h.sign(&[policy.clone(), close.clone()]);
            let suffix = h.sign(&[close.clone(), policy]);
            let retained = h.sign(&[close.clone()]);
            let denied_close = h.sign(&[close.clone()]);
            assert_eq!(
                retained.message.instructions[2].data,
                denied_close.message.instructions[2].data
            );
            let wires = [&prefix, &suffix, &retained, &denied_close]
                .map(|tx| bincode::serialize(tx).unwrap());
            for tx in [&prefix, &suffix, &retained, &denied_close] {
                h.preview(tx);
            }

            for (from, to) in [(4, 3), (3, 4)] {
                let economy = h.world.env.market_state().1;
                h.call(
                    &[rotate(
                        &h.world,
                        INSURER,
                        from,
                        to,
                        controls.authority_epoch,
                    )],
                    [1, 0, 0],
                );
                controls.authority_epoch += 1;
                let mut expected = profile;
                expected.insurance_authority = h.world.wallets[to].to_bytes();
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
                    expected
                );
                assert_eq!(h.world.env.market_state().1, economy);
                if to == 3 {
                    controls.trade_fee += 1;
                    h.call(
                        &[h.policy(to, CAP + 4, controls.trade_fee, controls.authority_epoch)],
                        [1, 0, 0],
                    );
                }
                assert_eq!(h.world.env.control_sequences(0), controls);
                assert_eq!(
                    h.world
                        .portfolios
                        .map(|key| h.world.env.svm.get_account(&key)),
                    portfolios
                );
                h.check(paid, false, 0, PAID_PREFIX, 0);
            }
            assert!(gap > controls.trade_fee);
            assert_eq!(
                h.world.env.market_state().0.marketauth,
                h.world.admin.pubkey().to_bytes()
            );
            h.deliver(
                prefix.clone(),
                Some((2, PercolatorError::EngineStale)),
                [0, 0, 0],
            );
            h.check(paid, false, 0, PAID_PREFIX, 0);
            h.deliver(
                denied_close.clone(),
                Some((2, PercolatorError::InvalidInstruction)),
                [0, 0, 0],
            );
            h.check(paid, false, 0, PAID_PREFIX, 0);

            // The only policy renewal changes its epoch; no trade bound is renewed.
            let mut renewed = prefix.clone();
            renewed.message.instructions[2].data =
                h.policy(4, CAP, gap, controls.authority_epoch).data;
            let mut signers = vec![&h.world.env.payer, &h.world.admin, &h.users[0]];
            if !cpi {
                signers.push(&h.users[1]);
            }
            renewed.sign(&signers, renewed.message.recent_blockhash);
            renewed.verify().unwrap();
            h.preview(&renewed);
            controls.trade_fee += 1;
            h.call(
                &[h.policy(4, CAP, controls.trade_fee, controls.authority_epoch)],
                [1, 0, 0],
            );
            h.deliver(
                suffix.clone(),
                Some((3, PercolatorError::EngineStale)),
                [1, 0, usize::from(cpi)],
            );
            h.check(paid, false, 0, PAID_PREFIX, 0);
            assert_eq!(
                h.world
                    .portfolios
                    .map(|key| h.world.env.svm.get_account(&key)),
                portfolios
            );
            assert_eq!(h.world.env.svm.get_account(&h.ledger), ledger_prefix);
            for (tx, wire) in [&prefix, &suffix, &retained, &denied_close]
                .into_iter()
                .zip(wires)
            {
                assert_eq!(bincode::serialize(tx).unwrap(), wire);
            }
            let accepted = if renew_policy {
                controls.trade_fee = gap;
                renewed
            } else {
                retained
            };
            h.deliver(
                accepted,
                None,
                [1 + usize::from(renew_policy), 0, usize::from(cpi)],
            );
            h.check(paid, true, 0, PAID_PREFIX, 0);
            assert_eq!(h.world.env.control_sequences(0), controls);
            for (key, epoch) in h.world.portfolios.into_iter().zip(positions) {
                assert_eq!(h.world.env.portfolio_position_epoch(key), epoch + 1);
            }
            assert_eq!(h.world.env.svm.get_account(&h.ledger), ledger_prefix);

            h.peak = h.peak.max(h.world.env.resolve());
            h.world.env.svm.warp_to_slot(7);
            for actor in [1, 0] {
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
                paid[actor] = PAYOUTS[actor] - CLOSE_FEE;
                assert!(resolved_portfolio_is_terminal(
                    &h.world.env,
                    h.world.portfolios[actor]
                ));
                h.check(paid, true, 0, PAID_PREFIX, 0);
                h.peak = h.peak.max(
                    h.world
                        .env
                        .close_portfolio_with_cu(&h.users[actor], h.world.portfolios[actor]),
                );
                h.check(paid, true, 0, PAID_PREFIX, 0);
            }
            let mut fee_tail = payout(
                &h.world,
                FEES,
                2,
                PROVIDER - PAID_PREFIX,
                controls.authority_epoch,
                h.ledger,
            );
            fee_tail.accounts[0].is_signer = false;
            let mut wrong = payout(&h.world, INSURER, 3, 1, controls.authority_epoch, h.ledger);
            wrong.accounts[0].is_signer = false;
            let failed = h.sign(&[fee_tail.clone(), wrong]);
            h.deliver(failed, Some((3, PercolatorError::Unauthorized)), [1, 1, 0]);
            h.check(paid, true, 0, PAID_PREFIX, 0);
            h.call(&[fee_tail], [1, 1, 0]);
            paid[2] = PROVIDER;
            h.check(paid, true, 0, PROVIDER, 0);
            let insurance = INSURANCE + SHARED + 2 * CLOSE_FEE;
            for (role, actor, amount) in [(INSURER, 4, insurance), (PRINCIPAL, 2, BACKING)] {
                let mut ix = payout(
                    &h.world,
                    role,
                    actor,
                    amount,
                    controls.authority_epoch,
                    h.ledger,
                );
                ix.accounts[0].is_signer = false;
                h.call(&[ix], [1, 1, 0]);
                paid[actor] += amount;
                h.check(
                    paid,
                    true,
                    if role == PRINCIPAL { BACKING } else { 0 },
                    PROVIDER,
                    insurance,
                );
            }
            assert_eq!(paid, [56_219, 1_994_592, 100_657, 0, 1_065]);
            assert_eq!(paid.iter().sum::<u64>(), SUPPLY);
            let ledger = state::read_backing_domain_ledger(
                &h.world.env.svm.get_account(&h.ledger).unwrap().data,
            )
            .unwrap();
            assert_eq!(ledger.authority, h.world.incumbent.pubkey().to_bytes());
            assert_eq!(ledger.total_earnings_withdrawn_atoms, u128::from(PROVIDER));
            assert_eq!(ledger.last_observed_bucket_earnings_atoms, 0);
            let token_frame = h.world.tokens.map(|key| h.world.env.svm.get_account(&key));
            let ledger_frame = h.world.env.svm.get_account(&h.ledger);
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
            let close_slab = wrap(
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
            h.call(&[close_slab], [1, 1, 0]);
            let slab = h.world.env.svm.get_account(&h.world.env.market).unwrap();
            assert_closed_market_tombstone(&slab);
            assert_eq!(slab.lamports, rent);
            assert_eq!(
                h.world.env.svm.get_account(&h.world.admin.pubkey()),
                Some(admin)
            );
            assert_eq!(
                h.world.tokens.map(|key| h.world.env.svm.get_account(&key)),
                token_frame
            );
            assert_eq!(h.world.env.svm.get_account(&h.ledger), ledger_frame);
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
            peak = peak.max(h.peak);
            outcomes.push(paid);
        }
    }
    assert_eq!(outcomes.len(), 4);
    assert!(outcomes.windows(2).all(|pair| pair[0] == pair[1]));
    eprintln!("INV-014 earned reserves: 4 worlds, 20 simulations, 16 exact rollbacks, 2 retained-envelope + 2 epoch-renewed-policy closes, 8 owner payouts, 12 terminal reserve payouts, 4 slab closures; peak CU={peak}");
}
