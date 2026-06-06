#![cfg(kani)]

extern crate kani;

use percolator_prog::ix::Instruction;
use percolator_prog::matcher_abi::{
    validate_matcher_return, MatcherReturn, FLAG_PARTIAL_OK, FLAG_REJECTED, FLAG_VALID,
};
use percolator_prog::policy_v16;

#[kani::proof]
fn kani_v16_premium_funding_rate_is_clamped_and_signed() {
    let mark_raw: u16 = kani::any();
    let index_raw: u16 = kani::any();
    let cap_raw: u16 = kani::any();
    let mark = mark_raw as u64 + 1;
    let index = index_raw as u64 + 1;
    let cap = cap_raw as u64;

    let rate = policy_v16::premium_funding_rate_e9(mark, index, cap).unwrap();
    let abs_rate = if rate < 0 {
        (-rate) as u128
    } else {
        rate as u128
    };
    assert!(abs_rate <= cap as u128);

    if cap == 0 || mark == index {
        assert_eq!(rate, 0);
    } else if mark > index {
        assert!(rate > 0);
    } else {
        assert!(rate < 0);
    }
}

#[kani::proof]
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

    let mut data = [0u8; 219];
    data[0] = 0;
    data[1..3].copy_from_slice(&max_portfolio_assets.to_le_bytes());
    data[3..11].copy_from_slice(&h_min.to_le_bytes());
    data[11..19].copy_from_slice(&h_max.to_le_bytes());
    data[19..27].copy_from_slice(&initial_price.to_le_bytes());
    data[27..43].copy_from_slice(&min_nonzero_mm_req.to_le_bytes());
    data[43..59].copy_from_slice(&min_nonzero_im_req.to_le_bytes());
    data[59..67].copy_from_slice(&maintenance_margin_bps.to_le_bytes());
    data[67..75].copy_from_slice(&initial_margin_bps.to_le_bytes());
    data[75..83].copy_from_slice(&max_trading_fee_bps.to_le_bytes());
    data[83..91].copy_from_slice(&trade_fee_base_bps.to_le_bytes());
    data[91..99].copy_from_slice(&liquidation_fee_bps.to_le_bytes());
    data[99..115].copy_from_slice(&liquidation_fee_cap.to_le_bytes());
    data[115..131].copy_from_slice(&min_liquidation_abs.to_le_bytes());
    data[131..139].copy_from_slice(&max_price_move_bps_per_slot.to_le_bytes());
    data[139..147].copy_from_slice(&max_accrual_dt_slots.to_le_bytes());
    data[147..155].copy_from_slice(&max_abs_funding_e9_per_slot.to_le_bytes());
    data[155..163].copy_from_slice(&min_funding_lifetime_slots.to_le_bytes());
    data[163..171].copy_from_slice(&max_account_b_settlement_chunks.to_le_bytes());
    data[171..179].copy_from_slice(&max_bankrupt_close_chunks.to_le_bytes());
    data[179..187].copy_from_slice(&max_bankrupt_close_lifetime_slots.to_le_bytes());
    data[187..203].copy_from_slice(&public_b_chunk_atoms.to_le_bytes());
    data[203..219].copy_from_slice(&maintenance_fee_per_slot.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
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
}

#[kani::proof]
fn kani_v16_amount_instructions_decode_preserves_wire_fields() {
    let tag: u8 = kani::any();
    kani::assume(
        tag == 3
            || tag == 4
            || tag == 9
            || tag == 28
            || tag == 30
            || tag == 41
            || tag == 42
            || tag == 47,
    );
    let amount: u128 = kani::any();

    let mut data = [0u8; 17];
    data[0] = tag;
    data[1..17].copy_from_slice(&amount.to_le_bytes());

    match (tag, Instruction::decode(&data).unwrap()) {
        (3, Instruction::Deposit { amount: got }) => assert_eq!(got, amount),
        (4, Instruction::Withdraw { amount: got }) => assert_eq!(got, amount),
        (9, Instruction::TopUpInsurance { amount: got }) => assert_eq!(got, amount),
        (28, Instruction::ConvertReleasedPnl { amount: got }) => assert_eq!(got, amount),
        (30, Instruction::CloseResolved { fee_rate_per_slot }) => {
            assert_eq!(fee_rate_per_slot, amount)
        }
        (41, Instruction::WithdrawInsurance { amount: got }) => assert_eq!(got, amount),
        (
            42,
            Instruction::CureAndCancelClose {
                optional_deposit: got,
            },
        ) => assert_eq!(got, amount),
        (47, Instruction::RefineResolvedUnreceiptedBound { decrease_num }) => {
            assert_eq!(decrease_num, amount)
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_domain_topup_and_asset_insurance_decode_preserves_wire_fields() {
    let domain: u8 = kani::any();
    let asset_index: u16 = kani::any();
    let amount: u128 = kani::any();

    let mut top_up = [0u8; 18];
    top_up[0] = 56;
    top_up[1] = domain;
    top_up[2..18].copy_from_slice(&amount.to_le_bytes());
    match Instruction::decode(&top_up).unwrap() {
        Instruction::TopUpInsuranceDomain {
            domain: got_domain,
            amount: got_amount,
        } => {
            assert_eq!(got_domain, domain);
            assert_eq!(got_amount, amount);
        }
        _ => unreachable!(),
    }

    let mut withdraw = [0u8; 19];
    withdraw[0] = 57;
    withdraw[1..3].copy_from_slice(&asset_index.to_le_bytes());
    withdraw[3..19].copy_from_slice(&amount.to_le_bytes());
    match Instruction::decode(&withdraw).unwrap() {
        Instruction::WithdrawInsuranceAsset {
            asset_index: got_asset,
            amount: got_amount,
        } => {
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_amount, amount);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_recovery_close_progress_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
    let side: u8 = kani::any();
    let b_delta_budget: u128 = kani::any();
    let reduce_q: u128 = kani::any();
    let close_q: u128 = kani::any();
    let now_slot: u64 = kani::any();

    let forfeit = Instruction::ForfeitRecoveryLeg {
        asset_index,
        b_delta_budget,
    }
    .encode();
    match Instruction::decode(&forfeit).unwrap() {
        Instruction::ForfeitRecoveryLeg {
            asset_index: got_asset,
            b_delta_budget: got_budget,
        } => {
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_budget, b_delta_budget);
        }
        _ => unreachable!(),
    }

    let rebalance = Instruction::RebalanceReduce {
        asset_index,
        reduce_q,
    }
    .encode();
    match Instruction::decode(&rebalance).unwrap() {
        Instruction::RebalanceReduce {
            asset_index: got_asset,
            reduce_q: got_reduce,
        } => {
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
fn kani_v16_top_up_backing_bucket_decode_preserves_wire_fields() {
    let domain: u8 = kani::any();
    let amount: u128 = kani::any();
    let expiry_slot: u64 = kani::any();

    let mut data = [0u8; 26];
    data[0] = 24;
    data[1] = domain;
    data[2..18].copy_from_slice(&amount.to_le_bytes());
    data[18..26].copy_from_slice(&expiry_slot.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::TopUpBackingBucket {
            domain: got_domain,
            amount: got_amount,
            expiry_slot: got_expiry,
        } => {
            assert_eq!(got_domain, domain);
            assert_eq!(got_amount, amount);
            assert_eq!(got_expiry, expiry_slot);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_withdraw_backing_bucket_decode_preserves_wire_fields() {
    let domain: u8 = kani::any();
    let amount: u128 = kani::any();

    let data = Instruction::WithdrawBackingBucket { domain, amount }.encode();

    match Instruction::decode(&data).unwrap() {
        Instruction::WithdrawBackingBucket {
            domain: got_domain,
            amount: got_amount,
        } => {
            assert_eq!(got_domain, domain);
            assert_eq!(got_amount, amount);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_asset_lifecycle_decode_preserves_wire_fields() {
    let action: u8 = kani::any();
    let asset_index: u16 = kani::any();
    let now_slot: u64 = kani::any();
    let initial_price: u64 = kani::any();
    let insurance_authority: [u8; 32] = kani::any();
    let insurance_operator: [u8; 32] = kani::any();
    let backing_bucket_authority: [u8; 32] = kani::any();
    let oracle_authority: [u8; 32] = kani::any();

    let data = Instruction::UpdateAssetLifecycle {
        action,
        asset_index,
        now_slot,
        initial_price,
        insurance_authority,
        insurance_operator,
        backing_bucket_authority,
        oracle_authority,
    }
    .encode();

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateAssetLifecycle {
            action: got_action,
            asset_index: got_asset_index,
            now_slot: got_now_slot,
            initial_price: got_initial_price,
            insurance_authority: got_insurance_authority,
            insurance_operator: got_insurance_operator,
            backing_bucket_authority: got_backing_bucket_authority,
            oracle_authority: got_oracle_authority,
        } => {
            assert_eq!(got_action, action);
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_now_slot, now_slot);
            assert_eq!(got_initial_price, initial_price);
            assert_eq!(got_insurance_authority, insurance_authority);
            assert_eq!(got_insurance_operator, insurance_operator);
            assert_eq!(got_backing_bucket_authority, backing_bucket_authority);
            assert_eq!(got_oracle_authority, oracle_authority);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_tradenocpi_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
    let size_q: i128 = kani::any();
    let exec_price: u64 = kani::any();
    let fee_bps: u64 = kani::any();

    let mut data = [0u8; 35];
    data[0] = 6;
    data[1..3].copy_from_slice(&asset_index.to_le_bytes());
    data[3..19].copy_from_slice(&size_q.to_le_bytes());
    data[19..27].copy_from_slice(&exec_price.to_le_bytes());
    data[27..35].copy_from_slice(&fee_bps.to_le_bytes());

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
fn kani_v16_tradecpi_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
    let size_q: i128 = kani::any();
    let fee_bps: u64 = kani::any();
    let limit_price: u64 = kani::any();

    let mut data = [0u8; 35];
    data[0] = 10;
    data[1..3].copy_from_slice(&asset_index.to_le_bytes());
    data[3..19].copy_from_slice(&size_q.to_le_bytes());
    data[19..27].copy_from_slice(&fee_bps.to_le_bytes());
    data[27..35].copy_from_slice(&limit_price.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::TradeCpi {
            asset_index: got_asset,
            size_q: got_size,
            fee_bps: got_fee,
            limit_price: got_limit,
        } => {
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_size, size_q);
            assert_eq!(got_fee, fee_bps);
            assert_eq!(got_limit, limit_price);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_matcher_return_accepts_only_bound_echoed_fills() {
    // Audit fix: the ret's echoed fields and abi_version are drawn INDEPENDENTLY of the
    // expected (bound) params, and sizes are full-width i128, so both the accept path AND
    // every rejection branch (abi mismatch, echoed-field mismatch, zero exec price, flag
    // checks, size guards) are symbolically exercised — not just the accept path.
    let abi_version: u32 = kani::any();
    let flags: u32 = kani::any();
    let exec_price_e6: u64 = kani::any();
    let exec_size: i128 = kani::any();
    let req_id_ret: u64 = kani::any();
    let lp_ret: u64 = kani::any();
    let oracle_ret: u64 = kani::any();
    let asset_ret: u64 = kani::any();
    // Bound (expected) params the validator echoes against — independent symbolics.
    let lp_account_id: u64 = kani::any();
    let asset_index: u16 = kani::any();
    let oracle_price_e6: u64 = kani::any();
    let req_size: i128 = kani::any();
    let req_id: u64 = kani::any();

    let ret = MatcherReturn {
        abi_version,
        flags,
        exec_price_e6,
        exec_size,
        req_id: req_id_ret,
        lp_account_id: lp_ret,
        oracle_price_e6: oracle_ret,
        asset_index: asset_ret,
    };

    let result = validate_matcher_return(
        &ret,
        lp_account_id,
        asset_index,
        oracle_price_e6,
        req_size,
        req_id,
    );

    // Rejection direction (the binding security property): a return with the wrong ABI,
    // a non-VALID/REJECTED flag state, any echoed field not bound to the expected param,
    // or a zero exec price MUST be rejected.
    if abi_version != percolator_prog::constants::MATCHER_ABI_VERSION
        || (flags & FLAG_VALID) == 0
        || (flags & FLAG_REJECTED) != 0
        || lp_ret != lp_account_id
        || oracle_ret != oracle_price_e6
        || asset_ret != asset_index as u64
        || req_id_ret != req_id
        || exec_price_e6 == 0
    {
        assert!(result.is_err());
    }

    // Accept direction: an accepted fill is bound to every expected field and within the
    // requested size, with the partial flag set whenever the fill is short.
    if result.is_ok() {
        assert!((flags & FLAG_VALID) != 0);
        assert!((flags & FLAG_REJECTED) == 0);
        assert_eq!(lp_ret, lp_account_id);
        assert_eq!(oracle_ret, oracle_price_e6);
        assert_eq!(asset_ret, asset_index as u64);
        assert_eq!(req_id_ret, req_id);
        assert!(exec_price_e6 != 0);
        if exec_size == 0 {
            assert!((flags & FLAG_PARTIAL_OK) != 0);
            assert_eq!(exec_price_e6, oracle_price_e6);
        } else {
            assert_eq!(exec_size.signum(), req_size.signum());
            assert!(exec_size.unsigned_abs() <= req_size.unsigned_abs());
            if exec_size.unsigned_abs() < req_size.unsigned_abs() {
                assert!((flags & FLAG_PARTIAL_OK) != 0);
            }
        }
    }
    // Ensure the accept path is reachable (non-vacuity of the accept assertions).
    kani::cover!(result.is_ok());
}

#[kani::proof]
fn kani_v16_permissionless_crank_decode_preserves_wire_fields() {
    let action: u8 = kani::any();
    let asset_index: u16 = kani::any();
    let recovery_reason: u8 = kani::any();
    let now_slot: u64 = kani::any();
    let funding_rate_e9: i128 = kani::any();
    let close_q: u128 = kani::any();
    let fee_bps: u64 = kani::any();

    let mut data = [0u8; 53];
    data[0] = 5;
    data[1] = action;
    data[2..4].copy_from_slice(&asset_index.to_le_bytes());
    data[4..12].copy_from_slice(&now_slot.to_le_bytes());
    data[12..28].copy_from_slice(&funding_rate_e9.to_le_bytes());
    data[28..44].copy_from_slice(&close_q.to_le_bytes());
    data[44..52].copy_from_slice(&fee_bps.to_le_bytes());
    data[52] = recovery_reason;

    match Instruction::decode(&data).unwrap() {
        Instruction::PermissionlessCrank {
            action: got_action,
            asset_index: got_asset,
            now_slot: got_slot,
            funding_rate_e9: got_rate,
            close_q: got_close,
            fee_bps: got_fee,
            recovery_reason: got_recovery,
        } => {
            assert_eq!(got_action, action);
            assert_eq!(got_asset, asset_index);
            assert_eq!(got_slot, now_slot);
            assert_eq!(got_rate, funding_rate_e9);
            assert_eq!(got_close, close_q);
            assert_eq!(got_fee, fee_bps);
            assert_eq!(got_recovery, recovery_reason);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_authority_decode_preserves_wire_fields() {
    let mut new_pubkey = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        new_pubkey[i] = kani::any();
        i += 1;
    }

    let mut data = [0u8; 33];
    data[0] = 32;
    data[1..33].copy_from_slice(&new_pubkey);

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateAuthority {
            new_pubkey: got_pubkey,
        } => {
            assert_eq!(got_pubkey, new_pubkey);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_asset_authority_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
    let kind: u8 = kani::any();
    let mut new_pubkey = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        new_pubkey[i] = kani::any();
        i += 1;
    }

    let mut data = [0u8; 36];
    data[0] = 65;
    data[1..3].copy_from_slice(&asset_index.to_le_bytes());
    data[3] = kind;
    data[4..36].copy_from_slice(&new_pubkey);

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateAssetAuthority {
            asset_index: got_asset_index,
            kind: got_kind,
            new_pubkey: got_pubkey,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_kind, kind);
            assert_eq!(got_pubkey, new_pubkey);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_restart_asset0_oracle_decode_preserves_wire_fields() {
    let now_slot: u64 = kani::any();
    let initial_price: u64 = kani::any();

    let data = Instruction::RestartAsset0Oracle {
        now_slot,
        initial_price,
    }
    .encode();

    match Instruction::decode(&data).unwrap() {
        Instruction::RestartAsset0Oracle {
            now_slot: got_slot,
            initial_price: got_price,
        } => {
            assert_eq!(got_slot, now_slot);
            assert_eq!(got_price, initial_price);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_batch_trade_nocpi_decode_does_not_collide_with_restart_asset0_oracle() {
    let asset_index: u16 = kani::any();
    let size_q: i128 = kani::any();
    let exec_price: u64 = kani::any();
    let fee_bps: u64 = kani::any();

    let data = Instruction::BatchTradeNoCpi {
        legs: vec![percolator_prog::ix::BatchTradeLeg {
            asset_index,
            size_q,
            exec_price,
            fee_bps,
        }],
    }
    .encode();

    assert_eq!(data[0], 66);
    match Instruction::decode(&data).unwrap() {
        Instruction::BatchTradeNoCpi { legs } => {
            assert_eq!(legs.len(), 1);
            assert_eq!(legs[0].asset_index, asset_index);
            assert_eq!(legs[0].size_q, size_q);
            assert_eq!(legs[0].exec_price, exec_price);
            assert_eq!(legs[0].fee_bps, fee_bps);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_liquidation_fee_policy_decode_preserves_wire_fields() {
    let cranker_share_bps: u16 = kani::any();

    let mut data = [0u8; 3];
    data[0] = 37;
    data[1..3].copy_from_slice(&cranker_share_bps.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: got,
        } => assert_eq!(got, cranker_share_bps),
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_maintenance_fee_policy_decode_preserves_wire_fields() {
    let cranker_share_bps: u16 = kani::any();

    let mut data = [0u8; 3];
    data[0] = 49;
    data[1..3].copy_from_slice(&cranker_share_bps.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: got,
        } => assert_eq!(got, cranker_share_bps),
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_backing_fee_policy_decode_preserves_wire_fields() {
    let domain: u8 = kani::any();
    let fee_bps: u16 = kani::any();
    let insurance_share_bps: u16 = kani::any();

    let mut data = [0u8; 6];
    data[0] = 51;
    data[1] = domain;
    data[2..4].copy_from_slice(&fee_bps.to_le_bytes());
    data[4..6].copy_from_slice(&insurance_share_bps.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateBackingFeePolicy {
            domain: got_domain,
            fee_bps: got_fee_bps,
            insurance_share_bps: got_insurance_share_bps,
        } => {
            assert_eq!(got_domain, domain);
            assert_eq!(got_fee_bps, fee_bps);
            assert_eq!(got_insurance_share_bps, insurance_share_bps);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_trade_fee_policy_decode_preserves_wire_fields() {
    let trade_fee_base_bps: u64 = kani::any();

    let mut data = [0u8; 9];
    data[0] = 55;
    data[1..9].copy_from_slice(&trade_fee_base_bps.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: got,
        } => assert_eq!(got, trade_fee_base_bps),
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_fee_redirect_policy_decode_preserves_wire_fields() {
    let redirect_bps: u16 = kani::any();

    let mut data = [0u8; 3];
    data[0] = 58;
    data[1..3].copy_from_slice(&redirect_bps.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateFeeRedirectPolicy { redirect_bps: got } => assert_eq!(got, redirect_bps),
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_update_market_init_fee_policy_decode_preserves_wire_fields() {
    let min_init_fee: u128 = kani::any();

    let mut data = [0u8; 17];
    data[0] = 59;
    data[1..17].copy_from_slice(&min_init_fee.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateMarketInitFeePolicy { min_init_fee: got } => {
            assert_eq!(got, min_init_fee)
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_base_unit_payloads_decode_preserves_wire_fields() {
    let primary_mint: [u8; 32] = kani::any();
    let secondary_mint: [u8; 32] = kani::any();
    let amount: u128 = kani::any();

    let mut update = [0u8; 65];
    update[0] = 60;
    update[1..33].copy_from_slice(&primary_mint);
    update[33..65].copy_from_slice(&secondary_mint);
    match Instruction::decode(&update).unwrap() {
        Instruction::UpdateBaseUnitMints {
            primary_mint: got_primary,
            secondary_mint: got_secondary,
        } => {
            assert_eq!(got_primary, primary_mint);
            assert_eq!(got_secondary, secondary_mint);
        }
        _ => unreachable!(),
    }

    let mut swap = [0u8; 17];
    swap[0] = 61;
    swap[1..17].copy_from_slice(&amount.to_le_bytes());
    match Instruction::decode(&swap).unwrap() {
        Instruction::SwapSecondaryForPrimary { amount: got } => assert_eq!(got, amount),
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_permissionless_resolve_decode_preserves_wire_fields() {
    let stale_slots: u64 = kani::any();
    let force_close_delay_slots: u64 = kani::any();
    let now_slot: u64 = kani::any();

    let mut configure = [0u8; 17];
    configure[0] = 38;
    configure[1..9].copy_from_slice(&stale_slots.to_le_bytes());
    configure[9..17].copy_from_slice(&force_close_delay_slots.to_le_bytes());
    match Instruction::decode(&configure).unwrap() {
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: got_stale,
            force_close_delay_slots: got_delay,
        } => {
            assert_eq!(got_stale, stale_slots);
            assert_eq!(got_delay, force_close_delay_slots);
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
fn kani_v16_configure_hybrid_oracle_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();
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
    let mut feeds = [[0u8; 32]; 3];
    let mut i = 0;
    while i < 3 {
        let mut j = 0;
        while j < 32 {
            feeds[i][j] = kani::any();
            j += 1;
        }
        i += 1;
    }

    let mut data = [0u8; 156];
    data[0] = 34;
    data[1..3].copy_from_slice(&asset_index.to_le_bytes());
    data[3..11].copy_from_slice(&now_slot.to_le_bytes());
    data[11..19].copy_from_slice(&now_unix_ts.to_le_bytes());
    data[19] = oracle_leg_count;
    data[20] = oracle_leg_flags;
    data[21..29].copy_from_slice(&max_staleness_secs.to_le_bytes());
    data[29..37].copy_from_slice(&hybrid_soft_stale_slots.to_le_bytes());
    data[37..45].copy_from_slice(&mark_ewma_halflife_slots.to_le_bytes());
    data[45..53].copy_from_slice(&mark_min_fee.to_le_bytes());
    data[53] = invert;
    data[54..58].copy_from_slice(&unit_scale.to_le_bytes());
    data[58..60].copy_from_slice(&conf_filter_bps.to_le_bytes());
    data[60..92].copy_from_slice(&feeds[0]);
    data[92..124].copy_from_slice(&feeds[1]);
    data[124..156].copy_from_slice(&feeds[2]);

    match Instruction::decode(&data).unwrap() {
        Instruction::ConfigureHybridOracle {
            asset_index: got_asset_index,
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
        } => {
            assert_eq!(got_asset_index, asset_index);
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
            assert_eq!(got_feeds, feeds);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_ewma_mark_decode_preserves_wire_fields() {
    let asset_index: u16 = kani::any();

    let now_slot: u64 = kani::any();
    let initial_mark_e6: u64 = kani::any();
    let mark_ewma_halflife_slots: u64 = kani::any();
    let mark_min_fee: u64 = kani::any();
    let push_mark_e6: u64 = kani::any();

    let mut configure = [0u8; 35];
    configure[0] = 35;
    configure[1..3].copy_from_slice(&asset_index.to_le_bytes());
    configure[3..11].copy_from_slice(&now_slot.to_le_bytes());
    configure[11..19].copy_from_slice(&initial_mark_e6.to_le_bytes());
    configure[19..27].copy_from_slice(&mark_ewma_halflife_slots.to_le_bytes());
    configure[27..35].copy_from_slice(&mark_min_fee.to_le_bytes());
    match Instruction::decode(&configure).unwrap() {
        Instruction::ConfigureEwmaMark {
            asset_index: got_asset_index,
            now_slot: got_now,
            initial_mark_e6: got_mark,
            mark_ewma_halflife_slots: got_halflife,
            mark_min_fee: got_min_fee,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_now, now_slot);
            assert_eq!(got_mark, initial_mark_e6);
            assert_eq!(got_halflife, mark_ewma_halflife_slots);
            assert_eq!(got_min_fee, mark_min_fee);
        }
        _ => unreachable!(),
    }

    let mut push = [0u8; 19];
    push[0] = 36;
    push[1..3].copy_from_slice(&asset_index.to_le_bytes());
    push[3..11].copy_from_slice(&now_slot.to_le_bytes());
    push[11..19].copy_from_slice(&push_mark_e6.to_le_bytes());
    match Instruction::decode(&push).unwrap() {
        Instruction::PushEwmaMark {
            asset_index: got_asset_index,
            now_slot: got_now,
            mark_e6: got_mark,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_now, now_slot);
            assert_eq!(got_mark, push_mark_e6);
        }
        _ => unreachable!(),
    }

    let mut configure_auth = [0u8; 19];
    configure_auth[0] = 62;
    configure_auth[1..3].copy_from_slice(&asset_index.to_le_bytes());
    configure_auth[3..11].copy_from_slice(&now_slot.to_le_bytes());
    configure_auth[11..19].copy_from_slice(&initial_mark_e6.to_le_bytes());
    match Instruction::decode(&configure_auth).unwrap() {
        Instruction::ConfigureAuthMark {
            asset_index: got_asset_index,
            now_slot: got_now,
            initial_mark_e6: got_mark,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_now, now_slot);
            assert_eq!(got_mark, initial_mark_e6);
        }
        _ => unreachable!(),
    }

    let mut push_auth = [0u8; 19];
    push_auth[0] = 63;
    push_auth[1..3].copy_from_slice(&asset_index.to_le_bytes());
    push_auth[3..11].copy_from_slice(&now_slot.to_le_bytes());
    push_auth[11..19].copy_from_slice(&push_mark_e6.to_le_bytes());
    match Instruction::decode(&push_auth).unwrap() {
        Instruction::PushAuthMark {
            asset_index: got_asset_index,
            now_slot: got_now,
            mark_e6: got_mark,
        } => {
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_now, now_slot);
            assert_eq!(got_mark, push_mark_e6);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_decode_rejects_trailing_bytes() {
    let extra: u8 = kani::any();
    let data = [1u8, extra];
    assert!(Instruction::decode(&data).is_err());
}

fn assert_rejects_trailing_byte(ix: Instruction, extra: u8) {
    let mut data = ix.encode();
    data.push(extra);
    assert!(Instruction::decode(&data).is_err());
}

#[kani::proof]
fn kani_v16_init_market_payload_rejects_trailing_byte() {
    let extra: u8 = kani::any();
    assert_rejects_trailing_byte(
        Instruction::InitMarket {
            max_portfolio_assets: 1,
            h_min: 1,
            h_max: 2,
            initial_price: 100,
            min_nonzero_mm_req: 1,
            min_nonzero_im_req: 2,
            maintenance_margin_bps: 500,
            initial_margin_bps: 1_000,
            max_trading_fee_bps: 10_000,
            trade_fee_base_bps: 0,
            liquidation_fee_bps: 0,
            liquidation_fee_cap: 0,
            min_liquidation_abs: 0,
            max_price_move_bps_per_slot: 100,
            max_accrual_dt_slots: 10,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 10,
            max_account_b_settlement_chunks: 1,
            max_bankrupt_close_chunks: 1,
            max_bankrupt_close_lifetime_slots: 100,
            public_b_chunk_atoms: percolator::MAX_VAULT_TVL,
            maintenance_fee_per_slot: 0,
        },
        extra,
    );
}

#[kani::proof]
fn kani_v16_custody_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(Instruction::InitPortfolio, extra);
    assert_rejects_trailing_byte(Instruction::Deposit { amount: 1 }, extra);
    assert_rejects_trailing_byte(Instruction::Withdraw { amount: 1 }, extra);
    assert_rejects_trailing_byte(Instruction::TopUpInsurance { amount: 1 }, extra);
    assert_rejects_trailing_byte(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 1,
            expiry_slot: 10,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(Instruction::WithdrawInsurance { amount: 1 }, extra);
    assert_rejects_trailing_byte(
        Instruction::WithdrawInsuranceAsset {
            asset_index: 0,
            amount: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(Instruction::SwapSecondaryForPrimary { amount: 1 }, extra);
}

#[kani::proof]
fn kani_v16_trade_and_crank_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: 1,
            exec_price: 100,
            fee_bps: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::TradeCpi {
            asset_index: 0,
            size_q: 1,
            fee_bps: 0,
            limit_price: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(Instruction::SyncMaintenanceFee { now_slot: 1 }, extra);
}

#[kani::proof]
fn kani_v16_admin_policy_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(Instruction::CloseSlab, extra);
    assert_rejects_trailing_byte(Instruction::ResolveMarket, extra);
    assert_rejects_trailing_byte(
        Instruction::UpdateAuthority {
            new_pubkey: [1u8; 32],
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateAssetAuthority {
            asset_index: 1,
            kind: 0,
            new_pubkey: [1u8; 32],
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 4_000,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 4_000,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateBackingFeePolicy {
            domain: 0,
            fee_bps: 25,
            insurance_share_bps: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 25,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateFeeRedirectPolicy { redirect_bps: 250 },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateMarketInitFeePolicy { min_init_fee: 50 },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateBaseUnitMints {
            primary_mint: [1u8; 32],
            secondary_mint: [2u8; 32],
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 5,
            force_close_delay_slots: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::ResolveStalePermissionless { now_slot: 5 },
        extra,
    );
}

#[kani::proof]
fn kani_v16_oracle_asset_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
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
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::ConfigureEwmaMark {
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::PushEwmaMark {
            asset_index: 0,
            now_slot: 2,
            mark_e6: 101,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::ConfigureAuthMark {
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 100,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::PushAuthMark {
            asset_index: 0,
            now_slot: 2,
            mark_e6: 101,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::UpdateAssetLifecycle {
            action: 0,
            asset_index: 1,
            now_slot: 2,
            initial_price: 100,
            insurance_authority: [1u8; 32],
            insurance_operator: [1u8; 32],
            backing_bucket_authority: [1u8; 32],
            oracle_authority: [1u8; 32],
        },
        extra,
    );
}

#[kani::proof]
fn kani_v16_resolved_recovery_payloads_reject_trailing_byte() {
    let extra: u8 = kani::any();

    assert_rejects_trailing_byte(Instruction::ConvertReleasedPnl { amount: 1 }, extra);
    assert_rejects_trailing_byte(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::CureAndCancelClose {
            optional_deposit: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::ForfeitRecoveryLeg {
            asset_index: 0,
            b_delta_budget: 1,
        },
        extra,
    );
    assert_rejects_trailing_byte(
        Instruction::RebalanceReduce {
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
        Instruction::RefineResolvedUnreceiptedBound { decrease_num: 1 },
        extra,
    );
    assert_rejects_trailing_byte(Instruction::ClosePortfolio, extra);
}

#[kani::proof]
fn kani_v16_unknown_or_truncated_tags_reject() {
    let tag: u8 = kani::any();
    kani::assume(tag != 0);
    kani::assume(tag != 1);
    kani::assume(tag != 3);
    kani::assume(tag != 4);
    kani::assume(tag != 5);
    kani::assume(tag != 6);
    kani::assume(tag != 8);
    kani::assume(tag != 9);
    kani::assume(tag != 10);
    kani::assume(tag != 13);
    kani::assume(tag != 19);
    kani::assume(tag != 23);
    kani::assume(tag != 24);
    kani::assume(tag != 28);
    kani::assume(tag != 30);
    kani::assume(tag != 32);
    kani::assume(tag != 33);
    kani::assume(tag != 34);
    kani::assume(tag != 35);
    kani::assume(tag != 36);
    kani::assume(tag != 37);
    kani::assume(tag != 38);
    kani::assume(tag != 39);
    kani::assume(tag != 40);
    kani::assume(tag != 41);
    kani::assume(tag != 42);
    kani::assume(tag != 43);
    kani::assume(tag != 44);
    kani::assume(tag != 45);
    kani::assume(tag != 46);
    kani::assume(tag != 47);
    kani::assume(tag != 48);
    kani::assume(tag != 49);
    kani::assume(tag != 50);
    kani::assume(tag != 51);
    kani::assume(tag != 52);
    kani::assume(tag != 53);
    kani::assume(tag != 54);
    kani::assume(tag != 55);
    assert!(Instruction::decode(&[tag]).is_err());

    let deposit_tag_only = [3u8];
    assert!(Instruction::decode(&deposit_tag_only).is_err());
}

#[kani::proof]
fn kani_v16_zero_length_decode_rejects() {
    let data: [u8; 0] = [];
    assert!(Instruction::decode(&data).is_err());
}

#[kani::proof]
fn kani_v16_every_active_payload_rejects_one_byte_truncation() {
    let init_market = [0u8; 80];
    assert!(Instruction::decode(&init_market).is_err());

    let deposit = [3u8; 16];
    assert!(Instruction::decode(&deposit).is_err());

    let withdraw = [4u8; 16];
    assert!(Instruction::decode(&withdraw).is_err());

    let crank = [5u8; 59];
    assert!(Instruction::decode(&crank).is_err());

    let asset_lifecycle = [40u8; 147];
    assert!(Instruction::decode(&asset_lifecycle).is_err());

    let trade = [6u8; 33];
    assert!(Instruction::decode(&trade).is_err());

    let trade_cpi = [10u8; 33];
    assert!(Instruction::decode(&trade_cpi).is_err());

    let top_up = [9u8; 16];
    assert!(Instruction::decode(&top_up).is_err());

    let top_up_domain = [56u8; 17];
    assert!(Instruction::decode(&top_up_domain).is_err());

    let top_up_backing = [24u8; 25];
    assert!(Instruction::decode(&top_up_backing).is_err());

    let withdraw_insurance = [23u8; 16];
    assert!(Instruction::decode(&withdraw_insurance).is_err());

    let withdraw_insurance_domain = [57u8; 17];
    assert!(Instruction::decode(&withdraw_insurance_domain).is_err());

    let convert_pnl = [28u8; 16];
    assert!(Instruction::decode(&convert_pnl).is_err());

    let close_resolved = [30u8; 16];
    assert!(Instruction::decode(&close_resolved).is_err());

    let update_authority = [32u8; 32];
    assert!(Instruction::decode(&update_authority).is_err());

    let update_asset_authority = [65u8; 35];
    assert!(Instruction::decode(&update_asset_authority).is_err());

    let update_insurance = [33u8; 11];
    assert!(Instruction::decode(&update_insurance).is_err());

    let configure_hybrid = [34u8; 155];
    assert!(Instruction::decode(&configure_hybrid).is_err());

    let configure_ewma_mark = [35u8; 34];
    assert!(Instruction::decode(&configure_ewma_mark).is_err());

    let push_ewma_mark = [36u8; 18];
    assert!(Instruction::decode(&push_ewma_mark).is_err());

    let configure_auth_mark = [62u8; 18];
    assert!(Instruction::decode(&configure_auth_mark).is_err());

    let push_auth_mark = [63u8; 18];
    assert!(Instruction::decode(&push_auth_mark).is_err());

    let update_liquidation = [37u8; 2];
    assert!(Instruction::decode(&update_liquidation).is_err());

    let update_redirect = [58u8; 2];
    assert!(Instruction::decode(&update_redirect).is_err());

    let update_base_units = [60u8; 64];
    assert!(Instruction::decode(&update_base_units).is_err());

    let swap_base_units = [61u8; 16];
    assert!(Instruction::decode(&swap_base_units).is_err());

    let configure_permissionless = [38u8; 16];
    assert!(Instruction::decode(&configure_permissionless).is_err());

    let resolve_permissionless = [39u8; 8];
    assert!(Instruction::decode(&resolve_permissionless).is_err());

    let withdraw_insurance_full = [41u8; 16];
    assert!(Instruction::decode(&withdraw_insurance_full).is_err());

    let cure = [42u8; 16];
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
