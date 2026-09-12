//! INV-008/024/031/064/080/081, row428: stale insurance consent rolls back
//! passive stock creation, including a fee-exhausted portfolio's rent-bearing
//! deletion. Fresh consent retries the unchanged fee/payout bundle successfully.
//! The bound epoch is the authority epoch; intrinsic stock binding remains OPEN.

use super::*;

const ORIGINAL: u128 = 7;
const RATE: u128 = 7;
const ELAPSED: u64 = 5;

fn deliver(
    env: &mut V16Svm,
    tx: Transaction,
    stale_index: Option<u8>,
    transfers: usize,
    evidence: &mut Evidence,
) {
    let before = frame(env, &tx);
    let mut payer = env
        .svm
        .get_account(&env.actors[PAYER].signer.pubkey())
        .unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let instructions = tx.message.instructions.len() - 2;
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(index) = stale_index {
        let failure = result.expect_err("superseded consent must roll back fee-created stock");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                index,
                InstructionError::Custom(PercolatorError::EngineStale as u32)
            )
        );
        for (key, account) in before {
            if key != env.actors[PAYER].signer.pubkey() {
                assert_eq!(
                    env.svm.get_account(&key),
                    account,
                    "exact rollback at {key}"
                );
            }
        }
        evidence.rollbacks += 1;
        evidence.transfers_rolled_back += transfers;
        evidence.peak_rejection_cu = evidence
            .peak_rejection_cu
            .max(failure.meta.compute_units_consumed);
        failure.meta
    } else {
        let success = result.expect("current fee and insurance continuation succeeds");
        evidence.successes += 1;
        evidence.peak_success_cu = evidence.peak_success_cu.max(success.compute_units_consumed);
        success
    };
    assert_eq!(
        env.svm.get_account(&env.actors[PAYER].signer.pubkey()),
        Some(payer)
    );
    for (program, count) in [
        (spl_token::ID, transfers),
        (
            env.program_id,
            stale_index.map_or(instructions, |index| usize::from(index - 2)),
        ),
    ] {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count
        );
    }
    assert!(meta.compute_units_consumed > 0 && meta.compute_units_consumed <= 300_000);
}

#[test]
fn v16_program_stale_insurance_retry_restores_fee_stock_and_exhausted_portfolio() {
    let mut evidence = Evidence::default();
    for capital in [101u128, 17] {
        let mut env = V16Svm::new(
            [capital as u8; 32],
            MarketConfig {
                maintenance_fee_per_slot: RATE,
                actor_deposits: [0, 0, capital, 0, 0],
                ..MarketConfig::default()
            },
        );
        for (asset, operator) in [(0, OPERATOR), (1, PEER)] {
            for (role, owner) in [
                (processor::ASSET_AUTH_INSURANCE, FUNDER),
                (processor::ASSET_AUTH_INSURANCE_OPERATOR, operator),
            ] {
                env.update_asset_authority_from_admin(asset, role, owner)
                    .unwrap();
            }
        }
        for (domain, amount) in [(0, ORIGINAL), (3, PEER_STOCK)] {
            env.top_up_insurance_domain_for_actor(FUNDER, domain, amount)
                .unwrap();
        }
        let fee_slot = env.primary_portfolio(SUCCESSOR).last_fee_slot.get();
        assert_eq!(fee_slot, env.current_slot());
        let due_slot = fee_slot + ELAPSED;
        let fee = capital.min(RATE * u128::from(ELAPSED));
        assert!(
            fee >= 2 * ORIGINAL,
            "stale retry remains fully funded after fresh payout"
        );
        let deleted = fee == capital;
        let sequences =
            std::array::from_fn::<_, ASSET_COUNT, _>(|i| env.primary_control_sequences(i));
        let profiles = std::array::from_fn::<_, ASSET_COUNT, _>(|i| env.primary_profile(i));
        let source = env.actors[SUCCESSOR].portfolio;
        let portfolio_before = env.svm.get_account(&source).unwrap();
        let market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
        let token_keys: Vec<_> = std::iter::once(env.vault)
            .chain(
                env.actors
                    .iter()
                    .flat_map(|a| [a.source_token, a.destination_token]),
            )
            .collect();
        let tokens: Vec<_> = token_keys
            .iter()
            .map(|key| env.svm.get_account(key).unwrap())
            .collect();
        let mint = env.svm.get_account(&env.mint).unwrap();
        let bystanders: Vec<_> = env
            .all_economic_account_lamports()
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| *key != env.market && *key != source && !token_keys.contains(key))
            .map(|key| (key, env.svm.get_account(&key)))
            .collect();

        let check =
            |env: &V16Svm, charged: bool, rotated: bool, paid: [u128; PRIMARY_ACTOR_COUNT]| {
                let charged_fee = if charged { fee } else { 0 };
                let owner_capital = capital - charged_fee - paid[SUCCESSOR];
                let mut budgets = [0; ASSET_COUNT * 2];
                budgets[0] = ORIGINAL + charged_fee / 2;
                budgets[1] = charged_fee - charged_fee / 2;
                let long_paid = paid[OPERATOR].min(budgets[0]);
                budgets[0] -= long_paid;
                budgets[1] -= paid[OPERATOR] - long_paid;
                budgets[3] = PEER_STOCK - paid[PEER];
                let insurance = budgets.iter().sum::<u128>();
                let vault = owner_capital + insurance;
                let group = env.primary_market_state().1;
                assert_eq!(group.mode, percolator::MarketModeV16::Live);
                assert_eq!(
                    (group.c_tot, group.insurance, group.vault),
                    (owner_capital, insurance, vault)
                );
                assert_eq!(group.insurance_domain_budget, budgets);
                assert_eq!(group.insurance_domain_budget_remaining_total, insurance);
                assert_eq!(group.insurance_domain_spent, [0; ASSET_COUNT * 2]);
                assert_eq!(
                    group.materialized_portfolio_count,
                    PRIMARY_ACTOR_COUNT as u64 - u64::from(charged && deleted)
                );
                assert_eq!(group.pnl_pos_tot, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
                for asset in 0..ASSET_COUNT {
                    let mut expected = sequences[asset];
                    let mut profile = profiles[asset];
                    if asset == 0 && rotated {
                        expected.authority_epoch += 1;
                        profile.insurance_authority =
                            env.actors[SUCCESSOR].signer.pubkey().to_bytes();
                    }
                    assert_eq!(env.primary_control_sequences(asset), expected);
                    assert_eq!(env.primary_profile(asset), profile);
                }
                if charged && deleted {
                    assert_eq!(env.svm.get_account(&source).map_or(0, |a| a.lamports), 0);
                } else {
                    let p = env.primary_portfolio(SUCCESSOR);
                    assert_eq!(p.capital.get(), owner_capital);
                    assert_eq!(
                        p.last_fee_slot.get(),
                        if charged { due_slot } else { fee_slot }
                    );
                    assert_eq!(p.pnl.get(), 0);
                    assert_eq!(
                        env.svm.get_account(&source).unwrap().lamports,
                        portfolio_before.lamports
                    );
                }
                assert_eq!(
                    env.svm.get_account(&env.market).unwrap().lamports,
                    market_lamports
                        + if charged && deleted {
                            portfolio_before.lamports
                        } else {
                            0
                        }
                );
                for (key, original) in token_keys.iter().zip(&tokens) {
                    let mut expected = original.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    if *key == env.vault {
                        token.amount = vault.try_into().unwrap();
                    } else if let Some(actor) =
                        env.actors.iter().position(|a| a.destination_token == *key)
                    {
                        token.amount += u64::try_from(paid[actor]).unwrap();
                    }
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(key), Some(expected));
                }
                for (key, account) in &bystanders {
                    assert_eq!(
                        env.svm.get_account(key),
                        *account,
                        "unrelated economic account {key}"
                    );
                }
                assert_eq!(env.svm.get_account(&env.mint), Some(mint.clone()));
                assert_eq!(env.token_supply_observed(), env.initial_token_supply);
                assert_eq!(u128::from(env.mint_supply()), env.initial_token_supply);
                assert_eq!(
                    vault + paid.iter().sum::<u128>(),
                    capital + ORIGINAL + PEER_STOCK
                );
                assert_public_stock_census("insurance fee reclassification", env).unwrap();
                assert_public_encumbrance_census("insurance fee reclassification", env).unwrap();
            };

        let stale_ix = withdraw(&env, 0, OPERATOR, ORIGINAL);
        let retained = sign(&env, &[stale_ix.clone()], 101);
        let retained_wire = bincode::serialize(&retained).unwrap();
        let peer = sign(&env, &[withdraw(&env, 1, PEER, PEER_STOCK)], 102);
        let peer_wire = bincode::serialize(&peer).unwrap();
        simulate(&mut env, &retained, &mut evidence);
        let mut paid = [0; PRIMARY_ACTOR_COUNT];
        check(&env, false, false, paid);
        let first = sign(&env, &[stale_ix.clone()], 103);
        deliver(&mut env, first, None, 1, &mut evidence);
        paid[OPERATOR] = ORIGINAL;
        check(&env, false, false, paid);
        env.update_asset_authority_between_actors(
            0,
            processor::ASSET_AUTH_INSURANCE,
            FUNDER,
            SUCCESSOR,
        )
        .unwrap();
        check(&env, false, true, paid);
        env.warp_to_slot(due_slot);

        // Only authenticated elapsed time supplies new insurance: no custody
        // transfer or top-up intent supplies or authorizes the reclassified stock.
        let sync = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
            ],
            data: ProgInstruction::SyncMaintenanceFee { now_slot: u64::MAX }.encode(),
        };
        let fresh_ix = withdraw(&env, 0, OPERATOR, ORIGINAL);
        assert_ne!(fresh_ix.data, stale_ix.data);
        assert_eq!(fresh_ix.accounts, stale_ix.accounts);
        let fresh = sign(&env, &[sync.clone(), fresh_ix.clone()], 104);
        let fresh_wire = bincode::serialize(&fresh).unwrap();
        simulate(&mut env, &fresh, &mut evidence);
        for (nonce, ixs, index, transfers) in [
            (105, vec![sync.clone(), stale_ix.clone()], 3, 0),
            (106, vec![sync, fresh_ix, stale_ix], 4, 1),
        ] {
            let tx = sign(&env, &ixs, nonce);
            deliver(&mut env, tx, Some(index), transfers, &mut evidence);
            check(&env, false, true, paid);
        }
        assert_eq!(bincode::serialize(&fresh).unwrap(), fresh_wire);
        deliver(&mut env, fresh, None, 1, &mut evidence);
        paid[OPERATOR] += ORIGINAL;
        check(&env, true, true, paid);
        assert_eq!(bincode::serialize(&retained).unwrap(), retained_wire);
        deliver(&mut env, retained, Some(2), 0, &mut evidence);
        check(&env, true, true, paid);

        let remaining = fee - ORIGINAL;
        let finish = sign(&env, &[withdraw(&env, 0, OPERATOR, remaining)], 107);
        deliver(&mut env, finish, None, 1, &mut evidence);
        paid[OPERATOR] += remaining;
        check(&env, true, true, paid);
        assert_eq!(bincode::serialize(&peer).unwrap(), peer_wire);
        deliver(&mut env, peer, None, 1, &mut evidence);
        paid[PEER] = PEER_STOCK;
        check(&env, true, true, paid);
        if !deleted {
            env.withdraw_primary(SUCCESSOR, capital - fee).unwrap();
            paid[SUCCESSOR] = capital - fee;
        }
        check(&env, true, true, paid);
        assert_eq!(env.token_amount(env.vault), 0);
    }
    assert_eq!(
        (
            evidence.simulations,
            evidence.successes,
            evidence.rollbacks,
            evidence.transfers_rolled_back
        ),
        (4, 8, 6, 2)
    );
    println!("INV-008 insurance fee reclassification: 2 histories, 4 simulations, 6 exact rollbacks, 2 rolled-back SPL payouts, 2 rolled-back portfolio deletions; peak success CU {}, rejection CU {}; row428 OPEN", evidence.peak_success_cu, evidence.peak_rejection_cu);
}
