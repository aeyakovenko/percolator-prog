//! INV-022 - Instruction decoding and schema/upgrade safety.
//!
//! Normative obligation: Symbolic payloads preserve every wire field, reject ambiguity, and
//! reject unknown, trailing, or truncated encodings.
//!
//! Evidence in this file (P): Kani executes the deployed wrapper arithmetic, decoder, or
//! matcher-validation code over symbolic inputs. These leaf/local proofs do not establish
//! wrapper-plus-engine whole-route conservation or liveness on their own.
//!
//! Guarantee boundary: this proves the deployed instruction decoder for the modeled payloads;
//! it does not prove account validation, authorization, engine semantics, or upgrade governance.

use super::*;

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_init_market_decode_preserves_wire_fields() {
    // Full-width symbolic inputs (audit: avoid the u16->u64/u128 widening collapse so
    // narrow-read / high-byte decode bugs are observable).
    let max_portfolio_assets: u16 = kani::any();
    let h_min: u64 = kani::any();
    let h_max: u64 = kani::any();
    let initial_price: u64 = kani::any();
    let min_nonzero_mm_req: u128 = kani::any();
    let min_nonzero_im_req: u128 = kani::any();
    let maintenance_margin_bps: u64 = kani::any();
    let initial_margin_bps: u64 = kani::any();
    let max_trading_fee_bps: u64 = kani::any();
    let trade_fee_base_bps: u64 = kani::any();
    let liquidation_fee_bps: u64 = kani::any();
    let liquidation_fee_cap: u128 = kani::any();
    let min_liquidation_abs: u128 = kani::any();
    let max_price_move_bps_per_slot: u64 = kani::any();
    let max_accrual_dt_slots: u64 = kani::any();
    let max_abs_funding_e9_per_slot: u64 = kani::any();
    let min_funding_lifetime_slots: u64 = kani::any();
    let max_account_b_settlement_chunks: u64 = kani::any();
    let max_bankrupt_close_chunks: u64 = kani::any();
    let max_bankrupt_close_lifetime_slots: u64 = kani::any();
    let public_b_chunk_atoms: u128 = kani::any();
    let maintenance_fee_per_slot: u128 = kani::any();

    let mut data = [0u8; 218];
    data[0..2].copy_from_slice(&max_portfolio_assets.to_le_bytes());
    data[2..10].copy_from_slice(&h_min.to_le_bytes());
    data[10..18].copy_from_slice(&h_max.to_le_bytes());
    data[18..26].copy_from_slice(&initial_price.to_le_bytes());
    data[26..42].copy_from_slice(&min_nonzero_mm_req.to_le_bytes());
    data[42..58].copy_from_slice(&min_nonzero_im_req.to_le_bytes());
    data[58..66].copy_from_slice(&maintenance_margin_bps.to_le_bytes());
    data[66..74].copy_from_slice(&initial_margin_bps.to_le_bytes());
    data[74..82].copy_from_slice(&max_trading_fee_bps.to_le_bytes());
    data[82..90].copy_from_slice(&trade_fee_base_bps.to_le_bytes());
    data[90..98].copy_from_slice(&liquidation_fee_bps.to_le_bytes());
    data[98..114].copy_from_slice(&liquidation_fee_cap.to_le_bytes());
    data[114..130].copy_from_slice(&min_liquidation_abs.to_le_bytes());
    data[130..138].copy_from_slice(&max_price_move_bps_per_slot.to_le_bytes());
    data[138..146].copy_from_slice(&max_accrual_dt_slots.to_le_bytes());
    data[146..154].copy_from_slice(&max_abs_funding_e9_per_slot.to_le_bytes());
    data[154..162].copy_from_slice(&min_funding_lifetime_slots.to_le_bytes());
    data[162..170].copy_from_slice(&max_account_b_settlement_chunks.to_le_bytes());
    data[170..178].copy_from_slice(&max_bankrupt_close_chunks.to_le_bytes());
    data[178..186].copy_from_slice(&max_bankrupt_close_lifetime_slots.to_le_bytes());
    data[186..202].copy_from_slice(&public_b_chunk_atoms.to_le_bytes());
    data[202..218].copy_from_slice(&maintenance_fee_per_slot.to_le_bytes());

    match Instruction::decode_body_for_proof(0, &data).unwrap() {
        Instruction::InitMarket {
            max_portfolio_assets: got_max_assets,
            h_min: got_h_min,
            h_max: got_h_max,
            initial_price: got_initial_price,
            min_nonzero_mm_req: got_min_mm,
            min_nonzero_im_req: got_min_im,
            maintenance_margin_bps: got_mm,
            initial_margin_bps: got_im,
            max_trading_fee_bps: got_fee,
            trade_fee_base_bps: got_base_fee,
            liquidation_fee_bps: got_liq_fee,
            liquidation_fee_cap: got_liq_cap,
            min_liquidation_abs: got_min_liq,
            max_price_move_bps_per_slot: got_move,
            max_accrual_dt_slots: got_dt,
            max_abs_funding_e9_per_slot: got_max_funding,
            min_funding_lifetime_slots: got_funding_life,
            max_account_b_settlement_chunks: got_b_chunks,
            max_bankrupt_close_chunks: got_bankrupt_chunks,
            max_bankrupt_close_lifetime_slots: got_bankrupt_lifetime,
            public_b_chunk_atoms: got_public_b,
            maintenance_fee_per_slot: got_maintenance_fee,
        } => {
            assert_eq!(got_max_assets, max_portfolio_assets);
            assert_eq!(got_h_min, h_min);
            assert_eq!(got_h_max, h_max);
            assert_eq!(got_initial_price, initial_price);
            assert_eq!(got_min_mm, min_nonzero_mm_req);
            assert_eq!(got_min_im, min_nonzero_im_req);
            assert_eq!(got_mm, maintenance_margin_bps);
            assert_eq!(got_im, initial_margin_bps);
            assert_eq!(got_fee, max_trading_fee_bps);
            assert_eq!(got_base_fee, trade_fee_base_bps);
            assert_eq!(got_liq_fee, liquidation_fee_bps);
            assert_eq!(got_liq_cap, liquidation_fee_cap);
            assert_eq!(got_min_liq, min_liquidation_abs);
            assert_eq!(got_move, max_price_move_bps_per_slot);
            assert_eq!(got_dt, max_accrual_dt_slots);
            assert_eq!(got_max_funding, max_abs_funding_e9_per_slot);
            assert_eq!(got_funding_life, min_funding_lifetime_slots);
            assert_eq!(got_b_chunks, max_account_b_settlement_chunks);
            assert_eq!(got_bankrupt_chunks, max_bankrupt_close_chunks);
            assert_eq!(got_bankrupt_lifetime, max_bankrupt_close_lifetime_slots);
            assert_eq!(got_public_b, public_b_chunk_atoms);
            assert_eq!(got_maintenance_fee, maintenance_fee_per_slot);
        }
        _ => unreachable!(),
    }

    let extra: u8 = kani::any();
    let mut trailing = [0u8; 219];
    trailing[..218].copy_from_slice(&data);
    trailing[218] = extra;
    assert!(Instruction::decode_body_for_proof(0, &trailing).is_err());
}

#[kani::proof]
fn kani_v16_deposit_decode_preserves_wire_fields() {
    let portfolio_id: u64 = kani::any();
    let expected_sequence: u64 = kani::any();
    let amount: u128 = kani::any();

    let mut data = [0u8; 33];
    data[0] = 3;
    data[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
    data[9..17].copy_from_slice(&expected_sequence.to_le_bytes());
    data[17..33].copy_from_slice(&amount.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::Deposit {
            portfolio_id: got_id,
            expected_sequence: got_sequence,
            amount: got,
        } => {
            assert_eq!(got_id, portfolio_id);
            assert_eq!(got_sequence, expected_sequence);
            assert_eq!(got, amount);
        }
        _ => unreachable!(),
    }

    let mut legacy = [0u8; 25];
    legacy[0] = 3;
    legacy[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
    legacy[9..25].copy_from_slice(&amount.to_le_bytes());
    assert!(Instruction::decode(&legacy).is_err());
}

#[kani::proof]
fn kani_v16_withdraw_decode_preserves_wire_fields() {
    let portfolio_id: u64 = kani::any();
    let expected_sequence: u64 = kani::any();
    let amount: u128 = kani::any();

    let mut data = [0u8; 33];
    data[0] = 4;
    data[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
    data[9..17].copy_from_slice(&expected_sequence.to_le_bytes());
    data[17..33].copy_from_slice(&amount.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::Withdraw {
            portfolio_id: got_id,
            expected_sequence: got_sequence,
            amount: got,
        } => {
            assert_eq!(got_id, portfolio_id);
            assert_eq!(got_sequence, expected_sequence);
            assert_eq!(got, amount);
        }
        _ => unreachable!(),
    }

    let mut legacy = [0u8; 25];
    legacy[0] = 4;
    legacy[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
    legacy[9..25].copy_from_slice(&amount.to_le_bytes());
    assert!(Instruction::decode(&legacy).is_err());
}

#[kani::proof]
fn kani_v16_convert_released_pnl_decode_preserves_wire_fields() {
    let portfolio_id: u64 = kani::any();
    let position_epoch: u64 = kani::any();
    let amount: u128 = kani::any();

    let mut data = [0u8; 33];
    data[0] = 28;
    data[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
    data[9..17].copy_from_slice(&position_epoch.to_le_bytes());
    data[17..33].copy_from_slice(&amount.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::ConvertReleasedPnl {
            portfolio_id: got_id,
            position_epoch: got_position_epoch,
            amount: got,
        } => {
            assert_eq!(got_id, portfolio_id);
            assert_eq!(got_position_epoch, position_epoch);
            assert_eq!(got, amount);
        }
        _ => unreachable!(),
    }

    let mut legacy = [0u8; 25];
    legacy[0] = 28;
    legacy[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
    legacy[9..25].copy_from_slice(&amount.to_le_bytes());
    assert!(Instruction::decode(&legacy).is_err());
}

#[kani::proof]
fn kani_v16_close_resolved_decode_preserves_wire_fields() {
    let amount: u128 = kani::any();

    let mut data = [0u8; 17];
    data[0] = 30;
    data[1..17].copy_from_slice(&amount.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::CloseResolved { fee_rate_per_slot } => {
            assert_eq!(fee_rate_per_slot, amount)
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_removed_withdraw_insurance_payload_rejects() {
    let amount: u128 = kani::any();

    let mut data = [0u8; 17];
    data[0] = 41;
    data[1..17].copy_from_slice(&amount.to_le_bytes());
    assert!(Instruction::decode(&data).is_err());
}

#[kani::proof]
#[kani::unwind(33)]
fn kani_v16_update_base_unit_mints_decode_preserves_wire_fields() {
    let primary_mint: [u8; 32] = kani::any();
    let secondary_mint: [u8; 32] = kani::any();
    let authority_epoch: u64 = kani::any();

    let mut data = [0u8; 72];
    data[..32].copy_from_slice(&primary_mint);
    data[32..64].copy_from_slice(&secondary_mint);
    data[64..72].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode_body_for_proof(60, &data).unwrap() {
        Instruction::UpdateBaseUnitMints {
            primary_mint: got_primary,
            secondary_mint: got_secondary,
            authority_epoch: got_epoch,
        } => {
            assert_eq!(got_primary, primary_mint);
            assert_eq!(got_secondary, secondary_mint);
            assert_eq!(got_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }

    let extra: u8 = kani::any();
    let mut trailing = [0u8; 73];
    trailing[..72].copy_from_slice(&data);
    trailing[72] = extra;
    assert!(Instruction::decode_body_for_proof(60, &trailing).is_err());
}

#[kani::proof]
fn kani_v16_swap_secondary_for_primary_decode_preserves_wire_fields() {
    let amount: u128 = kani::any();
    let authority_epoch: u64 = kani::any();

    let mut data = [0u8; 24];
    data[..16].copy_from_slice(&amount.to_le_bytes());
    data[16..24].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode_body_for_proof(61, &data).unwrap() {
        Instruction::SwapSecondaryForPrimary {
            amount: got_amount,
            authority_epoch: got_epoch,
        } => {
            assert_eq!(got_amount, amount);
            assert_eq!(got_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }

    let extra: u8 = kani::any();
    let mut trailing = [0u8; 25];
    trailing[..24].copy_from_slice(&data);
    trailing[24] = extra;
    assert!(Instruction::decode_body_for_proof(61, &trailing).is_err());
}

#[kani::proof]
#[kani::unwind(33)]
fn kani_v16_update_asset_lifecycle_decode_preserves_wire_fields() {
    let action: u8 = kani::any();
    let asset_index: u16 = kani::any();
    let market_id: u64 = kani::any();
    let authority_epoch: u64 = kani::any();
    let now_slot: u64 = kani::any();
    let initial_price: u64 = kani::any();
    let max_init_fee: u128 = kani::any();
    let insurance_authority: [u8; 32] = kani::any();
    let insurance_operator: [u8; 32] = kani::any();
    let backing_bucket_authority: [u8; 32] = kani::any();
    let oracle_authority: [u8; 32] = kani::any();

    let mut data = [0u8; 179];
    data[0] = action;
    data[1..3].copy_from_slice(&asset_index.to_le_bytes());
    data[3..11].copy_from_slice(&market_id.to_le_bytes());
    data[11..19].copy_from_slice(&authority_epoch.to_le_bytes());
    data[19..27].copy_from_slice(&now_slot.to_le_bytes());
    data[27..35].copy_from_slice(&initial_price.to_le_bytes());
    data[35..51].copy_from_slice(&max_init_fee.to_le_bytes());
    data[51..83].copy_from_slice(&insurance_authority);
    data[83..115].copy_from_slice(&insurance_operator);
    data[115..147].copy_from_slice(&backing_bucket_authority);
    data[147..179].copy_from_slice(&oracle_authority);

    match Instruction::decode_body_for_proof(40, &data).unwrap() {
        Instruction::UpdateAssetLifecycle {
            action: got_action,
            asset_index: got_asset_index,
            market_id: got_market_id,
            authority_epoch: got_authority_epoch,
            now_slot: got_now_slot,
            initial_price: got_initial_price,
            max_init_fee: got_max_init_fee,
            insurance_authority: got_insurance_authority,
            insurance_operator: got_insurance_operator,
            backing_bucket_authority: got_backing_bucket_authority,
            oracle_authority: got_oracle_authority,
        } => {
            assert_eq!(got_action, action);
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_authority_epoch, authority_epoch);
            assert_eq!(got_now_slot, now_slot);
            assert_eq!(got_initial_price, initial_price);
            assert_eq!(got_max_init_fee, max_init_fee);
            assert_eq!(got_insurance_authority, insurance_authority);
            assert_eq!(got_insurance_operator, insurance_operator);
            assert_eq!(got_backing_bucket_authority, backing_bucket_authority);
            assert_eq!(got_oracle_authority, oracle_authority);
        }
        _ => unreachable!(),
    }

    let extra: u8 = kani::any();
    let mut trailing = [0u8; 180];
    trailing[..179].copy_from_slice(&data);
    trailing[179] = extra;
    assert!(Instruction::decode_body_for_proof(40, &trailing).is_err());
}

#[kani::proof]
fn kani_v16_cure_and_cancel_close_decode_preserves_wire_fields() {
    let portfolio_id: u64 = kani::any();
    let position_epoch: u64 = kani::any();
    let amount: u128 = kani::any();

    let mut data = [0u8; 33];
    data[0] = 42;
    data[1..9].copy_from_slice(&portfolio_id.to_le_bytes());
    data[9..17].copy_from_slice(&position_epoch.to_le_bytes());
    data[17..33].copy_from_slice(&amount.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::CureAndCancelClose {
            portfolio_id: got_portfolio_id,
            position_epoch: got_position_epoch,
            optional_deposit: got,
        } => {
            assert_eq!(got_portfolio_id, portfolio_id);
            assert_eq!(got_position_epoch, position_epoch);
            assert_eq!(got, amount);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_cure_and_cancel_close_rejects_incarnationless_legacy_payload() {
    let amount: u128 = kani::any();
    let mut data = [0u8; 17];
    data[0] = 42;
    data[1..17].copy_from_slice(&amount.to_le_bytes());
    assert!(Instruction::decode(&data).is_err());
}

#[kani::proof]
fn kani_v16_set_matcher_config_decode_preserves_fee_consent() {
    let portfolio_id: u64 = kani::any();
    let expected_sequence: u64 = kani::any();
    let enabled: u8 = kani::any();
    let trade_fee_cap_bps: u16 = kani::any();
    let data = Instruction::SetMatcherConfig {
        portfolio_id,
        expected_sequence,
        enabled,
        trade_fee_cap_bps,
    }
    .encode();

    match Instruction::decode(&data).unwrap() {
        Instruction::SetMatcherConfig {
            portfolio_id: decoded_portfolio_id,
            expected_sequence: decoded_sequence,
            enabled: decoded_enabled,
            trade_fee_cap_bps: decoded_cap,
        } => {
            assert_eq!(decoded_portfolio_id, portfolio_id);
            assert_eq!(decoded_sequence, expected_sequence);
            assert_eq!(decoded_enabled, enabled);
            assert_eq!(decoded_cap, trade_fee_cap_bps);
        }
        _ => unreachable!(),
    }

    let legacy = [68, enabled];
    assert!(Instruction::decode(&legacy).is_err());

    let mut trailing = data.clone();
    trailing.push(0);
    assert!(Instruction::decode(&trailing).is_err());
}

#[kani::proof]
fn kani_v16_insurance_top_up_decode_preserves_wire_fields() {
    let market_id: u64 = kani::any();
    let intent_id: u64 = kani::any();
    let authority_epoch: u64 = kani::any();
    let amount: u128 = kani::any();

    let encoded = Instruction::TopUpInsurance {
        market_id,
        intent_id,
        authority_epoch,
        amount,
    }
    .encode();
    match Instruction::decode(&encoded).unwrap() {
        Instruction::TopUpInsurance {
            market_id: got_market_id,
            intent_id: got_intent_id,
            authority_epoch: got_authority_epoch,
            amount: got_amount,
        } => {
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_intent_id, intent_id);
            assert_eq!(got_authority_epoch, authority_epoch);
            assert_eq!(got_amount, amount);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_insurance_domain_top_up_decode_preserves_wire_fields() {
    let domain: u16 = kani::any();
    let market_id: u64 = kani::any();
    let intent_id: u64 = kani::any();
    let authority_epoch: u64 = kani::any();
    let amount: u128 = kani::any();
    let encoded = Instruction::TopUpInsuranceDomain {
        domain,
        market_id,
        intent_id,
        authority_epoch,
        amount,
    }
    .encode();

    match Instruction::decode(&encoded).unwrap() {
        Instruction::TopUpInsuranceDomain {
            domain: got_domain,
            market_id: got_market_id,
            intent_id: got_intent_id,
            authority_epoch: got_authority_epoch,
            amount: got_amount,
        } => {
            assert_eq!(got_domain, domain);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_intent_id, intent_id);
            assert_eq!(got_authority_epoch, authority_epoch);
            assert_eq!(got_amount, amount);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_asset_insurance_withdraw_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
    let market_id: u64 = kani::any();
    let authority_epoch: u64 = kani::any();
    let amount: u128 = kani::any();
    let encoded = Instruction::WithdrawInsuranceAsset {
        asset_index,
        market_id,
        authority_epoch,
        amount,
    }
    .encode();

    match Instruction::decode(&encoded).unwrap() {
        Instruction::WithdrawInsuranceAsset {
            asset_index: got_asset,
            market_id: got_market_id,
            authority_epoch: got_authority_epoch,
            amount: got_amount,
        } => {
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_authority_epoch, authority_epoch);
            assert_eq!(got_amount, amount);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_recovery_close_progress_decode_preserves_wire_fields() {
    let portfolio_id: u64 = kani::any();
    let position_epoch: u64 = kani::any();
    let asset_index: u16 = kani::any();
    let side: u8 = kani::any();
    let b_delta_budget: u128 = kani::any();
    let reduce_q: u128 = kani::any();
    let close_q: u128 = kani::any();
    let now_slot: u64 = kani::any();

    let forfeit = Instruction::ForfeitRecoveryLeg {
        portfolio_id,
        position_epoch,
        asset_index,
        b_delta_budget,
    }
    .encode();
    match Instruction::decode(&forfeit).unwrap() {
        Instruction::ForfeitRecoveryLeg {
            portfolio_id: got_portfolio_id,
            position_epoch: got_position_epoch,
            asset_index: got_asset,
            b_delta_budget: got_budget,
        } => {
            assert_eq!(got_portfolio_id, portfolio_id);
            assert_eq!(got_position_epoch, position_epoch);
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_budget, b_delta_budget);
        }
        _ => unreachable!(),
    }

    let rebalance = Instruction::RebalanceReduce {
        portfolio_id,
        position_epoch,
        asset_index,
        reduce_q,
    }
    .encode();
    match Instruction::decode(&rebalance).unwrap() {
        Instruction::RebalanceReduce {
            portfolio_id: got_portfolio_id,
            position_epoch: got_position_epoch,
            asset_index: got_asset,
            reduce_q: got_reduce,
        } => {
            assert_eq!(got_portfolio_id, portfolio_id);
            assert_eq!(got_position_epoch, position_epoch);
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_reduce, reduce_q);
        }
        _ => unreachable!(),
    }

    let finalize = Instruction::FinalizeResetSide { asset_index, side }.encode();
    match Instruction::decode(&finalize).unwrap() {
        Instruction::FinalizeResetSide {
            asset_index: got_asset,
            side: got_side,
        } => {
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_side, side);
        }
        _ => unreachable!(),
    }

    let force_close = Instruction::ForceCloseAbandonedAsset {
        asset_index,
        now_slot,
        close_q,
    }
    .encode();
    match Instruction::decode(&force_close).unwrap() {
        Instruction::ForceCloseAbandonedAsset {
            asset_index: got_asset,
            now_slot: got_slot,
            close_q: got_close,
        } => {
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_slot, now_slot);
            assert_eq!(got_close, close_q);
        }
        _ => unreachable!(),
    }

    match Instruction::decode(&Instruction::ClaimResolvedPayoutTopup.encode()).unwrap() {
        Instruction::ClaimResolvedPayoutTopup => {}
        _ => unreachable!(),
    }

    let sync_fee = Instruction::SyncMaintenanceFee { now_slot }.encode();
    match Instruction::decode(&sync_fee).unwrap() {
        Instruction::SyncMaintenanceFee { now_slot: got } => assert_eq!(got, now_slot),
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_position_consent_rejects_unbound_legacy_payloads() {
    let legacy_forfeit = [43u8; 19];
    let legacy_rebalance = [44u8; 19];
    let incarnation_only_rebalance = [44u8; 27];

    assert!(Instruction::decode(&legacy_forfeit).is_err());
    assert!(Instruction::decode(&legacy_rebalance).is_err());
    assert!(Instruction::decode(&incarnation_only_rebalance).is_err());
}

#[kani::proof]
fn kani_v16_top_up_backing_bucket_decode_preserves_wire_fields() {
    let domain: u16 = kani::any();
    let market_id: u64 = kani::any();
    let intent_id: u64 = kani::any();
    let authority_epoch: u64 = kani::any();
    let amount: u128 = kani::any();
    let expiry_slot: u64 = kani::any();

    let mut data = [0u8; 51];
    data[0] = 24;
    data[1..3].copy_from_slice(&domain.to_le_bytes());
    data[3..11].copy_from_slice(&market_id.to_le_bytes());
    data[11..19].copy_from_slice(&intent_id.to_le_bytes());
    data[19..27].copy_from_slice(&authority_epoch.to_le_bytes());
    data[27..43].copy_from_slice(&amount.to_le_bytes());
    data[43..51].copy_from_slice(&expiry_slot.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::TopUpBackingBucket {
            domain: got_domain,
            market_id: got_market_id,
            intent_id: got_intent_id,
            authority_epoch: got_authority_epoch,
            amount: got_amount,
            expiry_slot: got_expiry,
        } => {
            assert_eq!(got_domain, domain);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_intent_id, intent_id);
            assert_eq!(got_authority_epoch, authority_epoch);
            assert_eq!(got_amount, amount);
            assert_eq!(got_expiry, expiry_slot);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_topups_reject_intentless_legacy_payloads() {
    let insurance = [9u8; 25];
    let domain_insurance = [56u8; 27];
    let backing = [24u8; 35];

    assert!(Instruction::decode(&insurance).is_err());
    assert!(Instruction::decode(&domain_insurance).is_err());
    assert!(Instruction::decode(&backing).is_err());
}

#[kani::proof]
fn kani_v16_asset_controls_reject_generationless_legacy_payloads() {
    let legacy_insurance = [9u8; 17];
    let legacy_domain_insurance = [56u8; 19];
    let legacy_backing = [24u8; 27];
    let legacy_withdrawal = [57u8; 19];
    let legacy_backing_policy = [51u8; 15];
    let legacy_resolve = [19u8; 1];
    let legacy_resolve_policy = [38u8; 25];

    assert!(Instruction::decode(&legacy_insurance).is_err());
    assert!(Instruction::decode(&legacy_domain_insurance).is_err());
    assert!(Instruction::decode(&legacy_backing).is_err());
    assert!(Instruction::decode(&legacy_withdrawal).is_err());
    assert!(Instruction::decode(&legacy_backing_policy).is_err());
    assert!(Instruction::decode(&legacy_resolve).is_err());
    assert!(Instruction::decode(&legacy_resolve_policy).is_err());
}

#[derive(Clone, Copy)]
struct NoCpiTradeWireFields {
    account_a_portfolio_id: u64,
    account_a_position_epoch: u64,
    account_b_portfolio_id: u64,
    account_b_position_epoch: u64,
    asset_index: u16,
    market_id: u64,
    size_q: i128,
    exec_price: u64,
    fee_bps: u64,
}

fn canonical_nocpi_trade_fields() -> NoCpiTradeWireFields {
    NoCpiTradeWireFields {
        account_a_portfolio_id: 0x1111_1111_1111_1111,
        account_a_position_epoch: 0x2222_2222_2222_2222,
        account_b_portfolio_id: 0x3333_3333_3333_3333,
        account_b_position_epoch: 0x4444_4444_4444_4444,
        asset_index: 0x5555,
        market_id: 0x6666_6666_6666_6666,
        size_q: -0x7777_7777_7777_7777_7777_7777_7777_7777i128,
        exec_price: 0x8888_8888_8888_8888,
        fee_bps: 0x9999_9999_9999_9999,
    }
}

fn assert_single_nocpi_trade_decoder_preserves(fields: NoCpiTradeWireFields) {
    let mut single = [0u8; 75];
    single[0] = 6;
    single[1..9].copy_from_slice(&fields.account_a_portfolio_id.to_le_bytes());
    single[9..17].copy_from_slice(&fields.account_a_position_epoch.to_le_bytes());
    single[17..25].copy_from_slice(&fields.account_b_portfolio_id.to_le_bytes());
    single[25..33].copy_from_slice(&fields.account_b_position_epoch.to_le_bytes());
    single[33..35].copy_from_slice(&fields.asset_index.to_le_bytes());
    single[35..43].copy_from_slice(&fields.market_id.to_le_bytes());
    single[43..59].copy_from_slice(&fields.size_q.to_le_bytes());
    single[59..67].copy_from_slice(&fields.exec_price.to_le_bytes());
    single[67..75].copy_from_slice(&fields.fee_bps.to_le_bytes());
    match Instruction::decode_body_for_proof(6, &single[1..]).unwrap() {
        Instruction::TradeNoCpi {
            account_a_portfolio_id,
            account_a_position_epoch,
            account_b_portfolio_id,
            account_b_position_epoch,
            asset_index,
            market_id,
            size_q,
            exec_price,
            fee_bps,
        } => {
            assert_eq!(account_a_portfolio_id, fields.account_a_portfolio_id);
            assert_eq!(account_a_position_epoch, fields.account_a_position_epoch);
            assert_eq!(account_b_portfolio_id, fields.account_b_portfolio_id);
            assert_eq!(account_b_position_epoch, fields.account_b_position_epoch);
            assert_eq!(asset_index, fields.asset_index);
            assert_eq!(market_id, fields.market_id);
            assert_eq!(size_q, fields.size_q);
            assert_eq!(exec_price, fields.exec_price);
            assert_eq!(fee_bps, fields.fee_bps);
        }
        _ => unreachable!(),
    }
}

fn assert_batch_nocpi_trade_decoder_preserves(fields: NoCpiTradeWireFields) {
    let mut batch = [0u8; 76];
    batch[0] = 66;
    batch[1] = 1;
    batch[2..4].copy_from_slice(&fields.asset_index.to_le_bytes());
    batch[4..12].copy_from_slice(&fields.market_id.to_le_bytes());
    batch[12..28].copy_from_slice(&fields.size_q.to_le_bytes());
    batch[28..36].copy_from_slice(&fields.exec_price.to_le_bytes());
    batch[36..44].copy_from_slice(&fields.fee_bps.to_le_bytes());
    batch[44..52].copy_from_slice(&fields.account_a_portfolio_id.to_le_bytes());
    batch[52..60].copy_from_slice(&fields.account_a_position_epoch.to_le_bytes());
    batch[60..68].copy_from_slice(&fields.account_b_portfolio_id.to_le_bytes());
    batch[68..76].copy_from_slice(&fields.account_b_position_epoch.to_le_bytes());
    match Instruction::decode_body_for_proof(66, &batch[1..]).unwrap() {
        Instruction::BatchTradeNoCpi {
            account_a_portfolio_id,
            account_a_position_epoch,
            account_b_portfolio_id,
            account_b_position_epoch,
            legs,
        } => {
            assert_eq!(account_a_portfolio_id, fields.account_a_portfolio_id);
            assert_eq!(account_a_position_epoch, fields.account_a_position_epoch);
            assert_eq!(account_b_portfolio_id, fields.account_b_portfolio_id);
            assert_eq!(account_b_position_epoch, fields.account_b_position_epoch);
            assert_eq!(legs.len(), 1);
            assert_eq!(legs[0].asset_index, fields.asset_index);
            assert_eq!(legs[0].market_id, fields.market_id);
            assert_eq!(legs[0].size_q, fields.size_q);
            assert_eq!(legs[0].exec_price, fields.exec_price);
            assert_eq!(legs[0].fee_bps, fields.fee_bps);
        }
        _ => unreachable!(),
    }
}

#[derive(Clone, Copy)]
struct CpiTradeWireFields {
    account_a_portfolio_id: u64,
    account_a_position_epoch: u64,
    account_b_portfolio_id: u64,
    account_b_position_epoch: u64,
    asset_index: u16,
    market_id: u64,
    size_q: i128,
    fee_bps: u64,
    limit_price: u64,
}

fn canonical_cpi_trade_fields() -> CpiTradeWireFields {
    CpiTradeWireFields {
        account_a_portfolio_id: 0x1111_1111_1111_1111,
        account_a_position_epoch: 0x2222_2222_2222_2222,
        account_b_portfolio_id: 0x3333_3333_3333_3333,
        account_b_position_epoch: 0x4444_4444_4444_4444,
        asset_index: 0x5555,
        market_id: 0x6666_6666_6666_6666,
        size_q: -0x7777_7777_7777_7777_7777_7777_7777_7777i128,
        fee_bps: 0x8888_8888_8888_8888,
        limit_price: 0x9999_9999_9999_9999,
    }
}

fn assert_single_cpi_trade_decoder_preserves(fields: CpiTradeWireFields) {
    let mut single = [0u8; 75];
    single[0] = 10;
    single[1..9].copy_from_slice(&fields.account_a_portfolio_id.to_le_bytes());
    single[9..17].copy_from_slice(&fields.account_a_position_epoch.to_le_bytes());
    single[17..25].copy_from_slice(&fields.account_b_portfolio_id.to_le_bytes());
    single[25..33].copy_from_slice(&fields.account_b_position_epoch.to_le_bytes());
    single[33..35].copy_from_slice(&fields.asset_index.to_le_bytes());
    single[35..43].copy_from_slice(&fields.market_id.to_le_bytes());
    single[43..59].copy_from_slice(&fields.size_q.to_le_bytes());
    single[59..67].copy_from_slice(&fields.fee_bps.to_le_bytes());
    single[67..75].copy_from_slice(&fields.limit_price.to_le_bytes());
    match Instruction::decode_body_for_proof(10, &single[1..]).unwrap() {
        Instruction::TradeCpi {
            account_a_portfolio_id,
            account_a_position_epoch,
            account_b_portfolio_id,
            account_b_position_epoch,
            asset_index,
            market_id,
            size_q,
            fee_bps,
            limit_price,
        } => {
            assert_eq!(account_a_portfolio_id, fields.account_a_portfolio_id);
            assert_eq!(account_a_position_epoch, fields.account_a_position_epoch);
            assert_eq!(account_b_portfolio_id, fields.account_b_portfolio_id);
            assert_eq!(account_b_position_epoch, fields.account_b_position_epoch);
            assert_eq!(asset_index, fields.asset_index);
            assert_eq!(market_id, fields.market_id);
            assert_eq!(size_q, fields.size_q);
            assert_eq!(fee_bps, fields.fee_bps);
            assert_eq!(limit_price, fields.limit_price);
        }
        _ => unreachable!(),
    }
}

fn assert_batch_cpi_trade_decoder_preserves(fields: CpiTradeWireFields) {
    let mut batch = [0u8; 76];
    batch[0] = 67;
    batch[1] = 1;
    batch[2..4].copy_from_slice(&fields.asset_index.to_le_bytes());
    batch[4..12].copy_from_slice(&fields.market_id.to_le_bytes());
    batch[12..28].copy_from_slice(&fields.size_q.to_le_bytes());
    batch[28..36].copy_from_slice(&fields.fee_bps.to_le_bytes());
    batch[36..44].copy_from_slice(&fields.limit_price.to_le_bytes());
    batch[44..52].copy_from_slice(&fields.account_a_portfolio_id.to_le_bytes());
    batch[52..60].copy_from_slice(&fields.account_a_position_epoch.to_le_bytes());
    batch[60..68].copy_from_slice(&fields.account_b_portfolio_id.to_le_bytes());
    batch[68..76].copy_from_slice(&fields.account_b_position_epoch.to_le_bytes());
    match Instruction::decode_body_for_proof(67, &batch[1..]).unwrap() {
        Instruction::BatchTradeCpi {
            account_a_portfolio_id,
            account_a_position_epoch,
            account_b_portfolio_id,
            account_b_position_epoch,
            legs,
        } => {
            assert_eq!(account_a_portfolio_id, fields.account_a_portfolio_id);
            assert_eq!(account_a_position_epoch, fields.account_a_position_epoch);
            assert_eq!(account_b_portfolio_id, fields.account_b_portfolio_id);
            assert_eq!(account_b_position_epoch, fields.account_b_position_epoch);
            assert_eq!(legs.len(), 1);
            assert_eq!(legs[0].asset_index, fields.asset_index);
            assert_eq!(legs[0].market_id, fields.market_id);
            assert_eq!(legs[0].size_q, fields.size_q);
            assert_eq!(legs[0].fee_bps, fields.fee_bps);
            assert_eq!(legs[0].limit_price, fields.limit_price);
        }
        _ => unreachable!(),
    }
}

macro_rules! prove_nocpi_trade_field {
    ($single_name:ident, $batch_name:ident, $field:ident, $ty:ty) => {
        #[kani::proof]
        fn $single_name() {
            let mut fields = canonical_nocpi_trade_fields();
            fields.$field = kani::any::<$ty>();
            assert_single_nocpi_trade_decoder_preserves(fields);
        }

        #[kani::proof]
        fn $batch_name() {
            let mut fields = canonical_nocpi_trade_fields();
            fields.$field = kani::any::<$ty>();
            assert_batch_nocpi_trade_decoder_preserves(fields);
        }
    };
}

macro_rules! prove_cpi_trade_field {
    ($single_name:ident, $batch_name:ident, $field:ident, $ty:ty) => {
        #[kani::proof]
        fn $single_name() {
            let mut fields = canonical_cpi_trade_fields();
            fields.$field = kani::any::<$ty>();
            assert_single_cpi_trade_decoder_preserves(fields);
        }

        #[kani::proof]
        fn $batch_name() {
            let mut fields = canonical_cpi_trade_fields();
            fields.$field = kani::any::<$ty>();
            assert_batch_cpi_trade_decoder_preserves(fields);
        }
    };
}

prove_nocpi_trade_field!(
    kani_v16_single_nocpi_trade_decoder_preserves_account_a_portfolio_id,
    kani_v16_batch_nocpi_trade_decoder_preserves_account_a_portfolio_id,
    account_a_portfolio_id,
    u64
);
prove_nocpi_trade_field!(
    kani_v16_single_nocpi_trade_decoder_preserves_account_a_position_epoch,
    kani_v16_batch_nocpi_trade_decoder_preserves_account_a_position_epoch,
    account_a_position_epoch,
    u64
);
prove_nocpi_trade_field!(
    kani_v16_single_nocpi_trade_decoder_preserves_account_b_portfolio_id,
    kani_v16_batch_nocpi_trade_decoder_preserves_account_b_portfolio_id,
    account_b_portfolio_id,
    u64
);
prove_nocpi_trade_field!(
    kani_v16_single_nocpi_trade_decoder_preserves_account_b_position_epoch,
    kani_v16_batch_nocpi_trade_decoder_preserves_account_b_position_epoch,
    account_b_position_epoch,
    u64
);
prove_nocpi_trade_field!(
    kani_v16_single_nocpi_trade_decoder_preserves_asset_index,
    kani_v16_batch_nocpi_trade_decoder_preserves_asset_index,
    asset_index,
    u16
);
prove_nocpi_trade_field!(
    kani_v16_single_nocpi_trade_decoder_preserves_market_id,
    kani_v16_batch_nocpi_trade_decoder_preserves_market_id,
    market_id,
    u64
);
prove_nocpi_trade_field!(
    kani_v16_single_nocpi_trade_decoder_preserves_size_q,
    kani_v16_batch_nocpi_trade_decoder_preserves_size_q,
    size_q,
    i128
);
prove_nocpi_trade_field!(
    kani_v16_single_nocpi_trade_decoder_preserves_exec_price,
    kani_v16_batch_nocpi_trade_decoder_preserves_exec_price,
    exec_price,
    u64
);
prove_nocpi_trade_field!(
    kani_v16_single_nocpi_trade_decoder_preserves_fee_bps,
    kani_v16_batch_nocpi_trade_decoder_preserves_fee_bps,
    fee_bps,
    u64
);

prove_cpi_trade_field!(
    kani_v16_single_cpi_trade_decoder_preserves_account_a_portfolio_id,
    kani_v16_batch_cpi_trade_decoder_preserves_account_a_portfolio_id,
    account_a_portfolio_id,
    u64
);
prove_cpi_trade_field!(
    kani_v16_single_cpi_trade_decoder_preserves_account_a_position_epoch,
    kani_v16_batch_cpi_trade_decoder_preserves_account_a_position_epoch,
    account_a_position_epoch,
    u64
);
prove_cpi_trade_field!(
    kani_v16_single_cpi_trade_decoder_preserves_account_b_portfolio_id,
    kani_v16_batch_cpi_trade_decoder_preserves_account_b_portfolio_id,
    account_b_portfolio_id,
    u64
);
prove_cpi_trade_field!(
    kani_v16_single_cpi_trade_decoder_preserves_account_b_position_epoch,
    kani_v16_batch_cpi_trade_decoder_preserves_account_b_position_epoch,
    account_b_position_epoch,
    u64
);
prove_cpi_trade_field!(
    kani_v16_single_cpi_trade_decoder_preserves_asset_index,
    kani_v16_batch_cpi_trade_decoder_preserves_asset_index,
    asset_index,
    u16
);
prove_cpi_trade_field!(
    kani_v16_single_cpi_trade_decoder_preserves_market_id,
    kani_v16_batch_cpi_trade_decoder_preserves_market_id,
    market_id,
    u64
);
prove_cpi_trade_field!(
    kani_v16_single_cpi_trade_decoder_preserves_size_q,
    kani_v16_batch_cpi_trade_decoder_preserves_size_q,
    size_q,
    i128
);
prove_cpi_trade_field!(
    kani_v16_single_cpi_trade_decoder_preserves_fee_bps,
    kani_v16_batch_cpi_trade_decoder_preserves_fee_bps,
    fee_bps,
    u64
);
prove_cpi_trade_field!(
    kani_v16_single_cpi_trade_decoder_preserves_limit_price,
    kani_v16_batch_cpi_trade_decoder_preserves_limit_price,
    limit_price,
    u64
);

#[kani::proof]
fn kani_v16_trade_decoders_reject_position_epoch_less_legacy_schemas() {
    let nocpi = canonical_nocpi_trade_fields();
    let mut single_nocpi = [0u8; 59];
    single_nocpi[0] = 6;
    single_nocpi[1..9].copy_from_slice(&nocpi.account_a_portfolio_id.to_le_bytes());
    single_nocpi[9..17].copy_from_slice(&nocpi.account_b_portfolio_id.to_le_bytes());
    single_nocpi[17..19].copy_from_slice(&nocpi.asset_index.to_le_bytes());
    single_nocpi[19..27].copy_from_slice(&nocpi.market_id.to_le_bytes());
    single_nocpi[27..43].copy_from_slice(&nocpi.size_q.to_le_bytes());
    single_nocpi[43..51].copy_from_slice(&nocpi.exec_price.to_le_bytes());
    single_nocpi[51..59].copy_from_slice(&nocpi.fee_bps.to_le_bytes());
    assert!(Instruction::decode(&single_nocpi).is_err());

    let mut batch_nocpi = [0u8; 60];
    batch_nocpi[0] = 66;
    batch_nocpi[1] = 1;
    batch_nocpi[2..4].copy_from_slice(&nocpi.asset_index.to_le_bytes());
    batch_nocpi[4..12].copy_from_slice(&nocpi.market_id.to_le_bytes());
    batch_nocpi[12..28].copy_from_slice(&nocpi.size_q.to_le_bytes());
    batch_nocpi[28..36].copy_from_slice(&nocpi.exec_price.to_le_bytes());
    batch_nocpi[36..44].copy_from_slice(&nocpi.fee_bps.to_le_bytes());
    batch_nocpi[44..52].copy_from_slice(&nocpi.account_a_portfolio_id.to_le_bytes());
    batch_nocpi[52..60].copy_from_slice(&nocpi.account_b_portfolio_id.to_le_bytes());
    assert!(Instruction::decode(&batch_nocpi).is_err());

    let cpi = canonical_cpi_trade_fields();
    let mut single_cpi = [0u8; 59];
    single_cpi[0] = 10;
    single_cpi[1..9].copy_from_slice(&cpi.account_a_portfolio_id.to_le_bytes());
    single_cpi[9..17].copy_from_slice(&cpi.account_b_portfolio_id.to_le_bytes());
    single_cpi[17..19].copy_from_slice(&cpi.asset_index.to_le_bytes());
    single_cpi[19..27].copy_from_slice(&cpi.market_id.to_le_bytes());
    single_cpi[27..43].copy_from_slice(&cpi.size_q.to_le_bytes());
    single_cpi[43..51].copy_from_slice(&cpi.fee_bps.to_le_bytes());
    single_cpi[51..59].copy_from_slice(&cpi.limit_price.to_le_bytes());
    assert!(Instruction::decode(&single_cpi).is_err());

    let mut batch_cpi = [0u8; 60];
    batch_cpi[0] = 67;
    batch_cpi[1] = 1;
    batch_cpi[2..4].copy_from_slice(&cpi.asset_index.to_le_bytes());
    batch_cpi[4..12].copy_from_slice(&cpi.market_id.to_le_bytes());
    batch_cpi[12..28].copy_from_slice(&cpi.size_q.to_le_bytes());
    batch_cpi[28..36].copy_from_slice(&cpi.fee_bps.to_le_bytes());
    batch_cpi[36..44].copy_from_slice(&cpi.limit_price.to_le_bytes());
    batch_cpi[44..52].copy_from_slice(&cpi.account_a_portfolio_id.to_le_bytes());
    batch_cpi[52..60].copy_from_slice(&cpi.account_b_portfolio_id.to_le_bytes());
    assert!(Instruction::decode(&batch_cpi).is_err());
}

#[kani::proof]
fn kani_v16_permissionless_crank_decode_preserves_wire_fields() {
    let now_slot: u64 = kani::any();
    let asset_index_0: u16 = kani::any();
    let oracle_accounts_0: u8 = kani::any();
    let asset_index_1: u16 = kani::any();
    let oracle_accounts_1: u8 = kani::any();

    let mut data = [0u8; 16];
    data[0] = 5;
    data[1..9].copy_from_slice(&now_slot.to_le_bytes());
    data[9] = 2;
    data[10..12].copy_from_slice(&asset_index_0.to_le_bytes());
    data[12] = oracle_accounts_0;
    data[13..15].copy_from_slice(&asset_index_1.to_le_bytes());
    data[15] = oracle_accounts_1;

    kani::cover!(
        now_slot != 0 && asset_index_0 != asset_index_1 && oracle_accounts_0 != oracle_accounts_1,
        "crank wire proof covers distinct nontrivial observations"
    );
    match Instruction::decode(&data).unwrap() {
        Instruction::PermissionlessCrank {
            now_slot: got_slot,
            observations,
        } => {
            assert_eq!(got_slot, now_slot);
            assert_eq!(observations.len(), 2);
            assert_eq!(
                observations[0],
                CrankObservationHint {
                    asset_index: asset_index_0,
                    oracle_accounts: oracle_accounts_0,
                }
            );
            assert_eq!(
                observations[1],
                CrankObservationHint {
                    asset_index: asset_index_1,
                    oracle_accounts: oracle_accounts_1,
                }
            );
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_legacy_permissionless_crank_size_payload_is_rejected() {
    let now_slot: u64 = kani::any();
    let close_q: u128 = kani::any();
    let mut legacy = [0u8; 26];
    legacy[0] = 5;
    legacy[1..9].copy_from_slice(&now_slot.to_le_bytes());
    legacy[9..25].copy_from_slice(&close_q.to_le_bytes());
    legacy[25] = 0;

    kani::cover!(
        close_q as u8 <= 16,
        "legacy payload reaches the new observation parser and rejects trailing bytes"
    );
    kani::cover!(
        close_q as u8 > 16,
        "legacy payload rejects an oversized interpreted observation count"
    );
    assert!(Instruction::decode(&legacy).is_err());
}

#[kani::proof]
#[kani::unwind(34)]
fn kani_v16_update_authority_decode_preserves_wire_fields() {
    let authority_epoch: u64 = kani::any();
    let mut new_pubkey = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        new_pubkey[i] = kani::any();
        i += 1;
    }

    let mut data = [0u8; 41];
    data[0] = 32;
    data[1..9].copy_from_slice(&authority_epoch.to_le_bytes());
    data[9..41].copy_from_slice(&new_pubkey);

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateAuthority {
            authority_epoch: got_authority_epoch,
            new_pubkey: got_pubkey,
        } => {
            assert_eq!(got_authority_epoch, authority_epoch);
            assert_eq!(got_pubkey, new_pubkey);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn kani_v16_close_slab_decode_preserves_authority_epoch() {
    let authority_epoch: u64 = kani::any();
    let mut data = [0u8; 9];
    data[0] = 13;
    data[1..9].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::CloseSlab {
            authority_epoch: got_authority_epoch,
        } => assert_eq!(got_authority_epoch, authority_epoch),
        _ => unreachable!(),
    }
}

#[kani::proof]
#[kani::unwind(34)]
fn kani_v16_update_asset_authority_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
    let market_id: u64 = kani::any();
    let authority_epoch: u64 = kani::any();
    let kind: u8 = kani::any();
    let mut new_pubkey = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        new_pubkey[i] = kani::any();
        i += 1;
    }

    let mut data = [0u8; 52];
    data[0] = 65;
    data[1..3].copy_from_slice(&asset_index.to_le_bytes());
    data[3..11].copy_from_slice(&market_id.to_le_bytes());
    data[11..19].copy_from_slice(&authority_epoch.to_le_bytes());
    data[19] = kind;
    data[20..52].copy_from_slice(&new_pubkey);

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateAssetAuthority {
            asset_index: got_asset_index,
            market_id: got_market_id,
            authority_epoch: got_authority_epoch,
            kind: got_kind,
            new_pubkey: got_pubkey,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_authority_epoch, authority_epoch);
            assert_eq!(got_kind, kind);
            assert_eq!(got_pubkey, new_pubkey);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_restart_asset_oracle_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
    let market_id: u64 = kani::any();
    let now_slot: u64 = kani::any();
    let initial_price: u64 = kani::any();
    let observation_sequence: u64 = kani::any();
    let authority_epoch: u64 = kani::any();

    let data = Instruction::RestartAssetOracle {
        asset_index,
        market_id,
        now_slot,
        initial_price,
        observation_sequence,
        authority_epoch,
    }
    .encode();

    match Instruction::decode(&data).unwrap() {
        Instruction::RestartAssetOracle {
            asset_index: got_asset_index,
            market_id: got_market_id,
            now_slot: got_slot,
            initial_price: got_price,
            observation_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_slot, now_slot);
            assert_eq!(got_price, initial_price);
            assert_eq!(got_sequence, observation_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_liquidation_fee_policy_decode_preserves_wire_fields() {
    let cranker_share_bps: u16 = kani::any();
    let policy_sequence: u64 = kani::any();
    let authority_epoch: u64 = kani::any();

    let mut data = [0u8; 19];
    data[0] = 37;
    data[1..3].copy_from_slice(&cranker_share_bps.to_le_bytes());
    data[3..11].copy_from_slice(&policy_sequence.to_le_bytes());
    data[11..19].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: got,
            policy_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got, cranker_share_bps);
            assert_eq!(got_sequence, policy_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_maintenance_fee_policy_decode_preserves_wire_fields() {
    let cranker_share_bps: u16 = kani::any();
    let policy_sequence: u64 = kani::any();
    let authority_epoch: u64 = kani::any();

    let mut data = [0u8; 19];
    data[0] = 49;
    data[1..3].copy_from_slice(&cranker_share_bps.to_le_bytes());
    data[3..11].copy_from_slice(&policy_sequence.to_le_bytes());
    data[11..19].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: got,
            policy_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got, cranker_share_bps);
            assert_eq!(got_sequence, policy_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_backing_fee_policy_decode_preserves_wire_fields() {
    let domain: u16 = kani::any();
    let market_id: u64 = kani::any();
    let fee_bps: u16 = kani::any();
    let insurance_share_bps: u16 = kani::any();
    let policy_sequence: u64 = kani::any();
    let authority_epoch: u64 = kani::any();

    let mut data = [0u8; 31];
    data[0] = 51;
    data[1..3].copy_from_slice(&domain.to_le_bytes());
    data[3..11].copy_from_slice(&market_id.to_le_bytes());
    data[11..13].copy_from_slice(&fee_bps.to_le_bytes());
    data[13..15].copy_from_slice(&insurance_share_bps.to_le_bytes());
    data[15..23].copy_from_slice(&policy_sequence.to_le_bytes());
    data[23..31].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateBackingFeePolicy {
            domain: got_domain,
            market_id: got_market_id,
            fee_bps: got_fee_bps,
            insurance_share_bps: got_insurance_share_bps,
            policy_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got_domain, domain);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_fee_bps, fee_bps);
            assert_eq!(got_insurance_share_bps, insurance_share_bps);
            assert_eq!(got_sequence, policy_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_trade_fee_policy_decode_preserves_wire_fields() {
    let trade_fee_base_bps: u64 = kani::any();
    let policy_sequence: u64 = kani::any();
    let authority_epoch: u64 = kani::any();

    let mut data = [0u8; 25];
    data[0] = 55;
    data[1..9].copy_from_slice(&trade_fee_base_bps.to_le_bytes());
    data[9..17].copy_from_slice(&policy_sequence.to_le_bytes());
    data[17..25].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: got,
            policy_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got, trade_fee_base_bps);
            assert_eq!(got_sequence, policy_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_fee_redirect_policy_decode_preserves_wire_fields() {
    let redirect_bps: u16 = kani::any();
    let policy_sequence: u64 = kani::any();
    let authority_epoch: u64 = kani::any();

    let mut data = [0u8; 19];
    data[0] = 58;
    data[1..3].copy_from_slice(&redirect_bps.to_le_bytes());
    data[3..11].copy_from_slice(&policy_sequence.to_le_bytes());
    data[11..19].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateFeeRedirectPolicy {
            redirect_bps: got,
            policy_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got, redirect_bps);
            assert_eq!(got_sequence, policy_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_market_init_fee_policy_decode_preserves_wire_fields() {
    let min_init_fee: u128 = kani::any();
    let policy_sequence: u64 = kani::any();
    let authority_epoch: u64 = kani::any();

    let mut data = [0u8; 33];
    data[0] = 59;
    data[1..17].copy_from_slice(&min_init_fee.to_le_bytes());
    data[17..25].copy_from_slice(&policy_sequence.to_le_bytes());
    data[25..33].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateMarketInitFeePolicy {
            min_init_fee: got,
            policy_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got, min_init_fee);
            assert_eq!(got_sequence, policy_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_permissionless_resolve_decode_preserves_wire_fields() {
    let asset_generation_frontier: u64 = kani::any();
    let authority_epoch: u64 = kani::any();
    let stale_slots: u64 = kani::any();
    let force_close_delay_slots: u64 = kani::any();
    let policy_sequence: u64 = kani::any();
    let now_slot: u64 = kani::any();

    let mut resolve_market = [0u8; 17];
    resolve_market[0] = 19;
    resolve_market[1..9].copy_from_slice(&asset_generation_frontier.to_le_bytes());
    resolve_market[9..17].copy_from_slice(&authority_epoch.to_le_bytes());
    match Instruction::decode(&resolve_market).unwrap() {
        Instruction::ResolveMarket {
            asset_generation_frontier: got_frontier,
            authority_epoch: got_epoch,
        } => {
            assert_eq!(got_frontier, asset_generation_frontier);
            assert_eq!(got_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }

    let mut configure = [0u8; 41];
    configure[0] = 38;
    configure[1..9].copy_from_slice(&asset_generation_frontier.to_le_bytes());
    configure[9..17].copy_from_slice(&stale_slots.to_le_bytes());
    configure[17..25].copy_from_slice(&force_close_delay_slots.to_le_bytes());
    configure[25..33].copy_from_slice(&policy_sequence.to_le_bytes());
    configure[33..41].copy_from_slice(&authority_epoch.to_le_bytes());
    match Instruction::decode(&configure).unwrap() {
        Instruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: got_frontier,
            stale_slots: got_stale,
            force_close_delay_slots: got_delay,
            policy_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got_frontier, asset_generation_frontier);
            assert_eq!(got_stale, stale_slots);
            assert_eq!(got_delay, force_close_delay_slots);
            assert_eq!(got_sequence, policy_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }

    let mut resolve = [0u8; 9];
    resolve[0] = 39;
    resolve[1..9].copy_from_slice(&now_slot.to_le_bytes());
    match Instruction::decode(&resolve).unwrap() {
        Instruction::ResolveStalePermissionless { now_slot: got } => {
            assert_eq!(got, now_slot);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_configure_hybrid_oracle_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
    let market_id: u64 = kani::any();
    let oracle_leg_count: u8 = kani::any();
    let oracle_leg_flags: u8 = kani::any();
    let invert: u8 = kani::any();
    let conf_filter_bps: u16 = kani::any();
    let now_slot: u64 = kani::any();
    let now_unix_ts: i64 = kani::any();
    let max_staleness_secs: u64 = kani::any();
    let hybrid_soft_stale_slots: u64 = kani::any();
    let mark_ewma_halflife_slots: u64 = kani::any();
    let mark_min_fee: u64 = kani::any();
    let unit_scale: u32 = kani::any();
    let feeds: [[u8; 32]; 3] = kani::any();
    let observation_sequence: u64 = kani::any();
    let authority_epoch: u64 = kani::any();
    let feed_index: usize = kani::any();
    let byte_index: usize = kani::any();
    kani::assume(feed_index < feeds.len());
    kani::assume(byte_index < feeds[0].len());

    let mut data = [0u8; 179];
    data[0..2].copy_from_slice(&asset_index.to_le_bytes());
    data[2..10].copy_from_slice(&market_id.to_le_bytes());
    data[10..18].copy_from_slice(&now_slot.to_le_bytes());
    data[18..26].copy_from_slice(&now_unix_ts.to_le_bytes());
    data[26] = oracle_leg_count;
    data[27] = oracle_leg_flags;
    data[28..36].copy_from_slice(&max_staleness_secs.to_le_bytes());
    data[36..44].copy_from_slice(&hybrid_soft_stale_slots.to_le_bytes());
    data[44..52].copy_from_slice(&mark_ewma_halflife_slots.to_le_bytes());
    data[52..60].copy_from_slice(&mark_min_fee.to_le_bytes());
    data[60] = invert;
    data[61..65].copy_from_slice(&unit_scale.to_le_bytes());
    data[65..67].copy_from_slice(&conf_filter_bps.to_le_bytes());
    data[67..99].copy_from_slice(&feeds[0]);
    data[99..131].copy_from_slice(&feeds[1]);
    data[131..163].copy_from_slice(&feeds[2]);
    data[163..171].copy_from_slice(&observation_sequence.to_le_bytes());
    data[171..179].copy_from_slice(&authority_epoch.to_le_bytes());

    match Instruction::decode_body_for_proof(34, &data).unwrap() {
        Instruction::ConfigureHybridOracle {
            asset_index: got_asset_index,
            market_id: got_market_id,
            now_slot: got_now_slot,
            now_unix_ts: got_now_unix,
            oracle_leg_count: got_count,
            oracle_leg_flags: got_flags,
            max_staleness_secs: got_max_staleness,
            hybrid_soft_stale_slots: got_soft,
            mark_ewma_halflife_slots: got_halflife,
            mark_min_fee: got_min_fee,
            invert: got_invert,
            unit_scale: got_unit_scale,
            conf_filter_bps: got_conf,
            oracle_leg_feeds: got_feeds,
            observation_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_now_slot, now_slot);
            assert_eq!(got_now_unix, now_unix_ts);
            assert_eq!(got_count, oracle_leg_count);
            assert_eq!(got_flags, oracle_leg_flags);
            assert_eq!(got_max_staleness, max_staleness_secs);
            assert_eq!(got_soft, hybrid_soft_stale_slots);
            assert_eq!(got_halflife, mark_ewma_halflife_slots);
            assert_eq!(got_min_fee, mark_min_fee);
            assert_eq!(got_invert, invert);
            assert_eq!(got_unit_scale, unit_scale);
            assert_eq!(got_conf, conf_filter_bps);
            assert_eq!(got_sequence, observation_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
            // Arbitrary indices make this equivalent to whole-matrix equality without lowering
            // the 96-byte symbolic comparison to a SAT-heavy memcmp loop.
            assert_eq!(
                got_feeds[feed_index][byte_index],
                feeds[feed_index][byte_index]
            );
        }
        _ => unreachable!(),
    }

    let extra: u8 = kani::any();
    let mut trailing = [0u8; 180];
    trailing[..179].copy_from_slice(&data);
    trailing[179] = extra;
    assert!(Instruction::decode_body_for_proof(34, &trailing).is_err());
}

#[kani::proof]
fn kani_v16_generationless_hybrid_oracle_payload_rejects() {
    let legacy_body: [u8; 163] = kani::any();
    assert!(Instruction::decode_body_for_proof(34, &legacy_body).is_err());
}

#[kani::proof]
fn kani_v16_ewma_mark_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
    let market_id: u64 = kani::any();
    let now_slot: u64 = kani::any();
    let initial_mark_e6: u64 = kani::any();
    let mark_ewma_halflife_slots: u64 = kani::any();
    let mark_min_fee: u64 = kani::any();
    let push_mark_e6: u64 = kani::any();
    let observation_sequence: u64 = kani::any();
    let authority_epoch: u64 = kani::any();

    let mut configure = [0u8; 59];
    configure[0] = 35;
    configure[1..3].copy_from_slice(&asset_index.to_le_bytes());
    configure[3..11].copy_from_slice(&market_id.to_le_bytes());
    configure[11..19].copy_from_slice(&now_slot.to_le_bytes());
    configure[19..27].copy_from_slice(&initial_mark_e6.to_le_bytes());
    configure[27..35].copy_from_slice(&mark_ewma_halflife_slots.to_le_bytes());
    configure[35..43].copy_from_slice(&mark_min_fee.to_le_bytes());
    configure[43..51].copy_from_slice(&observation_sequence.to_le_bytes());
    configure[51..59].copy_from_slice(&authority_epoch.to_le_bytes());
    match Instruction::decode(&configure).unwrap() {
        Instruction::ConfigureEwmaMark {
            asset_index: got_asset_index,
            market_id: got_market_id,
            now_slot: got_now,
            initial_mark_e6: got_mark,
            mark_ewma_halflife_slots: got_halflife,
            mark_min_fee: got_min_fee,
            observation_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_now, now_slot);
            assert_eq!(got_mark, initial_mark_e6);
            assert_eq!(got_halflife, mark_ewma_halflife_slots);
            assert_eq!(got_min_fee, mark_min_fee);
            assert_eq!(got_sequence, observation_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }

    let mut push = [0u8; 43];
    push[0] = 36;
    push[1..3].copy_from_slice(&asset_index.to_le_bytes());
    push[3..11].copy_from_slice(&market_id.to_le_bytes());
    push[11..19].copy_from_slice(&now_slot.to_le_bytes());
    push[19..27].copy_from_slice(&push_mark_e6.to_le_bytes());
    push[27..35].copy_from_slice(&observation_sequence.to_le_bytes());
    push[35..43].copy_from_slice(&authority_epoch.to_le_bytes());
    match Instruction::decode(&push).unwrap() {
        Instruction::PushEwmaMark {
            asset_index: got_asset_index,
            market_id: got_market_id,
            now_slot: got_now,
            mark_e6: got_mark,
            observation_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_now, now_slot);
            assert_eq!(got_mark, push_mark_e6);
            assert_eq!(got_sequence, observation_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }

    let mut configure_auth = [0u8; 43];
    configure_auth[0] = 62;
    configure_auth[1..3].copy_from_slice(&asset_index.to_le_bytes());
    configure_auth[3..11].copy_from_slice(&market_id.to_le_bytes());
    configure_auth[11..19].copy_from_slice(&now_slot.to_le_bytes());
    configure_auth[19..27].copy_from_slice(&initial_mark_e6.to_le_bytes());
    configure_auth[27..35].copy_from_slice(&observation_sequence.to_le_bytes());
    configure_auth[35..43].copy_from_slice(&authority_epoch.to_le_bytes());
    match Instruction::decode(&configure_auth).unwrap() {
        Instruction::ConfigureAuthMark {
            asset_index: got_asset_index,
            market_id: got_market_id,
            now_slot: got_now,
            initial_mark_e6: got_mark,
            observation_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_now, now_slot);
            assert_eq!(got_mark, initial_mark_e6);
            assert_eq!(got_sequence, observation_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }

    let mut push_auth = [0u8; 43];
    push_auth[0] = 63;
    push_auth[1..3].copy_from_slice(&asset_index.to_le_bytes());
    push_auth[3..11].copy_from_slice(&market_id.to_le_bytes());
    push_auth[11..19].copy_from_slice(&now_slot.to_le_bytes());
    push_auth[19..27].copy_from_slice(&push_mark_e6.to_le_bytes());
    push_auth[27..35].copy_from_slice(&observation_sequence.to_le_bytes());
    push_auth[35..43].copy_from_slice(&authority_epoch.to_le_bytes());
    match Instruction::decode(&push_auth).unwrap() {
        Instruction::PushAuthMark {
            asset_index: got_asset_index,
            market_id: got_market_id,
            now_slot: got_now,
            mark_e6: got_mark,
            observation_sequence: got_sequence,
            authority_epoch: got_authority_epoch,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_market_id, market_id);
            assert_eq!(got_now, now_slot);
            assert_eq!(got_mark, push_mark_e6);
            assert_eq!(got_sequence, observation_sequence);
            assert_eq!(got_authority_epoch, authority_epoch);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_generationless_ewma_config_payload_rejects() {
    let mut ewma_config = [0u8; 43];
    ewma_config[0] = 35;
    assert!(Instruction::decode(&ewma_config).is_err());
}

#[kani::proof]
fn kani_v16_generationless_ewma_push_payload_rejects() {
    let mut ewma_push = [0u8; 27];
    ewma_push[0] = 36;
    assert!(Instruction::decode(&ewma_push).is_err());
}

#[kani::proof]
fn kani_v16_generationless_auth_config_payload_rejects() {
    let mut auth_config = [0u8; 27];
    auth_config[0] = 62;
    assert!(Instruction::decode(&auth_config).is_err());
}

#[kani::proof]
fn kani_v16_generationless_auth_push_payload_rejects() {
    let mut auth_push = [0u8; 27];
    auth_push[0] = 63;
    assert!(Instruction::decode(&auth_push).is_err());
}

#[kani::proof]
fn kani_v16_generationless_restart_payload_rejects() {
    let mut restart = [0u8; 27];
    restart[0] = 69;
    assert!(Instruction::decode(&restart).is_err());
}

#[kani::proof]
fn kani_v16_decode_rejects_trailing_bytes() {
    let extra: u8 = kani::any();
    let data = [1u8, extra];
    assert!(Instruction::decode(&data).is_err());
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_custody_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(Instruction::InitPortfolio, extra);
    assert_rejects_trailing_byte(
        Instruction::Deposit {
            portfolio_id: 1,
            expected_sequence: 1,
            amount: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::Withdraw {
            portfolio_id: 1,
            expected_sequence: 1,
            amount: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::TopUpInsurance {
            intent_id: 1,
            market_id: 1,
            authority_epoch: 0,
            amount: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::TopUpBackingBucket {
            intent_id: 1,
            domain: 1,
            market_id: 1,
            authority_epoch: 0,
            amount: 1,
            expiry_slot: 10,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::TopUpInsuranceDomain {
            intent_id: 1,
            domain: 1,
            market_id: 1,
            authority_epoch: 0,
            amount: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            market_id: 1,
            authority_epoch: 0,
            amount: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id: 1,
            authority_epoch: 0,
            amount: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: 1,
            authority_epoch: 0,
            amount: 1,
        },
        extra,
    );
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_trade_and_crank_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(
        Instruction::PermissionlessCrank {
            now_slot: 1,
            observations: vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::TradeNoCpi {
            account_a_portfolio_id: 1,
            account_a_position_epoch: 1,
            account_b_portfolio_id: 2,
            account_b_position_epoch: 1,
            asset_index: 0,
            market_id: 1,
            size_q: 1,
            exec_price: 100,
            fee_bps: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::TradeCpi {
            account_a_portfolio_id: 1,
            account_a_position_epoch: 1,
            account_b_portfolio_id: 2,
            account_b_position_epoch: 1,
            asset_index: 0,
            market_id: 1,
            size_q: 1,
            fee_bps: 0,
            limit_price: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(Instruction::SyncMaintenanceFee { now_slot: 1 }, extra);
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_close_and_resolve_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(Instruction::CloseSlab { authority_epoch: 1 }, extra);
    assert_rejects_trailing_byte(
        Instruction::ResolveMarket {
            asset_generation_frontier: 1,
            authority_epoch: 2,
        },
        extra,
    );
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_update_authority_payload_rejects_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(
        Instruction::UpdateAuthority {
            authority_epoch: 1,
            new_pubkey: [1u8; 32],
        },
        extra,
    );
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_update_asset_authority_payload_rejects_trailing_byte() {
    let extra: u8 = kani::any();
    let mut data = [0u8; 53];
    data[0] = 65;
    data[1..3].copy_from_slice(&1u16.to_le_bytes());
    data[3..11].copy_from_slice(&2u64.to_le_bytes());
    data[11..19].copy_from_slice(&3u64.to_le_bytes());
    data[19] = 0;
    data[20..52].copy_from_slice(&[1u8; 32]);
    data[52] = extra;
    assert!(Instruction::decode(&data).is_err());
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_policy_fee_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 4_000,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 4_000,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateBackingFeePolicy {
            domain: 0,
            market_id: 1,
            fee_bps: 25,
            insurance_share_bps: 0,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 25,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateFeeRedirectPolicy {
            redirect_bps: 250,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateMarketInitFeePolicy {
            min_init_fee: 50,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        extra,
    );
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_permissionless_config_payload_rejects_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(
        Instruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 1,
            stale_slots: 5,
            force_close_delay_slots: 1,
            policy_sequence: 1,
            authority_epoch: 0,
        },
        extra,
    );
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_permissionless_resolve_payload_rejects_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(
        Instruction::ResolveStalePermissionless { now_slot: 5 },
        extra,
    );
}

#[kani::proof]
fn kani_v16_mark_oracle_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    let mut configure_ewma = [0u8; 60];
    configure_ewma[0] = 35;
    configure_ewma[59] = extra;
    assert!(Instruction::decode(&configure_ewma).is_err());

    let mut push_ewma = [0u8; 44];
    push_ewma[0] = 36;
    push_ewma[43] = extra;
    assert!(Instruction::decode(&push_ewma).is_err());

    let mut configure_auth = [0u8; 44];
    configure_auth[0] = 62;
    configure_auth[43] = extra;
    assert!(Instruction::decode(&configure_auth).is_err());

    let mut push_auth = [0u8; 44];
    push_auth[0] = 63;
    push_auth[43] = extra;
    assert!(Instruction::decode(&push_auth).is_err());

    let mut restart = [0u8; 44];
    restart[0] = 69;
    restart[43] = extra;
    assert!(Instruction::decode(&restart).is_err());
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_resolved_recovery_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(
        Instruction::ConvertReleasedPnl {
            portfolio_id: 1,
            position_epoch: 2,
            amount: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::CureAndCancelClose {
            portfolio_id: 1,
            position_epoch: 2,
            optional_deposit: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::ForfeitRecoveryLeg {
            portfolio_id: 1,
            position_epoch: 2,
            asset_index: 0,
            b_delta_budget: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::RebalanceReduce {
            portfolio_id: 1,
            position_epoch: 2,
            asset_index: 0,
            reduce_q: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::ForceCloseAbandonedAsset {
            asset_index: 0,
            now_slot: 1,
            close_q: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(Instruction::ClaimResolvedPayoutTopup, extra);
    assert_rejects_trailing_byte(
        Instruction::ClosePortfolio {
            portfolio_id: 1,
            expected_sequence: 2,
            position_epoch: 3,
        },
        extra,
    );
}

#[kani::proof]
#[kani::unwind(24)]
fn kani_v16_unknown_or_truncated_tags_reject() {
    for tag in [
        2u8, 7, 11, 12, 14, 15, 16, 17, 18, 20, 21, 22, 25, 26, 27, 29, 31, 47, 70, 127, 255,
    ] {
        assert!(Instruction::decode(&[tag]).is_err());
    }

    let deposit_tag_only = [3u8];
    assert!(Instruction::decode(&deposit_tag_only).is_err());
}

#[kani::proof]
fn kani_v16_refine_resolved_bound_tag_is_not_public() {
    let decrease_num: u128 = kani::any();
    let mut data = [0u8; 17];
    data[0] = 47;
    data[1..].copy_from_slice(&decrease_num.to_le_bytes());

    assert!(Instruction::decode(&data).is_err());
}

#[kani::proof]
fn kani_v16_zero_length_decode_rejects() {
    let data: [u8; 0] = [];
    assert!(Instruction::decode(&data).is_err());
}

#[kani::proof]
fn kani_v16_core_payloads_reject_one_byte_truncation() {
    let init_market = [0u8; 80];
    assert!(Instruction::decode(&init_market).is_err());

    let deposit = [3u8; 16];
    assert!(Instruction::decode(&deposit).is_err());

    let withdraw = [4u8; 16];
    assert!(Instruction::decode(&withdraw).is_err());

    let crank = [5u8; 59];
    assert!(Instruction::decode(&crank).is_err());

    // The shipping lifecycle instruction is exactly 180 bytes including its tag.
    let asset_lifecycle = [40u8; 179];
    assert!(Instruction::decode(&asset_lifecycle).is_err());

    let trade = [6u8; 33];
    assert!(Instruction::decode(&trade).is_err());

    let trade_cpi = [10u8; 33];
    assert!(Instruction::decode(&trade_cpi).is_err());
}

#[kani::proof]
fn kani_v16_funding_backing_payloads_reject_one_byte_truncation() {
    let top_up = [9u8; 40];
    assert!(Instruction::decode(&top_up).is_err());

    let top_up_domain = [56u8; 42];
    assert!(Instruction::decode(&top_up_domain).is_err());

    let top_up_backing = [24u8; 50];
    assert!(Instruction::decode(&top_up_backing).is_err());

    let withdraw_insurance = [23u8; 16];
    assert!(Instruction::decode(&withdraw_insurance).is_err());

    let withdraw_backing = [50u8; 34];
    assert!(Instruction::decode(&withdraw_backing).is_err());

    let withdraw_backing_earnings = [52u8; 34];
    assert!(Instruction::decode(&withdraw_backing_earnings).is_err());

    let withdraw_insurance_domain = [57u8; 34];
    assert!(Instruction::decode(&withdraw_insurance_domain).is_err());

    let convert_pnl = [28u8; 16];
    assert!(Instruction::decode(&convert_pnl).is_err());

    let close_resolved = [30u8; 16];
    assert!(Instruction::decode(&close_resolved).is_err());
}

#[kani::proof]
fn kani_v16_authority_oracle_payloads_reject_one_byte_truncation() {
    let update_authority = [32u8; 32];
    assert!(Instruction::decode(&update_authority).is_err());

    let update_asset_authority = [65u8; 43];
    assert!(Instruction::decode(&update_asset_authority).is_err());

    let update_insurance = [33u8; 11];
    assert!(Instruction::decode(&update_insurance).is_err());

    let configure_hybrid = [34u8; 179];
    assert!(Instruction::decode(&configure_hybrid).is_err());

    let configure_ewma_mark = [35u8; 58];
    assert!(Instruction::decode(&configure_ewma_mark).is_err());

    let push_ewma_mark = [36u8; 42];
    assert!(Instruction::decode(&push_ewma_mark).is_err());

    let configure_auth_mark = [62u8; 42];
    assert!(Instruction::decode(&configure_auth_mark).is_err());

    let push_auth_mark = [63u8; 42];
    assert!(Instruction::decode(&push_auth_mark).is_err());

    let restart_asset_oracle = [69u8; 42];
    assert!(Instruction::decode(&restart_asset_oracle).is_err());
}

#[kani::proof]
fn kani_v16_policy_permissionless_payloads_reject_one_byte_truncation() {
    let update_liquidation = [37u8; 18];
    assert!(Instruction::decode(&update_liquidation).is_err());

    let update_maintenance = [49u8; 18];
    assert!(Instruction::decode(&update_maintenance).is_err());

    let update_backing = [51u8; 30];
    assert!(Instruction::decode(&update_backing).is_err());

    let update_trade = [55u8; 24];
    assert!(Instruction::decode(&update_trade).is_err());

    let update_redirect = [58u8; 18];
    assert!(Instruction::decode(&update_redirect).is_err());

    let update_market_init = [59u8; 32];
    assert!(Instruction::decode(&update_market_init).is_err());

    let update_base_units = [60u8; 72];
    assert!(Instruction::decode(&update_base_units).is_err());

    let swap_base_units = [61u8; 24];
    assert!(Instruction::decode(&swap_base_units).is_err());

    let configure_permissionless = [38u8; 40];
    assert!(Instruction::decode(&configure_permissionless).is_err());

    let resolve_permissionless = [39u8; 8];
    assert!(Instruction::decode(&resolve_permissionless).is_err());
}

#[kani::proof]
fn kani_v16_resolved_progress_payloads_reject_one_byte_truncation() {
    let withdraw_insurance_full = [41u8; 16];
    assert!(Instruction::decode(&withdraw_insurance_full).is_err());

    let cure = [42u8; 24];
    assert!(Instruction::decode(&cure).is_err());

    let forfeit = [43u8; 16];
    assert!(Instruction::decode(&forfeit).is_err());

    let rebalance = [44u8; 16];
    assert!(Instruction::decode(&rebalance).is_err());

    let finalize = [45u8; 2];
    assert!(Instruction::decode(&finalize).is_err());

    let refine = [47u8; 16];
    assert!(Instruction::decode(&refine).is_err());

    let sync_fee = [48u8; 8];
    assert!(Instruction::decode(&sync_fee).is_err());

    let force_close = [64u8; 26];
    assert!(Instruction::decode(&force_close).is_err());
}
