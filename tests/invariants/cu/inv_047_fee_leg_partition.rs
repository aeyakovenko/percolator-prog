//! INV-047/052: two distinct fee-bearing legs, four transports, one normalized frame.
//! Whole legs are partitioned, not quantities; each notional and fee ceiling is preserved.
//! All fixture accounts are constructed through System/SPL/ATA/wrapper instructions.
//! Signed bid/ask prints away from AuthMark additionally exercise exact slippage consent
//! while fees, health, owner value, and the normalized route frame stay mark-based.

use super::*;
use percolator_prog::constants::{PORTFOLIO_MATCHER_EXPIRY_LEN, PORTFOLIO_MATCHER_EXPIRY_OFF};
use percolator_prog::matcher_abi::{read_matcher_return, MATCHER_RETURN_BYTES};
use solana_sdk::{
    fee::FeeStructure, instruction::InstructionError, signature::SeedDerivable,
    transaction::TransactionError,
};

const CAPITAL: u128 = 1_000_000;
const PRICES: [u64; 2] = [100, 101];
const QUANTITIES: [u128; 2] = [72 * POS_SCALE / 100 + 1, 2 * POS_SCALE + POS_SCALE / 7 + 3];
const FEE_BPS: u64 = 137;

fn ceil(n: u128, d: u128) -> u128 {
    n / d + u128::from(n % d != 0)
}

fn notional(asset: usize) -> u128 {
    ceil(QUANTITIES[asset] * u128::from(PRICES[asset]), POS_SCALE)
}

fn fee(asset: usize) -> u128 {
    ceil(notional(asset) * u128::from(FEE_BPS), 10_000)
}

struct Fixture {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    matcher: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
}

impl Fixture {
    fn new() -> Self {
        // Stable identities allow whole-account comparisons without normalizing owners or mints.
        let key = |tag| Keypair::from_seed(&[tag; 32]).unwrap();
        let payer = key(1);
        let admin = key(2);
        let mint = key(3);
        let market = key(4);
        let owners = [key(5), key(6)];
        let accounts = [key(7), key(8)];
        let context = key(9);
        let matcher = key(10).pubkey();
        let program_id = percolator_prog::id();
        let mut svm = LiteSVM::new();
        for (id, path) in [
            (program_id, program_path()),
            (spl_token::ID, spl_token_program_path()),
            (
                associated_token_program_id(),
                associated_token_program_path(),
            ),
            (matcher, auth_matcher_program_path()),
        ] {
            svm.add_program(id, &std::fs::read(path).unwrap());
        }
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        for signer in [&admin, &owners[0], &owners[1]] {
            send_raw_tx(
                &mut svm,
                &payer,
                system_instruction::transfer(&payer.pubkey(), &signer.pubkey(), 1_000_000_000),
                &[],
            )
            .unwrap();
        }
        system_create_account_for_test(&mut svm, &payer, &mint, Mint::LEN, spl_token::ID);
        send_raw_tx(
            &mut svm,
            &payer,
            spl_token::instruction::initialize_mint2(
                &spl_token::ID,
                &mint.pubkey(),
                &admin.pubkey(),
                None,
                6,
            )
            .unwrap(),
            &[],
        )
        .unwrap();
        let params = V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: PRICES[0],
            ..V16CuMarketParams::default()
        };
        system_create_account_for_test(
            &mut svm,
            &payer,
            &market,
            state::market_account_len_for_capacity(2).unwrap(),
            program_id,
        );
        let vault_authority =
            Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
        let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint.pubkey());
        let init_market_cu = send_tx(
            &mut svm,
            program_id,
            &payer,
            init_market_instruction(&params),
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market.pubkey(), false),
                AccountMeta::new_readonly(mint.pubkey(), false),
            ],
            &[&admin],
        )
        .unwrap();
        let mut env = V16CuEnv {
            svm,
            program_id,
            payer,
            admin,
            init_market_cu,
            market: market.pubkey(),
            mint: mint.pubkey(),
            vault,
            vault_authority,
            portfolio_account_len: state::portfolio_account_len_for_market_slots(2).unwrap(),
            portfolios: Vec::new(),
        };
        for (asset, price) in PRICES.into_iter().enumerate() {
            env.configure_auth_mark_for_asset_as_admin(asset as u16, 0, price);
        }
        env.update_trade_fee_policy_with_cu(FEE_BPS);
        let mut tokens = [Pubkey::default(); 2];
        for actor in 0..2 {
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &accounts[actor],
                env.portfolio_account_len,
                program_id,
            );
            let portfolio = accounts[actor].pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            env.portfolios.push(portfolio);
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
                env.deposit_ix(portfolio, CAPITAL),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
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
        let portfolios = accounts.each_ref().map(Signer::pubkey);
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &context,
            MATCHER_CONTEXT_LEN,
            matcher,
        );
        let delegate = matcher_delegate_key(
            &program_id,
            &env.market,
            &portfolios[1],
            &owners[1].pubkey(),
            &matcher,
            &context.pubkey(),
        );
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            Instruction {
                program_id: matcher,
                accounts: vec![
                    AccountMeta::new_readonly(owners[1].pubkey(), true),
                    AccountMeta::new_readonly(delegate, false),
                    AccountMeta::new(context.pubkey(), false),
                    AccountMeta::new_readonly(program_id, false),
                    AccountMeta::new_readonly(env.market, false),
                    AccountMeta::new_readonly(portfolios[1], false),
                ],
                data: vec![2],
            },
            &[&owners[1]],
        )
        .unwrap();
        env.set_matcher_config_with_trade_fee_cap(
            matcher,
            &owners[1],
            portfolios[1],
            context.pubkey(),
            delegate,
            1,
            FEE_BPS as u16,
        );
        Self {
            env,
            owners,
            portfolios,
            tokens,
            matcher,
            context: context.pubkey(),
            delegate,
        }
    }

    fn keys(&self) -> Vec<Pubkey> {
        let mut keys = vec![
            self.env.market,
            self.env.vault,
            self.env.mint,
            self.env.admin.pubkey(),
            self.context,
            self.delegate,
            self.matcher,
            self.env.program_id,
            spl_token::ID,
            associated_token_program_id(),
            solana_sdk::system_program::ID,
            solana_sdk::compute_budget::ID,
        ];
        keys.extend(self.portfolios);
        keys.extend(self.tokens);
        keys.extend(self.owners.each_ref().map(Signer::pubkey));
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    fn frame(&self, keys: &[Pubkey]) -> Vec<(Pubkey, Option<Account>)> {
        keys.iter()
            .map(|key| (*key, self.env.svm.get_account(key)))
            .collect()
    }

    fn transaction(
        &mut self,
        cpi: bool,
        batch: bool,
        assets: &[usize],
        direction: i128,
        short_cap: bool,
        quotes: [u64; 2],
    ) -> Transaction {
        let size = |asset| direction * if asset == 0 { 1 } else { -1 } * QUANTITIES[asset] as i128;
        let ix = match (cpi, batch) {
            (false, false) => self.env.trade_no_cpi_ix(
                self.portfolios[0],
                self.portfolios[1],
                assets[0] as u16,
                size(assets[0]),
                quotes[assets[0]],
                FEE_BPS,
            ),
            (true, false) => self.env.trade_cpi_ix(
                self.portfolios[0],
                self.portfolios[1],
                assets[0] as u16,
                size(assets[0]),
                FEE_BPS,
                quotes[assets[0]],
            ),
            (false, true) => self.env.batch_trade_no_cpi_ix(
                self.portfolios[0],
                self.portfolios[1],
                assets
                    .iter()
                    .map(|&asset| BatchTradeLeg {
                        asset_index: asset as u16,
                        market_id: self.env.asset_market_id(asset as u16),
                        size_q: size(asset),
                        exec_price: quotes[asset],
                        fee_bps: FEE_BPS,
                    })
                    .collect(),
            ),
            (true, true) => self.env.batch_trade_cpi_ix_with_caps(
                self.portfolios[0],
                self.portfolios[1],
                assets
                    .iter()
                    .map(|&asset| BatchTradeCpiLeg {
                        asset_index: asset as u16,
                        market_id: self.env.asset_market_id(asset as u16),
                        size_q: size(asset),
                        limit_price: quotes[asset],
                        fee_bps: FEE_BPS,
                    })
                    .collect(),
                assets
                    .iter()
                    .map(|&asset| {
                        ceil(
                            QUANTITIES[asset] * u128::from(quotes[asset].abs_diff(PRICES[asset])),
                            POS_SCALE,
                        )
                    })
                    .sum(),
                assets.iter().map(|&asset| fee(asset)).sum::<u128>() - u128::from(short_cap),
            ),
        };
        let mut accounts = vec![AccountMeta::new(self.owners[0].pubkey(), true)];
        if !cpi {
            accounts.push(AccountMeta::new(self.owners[1].pubkey(), true));
        }
        accounts.extend([
            AccountMeta::new(self.env.market, false),
            AccountMeta::new(self.portfolios[0], false),
            AccountMeta::new(self.portfolios[1], false),
        ]);
        if cpi {
            accounts.extend([
                AccountMeta::new_readonly(self.matcher, false),
                AccountMeta::new(self.context, false),
                AccountMeta::new_readonly(self.delegate, false),
            ]);
        }
        let mut signers = vec![&self.env.payer, &self.owners[0]];
        if !cpi {
            signers.push(&self.owners[1]);
        }
        self.env.svm.expire_blockhash();
        Transaction::new_signed_with_payer(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(TRADE_CU_LIMIT as u32),
                Instruction {
                    program_id: self.env.program_id,
                    accounts,
                    data: ix.encode(),
                },
            ],
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        )
    }

    fn check(
        &self,
        filled: usize,
        direction: i128,
        commits: u64,
        cpi: bool,
        initial_epochs: [u64; 2],
    ) {
        let (cfg, group) = self.env.market_state();
        let fees: u128 = (0..filled).map(fee).sum();
        assert_eq!(cfg.trade_fee_base_bps, FEE_BPS);
        assert_eq!(cfg.fee_redirect_to_market_0_bps, 0);
        assert_eq!(cfg.matcher_req_seq, if cpi { commits } else { 0 });
        assert_eq!(group.vault, 2 * CAPITAL);
        assert_eq!(group.c_tot, 2 * (CAPITAL - fees));
        assert_eq!(group.insurance, 2 * fees);
        assert_eq!(group.vault, group.c_tot + group.insurance);
        assert_eq!(
            u128::from(self.env.token_amount(self.env.vault)),
            group.vault
        );
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(
            (u128::from(mint.supply), mint.mint_authority),
            (2 * CAPITAL, COption::None)
        );
        for asset in 0..2 {
            let q = if asset < filled { QUANTITIES[asset] } else { 0 };
            let fee = if asset < filled { fee(asset) } else { 0 };
            assert_eq!(
                [
                    group.assets[asset].oi_eff_long_q,
                    group.assets[asset].oi_eff_short_q
                ],
                [q; 2]
            );
            assert_eq!(
                [group.assets[asset].a_long, group.assets[asset].a_short],
                [ADL_ONE; 2]
            );
            assert_eq!(group.assets[asset].effective_price, PRICES[asset]);
            assert_eq!(
                &group.insurance_domain_budget[2 * asset..2 * asset + 2],
                &[fee; 2],
                "trade fees follow the two trade sides, not a per-owner half split"
            );
        }
        for actor in 0..2 {
            let account = self.env.portfolio_state(self.portfolios[actor]);
            assert_eq!(account.capital.get(), CAPITAL - fees);
            assert_eq!(account.pnl.get(), 0);
            assert_eq!(self.env.token_amount(self.tokens[actor]), 0);
            assert_eq!(
                self.env.portfolio_position_epoch(self.portfolios[actor]),
                initial_epochs[actor] + commits
            );
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&account)),
                filled as u32
            );
            for asset in 0..filled {
                let sign = direction * if actor == asset { 1 } else { -1 };
                assert_eq!(
                    active_leg_for_asset(&account, asset).basis_pos_q,
                    sign * QUANTITIES[asset] as i128
                );
            }
            if filled > 0 {
                let cert = health_cert(&account);
                assert!(cert.valid);
                assert_eq!(cert.certified_equity, (CAPITAL - fees) as i128);
                assert_eq!(
                    cert.certified_worst_case_loss,
                    (0..filled).map(notional).sum()
                );
            }
        }
    }

    fn normalized(&self, commits: u64, cpi: bool) -> Vec<(Pubkey, Option<Account>)> {
        let mut frame = self.frame(&self.keys());
        for (key, account) in &mut frame {
            let Some(account) = account else { continue };
            if *key == self.env.market {
                let (mut cfg, _) = state::read_market(&account.data).unwrap();
                assert_eq!(cfg.matcher_req_seq, if cpi { commits } else { 0 });
                cfg.matcher_req_seq = 0;
                state::write_wrapper_config(&mut account.data, &cfg).unwrap();
            } else if self.portfolios.contains(key) {
                let mut matcher = state::read_portfolio_matcher_config(&account.data).unwrap();
                // Exactly one extra instruction-level position episode in the single-leg partition.
                let (_, next_control) =
                    state::next_portfolio_position_control(matcher.control).unwrap();
                matcher.control -= (commits - 1) * (next_control - matcher.control);
                if *key == self.portfolios[1] {
                    assert_eq!(matcher.enabled(), u64::from(cpi));
                    assert_eq!(
                        self.env.portfolio_matcher_expiry(*key),
                        if cpi { u64::MAX } else { 0 }
                    );
                    matcher.set_enabled(0).unwrap();
                    account.data[PORTFOLIO_MATCHER_EXPIRY_OFF
                        ..PORTFOLIO_MATCHER_EXPIRY_OFF + PORTFOLIO_MATCHER_EXPIRY_LEN]
                        .fill(0);
                }
                state::write_portfolio_matcher_config(&mut account.data, &matcher).unwrap();
            } else if *key == self.context {
                account.data[..64].fill(0);
            }
        }
        frame
    }
}

fn check_nonintegral_fee_leg_routes(spreads: [u64; 2]) {
    assert_eq!([notional(0), notional(1)], [73, 217]);
    assert_eq!([fee(0), fee(1)], [2, 3]);
    for asset in 0..2 {
        assert_ne!(QUANTITIES[asset] % POS_SCALE, 0);
        assert_ne!(QUANTITIES[asset] * u128::from(PRICES[asset]) % POS_SCALE, 0);
        assert_ne!(notional(asset) * u128::from(FEE_BPS) % 10_000, 0);
    }
    assert_eq!(
        ceil(
            QUANTITIES[0] * u128::from(PRICES[0]) * u128::from(FEE_BPS),
            POS_SCALE * 10_000
        ),
        1
    );
    let mut counts = [0u64; 3]; // Worlds, accepted instructions, rejected instructions.
    let mut peak = [0u64; 3]; // Batch, single, rejected.
    let mut route_peak = [0u64; 4]; // No-CPI batch, CPI batch, no-CPI singles, CPI singles.
    for direction in [-1, 1] {
        let quotes = core::array::from_fn(|asset| {
            let side = direction * if asset == 0 { 1 } else { -1 };
            PRICES[asset]
                * if side < 0 {
                    10_000 - spreads[0]
                } else {
                    10_000 + spreads[1]
                }
                / 10_000
        });
        if spreads != [0; 2] {
            for asset in 0..2 {
                assert_ne!(quotes[asset], PRICES[asset]);
            }
            assert!(
                (0..2).any(|asset| {
                    ceil(
                        ceil(QUANTITIES[asset] * u128::from(quotes[asset]), POS_SCALE)
                            * u128::from(FEE_BPS),
                        10_000,
                    ) != fee(asset)
                }),
                "reported-price fee control must differ from the AuthMark fee"
            );
        }
        let mut expected: Option<Vec<(Pubkey, Option<Account>)>> = None;
        for (cpi, batch) in [(false, true), (true, true), (false, false), (true, false)] {
            let mut fixture = Fixture::new();
            let mut data = vec![4];
            for spread in spreads {
                data.extend_from_slice(&spread.to_le_bytes());
            }
            send_raw_tx(
                &mut fixture.env.svm,
                &fixture.env.payer,
                Instruction {
                    program_id: fixture.matcher,
                    accounts: vec![
                        AccountMeta::new_readonly(fixture.owners[1].pubkey(), true),
                        AccountMeta::new(fixture.context, false),
                    ],
                    data,
                },
                &[&fixture.owners[1]],
            )
            .expect("LP configures the same bid/ask quote in every public world");
            let initial_epochs = fixture
                .portfolios
                .map(|key| fixture.env.portfolio_position_epoch(key));
            let keys = fixture.keys();
            let mutable = [
                fixture.env.market,
                fixture.portfolios[0],
                fixture.portfolios[1],
                fixture.context,
            ];
            let passive: Vec<_> = keys
                .iter()
                .copied()
                .filter(|key| !mutable.contains(key))
                .collect();
            let initial_passive = fixture.frame(&passive);
            let initial_mutable = fixture.frame(&mutable);
            let context_before = fixture.env.svm.get_account(&fixture.context).unwrap();
            fixture.check(0, direction, 0, cpi, initial_epochs);
            if cpi && batch {
                let tx = fixture.transaction(cpi, batch, &[0, 1], direction, true, quotes);
                let mut tracked = keys.clone();
                tracked.extend(&tx.message.account_keys);
                tracked.sort_unstable();
                tracked.dedup();
                let before = fixture.frame(&tracked);
                let failed = fixture.env.svm.send_transaction(tx.clone()).unwrap_err();
                assert_eq!(
                    failed.err,
                    TransactionError::InstructionError(
                        1,
                        InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
                    )
                );
                assert!(failed
                    .meta
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {} success", fixture.matcher)));
                assert_cu_within(
                    "two-asset fee cap rollback",
                    failed.meta.compute_units_consumed,
                    TRADE_CU_LIMIT,
                );
                peak[2] = peak[2].max(failed.meta.compute_units_consumed);
                counts[2] += 1;
                for (key, mut account) in before {
                    if key == fixture.env.payer.pubkey() {
                        account.as_mut().unwrap().lamports -=
                            u64::from(tx.message.header.num_required_signatures)
                                * FeeStructure::default().lamports_per_signature;
                    }
                    assert_eq!(fixture.env.svm.get_account(&key), account, "rollback {key}");
                }
                fixture.check(0, direction, 0, cpi, initial_epochs);
            }
            let schedule: &[&[usize]] = if batch { &[&[0, 1]] } else { &[&[0], &[1]] };
            let mut filled = 0;
            for (step, assets) in schedule.iter().enumerate() {
                let tx = fixture.transaction(cpi, batch, assets, direction, false, quotes);
                let payer_before = fixture
                    .env
                    .svm
                    .get_account(&fixture.env.payer.pubkey())
                    .unwrap()
                    .lamports;
                let accepted = fixture.env.svm.send_transaction(tx.clone()).unwrap();
                assert_cu_within(
                    "two-asset fee leg partition",
                    accepted.compute_units_consumed,
                    TRADE_CU_LIMIT,
                );
                peak[usize::from(!batch)] =
                    peak[usize::from(!batch)].max(accepted.compute_units_consumed);
                let route = 2 * usize::from(!batch) + usize::from(cpi);
                route_peak[route] = route_peak[route].max(accepted.compute_units_consumed);
                counts[1] += 1;
                filled += assets.len();
                fixture.check(filled, direction, step as u64 + 1, cpi, initial_epochs);
                assert_eq!(fixture.frame(&passive), initial_passive);
                assert_eq!(
                    fixture
                        .env
                        .svm
                        .get_account(&fixture.env.payer.pubkey())
                        .unwrap()
                        .lamports,
                    payer_before
                        - u64::from(tx.message.header.num_required_signatures)
                            * FeeStructure::default().lamports_per_signature
                );
                let context = fixture.env.svm.get_account(&fixture.context).unwrap();
                if cpi {
                    let returns = if batch {
                        assert_eq!(accepted.return_data.program_id, fixture.matcher);
                        &accepted.return_data.data[..]
                    } else {
                        &context.data[..MATCHER_RETURN_BYTES]
                    };
                    assert_eq!(returns.len(), assets.len() * MATCHER_RETURN_BYTES);
                    for (&asset, bytes) in assets
                        .iter()
                        .zip(returns.chunks_exact(MATCHER_RETURN_BYTES))
                    {
                        let quoted = read_matcher_return(bytes).unwrap();
                        assert_eq!(quoted.abi_version, MATCHER_ABI_VERSION);
                        assert_eq!(quoted.flags, percolator_prog::matcher_abi::FLAG_VALID);
                        assert_eq!(quoted.req_id, step as u64 + 1);
                        assert_eq!(
                            quoted.lp_account_id,
                            u64::from_le_bytes(
                                fixture.delegate.to_bytes()[..8].try_into().unwrap()
                            )
                        );
                        assert_eq!(quoted.asset_index, asset as u64);
                        assert_eq!(
                            [quoted.exec_price_e6, quoted.oracle_price_e6],
                            [quotes[asset], PRICES[asset]]
                        );
                        assert_eq!(
                            quoted.exec_size,
                            direction * if asset == 0 { 1 } else { -1 } * QUANTITIES[asset] as i128
                        );
                    }
                }
                let mut framed_context = context.clone();
                if cpi && !batch {
                    framed_context.data[..64].copy_from_slice(&context_before.data[..64]);
                }
                assert_eq!(framed_context, context_before);
                for (key, before) in &initial_mutable {
                    let mut after = fixture.env.svm.get_account(key).unwrap();
                    after.data = before.as_ref().unwrap().data.clone();
                    assert_eq!(Some(after), *before, "only program data changes: {key}");
                }
            }
            let frame = fixture.normalized(schedule.len() as u64, cpi);
            if let Some(expected) = &expected {
                assert_eq!(frame.len(), expected.len());
                for ((key, actual), (expected_key, expected)) in frame.iter().zip(expected.iter()) {
                    assert_eq!(key, expected_key);
                    assert_eq!(
                        actual, expected,
                        "spreads={spreads:?}, direction={direction}, cpi={cpi}, batch={batch}, key={key}"
                    );
                }
            } else {
                expected = Some(frame);
            }
            counts[0] += 1;
        }
    }
    assert_eq!(counts, [8, 12, 2]);
    println!("INV-047/052 two-asset fee partitions: spreads={spreads:?}, counts={counts:?}, peak CU batch/single/rejected={peak:?}; notionals=[73,217], fees/owner=[2,3], domain credits=[2,2,3,3]");
    println!("INV-047/052 route peak CU no-CPI batch/CPI batch/no-CPI singles/CPI singles={route_peak:?}; limit={TRADE_CU_LIMIT}");
}

#[test]
fn v16_program_nonintegral_two_asset_fee_legs_match_cpi_nocpi_batch_and_singles() {
    check_nonintegral_fee_leg_routes([0, 0]);
}

#[test]
fn v16_program_off_mark_quotes_preserve_fractional_fee_route_equivalence() {
    check_nonintegral_fee_leg_routes([4_000, 2_500]);
}

#[test]
fn v16_program_retained_routes_apply_current_redirect_policy_per_payer() {
    // A three-atom fee redirects one atom at 50%: splitting each payer's redirect
    // credits [0, 2], whereas incorrectly pooling the two redirects credits [1, 1].
    let owner_fee = (QUANTITIES[1] * u128::from(PRICES[1]))
        .div_ceil(POS_SCALE)
        .checked_mul(u128::from(FEE_BPS))
        .unwrap()
        .div_ceil(10_000);
    assert_eq!(owner_fee, 3);
    let mut peak_cu = 0;
    let mut fills = 0;
    for direction in [-1, 1] {
        let mut reference = None;
        for (cpi, batch) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut fixture = Fixture::new();
            fixture.env.update_fee_redirect_policy_with_cu(3_333);
            let initial_sequence = fixture.env.control_sequences(0).fee_redirect;
            let initial_epochs = fixture
                .portfolios
                .map(|key| fixture.env.portfolio_position_epoch(key));
            let custody_keys = [
                fixture.env.vault,
                fixture.env.mint,
                fixture.tokens[0],
                fixture.tokens[1],
            ];
            let custody = fixture.frame(&custody_keys);
            let mut domains = [0u128; 4];
            let mut frames = Vec::new();
            for (step, redirect_bps) in [5_000u16, 10_000].into_iter().enumerate() {
                let sign = if step == 0 { direction } else { -direction };
                let retained = fixture.transaction(cpi, batch, &[1], sign, false, PRICES);
                retained.verify().unwrap();
                let signed_bytes = bincode::serialize(&retained).unwrap();
                let old_bps = fixture.env.market_state().0.fee_redirect_to_market_0_bps;
                let redirect = owner_fee * u128::from(redirect_bps) / 10_000;
                assert_ne!(redirect, owner_fee * u128::from(old_bps) / 10_000);
                fixture.env.update_fee_redirect_policy_with_cu(redirect_bps);
                assert_eq!(
                    fixture.env.control_sequences(0).fee_redirect,
                    initial_sequence + step as u64 + 1
                );
                assert_eq!(bincode::serialize(&retained).unwrap(), signed_bytes);
                let landed = fixture.env.svm.send_transaction(retained).unwrap();
                peak_cu = peak_cu.max(landed.compute_units_consumed);
                assert_cu_within(
                    "retained redirect route",
                    landed.compute_units_consumed,
                    TRADE_CU_LIMIT,
                );
                fills += 1;

                for source_side in 0..2 {
                    domains[source_side + 2] += owner_fee - redirect;
                    domains[0] += redirect / 2;
                    domains[1] += redirect - redirect / 2;
                }
                assert_eq!(
                    domains,
                    if step == 0 {
                        [0, 2, 2, 2]
                    } else {
                        [2, 6, 2, 2]
                    }
                );
                let commits = step as u64 + 1;
                let paid = u128::from(commits) * owner_fee;
                let (cfg, group) = fixture.env.market_state();
                assert_eq!(cfg.fee_redirect_to_market_0_bps, redirect_bps);
                assert_eq!(&group.insurance_domain_budget[..4], &domains);
                assert!(group.insurance_domain_budget[4..]
                    .iter()
                    .all(|budget| *budget == 0));
                assert_eq!(group.insurance, 2 * paid);
                assert_eq!(group.c_tot, 2 * (CAPITAL - paid));
                assert_eq!(group.vault, 2 * CAPITAL);
                assert_eq!(group.vault, group.c_tot + group.insurance);
                assert_eq!(
                    u128::from(fixture.env.token_amount(fixture.env.vault)),
                    group.vault
                );
                assert_eq!(fixture.frame(&custody_keys), custody);
                for asset in 0..2 {
                    let q = if step == 0 && asset == 1 {
                        QUANTITIES[1]
                    } else {
                        0
                    };
                    assert_eq!(
                        [
                            group.assets[asset].oi_eff_long_q,
                            group.assets[asset].oi_eff_short_q
                        ],
                        [q; 2]
                    );
                }
                for actor in 0..2 {
                    let account = fixture.env.portfolio_state(fixture.portfolios[actor]);
                    assert_eq!(account.capital.get(), CAPITAL - paid);
                    assert_eq!(account.pnl.get(), 0);
                    assert_eq!(
                        fixture
                            .env
                            .portfolio_position_epoch(fixture.portfolios[actor]),
                        initial_epochs[actor] + commits
                    );
                    assert_eq!(
                        percolator::active_bitmap_count_ones(active_bitmap(&account)),
                        u32::from(step == 0)
                    );
                    if step == 0 {
                        assert_eq!(
                            active_leg_for_asset(&account, 1).basis_pos_q,
                            direction * if actor == 0 { -1 } else { 1 } * QUANTITIES[1] as i128
                        );
                    }
                }
                frames.push(fixture.normalized(commits, cpi));
            }
            if let Some(expected) = &reference {
                assert_eq!(
                    &frames, expected,
                    "cpi={cpi}, batch={batch}, direction={direction}"
                );
            } else {
                reference = Some(frames);
            }
        }
    }
    assert_eq!(fills, 16);
    println!("INV-036/047 retained redirect: 8 worlds, 16 fills, peak CU={peak_cu}");
}

#[test]
fn v16_program_retained_resolution_routes_preserve_unsettled_fee_payouts() {
    // INV-010/024/036/047/081: unlike the empty-market resolution pair, retain
    // requests across mark changes, then compare latent PnL, side fees and SPL payouts.
    // Both marks must be accrued: authority resolution may freeze the old effective
    // price while a pushed mark is pending (INV-066's unaccrued-mark witness).
    const MARKS: [u64; 2] = [110, 91];
    const LOTS: [i128; 2] = [1, -2];
    const RESOLVE_SLOT: u64 = 3;
    let fees: [u128; 2] = core::array::from_fn(|asset| {
        (LOTS[asset].unsigned_abs() * u128::from(PRICES[asset]) * u128::from(FEE_BPS))
            .div_ceil(10_000)
    });
    let owner_fee: u128 = fees.iter().sum();
    let gain: i128 = (0..2)
        .map(|asset| LOTS[asset] * (i128::from(MARKS[asset]) - i128::from(PRICES[asset])))
        .sum();
    assert_eq!((fees, owner_fee, gain), ([2, 3], 5, 30));
    let mut peak_cu = 0;
    let mut payouts = 0;
    for direction in [-1, 1] {
        let mut reference = None;
        for permissionless in [false, true] {
            let mut fixture = Fixture::new();
            fixture.env.configure_permissionless_resolve_with_cu(2, 1);
            let empty = Keypair::from_seed(&[11; 32]).unwrap();
            system_create_account_for_test(
                &mut fixture.env.svm,
                &fixture.env.payer,
                &empty,
                fixture.env.portfolio_account_len,
                fixture.env.program_id,
            );
            let admin = fixture.env.admin.insecure_clone();
            fixture
                .env
                .send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(fixture.env.market, false),
                        AccountMeta::new(empty.pubkey(), false),
                    ],
                    &[&admin],
                )
                .unwrap();
            for asset in 0..2 {
                fixture.env.trade_asset_with_cu(
                    asset as u16,
                    &fixture.owners[0],
                    fixture.portfolios[0],
                    &fixture.owners[1],
                    fixture.portfolios[1],
                    direction * LOTS[asset] * POS_SCALE as i128,
                    PRICES[asset],
                    FEE_BPS,
                );
            }
            let mut keys = fixture.keys();
            keys.push(empty.pubkey());
            let mut frames = vec![fixture.frame(&keys)];
            let resolve = Instruction {
                program_id: fixture.env.program_id,
                accounts: if permissionless {
                    vec![AccountMeta::new(fixture.env.market, false)]
                } else {
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(fixture.env.market, false),
                    ]
                },
                data: if permissionless {
                    ProgInstruction::ResolveStalePermissionless { now_slot: 0 }
                } else {
                    ProgInstruction::ResolveMarket {
                        asset_generation_frontier: fixture.env.market_state().1.next_market_id,
                        authority_epoch: fixture.env.control_sequences(0).authority_epoch,
                    }
                }
                .encode(),
            };
            let sign = |fixture: &mut Fixture| {
                fixture.env.svm.expire_blockhash();
                let mut signers = vec![&fixture.env.payer];
                if !permissionless {
                    signers.push(&admin);
                }
                Transaction::new_signed_with_payer(
                    &[heap_ix(), cu_ix(), resolve.clone()],
                    Some(&fixture.env.payer.pubkey()),
                    &signers,
                    fixture.env.svm.latest_blockhash(),
                )
            };
            let retained = sign(&mut fixture);
            retained.verify().unwrap();
            fixture.env.svm.warp_to_slot(1);
            for asset in 0..2 {
                fixture
                    .env
                    .push_auth_mark_for_asset_as_admin(asset as u16, 1, MARKS[asset]);
            }
            assert_eq!(fixture.env.market_state().0.last_good_oracle_slot, 1);

            // Accrue while the marks are live, before stale maturity. An empty
            // target leaves both owners' gains/losses latent until terminal close.
            let cu = fixture
                .env
                .send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 1,
                        observations: crank_observations_for_assets(&[0, 1]),
                    },
                    vec![
                        AccountMeta::new(fixture.env.payer.pubkey(), true),
                        AccountMeta::new(fixture.env.market, false),
                        AccountMeta::new(empty.pubkey(), false),
                    ],
                    &[],
                )
                .unwrap();
            peak_cu = peak_cu.max(cu);
            fixture.env.svm.warp_to_slot(RESOLVE_SLOT);
            let (cfg, group) = fixture.env.market_state();
            assert_eq!(cfg.last_good_oracle_slot, 1);
            assert_eq!(
                (group.vault, group.c_tot, group.insurance),
                (2 * CAPITAL, 2 * (CAPITAL - owner_fee), 2 * owner_fee)
            );
            assert_eq!(
                &group.insurance_domain_budget[..4],
                &[fees[0], fees[0], fees[1], fees[1]]
            );
            for asset in 0..2 {
                assert_eq!(group.assets[asset].slot_last, 1);
                assert_eq!(group.assets[asset].effective_price, MARKS[asset]);
                assert_eq!(
                    [
                        group.assets[asset].oi_eff_long_q,
                        group.assets[asset].oi_eff_short_q
                    ],
                    [LOTS[asset].unsigned_abs() * POS_SCALE; 2]
                );
            }
            for portfolio in fixture.portfolios {
                let account = fixture.env.portfolio_state(portfolio);
                assert_eq!(
                    (account.capital.get(), account.pnl.get()),
                    (CAPITAL - owner_fee, 0)
                );
                assert_eq!(
                    percolator::active_bitmap_count_ones(active_bitmap(&account)),
                    2
                );
            }

            // Exclude only the fee payer from paired account frames, and check its
            // route-specific signature charge separately. Economic bytes stay exact.
            let mut payer = fixture
                .env
                .svm
                .get_account(&fixture.env.payer.pubkey())
                .unwrap();
            payer.lamports -=
                retained.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
            peak_cu = peak_cu.max(
                fixture
                    .env
                    .svm
                    .send_transaction(retained)
                    .unwrap()
                    .compute_units_consumed,
            );
            assert_eq!(
                fixture
                    .env
                    .svm
                    .get_account(&fixture.env.payer.pubkey())
                    .unwrap(),
                payer
            );
            let (_, resolved) = fixture.env.market_state();
            assert_eq!(resolved.mode, MarketModeV16::Resolved);
            assert_eq!(resolved.resolved_slot, RESOLVE_SLOT);
            frames.push(fixture.frame(&keys));
            let retry = sign(&mut fixture);
            let before = fixture.frame(&keys);
            let error = fixture.env.svm.send_transaction(retry).unwrap_err();
            assert_eq!(
                error.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                )
            );
            peak_cu = peak_cu.max(error.meta.compute_units_consumed);
            assert_eq!(fixture.frame(&keys), before, "resolved retry rolls back");

            fixture.env.svm.warp_to_slot(RESOLVE_SLOT + 1);
            let winner = usize::from(direction < 0);
            for actor in [1 - winner, winner] {
                let expected = u64::try_from(
                    (CAPITAL - owner_fee) as i128
                        + direction * if actor == 0 { gain } else { -gain },
                )
                .unwrap();
                for _ in 0..8 {
                    if resolved_portfolio_is_terminal(&fixture.env, fixture.portfolios[actor]) {
                        break;
                    }
                    fixture.env.svm.expire_blockhash();
                    let cu = fixture
                        .env
                        .send(
                            ProgInstruction::CloseResolved {
                                fee_rate_per_slot: 0,
                            },
                            vec![
                                AccountMeta::new_readonly(fixture.owners[actor].pubkey(), false),
                                AccountMeta::new(fixture.env.market, false),
                                AccountMeta::new(fixture.portfolios[actor], false),
                                AccountMeta::new(fixture.tokens[actor], false),
                                AccountMeta::new(fixture.env.vault, false),
                                AccountMeta::new_readonly(fixture.env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[],
                        )
                        .unwrap();
                    peak_cu = peak_cu.max(cu);
                    assert!(fixture.env.token_amount(fixture.tokens[actor]) <= expected);
                }
                assert!(resolved_portfolio_is_terminal(
                    &fixture.env,
                    fixture.portfolios[actor]
                ));
                assert_eq!(fixture.env.token_amount(fixture.tokens[actor]), expected);
                payouts += 1;
                frames.push(fixture.frame(&keys));
            }
            let (_, terminal) = fixture.env.market_state();
            assert_eq!(
                (terminal.vault, terminal.c_tot, terminal.insurance),
                (2 * owner_fee, 0, 2 * owner_fee)
            );
            assert_eq!(
                &terminal.insurance_domain_budget[..4],
                &[fees[0], fees[0], fees[1], fees[1]]
            );
            assert_eq!(
                u128::from(fixture.env.token_amount(fixture.env.vault)),
                2 * owner_fee
            );
            assert_eq!(
                fixture
                    .tokens
                    .map(|key| u128::from(fixture.env.token_amount(key)))
                    .iter()
                    .sum::<u128>()
                    + terminal.vault,
                2 * CAPITAL
            );
            for asset in &terminal.assets[..2] {
                assert_eq!((asset.oi_eff_long_q, asset.oi_eff_short_q), (0, 0));
            }
            if let Some(expected) = &reference {
                assert_eq!(
                    &frames, expected,
                    "resolution routes: direction={direction}"
                );
            } else {
                reference = Some(frames);
            }
        }
    }
    assert_eq!(payouts, 8);
    assert_cu_within(
        "retained resolution and terminal payout",
        peak_cu,
        CUSTODY_CU_LIMIT,
    );
    println!("INV-047 retained resolution: 4 worlds, 4 exact rollbacks, {payouts} owner payouts, peak CU={peak_cu}");
}
