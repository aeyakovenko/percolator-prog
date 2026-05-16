#![cfg(kani)]

extern crate kani;

use percolator_prog::ix::Instruction;

#[kani::proof]
fn kani_v13_init_market_decode_preserves_wire_fields() {
    let h_min_raw: u16 = kani::any();
    let h_max_raw: u16 = kani::any();
    let initial_price_raw: u16 = kani::any();
    let maintenance_margin_bps_raw: u16 = kani::any();
    let initial_margin_bps_raw: u16 = kani::any();
    let max_trading_fee_bps_raw: u16 = kani::any();
    let max_price_move_bps_raw: u16 = kani::any();
    let max_accrual_dt_raw: u16 = kani::any();
    let maintenance_fee_raw: u16 = kani::any();
    let h_min = h_min_raw as u64;
    let h_max = h_max_raw as u64;
    let initial_price = initial_price_raw as u64;
    let maintenance_margin_bps = maintenance_margin_bps_raw as u64;
    let initial_margin_bps = initial_margin_bps_raw as u64;
    let max_trading_fee_bps = max_trading_fee_bps_raw as u64;
    let max_price_move_bps_per_slot = max_price_move_bps_raw as u64;
    let max_accrual_dt_slots = max_accrual_dt_raw as u64;
    let maintenance_fee_per_slot = maintenance_fee_raw as u128;

    let mut data = [0u8; 81];
    data[0] = 0;
    data[1..9].copy_from_slice(&h_min.to_le_bytes());
    data[9..17].copy_from_slice(&h_max.to_le_bytes());
    data[17..25].copy_from_slice(&initial_price.to_le_bytes());
    data[25..33].copy_from_slice(&maintenance_margin_bps.to_le_bytes());
    data[33..41].copy_from_slice(&initial_margin_bps.to_le_bytes());
    data[41..49].copy_from_slice(&max_trading_fee_bps.to_le_bytes());
    data[49..57].copy_from_slice(&max_price_move_bps_per_slot.to_le_bytes());
    data[57..65].copy_from_slice(&max_accrual_dt_slots.to_le_bytes());
    data[65..81].copy_from_slice(&maintenance_fee_per_slot.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::InitMarket {
            h_min: got_h_min,
            h_max: got_h_max,
            initial_price: got_initial_price,
            maintenance_margin_bps: got_mm,
            initial_margin_bps: got_im,
            max_trading_fee_bps: got_fee,
            max_price_move_bps_per_slot: got_move,
            max_accrual_dt_slots: got_dt,
            maintenance_fee_per_slot: got_maintenance_fee,
        } => {
            assert_eq!(got_h_min, h_min);
            assert_eq!(got_h_max, h_max);
            assert_eq!(got_initial_price, initial_price);
            assert_eq!(got_mm, maintenance_margin_bps);
            assert_eq!(got_im, initial_margin_bps);
            assert_eq!(got_fee, max_trading_fee_bps);
            assert_eq!(got_move, max_price_move_bps_per_slot);
            assert_eq!(got_dt, max_accrual_dt_slots);
            assert_eq!(got_maintenance_fee, maintenance_fee_per_slot);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v13_amount_instructions_decode_preserves_wire_fields() {
    let tag: u8 = kani::any();
    kani::assume(tag == 3 || tag == 4 || tag == 9 || tag == 30);
    let amount_raw: u16 = kani::any();
    let amount = amount_raw as u128;

    let mut data = [0u8; 17];
    data[0] = tag;
    data[1..17].copy_from_slice(&amount.to_le_bytes());

    match (tag, Instruction::decode(&data).unwrap()) {
        (3, Instruction::Deposit { amount: got }) => assert_eq!(got, amount),
        (4, Instruction::Withdraw { amount: got }) => assert_eq!(got, amount),
        (9, Instruction::TopUpInsurance { amount: got }) => assert_eq!(got, amount),
        (30, Instruction::CloseResolved { fee_rate_per_slot }) => {
            assert_eq!(fee_rate_per_slot, amount)
        }
        _ => unreachable!(),
    }
}

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
fn kani_v13_update_authority_decode_preserves_wire_fields() {
    let kind: u8 = kani::any();
    let mut new_pubkey = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        new_pubkey[i] = kani::any();
        i += 1;
    }

    let mut data = [0u8; 34];
    data[0] = 32;
    data[1] = kind;
    data[2..34].copy_from_slice(&new_pubkey);

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateAuthority {
            kind: got_kind,
            new_pubkey: got_pubkey,
        } => {
            assert_eq!(got_kind, kind);
            assert_eq!(got_pubkey, new_pubkey);
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

#[kani::proof]
fn kani_v13_every_active_payload_rejects_trailing_byte() {
    let extra: u8 = kani::any();

    let mut init_market = Instruction::InitMarket {
        h_min: 1,
        h_max: 2,
        initial_price: 100,
        maintenance_margin_bps: 500,
        initial_margin_bps: 1_000,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 10,
        maintenance_fee_per_slot: 0,
    }
    .encode();
    init_market.push(extra);
    assert!(Instruction::decode(&init_market).is_err());

    let mut init_portfolio = Instruction::InitPortfolio.encode();
    init_portfolio.push(extra);
    assert!(Instruction::decode(&init_portfolio).is_err());

    let mut deposit = Instruction::Deposit { amount: 1 }.encode();
    deposit.push(extra);
    assert!(Instruction::decode(&deposit).is_err());

    let mut withdraw = Instruction::Withdraw { amount: 1 }.encode();
    withdraw.push(extra);
    assert!(Instruction::decode(&withdraw).is_err());

    let mut crank = Instruction::PermissionlessCrank {
        action: 0,
        asset_index: 0,
        now_slot: 1,
        effective_price: 100,
        funding_rate_e9: 0,
        close_q: 0,
        fee_bps: 0,
        recovery_reason: 0,
    }
    .encode();
    crank.push(extra);
    assert!(Instruction::decode(&crank).is_err());

    let mut trade = Instruction::TradeNoCpi {
        asset_index: 0,
        size_q: 1,
        exec_price: 100,
        fee_bps: 0,
    }
    .encode();
    trade.push(extra);
    assert!(Instruction::decode(&trade).is_err());

    let mut close_portfolio = Instruction::ClosePortfolio.encode();
    close_portfolio.push(extra);
    assert!(Instruction::decode(&close_portfolio).is_err());

    let mut top_up = Instruction::TopUpInsurance { amount: 1 }.encode();
    top_up.push(extra);
    assert!(Instruction::decode(&top_up).is_err());

    let mut close_slab = Instruction::CloseSlab.encode();
    close_slab.push(extra);
    assert!(Instruction::decode(&close_slab).is_err());

    let mut resolve = Instruction::ResolveMarket.encode();
    resolve.push(extra);
    assert!(Instruction::decode(&resolve).is_err());

    let mut close_resolved = Instruction::CloseResolved {
        fee_rate_per_slot: 0,
    }
    .encode();
    close_resolved.push(extra);
    assert!(Instruction::decode(&close_resolved).is_err());

    let mut update_authority = Instruction::UpdateAuthority {
        kind: 0,
        new_pubkey: [1u8; 32],
    }
    .encode();
    update_authority.push(extra);
    assert!(Instruction::decode(&update_authority).is_err());
}

#[kani::proof]
fn kani_v13_unknown_or_truncated_tags_reject() {
    let tag: u8 = kani::any();
    kani::assume(tag != 0);
    kani::assume(tag != 1);
    kani::assume(tag != 3);
    kani::assume(tag != 4);
    kani::assume(tag != 5);
    kani::assume(tag != 6);
    kani::assume(tag != 8);
    kani::assume(tag != 9);
    kani::assume(tag != 13);
    kani::assume(tag != 19);
    kani::assume(tag != 30);
    kani::assume(tag != 32);
    assert!(Instruction::decode(&[tag]).is_err());

    let deposit_tag_only = [3u8];
    assert!(Instruction::decode(&deposit_tag_only).is_err());
}

#[kani::proof]
fn kani_v13_zero_length_decode_rejects() {
    let data: [u8; 0] = [];
    assert!(Instruction::decode(&data).is_err());
}

#[kani::proof]
fn kani_v13_every_active_payload_rejects_one_byte_truncation() {
    let init_market = [0u8; 80];
    assert!(Instruction::decode(&init_market).is_err());

    let deposit = [3u8; 16];
    assert!(Instruction::decode(&deposit).is_err());

    let withdraw = [4u8; 16];
    assert!(Instruction::decode(&withdraw).is_err());

    let crank = [5u8; 59];
    assert!(Instruction::decode(&crank).is_err());

    let trade = [6u8; 33];
    assert!(Instruction::decode(&trade).is_err());

    let top_up = [9u8; 16];
    assert!(Instruction::decode(&top_up).is_err());

    let close_resolved = [30u8; 16];
    assert!(Instruction::decode(&close_resolved).is_err());

    let update_authority = [32u8; 33];
    assert!(Instruction::decode(&update_authority).is_err());
}
