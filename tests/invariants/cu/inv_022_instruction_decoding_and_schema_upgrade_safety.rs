//! INV-022 - Instruction decoding and schema/upgrade safety.
//!
//! Normative obligation: instruction decoding is total, rejects unknown
//! versions and trailing ambiguity, and cannot reinterpret old or malformed
//! signed bytes as a different public operation.
//!
//! This SBF owner complements the Kani decoder proofs with raw public
//! transactions. A canonical raw deposit first proves the fixture can mutate
//! state through the deployed entrypoint; malformed, truncated, oversized,
//! trailing, and systematically bit-mutated encodings must then fail before
//! any market, portfolio, or vault bytes change. The mutation matrix covers
//! every bit at each schema's tag and boundary-sensitive payload positions.

use super::*;

fn send_raw_program_instruction(
    env: &mut V16CuEnv,
    data: Vec<u8>,
    accounts: Vec<AccountMeta>,
    signers: &[&Keypair],
) -> Result<u64, String> {
    let instruction = Instruction {
        program_id: env.program_id,
        accounts,
        data,
    };
    let mut signer_refs = Vec::with_capacity(1 + signers.len());
    signer_refs.push(&env.payer);
    signer_refs.extend_from_slice(signers);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), instruction],
        Some(&env.payer.pubkey()),
        &signer_refs,
        env.svm.latest_blockhash(),
    );
    env.svm
        .send_transaction(tx)
        .map(|meta| meta.compute_units_consumed)
        .map_err(|error| format!("{error:?}"))
}

fn assert_decode_rejects_without_mutation(
    label: &str,
    env: &mut V16CuEnv,
    portfolio: Pubkey,
    data: Vec<u8>,
) {
    assert_raw_instruction_fails_without_mutation(
        label,
        env,
        portfolio,
        data,
        Some("InvalidInstructionData"),
    );
}

fn assert_raw_instruction_fails_without_mutation(
    label: &str,
    env: &mut V16CuEnv,
    portfolio: Pubkey,
    data: Vec<u8>,
    expected_error: Option<&str>,
) -> String {
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let error = send_raw_program_instruction(env, data, vec![], &[])
        .unwrap_err_or_else(|| panic!("{label}: raw instruction unexpectedly succeeded"));

    if let Some(expected_error) = expected_error {
        assert!(
            error.contains(expected_error),
            "{label}: expected {expected_error}, got {error}",
        );
    }
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "{label}: failed instruction rewrote market state",
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "{label}: failed instruction rewrote portfolio state",
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "{label}: failed instruction rewrote vault state",
    );
    error
}

trait ResultExt<T> {
    fn unwrap_err_or_else(self, f: impl FnOnce() -> String) -> String;
}

impl<T> ResultExt<T> for Result<T, String> {
    fn unwrap_err_or_else(self, f: impl FnOnce() -> String) -> String {
        match self {
            Ok(_) => panic!("{}", f()),
            Err(error) => error,
        }
    }
}

fn inv_022_known_public_tag(tag: u8) -> bool {
    matches!(
        tag,
        0 | 1
            | 3
            | 4
            | 5
            | 6
            | 8
            | 9
            | 10
            | 13
            | 19
            | 24
            | 28
            | 30
            | 32
            | 34
            | 35
            | 36
            | 37
            | 38
            | 39
            | 40
            | 41
            | 42
            | 43
            | 44
            | 45
            | 46
            | 48
            | 49
            | 50
            | 51
            | 52
            | 53
            | 54
            | 55
            | 56
            | 57
            | 58
            | 59
            | 60
            | 61
            | 62
            | 63
            | 64
            | 65
            | 66
            | 67
            | 68
            | 69
    )
}

fn inv_022_zero_payload_public_tag(tag: u8) -> bool {
    matches!(tag, 1 | 46 | 54)
}

fn inv_022_representative_public_instructions() -> Vec<ProgInstruction> {
    vec![
        ProgInstruction::InitMarket {
            max_portfolio_assets: 2,
            h_min: 1,
            h_max: 10,
            initial_price: 1_000_000,
            min_nonzero_mm_req: 1,
            min_nonzero_im_req: 2,
            maintenance_margin_bps: 500,
            initial_margin_bps: 1_000,
            max_trading_fee_bps: 10_000,
            trade_fee_base_bps: 0,
            liquidation_fee_bps: 50,
            liquidation_fee_cap: 100,
            min_liquidation_abs: 1,
            max_price_move_bps_per_slot: 50,
            max_accrual_dt_slots: 10,
            max_abs_funding_e9_per_slot: 1,
            min_funding_lifetime_slots: 1,
            max_account_b_settlement_chunks: 1,
            max_bankrupt_close_chunks: 1,
            max_bankrupt_close_lifetime_slots: 10,
            public_b_chunk_atoms: 1,
            maintenance_fee_per_slot: 0,
        },
        ProgInstruction::InitPortfolio,
        ProgInstruction::Deposit {
            portfolio_id: 1,
            expected_sequence: 0,
            amount: 1,
        },
        ProgInstruction::Withdraw {
            portfolio_id: 1,
            expected_sequence: 0,
            amount: 1,
        },
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        },
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: 1,
            account_a_position_epoch: 0,
            account_b_portfolio_id: 2,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: 1,
            size_q: 1,
            exec_price: 100,
            fee_bps: 0,
        },
        ProgInstruction::ClosePortfolio {
            portfolio_id: 1,
            expected_sequence: 2,
            position_epoch: 3,
        },
        ProgInstruction::TopUpInsurance {
            authority_epoch: 0,
            intent_id: 2,
            market_id: 1,
            amount: 1,
        },
        ProgInstruction::TradeCpi {
            account_a_portfolio_id: 1,
            account_a_position_epoch: 0,
            account_b_portfolio_id: 2,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: 1,
            size_q: 1,
            fee_bps: 0,
            limit_price: 100,
        },
        ProgInstruction::CloseSlab { authority_epoch: 1 },
        ProgInstruction::ResolveMarket {
            asset_generation_frontier: 1,
            authority_epoch: 2,
        },
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 4,
            domain: 0,
            market_id: 1,
            amount: 1,
            expiry_slot: 10,
        },
        ProgInstruction::ConvertReleasedPnl {
            portfolio_id: 1,
            position_epoch: 1,
            amount: 1,
        },
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        ProgInstruction::UpdateAuthority {
            authority_epoch: 1,
            new_pubkey: [1u8; 32],
        },
        ProgInstruction::ConfigureHybridOracle {
            asset_index: 0,
            market_id: 1,
            now_slot: 1,
            now_unix_ts: 1,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: [[1u8; 32], [0u8; 32], [0u8; 32]],
            observation_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::ConfigureEwmaMark {
            asset_index: 0,
            market_id: 1,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            observation_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::PushEwmaMark {
            asset_index: 0,
            market_id: 1,
            now_slot: 2,
            mark_e6: 101,
            observation_sequence: 2,
            authority_epoch: 0,
        },
        ProgInstruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 4_000,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 1,
            stale_slots: 5,
            force_close_delay_slots: 1,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::ResolveStalePermissionless { now_slot: 5 },
        ProgInstruction::UpdateAssetLifecycle {
            action: 0,
            asset_index: 1,
            market_id: 2,
            authority_epoch: 0,
            now_slot: 2,
            initial_price: 100,
            max_init_fee: 1,
            insurance_authority: [1u8; 32],
            insurance_operator: [1u8; 32],
            backing_bucket_authority: [1u8; 32],
            oracle_authority: [1u8; 32],
        },
        ProgInstruction::CureAndCancelClose {
            portfolio_id: 1,
            position_epoch: 1,
            optional_deposit: 1,
        },
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: 1,
            position_epoch: 2,
            asset_index: 0,
            b_delta_budget: 1,
        },
        ProgInstruction::RebalanceReduce {
            portfolio_id: 1,
            position_epoch: 2,
            asset_index: 0,
            reduce_q: 1,
        },
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        ProgInstruction::ClaimResolvedPayoutTopup,
        ProgInstruction::SyncMaintenanceFee { now_slot: 1 },
        ProgInstruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 4_000,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            market_id: 1,
            authority_epoch: 0,
            amount: 1,
        },
        ProgInstruction::UpdateBackingFeePolicy {
            domain: 0,
            market_id: 1,
            fee_bps: 25,
            insurance_share_bps: 0,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 0,
            market_id: 1,
            authority_epoch: 0,
            amount: 1,
        },
        ProgInstruction::SyncBackingDomainLedger { domain: 0 },
        ProgInstruction::SyncInsuranceLedger,
        ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 25,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::TopUpInsuranceDomain {
            authority_epoch: 0,
            intent_id: 3,
            domain: 0,
            market_id: 1,
            amount: 1,
        },
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: 1,
            authority_epoch: 0,
            amount: 1,
        },
        ProgInstruction::UpdateFeeRedirectPolicy {
            redirect_bps: 250,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::UpdateMarketInitFeePolicy {
            min_init_fee: 50,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: [1u8; 32],
            secondary_mint: [2u8; 32],
            authority_epoch: 0,
        },
        ProgInstruction::SwapSecondaryForPrimary {
            amount: 1,
            authority_epoch: 0,
        },
        ProgInstruction::ConfigureAuthMark {
            asset_index: 0,
            market_id: 1,
            now_slot: 1,
            initial_mark_e6: 100,
            observation_sequence: 1,
            authority_epoch: 0,
        },
        ProgInstruction::PushAuthMark {
            asset_index: 0,
            market_id: 1,
            now_slot: 2,
            mark_e6: 101,
            observation_sequence: 2,
            authority_epoch: 0,
        },
        ProgInstruction::ForceCloseAbandonedAsset {
            asset_index: 0,
            now_slot: 1,
            close_q: 1,
        },
        ProgInstruction::UpdateAssetAuthority {
            asset_index: 1,
            market_id: 2,
            authority_epoch: 1,
            kind: 0,
            new_pubkey: [1u8; 32],
        },
        ProgInstruction::BatchTradeNoCpi {
            account_a_portfolio_id: 1,
            account_a_position_epoch: 0,
            account_b_portfolio_id: 2,
            account_b_position_epoch: 0,
            legs: vec![BatchTradeLeg {
                asset_index: 0,
                market_id: 1,
                size_q: 1,
                exec_price: 100,
                fee_bps: 0,
            }],
        },
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: 1,
            account_a_position_epoch: 0,
            account_b_portfolio_id: 2,
            account_b_position_epoch: 0,
            legs: vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: 1,
                size_q: 1,
                fee_bps: 0,
                limit_price: 100,
            }],
        },
        ProgInstruction::SetMatcherConfig {
            portfolio_id: 1,
            expected_sequence: 1,
            enabled: 1,
            trade_fee_cap_bps: 25,
        },
        ProgInstruction::RestartAssetOracle {
            asset_index: 0,
            market_id: 1,
            now_slot: 3,
            initial_price: 100,
            observation_sequence: 3,
            authority_epoch: 0,
        },
    ]
}

#[test]
fn v16_program_one_byte_decoder_roster_rejects_every_unknown_or_truncated_tag() {
    for tag in 0u8..=u8::MAX {
        let decoded = ProgInstruction::decode(&[tag]);
        match (
            inv_022_known_public_tag(tag),
            inv_022_zero_payload_public_tag(tag),
        ) {
            (false, false) => assert!(
                decoded.is_err(),
                "unknown one-byte tag {tag} must not decode as a public instruction"
            ),
            (true, false) => assert!(
                decoded.is_err(),
                "known payload-bearing tag {tag} must reject when truncated to one byte"
            ),
            (true, true) => assert!(
                decoded.is_ok(),
                "known zero-payload tag {tag} should be the only one-byte decode success"
            ),
            (false, true) => unreachable!("zero-payload tags must be public tags"),
        }
    }
}

#[test]
fn v16_program_encoded_public_instruction_roster_rejects_trailing_bytes() {
    for ix in inv_022_representative_public_instructions() {
        let mut data = ix.encode();
        let tag = data[0];
        assert_eq!(
            ProgInstruction::decode(&data).expect("canonical representative decodes"),
            ix,
            "representative public tag {} must round-trip through the deployed decoder",
            tag,
        );
        assert!(
            ProgInstruction::decode(&data).is_ok(),
            "representative public tag {} must be canonical before trailing mutation",
            tag,
        );
        data.push(0);
        assert!(
            ProgInstruction::decode(&data).is_err(),
            "public tag {} must reject a trailing byte",
            tag,
        );
    }
}

#[test]
fn v16_host_decoder_exhausts_single_edit_neighborhood_for_every_schema() {
    let representatives = inv_022_representative_public_instructions();
    assert_eq!(representatives.len(), 49, "every public schema is owned");

    let mut proper_prefixes = 0usize;
    let mut byte_deletions = 0usize;
    let mut byte_insertions = 0usize;
    let mut byte_substitutions = 0usize;
    let mut accepted_canonical_mutations = 0usize;
    let mut rejected_mutations = 0usize;
    let mut canonical_bytes = 0usize;

    for instruction in representatives {
        let canonical = instruction.encode();
        canonical_bytes += canonical.len();
        let source_tag = canonical[0];
        assert_eq!(
            ProgInstruction::decode(&canonical).expect("canonical schema seed decodes"),
            instruction,
            "tag {source_tag}: canonical seed round-trips"
        );

        for prefix_len in 0..canonical.len() {
            proper_prefixes += 1;
            assert!(
                ProgInstruction::decode(&canonical[..prefix_len]).is_err(),
                "tag {source_tag}: proper prefix length {prefix_len} must not decode"
            );
        }

        for position in 0..canonical.len() {
            byte_deletions += 1;
            let mut mutated = canonical.clone();
            mutated.remove(position);
            match ProgInstruction::decode(&mutated) {
                Ok(decoded) => {
                    accepted_canonical_mutations += 1;
                    assert_eq!(
                        decoded.encode(),
                        mutated,
                        "tag {source_tag}: accepted deletion at byte {position} is not canonical"
                    );
                }
                Err(_) => rejected_mutations += 1,
            }
        }

        for position in 0..=canonical.len() {
            for inserted in u8::MIN..=u8::MAX {
                byte_insertions += 1;
                let mut mutated = canonical.clone();
                mutated.insert(position, inserted);
                match ProgInstruction::decode(&mutated) {
                    Ok(decoded) => {
                        accepted_canonical_mutations += 1;
                        assert_eq!(
                            decoded.encode(),
                            mutated,
                            "tag {source_tag}: accepted insertion of {inserted} at byte {position} is not canonical"
                        );
                    }
                    Err(_) => rejected_mutations += 1,
                }
            }
        }

        for position in 0..canonical.len() {
            for replacement in u8::MIN..=u8::MAX {
                if replacement == canonical[position] {
                    continue;
                }
                byte_substitutions += 1;
                let mut mutated = canonical.clone();
                mutated[position] = replacement;
                match ProgInstruction::decode(&mutated) {
                    Ok(decoded) => {
                        accepted_canonical_mutations += 1;
                        assert_eq!(
                            decoded.encode(),
                            mutated,
                            "tag {source_tag}: accepted replacement {replacement} at byte {position} is not canonical"
                        );
                        if position != 0 {
                            assert_eq!(
                                mutated[0], source_tag,
                                "payload mutation cannot change instruction kind"
                            );
                        }
                    }
                    Err(_) => rejected_mutations += 1,
                }
            }
        }
    }

    assert!(
        canonical_bytes >= 1_800,
        "canonical schema corpus unexpectedly small: {canonical_bytes} bytes"
    );
    assert_eq!(proper_prefixes, canonical_bytes);
    assert_eq!(byte_deletions, canonical_bytes);
    assert_eq!(byte_insertions, (canonical_bytes + 49) * 256);
    assert_eq!(byte_substitutions, canonical_bytes * 255);
    assert!(
        accepted_canonical_mutations > 0,
        "field mutations must exercise valid alternate canonical values"
    );
    assert!(
        rejected_mutations > 0,
        "tag, enum, length, and reserved-field mutations must exercise rejection"
    );
}

#[test]
fn v16_program_deployed_decoder_bit_mutation_matrix_is_total_canonical_and_atomic() {
    let representatives = inv_022_representative_public_instructions();
    let canonical_tags: std::collections::BTreeSet<u8> = representatives
        .iter()
        .map(|instruction| instruction.encode()[0])
        .collect();
    assert_eq!(
        canonical_tags.len(),
        49,
        "mutation roster must own every public instruction tag",
    );

    // Deduplicate payloads so identical cross-schema mutations cannot replay
    // the same transaction signature in LiteSVM. Each payload retains every
    // source tag whose mutation produced it.
    let mut mutations =
        std::collections::BTreeMap::<Vec<u8>, std::collections::BTreeSet<u8>>::new();
    let mut covered_tags = std::collections::BTreeSet::new();
    for instruction in representatives {
        let canonical = instruction.encode();
        let tag = canonical[0];
        assert_eq!(
            ProgInstruction::decode(&canonical).expect("canonical mutation seed decodes"),
            instruction,
            "tag {tag}: mutation seed must round-trip",
        );

        let mut positions = std::collections::BTreeSet::from([0usize]);
        if canonical.len() > 1 {
            positions.insert(1);
            positions.insert(canonical.len() / 2);
            positions.insert(canonical.len() - 1);
        }
        for position in positions {
            for bit in 0..u8::BITS {
                let mut mutated = canonical.clone();
                mutated[position] ^= 1u8 << bit;
                mutations.entry(mutated).or_default().insert(tag);
                covered_tags.insert(tag);
            }
        }
    }

    assert_eq!(
        covered_tags, canonical_tags,
        "every public schema must contribute bit mutations",
    );
    assert!(
        mutations.len() >= 1_200,
        "expected a substantive cross-schema mutation matrix, got {} cases",
        mutations.len(),
    );

    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let mut decoder_rejections = 0usize;
    let mut canonical_alternatives = 0usize;

    for (case, (data, source_tags)) in mutations.into_iter().enumerate() {
        let label = format!(
            "mutation {case} from tags {source_tags:?}, tag={}, len={}, data={data:02x?}",
            data[0],
            data.len(),
        );
        match ProgInstruction::decode(&data) {
            Ok(decoded) => {
                canonical_alternatives += 1;
                assert_eq!(
                    decoded.encode(),
                    data,
                    "{label}: every accepted mutation must be a canonical encoding",
                );
                assert_raw_instruction_fails_without_mutation(
                    &label, &mut env, portfolio, data, None,
                );
            }
            Err(_) => {
                decoder_rejections += 1;
                assert_raw_instruction_fails_without_mutation(
                    &label,
                    &mut env,
                    portfolio,
                    data,
                    Some("InvalidInstructionData"),
                );
            }
        }
    }

    assert!(
        decoder_rejections > 0,
        "matrix must exercise malformed deployed decoder inputs",
    );
    assert!(
        canonical_alternatives > 0,
        "matrix must exercise valid alternate field values through deployed dispatch",
    );
}

#[test]
fn v16_program_raw_instruction_decoder_rejects_ambiguity_without_mutation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let source = env.token_account(owner.pubkey(), 25);
    let market = env.market;
    let vault = env.vault;
    let canonical_deposit = ProgInstruction::Deposit {
        portfolio_id: env.portfolio_id(portfolio),
        expected_sequence: 0,
        amount: 7,
    }
    .encode();

    send_raw_program_instruction(
        &mut env,
        canonical_deposit.clone(),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(source, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    )
    .expect("canonical raw deposit must execute through the deployed entrypoint");
    assert_eq!(
        env.portfolio_state(portfolio).capital.get(),
        7,
        "control route must prove the raw fixture can mutate state",
    );

    let mut truncated_deposit = canonical_deposit.clone();
    truncated_deposit.pop();
    let mut trailing_deposit = canonical_deposit;
    trailing_deposit.push(0);

    let mut oversized_crank_vector = Vec::new();
    oversized_crank_vector.push(5);
    oversized_crank_vector.extend_from_slice(&0u64.to_le_bytes());
    oversized_crank_vector.push(17);

    let mut generationless_hybrid_oracle = vec![0u8; 164];
    generationless_hybrid_oracle[0] = 34;

    let mut generationless_ewma_config = vec![0u8; 43];
    generationless_ewma_config[0] = 35;

    let mut generationless_ewma_push = vec![0u8; 27];
    generationless_ewma_push[0] = 36;

    let mut generationless_auth_config = vec![0u8; 27];
    generationless_auth_config[0] = 62;

    let mut generationless_auth_push = vec![0u8; 27];
    generationless_auth_push[0] = 63;

    let mut generationless_restart_oracle = vec![0u8; 27];
    generationless_restart_oracle[0] = 69;

    for (label, data) in [
        ("empty payload", Vec::new()),
        ("unknown tag", vec![255]),
        ("truncated deposit", truncated_deposit),
        ("trailing deposit", trailing_deposit),
        ("trailing init portfolio", vec![1, 0]),
        ("oversized crank observation vector", oversized_crank_vector),
        ("generationless hybrid oracle", generationless_hybrid_oracle),
        ("generationless EWMA config", generationless_ewma_config),
        ("generationless EWMA push", generationless_ewma_push),
        ("generationless auth config", generationless_auth_config),
        ("generationless auth push", generationless_auth_push),
        (
            "generationless restart oracle",
            generationless_restart_oracle,
        ),
    ] {
        assert_decode_rejects_without_mutation(label, &mut env, portfolio, data);
    }
}
