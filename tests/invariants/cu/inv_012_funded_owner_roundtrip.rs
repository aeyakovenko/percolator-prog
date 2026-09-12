//! INV-003/007/012/024/080: a funded A-B-A portfolio history cannot revive A's
//! retained grant, even after its owner, delegate tuple and sequence coincide.
//! A current CPI prefix must roll back with that stale grant's late rejection.

use super::*;
use generation_bundle_rollback::sign;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

struct Oracle {
    id: u64,
    next_id: u64,
    sequence: u64,
    epochs: [u64; 2],
    positions: [i128; 3],
    capital: [u128; 2],
    wallets: [u128; 3],
    requests: u64,
}

impl Oracle {
    fn check(&self, h: &History, wallets: [Pubkey; 3], owners: [Pubkey; 3]) {
        let (cfg, group) = h.env.market_state();
        assert_eq!(cfg.next_portfolio_id, self.next_id);
        assert_eq!(cfg.matcher_req_seq, self.requests);
        assert_eq!(group.next_market_id, 4);
        assert_eq!(h.env.portfolio_id(h.portfolios[0]), 1);
        assert_eq!(h.env.portfolio_id(h.portfolios[1]), self.id);
        assert_eq!(
            h.env.portfolio_matcher_sequence(h.portfolios[1]),
            self.sequence
        );
        for actor in 0..2 {
            let p = h.env.portfolio_state(h.portfolios[actor]);
            assert_eq!(p.capital.get(), self.capital[actor]);
            assert_eq!(p.pnl.get(), 0);
            assert_eq!(
                h.env.portfolio_position_epoch(h.portfolios[actor]),
                self.epochs[actor]
            );
            let account = h.env.svm.get_account(&h.portfolios[actor]).unwrap();
            assert_eq!(
                state::read_portfolio_owner_preflight(&account.data)
                    .unwrap()
                    .1,
                h.owners[actor].pubkey().to_bytes()
            );
            for asset in 0..3 {
                assert_eq!(
                    has_active_leg_for_asset(&p, asset),
                    self.positions[asset] != 0
                );
                if self.positions[asset] != 0 {
                    let leg = active_leg_for_asset(&p, asset);
                    assert_eq!(leg.market_id, asset as u64 + 1);
                    assert_eq!(
                        leg.basis_pos_q,
                        self.positions[asset] * if actor == 0 { 1 } else { -1 }
                    );
                }
                assert_eq!(group.assets[asset].market_id, asset as u64 + 1);
                assert_eq!(
                    group.assets[asset].oi_eff_long_q,
                    self.positions[asset].unsigned_abs()
                );
                assert_eq!(
                    group.assets[asset].oi_eff_short_q,
                    self.positions[asset].unsigned_abs()
                );
            }
        }
        let grant = h.env.portfolio_matcher_config(h.portfolios[1]);
        assert_eq!(grant.enabled(), 1);
        assert_eq!(grant.trade_fee_cap_bps(), FEE_CAP);
        assert_eq!(grant.matcher_program, h.matcher.0.to_bytes());
        assert_eq!(grant.matcher_context, h.matcher.1.to_bytes());
        assert_eq!(grant.matcher_delegate, h.matcher.2.to_bytes());
        assert_eq!(h.env.portfolio_matcher_expiry(h.portfolios[1]), EXPIRY);
        assert_eq!(h.env.svm.get_sysvar::<Clock>().slot, SLOT);
        assert_eq!(group.materialized_portfolio_count, 2);
        assert_eq!(group.c_tot, self.capital.iter().sum::<u128>());
        assert_eq!(group.vault, group.c_tot);
        assert_eq!(group.insurance, 0);
        assert_eq!(h.env.token_amount(h.env.vault) as u128, group.vault);
        for i in 0..3 {
            let account = h.env.svm.get_account(&wallets[i]).unwrap();
            let token = TokenAccount::unpack(&account.data).unwrap();
            assert_eq!(token.owner, owners[i]);
            assert_eq!(token.mint, h.env.mint);
            assert_eq!(token.amount as u128, self.wallets[i]);
        }
        assert_eq!(group.vault + self.wallets.iter().sum::<u128>(), 3 * CAPITAL);
        assert_eq!(
            Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
                .unwrap()
                .supply as u128,
            3 * CAPITAL
        );
    }
}

fn grant(h: &History, id: u64, sequence: u64) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new_readonly(h.env.market, false),
            AccountMeta::new(h.portfolios[1], false),
            AccountMeta::new_readonly(h.matcher.0, false),
            AccountMeta::new_readonly(h.matcher.1, false),
            AccountMeta::new_readonly(h.matcher.2, false),
        ],
        data: ProgInstruction::SetMatcherConfig {
            portfolio_id: id,
            expected_sequence: sequence,
            enabled: 1,
            trade_fee_cap_bps: FEE_CAP,
            expiry_slot: EXPIRY,
        }
        .encode(),
    }
}

fn custody(h: &History, actor: usize, deposit: bool) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(h.owners[actor].pubkey(), true),
        AccountMeta::new(h.env.market, false),
        AccountMeta::new(h.portfolios[actor], false),
        AccountMeta::new(h.tokens[actor], false),
        AccountMeta::new(h.env.vault, false),
    ];
    if !deposit {
        accounts.push(AccountMeta::new_readonly(h.env.vault_authority, false));
    }
    accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
    Instruction {
        program_id: h.env.program_id,
        accounts,
        data: if deposit {
            h.env.deposit_ix(h.portfolios[actor], CAPITAL)
        } else {
            h.env.withdraw_ix(h.portfolios[actor], CAPITAL)
        }
        .encode(),
    }
}

fn cpi(h: &History, ix: &ProgInstruction) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[0].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.portfolios[1], false),
            AccountMeta::new_readonly(h.matcher.0, false),
            AccountMeta::new(h.matcher.1, false),
            AccountMeta::new_readonly(h.matcher.2, false),
        ],
        data: ix.encode(),
    }
}

fn owner_context(h: &mut History) -> (Pubkey, Pubkey, Pubkey) {
    let program = h.matcher.0;
    let context = Keypair::new();
    system_create_account_for_test(
        &mut h.env.svm,
        &h.env.payer,
        &context,
        MATCHER_CONTEXT_LEN,
        program,
    );
    let delegate = matcher_delegate_key(
        &h.env.program_id,
        &h.env.market,
        &h.portfolios[1],
        &h.owners[1].pubkey(),
        &program,
        &context.pubkey(),
    );
    send_raw_tx(
        &mut h.env.svm,
        &h.env.payer,
        Instruction {
            program_id: program,
            accounts: vec![
                AccountMeta::new_readonly(h.owners[1].pubkey(), true),
                AccountMeta::new_readonly(delegate, false),
                AccountMeta::new(context.pubkey(), false),
                AccountMeta::new_readonly(h.env.program_id, false),
                AccountMeta::new_readonly(h.env.market, false),
                AccountMeta::new_readonly(h.portfolios[1], false),
            ],
            data: vec![2],
        },
        &[&h.owners[1]],
    )
    .expect("owner initializes its canonical matcher context through public instructions");
    (program, context.pubkey(), delegate)
}

fn land(
    h: &mut History,
    tx: Transaction,
    protected: &[Pubkey],
    changed: &[Pubkey],
    failure: Option<u8>,
) -> u64 {
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    let keys = tx
        .message
        .account_keys
        .iter()
        .copied()
        .chain(protected.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    let before = keys
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect::<Vec<_>>();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = h.env.svm.send_transaction(tx);
    let meta = if let Some(index) = failure {
        let rejected =
            result.expect_err("old incarnation grant rejects after the current CPI prefix");
        assert_eq!(
            rejected.err,
            TransactionError::InstructionError(
                index,
                InstructionError::Custom(PercolatorError::EngineStale as u32)
            )
        );
        assert_eq!(
            rejected
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", h.env.program_id))
                .count(),
            usize::from(index - 2)
        );
        assert_eq!(
            rejected
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", h.matcher.0))
                .count(),
            usize::from(index - 2)
        );
        rejected.meta
    } else {
        result.expect("public history continuation")
    };
    for (key, mut account) in before {
        if key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        } else if failure.is_none() && changed.contains(&key) {
            continue;
        }
        assert_eq!(
            h.env.svm.get_account(&key),
            account,
            "complete Account frame: {key}"
        );
    }
    assert_cu_within(
        "funded owner roundtrip",
        meta.compute_units_consumed,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    meta.compute_units_consumed
}

#[test]
fn v16_program_funded_owner_roundtrip_rejects_old_grant_after_current_cpi_prefix() {
    let mut peak_cu = 0;
    let mut worlds = 0;
    for route in [Route::Single(1), Route::Batch] {
        for direction in [-1i128, 1] {
            let mut h = History::new();
            let owner_a = h.owners[1].insecure_clone();
            let owner_b = Keypair::new();
            h.env.svm.airdrop(&owner_b.pubkey(), 1_000_000_000).unwrap();
            let token_b =
                create_ata_for_test(&mut h.env.svm, &h.env.payer, owner_b.pubkey(), h.env.mint);
            send_raw_tx(
                &mut h.env.svm,
                &h.env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &h.env.mint,
                    &token_b,
                    &h.env.admin.pubkey(),
                    &[],
                    CAPITAL as u64,
                )
                .unwrap(),
                &[&h.env.admin],
            )
            .unwrap();
            let wallets = [h.tokens[0], h.tokens[1], token_b];
            let owners = [h.owners[0].pubkey(), owner_a.pubkey(), owner_b.pubkey()];
            let matcher_a = h.matcher;
            let original_context = h.env.svm.get_account(&matcher_a.1);
            let mut protected = vec![
                h.env.market,
                h.env.mint,
                h.env.vault,
                h.env.admin.pubkey(),
                h.portfolios[0],
                h.portfolios[1],
            ];
            protected.extend(wallets);
            protected.extend(owners);
            protected.extend([matcher_a.0, matcher_a.1, matcher_a.2]);
            let mut oracle = Oracle {
                id: 2,
                next_id: 3,
                sequence: 3,
                epochs: [0; 2],
                positions: [0; 3],
                capital: [CAPITAL; 2],
                wallets: [0, 0, CAPITAL],
                requests: 0,
            };
            oracle.check(&h, wallets, owners);
            let retained_ix = grant(&h, 2, 3);
            let retained = sign(&h, &[retained_ix.clone()]);
            let retained_bytes = bincode::serialize(&retained).unwrap();
            let before = protected
                .iter()
                .map(|key| h.env.svm.get_account(key))
                .collect::<Vec<_>>();
            h.env
                .svm
                .simulate_transaction(retained.clone().into())
                .expect("A's grant is admissible before either incarnation change");
            assert_eq!(
                protected
                    .iter()
                    .map(|key| h.env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                before
            );

            let sizes = match route {
                Route::Single(_) => [0, direction * POS_SCALE as i128, 0],
                Route::Batch => [
                    0,
                    direction * POS_SCALE as i128,
                    -2 * direction * POS_SCALE as i128,
                ],
            };
            // Predict two fresh portfolio IDs and B's two committed fills from inputs.
            let mut future_fill = h.instruction(route, sizes, direction < 0);
            match &mut future_fill {
                ProgInstruction::TradeCpi {
                    account_a_position_epoch,
                    account_b_portfolio_id,
                    ..
                }
                | ProgInstruction::BatchTradeCpi {
                    account_a_position_epoch,
                    account_b_portfolio_id,
                    ..
                } => {
                    *account_a_position_epoch = 2;
                    *account_b_portfolio_id = 4;
                }
                _ => unreachable!(),
            }
            let current = h.sign(&future_fill);
            let current_bytes = bincode::serialize(&current).unwrap();
            let retained_bundle = sign(&h, &[cpi(&h, &future_fill), retained_ix.clone()]);
            let bundle_bytes = bincode::serialize(&retained_bundle).unwrap();
            for cycle in 0..2 {
                // B opens and exits a real position episode between the two A incarnations.
                if cycle == 1 {
                    for delta in [sizes, sizes.map(|q| -q)] {
                        let tx = h.sign(&h.instruction(route, delta, direction < 0));
                        assert_eq!(
                            tx.signatures.len(),
                            2,
                            "the LP does not sign CPI consumption"
                        );
                        let changed = [h.env.market, h.portfolios[0], h.portfolios[1], h.matcher.1];
                        peak_cu = peak_cu.max(land(&mut h, tx, &protected, &changed, None));
                        oracle.requests += 1;
                        oracle.epochs = oracle.epochs.map(|epoch| epoch + 1);
                        for asset in 0..3 {
                            oracle.positions[asset] += delta[asset];
                        }
                        oracle.check(&h, wallets, owners);
                    }
                }
                let tx = sign(&h, &[custody(&h, 1, false)]);
                let changed = [h.env.market, h.portfolios[1], h.tokens[1], h.env.vault];
                peak_cu = peak_cu.max(land(&mut h, tx, &protected, &changed, None));
                oracle.capital[1] = 0;
                oracle.wallets[cycle + 1] = CAPITAL;
                oracle.sequence += 1;
                oracle.check(&h, wallets, owners);
                let owner_accounts = vec![
                    AccountMeta::new(h.owners[1].pubkey(), true),
                    AccountMeta::new(h.env.market, false),
                    AccountMeta::new(h.portfolios[1], false),
                ];
                let close = Instruction {
                    program_id: h.env.program_id,
                    accounts: owner_accounts,
                    data: ProgInstruction::ClosePortfolio {
                        portfolio_id: oracle.id,
                        expected_sequence: oracle.sequence,
                        position_epoch: oracle.epochs[1],
                    }
                    .encode(),
                };
                let market_lamports = h.env.svm.get_account(&h.env.market).unwrap().lamports;
                let rent = h.env.svm.get_account(&h.portfolios[1]).unwrap().lamports;
                let tx = sign(&h, &[close]);
                let changed = [h.env.market, h.portfolios[1]];
                peak_cu = peak_cu.max(land(&mut h, tx, &protected, &changed, None));
                assert_eq!(
                    h.env.svm.get_account(&h.env.market).unwrap().lamports,
                    market_lamports + rent
                );
                assert_eq!(h.env.market_state().0.next_portfolio_id, oracle.next_id);
                let closed = h.env.svm.get_account(&h.portfolios[1]).unwrap();
                assert_eq!(closed.lamports, 0);
                assert!(closed.data.iter().all(|byte| *byte == 0));
                h.owners[1] = if cycle == 0 {
                    owner_b.insecure_clone()
                } else {
                    owner_a.insecure_clone()
                };
                h.tokens[1] = wallets[if cycle == 0 { 2 } else { 1 }];
                let rent = h
                    .env
                    .svm
                    .minimum_balance_for_rent_exemption(h.env.portfolio_account_len);
                let initialize = Instruction {
                    program_id: h.env.program_id,
                    accounts: vec![
                        AccountMeta::new(h.owners[1].pubkey(), true),
                        AccountMeta::new(h.env.market, false),
                        AccountMeta::new(h.portfolios[1], false),
                    ],
                    data: ProgInstruction::InitPortfolio.encode(),
                };
                let tx = sign(
                    &h,
                    &[
                        system_instruction::transfer(&h.owners[1].pubkey(), &h.portfolios[1], rent),
                        initialize,
                    ],
                );
                let owner_lamports = h
                    .env
                    .svm
                    .get_account(&h.owners[1].pubkey())
                    .unwrap()
                    .lamports;
                let changed = [h.owners[1].pubkey(), h.env.market, h.portfolios[1]];
                peak_cu = peak_cu.max(land(&mut h, tx, &protected, &changed, None));
                assert_eq!(
                    h.env
                        .svm
                        .get_account(&h.owners[1].pubkey())
                        .unwrap()
                        .lamports,
                    owner_lamports - rent
                );
                assert_eq!(h.env.portfolio_matcher_config(h.portfolios[1]).enabled(), 0);
                assert_eq!(h.env.portfolio_matcher_sequence(h.portfolios[1]), 0);
                assert_eq!(h.env.portfolio_position_epoch(h.portfolios[1]), 0);
                oracle.id = oracle.next_id;
                oracle.next_id += 1;
                oracle.epochs[1] = 0;
                if cycle == 0 {
                    h.matcher = owner_context(&mut h);
                    assert_ne!(h.matcher.2, matcher_a.2);
                    protected.extend([h.matcher.0, h.matcher.1, h.matcher.2]);
                } else {
                    h.matcher = matcher_a;
                }
                let tx = sign(&h, &[grant(&h, oracle.id, 0)]);
                let changed = [h.portfolios[1]];
                peak_cu = peak_cu.max(land(&mut h, tx, &protected, &changed, None));
                let tx = sign(&h, &[custody(&h, 1, true), grant(&h, oracle.id, 2)]);
                let changed = [h.env.market, h.portfolios[1], h.tokens[1], h.env.vault];
                peak_cu = peak_cu.max(land(&mut h, tx, &protected, &changed, None));
                oracle.capital[1] = CAPITAL;
                oracle.wallets[if cycle == 0 { 2 } else { 1 }] = 0;
                oracle.sequence = 3;
                oracle.check(&h, wallets, owners);
                assert_eq!(h.env.svm.get_account(&matcher_a.1), original_context);
            }

            assert_eq!(oracle.id, 4);
            assert_eq!(oracle.epochs, [2, 0]);
            assert_eq!(h.matcher, matcher_a);
            assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
            assert_eq!(
                h.env.svm.latest_blockhash(),
                retained.message.recent_blockhash
            );
            // All original grant fields and metas coincide except the portfolio ID.
            let fresh_grant = grant(&h, 4, 3);
            assert_eq!(retained_ix.accounts, fresh_grant.accounts);
            let mut repaired = ProgInstruction::decode(&retained_ix.data).unwrap();
            if let ProgInstruction::SetMatcherConfig { portfolio_id, .. } = &mut repaired {
                *portfolio_id = 4;
            } else {
                unreachable!();
            }
            assert_eq!(repaired.encode(), fresh_grant.data);
            assert_eq!(
                future_fill.encode(),
                h.instruction(route, sizes, direction < 0).encode()
            );
            let cpi = cpi(&h, &future_fill);
            let before = protected
                .iter()
                .map(|key| h.env.svm.get_account(key))
                .collect::<Vec<_>>();
            h.env
                .svm
                .simulate_transaction(sign(&h, &[cpi.clone(), fresh_grant.clone()]).into())
                .expect(
                    "changing only the grant's portfolio ID admits the entire CPI/grant bundle",
                );
            assert_eq!(
                protected
                    .iter()
                    .map(|key| h.env.svm.get_account(key))
                    .collect::<Vec<_>>(),
                before
            );
            peak_cu = peak_cu.max(land(&mut h, retained, &protected, &[], Some(2)));
            assert_eq!(bincode::serialize(&retained_bundle).unwrap(), bundle_bytes);
            peak_cu = peak_cu.max(land(&mut h, retained_bundle, &protected, &[], Some(3)));
            oracle.check(&h, wallets, owners);
            assert_eq!(bincode::serialize(&current).unwrap(), current_bytes);
            assert_eq!(current.signatures.len(), 2);
            let changed = [h.env.market, h.portfolios[0], h.portfolios[1], h.matcher.1];
            peak_cu = peak_cu.max(land(&mut h, current, &protected, &changed, None));
            oracle.requests += 1;
            oracle.epochs = oracle.epochs.map(|epoch| epoch + 1);
            oracle.positions = sizes;
            oracle.check(&h, wallets, owners);
            let tx = sign(&h, &[fresh_grant]);
            let changed = [h.portfolios[1]];
            peak_cu = peak_cu.max(land(&mut h, tx, &protected, &changed, None));
            oracle.sequence += 1;
            oracle.check(&h, wallets, owners);
            let tx = h.sign(&h.instruction(route, sizes.map(|q| -q), direction < 0));
            let changed = [h.env.market, h.portfolios[0], h.portfolios[1], h.matcher.1];
            peak_cu = peak_cu.max(land(&mut h, tx, &protected, &changed, None));
            oracle.requests += 1;
            oracle.epochs = oracle.epochs.map(|epoch| epoch + 1);
            oracle.positions = [0; 3];
            oracle.check(&h, wallets, owners);
            for actor in 0..2 {
                let tx = sign(&h, &[custody(&h, actor, false)]);
                let changed = [
                    h.env.market,
                    h.portfolios[actor],
                    h.tokens[actor],
                    h.env.vault,
                ];
                peak_cu = peak_cu.max(land(&mut h, tx, &protected, &changed, None));
                oracle.capital[actor] = 0;
                oracle.wallets[actor] = CAPITAL;
                if actor == 1 {
                    oracle.sequence += 1;
                }
                oracle.check(&h, wallets, owners);
            }
            assert_eq!(oracle.wallets, [CAPITAL; 3]);
            worlds += 1;
        }
    }
    assert_eq!(worlds, 4);
    eprintln!("INV-012 funded owner roundtrip: worlds=4, used_B_incarnations=4, stale_grant_rejections=8, CPI_prefix_rollbacks=4, CPI_fills=16, principal_payouts=16, peak_cu={peak_cu}");
}
