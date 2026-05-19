#![cfg(kani)]

extern crate kani;

use percolator_prog::ix::Instruction;
use percolator_prog::matcher_abi::{
    validate_matcher_return, MatcherReturn, FLAG_PARTIAL_OK, FLAG_REJECTED, FLAG_VALID,
};

#[kani::proof]
fn kani_v16_init_market_decode_preserves_wire_fields() {
    let max_portfolio_assets: u8 = kani::any();
    let h_min_raw: u16 = kani::any();
    let h_max_raw: u16 = kani::any();
    let initial_price_raw: u16 = kani::any();
    let min_nonzero_mm_raw: u16 = kani::any();
    let min_nonzero_im_raw: u16 = kani::any();
    let maintenance_margin_bps_raw: u16 = kani::any();
    let initial_margin_bps_raw: u16 = kani::any();
    let max_trading_fee_bps_raw: u16 = kani::any();
    let trade_fee_base_bps_raw: u16 = kani::any();
    let liquidation_fee_bps_raw: u16 = kani::any();
    let liquidation_fee_cap_raw: u16 = kani::any();
    let min_liquidation_abs_raw: u16 = kani::any();
    let max_price_move_bps_raw: u16 = kani::any();
    let max_accrual_dt_raw: u16 = kani::any();
    let max_abs_funding_raw: u16 = kani::any();
    let min_funding_lifetime_raw: u16 = kani::any();
    let max_account_b_chunks_raw: u16 = kani::any();
    let max_bankrupt_close_chunks_raw: u16 = kani::any();
    let public_b_chunk_atoms_raw: u16 = kani::any();
    let maintenance_fee_raw: u16 = kani::any();
    let h_min = h_min_raw as u64;
    let h_max = h_max_raw as u64;
    let initial_price = initial_price_raw as u64;
    let min_nonzero_mm_req = min_nonzero_mm_raw as u128;
    let min_nonzero_im_req = min_nonzero_im_raw as u128;
    let maintenance_margin_bps = maintenance_margin_bps_raw as u64;
    let initial_margin_bps = initial_margin_bps_raw as u64;
    let max_trading_fee_bps = max_trading_fee_bps_raw as u64;
    let trade_fee_base_bps = trade_fee_base_bps_raw as u64;
    let liquidation_fee_bps = liquidation_fee_bps_raw as u64;
    let liquidation_fee_cap = liquidation_fee_cap_raw as u128;
    let min_liquidation_abs = min_liquidation_abs_raw as u128;
    let max_price_move_bps_per_slot = max_price_move_bps_raw as u64;
    let max_accrual_dt_slots = max_accrual_dt_raw as u64;
    let max_abs_funding_e9_per_slot = max_abs_funding_raw as u64;
    let min_funding_lifetime_slots = min_funding_lifetime_raw as u64;
    let max_account_b_settlement_chunks = max_account_b_chunks_raw as u64;
    let max_bankrupt_close_chunks = max_bankrupt_close_chunks_raw as u64;
    let public_b_chunk_atoms = public_b_chunk_atoms_raw as u128;
    let maintenance_fee_per_slot = maintenance_fee_raw as u128;

    let mut data = [0u8; 210];
    data[0] = 0;
    data[1] = max_portfolio_assets;
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
    data[178..194].copy_from_slice(&public_b_chunk_atoms.to_le_bytes());
    data[194..210].copy_from_slice(&maintenance_fee_per_slot.to_le_bytes());

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
            || tag == 23
            || tag == 28
            || tag == 30
            || tag == 41
            || tag == 42
            || tag == 47,
    );
    let amount_raw: u16 = kani::any();
    let amount = amount_raw as u128;

    let mut data = [0u8; 17];
    data[0] = tag;
    data[1..17].copy_from_slice(&amount.to_le_bytes());

    match (tag, Instruction::decode(&data).unwrap()) {
        (3, Instruction::Deposit { amount: got }) => assert_eq!(got, amount),
        (4, Instruction::Withdraw { amount: got }) => assert_eq!(got, amount),
        (9, Instruction::TopUpInsurance { amount: got }) => assert_eq!(got, amount),
        (23, Instruction::WithdrawInsuranceLimited { amount: got }) => {
            assert_eq!(got, amount)
        }
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
fn kani_v16_recovery_close_progress_decode_preserves_wire_fields() {
    let asset_index: u8 = kani::any();
    let side: u8 = kani::any();
    let budget_raw: u16 = kani::any();
    let reduce_raw: u16 = kani::any();
    let now_slot_raw: u16 = kani::any();
    let b_delta_budget = budget_raw as u128;
    let reduce_q = reduce_raw as u128;
    let now_slot = now_slot_raw as u64;

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
    let amount_raw: u16 = kani::any();
    let expiry_raw: u16 = kani::any();
    let amount = amount_raw as u128;
    let expiry_slot = expiry_raw as u64;

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
fn kani_v16_asset_lifecycle_decode_preserves_wire_fields() {
    let action: u8 = kani::any();
    let asset_index: u8 = kani::any();
    let now_slot_raw: u16 = kani::any();
    let initial_price_raw: u16 = kani::any();
    let now_slot = now_slot_raw as u64;
    let initial_price = initial_price_raw as u64;

    let data = Instruction::UpdateAssetLifecycle {
        action,
        asset_index,
        now_slot,
        initial_price,
    }
    .encode();

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateAssetLifecycle {
            action: got_action,
            asset_index: got_asset_index,
            now_slot: got_now_slot,
            initial_price: got_initial_price,
        } => {
            assert_eq!(got_action, action);
            assert_eq!(got_asset_index, asset_index);
            assert_eq!(got_now_slot, now_slot);
            assert_eq!(got_initial_price, initial_price);
        }
        _ => unreachable!(),
    }
}

#[kani::proof]
fn kani_v16_tradenocpi_decode_preserves_wire_fields() {
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
fn kani_v16_tradecpi_decode_preserves_wire_fields() {
    let asset_index: u8 = kani::any();
    let size_raw: i16 = kani::any();
    let fee_bps_raw: u16 = kani::any();
    let limit_price_raw: u16 = kani::any();
    let size_q = size_raw as i128;
    let fee_bps = fee_bps_raw as u64;
    let limit_price = limit_price_raw as u64;

    let mut data = [0u8; 34];
    data[0] = 10;
    data[1] = asset_index;
    data[2..18].copy_from_slice(&size_q.to_le_bytes());
    data[18..26].copy_from_slice(&fee_bps.to_le_bytes());
    data[26..34].copy_from_slice(&limit_price.to_le_bytes());

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
    let req_raw: i16 = kani::any();
    let exec_raw: i16 = kani::any();
    let price_raw: u16 = kani::any();
    let req_id_raw: u16 = kani::any();
    let lp_raw: u16 = kani::any();
    let asset_index: u8 = kani::any();
    let flags: u32 = kani::any();
    kani::assume(req_raw != 0);
    kani::assume(req_raw != i16::MIN);
    kani::assume(exec_raw != i16::MIN);
    let req_size = req_raw as i128;
    let exec_size = exec_raw as i128;
    let oracle_price = (price_raw as u64).saturating_add(1);
    let req_id = req_id_raw as u64;
    let lp_account_id = lp_raw as u64;
    let ret = MatcherReturn {
        abi_version: percolator_prog::constants::MATCHER_ABI_VERSION,
        flags,
        exec_price_e6: oracle_price,
        exec_size,
        req_id,
        lp_account_id,
        oracle_price_e6: oracle_price,
        asset_index: asset_index as u64,
    };

    if validate_matcher_return(
        &ret,
        lp_account_id,
        asset_index,
        oracle_price,
        req_size,
        req_id,
    )
    .is_ok()
    {
        assert!((flags & FLAG_VALID) != 0);
        assert!((flags & FLAG_REJECTED) == 0);
        if exec_size == 0 {
            assert!((flags & FLAG_PARTIAL_OK) != 0);
        } else {
            assert_eq!(exec_size.signum(), req_size.signum());
            assert!(exec_size.unsigned_abs() <= req_size.unsigned_abs());
            if exec_size.unsigned_abs() < req_size.unsigned_abs() {
                assert!((flags & FLAG_PARTIAL_OK) != 0);
            }
        }
    }
}

#[kani::proof]
fn kani_v16_permissionless_crank_decode_preserves_wire_fields() {
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
fn kani_v16_update_authority_decode_preserves_wire_fields() {
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
fn kani_v16_update_insurance_policy_decode_preserves_wire_fields() {
    let max_bps: u16 = kani::any();
    let deposits_only: u8 = kani::any();
    let cooldown_raw: u16 = kani::any();
    let cooldown_slots = cooldown_raw as u64;

    let mut data = [0u8; 12];
    data[0] = 33;
    data[1..3].copy_from_slice(&max_bps.to_le_bytes());
    data[3] = deposits_only;
    data[4..12].copy_from_slice(&cooldown_slots.to_le_bytes());

    match Instruction::decode(&data).unwrap() {
        Instruction::UpdateInsurancePolicy {
            max_bps: got_max_bps,
            deposits_only: got_deposits_only,
            cooldown_slots: got_cooldown,
        } => {
            assert_eq!(got_max_bps, max_bps);
            assert_eq!(got_deposits_only, deposits_only);
            assert_eq!(got_cooldown, cooldown_slots);
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
fn kani_v16_permissionless_resolve_decode_preserves_wire_fields() {
    let stale_slots_raw: u16 = kani::any();
    let delay_raw: u16 = kani::any();
    let now_slot_raw: u16 = kani::any();
    let stale_slots = stale_slots_raw as u64;
    let force_close_delay_slots = delay_raw as u64;
    let now_slot = now_slot_raw as u64;

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
    let now_slot_raw: u16 = kani::any();
    let now_unix_raw: i16 = kani::any();
    let oracle_leg_count: u8 = kani::any();
    let oracle_leg_flags: u8 = kani::any();
    let max_staleness_raw: u16 = kani::any();
    let soft_stale_raw: u16 = kani::any();
    let halflife_raw: u16 = kani::any();
    let mark_min_fee_raw: u16 = kani::any();
    let invert: u8 = kani::any();
    let unit_scale_raw: u16 = kani::any();
    let conf_filter_bps: u16 = kani::any();
    let now_slot = now_slot_raw as u64;
    let now_unix_ts = now_unix_raw as i64;
    let max_staleness_secs = max_staleness_raw as u64;
    let hybrid_soft_stale_slots = soft_stale_raw as u64;
    let mark_ewma_halflife_slots = halflife_raw as u64;
    let mark_min_fee = mark_min_fee_raw as u64;
    let unit_scale = unit_scale_raw as u32;
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

    let mut data = [0u8; 154];
    data[0] = 34;
    data[1..9].copy_from_slice(&now_slot.to_le_bytes());
    data[9..17].copy_from_slice(&now_unix_ts.to_le_bytes());
    data[17] = oracle_leg_count;
    data[18] = oracle_leg_flags;
    data[19..27].copy_from_slice(&max_staleness_secs.to_le_bytes());
    data[27..35].copy_from_slice(&hybrid_soft_stale_slots.to_le_bytes());
    data[35..43].copy_from_slice(&mark_ewma_halflife_slots.to_le_bytes());
    data[43..51].copy_from_slice(&mark_min_fee.to_le_bytes());
    data[51] = invert;
    data[52..56].copy_from_slice(&unit_scale.to_le_bytes());
    data[56..58].copy_from_slice(&conf_filter_bps.to_le_bytes());
    data[58..90].copy_from_slice(&feeds[0]);
    data[90..122].copy_from_slice(&feeds[1]);
    data[122..154].copy_from_slice(&feeds[2]);

    match Instruction::decode(&data).unwrap() {
        Instruction::ConfigureHybridOracle {
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
fn kani_v16_hyperp_mark_decode_preserves_wire_fields() {
    let now_slot_raw: u16 = kani::any();
    let mark_raw: u16 = kani::any();
    let halflife_raw: u16 = kani::any();
    let mark_min_fee_raw: u16 = kani::any();
    let push_mark_raw: u16 = kani::any();

    let now_slot = now_slot_raw as u64;
    let initial_mark_e6 = mark_raw as u64;
    let mark_ewma_halflife_slots = halflife_raw as u64;
    let mark_min_fee = mark_min_fee_raw as u64;
    let push_mark_e6 = push_mark_raw as u64;

    let mut configure = [0u8; 33];
    configure[0] = 35;
    configure[1..9].copy_from_slice(&now_slot.to_le_bytes());
    configure[9..17].copy_from_slice(&initial_mark_e6.to_le_bytes());
    configure[17..25].copy_from_slice(&mark_ewma_halflife_slots.to_le_bytes());
    configure[25..33].copy_from_slice(&mark_min_fee.to_le_bytes());
    match Instruction::decode(&configure).unwrap() {
        Instruction::ConfigureHyperpMark {
            now_slot: got_now,
            initial_mark_e6: got_mark,
            mark_ewma_halflife_slots: got_halflife,
            mark_min_fee: got_min_fee,
        } => {
            assert_eq!(got_now, now_slot);
            assert_eq!(got_mark, initial_mark_e6);
            assert_eq!(got_halflife, mark_ewma_halflife_slots);
            assert_eq!(got_min_fee, mark_min_fee);
        }
        _ => unreachable!(),
    }

    let mut push = [0u8; 17];
    push[0] = 36;
    push[1..9].copy_from_slice(&now_slot.to_le_bytes());
    push[9..17].copy_from_slice(&push_mark_e6.to_le_bytes());
    match Instruction::decode(&push).unwrap() {
        Instruction::PushHyperpMark {
            now_slot: got_now,
            mark_e6: got_mark,
        } => {
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

#[kani::proof]
fn kani_v16_every_active_payload_rejects_trailing_byte() {
    let extra: u8 = kani::any();

    let mut init_market = Instruction::InitMarket {
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
        public_b_chunk_atoms: percolator::MAX_VAULT_TVL,
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

    let mut trade_cpi = Instruction::TradeCpi {
        asset_index: 0,
        size_q: 1,
        fee_bps: 0,
        limit_price: 0,
    }
    .encode();
    trade_cpi.push(extra);
    assert!(Instruction::decode(&trade_cpi).is_err());

    let mut close_portfolio = Instruction::ClosePortfolio.encode();
    close_portfolio.push(extra);
    assert!(Instruction::decode(&close_portfolio).is_err());

    let mut top_up = Instruction::TopUpInsurance { amount: 1 }.encode();
    top_up.push(extra);
    assert!(Instruction::decode(&top_up).is_err());

    let mut top_up_backing = Instruction::TopUpBackingBucket {
        domain: 1,
        amount: 1,
        expiry_slot: 10,
    }
    .encode();
    top_up_backing.push(extra);
    assert!(Instruction::decode(&top_up_backing).is_err());

    let mut close_slab = Instruction::CloseSlab.encode();
    close_slab.push(extra);
    assert!(Instruction::decode(&close_slab).is_err());

    let mut resolve = Instruction::ResolveMarket.encode();
    resolve.push(extra);
    assert!(Instruction::decode(&resolve).is_err());

    let mut withdraw_insurance = Instruction::WithdrawInsuranceLimited { amount: 1 }.encode();
    withdraw_insurance.push(extra);
    assert!(Instruction::decode(&withdraw_insurance).is_err());

    let mut convert_pnl = Instruction::ConvertReleasedPnl { amount: 1 }.encode();
    convert_pnl.push(extra);
    assert!(Instruction::decode(&convert_pnl).is_err());

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

    let mut update_insurance = Instruction::UpdateInsurancePolicy {
        max_bps: 10_000,
        deposits_only: 1,
        cooldown_slots: 1,
    }
    .encode();
    update_insurance.push(extra);
    assert!(Instruction::decode(&update_insurance).is_err());

    let mut update_liquidation = Instruction::UpdateLiquidationFeePolicy {
        cranker_share_bps: 4_000,
    }
    .encode();
    update_liquidation.push(extra);
    assert!(Instruction::decode(&update_liquidation).is_err());

    let mut configure_permissionless = Instruction::ConfigurePermissionlessResolve {
        stale_slots: 5,
        force_close_delay_slots: 1,
    }
    .encode();
    configure_permissionless.push(extra);
    assert!(Instruction::decode(&configure_permissionless).is_err());

    let mut resolve_permissionless =
        Instruction::ResolveStalePermissionless { now_slot: 5 }.encode();
    resolve_permissionless.push(extra);
    assert!(Instruction::decode(&resolve_permissionless).is_err());

    let mut configure_hybrid = Instruction::ConfigureHybridOracle {
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
    }
    .encode();
    configure_hybrid.push(extra);
    assert!(Instruction::decode(&configure_hybrid).is_err());

    let mut configure_hyperp = Instruction::ConfigureHyperpMark {
        now_slot: 1,
        initial_mark_e6: 100,
        mark_ewma_halflife_slots: 1,
        mark_min_fee: 0,
    }
    .encode();
    configure_hyperp.push(extra);
    assert!(Instruction::decode(&configure_hyperp).is_err());

    let mut push_hyperp = Instruction::PushHyperpMark {
        now_slot: 2,
        mark_e6: 101,
    }
    .encode();
    push_hyperp.push(extra);
    assert!(Instruction::decode(&push_hyperp).is_err());

    let mut asset_lifecycle = Instruction::UpdateAssetLifecycle {
        action: 0,
        asset_index: 1,
        now_slot: 2,
        initial_price: 100,
    }
    .encode();
    asset_lifecycle.push(extra);
    assert!(Instruction::decode(&asset_lifecycle).is_err());

    let mut withdraw_insurance_full = Instruction::WithdrawInsurance { amount: 1 }.encode();
    withdraw_insurance_full.push(extra);
    assert!(Instruction::decode(&withdraw_insurance_full).is_err());

    let mut cure = Instruction::CureAndCancelClose {
        optional_deposit: 1,
    }
    .encode();
    cure.push(extra);
    assert!(Instruction::decode(&cure).is_err());

    let mut forfeit = Instruction::ForfeitRecoveryLeg {
        asset_index: 0,
        b_delta_budget: 1,
    }
    .encode();
    forfeit.push(extra);
    assert!(Instruction::decode(&forfeit).is_err());

    let mut rebalance = Instruction::RebalanceReduce {
        asset_index: 0,
        reduce_q: 1,
    }
    .encode();
    rebalance.push(extra);
    assert!(Instruction::decode(&rebalance).is_err());

    let mut finalize = Instruction::FinalizeResetSide {
        asset_index: 0,
        side: 0,
    }
    .encode();
    finalize.push(extra);
    assert!(Instruction::decode(&finalize).is_err());

    let mut topup = Instruction::ClaimResolvedPayoutTopup.encode();
    topup.push(extra);
    assert!(Instruction::decode(&topup).is_err());

    let mut refine = Instruction::RefineResolvedUnreceiptedBound { decrease_num: 1 }.encode();
    refine.push(extra);
    assert!(Instruction::decode(&refine).is_err());

    let mut sync_fee = Instruction::SyncMaintenanceFee { now_slot: 1 }.encode();
    sync_fee.push(extra);
    assert!(Instruction::decode(&sync_fee).is_err());
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

    let asset_lifecycle = [40u8; 18];
    assert!(Instruction::decode(&asset_lifecycle).is_err());

    let trade = [6u8; 33];
    assert!(Instruction::decode(&trade).is_err());

    let trade_cpi = [10u8; 33];
    assert!(Instruction::decode(&trade_cpi).is_err());

    let top_up = [9u8; 16];
    assert!(Instruction::decode(&top_up).is_err());

    let top_up_backing = [24u8; 25];
    assert!(Instruction::decode(&top_up_backing).is_err());

    let withdraw_insurance = [23u8; 16];
    assert!(Instruction::decode(&withdraw_insurance).is_err());

    let convert_pnl = [28u8; 16];
    assert!(Instruction::decode(&convert_pnl).is_err());

    let close_resolved = [30u8; 16];
    assert!(Instruction::decode(&close_resolved).is_err());

    let update_authority = [32u8; 33];
    assert!(Instruction::decode(&update_authority).is_err());

    let update_insurance = [33u8; 11];
    assert!(Instruction::decode(&update_insurance).is_err());

    let configure_hybrid = [34u8; 153];
    assert!(Instruction::decode(&configure_hybrid).is_err());

    let configure_hyperp = [35u8; 24];
    assert!(Instruction::decode(&configure_hyperp).is_err());

    let push_hyperp = [36u8; 16];
    assert!(Instruction::decode(&push_hyperp).is_err());

    let update_liquidation = [37u8; 2];
    assert!(Instruction::decode(&update_liquidation).is_err());

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
}
