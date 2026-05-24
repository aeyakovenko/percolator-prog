// Dump field offsets + sizes for the v16 state types so the TS parser
// can be generated against an exact source of truth.

use core::mem::{align_of, offset_of, size_of};
use percolator::{
    AssetStateV16Account, BackingBucketV16Account, CloseProgressLedgerV16Account,
    EngineAssetSlotV16Account, HealthCertV16Account, InsuranceCreditReservationV16Account,
    MarketGroupV16HeaderAccount, PortfolioAccountV16Account, PortfolioLegV16Account,
    PortfolioSourceDomainV16Account, ProvenanceHeaderV16Account, ResolvedPayoutLedgerV16Account,
    ResolvedPayoutReceiptV16Account, SourceCreditStateV16Account, V16ConfigAccount,
};
use percolator_prog::constants::{
    ASSET_ORACLE_PROFILE_LEN, HEADER_LEN, MARKET_ASSET_SLOT_LEN, MARKET_GROUP_OFF,
    PORTFOLIO_SOURCE_DOMAIN_LEN, WRAPPER_CONFIG_LEN,
};
use percolator_prog::state::WrapperConfigV16;

fn main() {
    println!(
        "=== HEADER_LEN={} WRAPPER_CONFIG_LEN={} MARKET_GROUP_OFF={}",
        HEADER_LEN, WRAPPER_CONFIG_LEN, MARKET_GROUP_OFF
    );

    println!(
        "\n=== WrapperConfigV16 (size={} align={}) ===",
        size_of::<WrapperConfigV16>(),
        align_of::<WrapperConfigV16>()
    );
    println!("  offset  size  field");
    macro_rules! wcf {
        ($f:ident, $t:ty) => {
            println!(
                "  {:>6}  {:>5}  {}",
                offset_of!(WrapperConfigV16, $f),
                size_of::<$t>(),
                stringify!($f)
            );
        };
    }
    wcf!(admin, [u8; 32]);
    wcf!(collateral_mint, [u8; 32]);
    wcf!(maintenance_fee_per_slot, u128);
    wcf!(trade_fee_base_bps, u64);
    wcf!(permissionless_resolve_stale_slots, u64);
    wcf!(force_close_delay_slots, u64);
    wcf!(last_good_oracle_slot, u64);
    wcf!(insurance_authority, [u8; 32]);
    wcf!(insurance_operator, [u8; 32]);
    wcf!(backing_bucket_authority, [u8; 32]);
    wcf!(asset_authority, [u8; 32]);
    wcf!(mark_authority, [u8; 32]);
    wcf!(insurance_withdraw_deposit_remaining, u128);
    wcf!(insurance_withdraw_max_bps, u16);
    wcf!(liquidation_cranker_fee_share_bps, u16);
    wcf!(unit_scale, u32);
    wcf!(conf_filter_bps, u16);
    wcf!(insurance_withdraw_deposits_only, u8);
    wcf!(oracle_mode, u8);
    wcf!(oracle_leg_count, u8);
    wcf!(oracle_leg_flags, u8);
    wcf!(invert, u8);
    wcf!(insurance_withdraw_cooldown_slots, u64);
    wcf!(last_insurance_withdraw_slot, u64);
    wcf!(max_staleness_secs, u64);
    wcf!(hybrid_soft_stale_slots, u64);
    wcf!(mark_ewma_e6, u64);
    wcf!(mark_ewma_last_slot, u64);
    wcf!(mark_ewma_halflife_slots, u64);
    wcf!(mark_min_fee, u64);
    wcf!(oracle_target_price_e6, u64);
    wcf!(oracle_target_publish_time, i64);
    wcf!(oracle_leg_feeds, [[u8; 32]; 3]);
    wcf!(oracle_leg_prices_e6, [u64; 3]);
    wcf!(oracle_leg_publish_times, [i64; 3]);

    println!(
        "\n=== MarketGroupV16HeaderAccount (size={} align={}) ===",
        size_of::<MarketGroupV16HeaderAccount>(),
        align_of::<MarketGroupV16HeaderAccount>()
    );
    println!("  offset  size  field");
    macro_rules! mgf {
        ($f:ident, $t:ty) => {
            println!(
                "  {:>6}  {:>5}  {}",
                offset_of!(MarketGroupV16HeaderAccount, $f),
                size_of::<$t>(),
                stringify!($f)
            );
        };
    }
    mgf!(market_group_id, [u8; 32]);
    mgf!(config, V16ConfigAccount);
    mgf!(asset_slot_capacity, u32);
    mgf!(vault, u128);
    mgf!(insurance, u128);
    mgf!(c_tot, u128);
    mgf!(pnl_pos_tot, u128);
    mgf!(pnl_pos_bound_tot_num, u128);
    mgf!(pnl_pos_bound_tot, u128);
    mgf!(pnl_matured_pos_tot, u128);
    mgf!(materialized_portfolio_count, u64);
    mgf!(stale_certificate_count, u64);
    mgf!(b_stale_account_count, u64);
    mgf!(negative_pnl_account_count, u64);
    mgf!(risk_epoch, u64);
    mgf!(asset_set_epoch, u64);
    mgf!(asset_activation_count, u64);
    mgf!(last_asset_activation_slot, u64);
    mgf!(next_market_id, u64);
    mgf!(oracle_epoch, u64);
    mgf!(funding_epoch, u64);
    mgf!(slot_last, u64);
    mgf!(current_slot, u64);
    mgf!(bankruptcy_hlock_active, u8);
    mgf!(threshold_stress_active, u8);
    mgf!(loss_stale_active, u8);
    mgf!(recovery_reason, u8);
    mgf!(mode, u8);
    mgf!(resolved_slot, u64);
    mgf!(payout_snapshot, u128);
    mgf!(payout_snapshot_pnl_pos_tot, u128);
    mgf!(payout_snapshot_captured, u8);
    mgf!(resolved_payout_ledger, ResolvedPayoutLedgerV16Account);

    println!(
        "\n=== PortfolioAccountV16Account (size={} align={}) ===",
        size_of::<PortfolioAccountV16Account>(),
        align_of::<PortfolioAccountV16Account>()
    );
    println!("  offset  size  field");
    macro_rules! pf {
        ($f:ident, $t:ty) => {
            println!(
                "  {:>6}  {:>5}  {}",
                offset_of!(PortfolioAccountV16Account, $f),
                size_of::<$t>(),
                stringify!($f)
            );
        };
    }
    pf!(provenance_header, ProvenanceHeaderV16Account);
    pf!(owner, [u8; 32]);
    pf!(capital, u128);
    pf!(pnl, i128);
    pf!(reserved_pnl, u128);
    pf!(fee_credits, i128);
    pf!(cancel_deposit_escrow, u128);
    pf!(last_fee_slot, u64);
    pf!(active_bitmap, u64); // [u64; 1]
    pf!(legs, PortfolioLegV16Account);
    pf!(health_cert, HealthCertV16Account);
    pf!(stale_state, u8);
    pf!(b_stale_state, u8);
    pf!(rebalance_lock, u8);
    pf!(liquidation_lock, u8);
    pf!(close_progress, CloseProgressLedgerV16Account);
    pf!(resolved_payout_receipt, ResolvedPayoutReceiptV16Account);

    println!(
        "\n=== Dynamic tails ===\n  market slot stride={} (EngineAssetSlotV16Account {} + oracle profile {})\n  portfolio source domain stride={}",
        MARKET_ASSET_SLOT_LEN,
        size_of::<EngineAssetSlotV16Account>(),
        ASSET_ORACLE_PROFILE_LEN,
        PORTFOLIO_SOURCE_DOMAIN_LEN
    );

    println!(
        "\n=== PortfolioSourceDomainV16Account (size={} align={}) ===",
        size_of::<PortfolioSourceDomainV16Account>(),
        align_of::<PortfolioSourceDomainV16Account>()
    );
    println!("  offset  size  field");
    macro_rules! sdf {
        ($f:ident, $t:ty) => {
            println!(
                "  {:>6}  {:>5}  {}",
                offset_of!(PortfolioSourceDomainV16Account, $f),
                size_of::<$t>(),
                stringify!($f)
            );
        };
    }
    sdf!(source_claim_market_id, u64);
    sdf!(source_claim_bound_num, u128);
    sdf!(source_claim_liened_num, u128);
    sdf!(source_claim_counterparty_liened_num, u128);
    sdf!(source_claim_insurance_liened_num, u128);
    sdf!(source_lien_effective_reserved, u128);
    sdf!(source_lien_counterparty_backing_num, u128);
    sdf!(source_lien_insurance_backing_num, u128);
    sdf!(source_claim_impaired_num, u128);
    sdf!(source_lien_impaired_effective_reserved, u128);

    println!(
        "\n=== EngineAssetSlotV16Account (size={} align={}) ===",
        size_of::<EngineAssetSlotV16Account>(),
        align_of::<EngineAssetSlotV16Account>()
    );
    println!("  offset  size  field");
    macro_rules! esf {
        ($f:ident, $t:ty) => {
            println!(
                "  {:>6}  {:>5}  {}",
                offset_of!(EngineAssetSlotV16Account, $f),
                size_of::<$t>(),
                stringify!($f)
            );
        };
    }
    esf!(asset, AssetStateV16Account);
    esf!(insurance_domain_budget_long, u128);
    esf!(insurance_domain_budget_short, u128);
    esf!(insurance_domain_spent_long, u128);
    esf!(insurance_domain_spent_short, u128);
    esf!(pending_domain_loss_barrier_long, u64);
    esf!(pending_domain_loss_barrier_short, u64);
    esf!(source_credit_long, SourceCreditStateV16Account);
    esf!(source_credit_short, SourceCreditStateV16Account);
    esf!(backing_long, BackingBucketV16Account);
    esf!(backing_short, BackingBucketV16Account);
    esf!(
        insurance_reservation_long,
        InsuranceCreditReservationV16Account
    );
    esf!(
        insurance_reservation_short,
        InsuranceCreditReservationV16Account
    );

    println!(
        "\n=== AssetStateV16Account ({}) ===",
        size_of::<AssetStateV16Account>()
    );
    println!("  offset  size  field");
    macro_rules! af {
        ($f:ident, $t:ty) => {
            println!(
                "  {:>6}  {:>5}  {}",
                offset_of!(AssetStateV16Account, $f),
                size_of::<$t>(),
                stringify!($f)
            );
        };
    }
    af!(lifecycle, u8);
    af!(raw_oracle_target_price, u64);
    af!(effective_price, u64);
    af!(fund_px_last, u64);
    af!(slot_last, u64);
    af!(a_long, u128);
    af!(a_short, u128);
    af!(k_long, i128);
    af!(k_short, i128);
    af!(f_long_num, i128);
    af!(f_short_num, i128);
    af!(k_epoch_start_long, i128);
    af!(k_epoch_start_short, i128);
    af!(f_epoch_start_long_num, i128);
    af!(f_epoch_start_short_num, i128);
    af!(b_long_num, u128);
    af!(b_short_num, u128);
    af!(b_epoch_start_long_num, u128);
    af!(b_epoch_start_short_num, u128);
    af!(oi_eff_long_q, u128);
    af!(oi_eff_short_q, u128);
    af!(stored_pos_count_long, u64);
    af!(stored_pos_count_short, u64);
    af!(stale_account_count_long, u64);
    af!(stale_account_count_short, u64);
    af!(pending_obligation_count_long, u64);
    af!(pending_obligation_count_short, u64);
    af!(loss_weight_sum_long, u128);
    af!(loss_weight_sum_short, u128);
    af!(social_loss_remainder_long_num, u128);
    af!(social_loss_remainder_short_num, u128);
    af!(social_loss_dust_long_num, u128);
    af!(social_loss_dust_short_num, u128);
    af!(explicit_unallocated_loss_long, u128);
    af!(explicit_unallocated_loss_short, u128);
    af!(epoch_long, u64);
    af!(epoch_short, u64);
    af!(mode_long, u8);
    af!(mode_short, u8);

    println!("\n=== Sub-struct sizes ===");
    println!(
        "  AssetStateV16Account             = {}",
        size_of::<AssetStateV16Account>()
    );
    println!(
        "  PortfolioLegV16Account           = {}",
        size_of::<PortfolioLegV16Account>()
    );
    println!(
        "  V16ConfigAccount                 = {}",
        size_of::<V16ConfigAccount>()
    );
    println!(
        "  ProvenanceHeaderV16Account       = {}",
        size_of::<ProvenanceHeaderV16Account>()
    );
    println!(
        "  HealthCertV16Account             = {}",
        size_of::<HealthCertV16Account>()
    );
    println!(
        "  ResolvedPayoutLedgerV16Account   = {}",
        size_of::<ResolvedPayoutLedgerV16Account>()
    );
    println!(
        "  ResolvedPayoutReceiptV16Account  = {}",
        size_of::<ResolvedPayoutReceiptV16Account>()
    );
    println!(
        "  SourceCreditStateV16Account      = {}",
        size_of::<SourceCreditStateV16Account>()
    );
    println!(
        "  BackingBucketV16Account          = {}",
        size_of::<BackingBucketV16Account>()
    );
    println!(
        "  InsuranceCreditReservationV16Account = {}",
        size_of::<InsuranceCreditReservationV16Account>()
    );
    println!(
        "  CloseProgressLedgerV16Account    = {}",
        size_of::<CloseProgressLedgerV16Account>()
    );
}
