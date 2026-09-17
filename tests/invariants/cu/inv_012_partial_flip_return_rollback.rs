//! Row 412 / INV-012/004: a synchronized partial reduction preserves the LP grant,
//! but consumes episode consent. Hostile cross-zero returns roll back that prefix;
//! retained current-episode consent remains usable, until same-tuple grant renewal.

use super::*;
use percolator_prog::matcher_abi::{read_matcher_return, FLAG_PARTIAL_OK, FLAG_VALID};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: u128 = 1_000_000;
const PRICE: u64 = 100;
const FEE_BPS: u64 = 100;
const EXPIRY: u64 = 100;

fn fee(quantity: i128) -> u128 {
    let notional = (quantity.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    (notional * u128::from(FEE_BPS)).div_ceil(10_000)
}

struct World {
    env: V16CuEnv,
    owners: [Keypair; 3],
    portfolios: [Pubkey; 3],
    tokens: [Pubkey; 3],
    matcher: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
    ids: [u64; 3],
    initial_epochs: [u64; 3],
    grant_sequence: u64,
    generation: u64,
    peer: Account,
    accepted_peak: u64,
    rejected_peak: u64,
    rejected: usize,
}

impl World {
    fn new() -> Self {
        let mut env = crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::
            inv018_public_spl_market_with_params(6, V16CuMarketParams {
                trade_fee_base_bps: FEE_BPS,
                ..V16CuMarketParams::default()
            });
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
        let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
        let keys = [Keypair::new(), Keypair::new(), Keypair::new()];
        let portfolios = keys.each_ref().map(Signer::pubkey);
        let mut tokens = [Pubkey::default(); 3];
        for actor in 0..3 {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(
                    &env.payer.pubkey(),
                    &owners[actor].pubkey(),
                    1_000_000_000,
                ),
                &[],
            )
            .unwrap();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &keys[actor],
                env.portfolio_account_len,
                env.program_id,
            );
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
        let matcher = Pubkey::new_unique();
        env.svm.add_program(
            matcher,
            &std::fs::read(hostile_matcher_program_path()).unwrap(),
        );
        let context_key = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &context_key,
            MATCHER_CONTEXT_LEN,
            matcher,
        );
        let context = context_key.pubkey();
        let delegate = matcher_delegate_key(
            &env.program_id,
            &env.market,
            &portfolios[1],
            &owners[1].pubkey(),
            &matcher,
            &context,
        );
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            Instruction {
                program_id: matcher,
                accounts: vec![
                    AccountMeta::new_readonly(owners[1].pubkey(), true),
                    AccountMeta::new(context, false),
                ],
                data: vec![10],
            },
            &[&owners[1]],
        )
        .unwrap();
        env.try_set_matcher_config_with_trade_fee_cap_and_expiry(
            matcher,
            &owners[1],
            portfolios[1],
            context,
            delegate,
            1,
            FEE_BPS as u16,
            EXPIRY,
        )
        .unwrap();
        Self {
            ids: portfolios.map(|key| env.portfolio_id(key)),
            initial_epochs: portfolios.map(|key| env.portfolio_position_epoch(key)),
            grant_sequence: env.portfolio_matcher_sequence(portfolios[1]),
            generation: env.asset_market_id(0),
            peer: env.svm.get_account(&portfolios[2]).unwrap(),
            env,
            owners,
            portfolios,
            tokens,
            matcher,
            context,
            delegate,
            accepted_peak: 0,
            rejected_peak: 0,
            rejected: 0,
        }
    }

    fn control(&self, mode: u8) -> Instruction {
        Instruction {
            program_id: self.matcher,
            accounts: vec![
                AccountMeta::new_readonly(self.owners[1].pubkey(), true),
                AccountMeta::new(self.context, false),
            ],
            data: vec![11, mode, 0],
        }
    }

    fn trade(&self, batch: bool, quantity: i128, future_epoch: u64) -> Instruction {
        let [taker, lp, _] = self.portfolios;
        let mut request = if batch {
            self.env.batch_trade_cpi_ix_with_caps(
                taker,
                lp,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: self.generation,
                    size_q: quantity,
                    fee_bps: FEE_BPS,
                    limit_price: PRICE,
                }],
                0,
                fee(quantity),
            )
        } else {
            self.env
                .trade_cpi_ix(taker, lp, 0, quantity, FEE_BPS, PRICE)
        };
        match &mut request {
            ProgInstruction::TradeCpi {
                account_a_position_epoch,
                account_b_position_epoch,
                ..
            }
            | ProgInstruction::BatchTradeCpi {
                account_a_position_epoch,
                account_b_position_epoch,
                ..
            } => {
                *account_a_position_epoch += future_epoch;
                *account_b_position_epoch += future_epoch;
            }
            _ => unreachable!(),
        }
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.owners[0].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
                AccountMeta::new_readonly(self.matcher, false),
                AccountMeta::new(self.context, false),
                AccountMeta::new_readonly(self.delegate, false),
            ],
            data: request.encode(),
        }
    }

    fn sign(&self, instructions: &[Instruction]) -> Transaction {
        let mut ixs = vec![heap_ix(), cu_ix()];
        ixs.extend_from_slice(instructions);
        let mut signers = vec![&self.env.payer];
        for owner in &self.owners {
            if instructions
                .iter()
                .flat_map(|ix| &ix.accounts)
                .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
            {
                signers.push(owner);
            }
        }
        let tx = Transaction::new_signed_with_payer(
            &ixs,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        tx.verify().unwrap();
        assert!(
            bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64
        );
        tx
    }

    fn snapshot(&self, tx: &Transaction) -> Vec<(Pubkey, Option<Account>)> {
        let mut keys = tx.message.account_keys.clone();
        keys.extend(self.portfolios);
        keys.extend(self.tokens);
        keys.extend(self.owners.each_ref().map(Signer::pubkey));
        keys.extend([
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.vault_authority,
            self.env.admin.pubkey(),
            self.matcher,
            self.context,
            self.delegate,
            self.env.program_id,
            spl_token::ID,
            associated_token_program_id(),
            solana_sdk::system_program::ID,
            solana_sdk::sysvar::clock::ID,
        ]);
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter()
            .map(|key| (key, self.env.svm.get_account(&key)))
            .collect()
    }

    fn calls(&self, meta: &litesvm::types::TransactionMetadata, cpis: usize, commits: usize) {
        for (log, count) in [
            (format!("Program {} invoke [2]", self.matcher), cpis),
            (format!("Program {} success", self.env.program_id), commits),
        ] {
            assert_eq!(
                meta.logs.iter().filter(|line| **line == log).count(),
                count,
                "{meta:?}"
            );
        }
    }

    fn land(
        &mut self,
        tx: Transaction,
        changed: &[Pubkey],
        failure: Option<(u8, InstructionError, usize)>,
    ) -> litesvm::types::TransactionMetadata {
        tx.verify().unwrap();
        let mut before = self.snapshot(&tx);
        let network_fee = FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        before
            .iter_mut()
            .find(|(key, _)| *key == self.env.payer.pubkey())
            .unwrap()
            .1
            .as_mut()
            .unwrap()
            .lamports -= network_fee;
        let result = self.env.svm.send_transaction(tx);
        let meta = if let Some((index, error, calls)) = failure {
            let failed = result.expect_err("retained or hostile suffix must reject");
            assert_eq!(
                failed.err,
                TransactionError::InstructionError(index, error),
                "{failed:?}"
            );
            self.calls(&failed.meta, calls, usize::from(index == 4));
            for (key, account) in before {
                assert_eq!(
                    self.env.svm.get_account(&key),
                    account,
                    "exact rollback {key}"
                );
            }
            self.rejected += 1;
            self.rejected_peak = self.rejected_peak.max(failed.meta.compute_units_consumed);
            failed.meta
        } else {
            let meta = result.expect("current consent must remain live");
            for (key, account) in before {
                if !changed.contains(&key) {
                    assert_eq!(
                        self.env.svm.get_account(&key),
                        account,
                        "unchanged account {key}"
                    );
                }
            }
            self.accepted_peak = self.accepted_peak.max(meta.compute_units_consumed);
            meta
        };
        assert_cu_within(
            "INV-012 partial/flip composition",
            meta.compute_units_consumed,
            1_200_000,
        );
        assert_eq!(
            self.env.svm.get_account(&self.portfolios[2]).unwrap(),
            self.peer
        );
        meta
    }

    fn economics(&self, quantity: i128, fees: u128, fills: u64, renewals: u64) {
        for actor in 0..3 {
            let key = self.portfolios[actor];
            let portfolio = self.env.portfolio_state(key);
            let expected = if actor == 2 {
                0
            } else if actor == 0 {
                quantity
            } else {
                -quantity
            };
            assert_eq!(
                portfolio.capital.get(),
                CAPITAL - if actor < 2 { fees } else { 0 }
            );
            assert_eq!(portfolio.pnl.get(), 0);
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&portfolio)),
                u32::from(expected != 0)
            );
            if expected != 0 {
                assert_eq!(active_leg_for_asset(&portfolio, 0).basis_pos_q, expected);
            }
            assert_eq!(self.env.portfolio_id(key), self.ids[actor]);
            assert_eq!(
                self.env.portfolio_position_epoch(key),
                self.initial_epochs[actor] + if actor < 2 { fills } else { 0 }
            );
            assert_eq!(self.env.token_amount(self.tokens[actor]), 0);
        }
        let (config, market) = self.env.market_state();
        assert_eq!(config.matcher_req_seq, fills);
        assert_eq!(market.assets[0].market_id, self.generation);
        assert_eq!(
            [
                market.assets[0].oi_eff_long_q,
                market.assets[0].oi_eff_short_q
            ],
            [quantity.unsigned_abs(); 2]
        );
        assert_eq!(market.insurance, 2 * fees);
        assert_eq!(&market.insurance_domain_budget[..2], &[fees; 2]);
        assert_eq!(market.c_tot, 3 * CAPITAL - 2 * fees);
        assert_eq!(market.vault, 3 * CAPITAL);
        assert_eq!(market.c_tot + market.insurance, market.vault);
        assert_eq!(
            u128::from(self.env.token_amount(self.env.vault)),
            market.vault
        );
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), 3 * CAPITAL);
        assert_eq!(mint.mint_authority, COption::None);
        let grant = self.env.portfolio_matcher_config(self.portfolios[1]);
        assert_eq!(grant.enabled(), 1);
        assert_eq!(grant.matcher_program, self.matcher.to_bytes());
        assert_eq!(grant.matcher_context, self.context.to_bytes());
        assert_eq!(grant.matcher_delegate, self.delegate.to_bytes());
        assert_eq!(grant.trade_fee_cap_bps(), FEE_BPS as u16);
        assert_eq!(
            self.env.portfolio_matcher_expiry(self.portfolios[1]),
            EXPIRY
        );
        assert_eq!(
            self.env.portfolio_matcher_sequence(self.portfolios[1]),
            self.grant_sequence + renewals
        );
        assert_eq!(
            self.delegate,
            matcher_delegate_key(
                &self.env.program_id,
                &self.env.market,
                &self.portfolios[1],
                &self.owners[1].pubkey(),
                &self.matcher,
                &self.context
            )
        );
    }

    fn response(
        &self,
        meta: &litesvm::types::TransactionMetadata,
        batch: bool,
        request: i128,
        fill: i128,
        ordinal: u64,
        partial: bool,
    ) {
        let context = self.env.svm.get_account(&self.context).unwrap();
        let bytes = if batch {
            assert_eq!(meta.return_data.program_id, self.matcher);
            assert_eq!(meta.return_data.data.len(), 64);
            &meta.return_data.data
        } else {
            &context.data
        };
        let ret = read_matcher_return(bytes).unwrap();
        assert_eq!(ret.exec_size, fill);
        assert_eq!(ret.exec_size.signum(), request.signum());
        assert!(ret.exec_size.unsigned_abs() <= request.unsigned_abs());
        assert_eq!(ret.exec_price_e6, PRICE);
        assert_eq!(ret.oracle_price_e6, PRICE);
        assert_eq!(ret.asset_index, 0);
        assert_eq!(
            ret.lp_account_id,
            u64::from_le_bytes(self.delegate.to_bytes()[..8].try_into().unwrap())
        );
        assert_eq!(ret.req_id, ordinal);
        assert_eq!(
            ret.flags,
            FLAG_VALID | if partial { FLAG_PARTIAL_OK } else { 0 }
        );
        assert!(fee(fill) <= fee(request));
    }
}

#[test]
fn v16_program_retained_partial_flip_binds_episode_grant_and_hostile_return() {
    let mut peaks = [0; 2];
    let mut rejections = 0;
    for batch_suffix in [false, true] {
        for direction in [-1i128, 1] {
            let mut w = World::new();
            let q = direction * POS_SCALE as i128;
            let changed = [w.env.market, w.portfolios[0], w.portfolios[1], w.context];
            let open = w.sign(&[w.trade(false, 4 * q, 0)]);
            let meta = w.land(open, &changed, None);
            w.response(&meta, false, 4 * q, 4 * q, 1, false);
            w.economics(4 * q, 4, 1, 0);
            let mode = w.sign(&[w.control(15)]);
            w.land(mode, &[w.context], None);

            // Only the two typed episode fields differ. Every transaction is signed before
            // the partial commits; standalone fills have no LP-owner signature/account.
            let partial = w.trade(false, -4 * q, 0);
            let old_flip = w.trade(batch_suffix, -4 * q, 0);
            let flip = w.trade(batch_suffix, -4 * q, 1);
            let partial_tx = w.sign(&[partial.clone()]);
            let mut old_tx = w.sign(&[old_flip.clone()]);
            // Separate the identical single-route request from transaction-cache replay.
            old_tx.message.instructions[1].data =
                ComputeBudgetInstruction::set_compute_unit_limit(1_199_999).data;
            old_tx.sign(&[&w.env.payer, &w.owners[0]], w.env.svm.latest_blockhash());
            let flip_tx = w.sign(&[flip.clone()]);
            for tx in [&partial_tx, &flip_tx] {
                assert_eq!(tx.message.header.num_required_signatures, 2);
                assert!(!tx.message.account_keys.contains(&w.owners[1].pubkey()));
            }
            let repaired = w.sign(&[partial.clone(), w.control(9), flip.clone()]);
            let before = w.snapshot(&repaired);
            let simulated = w
                .env
                .svm
                .simulate_transaction(repaired.clone().into())
                .unwrap();
            w.calls(&simulated, 2, 2);
            assert_eq!(w.snapshot(&repaired), before);
            w.accepted_peak = w.accepted_peak.max(simulated.compute_units_consumed);

            let stale = w.sign(&[partial.clone(), w.control(9), old_flip]);
            w.land(
                stale,
                &[],
                Some((
                    4,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                    1,
                )),
            );
            w.economics(4 * q, 4, 1, 0);
            // Direction, request identity, LP identity, and missing partial authorization.
            for mode in [1, 4, 5, 7] {
                let hostile = w.sign(&[partial.clone(), w.control(mode), flip.clone()]);
                w.land(
                    hostile,
                    &[],
                    Some((4, InstructionError::InvalidAccountData, 2)),
                );
                w.economics(4 * q, 4, 1, 0);
            }

            let meta = w.land(partial_tx, &changed, None);
            w.response(&meta, false, -4 * q, -2 * q, 2, true);
            w.economics(2 * q, 6, 2, 0);
            let mode = w.sign(&[w.control(9)]);
            w.land(mode, &[w.context], None);
            w.land(
                old_tx,
                &[],
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                    0,
                )),
            );
            w.economics(2 * q, 6, 2, 0);
            let meta = w.land(flip_tx, &changed, None);
            w.response(&meta, batch_suffix, -4 * q, -4 * q, 3, false);
            w.economics(-2 * q, 10, 3, 0);

            // Renewal changes the grant sequence without changing the position episode.
            let old_exit = w.sign(&[w.trade(!batch_suffix, 2 * q, 0)]);
            w.env
                .try_set_matcher_config_with_trade_fee_cap_and_expiry(
                    w.matcher,
                    &w.owners[1],
                    w.portfolios[1],
                    w.context,
                    w.delegate,
                    1,
                    FEE_BPS as u16,
                    EXPIRY,
                )
                .unwrap();
            w.economics(-2 * q, 10, 3, 1);
            w.land(
                old_exit,
                &[],
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                    0,
                )),
            );
            let exit = w.sign(&[w.trade(!batch_suffix, 2 * q, 0)]);
            let meta = w.land(exit, &changed, None);
            w.response(&meta, !batch_suffix, 2 * q, 2 * q, 4, false);
            w.economics(0, 12, 4, 1);

            for actor in 0..2 {
                let payout = w.sign(&[Instruction {
                    program_id: w.env.program_id,
                    accounts: vec![
                        AccountMeta::new(w.owners[actor].pubkey(), true),
                        AccountMeta::new(w.env.market, false),
                        AccountMeta::new(w.portfolios[actor], false),
                        AccountMeta::new(w.tokens[actor], false),
                        AccountMeta::new(w.env.vault, false),
                        AccountMeta::new_readonly(w.env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: w
                        .env
                        .withdraw_ix(w.portfolios[actor], CAPITAL - 12)
                        .encode(),
                }]);
                w.land(
                    payout,
                    &[
                        w.env.market,
                        w.portfolios[actor],
                        w.tokens[actor],
                        w.env.vault,
                    ],
                    None,
                );
                assert_eq!(w.env.portfolio_state(w.portfolios[actor]).capital.get(), 0);
                assert_eq!(
                    u128::from(w.env.token_amount(w.tokens[actor])),
                    CAPITAL - 12
                );
                let market = w.env.market_state().1;
                let remaining = 3 * CAPITAL - (actor as u128 + 1) * (CAPITAL - 12);
                assert_eq!(market.vault, remaining);
                assert_eq!(u128::from(w.env.token_amount(w.env.vault)), remaining);
                assert_eq!(market.c_tot + market.insurance, remaining);
                assert_eq!(market.insurance, 24);
            }
            peaks[0] = peaks[0].max(w.accepted_peak);
            peaks[1] = peaks[1].max(w.rejected_peak);
            rejections += w.rejected;
        }
    }
    assert_eq!(rejections, 28);
    println!("INV-012 partial/flip returns: 4 worlds, 16 fills, 8 payouts, {rejections} exact rollbacks, accepted/simulated_peak={}, rejected_peak={}", peaks[0], peaks[1]);
}
