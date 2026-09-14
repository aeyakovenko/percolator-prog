//! Primary INV-027: retained first-risk consent after elapsed flat fees and a
//! funded fee-authority return preserves post-liability senior claims on four routes.
//! Bounded INV-005/010/011/014/024/036/044/047/053/055/060/062/081 evidence.
//! Explicit sync or implicit withdrawal collects flat fees before admission;
//! bare admission with deferred fees, junior support, lag and funding are excluded.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::fee::FeeStructure;

const CAP: u64 = 37;
const Q: i128 = 5 * POS_SCALE as i128;
const PART: u128 = 7;
const LIMIT: u32 = 700_000;

fn fee(route: TradeRoute, bps: u64) -> u128 {
    let charged = if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
        bps
    } else {
        CAP
    };
    (500 * u128::from(charged)).div_ceil(10_000)
}

struct World {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    matcher: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
    funds: [u128; 2],
    frames: [solana_sdk::account::Account; 3],
    mint: solana_sdk::account::Account,
    nonce: u32,
    peak: u64,
}

#[derive(Default)]
struct Book {
    maintenance: [u128; 2],
    trading: u128,
    paid: [u128; 2],
    position: i128,
}

impl World {
    fn new(open_fee: u128, policy: u64) -> Self {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                maintenance_fee_per_slot: FEE_RATE,
                trade_fee_base_bps: policy,
                initial_margin_bps: 10_000,
                maintenance_margin_bps: 5_000,
                max_price_move_bps_per_slot: 500,
                max_abs_funding_e9_per_slot: 0,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(START);
        env.configure_auth_mark_for_asset_as_admin(0, START, PRICE);
        let owners = [Keypair::new(), Keypair::new()];
        let portfolios = owners
            .each_ref()
            .map(|owner| public_portfolio(&mut env, owner));
        let funds = [500 + open_fee + FEE + PART, 573 + open_fee + FEE];
        let tokens =
            std::array::from_fn(|i| public_deposit(&mut env, &owners[i], portfolios[i], funds[i]));
        let (matcher, context, delegate) =
            auth_matcher_for_lp_via_system_create(&mut env, &owners[1], portfolios[1]);
        env.set_matcher_config_with_trade_fee_cap(
            matcher,
            &owners[1],
            portfolios[1],
            context,
            delegate,
            1,
            137,
        );
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        let frames = [tokens[0], tokens[1], env.vault].map(|p| env.svm.get_account(&p).unwrap());
        let mint = env.svm.get_account(&env.mint).unwrap();
        Self {
            env,
            owners,
            portfolios,
            tokens,
            matcher,
            context,
            delegate,
            funds,
            frames,
            mint,
            nonce: 0,
            peak: 0,
        }
    }

    fn ix(&self, data: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            data: data.encode(),
            accounts,
        }
    }

    fn sync(&self, owner: usize) -> Instruction {
        self.ix(
            ProgInstruction::SyncMaintenanceFee {
                now_slot: ADMISSION,
            },
            vec![
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[owner], false),
            ],
        )
    }

    fn withdraw(&self, owner: usize, amount: u128) -> Instruction {
        self.ix(
            self.env.withdraw_ix(self.portfolios[owner], amount),
            vec![
                AccountMeta::new(self.owners[owner].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[owner], false),
                AccountMeta::new(self.tokens[owner], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        )
    }

    fn trade(&self, route: TradeRoute, q: i128) -> Instruction {
        let [a, b] = self.portfolios;
        let data = match route {
            TradeRoute::NoCpi => self.env.trade_no_cpi_ix(a, b, 0, q, PRICE, CAP),
            TradeRoute::Cpi => self.env.trade_cpi_ix(a, b, 0, q, CAP, PRICE),
            TradeRoute::BatchNoCpi => self.env.batch_trade_no_cpi_ix(
                a,
                b,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: self.env.asset_market_id(0),
                    size_q: q,
                    exec_price: PRICE,
                    fee_bps: CAP,
                }],
            ),
            TradeRoute::BatchCpi => self.env.batch_trade_cpi_ix_with_caps(
                a,
                b,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: self.env.asset_market_id(0),
                    size_q: q,
                    limit_price: PRICE,
                    fee_bps: CAP,
                }],
                0,
                ((q.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE) * u128::from(CAP))
                    .div_ceil(10_000),
            ),
        };
        let mut accounts = vec![AccountMeta::new(self.owners[0].pubkey(), true)];
        let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
        if !cpi {
            accounts.push(AccountMeta::new(self.owners[1].pubkey(), true));
        }
        accounts.extend([
            AccountMeta::new(self.env.market, false),
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
        self.ix(data, accounts)
    }

    fn policy(&self, who: Pubkey, bps: u64, sequence: u64, epoch: u64) -> Instruction {
        self.ix(
            ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: bps,
                policy_sequence: sequence,
                authority_epoch: epoch,
            },
            vec![
                AccountMeta::new(who, true),
                AccountMeta::new(self.env.market, false),
            ],
        )
    }

    fn handoff(&self, from: Pubkey, to: Pubkey, epoch: u64) -> Instruction {
        self.ix(
            ProgInstruction::UpdateAssetAuthority {
                asset_index: 0,
                market_id: self.env.asset_market_id(0),
                authority_epoch: epoch,
                kind: processor::ASSET_AUTH_INSURANCE,
                new_pubkey: to.to_bytes(),
            },
            vec![
                AccountMeta::new(from, true),
                AccountMeta::new_readonly(to, true),
                AccountMeta::new(self.env.market, false),
            ],
        )
    }

    fn sign(&mut self, instructions: &[Instruction]) -> Transaction {
        self.nonce += 1;
        let mut signers = vec![&self.env.payer];
        for signer in [&self.env.admin, &self.owners[0], &self.owners[1]] {
            if instructions
                .iter()
                .flat_map(|ix| &ix.accounts)
                .any(|a| a.is_signer && a.pubkey == signer.pubkey())
            {
                signers.push(signer);
            }
        }
        let ixs = [
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT - self.nonce),
        ]
        .into_iter()
        .chain(instructions.iter().cloned())
        .collect::<Vec<_>>();
        let tx = Transaction::new_signed_with_payer(
            &ixs,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        tx
    }

    fn frame(&self, tx: &Transaction) -> Vec<(Pubkey, Option<solana_sdk::account::Account>)> {
        let mut keys = tx.message.account_keys.clone();
        keys.extend([
            self.env.market,
            self.env.vault,
            self.env.mint,
            self.env.admin.pubkey(),
            self.context,
            self.matcher,
            self.delegate,
            self.env.vault_authority,
            self.owners[0].pubkey(),
            self.owners[1].pubkey(),
        ]);
        keys.extend(self.portfolios);
        keys.extend(self.tokens);
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter()
            .map(|key| (key, self.env.svm.get_account(&key)))
            .collect()
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
            .map(|(_, p)| *p)
            .collect::<Vec<_>>();
        let fee = FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let rejected = error.is_some();
        let result = self.env.svm.send_transaction(tx);
        let meta = if let Some((index, e)) = error {
            let failure = result.expect_err("retained policy or post-maintenance margin boundary");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(e as u32)),
                "{failure:?}"
            );
            failure.meta
        } else {
            result.expect("consented post-liability progress")
        };
        for (key, mut old) in before {
            if key == self.env.payer.pubkey() {
                old.as_mut().unwrap().lamports -= fee;
            }
            if rejected || !writable.contains(&key) || key == self.env.payer.pubkey() {
                assert_eq!(
                    self.env.svm.get_account(&key),
                    old,
                    "complete Account {key}"
                );
            }
        }
        for (program, count) in [self.env.program_id, spl_token::ID, self.matcher]
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

    fn check(&self, book: &Book, current: bool) {
        let group = self.env.market_state().1;
        let fees = book.maintenance.iter().sum::<u128>() + 2 * book.trading;
        let custody = self.funds.iter().sum::<u128>() - book.paid.iter().sum::<u128>();
        assert_eq!(
            (group.mode, group.assets[0].lifecycle),
            (MarketModeV16::Live, AssetLifecycleV16::Active)
        );
        assert_eq!(
            (group.insurance, group.vault, group.c_tot),
            (fees, custody, custody - fees)
        );
        assert_eq!(
            (
                group.pnl_pos_tot,
                group.source_claim_bound_total_num,
                group.backing_provider_earnings_total
            ),
            (0, 0, 0)
        );
        let long = book.maintenance.iter().map(|fee| fee / 2).sum::<u128>() + book.trading;
        assert_eq!(&group.insurance_domain_budget[..2], &[long, fees - long]);
        assert!(group.insurance_domain_budget[2..].iter().all(|v| *v == 0));
        assert_eq!(group.assets[0].oi_eff_long_q, book.position.unsigned_abs());
        assert_eq!(group.assets[0].oi_eff_short_q, book.position.unsigned_abs());
        assert_eq!(
            self.env.svm.get_account(&self.env.mint),
            Some(self.mint.clone())
        );
        for ((key, frame), amount) in [self.tokens[0], self.tokens[1], self.env.vault]
            .into_iter()
            .zip(&self.frames)
            .zip([book.paid[0], book.paid[1], custody])
        {
            let mut expected = frame.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = u64::try_from(amount).unwrap();
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(self.env.svm.get_account(&key), Some(expected));
        }
        let accounts = self.portfolios.map(|p| self.env.portfolio_state(p));
        for (i, account) in accounts.iter().enumerate() {
            let capital = self.funds[i] - book.maintenance[i] - book.trading - book.paid[i];
            assert_eq!(account.capital.get(), capital);
            assert_eq!((account.pnl.get(), account.fee_credits.get()), (0, 0));
            assert_eq!(
                account.last_fee_slot.get(),
                if book.maintenance[i] == 0 {
                    START
                } else {
                    ADMISSION
                }
            );
            assert_eq!(account.owner, self.owners[i].pubkey().to_bytes());
            assert_eq!(has_active_leg_for_asset(account, 0), book.position != 0);
            if book.position != 0 {
                assert_eq!(
                    active_leg_for_asset(account, 0).basis_pos_q,
                    if i == 0 {
                        book.position
                    } else {
                        -book.position
                    }
                );
            }
            let certified = assert_current_certificate_matches_independent(
                "retained first admission",
                &group,
                account,
            )
            .unwrap();
            if current {
                assert!(certified);
                let cert = health_cert(account);
                assert_eq!(cert.certified_equity, capital as i128);
                assert_eq!(
                    cert.certified_initial_req,
                    if book.position == 0 { 0 } else { 500 }
                );
                assert_eq!(
                    cert.certified_maintenance_req,
                    if book.position == 0 { 0 } else { 250 }
                );
                assert_eq!(cert.certified_worst_case_loss, cert.certified_initial_req);
                assert_eq!(cert.certified_liq_deficit, 0);
            }
        }
        assert_market_stock_census(
            "retained first admission",
            &group,
            &self.env.svm.get_account(&self.env.market).unwrap().data,
            &accounts,
            custody,
        )
        .unwrap();
        assert_reservation_encumbrance_census("retained first admission", &group, &accounts)
            .unwrap();
    }
}

#[test]
fn v16_generated_retained_first_admission_survives_funded_policy_return_and_fee_collection() {
    let routes = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];
    let mut worlds = 0;
    let mut peak = 0;
    let mut outcomes = Vec::new();
    for (index, route) in routes.into_iter().enumerate() {
        for restored_bps in [19, CAP] {
            for explicit in [false, true] {
                for temporary in [0, 1] {
                    let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
                    let mut w = World::new(fee(route, restored_bps), restored_bps);
                    let mut book = Book::default();
                    let admin = w.env.admin.pubkey();
                    let holder = w.owners[temporary].pubkey();
                    let profiles = state::read_asset_oracle_profile(
                        &w.env.svm.get_account(&w.env.market).unwrap().data,
                        0,
                    )
                    .unwrap();
                    w.check(&book, false);
                    let flat = w.portfolios.map(|p| w.env.svm.get_account(&p));
                    w.env.svm.warp_to_slot(ADMISSION);
                    w.env
                        .configure_auth_mark_for_asset_as_admin(0, ADMISSION, PRICE);
                    assert_eq!(w.portfolios.map(|p| w.env.svm.get_account(&p)), flat);
                    w.call(&[w.sync(1)], [1, 0, 0]);
                    book.maintenance[1] = FEE;
                    w.check(&book, false);
                    if explicit {
                        w.call(&[w.sync(0)], [1, 0, 0]);
                        book.maintenance[0] = FEE;
                        w.check(&book, false);
                    }
                    let mut controls = w.env.control_sequences(0);
                    let gap = controls.trade_fee + 100;
                    let bundle = [w.withdraw(0, PART), w.trade(route, Q)];
                    let old_policy = w.policy(admin, restored_bps, gap, controls.authority_epoch);
                    let prefix =
                        w.sign(&[old_policy.clone(), bundle[0].clone(), bundle[1].clone()]);
                    let suffix = w.sign(&[bundle[0].clone(), bundle[1].clone(), old_policy]);
                    let denied = w.sign(&bundle);
                    let retained = w.sign(&bundle);
                    let wire = bincode::serialize(&retained).unwrap();
                    let before = w.frame(&retained);
                    let preview = w
                        .env
                        .svm
                        .simulate_transaction(retained.clone().into())
                        .unwrap();
                    w.peak = w.peak.max(preview.compute_units_consumed);
                    assert_eq!(w.frame(&retained), before);
                    let accounts = w.portfolios.map(|p| w.env.svm.get_account(&p));
                    for (from, to) in [(admin, holder), (holder, admin)] {
                        let economy = w.env.market_state().1;
                        w.call(&[w.handoff(from, to, controls.authority_epoch)], [1, 0, 0]);
                        controls.authority_epoch += 1;
                        assert_eq!(w.env.market_state().1, economy);
                        assert_eq!(w.portfolios.map(|p| w.env.svm.get_account(&p)), accounts);
                        if to == holder {
                            controls.trade_fee += 1;
                            w.call(
                                &[w.policy(
                                    holder,
                                    CAP + 4,
                                    controls.trade_fee,
                                    controls.authority_epoch,
                                )],
                                [1, 0, 0],
                            );
                        }
                        w.check(&book, false);
                    }
                    let returned = state::read_asset_oracle_profile(
                        &w.env.svm.get_account(&w.env.market).unwrap().data,
                        0,
                    )
                    .unwrap();
                    assert_eq!(
                        (
                            returned.insurance_authority,
                            returned.insurance_operator,
                            returned.backing_bucket_authority,
                            returned.oracle_authority
                        ),
                        (
                            profiles.insurance_authority,
                            profiles.insurance_operator,
                            profiles.backing_bucket_authority,
                            profiles.oracle_authority
                        )
                    );
                    assert_eq!(w.env.control_sequences(0), controls);
                    assert!(gap > controls.trade_fee);
                    w.deliver(prefix, Some((2, PercolatorError::EngineStale)), [0, 0, 0]);
                    w.check(&book, false);
                    w.deliver(
                        denied,
                        Some((
                            3,
                            if matches!(route, TradeRoute::BatchCpi) {
                                PercolatorError::EngineInvalidConfig
                            } else {
                                PercolatorError::InvalidInstruction
                            },
                        )),
                        [1, 1, usize::from(matches!(route, TradeRoute::BatchCpi))],
                    );
                    w.check(&book, false);
                    controls.trade_fee += 1;
                    w.call(
                        &[w.policy(
                            admin,
                            restored_bps,
                            controls.trade_fee,
                            controls.authority_epoch,
                        )],
                        [1, 0, 0],
                    );
                    w.deliver(
                        suffix,
                        Some((4, PercolatorError::EngineStale)),
                        [2, 1, usize::from(cpi)],
                    );
                    w.check(&book, false);
                    let excessive = w.sign(&[bundle[0].clone(), w.trade(route, Q + 1)]);
                    w.deliver(
                        excessive,
                        Some((3, PercolatorError::EngineInvalidConfig)),
                        [1, 1, usize::from(cpi)],
                    );
                    w.check(&book, false);
                    assert_eq!(bincode::serialize(&retained).unwrap(), wire);
                    w.deliver(retained, None, [2, 1, usize::from(cpi)]);
                    book.maintenance[0] = FEE;
                    book.paid[0] = PART;
                    book.trading = fee(route, restored_bps);
                    book.position = Q;
                    w.check(&book, true);
                    assert_eq!(
                        health_cert(&w.env.portfolio_state(w.portfolios[0])).certified_equity,
                        500
                    );
                    let next = routes[3 - index];
                    if matches!(next, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                        w.env.set_matcher_config_with_trade_fee_cap(
                            w.matcher,
                            &w.owners[1],
                            w.portfolios[1],
                            w.context,
                            w.delegate,
                            1,
                            137,
                        );
                        w.check(&book, false);
                    }
                    w.call(
                        &[w.trade(next, -Q)],
                        [
                            1,
                            0,
                            usize::from(matches!(next, TradeRoute::Cpi | TradeRoute::BatchCpi)),
                        ],
                    );
                    book.trading += fee(next, restored_bps);
                    book.position = 0;
                    w.check(&book, true);
                    for owner in [temporary, 1 - temporary] {
                        let amount = w.funds[owner] - FEE - book.trading - book.paid[owner];
                        w.call(&[w.withdraw(owner, amount)], [1, 1, 0]);
                        book.paid[owner] += amount;
                        w.check(&book, false);
                    }
                    assert_eq!(w.env.market_state().1.c_tot, 0);
                    assert_eq!(w.env.control_sequences(0), controls);
                    outcomes.push([
                        book.paid[0] as i128 + book.trading as i128 - w.funds[0] as i128,
                        book.paid[1] as i128 + book.trading as i128 - w.funds[1] as i128,
                        (w.env.market_state().1.insurance - 2 * book.trading) as i128,
                    ]);
                    peak = peak.max(w.peak);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 32);
    assert!(outcomes
        .iter()
        .all(|p| *p == [-(FEE as i128), -(FEE as i128), 2 * FEE as i128]));
    eprintln!("INV-027 retained policy admission: {worlds} worlds, 32 simulations, 128 exact rollbacks, 64 funded handoffs, 32 exact first admissions, 64 owner exits; peak CU={peak}");
}
