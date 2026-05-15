#![cfg(kani)]

extern crate kani;

use percolator_prog::ix::Instruction;

#[kani::proof]
fn kani_v13_tradenocpi_decode_preserves_wire_fields() {
    let asset_index: u8 = kani::any();
    let size_raw: i16 = kani::any();
    let exec_price_raw: u16 = kani::any();
    let fee_bps_raw: u16 = kani::any();
    let size_q = size_raw as i128;
    let exec_price = exec_price_raw as u64;
    let fee_bps = fee_bps_raw as u64;

    let mut data = [0u8; 34];
    data[0] = 6;
    data[1] = asset_index;
    data[2..18].copy_from_slice(&size_q.to_le_bytes());
    data[18..26].copy_from_slice(&exec_price.to_le_bytes());
    data[26..34].copy_from_slice(&fee_bps.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::TradeNoCpi {
            asset_index: got_asset,
            size_q: got_size,
            exec_price: got_price,
            fee_bps: got_fee,
        } => {
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_size, size_q);
            assert_eq!(got_price, exec_price);
            assert_eq!(got_fee, fee_bps);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v13_permissionless_crank_decode_preserves_wire_fields() {
    let action: u8 = kani::any();
    let asset_index: u8 = kani::any();
    let now_slot_raw: u16 = kani::any();
    let effective_price_raw: u16 = kani::any();
    let funding_rate_raw: i16 = kani::any();
    let close_q_raw: u16 = kani::any();
    let fee_bps_raw: u16 = kani::any();
    let recovery_reason: u8 = kani::any();
    let now_slot = now_slot_raw as u64;
    let effective_price = effective_price_raw as u64;
    let funding_rate_e9 = funding_rate_raw as i128;
    let close_q = close_q_raw as u128;
    let fee_bps = fee_bps_raw as u64;

    let mut data = [0u8; 60];
    data[0] = 5;
    data[1] = action;
    data[2] = asset_index;
    data[3..11].copy_from_slice(&now_slot.to_le_bytes());
    data[11..19].copy_from_slice(&effective_price.to_le_bytes());
    data[19..35].copy_from_slice(&funding_rate_e9.to_le_bytes());
    data[35..51].copy_from_slice(&close_q.to_le_bytes());
    data[51..59].copy_from_slice(&fee_bps.to_le_bytes());
    data[59] = recovery_reason;

    match Instruction::decode(&data).unwrap() {
        Instruction::PermissionlessCrank {
            action: got_action,
            asset_index: got_asset,
            now_slot: got_slot,
            effective_price: got_price,
            funding_rate_e9: got_rate,
            close_q: got_close,
            fee_bps: got_fee,
            recovery_reason: got_recovery,
        } => {
            assert_eq!(got_action, action);
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_slot, now_slot);
            assert_eq!(got_price, effective_price);
            assert_eq!(got_rate, funding_rate_e9);
            assert_eq!(got_close, close_q);
            assert_eq!(got_fee, fee_bps);
            assert_eq!(got_recovery, recovery_reason);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v13_decode_rejects_trailing_bytes() {
    let extra: u8 = kani::any();
    let data = [1u8, extra];
    assert!(Instruction::decode(&data).is_err());
}
