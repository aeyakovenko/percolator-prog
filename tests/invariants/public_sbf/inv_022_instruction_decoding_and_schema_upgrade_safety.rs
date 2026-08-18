//! INV-022 - Instruction decoding and schema/upgrade safety.
//!
//! Host decoder coverage for the deployed wrapper instruction schema. The deployed API names are
//! `Instruction::decode` and `Instruction::encode`; these are the wrapper's unpack/pack boundary.
//! This file deliberately avoids duplicating the field-by-field Kani proofs and instead exercises
//! deterministic arbitrary bytes, prior-schema payloads, canonical round trips, and vector length
//! edges under the host test harness.

use percolator_prog::ix::{
    BatchTradeCpiLeg, BatchTradeLeg, CrankObservationHint, Instruction as ProgInstruction,
};
use std::panic::{catch_unwind, AssertUnwindSafe};

const MAX_BATCH_LEGS: usize = 16;

fn assert_decode_total(label: &str, data: &[u8]) -> Result<ProgInstruction, String> {
    catch_unwind(AssertUnwindSafe(|| ProgInstruction::decode(data)))
        .unwrap_or_else(|_| panic!("{label}: decoder panicked on payload {data:02x?}"))
        .map_err(|error| format!("{error:?}"))
}

fn assert_rejects(label: &str, data: &[u8]) {
    assert!(
        assert_decode_total(label, data).is_err(),
        "{label}: malformed payload unexpectedly decoded as canonical instruction"
    );
}

fn assert_canonical_roundtrip(label: &str, instruction: ProgInstruction) {
    let encoded = instruction.encode();
    let decoded = assert_decode_total(label, &encoded)
        .unwrap_or_else(|error| panic!("{label}: canonical encoding rejected: {error}"));
    assert_eq!(
        decoded, instruction,
        "{label}: decode(encode(ix)) changed ix"
    );
    assert_eq!(
        decoded.encode(),
        encoded,
        "{label}: accepted encoding did not re-pack canonically"
    );
}

fn public_instruction_corpus() -> Vec<ProgInstruction> {
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
            amount: 1,
        },
        ProgInstruction::Withdraw {
            portfolio_id: 1,
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
            account_b_portfolio_id: 2,
            asset_index: 0,
            market_id: 1,
            size_q: 1,
            exec_price: 100,
            fee_bps: 0,
        },
        ProgInstruction::TradeCpi {
            account_a_portfolio_id: 1,
            account_b_portfolio_id: 2,
            asset_index: 0,
            market_id: 1,
            size_q: 1,
            fee_bps: 0,
            limit_price: 100,
        },
        ProgInstruction::BatchTradeNoCpi {
            account_a_portfolio_id: 1,
            account_b_portfolio_id: 2,
            legs: vec![batch_nocpi_leg(0)],
        },
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: 1,
            account_b_portfolio_id: 2,
            legs: vec![batch_cpi_leg(0)],
        },
        ProgInstruction::SetMatcherConfig {
            portfolio_id: 1,
            expected_sequence: 1,
            enabled: 1,
            trade_fee_cap_bps: 25,
        },
        ProgInstruction::ClosePortfolio {
            portfolio_id: 1,
            expected_sequence: 2,
            position_epoch: 3,
        },
        ProgInstruction::TopUpInsurance {
            market_id: 1,
            amount: 1,
        },
        ProgInstruction::TopUpInsuranceDomain {
            domain: 0,
            market_id: 1,
            amount: 1,
        },
        ProgInstruction::CloseSlab,
        ProgInstruction::ResolveMarket {
            asset_generation_frontier: 1,
        },
        ProgInstruction::TopUpBackingBucket {
            domain: 0,
            market_id: 1,
            amount: 1,
            expiry_slot: 10,
        },
        ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            amount: 1,
        },
        ProgInstruction::ConvertReleasedPnl {
            portfolio_id: 1,
            amount: 1,
        },
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        ProgInstruction::UpdateAuthority {
            new_pubkey: [1u8; 32],
        },
        ProgInstruction::UpdateAssetAuthority {
            asset_index: 1,
            kind: 0,
            new_pubkey: [1u8; 32],
        },
        ProgInstruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 4_000,
            policy_sequence: 1,
        },
        ProgInstruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 4_000,
            policy_sequence: 1,
        },
        ProgInstruction::UpdateBackingFeePolicy {
            domain: 0,
            market_id: 1,
            fee_bps: 25,
            insurance_share_bps: 0,
            policy_sequence: 1,
        },
        ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 25,
            policy_sequence: 1,
        },
        ProgInstruction::UpdateFeeRedirectPolicy {
            redirect_bps: 250,
            policy_sequence: 1,
        },
        ProgInstruction::UpdateMarketInitFeePolicy {
            min_init_fee: 50,
            policy_sequence: 1,
        },
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 0,
            amount: 1,
        },
        ProgInstruction::SyncBackingDomainLedger { domain: 0 },
        ProgInstruction::SyncInsuranceLedger,
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 1,
            stale_slots: 5,
            force_close_delay_slots: 1,
            policy_sequence: 1,
        },
        ProgInstruction::ResolveStalePermissionless { now_slot: 5 },
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
        },
        ProgInstruction::ConfigureEwmaMark {
            asset_index: 0,
            market_id: 1,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            observation_sequence: 1,
        },
        ProgInstruction::PushEwmaMark {
            asset_index: 0,
            market_id: 1,
            now_slot: 2,
            mark_e6: 101,
            observation_sequence: 2,
        },
        ProgInstruction::ConfigureAuthMark {
            asset_index: 0,
            market_id: 1,
            now_slot: 1,
            initial_mark_e6: 100,
            observation_sequence: 1,
        },
        ProgInstruction::PushAuthMark {
            asset_index: 0,
            market_id: 1,
            now_slot: 2,
            mark_e6: 101,
            observation_sequence: 2,
        },
        ProgInstruction::ForceCloseAbandonedAsset {
            asset_index: 0,
            now_slot: 1,
            close_q: 1,
        },
        ProgInstruction::RestartAssetOracle {
            asset_index: 0,
            market_id: 1,
            now_slot: 3,
            initial_price: 100,
            observation_sequence: 3,
        },
        ProgInstruction::UpdateAssetLifecycle {
            action: 0,
            asset_index: 1,
            now_slot: 2,
            initial_price: 100,
            max_init_fee: 1,
            insurance_authority: [1u8; 32],
            insurance_operator: [1u8; 32],
            backing_bucket_authority: [1u8; 32],
            oracle_authority: [1u8; 32],
        },
        ProgInstruction::WithdrawInsurance { amount: 1 },
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: 1,
            amount: 1,
        },
        ProgInstruction::CureAndCancelClose {
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
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: [1u8; 32],
            secondary_mint: [2u8; 32],
        },
        ProgInstruction::SwapSecondaryForPrimary { amount: 1 },
    ]
}

fn batch_nocpi_leg(index: usize) -> BatchTradeLeg {
    BatchTradeLeg {
        asset_index: index as u16,
        market_id: 1_000 + index as u64,
        size_q: if index % 2 == 0 {
            (index as i128 + 1) * 17
        } else {
            -((index as i128 + 1) * 17)
        },
        exec_price: 100 + index as u64,
        fee_bps: index as u64 % 10_000,
    }
}

fn batch_cpi_leg(index: usize) -> BatchTradeCpiLeg {
    BatchTradeCpiLeg {
        asset_index: index as u16,
        market_id: 2_000 + index as u64,
        size_q: if index % 2 == 0 {
            (index as i128 + 1) * 31
        } else {
            -((index as i128 + 1) * 31)
        },
        fee_bps: index as u64 % 10_000,
        limit_price: 200 + index as u64,
    }
}

fn deterministic_payload(mut state: u64, case: usize) -> Vec<u8> {
    fn next(state: &mut u64) -> u8 {
        *state ^= *state << 7;
        *state ^= *state >> 9;
        *state ^= *state << 8;
        (*state >> 24) as u8
    }

    let len = match case % 19 {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 8,
        5 => 9,
        6 => 16,
        7 => 24,
        8 => 32,
        9 => 48,
        10 => 64,
        11 => 96,
        12 => 128,
        _ => usize::from(next(&mut state) % 96),
    };
    let mut data = Vec::with_capacity(len);
    for _ in 0..len {
        data.push(next(&mut state));
    }
    if let Some(tag) = data.first_mut() {
        match case % 11 {
            0 => *tag = 66,
            1 => *tag = 67,
            2 => *tag = 5,
            3 => *tag = 34,
            4 => *tag = 255,
            _ => {}
        }
    }
    if data.len() > 1 && matches!(data[0], 5 | 66 | 67) {
        data[1] = next(&mut state) % 24;
    }
    data
}

#[test]
fn host_instruction_decoder_is_total_and_canonical_for_deterministic_arbitrary_bytes() {
    let mut accepted = 0usize;
    for data in [
        ProgInstruction::InitPortfolio.encode(),
        ProgInstruction::CloseSlab.encode(),
        ProgInstruction::ClaimResolvedPayoutTopup.encode(),
        ProgInstruction::SyncInsuranceLedger.encode(),
    ] {
        let decoded = assert_decode_total("zero-payload canonical seed", &data).unwrap();
        assert_eq!(decoded.encode(), data);
        accepted += 1;
    }

    for case in 0..4_096 {
        let data = deterministic_payload(0x0220_2200_d15c_0dec ^ case as u64, case);
        if let Ok(decoded) = assert_decode_total("deterministic arbitrary payload", &data) {
            accepted += 1;
            assert_eq!(
                decoded.encode(),
                data,
                "accepted arbitrary payload must already be the canonical encoding"
            );
            assert_eq!(
                assert_decode_total("arbitrary payload re-pack", &decoded.encode()).unwrap(),
                decoded,
                "canonical re-pack must decode to the same instruction"
            );
        }
    }
    assert!(
        accepted > 0,
        "arbitrary corpus should include at least one canonical payload"
    );
}

#[test]
fn host_instruction_canonical_corpus_roundtrips_and_rejects_truncation_or_trailing_bytes() {
    for instruction in public_instruction_corpus() {
        let encoded = instruction.encode();
        let tag = encoded[0];
        assert_canonical_roundtrip(&format!("canonical tag {tag}"), instruction);

        for cut in 0..encoded.len() {
            assert_rejects(
                &format!("tag {tag} truncation at byte {cut}"),
                &encoded[..cut],
            );
        }

        let mut trailing_zero = encoded.clone();
        trailing_zero.push(0);
        assert_rejects(&format!("tag {tag} trailing zero"), &trailing_zero);

        let mut trailing_marker = encoded;
        trailing_marker.extend_from_slice(&[0xaa, 0x55, tag]);
        assert_rejects(&format!("tag {tag} trailing marker"), &trailing_marker);
    }
}

#[test]
fn host_instruction_decoder_rejects_unknown_one_byte_tags() {
    let known_tags: std::collections::BTreeSet<u8> = public_instruction_corpus()
        .into_iter()
        .map(|instruction| instruction.encode()[0])
        .collect();
    assert_eq!(
        known_tags.len(),
        50,
        "corpus must list every public instruction tag"
    );

    for tag in u8::MIN..=u8::MAX {
        if !known_tags.contains(&tag) {
            assert_rejects(&format!("unknown tag {tag}"), &[tag]);
        }
    }
}

#[test]
fn host_instruction_decoder_rejects_curated_prior_schema_payloads() {
    let mut legacy_trade_nocpi = vec![0u8; 35];
    legacy_trade_nocpi[0] = 6;

    let mut legacy_trade_cpi = vec![0u8; 35];
    legacy_trade_cpi[0] = 10;

    let mut legacy_batch_nocpi = vec![0u8; 36];
    legacy_batch_nocpi[0] = 66;
    legacy_batch_nocpi[1] = 1;

    let mut legacy_batch_cpi = vec![0u8; 28];
    legacy_batch_cpi[0] = 67;
    legacy_batch_cpi[1] = 1;

    let mut legacy_crank_with_close_size = vec![0u8; 26];
    legacy_crank_with_close_size[0] = 5;

    let mut generationless_hybrid = vec![0u8; 164];
    generationless_hybrid[0] = 34;

    let mut generationless_ewma_config = vec![0u8; 43];
    generationless_ewma_config[0] = 35;

    let mut generationless_ewma_push = vec![0u8; 27];
    generationless_ewma_push[0] = 36;

    let mut generationless_auth_config = vec![0u8; 27];
    generationless_auth_config[0] = 62;

    let mut generationless_auth_push = vec![0u8; 27];
    generationless_auth_push[0] = 63;

    let mut generationless_restart = vec![0u8; 27];
    generationless_restart[0] = 69;

    let mut legacy_base_unit_mints = vec![0u8; 33];
    legacy_base_unit_mints[0] = 60;

    for (label, data) in [
        (
            "legacy TradeNoCpi without portfolio and asset generations",
            legacy_trade_nocpi,
        ),
        (
            "legacy TradeCpi without portfolio and asset generations",
            legacy_trade_cpi,
        ),
        (
            "legacy BatchTradeNoCpi leg without market generation",
            legacy_batch_nocpi,
        ),
        (
            "legacy BatchTradeCpi leg without market generation",
            legacy_batch_cpi,
        ),
        (
            "legacy PermissionlessCrank close-size payload",
            legacy_crank_with_close_size,
        ),
        (
            "generationless ConfigureHybridOracle",
            generationless_hybrid,
        ),
        (
            "generationless ConfigureEwmaMark",
            generationless_ewma_config,
        ),
        ("generationless PushEwmaMark", generationless_ewma_push),
        (
            "generationless ConfigureAuthMark",
            generationless_auth_config,
        ),
        ("generationless PushAuthMark", generationless_auth_push),
        ("generationless RestartAssetOracle", generationless_restart),
        (
            "legacy single-mint UpdateBaseUnitMints",
            legacy_base_unit_mints,
        ),
    ] {
        assert_rejects(label, &data);
    }
}

#[test]
fn host_instruction_decoder_handles_batch_and_observation_length_edges() {
    assert_canonical_roundtrip(
        "zero-leg BatchTradeNoCpi",
        ProgInstruction::BatchTradeNoCpi {
            account_a_portfolio_id: 1,
            account_b_portfolio_id: 2,
            legs: vec![],
        },
    );
    assert_canonical_roundtrip(
        "zero-leg BatchTradeCpi",
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: 1,
            account_b_portfolio_id: 2,
            legs: vec![],
        },
    );
    assert_canonical_roundtrip(
        "max-leg BatchTradeNoCpi",
        ProgInstruction::BatchTradeNoCpi {
            account_a_portfolio_id: 1,
            account_b_portfolio_id: 2,
            legs: (0..MAX_BATCH_LEGS).map(batch_nocpi_leg).collect(),
        },
    );
    assert_canonical_roundtrip(
        "max-leg BatchTradeCpi",
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: 1,
            account_b_portfolio_id: 2,
            legs: (0..MAX_BATCH_LEGS).map(batch_cpi_leg).collect(),
        },
    );
    assert_canonical_roundtrip(
        "max-observation PermissionlessCrank",
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: (0..MAX_BATCH_LEGS)
                .map(|index| CrankObservationHint {
                    asset_index: index as u16,
                    oracle_accounts: (index % 3) as u8,
                })
                .collect(),
        },
    );

    assert_rejects(
        "PermissionlessCrank over-max observation length",
        &[5, 0, 0, 0, 0, 0, 0, 0, 0, 17],
    );
    assert_rejects("BatchTradeNoCpi over-max leg length", &[66, 17]);
    assert_rejects("BatchTradeCpi over-max leg length", &[67, 17]);

    let mut nocpi_max = ProgInstruction::BatchTradeNoCpi {
        account_a_portfolio_id: 1,
        account_b_portfolio_id: 2,
        legs: (0..MAX_BATCH_LEGS).map(batch_nocpi_leg).collect(),
    }
    .encode();
    nocpi_max[1] = (MAX_BATCH_LEGS - 1) as u8;
    assert_rejects(
        "BatchTradeNoCpi count smaller than encoded body",
        &nocpi_max,
    );
    nocpi_max[1] = MAX_BATCH_LEGS as u8;
    nocpi_max.pop();
    assert_rejects("BatchTradeNoCpi max body truncated by one byte", &nocpi_max);

    let mut cpi_max = ProgInstruction::BatchTradeCpi {
        account_a_portfolio_id: 1,
        account_b_portfolio_id: 2,
        legs: (0..MAX_BATCH_LEGS).map(batch_cpi_leg).collect(),
    }
    .encode();
    cpi_max[1] = (MAX_BATCH_LEGS - 1) as u8;
    assert_rejects("BatchTradeCpi count smaller than encoded body", &cpi_max);
    cpi_max[1] = MAX_BATCH_LEGS as u8;
    cpi_max.pop();
    assert_rejects("BatchTradeCpi max body truncated by one byte", &cpi_max);
}
