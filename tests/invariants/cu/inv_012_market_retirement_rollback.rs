//! INV-001/007/012/021/024/080: whole-market retirement is atomic with retained consent.
//! Closing both funded portfolios, resolving, and tombstoning the market must all
//! roll back when same-address initialization rejects. Original signed CPI consent
//! remains executable; committed retirement returns principal and rent exactly.

use super::*;
use generation_bundle_rollback::sign;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn sign_retirement(h: &History, instructions: &[Instruction]) -> Transaction {
    // Six retirement instructions already request 1.2M CU by default. Omitting
    // the redundant limit instruction keeps the InitMarket bundle within 1,232 bytes.
    let mut ixs = vec![heap_ix()];
    ixs.extend_from_slice(instructions);
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&h.env.payer.pubkey()),
        &[&h.env.payer, &h.env.admin, &h.owners[0], &h.owners[1]],
        h.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn retirement(h: &History, destination: Pubkey, reverse: bool) -> Vec<Instruction> {
    let mut instructions = Vec::new();
    for actor in if reverse { [1, 0] } else { [0, 1] } {
        let portfolio = h.portfolios[actor];
        let owner = h.owners[actor].pubkey();
        instructions.push(Instruction {
            program_id: h.env.program_id,
            accounts: vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(h.tokens[actor], false),
                AccountMeta::new(h.env.vault, false),
                AccountMeta::new_readonly(h.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: h.env.withdraw_ix(portfolio, CAPITAL).encode(),
        });
        instructions.push(Instruction {
            program_id: h.env.program_id,
            accounts: vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(h.env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            data: ProgInstruction::ClosePortfolio {
                portfolio_id: h.env.portfolio_id(portfolio),
                expected_sequence: h.env.portfolio_matcher_sequence(portfolio) + 1,
                position_epoch: h.epoch,
            }
            .encode(),
        });
    }
    let authority_epoch = h.env.control_sequences(0).authority_epoch;
    instructions.push(Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.env.admin.pubkey(), true),
            AccountMeta::new(h.env.market, false),
        ],
        data: ProgInstruction::ResolveMarket {
            asset_generation_frontier: h.next_id,
            authority_epoch,
        }
        .encode(),
    });
    instructions.push(Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.env.admin.pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.env.vault, false),
            AccountMeta::new_readonly(h.env.vault_authority, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseSlab { authority_epoch }.encode(),
    });
    instructions
}

fn initialize(h: &History, market: Pubkey) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.env.admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new_readonly(h.env.mint, false),
        ],
        data: init_market_instruction(&V16CuMarketParams {
            max_portfolio_assets: 3,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        })
        .encode(),
    }
}

#[test]
fn v16_program_market_retirement_rollback_preserves_retained_cpi_and_attributed_exit() {
    let mut evidence = Evidence::default();
    let mut rollback_cu = 0;
    let mut retirement_cu = 0;
    let mut fresh_market_cu = 0;
    let mut packet_bytes = 0;
    for route in [Route::Single(1), Route::Batch] {
        for direction in [-1i128, 1] {
            let mut h = History::new();
            let admin_token = create_ata_for_test(
                &mut h.env.svm,
                &h.env.payer,
                h.env.admin.pubkey(),
                h.env.mint,
            );
            let sizes = match route {
                Route::Single(_) => [0, direction, 0],
                Route::Batch => [0, direction, -2 * direction],
            };
            let retained = h.sign(&h.instruction(route, sizes, direction < 0));
            retained.verify().unwrap();
            let retained_bytes = bincode::serialize(&retained).unwrap();
            h.simulate(&retained, 0, &mut evidence);

            let prefix = retirement(&h, admin_token, direction < 0);
            let positive = sign_retirement(&h, &prefix);
            let mut instructions = prefix;
            instructions.push(initialize(&h, h.env.market));
            let rejected = sign_retirement(&h, &instructions);
            packet_bytes = packet_bytes.max(bincode::serialized_size(&rejected).unwrap());
            let keys = rejected
                .message
                .account_keys
                .iter()
                .copied()
                .chain(retained.message.account_keys.iter().copied())
                .collect::<std::collections::BTreeSet<_>>();
            let mut before = keys
                .into_iter()
                .map(|key| (key, h.env.svm.get_account(&key)))
                .collect::<Vec<_>>();
            h.env
                .svm
                .simulate_transaction(positive.into())
                .expect("both payouts, both closes, resolution and CloseSlab are admissible");
            for (key, account) in &before {
                assert_eq!(h.env.svm.get_account(key), *account);
            }
            let fee = FeeStructure::default().lamports_per_signature
                * u64::from(rejected.message.header.num_required_signatures);
            let failure = h
                .env
                .svm
                .send_transaction(rejected)
                .expect_err("a retired market address cannot be initialized again");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    7,
                    InstructionError::Custom(PercolatorError::AlreadyInitialized as u32),
                )
            );
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", h.env.program_id))
                    .count(),
                6,
                "all retirement instructions executed before the rejection"
            );
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", spl_token::ID))
                    .count(),
                3,
                "two SPL payouts and vault closure must execute"
            );
            for (key, account) in &mut before {
                if *key == h.env.payer.pubkey() {
                    account.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(
                    h.env.svm.get_account(key),
                    *account,
                    "exact bytes, account length, authority, SPL and rent rollback: {key}"
                );
            }
            h.assert_state();
            rollback_cu = rollback_cu.max(failure.meta.compute_units_consumed);

            assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
            let fill = h.env.svm.send_transaction(retained).unwrap();
            assert!(fill
                .logs
                .iter()
                .any(|line| *line == format!("Program {} success", h.matcher.0)));
            h.positions = sizes;
            h.epoch += 1;
            evidence.fills += 1;
            evidence.fill_cu = evidence.fill_cu.max(fill.compute_units_consumed);
            h.assert_state();
            h.fill(route, sizes.map(|size| -size), direction > 0, &mut evidence);

            // Only the preceding committed trades update the final close's episode bindings.
            let final_tx = sign_retirement(&h, &retirement(&h, admin_token, direction < 0));
            let owner_before = h
                .owners
                .each_ref()
                .map(|owner| h.env.svm.get_account(&owner.pubkey()).unwrap());
            let portfolio_rent = h
                .portfolios
                .map(|key| h.env.svm.get_account(&key).unwrap().lamports);
            let mut admin_before = h.env.svm.get_account(&h.env.admin.pubkey()).unwrap();
            let mut payer_before = h.env.svm.get_account(&h.env.payer.pubkey()).unwrap();
            let market_before = h.env.svm.get_account(&h.env.market).unwrap();
            let vault_rent = h.env.svm.get_account(&h.env.vault).unwrap().lamports;
            let mint_before = h.env.svm.get_account(&h.env.mint).unwrap();
            let context_before = h.env.svm.get_account(&h.matcher.1);
            let delegate_before = h.env.svm.get_account(&h.matcher.2);
            let admin_token_before = h.env.svm.get_account(&admin_token);
            payer_before.lamports -= FeeStructure::default().lamports_per_signature
                * u64::from(final_tx.message.header.num_required_signatures);
            let closed = h.env.svm.send_transaction(final_tx).unwrap();
            retirement_cu = retirement_cu.max(closed.compute_units_consumed);
            for actor in 0..2 {
                assert_eq!(
                    h.env.svm.get_account(&h.owners[actor].pubkey()).unwrap(),
                    owner_before[actor]
                );
                assert!(h
                    .env
                    .svm
                    .get_account(&h.portfolios[actor])
                    .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                let token =
                    TokenAccount::unpack(&h.env.svm.get_account(&h.tokens[actor]).unwrap().data)
                        .unwrap();
                assert_eq!(token.owner, h.owners[actor].pubkey());
                assert_eq!(token.mint, h.env.mint);
                assert_eq!(token.amount as u128, CAPITAL);
            }
            let tombstone = h.env.svm.get_account(&h.env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            let rent = h
                .env
                .svm
                .minimum_balance_for_rent_exemption(tombstone.data.len());
            assert_eq!(tombstone.lamports, rent);
            assert!(h
                .env
                .svm
                .get_account(&h.env.vault)
                .is_none_or(
                    |account| account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
                ));
            // Portfolio closure sweeps its lamports into the slab before CloseSlab's refund.
            admin_before.lamports +=
                market_before.lamports - rent + vault_rent + portfolio_rent.iter().sum::<u64>();
            assert_eq!(
                h.env.svm.get_account(&h.env.admin.pubkey()).unwrap(),
                admin_before
            );
            assert_eq!(
                h.env.svm.get_account(&h.env.payer.pubkey()).unwrap(),
                payer_before
            );
            assert_eq!(h.env.svm.get_account(&admin_token), admin_token_before);
            assert_eq!(h.env.svm.get_account(&h.env.mint).unwrap(), mint_before);
            assert_eq!(h.env.svm.get_account(&h.matcher.1), context_before);
            assert_eq!(h.env.svm.get_account(&h.matcher.2), delegate_before);

            let fresh = Keypair::new();
            system_create_account_for_test(
                &mut h.env.svm,
                &h.env.payer,
                &fresh,
                market_before.data.len(),
                h.env.program_id,
            );
            let fresh_tx = sign(&h, &[initialize(&h, fresh.pubkey())]);
            let fresh_before = h.env.svm.get_account(&fresh.pubkey()).unwrap();
            let mut protected = before
                .iter()
                .map(|(key, _)| (*key, h.env.svm.get_account(key)))
                .collect::<Vec<_>>();
            let fee = FeeStructure::default().lamports_per_signature
                * u64::from(fresh_tx.message.header.num_required_signatures);
            let initialized = h.env.svm.send_transaction(fresh_tx).unwrap();
            fresh_market_cu = fresh_market_cu.max(initialized.compute_units_consumed);
            for (key, account) in &mut protected {
                if *key == h.env.payer.pubkey() {
                    account.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(
                    h.env.svm.get_account(key),
                    *account,
                    "fresh market isolation: {key}"
                );
            }
            let fresh_account = h.env.svm.get_account(&fresh.pubkey()).unwrap();
            assert_eq!(fresh_account.owner, fresh_before.owner);
            assert_eq!(fresh_account.lamports, fresh_before.lamports);
            assert_eq!(fresh_account.data.len(), fresh_before.data.len());
            assert_eq!(fresh_account.executable, fresh_before.executable);
            assert_eq!(fresh_account.rent_epoch, fresh_before.rent_epoch);
            let (cfg, group) = state::read_market(&fresh_account.data).unwrap();
            assert_eq!(cfg.marketauth, h.env.admin.pubkey().to_bytes());
            assert_eq!(cfg.collateral_mint, h.env.mint.to_bytes());
            assert_eq!(cfg.next_portfolio_id, 1);
            assert_eq!(group.mode, MarketModeV16::Live);
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(group.c_tot, 0);
            assert_eq!(group.vault, 0);
            assert_eq!(group.insurance, 0);
            assert_eq!(h.env.svm.get_account(&h.env.market).unwrap(), tombstone);
            assert_eq!(h.env.svm.get_account(&h.env.mint).unwrap(), mint_before);
            evidence.worlds += 1;
        }
    }
    assert_eq!(evidence.worlds, 4);
    assert_eq!(evidence.fills, 8);
    assert_eq!(evidence.live_simulations, 4);
    for cu in [
        rollback_cu,
        retirement_cu,
        fresh_market_cu,
        evidence.fill_cu,
    ] {
        assert_cu_within(
            "whole-market retirement and retained consent",
            cu,
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        );
    }
    eprintln!("INV-001/012 market retirement: worlds=4, rollbacks=4, retained_fills=4, total_fills=8, owner_payouts=8, committed_retirements=4, fresh_markets=4, packet_bytes={packet_bytes}, rollback_cu={rollback_cu}, retirement_cu={retirement_cu}, fill_cu={}, fresh_market_cu={fresh_market_cu}", evidence.fill_cu);
}
