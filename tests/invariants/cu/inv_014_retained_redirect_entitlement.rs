//! INV-014/024/036/047/080/081: retained fee consent composes with recipient-local
//! earnings after prior payouts and a redirect-policy change. Exact transports
//! and an equivalent single-CPI partial must realize the same four entitlements.

use super::super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeSet;

const PRICE: u64 = 100;
const OLD_BPS: u64 = 19;
const CAP_BPS: u64 = 37;
const DEPOSITS: [u128; 2] = [100_003, 200_007];
const REQUEST: i128 = (255 * POS_SCALE + POS_SCALE / 2 + 1) as i128;
const FILLED: i128 = REQUEST * 127 / 255;
const REDIRECTS: [u16; 2] = [3_333, 6_667];

fn fee(quantity: i128) -> u128 {
    let notional = (quantity.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    (notional * u128::from(CAP_BPS)).div_ceil(10_000)
}

fn credits(redirect_bps: u16) -> [u128; 4] {
    let charge = fee(FILLED);
    let redirected = charge * u128::from(redirect_bps) / 10_000;
    // Redirect each side's earned fee separately; each odd split favors base short.
    [
        2 * (redirected / 2),
        2 * (redirected - redirected / 2),
        charge - redirected,
        charge - redirected,
    ]
}

struct Fixture {
    env: V16CuEnv,
    owners: [Keypair; 2],
    portfolios: [Pubkey; 2],
    tokens: [Pubkey; 2],
    recipients: [Keypair; 2],
    destinations: [Pubkey; 2],
    matcher: Pubkey,
    context: Pubkey,
    delegate: Pubkey,
    nonce: u32,
}

impl Fixture {
    fn new() -> Self {
        let mut env = inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                trade_fee_base_bps: OLD_BPS,
                ..V16CuMarketParams::default()
            },
        );
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset, 0, PRICE);
        }
        let owners = [Keypair::new(), Keypair::new()];
        let recipients = [Keypair::new(), Keypair::new()];
        let keys = [Keypair::new(), Keypair::new()];
        let portfolios = keys.each_ref().map(Signer::pubkey);
        let mut tokens = [Pubkey::default(); 2];
        let mut destinations = [Pubkey::default(); 2];
        for signer in owners.iter().chain(&recipients) {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(&env.payer.pubkey(), &signer.pubkey(), 1_000_000),
                &[],
            )
            .unwrap();
        }
        for actor in 0..2 {
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
            destinations[actor] = create_ata_for_test(
                &mut env.svm,
                &env.payer,
                recipients[actor].pubkey(),
                env.mint,
            );
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    DEPOSITS[actor] as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[actor], DEPOSITS[actor]),
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
            let authority = ProgInstruction::UpdateAssetAuthority {
                asset_index: actor as u16,
                market_id: env.asset_market_id(actor as u16),
                authority_epoch: env.control_sequences(actor).authority_epoch,
                kind: processor::ASSET_AUTH_INSURANCE_OPERATOR,
                new_pubkey: recipients[actor].pubkey().to_bytes(),
            };
            send_tx(
                &mut env.svm,
                env.program_id,
                &env.payer,
                authority,
                vec![
                    AccountMeta::new(env.admin.pubkey(), true),
                    AccountMeta::new(recipients[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                &[&env.admin, &recipients[actor]],
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
        env.set_matcher_config_with_trade_fee_cap(
            matcher,
            &owners[1],
            portfolios[1],
            context,
            delegate,
            1,
            CAP_BPS as u16,
        );
        Self {
            env,
            owners,
            portfolios,
            tokens,
            recipients,
            destinations,
            matcher,
            context,
            delegate,
            nonce: 0,
        }
    }

    fn control(&mut self, partial: bool) {
        send_raw_tx(
            &mut self.env.svm,
            &self.env.payer,
            Instruction {
                program_id: self.matcher,
                accounts: vec![
                    AccountMeta::new_readonly(self.owners[1].pubkey(), true),
                    AccountMeta::new(self.context, false),
                ],
                data: if partial {
                    vec![11, 19, 127]
                } else {
                    vec![11, 9, 0]
                },
            },
            &[&self.owners[1]],
        )
        .unwrap();
    }

    fn trade(&self, route: usize, quantity: i128) -> Instruction {
        let [a, b] = self.portfolios;
        let env = &self.env;
        let ix = match route {
            0 => env.trade_no_cpi_ix(a, b, 1, quantity, PRICE, CAP_BPS),
            1 => env.batch_trade_no_cpi_ix(
                a,
                b,
                vec![BatchTradeLeg {
                    asset_index: 1,
                    market_id: env.asset_market_id(1),
                    size_q: quantity,
                    exec_price: PRICE,
                    fee_bps: CAP_BPS,
                }],
            ),
            2 => env.trade_cpi_ix(a, b, 1, quantity, CAP_BPS, PRICE),
            3 => env.batch_trade_cpi_ix_with_caps(
                a,
                b,
                vec![BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: env.asset_market_id(1),
                    size_q: quantity,
                    limit_price: PRICE,
                    fee_bps: CAP_BPS,
                }],
                0,
                fee(quantity),
            ),
            _ => unreachable!(),
        };
        let mut accounts = vec![AccountMeta::new(self.owners[0].pubkey(), true)];
        if route < 2 {
            accounts.push(AccountMeta::new(self.owners[1].pubkey(), true));
        }
        accounts.extend([
            AccountMeta::new(env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
        ]);
        if route >= 2 {
            accounts.extend([
                AccountMeta::new_readonly(self.matcher, false),
                AccountMeta::new(self.context, false),
                AccountMeta::new_readonly(self.delegate, false),
            ]);
        }
        Instruction {
            program_id: env.program_id,
            accounts,
            data: ix.encode(),
        }
    }

    fn insurance(&self, asset: usize, amount: u128) -> Instruction {
        let env = &self.env;
        Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(self.recipients[asset].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(self.destinations[asset], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env
                .withdraw_insurance_asset_instruction(
                    self.recipients[asset].pubkey(),
                    asset as u16,
                    amount,
                )
                .encode(),
        }
    }

    fn redirect(&self, bps: u16) -> Instruction {
        let env = &self.env;
        let seq = env.control_sequences(0);
        Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            data: ProgInstruction::UpdateFeeRedirectPolicy {
                redirect_bps: bps,
                policy_sequence: seq.fee_redirect + 1,
                authority_epoch: seq.authority_epoch,
            }
            .encode(),
        }
    }

    fn sign(&mut self, instructions: &[Instruction]) -> Transaction {
        self.nonce += 1;
        let mut ixs = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000 - self.nonce),
        ];
        ixs.extend_from_slice(instructions);
        let mut signers = vec![&self.env.payer];
        for signer in [&self.env.admin]
            .into_iter()
            .chain(&self.owners)
            .chain(&self.recipients)
        {
            if instructions
                .iter()
                .flat_map(|ix| &ix.accounts)
                .any(|m| m.is_signer && m.pubkey == signer.pubkey())
            {
                signers.push(signer);
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

    fn frame(&self, tx: &Transaction) -> Vec<(Pubkey, Option<Account>)> {
        let keys: BTreeSet<_> = tx
            .message
            .account_keys
            .iter()
            .copied()
            .chain([
                self.env.market,
                self.env.mint,
                self.env.vault,
                self.context,
                self.delegate,
                self.matcher,
            ])
            .chain(self.portfolios)
            .chain(self.tokens)
            .chain(self.destinations)
            .chain(
                self.owners
                    .iter()
                    .chain(&self.recipients)
                    .map(Signer::pubkey),
            )
            .collect();
        keys.into_iter()
            .map(|key| (key, self.env.svm.get_account(&key)))
            .collect()
    }

    fn deliver(
        &mut self,
        tx: Transaction,
        reject: bool,
        wrapper_successes: usize,
        spl_successes: usize,
        matcher_successes: usize,
    ) -> u64 {
        let before = self.frame(&tx);
        let mut payer = self.env.svm.get_account(&self.env.payer.pubkey()).unwrap();
        payer.lamports -= FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let result = self.env.svm.send_transaction(tx);
        let meta = if reject {
            let failure = result.expect_err("recipient cannot spend its peer's last atom");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    5,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                )
            );
            failure.meta
        } else {
            result.expect("consented public progress")
        };
        for (key, account) in before {
            if key == self.env.payer.pubkey() {
                continue;
            }
            let actual = self.env.svm.get_account(&key);
            let mutable = [self.env.market, self.env.vault, self.context].contains(&key)
                || self.portfolios.contains(&key)
                || self.tokens.contains(&key)
                || self.destinations.contains(&key);
            if reject || !mutable {
                assert_eq!(actual, account, "complete Account frame: {key}");
            } else {
                let mut expected = account.unwrap();
                expected.data = actual.as_ref().unwrap().data.clone();
                assert_eq!(actual, Some(expected), "only data may change: {key}");
            }
        }
        assert_eq!(
            self.env.svm.get_account(&self.env.payer.pubkey()),
            Some(payer)
        );
        for (program, count) in [
            (self.env.program_id, wrapper_successes),
            (spl_token::ID, spl_successes),
            (self.matcher, matcher_successes),
        ] {
            assert_eq!(
                meta.logs
                    .iter()
                    .filter(|log| **log == format!("Program {program} success"))
                    .count(),
                count
            );
        }
        assert!(meta.compute_units_consumed > 0 && meta.compute_units_consumed <= 1_400_000);
        meta.compute_units_consumed
    }

    fn check(
        &self,
        quantity: i128,
        fees: u128,
        budgets: [u128; 4],
        paid: [u128; 2],
        owner_paid: [u128; 2],
    ) {
        let env = &self.env;
        let accounts = self.portfolios.map(|key| env.portfolio_state(key));
        let (cfg, group) = env.market_state();
        assert_eq!(cfg.trade_fee_base_bps, CAP_BPS);
        for asset in 0..2 {
            assert_eq!(group.assets[asset].effective_price, PRICE);
        }
        assert_eq!(&group.insurance_domain_budget[..4], &budgets);
        assert!(group.insurance_domain_budget[4..].iter().all(|v| *v == 0));
        assert_eq!(group.insurance, 2 * fees - paid.iter().sum::<u128>());
        assert_eq!(group.insurance, budgets.iter().sum::<u128>());
        assert_eq!(
            group.c_tot,
            DEPOSITS.iter().sum::<u128>() - 2 * fees - owner_paid.iter().sum::<u128>()
        );
        assert_eq!(group.vault, group.c_tot + group.insurance);
        assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(
            group.assets[0].oi_eff_long_q + group.assets[0].oi_eff_short_q,
            0
        );
        assert_eq!(group.assets[1].oi_eff_long_q, quantity.unsigned_abs());
        assert_eq!(group.assets[1].oi_eff_short_q, quantity.unsigned_abs());
        for actor in 0..2 {
            assert_eq!(
                accounts[actor].owner,
                self.owners[actor].pubkey().to_bytes()
            );
            assert_eq!(
                accounts[actor].capital.get(),
                DEPOSITS[actor] - fees - owner_paid[actor]
            );
            assert_eq!(accounts[actor].pnl.get(), 0);
            if quantity == 0 {
                assert!(!has_active_leg_for_asset(&accounts[actor], 1));
            } else {
                assert_eq!(
                    active_leg_for_asset(&accounts[actor], 1).basis_pos_q,
                    quantity * if actor == 0 { 1 } else { -1 }
                );
            }
            for (key, owner, amount) in [
                (
                    self.tokens[actor],
                    self.owners[actor].pubkey(),
                    owner_paid[actor],
                ),
                (
                    self.destinations[actor],
                    self.recipients[actor].pubkey(),
                    paid[actor],
                ),
            ] {
                let raw = env.svm.get_account(&key).unwrap();
                assert_eq!(raw.owner, spl_token::ID);
                let token = TokenAccount::unpack(&raw.data).unwrap();
                assert_eq!(
                    (token.owner, token.mint, u128::from(token.amount)),
                    (owner, env.mint, amount)
                );
            }
        }
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(u128::from(mint.supply), DEPOSITS.iter().sum::<u128>());
        assert_eq!(
            group.vault + paid.iter().sum::<u128>() + owner_paid.iter().sum::<u128>(),
            u128::from(mint.supply)
        );
        assert_market_stock_census(
            "retained redirect entitlement",
            &group,
            &env.svm.get_account(&env.market).unwrap().data,
            &accounts,
            group.vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("retained redirect entitlement", &group, &accounts)
            .unwrap();
    }
}

#[test]
fn v16_retained_fee_routes_preserve_recipient_entitlement_after_paid_redirect_history() {
    let mut peak_cu = 0;
    let mut worlds = 0;
    let mut endpoint = None;
    let earned = REDIRECTS.map(credits);
    assert_ne!(
        earned[0][0], earned[0][1],
        "odd redirect rounding is observable"
    );
    assert_eq!(fee(FILLED), 48);
    for (open_route, partial) in [(0, false), (1, false), (2, false), (3, false), (2, true)] {
        for direction in [-1, 1] {
            for order in [[0, 1], [1, 0]] {
                let mut f = Fixture::new();
                let close_route = (open_route + 2) % 4;
                let open = f.trade(
                    open_route,
                    direction * if partial { REQUEST } else { FILLED },
                );
                let retained = f.sign(&[open]);
                let open_bytes = bincode::serialize(&retained).unwrap();
                let before = f.frame(&retained);
                f.env
                    .svm
                    .simulate_transaction(retained.clone().into())
                    .unwrap();
                assert_eq!(f.frame(&retained), before);
                f.env.update_trade_fee_policy_with_cu(CAP_BPS);
                f.env.update_fee_redirect_policy_with_cu(REDIRECTS[0]);
                if partial {
                    f.control(true);
                }
                f.check(0, 0, [0; 4], [0; 2], [0; 2]);
                assert_eq!(bincode::serialize(&retained).unwrap(), open_bytes);
                peak_cu =
                    peak_cu.max(f.deliver(retained, false, 1, 0, usize::from(open_route >= 2)));
                f.check(direction * FILLED, fee(FILLED), earned[0], [0; 2], [0; 2]);
                if partial {
                    let context = f.env.svm.get_account(&f.context).unwrap();
                    let response =
                        percolator_prog::matcher_abi::read_matcher_return(&context.data).unwrap();
                    assert_eq!(response.exec_size, direction * FILLED);
                    assert_ne!(
                        response.flags & percolator_prog::matcher_abi::FLAG_PARTIAL_OK,
                        0
                    );
                    f.control(false);
                }

                let mut budgets = earned[0];
                let mut paid = [0; 2];
                for asset in order {
                    let amount = budgets[2 * asset] + budgets[2 * asset + 1];
                    let tx = f.sign(&[f.insurance(asset, amount)]);
                    peak_cu = peak_cu.max(f.deliver(tx, false, 1, 1, 0));
                    paid[asset] += amount;
                    budgets[2 * asset..2 * asset + 2].fill(0);
                    f.check(direction * FILLED, fee(FILLED), budgets, paid, [0; 2]);
                }

                if close_route >= 2 {
                    // The bilateral opening revokes the standing grant. The LP authorizes
                    // this new episode before either complete close message is signed.
                    f.env.set_matcher_config_with_trade_fee_cap(
                        f.matcher,
                        &f.owners[1],
                        f.portfolios[1],
                        f.context,
                        f.delegate,
                        1,
                        CAP_BPS as u16,
                    );
                }
                let grant = state::read_portfolio_matcher_config(
                    &f.env.svm.get_account(&f.portfolios[1]).unwrap().data,
                )
                .unwrap();
                assert_eq!(u64::from(grant.trade_fee_cap_bps()), CAP_BPS);
                let close = f.trade(close_route, -direction * FILLED);
                let redirect = f.redirect(REDIRECTS[1]);
                let [first, second] = order;
                let amounts = [earned[1][0] + earned[1][1], earned[1][2] + earned[1][3]];
                let prefix = [redirect, close, f.insurance(first, amounts[first] - 1)];
                let mut bad = prefix.to_vec();
                bad.push(f.insurance(second, amounts[second] + 1));
                let rejected = f.sign(&bad);
                let mut good = prefix.to_vec();
                good.push(f.insurance(second, amounts[second]));
                let accepted = f.sign(&good);
                let signed_bytes = bincode::serialize(&accepted).unwrap();
                let before = f.frame(&accepted);
                f.env
                    .svm
                    .simulate_transaction(accepted.clone().into())
                    .unwrap();
                assert_eq!(f.frame(&accepted), before);
                // Independent policy lanes may change while these complete messages are retained.
                // Both values remain within the original owners' bounds.
                f.env.update_trade_fee_policy_with_cu(7);
                f.env.update_trade_fee_policy_with_cu(CAP_BPS);
                f.check(direction * FILLED, fee(FILLED), [0; 4], paid, [0; 2]);
                let control = f.env.control_sequences(0);
                let epochs = f.portfolios.map(|key| f.env.portfolio_position_epoch(key));
                // The attempted final debit fits total insurance exactly, but includes one
                // atom still owed to the already-partially-paid peer. Global stock is not consent.
                assert_eq!(2 * fee(FILLED) - (amounts[first] - 1), amounts[second] + 1);
                peak_cu =
                    peak_cu.max(f.deliver(rejected, true, 3, 1, usize::from(close_route >= 2)));
                assert_eq!(f.env.control_sequences(0), control);
                assert_eq!(
                    f.portfolios.map(|key| f.env.portfolio_position_epoch(key)),
                    epochs
                );
                f.check(direction * FILLED, fee(FILLED), [0; 4], paid, [0; 2]);
                assert_eq!(bincode::serialize(&accepted).unwrap(), signed_bytes);
                peak_cu =
                    peak_cu.max(f.deliver(accepted, false, 4, 2, usize::from(close_route >= 2)));
                assert_eq!(
                    f.env.control_sequences(0).fee_redirect,
                    control.fee_redirect + 1
                );
                assert_eq!(
                    f.env.market_state().0.fee_redirect_to_market_0_bps,
                    REDIRECTS[1]
                );
                let final_grant = state::read_portfolio_matcher_config(
                    &f.env.svm.get_account(&f.portfolios[1]).unwrap().data,
                )
                .unwrap();
                assert_eq!(final_grant.position_epoch(), grant.position_epoch() + 1);
                assert_eq!(final_grant.trade_fee_cap_bps(), grant.trade_fee_cap_bps());
                assert_eq!(final_grant.matcher_program, grant.matcher_program);
                assert_eq!(final_grant.matcher_context, grant.matcher_context);
                assert_eq!(final_grant.matcher_delegate, grant.matcher_delegate);
                if close_route >= 2 {
                    assert_eq!(final_grant.enabled(), grant.enabled());
                } else {
                    assert_eq!(final_grant.enabled(), 0);
                    assert_eq!(
                        state::read_portfolio_matcher_expiry(
                            &f.env.svm.get_account(&f.portfolios[1]).unwrap().data,
                        )
                        .unwrap(),
                        0
                    );
                }
                assert_eq!(
                    f.portfolios.map(|key| f.env.portfolio_position_epoch(key)),
                    epochs.map(|epoch| epoch + 1)
                );
                budgets = [0; 4];
                budgets[2 * first + 1] = 1;
                paid[first] += amounts[first] - 1;
                paid[second] += amounts[second];
                f.check(0, 2 * fee(FILLED), budgets, paid, [0; 2]);
                let tx = f.sign(&[f.insurance(first, 1)]);
                peak_cu = peak_cu.max(f.deliver(tx, false, 1, 1, 0));
                paid[first] += 1;
                f.check(0, 2 * fee(FILLED), [0; 4], paid, [0; 2]);

                let mut owner_paid = [0; 2];
                for actor in order {
                    let amount = DEPOSITS[actor] - 2 * fee(FILLED);
                    let tx = f.sign(&[Instruction {
                        program_id: f.env.program_id,
                        accounts: vec![
                            AccountMeta::new(f.owners[actor].pubkey(), true),
                            AccountMeta::new(f.env.market, false),
                            AccountMeta::new(f.portfolios[actor], false),
                            AccountMeta::new(f.tokens[actor], false),
                            AccountMeta::new(f.env.vault, false),
                            AccountMeta::new_readonly(f.env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: f.env.withdraw_ix(f.portfolios[actor], amount).encode(),
                    }]);
                    peak_cu = peak_cu.max(f.deliver(tx, false, 1, 1, 0));
                    owner_paid[actor] = amount;
                    f.check(0, 2 * fee(FILLED), [0; 4], paid, owner_paid);
                }
                let actual = f
                    .tokens
                    .into_iter()
                    .chain(f.destinations)
                    .map(|key| f.env.token_amount(key))
                    .collect::<Vec<_>>();
                if let Some(expected) = &endpoint {
                    assert_eq!(&actual, expected);
                } else {
                    endpoint = Some(actual);
                }
                assert_eq!(f.env.token_amount(f.env.vault), 0);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 20);
    println!("INV-014 retained redirect: {worlds} worlds, 40 simulations, 20 exact rollbacks, 80 final owner/recipient entitlements; peak CU={peak_cu}");
}
