//! INV-047: pre-existing inventory inside a signed deposit/trade/withdraw word.
//! Publicly rebuilt identical snapshots; no account restoration or engine transitions.
//! Economic bytes and protocol fees stay exact. Transport state has separate postconditions.

use super::*;
use percolator_prog::matcher_abi::{read_matcher_return, MATCHER_RETURN_BYTES};
use solana_sdk::{
    fee::FeeStructure, instruction::InstructionError, signature::SeedDerivable,
    transaction::TransactionError,
};

const CAPITAL: u128 = 1_000_000;
const CASH_CAPITAL: u128 = 97;
const DEPOSIT: u128 = 211;
const WITHDRAW: u128 = 73;
const PRICES: [u64; 3] = [100, 101, 103];
const OPEN_LOTS: [i128; 3] = [3, -2, 5];
const DELTA_LOTS: [i128; 2] = [-3, 3];
const FEE_BPS: u64 = 137;
const WORD_CU_LIMIT: u64 = 1_400_000;

#[derive(Clone, Copy, Debug)]
struct Route {
    cpi: bool,
    batch: bool,
}

const ROUTES: [Route; 4] = [
    Route {
        cpi: false,
        batch: false,
    },
    Route {
        cpi: true,
        batch: false,
    },
    Route {
        cpi: false,
        batch: true,
    },
    Route {
        cpi: true,
        batch: true,
    },
];

#[derive(Clone, Debug)]
enum Step {
    Deposit,
    Trade(Vec<usize>),
    Withdraw,
}

#[derive(Default)]
struct Progress {
    deposited: bool,
    withdrawn: bool,
    filled: [bool; 2],
    trade_calls: u64,
}

fn fee(asset: usize) -> u128 {
    let notional = DELTA_LOTS[asset].unsigned_abs() * u128::from(PRICES[asset]);
    (notional * u128::from(FEE_BPS)).div_ceil(10_000)
}

type Accounts = Vec<(Pubkey, Option<Account>)>;

struct World {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    cash_portfolio: Pubkey,
    tokens: [Pubkey; 2],
    matcher: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
    direction: i128,
}

impl World {
    fn new(direction: i128) -> Self {
        // Fixed public identities make the entire starting Account frame comparable.
        let key = |tag| Keypair::from_seed(&[tag; 32]).unwrap();
        let payer = key(31);
        let admin = key(32);
        let mint = key(33);
        let market = key(34);
        let owners = [key(35), key(36)];
        let portfolios = [key(37), key(38)];
        let cash_portfolio = key(41);
        let context = key(39);
        let matcher = key(40).pubkey();
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
        system_create_account_for_test(
            &mut svm,
            &payer,
            &market,
            state::market_account_len_for_capacity(3).unwrap(),
            program_id,
        );
        let vault_authority =
            Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
        let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint.pubkey());
        let params = V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: PRICES[0],
            max_abs_funding_e9_per_slot: 0,
            ..V16CuMarketParams::default()
        };
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
            portfolio_account_len: state::portfolio_account_len_for_market_slots(3).unwrap(),
            portfolios: Vec::new(),
        };
        for (asset, price) in PRICES.into_iter().enumerate() {
            env.configure_auth_mark_for_asset_as_admin(asset as u16, 0, price);
        }
        let mut tokens = [Pubkey::default(); 2];
        for actor in 0..2 {
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolios[actor],
                env.portfolio_account_len,
                program_id,
            );
            let portfolio = portfolios[actor].pubkey();
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
                    (CAPITAL
                        + if actor == 0 {
                            DEPOSIT + CASH_CAPITAL
                        } else {
                            0
                        }) as u64,
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
        // Withdraw is flat-only; the taker's separately funded sibling supplies the debit.
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &cash_portfolio,
            env.portfolio_account_len,
            program_id,
        );
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(cash_portfolio.pubkey(), false),
            ],
            &[&owners[0]],
        )
        .unwrap();
        env.portfolios.push(cash_portfolio.pubkey());
        env.send(
            env.deposit_ix(cash_portfolio.pubkey(), CASH_CAPITAL),
            vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(cash_portfolio.pubkey(), false),
                AccountMeta::new(tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[0]],
        )
        .unwrap();
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
        let portfolios = portfolios.each_ref().map(Signer::pubkey);
        for asset in [2, 0, 1] {
            env.trade_asset_with_cu(
                asset as u16,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                direction * OPEN_LOTS[asset] * POS_SCALE as i128,
                PRICES[asset],
                0,
            );
        }
        env.update_trade_fee_policy_with_cu(FEE_BPS);
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
            cash_portfolio: cash_portfolio.pubkey(),
            tokens,
            matcher,
            context: context.pubkey(),
            delegate,
            direction,
        }
    }

    fn keys(&self) -> Vec<Pubkey> {
        let mut keys = vec![
            self.env.market,
            self.cash_portfolio,
            self.env.vault,
            self.env.mint,
            self.env.vault_authority,
            self.env.payer.pubkey(),
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

    fn frame(&self, keys: &[Pubkey]) -> Accounts {
        keys.iter()
            .map(|key| (*key, self.env.svm.get_account(key)))
            .collect()
    }

    fn word(&self, route: Route, order: [usize; 2]) -> Vec<(Step, Instruction)> {
        let mut steps = vec![Step::Deposit];
        if route.batch {
            steps.push(Step::Trade(order.to_vec()));
        } else {
            steps.extend(order.map(|asset| Step::Trade(vec![asset])));
        }
        steps.push(Step::Withdraw);
        let mut trade_offset = 0;
        steps
            .into_iter()
            .map(|step| {
                let (ix, accounts) = match &step {
                    Step::Deposit | Step::Withdraw => {
                        let portfolio = if matches!(step, Step::Deposit) {
                            self.portfolios[0]
                        } else {
                            self.cash_portfolio
                        };
                        let sequence = self.env.portfolio_matcher_sequence(portfolio);
                        let portfolio_id = self.env.portfolio_id(portfolio);
                        let ix = if matches!(step, Step::Deposit) {
                            ProgInstruction::Deposit {
                                portfolio_id,
                                expected_sequence: sequence,
                                amount: DEPOSIT,
                            }
                        } else {
                            ProgInstruction::Withdraw {
                                portfolio_id,
                                expected_sequence: sequence,
                                amount: WITHDRAW,
                            }
                        };
                        let mut accounts = vec![
                            AccountMeta::new(self.owners[0].pubkey(), true),
                            AccountMeta::new(self.env.market, false),
                            AccountMeta::new(portfolio, false),
                            AccountMeta::new(self.tokens[0], false),
                            AccountMeta::new(self.env.vault, false),
                        ];
                        if matches!(step, Step::Withdraw) {
                            accounts
                                .push(AccountMeta::new_readonly(self.env.vault_authority, false));
                        }
                        accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
                        (ix, accounts)
                    }
                    Step::Trade(assets) => {
                        let size =
                            |asset: usize| self.direction * DELTA_LOTS[asset] * POS_SCALE as i128;
                        let a = self.portfolios[0];
                        let b = self.portfolios[1];
                        let asset = assets[0];
                        let mut ix = match (route.cpi, route.batch) {
                            (false, false) => self.env.trade_no_cpi_ix(
                                a,
                                b,
                                asset as u16,
                                size(asset),
                                PRICES[asset],
                                FEE_BPS,
                            ),
                            (true, false) => self.env.trade_cpi_ix(
                                a,
                                b,
                                asset as u16,
                                size(asset),
                                FEE_BPS,
                                PRICES[asset],
                            ),
                            (false, true) => self.env.batch_trade_no_cpi_ix(
                                a,
                                b,
                                assets
                                    .iter()
                                    .map(|&asset| BatchTradeLeg {
                                        asset_index: asset as u16,
                                        market_id: self.env.asset_market_id(asset as u16),
                                        size_q: size(asset),
                                        exec_price: PRICES[asset],
                                        fee_bps: FEE_BPS,
                                    })
                                    .collect(),
                            ),
                            (true, true) => self.env.batch_trade_cpi_ix_with_caps(
                                a,
                                b,
                                assets
                                    .iter()
                                    .map(|&asset| BatchTradeCpiLeg {
                                        asset_index: asset as u16,
                                        market_id: self.env.asset_market_id(asset as u16),
                                        size_q: size(asset),
                                        limit_price: PRICES[asset],
                                        fee_bps: FEE_BPS,
                                    })
                                    .collect(),
                                0,
                                assets.iter().map(|&asset| fee(asset)).sum(),
                            ),
                        };
                        // Bind the complete word up front, including each single's next episode.
                        match &mut ix {
                            ProgInstruction::TradeNoCpi {
                                account_a_position_epoch,
                                account_b_position_epoch,
                                ..
                            }
                            | ProgInstruction::TradeCpi {
                                account_a_position_epoch,
                                account_b_position_epoch,
                                ..
                            }
                            | ProgInstruction::BatchTradeNoCpi {
                                account_a_position_epoch,
                                account_b_position_epoch,
                                ..
                            }
                            | ProgInstruction::BatchTradeCpi {
                                account_a_position_epoch,
                                account_b_position_epoch,
                                ..
                            } => {
                                *account_a_position_epoch += trade_offset;
                                *account_b_position_epoch += trade_offset;
                            }
                            _ => unreachable!(),
                        }
                        trade_offset += 1;
                        let mut accounts = vec![AccountMeta::new(self.owners[0].pubkey(), true)];
                        if !route.cpi {
                            accounts.push(AccountMeta::new(self.owners[1].pubkey(), true));
                        }
                        accounts.extend([
                            AccountMeta::new(self.env.market, false),
                            AccountMeta::new(a, false),
                            AccountMeta::new(b, false),
                        ]);
                        if route.cpi {
                            accounts.extend([
                                AccountMeta::new_readonly(self.matcher, false),
                                AccountMeta::new(self.context, false),
                                AccountMeta::new_readonly(self.delegate, false),
                            ]);
                        }
                        (ix, accounts)
                    }
                };
                (
                    step,
                    Instruction {
                        program_id: self.env.program_id,
                        accounts,
                        data: ix.encode(),
                    },
                )
            })
            .collect()
    }

    fn transaction(&mut self, instructions: &[Instruction], reject_suffix: bool) -> Transaction {
        let mut word = vec![ComputeBudgetInstruction::set_compute_unit_limit(
            WORD_CU_LIMIT as u32,
        )];
        word.extend_from_slice(instructions);
        if reject_suffix {
            word.push(Instruction {
                program_id: solana_sdk::system_program::ID,
                accounts: Vec::new(),
                data: Vec::new(),
            });
        }
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
        self.env.svm.expire_blockhash();
        Transaction::new_signed_with_payer(
            &word,
            Some(&self.env.payer.pubkey()),
            &signers,
            self.env.svm.latest_blockhash(),
        )
    }

    fn check(&self, progress: &Progress, route: Route, initial: &Accounts) {
        let initial_account = |key| {
            initial
                .iter()
                .find(|(k, _)| *k == key)
                .unwrap()
                .1
                .as_ref()
                .unwrap()
        };
        let initial_market = initial_account(self.env.market);
        let (mut expected_config, initial_group) =
            state::read_market(&initial_market.data).unwrap();
        expected_config.matcher_req_seq += if route.cpi { progress.trade_calls } else { 0 };
        let (config, group) = self.env.market_state();
        assert_eq!(config, expected_config);
        let fees: u128 = (0..2)
            .filter(|&asset| progress.filled[asset])
            .map(fee)
            .sum();
        let deposited = if progress.deposited { DEPOSIT } else { 0 };
        let withdrawn = if progress.withdrawn { WITHDRAW } else { 0 };
        assert_eq!(
            group.vault,
            2 * CAPITAL + CASH_CAPITAL + deposited - withdrawn
        );
        assert_eq!(group.c_tot, group.vault - 2 * fees);
        assert_eq!(group.insurance, 2 * fees);
        assert_eq!(
            u128::from(self.env.token_amount(self.env.vault)),
            group.vault
        );
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(
            (u128::from(mint.supply), mint.mint_authority),
            (2 * CAPITAL + CASH_CAPITAL + DEPOSIT, COption::None)
        );
        assert_eq!(
            u128::from(self.env.token_amount(self.tokens[0])),
            DEPOSIT - deposited + withdrawn
        );
        assert_eq!(self.env.token_amount(self.tokens[1]), 0);
        assert_eq!(group.assets[2], initial_group.assets[2]);
        for asset in 0..3 {
            let lots = OPEN_LOTS[asset]
                + if asset < 2 && progress.filled[asset] {
                    DELTA_LOTS[asset]
                } else {
                    0
                };
            assert_eq!(
                [
                    group.assets[asset].oi_eff_long_q,
                    group.assets[asset].oi_eff_short_q
                ],
                [lots.unsigned_abs() * POS_SCALE; 2]
            );
            assert_eq!(
                [group.assets[asset].a_long, group.assets[asset].a_short],
                [ADL_ONE; 2]
            );
            let credit = if asset < 2 && progress.filled[asset] {
                fee(asset)
            } else {
                0
            };
            assert_eq!(
                &group.insurance_domain_budget[2 * asset..2 * asset + 2],
                &[credit; 2]
            );
            for actor in 0..2 {
                let account = self.env.portfolio_state(self.portfolios[actor]);
                assert_eq!(
                    route_basis_for_asset(&account, asset),
                    self.direction * if actor == 0 { 1 } else { -1 } * lots * POS_SCALE as i128
                );
            }
        }
        for actor in 0..2 {
            let key = self.portfolios[actor];
            let before = initial_account(key);
            let after = self.env.svm.get_account(&key).unwrap();
            let account = self.env.portfolio_state(key);
            assert_eq!(
                active_route_legs(&account).len(),
                3 - usize::from(progress.filled[0]),
                "closing asset 0 must detach exactly one leg"
            );
            if progress.trade_calls == 0 {
                let expected: Vec<_> = [2, 0, 1]
                    .into_iter()
                    .enumerate()
                    .map(|(slot, asset)| {
                        (
                            slot,
                            asset as u32,
                            self.direction
                                * if actor == 0 { 1 } else { -1 }
                                * OPEN_LOTS[asset]
                                * POS_SCALE as i128,
                        )
                    })
                    .collect();
                assert_eq!(active_route_legs(&account), expected);
            }
            assert_eq!(
                account.capital.get(),
                CAPITAL - fees + if actor == 0 { deposited } else { 0 }
            );
            assert_eq!(account.pnl.get(), 0);
            assert_eq!(
                active_leg_for_asset(&account, 2),
                active_leg_for_asset(&state::read_portfolio(&before.data).unwrap(), 2)
            );
            let mut matcher = state::read_portfolio_matcher_config(&before.data).unwrap();
            for _ in 0..progress.trade_calls {
                matcher.control = state::next_portfolio_position_control(matcher.control)
                    .unwrap()
                    .1;
            }
            let enabled = actor == 1 && (route.cpi || progress.trade_calls == 0);
            matcher.set_enabled(u8::from(enabled)).unwrap();
            assert_eq!(
                state::read_portfolio_matcher_config(&after.data).unwrap(),
                matcher
            );
            assert_eq!(
                state::read_portfolio_matcher_expiry(&after.data).unwrap(),
                if enabled { u64::MAX } else { 0 }
            );
            assert_eq!(
                state::read_portfolio_id(&after.data).unwrap(),
                state::read_portfolio_id(&before.data).unwrap()
            );
            assert_eq!(
                state::read_portfolio_matcher_sequence(&after.data).unwrap(),
                state::read_portfolio_matcher_sequence(&before.data).unwrap()
                    + if actor == 0 {
                        u64::from(progress.deposited)
                    } else {
                        0
                    }
            );
        }
        let cash_before = initial_account(self.cash_portfolio);
        let cash_after = self.env.svm.get_account(&self.cash_portfolio).unwrap();
        let cash = state::read_portfolio(&cash_after.data).unwrap();
        assert_eq!(cash.capital.get(), CASH_CAPITAL - withdrawn);
        assert_eq!(cash.pnl.get(), 0);
        assert!(active_route_legs(&cash).is_empty());
        assert_eq!(
            state::read_portfolio_matcher_sequence(&cash_after.data).unwrap(),
            state::read_portfolio_matcher_sequence(&cash_before.data).unwrap()
                + u64::from(progress.withdrawn)
        );
        assert_eq!(
            state::read_portfolio_matcher_config(&cash_after.data).unwrap(),
            state::read_portfolio_matcher_config(&cash_before.data).unwrap()
        );
        assert_eq!(
            state::read_portfolio_id(&cash_after.data).unwrap(),
            state::read_portfolio_id(&cash_before.data).unwrap()
        );
        assert_eq!(
            state::read_portfolio_matcher_expiry(&cash_after.data).unwrap(),
            0
        );
        // No mutable-account metadata, passive Account, or unrelated matcher byte may drift.
        for (key, before) in initial {
            if *key == self.env.payer.pubkey() {
                continue;
            }
            let after = self.env.svm.get_account(key);
            if [
                self.env.market,
                self.portfolios[0],
                self.portfolios[1],
                self.cash_portfolio,
                self.env.vault,
                self.tokens[0],
                self.context,
            ]
            .contains(key)
            {
                let before = before.as_ref().unwrap();
                let after = after.unwrap();
                assert_eq!(
                    (
                        after.owner,
                        after.lamports,
                        after.executable,
                        after.rent_epoch,
                        after.data.len()
                    ),
                    (
                        before.owner,
                        before.lamports,
                        before.executable,
                        before.rent_epoch,
                        before.data.len()
                    ),
                    "metadata {key}"
                );
                if *key == self.context {
                    if route.cpi && !route.batch && progress.trade_calls > 0 {
                        assert_eq!(
                            &after.data[MATCHER_RETURN_BYTES..],
                            &before.data[MATCHER_RETURN_BYTES..]
                        );
                    } else {
                        assert_eq!(&after, before);
                    }
                }
            } else {
                assert_eq!(&after, before, "passive {key}");
            }
        }
    }

    fn economic_bytes(&self) -> Vec<Vec<u8>> {
        let mut result = vec![self.env.svm.get_account(&self.env.market).unwrap().data
            [MARKET_GROUP_OFF..]
            .to_vec()];
        for key in [self.portfolios[0], self.portfolios[1], self.cash_portfolio] {
            result.push(
                self.env.svm.get_account(&key).unwrap().data[..PORTFOLIO_ENGINE_ACCOUNT_LEN]
                    .to_vec(),
            );
        }
        for key in [
            self.env.vault,
            self.env.mint,
            self.tokens[0],
            self.tokens[1],
        ] {
            result.push(self.env.svm.get_account(&key).unwrap().data);
        }
        result
    }
}

fn assert_accounts_eq(actual: &Accounts, expected: &Accounts, label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}");
    for ((key, actual), (expected_key, expected)) in actual.iter().zip(expected) {
        assert_eq!(key, expected_key, "{label}");
        assert!(actual == expected, "{label}: Account {key} differs");
    }
}

fn verify(retry: bool) {
    assert_eq!([fee(0), fee(1)], [5, 5]);
    let mut counts = [0u64; 3]; // Worlds, commits, deliberate suffix rejections.
    let mut peak = [0u64; 2];
    for direction in [-1, 1] {
        let mut common_initial = None;
        for order in [[0, 1], [1, 0]] {
            let mut common_economics: Option<Vec<Vec<u8>>> = None;
            for route in ROUTES {
                let mut direct_frame = None;
                for composite in [false, true] {
                    let label = format!("direction={direction} order={order:?} route={route:?} composite={composite} retry={retry}");
                    let mut world = World::new(direction);
                    let keys = world.keys();
                    let initial = world.frame(&keys);
                    if let Some(expected) = &common_initial {
                        assert_accounts_eq(
                            &initial,
                            expected,
                            "identical public starting snapshot",
                        );
                    } else {
                        common_initial = Some(initial.clone());
                    }
                    let word = world.word(route, order);
                    let width = if composite { word.len() } else { 1 };
                    let mut progress = Progress::default();
                    let mut network_fees = 0;
                    world.check(&progress, route, &initial);
                    for (chunk_index, chunk) in word.chunks(width).enumerate() {
                        let label = format!("{label} chunk={chunk_index}");
                        let instructions: Vec<_> = chunk.iter().map(|(_, ix)| ix.clone()).collect();
                        if retry {
                            let tx = world.transaction(&instructions, true);
                            let mut tracked = keys.clone();
                            tracked.extend(&tx.message.account_keys);
                            tracked.sort_unstable();
                            tracked.dedup();
                            let mut before = world.frame(&tracked);
                            let fee = u64::from(tx.message.header.num_required_signatures)
                                * FeeStructure::default().lamports_per_signature;
                            let failed = world
                                .env
                                .svm
                                .send_transaction(tx)
                                .expect_err("invalid System suffix");
                            assert_eq!(
                                failed.err,
                                TransactionError::InstructionError(
                                    (instructions.len() + 1) as u8,
                                    InstructionError::InvalidInstructionData
                                ),
                                "{label}"
                            );
                            let successes = failed
                                .meta
                                .logs
                                .iter()
                                .filter(|line| {
                                    **line == format!("Program {} success", world.env.program_id)
                                })
                                .count();
                            assert_eq!(
                                successes,
                                instructions.len(),
                                "all public prefix instructions ran: {label}"
                            );
                            let matcher_successes = failed
                                .meta
                                .logs
                                .iter()
                                .filter(|line| {
                                    **line == format!("Program {} success", world.matcher)
                                })
                                .count();
                            assert_eq!(
                                matcher_successes,
                                if route.cpi {
                                    chunk
                                        .iter()
                                        .filter(|(step, _)| matches!(step, Step::Trade(_)))
                                        .count()
                                } else {
                                    0
                                },
                                "{label}"
                            );
                            before
                                .iter_mut()
                                .find(|(key, _)| *key == world.env.payer.pubkey())
                                .unwrap()
                                .1
                                .as_mut()
                                .unwrap()
                                .lamports -= fee;
                            assert_accounts_eq(
                                &world.frame(&tracked),
                                &before,
                                &format!("suffix rollback {label}"),
                            );
                            world.check(&progress, route, &initial);
                            network_fees += fee;
                            counts[2] += 1;
                            peak[1] = peak[1].max(failed.meta.compute_units_consumed);
                            assert_cu_within(
                                "inventory suffix rejection",
                                failed.meta.compute_units_consumed,
                                WORD_CU_LIMIT,
                            );
                        }
                        // Reuse exactly the same wrapper payloads, accounts and signer roles.
                        let tx = world.transaction(&instructions, false);
                        let fee = u64::from(tx.message.header.num_required_signatures)
                            * FeeStructure::default().lamports_per_signature;
                        let accepted = world
                            .env
                            .svm
                            .send_transaction(tx)
                            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
                        network_fees += fee;
                        counts[1] += 1;
                        peak[0] = peak[0].max(accepted.compute_units_consumed);
                        assert_cu_within(
                            "inventory cashflow partition",
                            accepted.compute_units_consumed,
                            WORD_CU_LIMIT,
                        );
                        for (step, _) in chunk {
                            match step {
                                Step::Deposit => progress.deposited = true,
                                Step::Withdraw => progress.withdrawn = true,
                                Step::Trade(assets) => {
                                    progress.trade_calls += 1;
                                    for &asset in assets {
                                        progress.filled[asset] = true;
                                    }
                                }
                            }
                        }
                        world.check(&progress, route, &initial);
                        if route.cpi && !route.batch && progress.trade_calls > 0 {
                            let context = world.env.svm.get_account(&world.context).unwrap();
                            let quote =
                                read_matcher_return(&context.data[..MATCHER_RETURN_BYTES]).unwrap();
                            let asset = order[progress.trade_calls as usize - 1];
                            assert_eq!(quote.abi_version, MATCHER_ABI_VERSION);
                            assert_eq!(quote.flags, percolator_prog::matcher_abi::FLAG_VALID);
                            assert_eq!(quote.req_id, progress.trade_calls);
                            assert_eq!(
                                quote.lp_account_id,
                                u64::from_le_bytes(
                                    world.delegate.to_bytes()[..8].try_into().unwrap()
                                )
                            );
                            assert_eq!(quote.asset_index, asset as u64);
                            assert_eq!(
                                [quote.exec_price_e6, quote.oracle_price_e6],
                                [PRICES[asset]; 2]
                            );
                            assert_eq!(
                                quote.exec_size,
                                direction * DELTA_LOTS[asset] * POS_SCALE as i128
                            );
                        }
                        let initial_payer = initial
                            .iter()
                            .find(|(key, _)| *key == world.env.payer.pubkey())
                            .unwrap()
                            .1
                            .as_ref()
                            .unwrap()
                            .lamports;
                        assert_eq!(
                            world
                                .env
                                .svm
                                .get_account(&world.env.payer.pubkey())
                                .unwrap()
                                .lamports,
                            initial_payer - network_fees,
                            "{label}"
                        );
                    }
                    assert!(
                        progress.deposited && progress.withdrawn && progress.filled == [true; 2]
                    );
                    let economics = world.economic_bytes();
                    if let Some(expected) = &common_economics {
                        for (index, (actual, expected)) in
                            economics.iter().zip(expected).enumerate()
                        {
                            assert!(
                                actual == expected,
                                "economic bytes {index} differ: {label}; first offset {:?}",
                                actual.iter().zip(expected).position(|(a, b)| a != b)
                            );
                        }
                    } else {
                        common_economics = Some(economics);
                    }
                    let mut final_frame = world.frame(&keys);
                    // Only documented network signature fees are adjusted in Account comparisons.
                    final_frame
                        .iter_mut()
                        .find(|(key, _)| *key == world.env.payer.pubkey())
                        .unwrap()
                        .1
                        .as_mut()
                        .unwrap()
                        .lamports += network_fees;
                    if let Some(expected) = &direct_frame {
                        assert_accounts_eq(
                            &final_frame,
                            expected,
                            &format!("direct/composite {label}"),
                        );
                    } else {
                        direct_frame = Some(final_frame);
                    }
                    counts[0] += 1;
                }
            }
        }
    }
    assert_eq!(counts, [32, 72, if retry { 72 } else { 0 }]);
    println!("INV-047 inventory cashflows retry={retry}: worlds/commits/rejections={counts:?}, peak CU accepted/rejected={peak:?}, fee/owner=10, net custody credit=138");
}

#[test]
fn v16_program_inventory_cashflows_match_direct_composite_and_partition_routes() {
    verify(false);
}

#[test]
fn v16_program_inventory_cashflow_suffix_rollback_retries_preserve_route_equivalence() {
    verify(true);
}
