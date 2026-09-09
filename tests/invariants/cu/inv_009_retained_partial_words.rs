//! INV-008/009/010: retained partial/residual words across atomic failure and delivery order.
//! All economic setup and matcher controls use public instructions. A partial consumes one-shot
//! consent completely; the pre-signed next-episode residual is a separate authorization.

use super::*;
use percolator_prog::matcher_abi::{read_matcher_return, FLAG_PARTIAL_OK, MATCHER_RETURN_BYTES};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const CAPITAL: u128 = 1_000_000;
const PRICE: u64 = 100;
const FEE_BPS: u64 = 100;

fn fee_per_owner(quantity: i128) -> u128 {
    let notional = (quantity.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    (notional * u128::from(FEE_BPS)).div_ceil(10_000)
}

struct World {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    matcher: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
    initial_epochs: [u64; 2],
    initial_ids: [u64; 2],
    accepted: u64,
    rejected: u64,
    peak_cu: u64,
}

impl World {
    fn new(numerator: u8) -> Self {
        let mut env = crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::
            inv018_public_spl_market_with_params(6, V16CuMarketParams {
                trade_fee_base_bps: FEE_BPS,
                ..V16CuMarketParams::default()
            });
        env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
        let owners = [Keypair::new(), Keypair::new()];
        let portfolio_keys = [Keypair::new(), Keypair::new()];
        let portfolios = portfolio_keys.each_ref().map(Signer::pubkey);
        let mut tokens = [Pubkey::default(); 2];
        for actor in 0..2 {
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
                &portfolio_keys[actor],
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
        for data in [vec![10], vec![11, 19, numerator]] {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                Instruction {
                    program_id: matcher,
                    accounts: vec![
                        AccountMeta::new_readonly(owners[1].pubkey(), true),
                        AccountMeta::new(context, false),
                    ],
                    data,
                },
                &[&owners[1]],
            )
            .unwrap();
        }
        env.set_matcher_config_with_trade_fee_cap(
            matcher,
            &owners[1],
            portfolios[1],
            context,
            delegate,
            1,
            FEE_BPS as u16,
        );
        let initial_epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
        let initial_ids = portfolios.map(|key| env.portfolio_id(key));
        Self {
            env,
            owners,
            portfolios,
            tokens,
            matcher,
            context,
            delegate,
            initial_epochs,
            initial_ids,
            accepted: 0,
            rejected: 0,
            peak_cu: 0,
        }
    }

    fn trade(&self, route: PartialRetryRoute, quantity: i128, episode_offset: u64) -> Instruction {
        let mut request = retained_partial_retry_ix(
            &self.env,
            route,
            self.portfolios[0],
            self.portfolios[1],
            quantity,
        );
        // Bind the future episode in the typed request before signing, never in account bytes.
        match &mut request {
            ProgInstruction::TradeNoCpi {
                account_a_position_epoch,
                account_b_position_epoch,
                ..
            }
            | ProgInstruction::BatchTradeNoCpi {
                account_a_position_epoch,
                account_b_position_epoch,
                ..
            }
            | ProgInstruction::TradeCpi {
                account_a_position_epoch,
                account_b_position_epoch,
                ..
            }
            | ProgInstruction::BatchTradeCpi {
                account_a_position_epoch,
                account_b_position_epoch,
                ..
            } => {
                *account_a_position_epoch += episode_offset;
                *account_b_position_epoch += episode_offset;
            }
            _ => unreachable!(),
        }
        match &mut request {
            ProgInstruction::TradeCpi { limit_price, .. } => *limit_price = PRICE,
            ProgInstruction::BatchTradeCpi {
                legs,
                max_fee_atoms,
                max_slippage_atoms,
                ..
            } => {
                legs[0].limit_price = PRICE;
                *max_fee_atoms = fee_per_owner(quantity);
                *max_slippage_atoms = 0;
            }
            _ => {}
        }
        let accounts = if matches!(route, PartialRetryRoute::Cpi | PartialRetryRoute::BatchCpi) {
            vec![
                AccountMeta::new(self.owners[0].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[0], false),
                AccountMeta::new(self.portfolios[1], false),
                AccountMeta::new_readonly(self.matcher, false),
                AccountMeta::new(self.context, false),
                AccountMeta::new_readonly(self.delegate, false),
            ]
        } else {
            vec![
                AccountMeta::new(self.owners[0].pubkey(), true),
                AccountMeta::new(self.owners[1].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[0], false),
                AccountMeta::new(self.portfolios[1], false),
            ]
        };
        Instruction {
            program_id: self.env.program_id,
            accounts,
            data: request.encode(),
        }
    }

    fn retain(&self, instructions: &[Instruction]) -> Transaction {
        let mut word = vec![heap_ix(), cu_ix()];
        word.extend_from_slice(instructions);
        let mut signers = vec![&self.env.payer];
        for owner in &self.owners {
            if word
                .iter()
                .flat_map(|ix| &ix.accounts)
                .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
            {
                signers.push(owner);
            }
        }
        let transaction = Transaction::new_signed_with_payer(
            &word,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        );
        assert!(
            bincode::serialized_size(&transaction).unwrap()
                <= solana_sdk::packet::PACKET_DATA_SIZE as u64,
            "retained words must fit a public transaction packet"
        );
        transaction
    }

    fn keys(&self, transaction: &Transaction) -> Vec<Pubkey> {
        let mut keys = transaction.message.account_keys.clone();
        keys.extend([
            self.env.market,
            self.env.mint,
            self.env.vault,
            self.env.vault_authority,
            self.env.admin.pubkey(),
            self.context,
            self.delegate,
            self.matcher,
            self.env.program_id,
            spl_token::ID,
            associated_token_program_id(),
            solana_sdk::system_program::ID,
            solana_sdk::sysvar::clock::ID,
        ]);
        keys.extend(self.portfolios);
        keys.extend(self.tokens);
        keys.extend(self.owners.each_ref().map(Signer::pubkey));
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    fn assert_economics(&self, quantity: i128, fees: u128, fills: u64) {
        for actor in 0..2 {
            let key = self.portfolios[actor];
            let portfolio = self.env.portfolio_state(key);
            assert_eq!(portfolio.capital.get(), CAPITAL - fees);
            assert_eq!(portfolio.pnl.get(), 0);
            assert_eq!(self.env.portfolio_id(key), self.initial_ids[actor]);
            assert_eq!(
                self.env.portfolio_position_epoch(key),
                self.initial_epochs[actor] + fills
            );
            if quantity == 0 {
                assert!(!has_active_leg_for_asset(&portfolio, 0));
            } else {
                assert_eq!(
                    active_leg_for_asset(&portfolio, 0).basis_pos_q,
                    quantity * if actor == 0 { 1 } else { -1 }
                );
            }
            assert_eq!(self.env.token_amount(self.tokens[actor]), 0);
        }
        let (_, market) = self.env.market_state();
        assert_eq!(
            [
                market.assets[0].oi_eff_long_q,
                market.assets[0].oi_eff_short_q
            ],
            [quantity.unsigned_abs(); 2]
        );
        assert_eq!(market.insurance, 2 * fees);
        assert_eq!(&market.insurance_domain_budget[..2], &[fees; 2]);
        assert_eq!(market.c_tot, 2 * (CAPITAL - fees));
        assert_eq!(market.vault, 2 * CAPITAL);
        assert_eq!(market.c_tot + market.insurance, market.vault);
        assert_eq!(
            u128::from(self.env.token_amount(self.env.vault)),
            market.vault
        );
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), 2 * CAPITAL);
        assert_eq!(mint.mint_authority, COption::None);
    }

    fn deliver(
        &mut self,
        retained: &Transaction,
        error: Option<(u8, InstructionError)>,
        economics: (i128, u128, u64),
        label: &str,
    ) {
        let mut transaction = retained.clone();
        self.env.svm.expire_blockhash();
        let mut signers = vec![&self.env.payer];
        for owner in &self.owners {
            if transaction
                .message
                .account_keys
                .iter()
                .enumerate()
                .any(|(index, key)| *key == owner.pubkey() && transaction.message.is_signer(index))
            {
                signers.push(owner);
            }
        }
        transaction.sign(&signers, self.env.svm.latest_blockhash());
        transaction.verify().unwrap();
        let mut unchanged = transaction.message.clone();
        unchanged.recent_blockhash = retained.message.recent_blockhash;
        assert_eq!(
            unchanged, retained.message,
            "{label}: only the transport envelope changes"
        );
        assert_ne!(transaction.signatures, retained.signatures);
        let keys = self.keys(&transaction);
        let frame = |env: &V16CuEnv| {
            keys.iter()
                .map(|key| (*key, env.svm.get_account(key)))
                .collect::<Vec<_>>()
        };
        let mut before = frame(&self.env);
        let network_fee = u64::from(transaction.message.header.num_required_signatures)
            * FeeStructure::default().lamports_per_signature;
        let payer = self.env.payer.pubkey();
        let payer_before = before
            .iter_mut()
            .find(|(key, _)| *key == payer)
            .unwrap()
            .1
            .as_mut()
            .unwrap();
        payer_before.lamports -= network_fee;
        let result = self.env.svm.send_transaction(transaction);
        let cu = if let Some((index, error)) = error {
            let failure = result.unwrap_err();
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, error),
                "{label}"
            );
            assert_eq!(
                frame(&self.env),
                before,
                "{label}: complete Account rollback plus exact payer fee"
            );
            self.rejected += 1;
            failure.meta.compute_units_consumed
        } else {
            let metadata = result.unwrap_or_else(|failure| panic!("{label}: {failure:?}"));
            for (key, account) in &before {
                if ![
                    self.env.market,
                    self.portfolios[0],
                    self.portfolios[1],
                    self.context,
                ]
                .contains(key)
                {
                    assert_eq!(
                        self.env.svm.get_account(key),
                        *account,
                        "{label}: unchanged {key}"
                    );
                }
            }
            self.accepted += 1;
            metadata.compute_units_consumed
        };
        assert_cu_within(label, cu, 1_400_000);
        self.peak_cu = self.peak_cu.max(cu);
        self.assert_economics(economics.0, economics.1, economics.2);
    }
}

#[test]
fn v16_program_retained_partial_words_preserve_one_shot_and_residual_budgets() {
    let mut totals = [0u64; 4];
    for direction in [-1i128, 1] {
        for numerator in [127u8, 254] {
            for route in PartialRetryRoute::ALL {
                let mut world = World::new(numerator);
                let total = direction * (255 * POS_SCALE + POS_SCALE / 2 + 1) as i128;
                let partial =
                    direction * (total.unsigned_abs() * u128::from(numerator) / 255) as i128;
                let residual = total - partial;
                assert!(partial != 0 && residual != 0);
                assert_ne!(partial.unsigned_abs() % POS_SCALE, 0);
                let partial_fee = fee_per_owner(partial);
                let residual_fee = fee_per_owner(residual);
                let budget = partial_fee + residual_fee;
                let empty = (0, 0, 0);
                let after_partial = (partial, partial_fee, 1);
                let complete = (total, budget, 2);
                let case = format!("{direction}/{numerator}/{route:?}");
                world.assert_economics(0, 0, 0);

                // Every payload and signature is retained before the first attempted delivery.
                let p = world.trade(PartialRetryRoute::Cpi, total, 0);
                let old = world.trade(route, total, 0);
                let r = world.trade(route, residual, 1);
                let full = Instruction {
                    program_id: world.matcher,
                    accounts: vec![
                        AccountMeta::new_readonly(world.owners[1].pubkey(), true),
                        AccountMeta::new(world.context, false),
                    ],
                    data: vec![11, 9, 0],
                };
                let insufficient = spl_token::instruction::transfer(
                    &spl_token::ID,
                    &world.tokens[0],
                    &world.tokens[1],
                    &world.owners[0].pubkey(),
                    &[],
                    1,
                )
                .unwrap();
                let p_only = world.retain(&[p.clone()]);
                let r_only = world.retain(&[r.clone()]);
                let old_only = world.retain(&[old.clone()]);
                let full_only = world.retain(&[full.clone()]);
                let duplicate_partial = world.retain(&[p.clone(), p.clone()]);
                let alternate_duplicate = world.retain(&[p.clone(), full.clone(), old]);
                let failed_complete = world.retain(&[p.clone(), full, r.clone(), insufficient]);
                let reversed_retry = world.retain(&[r.clone(), p]);
                let duplicate_residual = world.retain(&[r.clone(), r]);
                let stale = |index| {
                    Some((
                        index,
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    ))
                };

                world.deliver(
                    &r_only,
                    stale(2),
                    empty,
                    &format!("{case}: future episode first"),
                );
                world.deliver(
                    &duplicate_partial,
                    stale(3),
                    empty,
                    &format!("{case}: duplicate partial word"),
                );
                world.deliver(
                    &alternate_duplicate,
                    stale(4),
                    empty,
                    &format!("{case}: alternate duplicate word"),
                );
                world.deliver(
                    &failed_complete,
                    Some((
                        5,
                        InstructionError::Custom(
                            spl_token::error::TokenError::InsufficientFunds as u32,
                        ),
                    )),
                    empty,
                    &format!("{case}: two fills then rejected SPL suffix"),
                );
                // Both fills reached the SPL suffix, but neither authorization nor fee was consumed.
                world.deliver(
                    &p_only,
                    None,
                    after_partial,
                    &format!("{case}: retained partial retry"),
                );
                let context = world.env.svm.get_account(&world.context).unwrap();
                let fill = read_matcher_return(&context.data[..MATCHER_RETURN_BYTES]).unwrap();
                assert_eq!(fill.exec_size, partial);
                assert_eq!(fill.exec_price_e6, PRICE);
                assert_ne!(fill.flags & FLAG_PARTIAL_OK, 0);
                assert_eq!(budget - partial_fee, residual_fee);
                assert_eq!(total - partial, residual);
                // The remaining economic plan is nonzero, but this consumed one-shot has no allowance.
                world.deliver(
                    &p_only,
                    stale(2),
                    after_partial,
                    &format!("{case}: later partial duplicate"),
                );
                world.deliver(
                    &full_only,
                    None,
                    after_partial,
                    &format!("{case}: full matcher capacity"),
                );
                world.deliver(
                    &old_only,
                    stale(2),
                    after_partial,
                    &format!("{case}: alternate stale delivery"),
                );
                world.deliver(
                    &reversed_retry,
                    stale(3),
                    after_partial,
                    &format!("{case}: residual then delayed old suffix"),
                );
                world.deliver(
                    &duplicate_residual,
                    stale(3),
                    after_partial,
                    &format!("{case}: duplicate residual word"),
                );
                world.deliver(
                    &r_only,
                    None,
                    complete,
                    &format!("{case}: retained exact residual retry"),
                );
                for (label, request) in [
                    ("old partial", &p_only),
                    ("alternate old", &old_only),
                    ("residual", &r_only),
                ] {
                    world.deliver(
                        request,
                        stale(2),
                        complete,
                        &format!("{case}: completed {label}"),
                    );
                }
                assert!(budget >= fee_per_owner(total));
                assert!(budget - fee_per_owner(total) <= 2);
                assert_eq!((world.accepted, world.rejected), (3, 11));
                totals[0] += 1;
                totals[1] += world.accepted;
                totals[2] += world.rejected;
                totals[3] = totals[3].max(world.peak_cu);
            }
        }
    }
    assert_eq!(&totals[..3], &[16, 48, 176]);
    println!("INV-008/009/010 retained partial words: {} worlds, {} accepted transactions (32 fills), {} exact rejections, peak CU={}",
        totals[0], totals[1], totals[2], totals[3]);
}
