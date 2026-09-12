//! INV-005/024/025/027/036/070/080/081: operator succession does not transfer
//! terminal insurance entitlement, including when both operators leave at shutdown.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeSet;

const CAPITAL: u128 = 701;
const INSURANCE: u128 = 53;
const PEER_INSURANCE: u128 = 29;
const PRINCIPAL: [u128; 2] = [37, 61];
const RATE: u128 = 2;
const RESOLVE_SLOT: u64 = 7;
const SUPPLY: u128 = CAPITAL + INSURANCE + PEER_INSURANCE + PRINCIPAL[0] + PRINCIPAL[1];
const OLD_PAID: u128 = 7;
const NEW_PAID: u128 = 11;
const TERMINAL_PREFIX: u128 = 17;

struct Stocks {
    tokens: [Pubkey; 8],
    wallets: [u128; 8],
    insurance: [u128; 4],
    backing: [u128; 4],
    capital: u128,
    portfolios: u64,
    roles: [[Pubkey; 4]; 2],
    epochs: [u64; 2],
    mode: MarketModeV16,
}

impl Stocks {
    fn pay_insurance(&mut self, asset: usize, recipient: usize, amount: u128) {
        let long = self.insurance[2 * asset].min(amount);
        self.insurance[2 * asset] -= long;
        self.insurance[2 * asset + 1] -= amount - long;
        self.wallets[recipient] += amount;
    }

    fn check(&self, env: &V16CuEnv) {
        let (cfg, group) = env.market_state();
        assert_eq!(cfg.marketauth, env.admin.pubkey().to_bytes());
        assert_eq!(group.mode, self.mode);
        let insurance = self.insurance.iter().sum::<u128>();
        let vault = self.capital + insurance + self.backing.iter().sum::<u128>();
        assert_eq!(group.c_tot, self.capital);
        assert_eq!(group.materialized_portfolio_count, self.portfolios);
        assert_eq!(group.insurance, insurance);
        assert_eq!(group.insurance_domain_budget_remaining_total, insurance);
        assert_eq!(group.insurance_domain_budget, self.insurance);
        assert_eq!(group.insurance_domain_spent, [0; 4]);
        assert_eq!(group.vault, vault);
        assert_eq!(env.token_amount(env.vault) as u128, vault);
        assert_eq!(group.pnl_pos_tot, 0);
        assert_eq!(group.backing_provider_earnings_total, 0);
        assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
        for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                self.backing[domain] * BOUND_SCALE
            );
            assert_eq!(bucket.valid_liened_backing_num, 0);
            assert_eq!(bucket.consumed_liened_backing_num, 0);
            assert_eq!(bucket.impaired_liened_backing_num, 0);
            assert_eq!(bucket.utilization_fee_earnings, 0);
        }
        for (token, amount) in self.tokens.iter().zip(self.wallets) {
            assert_eq!(env.token_amount(*token) as u128, amount, "wallet {token}");
        }
        assert_eq!(vault + self.wallets.iter().sum::<u128>(), SUPPLY);
        let mint = env.svm.get_account(&env.mint).unwrap();
        let mint = Mint::unpack(&mint.data).unwrap();
        assert_eq!(mint.supply as u128, SUPPLY);
        assert_eq!(mint.mint_authority, COption::None);
        let mut data = env.svm.get_account(&env.market).unwrap().data;
        for asset in 0..2 {
            let profile = state::read_asset_oracle_profile(&data, asset).unwrap();
            assert_eq!(
                [
                    profile.insurance_authority,
                    profile.insurance_operator,
                    profile.backing_bucket_authority,
                    profile.asset_admin
                ],
                self.roles[asset].map(|key| key.to_bytes()),
            );
            assert_eq!(
                env.control_sequences(asset).authority_epoch,
                self.epochs[asset]
            );
        }
        state::market_view_mut(&mut data)
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
    }
}

fn instruction(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn transaction(env: &V16CuEnv, instructions: &[Instruction], signers: &[&Keypair]) -> Transaction {
    let mut ixs = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(1_200_000),
    ];
    ixs.extend_from_slice(instructions);
    let mut all_signers = vec![&env.payer];
    all_signers.extend_from_slice(signers);
    Transaction::new_signed_with_payer(
        &ixs,
        Some(&env.payer.pubkey()),
        &all_signers,
        env.svm.latest_blockhash(),
    )
}

// Include every compiled account and every economic account, even absent accounts.
// On success, also frame every account outside the transaction's writable set.
#[track_caller]
fn execute(
    env: &mut V16CuEnv,
    tracked: &[Pubkey],
    tx: Transaction,
    rejection: Option<(u8, PercolatorError)>,
) {
    let keys: BTreeSet<_> = tracked
        .iter()
        .copied()
        .chain(tx.message.account_keys.iter().copied())
        .collect();
    let before: Vec<_> = keys
        .iter()
        .map(|key| (*key, env.svm.get_account(key)))
        .collect();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let writable: BTreeSet<_> = tx
        .message
        .account_keys
        .iter()
        .enumerate()
        .filter(|(index, _)| tx.message.is_writable(*index))
        .map(|(_, key)| *key)
        .collect();
    let result = env.svm.send_transaction(tx);
    let rejected = rejection.is_some();
    if let Some((index, error)) = rejection {
        let failure = result.expect_err("role/stock boundary must reject");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index + 2, InstructionError::Custom(error as u32),),
            "{failure:?}"
        );
    } else {
        let meta = result.expect("normal public conformance transition");
        assert!(meta.compute_units_consumed <= 1_200_000);
    }
    for (key, mut expected) in before {
        if rejected || !writable.contains(&key) {
            if key == env.payer.pubkey() {
                expected.as_mut().unwrap().lamports -= fee;
            }
            assert_eq!(env.svm.get_account(&key), expected, "account frame {key}");
        }
    }
}

fn rotate(
    env: &V16CuEnv,
    asset: u16,
    kind: u8,
    current: Pubkey,
    incoming: Pubkey,
    epoch: u64,
) -> Instruction {
    instruction(
        env,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: epoch,
            kind,
            new_pubkey: incoming.to_bytes(),
        },
        vec![
            AccountMeta::new(current, true),
            AccountMeta::new_readonly(incoming, incoming != Pubkey::default()),
            AccountMeta::new(env.market, false),
        ],
    )
}

fn reserve(
    env: &V16CuEnv,
    asset: u16,
    authority: Pubkey,
    destination: Pubkey,
    amount: u128,
    ledger: Option<Pubkey>,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(authority, true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(destination, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    if let Some(ledger) = ledger {
        accounts.push(AccountMeta::new(ledger, false));
    }
    instruction(
        env,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: env.control_sequences(asset as usize).authority_epoch,
            amount,
        },
        accounts,
    )
}

fn slab_close(env: &V16CuEnv, destination: Pubkey) -> Instruction {
    instruction(
        env,
        ProgInstruction::CloseSlab {
            authority_epoch: env.control_sequences(0).authority_epoch,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ],
    )
}

#[test]
fn v16_program_shutdown_operator_departure_preserves_terminal_beneficiary_and_backing() {
    use super::super::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_capacity;

    for asset in [0u16, 1] {
        for handoff_after_shutdown in [false, true] {
            let peer = 1 - asset;
            let mut env = inv018_public_spl_market_with_capacity(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    maintenance_fee_per_slot: RATE,
                    ..V16CuMarketParams::default()
                },
                2,
            );
            let beneficiary = Keypair::new();
            let peer_beneficiary = Keypair::new();
            let provider = Keypair::new();
            let old_operator = Keypair::new();
            let new_operator = Keypair::new();
            let cold_admin = Keypair::new();
            let owner = Keypair::new();
            let actors = [
                beneficiary.pubkey(),
                peer_beneficiary.pubkey(),
                provider.pubkey(),
                old_operator.pubkey(),
                new_operator.pubkey(),
                owner.pubkey(),
                cold_admin.pubkey(),
                env.admin.pubkey(),
            ];
            for key in &actors[..7] {
                env.svm.airdrop(key, 1_000_000_000).unwrap();
            }
            let tokens =
                actors.map(|key| create_ata_for_test(&mut env.svm, &env.payer, key, env.mint));
            let insurers = if asset == 0 {
                [beneficiary.pubkey(), peer_beneficiary.pubkey()]
            } else {
                [peer_beneficiary.pubkey(), beneficiary.pubkey()]
            };
            let operators = if asset == 0 {
                [old_operator.pubkey(), peer_beneficiary.pubkey()]
            } else {
                [peer_beneficiary.pubkey(), old_operator.pubkey()]
            };
            env.configure_permissionless_resolve_with_cu(10_000, 5);
            for configured_asset in [0, 1] {
                for (kind, incoming) in [
                    (
                        processor::ASSET_AUTH_INSURANCE,
                        if asset == configured_asset {
                            &beneficiary
                        } else {
                            &peer_beneficiary
                        },
                    ),
                    (
                        processor::ASSET_AUTH_INSURANCE_OPERATOR,
                        if asset == configured_asset {
                            &old_operator
                        } else {
                            &peer_beneficiary
                        },
                    ),
                    (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
                ] {
                    let ix = rotate(
                        &env,
                        configured_asset,
                        kind,
                        env.admin.pubkey(),
                        incoming.pubkey(),
                        env.control_sequences(configured_asset as usize)
                            .authority_epoch,
                    );
                    let tx = transaction(&env, &[ix], &[&env.admin, incoming]);
                    execute(&mut env, &tokens, tx, None);
                }
            }
            let ix = rotate(
                &env,
                asset,
                processor::ASSET_AUTH_ADMIN,
                env.admin.pubkey(),
                cold_admin.pubkey(),
                env.control_sequences(asset as usize).authority_epoch,
            );
            let tx = transaction(&env, &[ix], &[&env.admin, &cold_admin]);
            execute(&mut env, &tokens, tx, None);

            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            let portfolio = portfolio.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&owner],
            )
            .unwrap();
            env.portfolios.push(portfolio);
            let ledger = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &ledger,
                state::insurance_ledger_account_len(),
                env.program_id,
            );
            let ledger = ledger.pubkey();
            let mut tracked = tokens.to_vec();
            tracked.extend(actors);
            tracked.extend([
                env.market,
                env.mint,
                env.vault,
                env.payer.pubkey(),
                portfolio,
                ledger,
            ]);
            for (token, amount) in [
                (tokens[0], INSURANCE),
                (tokens[1], PEER_INSURANCE),
                (tokens[2], PRINCIPAL.iter().sum()),
                (tokens[5], CAPITAL),
            ] {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &token,
                        &env.admin.pubkey(),
                        &[],
                        amount as u64,
                    )
                    .unwrap(),
                    &[&env.admin],
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
            let mut stocks = Stocks {
                tokens,
                wallets: [
                    INSURANCE,
                    PEER_INSURANCE,
                    PRINCIPAL.iter().sum(),
                    0,
                    0,
                    CAPITAL,
                    0,
                    0,
                ],
                insurance: [0; 4],
                backing: [0; 4],
                capital: 0,
                portfolios: 1,
                roles: std::array::from_fn(|index| {
                    [
                        insurers[index],
                        operators[index],
                        provider.pubkey(),
                        if index == asset as usize {
                            cold_admin.pubkey()
                        } else {
                            env.admin.pubkey()
                        },
                    ]
                }),
                epochs: std::array::from_fn(|index| env.control_sequences(index).authority_epoch),
                mode: MarketModeV16::Live,
            };
            stocks.check(&env);
            for (index, target, signer) in [(0, asset, &beneficiary), (1, peer, &peer_beneficiary)]
            {
                let amount = stocks.wallets[index];
                let mut accounts = vec![
                    AccountMeta::new(signer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(tokens[index], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ];
                if index == 0 {
                    accounts.push(AccountMeta::new(ledger, false));
                }
                let seq = env.control_sequences(target as usize);
                let ix = instruction(
                    &env,
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: target * 2,
                        market_id: env.asset_market_id(target),
                        authority_epoch: seq.authority_epoch,
                        intent_id: next_control_sequence(seq.insurance_top_up),
                        amount,
                    },
                    accounts,
                );
                let tx = transaction(&env, &[ix], &[signer]);
                execute(&mut env, &tracked, tx, None);
                stocks.wallets[index] = 0;
                stocks.insurance[target as usize * 2] = amount;
                stocks.check(&env);
            }
            for (side, amount) in PRINCIPAL.into_iter().enumerate() {
                let domain = asset * 2 + side as u16;
                let seq = env.control_sequences(asset as usize);
                let ix = instruction(
                    &env,
                    ProgInstruction::TopUpBackingBucket {
                        domain,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: seq.authority_epoch,
                        intent_id: next_control_sequence(seq.backing_top_up),
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount,
                        expiry_slot: 10_000,
                    },
                    vec![
                        AccountMeta::new(provider.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(tokens[2], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                );
                let tx = transaction(&env, &[ix], &[&provider]);
                execute(&mut env, &tracked, tx, None);
                stocks.wallets[2] -= amount;
                stocks.backing[domain as usize] = amount;
                stocks.check(&env);
            }
            let deposit = instruction(
                &env,
                env.deposit_ix(portfolio, CAPITAL),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(tokens[5], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            let tx = transaction(&env, &[deposit], &[&owner]);
            execute(&mut env, &tracked, tx, None);
            stocks.wallets[5] = 0;
            stocks.capital = CAPITAL;
            stocks.check(&env);
            let paid = reserve(
                &env,
                asset,
                old_operator.pubkey(),
                tokens[3],
                OLD_PAID,
                Some(ledger),
            );
            let tx = transaction(&env, &[paid], &[&old_operator]);
            execute(&mut env, &tracked, tx, None);
            stocks.pay_insurance(asset as usize, 3, OLD_PAID);
            stocks.check(&env);

            env.svm.warp_to_slot(3);
            if handoff_after_shutdown {
                env.update_asset_lifecycle_as_admin_with_cu(
                    processor::ASSET_ACTION_SHUTDOWN,
                    asset,
                    3,
                    0,
                );
                stocks.check(&env);
            }
            let epoch = env.control_sequences(asset as usize).authority_epoch;
            let handoff = rotate(
                &env,
                asset,
                processor::ASSET_AUTH_INSURANCE_OPERATOR,
                old_operator.pubkey(),
                new_operator.pubkey(),
                epoch,
            );
            let burn = rotate(
                &env,
                asset,
                processor::ASSET_AUTH_ADMIN,
                cold_admin.pubkey(),
                Pubkey::default(),
                epoch + 1,
            );
            let mut paid = reserve(
                &env,
                asset,
                new_operator.pubkey(),
                tokens[4],
                NEW_PAID,
                Some(ledger),
            );
            paid.data = ProgInstruction::WithdrawInsuranceAsset {
                asset_index: asset,
                market_id: env.asset_market_id(asset),
                authority_epoch: epoch + 2,
                amount: NEW_PAID,
            }
            .encode();
            let mut former = reserve(
                &env,
                asset,
                old_operator.pubkey(),
                tokens[3],
                1,
                Some(ledger),
            );
            former.data = ProgInstruction::WithdrawInsuranceAsset {
                asset_index: asset,
                market_id: env.asset_market_id(asset),
                authority_epoch: epoch + 2,
                amount: 1,
            }
            .encode();
            let prefix = [handoff, burn, paid];
            let mut rejected = prefix.to_vec();
            rejected.push(former);
            let tx = transaction(
                &env,
                &rejected,
                &[&old_operator, &new_operator, &cold_admin],
            );
            execute(
                &mut env,
                &tracked,
                tx,
                Some((3, PercolatorError::Unauthorized)),
            );
            stocks.check(&env);
            let tx = transaction(&env, &prefix, &[&old_operator, &new_operator, &cold_admin]);
            execute(&mut env, &tracked, tx, None);
            stocks.pay_insurance(asset as usize, 4, NEW_PAID);
            stocks.roles[asset as usize][1] = new_operator.pubkey();
            stocks.roles[asset as usize][3] = Pubkey::default();
            stocks.epochs[asset as usize] += 2;
            stocks.check(&env);
            assert_eq!(
                env.control_sequences(asset as usize).authority_epoch,
                epoch + 2
            );
            let data = env.svm.get_account(&env.market).unwrap().data;
            let profile = state::read_asset_oracle_profile(&data, asset as usize).unwrap();
            assert_eq!(profile.asset_admin, [0; 32]);
            assert_eq!(profile.insurance_operator, new_operator.pubkey().to_bytes());
            assert_eq!(profile.insurance_authority, beneficiary.pubkey().to_bytes());
            assert_eq!(
                profile.backing_bucket_authority,
                provider.pubkey().to_bytes()
            );
            if !handoff_after_shutdown {
                env.update_asset_lifecycle_as_admin_with_cu(
                    processor::ASSET_ACTION_SHUTDOWN,
                    asset,
                    3,
                    0,
                );
                stocks.check(&env);
            }

            // Retain only packets signed at departure. All later successful transactions
            // run after both operator keypairs and the cold-admin keypair are dropped.
            let beneficiary_prefix = reserve(
                &env,
                asset,
                beneficiary.pubkey(),
                tokens[0],
                TERMINAL_PREFIX,
                Some(ledger),
            );
            let departed_packets = [&old_operator, &new_operator].map(|operator| {
                let index = if operator.pubkey() == actors[3] { 3 } else { 4 };
                transaction(
                    &env,
                    &[
                        beneficiary_prefix.clone(),
                        reserve(
                            &env,
                            asset,
                            operator.pubkey(),
                            tokens[index],
                            1,
                            Some(ledger),
                        ),
                    ],
                    &[&beneficiary, operator],
                )
            });
            drop(old_operator);
            drop(new_operator);
            drop(cold_admin);
            env.svm.warp_to_slot(RESOLVE_SLOT);
            let resolve = instruction(
                &env,
                ProgInstruction::ResolveMarket {
                    asset_generation_frontier: env.market_state().1.next_market_id,
                    authority_epoch: env.control_sequences(0).authority_epoch,
                },
                vec![
                    AccountMeta::new(env.admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
            );
            let tx = transaction(&env, &[resolve], &[&env.admin]);
            execute(&mut env, &tracked, tx, None);
            stocks.mode = MarketModeV16::Resolved;
            stocks.check(&env);
            assert_eq!(env.market_state().1.resolved_slot, RESOLVE_SLOT);
            let tx = transaction(&env, &[beneficiary_prefix.clone()], &[&beneficiary]);
            execute(
                &mut env,
                &tracked,
                tx,
                Some((0, PercolatorError::EngineLockActive)),
            );
            stocks.check(&env);

            env.svm.warp_to_slot(70);
            let payout = instruction(
                &env,
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: RATE,
                },
                vec![
                    AccountMeta::new_readonly(owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(tokens[5], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            let tx = transaction(&env, &[payout], &[]);
            execute(&mut env, &tracked, tx, None);
            let fees = RATE * RESOLVE_SLOT as u128;
            stocks.capital = 0;
            stocks.wallets[5] = CAPITAL - fees;
            stocks.insurance[0] += fees / 2;
            stocks.insurance[1] += fees - fees / 2;
            stocks.check(&env);
            let close = instruction(
                &env,
                env.close_portfolio_ix(portfolio),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
            );
            let tx = transaction(&env, &[close], &[&owner]);
            execute(&mut env, &tracked, tx, None);
            stocks.portfolios = 0;
            stocks.check(&env);
            for packet in departed_packets {
                execute(
                    &mut env,
                    &tracked,
                    packet,
                    Some((1, PercolatorError::Unauthorized)),
                );
                stocks.check(&env);
            }
            // Scanning past the peer may commit progress before reaching fresh backing.
            // Put the bounded scan and its eventual denial in the same transaction.
            let mut premature_close = vec![beneficiary_prefix.clone()];
            premature_close.extend((0..=asset).map(|_| slab_close(&env, tokens[7])));
            let tx = transaction(&env, &premature_close, &[&beneficiary, &env.admin]);
            execute(
                &mut env,
                &tracked,
                tx,
                Some((asset as u8 + 1, PercolatorError::EngineLockActive)),
            );
            stocks.check(&env);
            env.svm.expire_blockhash();
            let tx = transaction(&env, &[beneficiary_prefix], &[&beneficiary]);
            execute(&mut env, &tracked, tx, None);
            stocks.pay_insurance(asset as usize, 0, TERMINAL_PREFIX);
            stocks.check(&env);
            for (target, index, signer) in [(asset, 0, &beneficiary), (peer, 1, &peer_beneficiary)]
            {
                let amount = stocks.insurance[target as usize * 2..target as usize * 2 + 2]
                    .iter()
                    .sum();
                let ix = reserve(
                    &env,
                    target,
                    signer.pubkey(),
                    tokens[index],
                    amount,
                    if index == 0 { Some(ledger) } else { None },
                );
                let tx = transaction(&env, &[ix], &[signer]);
                execute(&mut env, &tracked, tx, None);
                stocks.pay_insurance(target as usize, index, amount);
                stocks.check(&env);
            }
            let overdraw = reserve(
                &env,
                asset,
                beneficiary.pubkey(),
                tokens[0],
                1,
                Some(ledger),
            );
            let tx = transaction(&env, &[overdraw], &[&beneficiary]);
            execute(
                &mut env,
                &tracked,
                tx,
                Some((0, PercolatorError::EngineLockActive)),
            );
            stocks.check(&env);
            let ledger_state =
                state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
            assert_eq!(ledger_state.authority, beneficiary.pubkey().to_bytes());
            assert_eq!(ledger_state.market_group, env.market.to_bytes());
            assert_eq!(ledger_state.total_deposited_atoms, INSURANCE);
            assert_eq!(
                ledger_state.total_withdrawn_atoms,
                INSURANCE + if asset == 0 { fees } else { 0 }
            );
            assert_eq!(
                ledger_state.cumulative_profit_atoms,
                if asset == 0 { fees } else { 0 }
            );
            assert_eq!(ledger_state.total_principal_atoms, 0);
            assert_eq!(ledger_state.last_observed_insurance_atoms, 0);
            assert_eq!(ledger_state.cumulative_loss_atoms, 0);
            for (side, amount) in PRINCIPAL.into_iter().enumerate() {
                let domain = asset * 2 + side as u16;
                let ix = instruction(
                    &env,
                    ProgInstruction::WithdrawBackingBucket {
                        domain,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: env.control_sequences(asset as usize).authority_epoch,
                        amount,
                    },
                    vec![
                        AccountMeta::new(provider.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(tokens[2], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                );
                let tx = transaction(&env, &[ix], &[&provider]);
                execute(&mut env, &tracked, tx, None);
                stocks.backing[domain as usize] = 0;
                stocks.wallets[2] += amount;
                stocks.check(&env);
            }
            let market_before = env.svm.get_account(&env.market).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            let admin_before = env.svm.get_account(&env.admin.pubkey()).unwrap();
            let mint_before = env.svm.get_account(&env.mint).unwrap();
            let tx = transaction(&env, &[slab_close(&env, tokens[7])], &[&env.admin]);
            execute(&mut env, &tracked, tx, None);
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
            assert_eq!(
                env.svm.get_account(&env.admin.pubkey()).unwrap().lamports,
                admin_before.lamports + market_before.lamports + vault_before.lamports
                    - tombstone.lamports
            );
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|account| account.lamports == 0));
            for (token, amount) in stocks.tokens.iter().zip(stocks.wallets) {
                assert_eq!(env.token_amount(*token) as u128, amount);
            }
            assert_eq!(stocks.wallets.iter().sum::<u128>(), SUPPLY);
            assert_eq!(
                stocks.wallets[0],
                INSURANCE - OLD_PAID - NEW_PAID + if asset == 0 { fees } else { 0 }
            );
            assert_eq!(
                stocks.wallets[1],
                PEER_INSURANCE + if peer == 0 { fees } else { 0 }
            );
            eprintln!("asset={asset} handoff_after_shutdown={handoff_after_shutdown}: operator-paid=7/11 beneficiary={} peer={} provider=98 user=687; exact retirement",
                stocks.wallets[0], stocks.wallets[1]);
        }
    }
}
