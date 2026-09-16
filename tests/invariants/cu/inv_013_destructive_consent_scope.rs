//! INV-013 - Destructive-consent scope.
//!
//! Normative obligation: close, forfeit, recovery, liquidation, and reduction
//! consent must only apply to the exact market/asset/portfolio/position/claim
//! episode it was signed for. A later episode at the same visible pubkeys must
//! reject stale destructive authority before any mutation.
//!
//! Evidence in this file (I/C): the reduction route is exercised through the
//! deployed LiteSVM wrapper only. The test opens exposure, records the owner's
//! old position episode, closes and reopens through public trades, then submits
//! the old `RebalanceReduce` request. The stale request must reject with exact
//! market, portfolio, counterparty, and custody rollback; a current-episode
//! request must still reduce exposure, proving this is not a blanket user-exit
//! DoS.
//!
//! The funding-round-trip test additionally checks instruction-local close consent:
//! a deposit and withdrawal that restore the original balances still invalidate an
//! earlier close in the same transaction. Rejection rolls back both SPL transfers
//! and both sequence advances, preserving the original signed close. Committing
//! that same funding prefix instead requires fresh close consent.
//!
//! Guarantee boundary: this is the public SVM owner for reduction destructive consent.
//! INV-004's finding-blind episode matrix additionally owns Recovery forfeit, released-PnL
//! conversion, and close/cure episodes. INV-002 owns asset-generation shutdown/resolve scope;
//! INV-005 owns every configured-authority incarnation, and INV-001/007 permanently retire a
//! closed market address. Permissionless liquidation, abandoned-asset close, reset finalization,
//! and terminal payout derive their action from current state and carry no retained user consent.

use super::*;

fn inv013_source_defines_test(source: &str, function: &str) -> bool {
    let expected = format!("fn {function}");
    let mut test_attribute = false;

    for line in source.lines() {
        let line = line.trim();
        if line == "#[test]" {
            test_attribute = true;
        } else if line.starts_with("fn ") {
            if test_attribute
                && line
                    .strip_prefix(&expected)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            test_attribute = false;
        } else if test_attribute && !line.is_empty() && !line.starts_with("#") {
            test_attribute = false;
        }
    }

    false
}

#[test]
fn v16_program_close_consent_tracks_only_committed_funding_round_trips() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };
    use std::collections::BTreeSet;

    let mut peak_cu = 0;
    for amount in [1u64, 37] {
        for commit_funding in [false, true] {
            let mut env = inv018_public_spl_market(6);
            let owner = Keypair::new();
            env.ensure_signer_account(owner.pubkey());
            let portfolio_keypair = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_keypair,
                env.portfolio_account_len,
                env.program_id,
            );
            let portfolio = portfolio_keypair.pubkey();
            let owner_accounts = vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ];
            env.send(
                ProgInstruction::InitPortfolio,
                owner_accounts.clone(),
                &[&owner],
            )
            .expect("initialize the System-created empty portfolio");
            let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
            for ix in [
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &env.admin.pubkey(),
                    &[],
                    amount,
                )
                .unwrap(),
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &env.admin.pubkey(),
                    &[],
                )
                .unwrap(),
            ] {
                send_raw_tx(&mut env.svm, &env.payer, ix, &[&env.admin]).unwrap();
            }
            let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
            assert_eq!(mint.supply, amount);
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(
                TokenAccount::unpack(&env.svm.get_account(&token).unwrap().data)
                    .unwrap()
                    .amount,
                amount
            );
            let portfolio_id = env.portfolio_id(portfolio);
            let sequence = env.portfolio_matcher_sequence(portfolio);
            let position_epoch = env.portfolio_position_epoch(portfolio);
            let close = |expected_sequence| Instruction {
                program_id: env.program_id,
                accounts: owner_accounts.clone(),
                data: ProgInstruction::ClosePortfolio {
                    portfolio_id,
                    expected_sequence,
                    position_epoch,
                }
                .encode(),
            };
            let prefix = [
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(token, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::Deposit {
                        portfolio_id,
                        expected_sequence: sequence,
                        amount: amount.into(),
                    }
                    .encode(),
                },
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(token, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::Withdraw {
                        portfolio_id,
                        expected_sequence: sequence + 1,
                        amount: amount.into(),
                    }
                    .encode(),
                },
            ];
            let sign = |ixs: &[Instruction], nonce: u32| {
                let mut instructions = vec![
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(300_000 - nonce),
                ];
                instructions.extend_from_slice(ixs);
                let tx = Transaction::new_signed_with_payer(
                    &instructions,
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &owner],
                    env.svm.latest_blockhash(),
                );
                tx.verify().expect("valid retained close envelope");
                tx
            };
            let retained_close = sign(&[close(sequence)], 0);
            let retained_bytes = bincode::serialize(&retained_close).unwrap();
            let mut stale_bundle_ixs = prefix.to_vec();
            stale_bundle_ixs.push(close(sequence));
            let stale_bundle = sign(&stale_bundle_ixs, 1);
            let funding = sign(&prefix, 2);
            let fresh_close = sign(&[close(sequence + 2)], 3);
            let mut fresh_bundle_ixs = prefix.to_vec();
            fresh_bundle_ixs.push(close(sequence + 2));
            let fresh_bundle = sign(&fresh_bundle_ixs, 4);

            let unchanged = [
                env.mint,
                token,
                env.vault,
                owner.pubkey(),
                env.admin.pubkey(),
            ]
            .map(|key| (key, env.svm.get_account(&key)));
            let send = |env: &mut V16CuEnv, tx: Transaction, rejection: Option<(u8, usize)>| {
                let frame: Vec<_> = tx
                    .message
                    .account_keys
                    .iter()
                    .copied()
                    .chain(unchanged.iter().map(|(key, _)| *key))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(|key| (key, env.svm.get_account(&key)))
                    .collect();
                let fee = FeeStructure::default().lamports_per_signature
                    * u64::from(tx.message.header.num_required_signatures);
                let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                payer.lamports -= fee;
                let result = env.svm.send_transaction(tx);
                let meta = if let Some((index, prefix_successes)) = rejection {
                    let failure = result.expect_err("stale destructive consent must reject");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            index,
                            InstructionError::Custom(PercolatorError::EngineStale as u32)
                        )
                    );
                    for program in [env.program_id, spl_token::ID] {
                        assert_eq!(
                            failure.meta.logs.iter().filter(|line| {
                                **line == format!("Program {program} success")
                            }).count(),
                            prefix_successes,
                            "both funding instructions and SPL transfers must finish before rejection"
                        );
                    }
                    for (key, before) in frame {
                        if key != env.payer.pubkey() {
                            assert_eq!(env.svm.get_account(&key), before, "rollback at {key}");
                        }
                    }
                    failure.meta
                } else {
                    result.expect("current close consent or its funding prefix must execute")
                };
                assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                for (key, before) in &unchanged {
                    assert_eq!(env.svm.get_account(key), *before, "no net change at {key}");
                }
                assert_cu_within(
                    "INV-013 close consent",
                    meta.compute_units_consumed,
                    CUSTODY_CU_LIMIT,
                );
                meta.compute_units_consumed
            };

            // Both alternatives are signed before funding; simulation cannot consume consent.
            for tx in [retained_close.clone(), fresh_bundle] {
                let frame: Vec<_> = tx
                    .message
                    .account_keys
                    .iter()
                    .map(|key| (*key, env.svm.get_account(key)))
                    .collect();
                env.svm
                    .simulate_transaction(tx.into())
                    .expect("original close and fully rebound funding/close bundle are admissible");
                for (key, before) in frame {
                    assert_eq!(env.svm.get_account(&key), before);
                }
            }
            peak_cu = peak_cu.max(send(&mut env, stale_bundle, Some((4, 2))));
            assert_eq!(env.portfolio_matcher_sequence(portfolio), sequence);
            if commit_funding {
                peak_cu = peak_cu.max(send(&mut env, funding, None));
                assert_eq!(env.portfolio_matcher_sequence(portfolio), sequence + 2);
            }
            assert_eq!(env.portfolio_id(portfolio), portfolio_id);
            assert_eq!(env.portfolio_position_epoch(portfolio), position_epoch);
            assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
            assert_eq!(env.portfolio_state(portfolio).pnl.get(), 0);
            assert_eq!(env.market_state().1.vault, 0);
            assert_eq!(env.market_state().1.c_tot, 0);
            assert_eq!(env.market_state().1.materialized_portfolio_count, 1);
            assert_eq!(bincode::serialize(&retained_close).unwrap(), retained_bytes);

            let rent = env.svm.get_account(&portfolio).unwrap().lamports;
            let market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
            assert!(rent > 0);
            if commit_funding {
                peak_cu = peak_cu.max(send(&mut env, retained_close, Some((2, 0))));
                peak_cu = peak_cu.max(send(&mut env, fresh_close, None));
            } else {
                peak_cu = peak_cu.max(send(&mut env, retained_close, None));
            }
            assert!(env
                .svm
                .get_account(&portfolio)
                .map_or(true, |account| account.lamports == 0
                    && account.data.is_empty()));
            assert_eq!(
                env.svm.get_account(&env.market).unwrap().lamports,
                market_lamports + rent
            );
            assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
        }
    }
    eprintln!("INV-013 close consent: 4 histories, 4 two-transfer rollbacks, 2 preserved closes, 2 stale closes followed by fresh closes; peak CU={peak_cu}");
}

fn inv013_braced_body<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing source marker {marker}"));
    let open = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("missing opening brace for {marker}"));
    let mut depth = 0usize;
    for (offset, byte) in source[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated source item {marker}")
}

#[test]
fn v16_program_destructive_consent_composition_is_source_complete() {
    let source = include_str!("../../../src/v16_program.rs");
    let instruction_enum = inv013_braced_body(source, "pub enum Instruction {");
    let close_slab = inv013_braced_body(instruction_enum, "CloseSlab {");
    assert!(close_slab.contains("authority_epoch: u64"));

    let dispatch = inv013_braced_body(source, "pub fn process_instruction<'a>(");
    assert!(dispatch.contains(
        "Instruction::CloseSlab { authority_epoch } => {\n                handle_close_slab(program_id, accounts, authority_epoch)"
    ));
    let handler = inv013_braced_body(source, "fn handle_close_slab<'a>(");
    for guard in [
        "expect_signer(admin_dest)?;",
        "expect_writable(market_ai)?;",
        "expect_owner(market_ai, program_id)?;",
        "expect_live_authority(&cfg.marketauth, admin_dest.key)?;",
        "require_authority_epoch_view(&group, 0, expected_authority_epoch)?;",
        "market_ai.realloc(constants::HEADER_LEN, false)?;",
        "state::write_closed_market_tombstone",
    ] {
        assert!(handler.contains(guard), "CloseSlab lost guard {guard}");
    }

    let authority_evidence = include_str!("inv_005_authority_incarnation_binding.rs");
    assert!(authority_evidence.contains("let expected_open = std::collections::BTreeSet::new();"));
    let position_evidence = include_str!("inv_004_position_episode_binding.rs");
    let asset_evidence = include_str!("inv_002_asset_generation_binding.rs");
    let transaction_domain_evidence =
        include_str!("../public_sbf/inv_006_program_chain_message_type_and_version_binding.rs");
    let account_evidence = include_str!("../public_sbf/inv_007_no_aba_reuse.rs");
    let mut composition_witnesses = std::collections::BTreeSet::new();
    for (path, source, witness) in [
        (
            "tests/invariants/cu/inv_004_position_episode_binding.rs",
            position_evidence,
            "v16_program_retained_position_binding_and_writer_rosters_are_source_complete",
        ),
        (
            "tests/invariants/cu/inv_002_asset_generation_binding.rs",
            asset_evidence,
            "v16_program_asset_generation_field_and_guard_roster_is_source_complete",
        ),
        (
            "tests/invariants/cu/inv_005_authority_incarnation_binding.rs",
            authority_evidence,
            "v16_program_configured_authority_route_dispositions_are_source_complete",
        ),
        (
            "tests/invariants/public_sbf/inv_006_program_chain_message_type_and_version_binding.rs",
            transaction_domain_evidence,
            "deployed_wrapper_has_no_detached_signature_interpreter",
        ),
        (
            "tests/invariants/public_sbf/inv_007_no_aba_reuse.rs",
            account_evidence,
            "v16_wrapper_account_incarnation_census_is_source_complete",
        ),
    ] {
        assert!(
            path.starts_with("tests/invariants/") && path.ends_with(".rs"),
            "INV-013 destructive-consent witness must resolve to an invariant source file: {path}"
        );
        assert!(
            witness.starts_with("v16_")
                || witness == "deployed_wrapper_has_no_detached_signature_interpreter",
            "INV-013 destructive-consent witness must be a reviewed regression: {path}#{witness}"
        );
        assert!(
            composition_witnesses.insert((path, witness)),
            "duplicate INV-013 destructive-consent witness {path}#{witness}"
        );
        assert!(
            inv013_source_defines_test(source, witness),
            "INV-013 lost destructive-consent composition witness {path}#{witness}"
        );
    }
    assert_eq!(
        composition_witnesses.len(),
        5,
        "INV-013 destructive-consent witness roster drift"
    );
}

#[test]
fn v16_program_stale_rebalance_reduce_episode_rejects_atomically_after_reopen() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&owner, portfolio, 1_000_000);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);

    let size_q = POS_SCALE as i128;
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        size_q,
        100,
        0,
    );
    let stale_epoch = env.portfolio_position_epoch(portfolio);
    let stale_portfolio_id = env.portfolio_id(portfolio);
    assert!(
        env.portfolio_state(portfolio).legs[0].basis_pos_q.get() > 0,
        "setup opened a positive position"
    );

    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -size_q,
        100,
        0,
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(portfolio), 0),
        "public close clears the first episode"
    );

    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        size_q,
        100,
        0,
    );
    let current_epoch = env.portfolio_position_epoch(portfolio);
    assert!(
        current_epoch > stale_epoch,
        "reopen creates a later position episode"
    );
    assert_eq!(
        env.portfolio_id(portfolio),
        stale_portfolio_id,
        "episode changed without replacing the portfolio incarnation"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let counterparty_before = env.svm.get_account(&counterparty).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let stale = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: stale_portfolio_id,
            position_epoch: stale_epoch,
            asset_index: 0,
            reduce_q: POS_SCALE / 4,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );
    assert!(
        stale.is_err(),
        "stale destructive reduction from a prior position episode must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale reduction rejection leaves market exposure unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "stale reduction rejection leaves the reopened owner leg intact"
    );
    assert_eq!(
        env.svm.get_account(&counterparty).unwrap(),
        counterparty_before,
        "stale reduction rejection leaves the counterparty untouched"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "stale reduction rejection moves no custody"
    );

    env.svm.expire_blockhash();
    let current = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: stale_portfolio_id,
            position_epoch: current_epoch,
            asset_index: 0,
            reduce_q: POS_SCALE / 4,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );
    assert!(
        current.is_ok(),
        "current destructive reduction remains live: {current:?}"
    );
    assert!(
        env.portfolio_state(portfolio).legs[0]
            .basis_pos_q
            .get()
            .unsigned_abs()
            < POS_SCALE,
        "current-episode reduction changed the reopened exposure"
    );
}

// ForfeitRecoveryLeg owner-gating + input guard (sibling of v16_attack_rebalance_reduce_owner_gated, which
// was tested while ForfeitRecoveryLeg was not). handle_forfeit_recovery_leg uses with_one_portfolio_view
// (owner_must_sign=true), so a non-owner forfeiting a victim's recovery leg -- which would force the victim
// to realize a loss -- must reject before any engine mutation. Also guards the b_delta_budget==0 reject.
#[test]
fn v16_attack_forfeit_recovery_leg_owner_gated_and_zero_budget_rejected() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q.get();
    assert!(basis0 != 0, "la opened a position");
    let (_, g0) = env.market_state();

    // ATTACK: a non-owner forfeits la's recovery leg -> reject (owner mismatch, before engine).
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.svm.expire_blockhash();
    let r_grief = env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: env.portfolio_id(pa),
            position_epoch: env.portfolio_position_epoch(pa),
            asset_index: 0,
            b_delta_budget: 1_000,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[&mallory],
    );
    assert!(
        r_grief.is_err(),
        "non-owner forfeit of a victim's recovery leg must reject"
    );
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        basis0,
        "victim's position untouched by rejected griefing forfeit"
    );
    assert_eq!(
        env.market_state().1.vault,
        g0.vault,
        "vault unchanged by rejected griefing forfeit"
    );

    // INPUT GUARD: b_delta_budget == 0 rejected (checked before with_one_portfolio_view).
    env.svm.expire_blockhash();
    let r_zero = env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: env.portfolio_id(pa),
            position_epoch: env.portfolio_position_epoch(pa),
            asset_index: 0,
            b_delta_budget: 0,
        },
        vec![
            AccountMeta::new(la.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[&la],
    );
    assert!(r_zero.is_err(), "b_delta_budget == 0 must be rejected");
}
