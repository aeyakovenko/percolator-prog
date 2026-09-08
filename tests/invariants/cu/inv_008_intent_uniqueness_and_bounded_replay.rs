//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: each retained top-up carries a per-asset monotonic intent ID. The two
//! insurance entrypoints share one lane, backing uses a separate lane, and a successful mutation
//! consumes its lane only after all wrapper and engine validation. Public-SBF and stateful owners
//! prove stale-retry rollback and fresh-intent liveness; this file pins source composition and the
//! no-growth zero-copy layout used by those routes. The replay-disposition roster also classifies
//! every public instruction and requires all retry/supersession generator kinds to retain a
//! production route, so adding a public variant cannot silently bypass an explicit replay owner.
//! A public history also rolls back a successful top-up on a later SPL error, consumes that intent
//! through the alternate insurance route, then rejects the retained instruction and failed bundle
//! both before and after a fresh intent. This complements the same-route failed-CPI retry probes
//! and the duplicate-intent bundles; no account repair or instruction rebinding supplies recovery.
//! The refund history pairs consumed top-ups with a live insurance withdrawal in both instruction
//! orders, then returns the exact deposited tokens to the source. Restoring the original custody
//! and insurance balances must not restore consent: both retained routes stay stale, while a
//! fresh alternate-route intent remains live. This is not a terminal payout or partial-fill probe.
//! A separate portfolio-withdrawal history restores capital and SPL custody by redepositing the
//! exact payout. Duplicate and mixed deposit/withdraw bundles roll back their successful prefix;
//! neither restored balances nor a fresh owner sequence can revive the retained withdrawal.
//! This does not certify insurance-withdrawal stock binding (counterexample 415 remains open).
//! This is bounded asset-0 evidence using fresh blockhash envelopes around retained instruction
//! bytes, not detached-signature, durable-nonce, or arbitrary-history coverage.

use super::*;
use crate::support::invariant_discovery::{RetryIntentKind, SupersededIntentKind};
use std::collections::{BTreeMap, BTreeSet};

fn braced_block_after<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing source marker {marker}"));
    let open = start
        + source[start..]
            .find('{')
            .unwrap_or_else(|| panic!("missing opening brace after {marker}"));
    let mut depth = 0i32;
    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[(open + 1)..(open + offset)];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated source block after {marker}");
}

fn assert_ordered(body: &str, markers: &[&str]) {
    let mut cursor = 0usize;
    for marker in markers {
        let offset = body[cursor..]
            .find(marker)
            .unwrap_or_else(|| panic!("missing ordered marker {marker}"));
        cursor += offset + marker.len();
    }
}

fn instruction_variants(source: &str) -> BTreeSet<String> {
    braced_block_after(source, "pub enum Instruction")
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let first = line.as_bytes().first().copied()?;
            if !first.is_ascii_uppercase() {
                return None;
            }
            line.split(|character| character == '{' || character == ',')
                .next()
                .map(str::trim)
                .filter(|variant| !variant.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

fn debug_names<T: core::fmt::Debug>(values: &[T]) -> BTreeSet<String> {
    values.iter().map(|value| format!("{value:?}")).collect()
}

#[test]
fn v16_public_replay_disposition_roster_is_source_complete() {
    let source = include_str!("../../../src/v16_program.rs");
    let source_variants = instruction_variants(source);
    assert_eq!(
        source_variants.len(),
        49,
        "production instruction roster changed"
    );

    let mut public_registry = BTreeMap::new();
    for line in include_str!("../public_instruction_coverage.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields: Vec<&str> = line.splitn(5, '\t').collect();
        assert_eq!(fields.len(), 5, "malformed public instruction row: {line}");
        let tag: u8 = fields[0].parse().expect("numeric instruction tag");
        assert!(
            public_registry.insert(tag, fields[1]).is_none(),
            "duplicate public instruction tag {tag}"
        );
    }

    let mut dispositions = BTreeMap::new();
    let mut retry_variants = BTreeSet::new();
    let mut retry_kinds = BTreeSet::new();
    let mut supersession_variants = BTreeSet::new();
    let mut supersession_kinds = BTreeSet::new();
    for line in include_str!("../inv_008_replay_disposition.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields: Vec<&str> = line.splitn(4, '\t').collect();
        assert_eq!(fields.len(), 4, "malformed replay disposition row: {line}");
        let tag: u8 = fields[0].parse().expect("numeric replay disposition tag");
        let variant = fields[1];
        let disposition = fields[2];
        assert!(!fields[3].trim().is_empty(), "empty boundary for {variant}");
        assert_eq!(
            public_registry.get(&tag),
            Some(&variant),
            "replay disposition must bind the production tag and variant"
        );
        assert!(
            dispositions.insert(variant, disposition).is_none(),
            "duplicate replay disposition for {variant}"
        );

        if let Some(kinds) = disposition.strip_prefix("retry:") {
            retry_variants.insert(variant);
            retry_kinds.extend(kinds.split(',').map(str::to_owned));
        } else if let Some(kinds) = disposition.strip_prefix("supersession:") {
            supersession_variants.insert(variant);
            supersession_kinds.extend(kinds.split(',').map(str::to_owned));
        } else {
            assert!(
                matches!(
                    disposition,
                    "init-once"
                        | "incarnation-episode"
                        | "state-derived"
                        | "live-authority"
                        | "balance-bounded"
                ),
                "unknown replay disposition {disposition} for {variant}"
            );
        }
    }

    assert_eq!(
        dispositions.keys().copied().collect::<BTreeSet<_>>(),
        source_variants.iter().map(String::as_str).collect(),
        "every production instruction needs an explicit replay disposition"
    );
    assert_eq!(
        retry_kinds,
        debug_names(&RetryIntentKind::ALL),
        "every executable retry generator kind needs a production route and vice versa"
    );
    assert_eq!(
        supersession_kinds,
        debug_names(&SupersededIntentKind::ALL),
        "every executable supersession generator kind needs a production route and vice versa"
    );
    let retry_route_families = [
        ("BatchTradeCpi", "trade"),
        ("BatchTradeNoCpi", "trade"),
        ("ConvertReleasedPnl", "conversion"),
        ("Deposit", "deposit"),
        ("RebalanceReduce", "reduction"),
        ("TopUpBackingBucket", "backing-top-up"),
        ("TopUpInsurance", "insurance-top-up"),
        ("TopUpInsuranceDomain", "insurance-top-up"),
        ("TradeCpi", "trade"),
        ("TradeNoCpi", "trade"),
        ("UpdateAssetLifecycle", "asset-activation"),
        ("Withdraw", "withdrawal"),
        ("WithdrawInsuranceAsset", "insurance-withdrawal"),
    ];
    assert_eq!(
        retry_variants,
        retry_route_families
            .iter()
            .map(|(variant, _)| *variant)
            .collect(),
        "new retryable economic routes must enter the executable INV-008 matrix"
    );
    let mut routes_by_family = BTreeMap::<&str, BTreeSet<&str>>::new();
    for (variant, family) in retry_route_families {
        routes_by_family.entry(family).or_default().insert(variant);
    }
    assert_eq!(
        routes_by_family
            .into_iter()
            .filter(|(_, variants)| variants.len() > 1)
            .collect::<BTreeMap<_, _>>(),
        [
            (
                "insurance-top-up",
                ["TopUpInsurance", "TopUpInsuranceDomain"]
                    .into_iter()
                    .collect(),
            ),
            (
                "trade",
                ["BatchTradeCpi", "BatchTradeNoCpi", "TradeCpi", "TradeNoCpi",]
                    .into_iter()
                    .collect(),
            ),
        ]
        .into_iter()
        .collect(),
        "a new multi-entrypoint retained family needs an ordered exact-once route matrix"
    );
    assert_eq!(
        supersession_variants,
        [
            "ConfigureAuthMark",
            "ConfigureEwmaMark",
            "ConfigureHybridOracle",
            "ConfigurePermissionlessResolve",
            "PushAuthMark",
            "PushEwmaMark",
            "RestartAssetOracle",
            "SetMatcherConfig",
            "UpdateBackingFeePolicy",
            "UpdateFeeRedirectPolicy",
            "UpdateLiquidationFeePolicy",
            "UpdateMaintenanceFeePolicy",
            "UpdateMarketInitFeePolicy",
            "UpdateTradeFeePolicy",
        ]
        .into_iter()
        .collect(),
        "new delayed controls must enter the executable supersession matrix"
    );
}

#[test]
fn v16_top_up_intent_wire_and_dispatch_roster_is_complete() {
    let source = include_str!("../../../src/v16_program.rs");
    let instruction = braced_block_after(source, "pub enum Instruction");
    for variant in [
        "TopUpInsurance",
        "TopUpInsuranceDomain",
        "TopUpBackingBucket",
    ] {
        let body = braced_block_after(instruction, variant);
        assert!(
            body.contains("intent_id: u64"),
            "{variant} must carry the monotonic top-up intent"
        );
    }

    let decode = braced_block_after(source, "pub fn decode(input: &[u8])");
    let encode = braced_block_after(source, "pub fn encode(&self)");
    let process = braced_block_after(source, "pub fn process_instruction");
    assert_eq!(decode.matches("intent_id: read_u64(&mut rest)?").count(), 3);
    assert_eq!(encode.matches("push_u64(&mut out, intent_id)").count(), 3);
    assert_eq!(process.matches("intent_id,").count(), 6);
}

#[test]
fn v16_top_up_intent_guards_precede_mutation_and_consumption_is_last() {
    let source = include_str!("../../../src/v16_program.rs");
    let market = braced_block_after(source, "fn handle_top_up_insurance<'a>");
    let backing = braced_block_after(source, "fn handle_top_up_backing_bucket<'a>");

    assert_ordered(
        market,
        &[
            "require_newer_control_sequence(sequences.insurance_top_up, intent_id)",
            "deposit_market_zero_insurance_view",
            "group.validate_shape()",
            "ControlSequenceLane::InsuranceTopUp",
            "transfer_tokens",
        ],
    );
    assert_ordered(
        market,
        &[
            "require_newer_control_sequence(sequences.insurance_top_up, intent_id)",
            "deposit_domain_insurance_not_atomic",
            "group.validate_shape()",
            "ControlSequenceLane::InsuranceTopUp",
            "transfer_tokens",
        ],
    );
    assert_ordered(
        backing,
        &[
            "require_newer_control_sequence(sequences.backing_top_up, intent_id)",
            "deposit_fresh_counterparty_backing_not_atomic",
            "group.validate_shape()",
            "ControlSequenceLane::BackingTopUp",
            "transfer_tokens",
        ],
    );
    assert_eq!(
        market
            .matches("ControlSequenceLane::InsuranceTopUp")
            .count(),
        1
    );
    assert_eq!(
        backing.matches("ControlSequenceLane::BackingTopUp").count(),
        2
    );
}

#[test]
fn v16_top_up_sequences_reuse_the_existing_zero_copy_tail_without_growth() {
    assert_eq!(
        core::mem::size_of::<state::AssetControlSequencesV16>(),
        percolator_prog::constants::ASSET_CONTROL_SEQUENCES_LEN
    );
    assert_eq!(percolator_prog::constants::ASSET_CONTROL_SEQUENCES_LEN, 88);

    let sequences = state::AssetControlSequencesV16 {
        permissionless_resolve: 9,
        insurance_top_up: 10,
        backing_top_up: 11,
        ..state::AssetControlSequencesV16::default()
    };
    assert_eq!(sequences.permissionless_resolve, 9);
    assert_eq!(sequences.insurance_top_up, 10);
    assert_eq!(sequences.backing_top_up, 11);
    state::validate_asset_control_sequences(&sequences).expect("all u64 watermarks are canonical");
}

#[test]
fn v16_insurance_failed_bundle_retry_stays_consumed_after_alternate_route() {
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const AMOUNT: u64 = 100;
    const FUNDING: u64 = 3 * AMOUNT;

    for failed_direct in [true, false] {
        let mut env = V16CuEnv::new();
        let ledger = env.insurance_ledger_account();
        let source = env.token_account(env.admin.pubkey(), FUNDING);
        let destination = env.token_account(env.admin.pubkey(), 0);
        let sequences_before = env.control_sequences(0);
        let intent_id = next_control_sequence(sequences_before.insurance_top_up);
        let market_id = env.asset_market_id(0);
        let program_id = env.program_id;
        let accounts = vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ];
        let top_up = |direct: bool, intent_id| Instruction {
            program_id,
            accounts: accounts.clone(),
            data: if direct {
                ProgInstruction::TopUpInsurance {
                    authority_epoch: sequences_before.authority_epoch,
                    intent_id,
                    market_id,
                    amount: AMOUNT as u128,
                }
            } else {
                ProgInstruction::TopUpInsuranceDomain {
                    authority_epoch: sequences_before.authority_epoch,
                    intent_id,
                    market_id,
                    domain: 1,
                    amount: AMOUNT as u128,
                }
            }
            .encode(),
        };
        let retained = top_up(failed_direct, intent_id);
        let alternate = top_up(!failed_direct, intent_id);
        let failed_bundle = vec![
            retained.clone(),
            spl_token::instruction::transfer(
                &spl_token::ID,
                &source,
                &destination,
                &env.admin.pubkey(),
                &[],
                FUNDING,
            )
            .unwrap(),
        ];

        // All economic accounts, including non-payer lamports; only network fees are excluded.
        let frame_keys = [
            env.market,
            ledger,
            source,
            destination,
            env.vault,
            env.mint,
            env.admin.pubkey(),
        ];
        let frame = |env: &V16CuEnv| {
            frame_keys.map(|key| env.svm.get_account(&key).expect("fixture account"))
        };
        let mint_before = env.svm.get_account(&env.mint).unwrap();
        let mut signatures = BTreeSet::new();
        let mut max_cu = 0;
        let mut execute = |env: &mut V16CuEnv, instructions: Vec<Instruction>| {
            // Keep retained instruction bytes intact, without send_tx's current-guard rebinding.
            env.svm.expire_blockhash();
            let mut message = vec![heap_ix(), cu_ix()];
            message.extend(instructions);
            let tx = Transaction::new_signed_with_payer(
                &message,
                Some(&env.payer.pubkey()),
                &[&env.payer, &env.admin],
                env.svm.latest_blockhash(),
            );
            tx.verify()
                .expect("valid independent transaction signature");
            assert!(signatures.insert(tx.signatures[0].to_string()));
            let result = env.svm.send_transaction(tx);
            let meta = match &result {
                Ok(meta) => meta,
                Err(error) => &error.meta,
            };
            assert!(meta.compute_units_consumed > 0);
            assert_cu_within(
                "insurance late-error/alternate-route replay",
                meta.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
            max_cu = max_cu.max(meta.compute_units_consumed);
            result
        };

        let before_failure = frame(&env);
        let error = execute(&mut env, failed_bundle.clone())
            .expect_err("the trailing transfer must fail after the top-up debits its source");
        assert_eq!(
            error.err,
            TransactionError::InstructionError(
                3,
                InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32),
            )
        );
        for program in [program_id, spl_token::ID] {
            assert!(
                error
                    .meta
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {program} success")),
                "the wrapper and its SPL debit must succeed before the trailing error: {error:?}"
            );
        }
        assert_eq!(frame(&env), before_failure);
        assert_eq!(env.control_sequences(0), sequences_before);

        execute(&mut env, vec![alternate])
            .expect("rollback must leave the same intent executable through the alternate route");
        for completed in 1..=2u64 {
            assert_eq!(
                env.control_sequences(0).insurance_top_up,
                intent_id + completed - 1
            );
            assert_eq!(env.token_amount(source), FUNDING - completed * AMOUNT);
            assert!(env.token_amount(source) >= AMOUNT, "replay remains funded");
            assert_eq!(env.token_amount(env.vault), completed * AMOUNT);
            assert_eq!(env.token_amount(destination), 0);
            assert_eq!(
                env.token_amount(source)
                    + env.token_amount(env.vault)
                    + env.token_amount(destination),
                FUNDING
            );
            assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
            let (_, group) = env.market_state();
            let total = (completed * AMOUNT) as u128;
            let long = if completed == 1 && failed_direct {
                0
            } else {
                (AMOUNT / 2) as u128
            };
            assert_eq!(group.vault, total);
            assert_eq!(group.insurance, total);
            assert_eq!(group.c_tot, 0);
            assert_eq!(group.insurance_domain_budget[0], long);
            assert_eq!(group.insurance_domain_budget[1], total - long);
            assert_eq!(group.insurance_domain_budget_remaining_total, total);

            for retry in [vec![retained.clone()], failed_bundle.clone()] {
                let before_retry = frame(&env);
                let error = execute(&mut env, retry)
                    .expect_err("alternate-route consumption must invalidate every old retry");
                assert_eq!(
                    error.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    ),
                    "retry must stop at the wrapper intent guard, not the trailing SPL error"
                );
                assert!(
                    !error.meta.logs.iter().any(|line| {
                        line.starts_with(&format!("Program {} invoke", spl_token::ID))
                    }),
                    "consumed retries must not reach either token transfer"
                );
                assert_eq!(frame(&env), before_retry);
            }

            if completed == 1 {
                execute(&mut env, vec![top_up(failed_direct, intent_id + 1)])
                    .expect("stale retries must not block a fresh intent on the original route");
            }
        }
        assert_eq!(
            signatures.len(),
            7,
            "no transaction-cache rejection witness"
        );
        eprintln!("insurance failed_direct={failed_direct}: 7 transactions, max CU={max_cu}");
    }
}

#[test]
fn v16_insurance_refund_does_not_revive_consumed_cross_route_intent() {
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const AMOUNT: u64 = 100;
    const FUNDING: u64 = 3 * AMOUNT;

    for direct_first in [true, false] {
        let mut env = V16CuEnv::new();
        let source = env.token_account(env.admin.pubkey(), FUNDING);
        let sequences_before = env.control_sequences(0);
        let intent_id = next_control_sequence(sequences_before.insurance_top_up);
        let program_id = env.program_id;
        let market_id = env.asset_market_id(0);
        let accounts = vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ];
        let top_up = |direct: bool, intent_id| Instruction {
            program_id,
            accounts: accounts.clone(),
            data: if direct {
                ProgInstruction::TopUpInsurance {
                    authority_epoch: sequences_before.authority_epoch,
                    intent_id,
                    market_id,
                    amount: AMOUNT as u128,
                }
            } else {
                ProgInstruction::TopUpInsuranceDomain {
                    authority_epoch: sequences_before.authority_epoch,
                    intent_id,
                    market_id,
                    domain: 1,
                    amount: AMOUNT as u128,
                }
            }
            .encode(),
        };
        let retained = [
            top_up(direct_first, intent_id),
            top_up(!direct_first, intent_id),
        ];
        let fresh = top_up(!direct_first, next_control_sequence(intent_id));
        let refund = Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env
                .withdraw_insurance_asset_instruction(env.admin.pubkey(), 0, AMOUNT as u128)
                .encode(),
        };

        // Snapshot full accounts, including non-payer lamports and absent PDA accounts.
        let frame_keys = [
            env.market,
            source,
            env.vault,
            env.mint,
            env.admin.pubkey(),
            env.vault_authority,
            program_id,
            spl_token::ID,
        ];
        let frame = |env: &V16CuEnv| frame_keys.map(|key| (key, env.svm.get_account(&key)));
        let custody = |env: &V16CuEnv| {
            [source, env.vault, env.mint].map(|key| env.svm.get_account(&key).unwrap())
        };
        let custody_before = custody(&env);
        let mut signatures = BTreeSet::new();
        let mut max_cu = 0;
        let mut execute = |env: &mut V16CuEnv, instructions: Vec<Instruction>| {
            // Retain every instruction byte/account meta; only the signed envelope is renewed.
            env.svm.expire_blockhash();
            let mut message = vec![heap_ix(), cu_ix()];
            message.extend(instructions);
            let tx = Transaction::new_signed_with_payer(
                &message,
                Some(&env.payer.pubkey()),
                &[&env.payer, &env.admin],
                env.svm.latest_blockhash(),
            );
            tx.verify()
                .expect("valid independent transaction signature");
            assert!(signatures.insert(tx.signatures[0].to_string()));
            let result = env.svm.send_transaction(tx);
            let meta = match &result {
                Ok(meta) => meta,
                Err(error) => &error.meta,
            };
            assert!(meta.compute_units_consumed > 0);
            assert_cu_within(
                "insurance refund/cross-route intent replay",
                meta.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
            max_cu = max_cu.max(meta.compute_units_consumed);
            result
        };
        let assert_insurance = |env: &V16CuEnv, total: u128, long: u128, intent| {
            let (_, group) = env.market_state();
            assert_eq!(group.mode, MarketModeV16::Live);
            assert_eq!(group.vault, total);
            assert_eq!(group.insurance, total);
            assert_eq!(group.c_tot, 0);
            assert_eq!(group.insurance_domain_budget[0], long);
            assert_eq!(group.insurance_domain_budget[1], total - long);
            assert_eq!(group.insurance_domain_budget.iter().sum::<u128>(), total);
            assert_eq!(group.insurance_domain_budget_remaining_total, total);
            assert_eq!(env.token_amount(env.vault) as u128, total);
            assert_eq!(env.token_amount(source) as u128, FUNDING as u128 - total);
            let mut expected_sequences = sequences_before;
            expected_sequences.insurance_top_up = intent;
            assert_eq!(env.control_sequences(0), expected_sequences);
            assert_eq!(group.assets[0].market_id, market_id);
            assert_eq!(env.svm.get_account(&env.mint).unwrap(), custody_before[2]);
        };

        assert_insurance(&env, 0, 0, sequences_before.insurance_top_up);
        execute(&mut env, vec![retained[0].clone()]).expect("first retained top-up lands");
        let initial_long = if direct_first { AMOUNT / 2 } else { 0 };
        assert_insurance(&env, AMOUNT as u128, initial_long as u128, intent_id);

        // Unlike the existing duplicate-top-up bundles, the other operation here reverses
        // the original debit. Neither instruction order may erase its consumed watermark.
        for retry in &retained {
            for refund_first in [false, true] {
                let before = frame(&env);
                let bundle = if refund_first {
                    vec![refund.clone(), retry.clone()]
                } else {
                    vec![retry.clone(), refund.clone()]
                };
                let error = execute(&mut env, bundle).expect_err("consumed intent aborts bundle");
                assert_eq!(
                    error.err,
                    TransactionError::InstructionError(
                        if refund_first { 3 } else { 2 },
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    ),
                    "direct_first={direct_first}, refund_first={refund_first}"
                );
                for program in [program_id, spl_token::ID] {
                    assert_eq!(
                        error
                            .meta
                            .logs
                            .iter()
                            .filter(|line| { *line == &format!("Program {program} success") })
                            .count(),
                        usize::from(refund_first),
                        "only a preceding refund and its SPL transfer may complete"
                    );
                }
                assert_eq!(
                    frame(&env),
                    before,
                    "refund/retry bundle must roll back exactly"
                );
            }
        }

        execute(&mut env, vec![refund]).expect("unchanged refund stays live after aborted bundles");
        assert_eq!(
            custody(&env),
            custody_before,
            "public refund restores original token accounts"
        );
        assert_insurance(&env, 0, 0, intent_id);

        for after_fresh in [false, true] {
            if after_fresh {
                execute(&mut env, vec![fresh.clone()])
                    .expect("fresh alternate-route intent lands against the restored balances");
                let fresh_long = if direct_first { 0 } else { AMOUNT / 2 };
                assert_insurance(
                    &env,
                    AMOUNT as u128,
                    fresh_long as u128,
                    next_control_sequence(intent_id),
                );
            }
            for retry in &retained {
                assert!(
                    env.token_amount(source) >= AMOUNT,
                    "retry remains fully funded"
                );
                let before = frame(&env);
                let error = execute(&mut env, vec![retry.clone()])
                    .expect_err("refunding source tokens cannot resurrect either retained route");
                assert_eq!(
                    error.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    )
                );
                assert!(
                    !error.meta.logs.iter().any(|line| {
                        line.starts_with(&format!("Program {} invoke", spl_token::ID))
                    }),
                    "stale top-up must stop before token CPI"
                );
                assert_eq!(
                    frame(&env),
                    before,
                    "funded stale retry must leave every account intact"
                );
            }
        }
        assert_eq!(
            signatures.len(),
            11,
            "no transaction-cache rejection witness"
        );
        eprintln!("insurance refund direct_first={direct_first}: 11 transactions, max CU={max_cu}");
    }
}

#[test]
fn v16_retained_withdrawal_stays_consumed_after_redeposit_restores_custody() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
    use litesvm::types::FailedTransactionMetadata;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const FUNDING: u64 = 123;
    const AMOUNT: u64 = 37;

    let mut env = inv018_public_spl_market(6);
    let owner = Keypair::new();
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let portfolio_key = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio_key,
        env.portfolio_account_len,
        env.program_id,
    );
    let portfolio = portfolio_key.pubkey();
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    )
    .expect("initialize System-created portfolio through the public wrapper");
    let user_token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &env.mint,
            &user_token,
            &env.admin.pubkey(),
            &[],
            FUNDING,
        )
        .unwrap(),
        &[&env.admin],
    )
    .expect("one finite public SPL endowment");

    let deposit_accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
        AccountMeta::new(user_token, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    env.send(
        env.deposit_ix(portfolio, FUNDING as u128),
        deposit_accounts.clone(),
        &[&owner],
    )
    .expect("public deposit funds the withdrawal without injected program state");

    let program_id = env.program_id;
    let retained = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(user_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: env.withdraw_ix(portfolio, AMOUNT as u128).encode(),
    };
    let portfolio_id = env.portfolio_id(portfolio);
    let position_epoch = env.portfolio_position_epoch(portfolio);
    let sequence = env.portfolio_matcher_sequence(portfolio);
    assert_eq!(sequence, 1, "only the initial deposit consumed owner state");
    let custody_keys = [user_token, env.vault, env.mint];
    let custody = |env: &V16CuEnv| custody_keys.map(|key| env.svm.get_account(&key).unwrap());
    let custody_before = custody(&env);
    assert_eq!(
        Mint::unpack(&custody_before[2].data).unwrap().supply,
        FUNDING
    );
    let controls_before = env.control_sequences(0);

    let assert_economics = |env: &V16CuEnv, paid: u64, expected_sequence: u64| {
        let remaining = u128::from(FUNDING - paid);
        let (_, group) = env.market_state();
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(
            (group.vault, group.c_tot, group.insurance),
            (remaining, remaining, 0)
        );
        assert_eq!(group.materialized_portfolio_count, 1);
        assert_eq!(group.assets[0].oi_eff_long_q, 0);
        assert_eq!(group.assets[0].oi_eff_short_q, 0);
        assert_eq!(env.portfolio_state(portfolio).capital.get(), remaining);
        assert_eq!(env.portfolio_state(portfolio).pnl.get(), 0);
        assert_eq!(env.portfolio_id(portfolio), portfolio_id);
        assert_eq!(env.portfolio_position_epoch(portfolio), position_epoch);
        assert_eq!(env.portfolio_matcher_sequence(portfolio), expected_sequence);
        assert_eq!(env.control_sequences(0), controls_before);
        assert_eq!(env.token_amount(user_token), paid);
        assert_eq!(env.token_amount(env.vault), FUNDING - paid);
        assert_eq!(env.svm.get_account(&env.mint).unwrap(), custody_before[2]);
    };
    let assert_completed_prefix = |logs: &[String], count: usize| {
        for program in [program_id, spl_token::ID] {
            assert_eq!(
                logs.iter()
                    .filter(|line| *line == &format!("Program {program} success"))
                    .count(),
                count,
                "only the expected prefix and its SPL transfer may complete"
            );
        }
        if count == 0 {
            assert!(
                !logs
                    .iter()
                    .any(|line| { line.starts_with(&format!("Program {} invoke", spl_token::ID)) }),
                "standalone stale request must stop before token CPI"
            );
        }
    };
    let assert_stale = |error: FailedTransactionMetadata, completed_prefix: usize| {
        assert_eq!(
            error.err,
            TransactionError::InstructionError(
                2 + completed_prefix as u8,
                InstructionError::Custom(PercolatorError::EngineStale as u32),
            )
        );
        assert_completed_prefix(&error.meta.logs, completed_prefix);
    };
    let mut signatures = BTreeSet::new();
    let mut max_cu = 0;
    let mut committed = 0;
    let mut rejected = 0;
    let mut execute = |env: &mut V16CuEnv, instructions: Vec<Instruction>| {
        // Never use send_tx's binding adapters for retained instructions. Only renew the envelope.
        env.svm.expire_blockhash();
        let mut message = vec![heap_ix(), cu_ix()];
        message.extend(instructions);
        let tx = Transaction::new_signed_with_payer(
            &message,
            Some(&env.payer.pubkey()),
            &[&env.payer, &owner],
            env.svm.latest_blockhash(),
        );
        tx.verify().expect("valid independently signed envelope");
        assert!(signatures.insert(tx.signatures[0].to_string()));
        let before: Vec<_> = tx
            .message
            .account_keys
            .iter()
            .map(|key| (*key, env.svm.get_account(key)))
            .collect();
        let mut expected_payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
        expected_payer.lamports -= FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let result = env.svm.send_transaction(tx);
        let meta = match &result {
            Ok(meta) => {
                committed += 1;
                assert_completed_prefix(&meta.logs, 1);
                meta
            }
            Err(error) => {
                rejected += 1;
                for (key, account) in before {
                    if key != env.payer.pubkey() {
                        assert_eq!(
                            env.svm.get_account(&key),
                            account,
                            "replay must restore all bytes, metadata and lamports at {key}"
                        );
                    }
                }
                &error.meta
            }
        };
        assert_eq!(
            env.svm.get_account(&env.payer.pubkey()).unwrap(),
            expected_payer
        );
        assert_eq!(env.svm.get_account(&env.mint).unwrap(), custody_before[2]);
        assert!(meta.compute_units_consumed > 0);
        assert_cu_within(
            "withdrawal/redeposit replay",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        max_cu = max_cu.max(meta.compute_units_consumed);
        result
    };

    assert_economics(&env, 0, sequence);
    let error = execute(&mut env, vec![retained.clone(), retained.clone()])
        .expect_err("second copy aborts even the first successful withdrawal and SPL transfer");
    assert_stale(error, 1);
    assert_economics(&env, 0, sequence);
    execute(&mut env, vec![retained.clone()])
        .expect("exact original request remains live after duplicate-bundle rollback");
    assert_economics(&env, AMOUNT, sequence + 1);
    assert!(
        FUNDING - AMOUNT >= AMOUNT,
        "retries are not blocked by depleted stock"
    );
    assert_stale(
        execute(&mut env, vec![retained.clone()]).expect_err("standalone retained retry is stale"),
        0,
    );

    let redeposit = Instruction {
        program_id,
        accounts: deposit_accounts,
        data: env.deposit_ix(portfolio, AMOUNT as u128).encode(),
    };
    for redeposit_first in [false, true] {
        let bundle = if redeposit_first {
            vec![redeposit.clone(), retained.clone()]
        } else {
            vec![retained.clone(), redeposit.clone()]
        };
        assert_stale(
            execute(&mut env, bundle).expect_err("redeposit cannot revive a consumed withdrawal"),
            usize::from(redeposit_first),
        );
        assert_economics(&env, AMOUNT, sequence + 1);
    }
    execute(&mut env, vec![redeposit])
        .expect("unchanged current deposit survives both aborted instruction orders");
    assert_eq!(
        custody(&env),
        custody_before,
        "public redeposit restores exact original custody"
    );
    assert_economics(&env, 0, sequence + 2);
    assert_stale(
        execute(&mut env, vec![retained.clone()])
            .expect_err("restored capital and custody cannot restore the old withdrawal consent"),
        0,
    );

    let fresh = Instruction {
        data: env.withdraw_ix(portfolio, AMOUNT as u128).encode(),
        ..retained.clone()
    };
    assert_eq!(
        fresh.data,
        ProgInstruction::Withdraw {
            portfolio_id,
            expected_sequence: sequence + 2,
            amount: AMOUNT as u128,
        }
        .encode(),
        "fresh intent changes only its owner-state sequence"
    );
    for fresh_first in [false, true] {
        let bundle = if fresh_first {
            vec![fresh.clone(), retained.clone()]
        } else {
            vec![retained.clone(), fresh.clone()]
        };
        assert_stale(
            execute(&mut env, bundle).expect_err("old retry also aborts a current withdrawal"),
            usize::from(fresh_first),
        );
        assert_economics(&env, 0, sequence + 2);
    }
    execute(&mut env, vec![fresh.clone()])
        .expect("exact current withdrawal survives stale-first and successful-prefix rollbacks");
    assert_economics(&env, AMOUNT, sequence + 3);
    for replay in [retained, fresh] {
        assert_stale(
            execute(&mut env, vec![replay])
                .expect_err("both consumed withdrawal intents stay stale"),
            0,
        );
        assert_economics(&env, AMOUNT, sequence + 3);
    }
    assert_eq!((committed, rejected, signatures.len()), (3, 9, 12));
    eprintln!(
        "withdrawal/redeposit: 12 transactions, 9 exact stale rollbacks, max CU={max_cu}; \
         two distinct withdrawals paid {AMOUNT} each, one redeposit returned {AMOUNT}"
    );
}
