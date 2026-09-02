use litesvm::LiteSVM;
use percolator::{
    AssetLifecycleV16, BackingBucketStatusV16, CloseProgressLedgerV16, MarketModeV16,
    PermissionlessRecoveryReasonV16, ResolvedPayoutLedgerV16, ResolvedPayoutReceiptV16,
    SideModeV16, SideV16, TradeRequestV16, ADL_ONE, BOUND_SCALE, POS_SCALE,
};
use percolator_prog::{
    constants::{
        ASSET_ORACLE_WRAPPER_LEN, MARKET_GROUP_OFF, MATCHER_ABI_VERSION,
        ORACLE_LEG_FLAG_DIVIDE_LEG2, ORACLE_LEG_FLAG_DIVIDE_LEG3, PORTFOLIO_ENGINE_ACCOUNT_LEN,
    },
    error::PercolatorError,
    ix::{BatchTradeCpiLeg, BatchTradeLeg, CrankObservationHint, Instruction as ProgInstruction},
    oracle_v16, processor, state,
    state::{MarketGroupV16, PortfolioAccountV16},
};
use solana_sdk::{
    account::Account,
    clock::Clock,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};
use std::path::PathBuf;

#[allow(dead_code)]
mod support;

use support::v16_svm::assert_closed_market_tombstone;

const CRANK_CU_LIMIT: u64 = 325_000;
const CUSTODY_CU_LIMIT: u64 = 300_000;
const TRADE_CU_LIMIT: u64 = 345_000;
const MULTI_ASSET_OPEN_TRADE_CU_LIMIT: u64 = 750_000;
const MATCHER_CONTEXT_LEN: usize = 320;
const MAX_10M_MARKET_SLOTS: usize = 5_782;
const CERTIFIED_ENGINE_GIT_SOURCE: &str =
    "git+https://github.com/aeyakovenko/percolator?rev=495a5590c97055bd71c6f94d849ff0298f243145#\
     495a5590c97055bd71c6f94d849ff0298f243145";

fn assert_certified_engine_pin(context: &str) {
    assert!(
        include_str!("../Cargo.lock").contains(CERTIFIED_ENGINE_GIT_SOURCE),
        "{context} is bound to the exact certified engine pin",
    );
}

fn next_control_sequence(current: u64) -> u64 {
    current.checked_add(1).expect("control sequence exhausted")
}

const fn first_generation_market_id(asset_index: u16) -> u64 {
    asset_index as u64 + 1
}

fn crank_observations(asset_index: u16) -> Vec<CrankObservationHint> {
    crank_observations_for_assets(&[asset_index])
}

fn crank_observations_for_assets(asset_indices: &[u16]) -> Vec<CrankObservationHint> {
    asset_indices
        .iter()
        .copied()
        .map(|asset_index| CrankObservationHint {
            asset_index,
            oracle_accounts: 0,
        })
        .collect()
}

// Two independent winners register claims against one undercollateralized source domain. Vary both
// refresh and permissionless resolved-close priority; neither schedule may capture backing first.

fn crank_observations_with_accounts(
    asset_index: u16,
    oracle_accounts: u8,
) -> Vec<CrankObservationHint> {
    vec![CrankObservationHint {
        asset_index,
        oracle_accounts,
    }]
}

struct PublicActiveCloseFixture {
    env: V16CuEnv,
    loss_owner: Keypair,
    loss: Pubkey,
    asset1_counterparty_owner: Keypair,
    asset1_counterparty: Pubkey,
    live_counterparty_owner: Keypair,
    live_counterparty: Pubkey,
    live_peer_owner: Keypair,
    live_peer: Pubkey,
}

const PUBLIC_RELEASED_PNL_FIXTURE_AMOUNT: u128 = 50_000;

struct PublicReleasedPnlFixture {
    env: V16CuEnv,
    winner_owner: Keypair,
    winner: Pubkey,
    loser: Pubkey,
}

fn public_released_pnl_fixture() -> PublicReleasedPnlFixture {
    const INITIAL_PRICE: u64 = 1_000_000;
    const WINNING_PRICE: u64 = 1_050_000;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        max_abs_funding_e9_per_slot: 1_000,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.top_up_backing_bucket(1, 75_000, 10_000);

    let winner_owner = Keypair::new();
    let loser_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser = env.create_portfolio(&loser_owner);
    env.deposit(&winner_owner, winner, 1_000_000);
    env.deposit(&loser_owner, loser, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, WINNING_PRICE);
    for portfolio in [loser, winner] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
    }
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        -SIZE_Q,
        WINNING_PRICE,
        0,
    );

    let winner_state = env.portfolio_state(winner);
    assert!(
        !has_active_leg_for_asset(&winner_state, 0),
        "public close must leave the winner flat"
    );
    assert_eq!(
        winner_state.pnl.get(),
        PUBLIC_RELEASED_PNL_FIXTURE_AMOUNT as i128,
        "public price move and close must create the expected source-backed claim"
    );
    PublicReleasedPnlFixture {
        env,
        winner_owner,
        winner,
        loser,
    }
}

fn public_asset1_bankrupt_close_fixture() -> PublicActiveCloseFixture {
    public_asset1_bankrupt_close_fixture_impl(None, None, 1, true).0
}

fn public_asset1_bankrupt_close_fixture_before_close_with_b_chunk_atoms(
    public_b_chunk_atoms: u128,
) -> PublicActiveCloseFixture {
    public_asset1_bankrupt_close_fixture_impl(None, None, public_b_chunk_atoms, false).0
}

fn public_asset1_bankrupt_close_fixture_with_asset0_external_oracle(
) -> (PublicActiveCloseFixture, Pubkey) {
    let (fixture, oracle) =
        public_asset1_bankrupt_close_fixture_impl(Some([0x58; 32]), None, 1, true);
    (
        fixture,
        oracle.expect("external-oracle fixture must return its feed account"),
    )
}

fn public_asset1_bankrupt_close_fixture_with_counterparty_asset0_short() -> PublicActiveCloseFixture
{
    public_asset1_bankrupt_close_fixture_impl(None, Some(POS_SCALE / 20), 1, true).0
}

fn public_asset1_bankrupt_close_fixture_impl(
    asset0_external_feed: Option<[u8; 32]>,
    counterparty_asset0_short_q: Option<u128>,
    public_b_chunk_atoms: u128,
    start_close: bool,
) -> (PublicActiveCloseFixture, Option<Pubkey>) {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: if counterparty_asset0_short_q.is_some() {
            2
        } else {
            1
        },
        max_bankrupt_close_lifetime_slots: 2,
        public_b_chunk_atoms,
        ..V16CuMarketParams::default()
    });
    let asset0_oracle = if let Some(feed) = asset0_external_feed {
        set_test_clock(&mut env, 1, 100);
        let oracle = env.set_pyth_price_with_conf(&feed, 100, -6, 0, 100);
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [feed, [0; 32], [0; 32]],
            &[oracle],
            1,
            100,
            0,
            0,
            10,
            0,
        )
        .expect("configure fixture asset-0 external oracle");
        Some(oracle)
    } else {
        env.configure_auth_mark_with_cu(0, 100);
        None
    };
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.update_market_init_fee_policy_with_cu(1);

    let base_long_owner = Keypair::new();
    let base_short_owner = Keypair::new();
    let base_long = env.create_portfolio(&base_long_owner);
    let base_short = env.create_portfolio(&base_short_owner);
    env.deposit(&base_long_owner, base_long, 1_000);
    env.deposit(&base_short_owner, base_short, 1_000);
    env.trade_asset_with_cu(
        0,
        &base_long_owner,
        base_long,
        &base_short_owner,
        base_short,
        POS_SCALE as i128,
        100,
        0,
    );

    let creator = if counterparty_asset0_short_q.is_some() {
        Keypair::from_bytes(&env.admin.to_bytes()).expect("copy fixture market authority")
    } else {
        Keypair::new()
    };
    let creator_key = creator.pubkey();
    if counterparty_asset0_short_q.is_none() {
        env.activate_permissionless_asset_with_fee(
            &creator,
            1,
            1,
            100,
            creator_key,
            creator_key,
            creator_key,
            creator_key,
            1,
        );
    }
    env.configure_auth_mark_for_asset_with_authority(1, &creator, 1, 100);

    let loss_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let loss = env.create_portfolio(&loss_owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&loss_owner, loss, 2);
    env.deposit(&counterparty_owner, counterparty, 10);
    env.trade_asset_with_cu(
        1,
        &counterparty_owner,
        counterparty,
        &loss_owner,
        loss,
        (POS_SCALE / 50) as i128,
        100,
        0,
    );
    if let Some(size_q) = counterparty_asset0_short_q {
        env.trade_asset_with_cu(
            0,
            &base_long_owner,
            base_long,
            &counterparty_owner,
            counterparty,
            size_q as i128,
            100,
            0,
        );
    }

    for (slot, mark) in [(2u64, 200u64), (3, 300)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_with_authority(1, &creator, slot, mark);
        env.crank(
            counterparty,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(1),
            },
        );
    }
    env.crank(
        loss,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(1),
        },
    );

    env.svm.warp_to_slot(4);
    env.try_shutdown_asset_with_authority(&creator, 1, 4)
        .expect("permissionless creator shuts down only its own asset");
    if start_close {
        env.forfeit_recovery_leg_with_cu(&loss_owner, loss, 1, 1);
        let ledger = close_progress(&env.portfolio_state(loss));
        assert!(ledger.active && ledger.residual_remaining > 0);
        assert_eq!(ledger.asset_index, 1);
    }
    assert_eq!(env.market_state().1.mode, MarketModeV16::Live);

    (
        PublicActiveCloseFixture {
            env,
            loss_owner,
            loss,
            asset1_counterparty_owner: counterparty_owner,
            asset1_counterparty: counterparty,
            live_counterparty_owner: base_long_owner,
            live_counterparty: base_long,
            live_peer_owner: base_short_owner,
            live_peer: base_short,
        },
        asset0_oracle,
    )
}

fn active_bitmap_with(indices: &[usize]) -> percolator::V16ActiveBitmap {
    let mut bitmap = percolator::active_bitmap_empty();
    for &idx in indices {
        assert!(
            idx < percolator::V16_MAX_PORTFOLIO_ASSETS_N,
            "active bitmap test index out of range"
        );
        bitmap[idx / 64] |= 1u64 << (idx % 64);
    }
    bitmap
}

fn active_leg_for_asset(
    account: &PortfolioAccountV16,
    asset_index: usize,
) -> percolator::PortfolioLegV16 {
    account
        .legs
        .iter()
        .copied()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index as usize == asset_index)
        .unwrap()
}

fn reference_current_epoch_effective_abs(
    group: &MarketGroupV16,
    leg: percolator::PortfolioLegV16,
) -> u128 {
    let asset = group.assets[leg.asset_index as usize];
    let (current_a, current_epoch) = match leg.side {
        SideV16::Long => (asset.a_long, asset.epoch_long),
        SideV16::Short => (asset.a_short, asset.epoch_short),
    };
    assert_eq!(
        leg.epoch_snap, current_epoch,
        "effective-quantity oracle requires a current-epoch leg"
    );
    support::reference_math::mul_div_ceil(leg.basis_pos_q.unsigned_abs(), current_a, leg.a_basis)
        .expect("bounded reference ADL effective quantity")
}

fn reference_raw_basis_for_current_effective(
    group: &MarketGroupV16,
    leg: percolator::PortfolioLegV16,
    effective_abs_q: u128,
) -> u128 {
    let asset = group.assets[leg.asset_index as usize];
    let (current_a, current_epoch) = match leg.side {
        SideV16::Long => (asset.a_long, asset.epoch_long),
        SideV16::Short => (asset.a_short, asset.epoch_short),
    };
    assert_eq!(
        leg.epoch_snap, current_epoch,
        "raw-basis oracle requires a current-epoch leg"
    );
    support::reference_math::mul_div_floor(effective_abs_q, leg.a_basis, current_a)
        .expect("bounded reference retained raw basis")
}

fn has_active_leg_for_asset(account: &PortfolioAccountV16, asset_index: usize) -> bool {
    account
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .any(|leg| leg.active && leg.asset_index as usize == asset_index)
}

fn active_bitmap(account: &PortfolioAccountV16) -> percolator::V16ActiveBitmap {
    state::portfolio_active_bitmap(account)
}

fn leg(account: &PortfolioAccountV16, slot: usize) -> percolator::PortfolioLegV16 {
    account.legs[slot].try_to_runtime().unwrap()
}

fn health_cert(account: &PortfolioAccountV16) -> percolator::HealthCertV16 {
    account.health_cert.try_to_runtime().unwrap()
}

fn close_progress(account: &PortfolioAccountV16) -> CloseProgressLedgerV16 {
    account.close_progress.try_to_runtime().unwrap()
}

fn resolved_receipt(account: &PortfolioAccountV16) -> ResolvedPayoutReceiptV16 {
    account.resolved_payout_receipt.try_to_runtime().unwrap()
}

fn assert_domain_budget_remaining_total_consistent(group: &MarketGroupV16, label: &str) {
    assert_eq!(
        group.insurance_domain_budget.len(),
        group.insurance_domain_spent.len(),
        "{label}: budget/spent domain arrays have matching length",
    );
    let mut remaining_total = 0u128;
    for (domain, (&budget, &spent)) in group
        .insurance_domain_budget
        .iter()
        .zip(group.insurance_domain_spent.iter())
        .enumerate()
    {
        let remaining = budget
            .checked_sub(spent)
            .unwrap_or_else(|| panic!("{label}: domain {domain} spent exceeds budget"));
        remaining_total = remaining_total
            .checked_add(remaining)
            .unwrap_or_else(|| panic!("{label}: remaining-total overflow"));
    }
    assert_eq!(
        group.insurance_domain_budget_remaining_total, remaining_total,
        "{label}: aggregate remaining budget matches per-domain budget minus spent",
    );
}

fn market_engine_slot_bytes(data: &[u8], asset_index: usize) -> &[u8] {
    let slot_start = MARKET_GROUP_OFF
        + percolator::MarketGroupV16HeaderAccount::dynamic_asset_slot_offset::<
            state::AssetOracleStorageV16,
        >(asset_index)
        .unwrap()
        + ASSET_ORACLE_WRAPPER_LEN;
    let slot_end = slot_start + core::mem::size_of::<percolator::EngineAssetSlotV16Account>();
    &data[slot_start..slot_end]
}

fn market_group_header_bytes(data: &[u8]) -> &percolator::MarketGroupV16HeaderAccount {
    let start = MARKET_GROUP_OFF;
    let end = start + core::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
    bytemuck::from_bytes(&data[start..end])
}

fn changed_byte_offsets(before: &[u8], after: &[u8]) -> Vec<usize> {
    assert_eq!(before.len(), after.len());
    before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter_map(|(offset, (before, after))| (before != after).then_some(offset))
        .collect()
}

fn canonical_active_engine_slot(
    market_id: u64,
    price: u64,
    slot_last: u64,
    budget_long: u128,
    budget_short: u128,
) -> percolator::EngineAssetSlotV16Account {
    let mut asset = percolator::AssetStateV16::default();
    asset.market_id = market_id;
    asset.lifecycle = AssetLifecycleV16::Active;
    asset.raw_oracle_target_price = price;
    asset.effective_price = price;
    asset.fund_px_last = price;
    asset.slot_last = slot_last;
    let mut slot = percolator::EngineAssetSlotV16Account::empty_for_market(market_id);
    slot.asset = percolator::AssetStateV16Account::from_runtime(&asset);
    slot.insurance_domain_budget_long = percolator::V16PodU128::new(budget_long);
    slot.insurance_domain_budget_short = percolator::V16PodU128::new(budget_short);
    slot
}

fn canonical_retired_engine_slot(
    market_id: u64,
    price: u64,
    slot_last: u64,
    retired_slot: u64,
    budget_long: u128,
    budget_short: u128,
) -> percolator::EngineAssetSlotV16Account {
    let mut asset = percolator::AssetStateV16::default();
    asset.market_id = market_id;
    asset.retired_slot = retired_slot;
    asset.lifecycle = AssetLifecycleV16::Retired;
    asset.raw_oracle_target_price = price;
    asset.effective_price = price;
    asset.fund_px_last = price;
    asset.slot_last = slot_last;
    let mut slot = percolator::EngineAssetSlotV16Account::empty_for_market(market_id);
    slot.asset = percolator::AssetStateV16Account::from_runtime(&asset);
    slot.insurance_domain_budget_long = percolator::V16PodU128::new(budget_long);
    slot.insurance_domain_budget_short = percolator::V16PodU128::new(budget_short);
    slot
}

fn program_path() -> PathBuf {
    let path = if let Some(path) = std::env::var_os("PERCOLATOR_FUZZ_SBF") {
        PathBuf::from(path)
    } else if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        PathBuf::from(target_dir)
            .join("deploy")
            .join("percolator_prog.so")
    } else {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("target/deploy/percolator_prog.so");
        path
    };
    assert!(
        path.exists(),
        "BPF not found at {:?}. Run `cargo build-sbf --no-default-features` first",
        path
    );
    path
}

fn matcher_program_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push("percolator-match/target/deploy/percolator_match.so");
    assert!(
        path.exists(),
        "matcher BPF not found at {:?}. Run `cd ../percolator-match && cargo build-sbf` first",
        path
    );
    path
}

fn auth_matcher_program_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures/auth_matcher/target/deploy/auth_matcher.so");
    assert!(
        path.exists(),
        "auth matcher BPF not found at {:?}. Run `cd tests/fixtures/auth_matcher && cargo build-sbf` first",
        path
    );
    path
}

fn spl_token_program_path() -> PathBuf {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
            home.push(".cargo");
            home
        });
    let registry_src = cargo_home.join("registry/src");
    for registry in std::fs::read_dir(&registry_src).expect("registry/src") {
        let registry = registry.expect("registry entry").path();
        let candidate = registry.join("litesvm-0.1.0/src/spl/programs/spl_token-3.5.0.so");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("could not find LiteSVM SPL Token BPF under {registry_src:?}");
}

fn spl_token_2022_program_path() -> PathBuf {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
            home.push(".cargo");
            home
        });
    let registry_src = cargo_home.join("registry/src");
    for registry in std::fs::read_dir(&registry_src).expect("registry/src") {
        let registry = registry.expect("registry entry").path();
        let candidate = registry.join("litesvm-0.1.0/src/spl/programs/spl_token_2022-1.0.0.so");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("could not find LiteSVM SPL Token-2022 BPF under {registry_src:?}");
}

fn associated_token_program_path() -> PathBuf {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
            home.push(".cargo");
            home
        });
    let registry_src = cargo_home.join("registry/src");
    for registry in std::fs::read_dir(&registry_src).expect("registry/src") {
        let registry = registry.expect("registry entry").path();
        let candidate =
            registry.join("litesvm-0.1.0/src/spl/programs/spl_associated_token_account-1.1.1.so");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("could not find LiteSVM Associated Token BPF under {registry_src:?}");
}

fn matcher_delegate_key(
    program_id: &Pubkey,
    market: &Pubkey,
    maker: &Pubkey,
    maker_owner: &Pubkey,
    matcher_program: &Pubkey,
    matcher_context: &Pubkey,
) -> Pubkey {
    Pubkey::find_program_address(
        &[
            b"matcher",
            market.as_ref(),
            maker.as_ref(),
            maker_owner.as_ref(),
            matcher_program.as_ref(),
            matcher_context.as_ref(),
        ],
        program_id,
    )
    .0
}

fn encode_matcher_init_passive(max_fill_abs: u128) -> Vec<u8> {
    encode_matcher_init_passive_with_spread(max_fill_abs, 0, 100)
}

fn encode_matcher_init_passive_with_spread(
    max_fill_abs: u128,
    base_spread_bps: u32,
    max_total_bps: u32,
) -> Vec<u8> {
    let mut data = vec![0u8; 66];
    data[0] = 2;
    data[1] = 0;
    data[6..10].copy_from_slice(&base_spread_bps.to_le_bytes());
    data[10..14].copy_from_slice(&max_total_bps.to_le_bytes());
    data[34..50].copy_from_slice(&max_fill_abs.to_le_bytes());
    data
}

fn make_mint_data_with_decimals(decimals: u8) -> Vec<u8> {
    let mut data = vec![0u8; Mint::LEN];
    Mint::pack(
        Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut data,
    )
    .unwrap();
    data
}

fn make_mint_data() -> Vec<u8> {
    make_mint_data_with_decimals(0)
}

/// The canonical vault address the wrapper now pins to (F-VAULT-FRAG fix): the Associated Token
/// Account of the vault_authority PDA for the given mint.
fn associated_token_program_id() -> Pubkey {
    solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
}

fn canonical_vault_ata(vault_authority: Pubkey, mint: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            vault_authority.as_ref(),
            spl_token::ID.as_ref(),
            mint.as_ref(),
        ],
        &associated_token_program_id(),
    )
    .0
}

fn make_token_data(mint: Pubkey, owner: Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint,
            owner,
            amount,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        },
        &mut data,
    )
    .unwrap();
    data
}

fn make_delegated_token_data(
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
    delegate: Pubkey,
    delegated_amount: u64,
) -> Vec<u8> {
    let mut data = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint,
            owner,
            amount,
            delegate: COption::Some(delegate),
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount,
            close_authority: COption::None,
        },
        &mut data,
    )
    .unwrap();
    data
}

fn make_closable_token_data(
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
    close_authority: Pubkey,
) -> Vec<u8> {
    let mut data = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint,
            owner,
            amount,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::Some(close_authority),
        },
        &mut data,
    )
    .unwrap();
    data
}

fn make_pyth_data(
    feed_id: &[u8; 32],
    price: i64,
    expo: i32,
    conf: u64,
    publish_time: i64,
) -> Vec<u8> {
    let mut data = vec![0u8; 134];
    data[0..8].copy_from_slice(&[0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd]);
    data[40] = 1;
    data[41..73].copy_from_slice(feed_id);
    data[73..81].copy_from_slice(&price.to_le_bytes());
    data[81..89].copy_from_slice(&conf.to_le_bytes());
    data[89..93].copy_from_slice(&expo.to_le_bytes());
    data[93..101].copy_from_slice(&publish_time.to_le_bytes());
    data
}

// Craft a Switchboard On-Demand PullFeed account (mirrors read_switchboard_price_e6 / SB_OFF_* in
// src/v16_program.rs). Layout: 8-byte discriminator + fields at the documented absolute offsets.
fn make_switchboard_data(
    feed_hash: &[u8; 32],
    value: i128,
    std_dev: i128,
    publish_time: i64,
    num_samples: u8,
    min_sample_size: u8,
    result_slot: u64,
) -> Vec<u8> {
    const SB_LEN: usize = 3_208;
    const DISC: [u8; 8] = [196, 27, 108, 196, 10, 215, 219, 40];
    let mut data = vec![0u8; SB_LEN];
    data[0..8].copy_from_slice(&DISC);
    data[2120..2152].copy_from_slice(feed_hash); // SB_OFF_FEED_HASH = 8 + 2112
    data[2215] = min_sample_size; // SB_OFF_MIN_SAMPLE_SIZE = 8 + 2207
    data[2216..2224].copy_from_slice(&publish_time.to_le_bytes()); // SB_OFF_LAST_UPDATE_TIMESTAMP = 8 + 2208
    data[2264..2280].copy_from_slice(&value.to_le_bytes()); // SB_OFF_RESULT_VALUE = 8 + 2256
    data[2280..2296].copy_from_slice(&std_dev.to_le_bytes()); // SB_OFF_RESULT_STD_DEV = 8 + 2272
    data[2360] = num_samples; // SB_OFF_RESULT_NUM_SAMPLES = 8 + 2352
    data[2361] = 0; // SB_OFF_RESULT_SUBMISSION_IDX = 8 + 2353
    data[2368..2376].copy_from_slice(&result_slot.to_le_bytes()); // SB_OFF_RESULT_SLOT = 8 + 2360
    data[2952..2960].copy_from_slice(&publish_time.to_le_bytes()); // selected submission timestamp
    data
}

fn make_chainlink_data(
    version: u8,
    decimals: u8,
    latest_round_id: u32,
    live_length: u32,
    result_slot: u64,
    publish_time: u32,
    answer: i128,
) -> Vec<u8> {
    const CL_LEN: usize = 8 + 192 + 48;
    const DISC: [u8; 8] = [96, 179, 69, 66, 128, 129, 73, 117];
    let mut data = vec![0u8; CL_LEN];
    data[0..8].copy_from_slice(&DISC);
    data[8] = version;
    data[138] = decimals;
    data[143..147].copy_from_slice(&latest_round_id.to_le_bytes());
    data[148..152].copy_from_slice(&live_length.to_le_bytes());
    data[200..208].copy_from_slice(&result_slot.to_le_bytes());
    data[208..212].copy_from_slice(&publish_time.to_le_bytes());
    data[216..232].copy_from_slice(&answer.to_le_bytes());
    data
}

fn cu_ix() -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)
}

fn heap_ix() -> Instruction {
    ComputeBudgetInstruction::request_heap_frame(128 * 1024)
}

struct V16CuEnv {
    svm: LiteSVM,
    program_id: Pubkey,
    payer: Keypair,
    admin: Keypair,
    init_market_cu: u64,
    market: Pubkey,
    mint: Pubkey,
    vault: Pubkey,
    vault_authority: Pubkey,
    portfolio_account_len: usize,
    portfolios: Vec<Pubkey>,
}

#[derive(Clone, Copy)]
struct V16CuMarketParams {
    max_portfolio_assets: u16,
    h_min: u64,
    h_max: u64,
    initial_price: u64,
    min_nonzero_mm_req: u128,
    min_nonzero_im_req: u128,
    maintenance_margin_bps: u64,
    initial_margin_bps: u64,
    max_trading_fee_bps: u64,
    trade_fee_base_bps: u64,
    liquidation_fee_bps: u64,
    liquidation_fee_cap: u128,
    min_liquidation_abs: u128,
    max_price_move_bps_per_slot: u64,
    max_accrual_dt_slots: u64,
    max_abs_funding_e9_per_slot: u64,
    min_funding_lifetime_slots: u64,
    max_account_b_settlement_chunks: u64,
    max_bankrupt_close_chunks: u64,
    max_bankrupt_close_lifetime_slots: u64,
    public_b_chunk_atoms: u128,
    maintenance_fee_per_slot: u128,
}

impl Default for V16CuMarketParams {
    fn default() -> Self {
        Self {
            max_portfolio_assets: 1,
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            min_nonzero_mm_req: 1,
            min_nonzero_im_req: 2,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            trade_fee_base_bps: 0,
            liquidation_fee_bps: 0,
            liquidation_fee_cap: 0,
            min_liquidation_abs: 0,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 1,
            max_account_b_settlement_chunks: 1,
            max_bankrupt_close_chunks: 1,
            max_bankrupt_close_lifetime_slots: 100,
            public_b_chunk_atoms: percolator::MAX_VAULT_TVL,
            maintenance_fee_per_slot: 0,
        }
    }
}

fn init_host_market_data_for_serializer_probe() -> Vec<u8> {
    let params = V16CuMarketParams::default();
    let mut engine_config = percolator::V16Config::public_user_fund(
        params.max_portfolio_assets,
        params.h_min,
        params.h_max,
    );
    engine_config.min_nonzero_mm_req = params.min_nonzero_mm_req;
    engine_config.min_nonzero_im_req = params.min_nonzero_im_req;
    engine_config.maintenance_margin_bps = params.maintenance_margin_bps;
    engine_config.initial_margin_bps = params.initial_margin_bps;
    engine_config.max_trading_fee_bps = params.max_trading_fee_bps;
    engine_config.liquidation_fee_bps = params.liquidation_fee_bps;
    engine_config.liquidation_fee_cap = params.liquidation_fee_cap;
    engine_config.min_liquidation_abs = params.min_liquidation_abs;
    engine_config.max_price_move_bps_per_slot = params.max_price_move_bps_per_slot;
    engine_config.max_accrual_dt_slots = params.max_accrual_dt_slots;
    engine_config.max_abs_funding_e9_per_slot = params.max_abs_funding_e9_per_slot;
    engine_config.min_funding_lifetime_slots = params.min_funding_lifetime_slots;
    engine_config.max_account_b_settlement_chunks = params.max_account_b_settlement_chunks;
    engine_config.max_bankrupt_close_chunks = params.max_bankrupt_close_chunks;
    engine_config.max_bankrupt_close_lifetime_slots = params.max_bankrupt_close_lifetime_slots;
    engine_config.public_b_chunk_atoms = params.public_b_chunk_atoms;

    let mut wrapper = state::WrapperConfigV16::default();
    wrapper.marketauth = [1u8; 32];
    wrapper.collateral_mint = [2u8; 32];
    wrapper.oracle_mode = percolator_prog::constants::ORACLE_MODE_MANUAL;
    wrapper.mark_ewma_e6 = params.initial_price;
    wrapper.oracle_target_price_e6 = params.initial_price;
    wrapper.mark_ewma_halflife_slots = percolator_prog::constants::DEFAULT_MARK_EWMA_HALFLIFE_SLOTS;

    let mut data = vec![
        0u8;
        state::market_account_len_for_capacity(params.max_portfolio_assets as usize)
            .unwrap()
    ];
    state::init_market_account_zero_copy(
        &mut data,
        &wrapper,
        engine_config,
        [9u8; 32],
        params.initial_price,
        0,
    )
    .unwrap();
    data
}

fn init_market_instruction(params: &V16CuMarketParams) -> ProgInstruction {
    ProgInstruction::InitMarket {
        max_portfolio_assets: params.max_portfolio_assets,
        h_min: params.h_min,
        h_max: params.h_max,
        initial_price: params.initial_price,
        min_nonzero_mm_req: params.min_nonzero_mm_req,
        min_nonzero_im_req: params.min_nonzero_im_req,
        maintenance_margin_bps: params.maintenance_margin_bps,
        initial_margin_bps: params.initial_margin_bps,
        max_trading_fee_bps: params.max_trading_fee_bps,
        trade_fee_base_bps: params.trade_fee_base_bps,
        liquidation_fee_bps: params.liquidation_fee_bps,
        liquidation_fee_cap: params.liquidation_fee_cap,
        min_liquidation_abs: params.min_liquidation_abs,
        max_price_move_bps_per_slot: params.max_price_move_bps_per_slot,
        max_accrual_dt_slots: params.max_accrual_dt_slots,
        max_abs_funding_e9_per_slot: params.max_abs_funding_e9_per_slot,
        min_funding_lifetime_slots: params.min_funding_lifetime_slots,
        max_account_b_settlement_chunks: params.max_account_b_settlement_chunks,
        max_bankrupt_close_chunks: params.max_bankrupt_close_chunks,
        max_bankrupt_close_lifetime_slots: params.max_bankrupt_close_lifetime_slots,
        public_b_chunk_atoms: params.public_b_chunk_atoms,
        maintenance_fee_per_slot: params.maintenance_fee_per_slot,
    }
}

impl V16CuEnv {
    fn new() -> Self {
        Self::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000)
    }

    fn new_with_market_params_and_price_move(
        max_portfolio_assets: u16,
        maintenance_margin_bps: u64,
        initial_margin_bps: u64,
        max_price_move_bps_per_slot: u64,
    ) -> Self {
        Self::new_with_market_params_price_move_and_maintenance_fee(
            max_portfolio_assets,
            maintenance_margin_bps,
            initial_margin_bps,
            max_price_move_bps_per_slot,
            0,
        )
    }

    fn new_with_market_params_price_move_and_maintenance_fee(
        max_portfolio_assets: u16,
        maintenance_margin_bps: u64,
        initial_margin_bps: u64,
        max_price_move_bps_per_slot: u64,
        maintenance_fee_per_slot: u128,
    ) -> Self {
        Self::new_with_init_params(V16CuMarketParams {
            max_portfolio_assets,
            maintenance_margin_bps,
            initial_margin_bps,
            max_price_move_bps_per_slot,
            maintenance_fee_per_slot,
            ..V16CuMarketParams::default()
        })
    }

    fn new_with_init_params(params: V16CuMarketParams) -> Self {
        Self::new_with_init_params_and_market_capacity(params, params.max_portfolio_assets as usize)
    }

    fn new_with_init_params_and_market_capacity(
        params: V16CuMarketParams,
        market_capacity: usize,
    ) -> Self {
        Self::new_with_init_params_market_capacity_and_mint_decimals(params, market_capacity, 0)
    }

    fn new_with_init_params_market_capacity_and_mint_decimals(
        params: V16CuMarketParams,
        market_capacity: usize,
        mint_decimals: u8,
    ) -> Self {
        let mut svm = LiteSVM::new();
        let program_id = percolator_prog::id();
        let program_bytes = std::fs::read(program_path()).expect("read BPF");
        svm.add_program(program_id, &program_bytes);
        let token_program_bytes = std::fs::read(spl_token_program_path()).expect("read token BPF");
        svm.add_program(spl_token::ID, &token_program_bytes);

        let payer = Keypair::new();
        let admin = Keypair::new();
        let market = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let vault_authority =
            Pubkey::find_program_address(&[b"vault", market.as_ref()], &program_id).0;
        // F-VAULT-FRAG fix: the vault must be the canonical ATA of (vault_authority, mint).
        let vault = canonical_vault_ata(vault_authority, mint);
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
        svm.set_account(
            mint,
            Account {
                lamports: 1_000_000_000,
                data: make_mint_data_with_decimals(mint_decimals),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        svm.set_account(
            vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(mint, vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        svm.set_account(
            market,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; state::market_account_len_for_capacity(market_capacity).unwrap()],
                owner: program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        let init_market_cu = send_tx(
            &mut svm,
            program_id,
            &payer,
            ProgInstruction::InitMarket {
                max_portfolio_assets: params.max_portfolio_assets,
                h_min: params.h_min,
                h_max: params.h_max,
                initial_price: params.initial_price,
                min_nonzero_mm_req: params.min_nonzero_mm_req,
                min_nonzero_im_req: params.min_nonzero_im_req,
                maintenance_margin_bps: params.maintenance_margin_bps,
                initial_margin_bps: params.initial_margin_bps,
                max_trading_fee_bps: params.max_trading_fee_bps,
                trade_fee_base_bps: params.trade_fee_base_bps,
                liquidation_fee_bps: params.liquidation_fee_bps,
                liquidation_fee_cap: params.liquidation_fee_cap,
                min_liquidation_abs: params.min_liquidation_abs,
                max_price_move_bps_per_slot: params.max_price_move_bps_per_slot,
                max_accrual_dt_slots: params.max_accrual_dt_slots,
                max_abs_funding_e9_per_slot: params.max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots: params.min_funding_lifetime_slots,
                max_account_b_settlement_chunks: params.max_account_b_settlement_chunks,
                max_bankrupt_close_chunks: params.max_bankrupt_close_chunks,
                max_bankrupt_close_lifetime_slots: params.max_bankrupt_close_lifetime_slots,
                public_b_chunk_atoms: params.public_b_chunk_atoms,
                maintenance_fee_per_slot: params.maintenance_fee_per_slot,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new_readonly(mint, false),
            ],
            &[&admin],
        )
        .expect("init market");
        Self {
            svm,
            program_id,
            payer,
            admin,
            init_market_cu,
            market,
            mint,
            vault,
            vault_authority,
            portfolio_account_len: state::portfolio_account_len_for_market_slots(
                params.max_portfolio_assets as usize,
            )
            .unwrap(),
            portfolios: Vec::new(),
        }
    }

    fn create_portfolio(&mut self, owner: &Keypair) -> Pubkey {
        self.create_portfolio_with_cu(owner).0
    }

    fn create_portfolio_with_cu(&mut self, owner: &Keypair) -> (Pubkey, u64) {
        let portfolio = Pubkey::new_unique();
        self.ensure_signer_account(owner.pubkey());
        self.svm
            .set_account(
                portfolio,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; self.portfolio_account_len],
                    owner: self.program_id,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = self
            .send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(self.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[owner],
            )
            .expect("init portfolio");
        self.portfolios.push(portfolio);
        (portfolio, cu)
    }

    fn deposit(&mut self, owner: &Keypair, portfolio: Pubkey, amount: u128) -> Pubkey {
        self.deposit_with_cu(owner, portfolio, amount).0
    }

    fn activate_asset(&mut self, asset_index: u16, now_slot: u64, initial_price: u64) -> u64 {
        self.activate_asset_with_authorities(
            asset_index,
            now_slot,
            initial_price,
            self.admin.pubkey(),
            self.admin.pubkey(),
            self.admin.pubkey(),
            self.admin.pubkey(),
        )
    }

    fn activate_asset_with_authorities(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        insurance_authority: Pubkey,
        insurance_operator: Pubkey,
        backing_bucket_authority: Pubkey,
        oracle_authority: Pubkey,
    ) -> u64 {
        let clock = self.svm.get_sysvar::<Clock>();
        if clock.slot < now_slot {
            self.svm.warp_to_slot(now_slot);
        }
        let market_id = self.market_state().1.next_market_id;
        let authority_epoch = self.control_sequences(0).authority_epoch;
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index,
                market_id,
                authority_epoch,
                now_slot,
                initial_price,
                max_init_fee: u128::MAX,
                insurance_authority: insurance_authority.to_bytes(),
                insurance_operator: insurance_operator.to_bytes(),
                backing_bucket_authority: backing_bucket_authority.to_bytes(),
                oracle_authority: oracle_authority.to_bytes(),
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("activate asset")
    }

    fn update_market_init_fee_policy_with_cu(&mut self, min_init_fee: u128) -> u64 {
        let sequences = self.control_sequences(0);
        let policy_sequence = next_control_sequence(sequences.market_init_fee);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateMarketInitFeePolicy {
                min_init_fee,
                policy_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update market init fee policy")
    }

    fn update_asset_lifecycle_as_admin_with_cu(
        &mut self,
        action: u8,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
    ) -> u64 {
        let authenticated_slot = self.svm.get_sysvar::<Clock>().slot;
        let max_attempts = self.terminal_accrual_attempt_bound(asset_index, authenticated_slot);
        let mut max_cu = 0;
        for _ in 0..max_attempts {
            self.svm.expire_blockhash();
            let market_id = if action == percolator_prog::processor::ASSET_ACTION_ACTIVATE {
                self.market_state().1.next_market_id
            } else {
                self.asset_market_id(asset_index)
            };
            let authority_epoch = self.control_sequences(0).authority_epoch;
            let result = send_tx(
                &mut self.svm,
                self.program_id,
                &self.payer,
                ProgInstruction::UpdateAssetLifecycle {
                    action,
                    asset_index,
                    market_id,
                    authority_epoch,
                    now_slot,
                    initial_price,
                    max_init_fee: u128::MAX,
                    insurance_authority: self.admin.pubkey().to_bytes(),
                    insurance_operator: self.admin.pubkey().to_bytes(),
                    backing_bucket_authority: self.admin.pubkey().to_bytes(),
                    oracle_authority: self.admin.pubkey().to_bytes(),
                },
                vec![
                    AccountMeta::new(self.admin.pubkey(), true),
                    AccountMeta::new(self.market, false),
                ],
                &[&self.admin],
            );
            match result {
                Ok(cu) => return max_cu.max(cu),
                Err(err)
                    if action == percolator_prog::processor::ASSET_ACTION_SHUTDOWN
                        && is_engine_stale_error(&err) =>
                {
                    max_cu = max_cu.max(
                        self.public_terminal_accrual_step(asset_index, authenticated_slot)
                            .unwrap_or_else(|crank_err| {
                                panic!(
                                    "public accrual before asset shutdown failed: {crank_err}; shutdown error: {err}"
                                )
                            }),
                    );
                }
                Err(err) => panic!("update asset lifecycle as admin: {err}"),
            }
        }
        panic!(
            "asset lifecycle transition remained stale after {max_attempts} bounded public accrual attempts"
        )
    }

    fn terminal_accrual_attempt_bound(&self, asset_index: u16, now_slot: u64) -> usize {
        let market_data = self.svm.get_account(&self.market).unwrap().data;
        let (_, group) = state::read_market(&market_data).unwrap();
        let asset = &group.assets[asset_index as usize];
        let max_dt = group.config.max_accrual_dt_slots.max(1);
        let elapsed = now_slot.saturating_sub(asset.slot_last);
        let segments = elapsed / max_dt + u64::from(elapsed % max_dt != 0);
        usize::try_from(segments.saturating_add(4).min(16_384)).unwrap()
    }

    fn public_terminal_accrual_step(
        &mut self,
        asset_index: u16,
        now_slot: u64,
    ) -> Result<u64, String> {
        let portfolios: Vec<Pubkey> = self
            .portfolios
            .iter()
            .rev()
            .copied()
            .filter(|key| {
                self.svm
                    .get_account(key)
                    .is_some_and(|account| account.owner == self.program_id)
            })
            .collect();
        if portfolios.is_empty() {
            return Err("no live public portfolio is available for accrual".to_string());
        }
        let market_data = self.svm.get_account(&self.market).unwrap().data;
        let (cfg, _) = state::read_market(&market_data).unwrap();
        let profile = state::read_asset_oracle_profile(&market_data, asset_index as usize)
            .map_err(|err| format!("read oracle profile: {err:?}"))?;
        let resolve_matured = cfg.permissionless_resolve_stale_slots != 0
            && (now_slot.saturating_sub(cfg.last_good_oracle_slot)
                >= cfg.permissionless_resolve_stale_slots
                || now_slot.saturating_sub(profile.last_good_oracle_slot)
                    >= cfg.permissionless_resolve_stale_slots);
        let oracle_account_count = if resolve_matured {
            0
        } else {
            profile.oracle_leg_count
        };
        let oracle_accounts: Vec<AccountMeta> = profile.oracle_leg_feeds
            [..oracle_account_count as usize]
            .iter()
            .copied()
            .map(Pubkey::new_from_array)
            .map(|key| AccountMeta::new_readonly(key, false))
            .collect();
        let mut failures = Vec::new();
        for portfolio in portfolios {
            let mut accounts = vec![
                AccountMeta::new(self.payer.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ];
            accounts.extend(oracle_accounts.iter().cloned());
            self.svm.expire_blockhash();
            match self.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot,
                    observations: crank_observations_with_accounts(
                        asset_index,
                        oracle_account_count,
                    ),
                },
                accounts,
                &[],
            ) {
                Ok(cu) => return Ok(cu),
                Err(err) => failures.push(err),
            }
        }
        Err(format!(
            "all live public portfolios rejected the accrual crank: {}",
            failures.join(" | ")
        ))
    }

    fn public_terminal_accrual_any_exposed(&mut self, now_slot: u64) -> Result<u64, String> {
        let market_data = self.svm.get_account(&self.market).unwrap().data;
        let (_, group) = state::read_market(&market_data).unwrap();
        let exposed_assets: Vec<u16> = group
            .assets
            .iter()
            .enumerate()
            .filter(|(_, asset)| asset.oi_eff_long_q != 0 || asset.oi_eff_short_q != 0)
            .map(|(asset_index, _)| asset_index as u16)
            .collect();
        let mut failures = Vec::new();
        for asset_index in exposed_assets {
            match self.public_terminal_accrual_step(asset_index, now_slot) {
                Ok(cu) => return Ok(cu),
                Err(err) => {
                    let market_data = self.svm.get_account(&self.market).unwrap().data;
                    let (_, group) = state::read_market(&market_data).unwrap();
                    let asset = group.assets[asset_index as usize];
                    let profile =
                        state::read_asset_oracle_profile(&market_data, asset_index as usize)
                            .unwrap();
                    failures.push(format!(
                        "asset {asset_index} slot={} current={} effective={} raw={} mark={} mark_slot={} funding_mark={} pending={} pending_slot={}: {err}",
                        asset.slot_last,
                        group.current_slot,
                        asset.effective_price,
                        asset.raw_oracle_target_price,
                        profile.mark_ewma_e6,
                        profile.mark_ewma_last_slot,
                        profile.funding_mark_e6,
                        profile.funding_mark_pending_e6,
                        profile.funding_mark_pending_slot,
                    ));
                }
            }
        }
        Err(format!(
            "no exposed asset accepted a terminal accrual step: {}",
            failures.join(" | ")
        ))
    }

    fn try_shutdown_asset_with_authority(
        &mut self,
        authority: &Keypair,
        asset_index: u16,
        now_slot: u64,
    ) -> Result<u64, String> {
        let market_id = self.asset_market_id(asset_index);
        let market = self.svm.get_account(&self.market).expect("market account");
        let (cfg, _, _, _) = state::read_market_config_mode_and_capacity(&market.data)
            .expect("decode lifecycle market config");
        let epoch_asset = if authority.pubkey().to_bytes() == cfg.marketauth {
            0
        } else {
            asset_index as usize
        };
        let authority_epoch = self.control_sequences(epoch_asset).authority_epoch;
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
                asset_index,
                market_id,
                authority_epoch,
                now_slot,
                initial_price: 0,
                max_init_fee: u128::MAX,
                insurance_authority: authority.pubkey().to_bytes(),
                insurance_operator: authority.pubkey().to_bytes(),
                backing_bucket_authority: authority.pubkey().to_bytes(),
                oracle_authority: authority.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    fn try_restart_asset_oracle_with_authority(
        &mut self,
        authority: &Keypair,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
    ) -> Result<u64, String> {
        let sequences = self.control_sequences(asset_index as usize);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::RestartAssetOracle {
                market_id: 0,
                asset_index,
                now_slot,
                initial_price,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    fn update_liquidation_fee_policy_with_cu(&mut self, cranker_share_bps: u16) -> u64 {
        let sequences = self.control_sequences(0);
        let policy_sequence = next_control_sequence(sequences.liquidation_fee);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateLiquidationFeePolicy {
                cranker_share_bps,
                policy_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update liquidation fee policy")
    }

    fn update_backing_fee_policy_with_cu(
        &mut self,
        domain: u16,
        fee_bps: u16,
        insurance_share_bps: u16,
    ) -> u64 {
        let sequences = self.control_sequences(domain as usize / 2);
        let policy_sequence =
            next_control_sequence(sequences.backing_fee.max(sequences.authority_epoch));
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateBackingFeePolicy {
                market_id: 0,
                domain,
                fee_bps,
                insurance_share_bps,
                policy_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update backing fee policy")
    }

    fn update_trade_fee_policy_with_cu(&mut self, trade_fee_base_bps: u64) -> u64 {
        let sequences = self.control_sequences(0);
        let policy_sequence = next_control_sequence(sequences.trade_fee);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps,
                policy_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update trade fee policy")
    }

    fn update_fee_redirect_policy_with_cu(&mut self, redirect_bps: u16) -> u64 {
        let sequences = self.control_sequences(0);
        let policy_sequence = next_control_sequence(sequences.fee_redirect);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateFeeRedirectPolicy {
                redirect_bps,
                policy_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update fee redirect policy")
    }

    fn update_asset_authority_with_cu(&mut self, new_authority: &Keypair) -> u64 {
        self.ensure_signer_account(new_authority.pubkey());
        let authority_epoch = self.control_sequences(0).authority_epoch;
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateAuthority {
                authority_epoch,
                new_pubkey: new_authority.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(new_authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin, new_authority],
        )
        .expect("update market authority")
    }

    fn try_update_per_asset_authority_with_cu(
        &mut self,
        signer: &Keypair,
        new_authority: Option<&Keypair>,
        asset_index: u16,
        kind: u8,
        new_pubkey: [u8; 32],
    ) -> Result<u64, String> {
        self.ensure_signer_account(signer.pubkey());
        let market_id = self.asset_market_id(asset_index);
        let authority_epoch = self.control_sequences(asset_index as usize).authority_epoch;
        let mut signers = vec![signer];
        let new_authority_key = if let Some(new_authority) = new_authority {
            self.ensure_signer_account(new_authority.pubkey());
            signers.push(new_authority);
            new_authority.pubkey()
        } else {
            self.payer.pubkey()
        };
        self.send(
            ProgInstruction::UpdateAssetAuthority {
                asset_index,
                market_id,
                authority_epoch,
                kind,
                new_pubkey,
            },
            vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(new_authority_key, new_authority.is_some()),
                AccountMeta::new(self.market, false),
            ],
            &signers,
        )
    }

    fn update_base_unit_mints_with_cu(
        &mut self,
        primary_mint: Pubkey,
        secondary_mint: Pubkey,
    ) -> u64 {
        let authority_epoch = self.control_sequences(0).authority_epoch;
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateBaseUnitMints {
                primary_mint: primary_mint.to_bytes(),
                secondary_mint: secondary_mint.to_bytes(),
                authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new_readonly(primary_mint, false),
                AccountMeta::new_readonly(secondary_mint, false),
            ],
            &[&self.admin],
        )
        .expect("update base unit mints")
    }

    fn swap_secondary_for_primary_with_cu(
        &mut self,
        primary_source: Pubkey,
        primary_vault: Pubkey,
        secondary_dest: Pubkey,
        secondary_vault: Pubkey,
        amount: u128,
    ) -> u64 {
        let authority_epoch = self.control_sequences(0).authority_epoch;
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::SwapSecondaryForPrimary {
                amount,
                authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new_readonly(self.market, false),
                AccountMeta::new(primary_source, false),
                AccountMeta::new(primary_vault, false),
                AccountMeta::new(secondary_dest, false),
                AccountMeta::new(secondary_vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("swap secondary for primary")
    }

    fn token_account(&mut self, owner: Pubkey, amount: u64) -> Pubkey {
        let token = Pubkey::new_unique();
        self.svm
            .set_account(
                token,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, owner, amount),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        token
    }

    fn ensure_signer_account(&mut self, key: Pubkey) {
        if self.svm.get_account(&key).is_none() {
            self.svm.airdrop(&key, 1_000_000_000).unwrap();
        }
    }

    fn create_mint(&mut self) -> Pubkey {
        let mint = Pubkey::new_unique();
        self.svm
            .set_account(
                mint,
                Account {
                    lamports: 1_000_000_000,
                    data: make_mint_data(),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        mint
    }

    fn token_account_for_mint(&mut self, mint: Pubkey, owner: Pubkey, amount: u64) -> Pubkey {
        let token = Pubkey::new_unique();
        self.svm
            .set_account(
                token,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(mint, owner, amount),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        token
    }

    fn program_account(&mut self, data_len: usize) -> Pubkey {
        let key = Pubkey::new_unique();
        self.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; data_len],
                    owner: self.program_id,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    }

    fn backing_domain_ledger_account(&mut self) -> Pubkey {
        self.program_account(state::backing_domain_ledger_account_len())
    }

    fn insurance_ledger_account(&mut self) -> Pubkey {
        self.program_account(state::insurance_ledger_account_len())
    }

    fn set_token_account_amount(
        &mut self,
        token: Pubkey,
        mint: Pubkey,
        owner: Pubkey,
        amount: u64,
    ) {
        let mut account = self.svm.get_account(&token).expect("token account");
        account.data = make_token_data(mint, owner, amount);
        account.owner = spl_token::ID;
        self.svm.set_account(token, account).unwrap();
    }

    fn market_state(&self) -> (state::WrapperConfigV16, MarketGroupV16) {
        let account = self.svm.get_account(&self.market).expect("market account");
        state::read_market(&account.data).unwrap()
    }

    fn asset_market_id(&self, asset_index: u16) -> u64 {
        self.market_state().1.assets[asset_index as usize].market_id
    }

    fn portfolio_state(&self, portfolio: Pubkey) -> PortfolioAccountV16 {
        let account = self.svm.get_account(&portfolio).expect("portfolio account");
        state::read_portfolio(&account.data).unwrap()
    }

    fn portfolio_matcher_config(&self, portfolio: Pubkey) -> state::PortfolioMatcherConfigV16 {
        let account = self.svm.get_account(&portfolio).expect("portfolio account");
        state::read_portfolio_matcher_config(&account.data).unwrap()
    }

    fn portfolio_id(&self, portfolio: Pubkey) -> u64 {
        let account = self.svm.get_account(&portfolio).expect("portfolio account");
        state::read_portfolio_id(&account.data).unwrap()
    }

    fn portfolio_matcher_sequence(&self, portfolio: Pubkey) -> u64 {
        let account = self.svm.get_account(&portfolio).expect("portfolio account");
        state::read_portfolio_matcher_sequence(&account.data).unwrap()
    }

    fn control_sequences(&self, asset_index: usize) -> state::AssetControlSequencesV16 {
        let account = self.svm.get_account(&self.market).expect("market account");
        state::read_asset_control_sequences(&account.data, asset_index).unwrap()
    }

    fn backing_fee_policy(&self, domain: u16) -> (u16, u16) {
        let account = self.svm.get_account(&self.market).expect("market account");
        let profile = state::read_asset_oracle_profile(&account.data, domain as usize / 2)
            .expect("decode backing fee profile");
        if domain % 2 == 0 {
            (
                profile.backing_trade_fee_bps_long,
                profile.backing_trade_fee_insurance_share_bps_long,
            )
        } else {
            (
                profile.backing_trade_fee_bps_short,
                profile.backing_trade_fee_insurance_share_bps_short,
            )
        }
    }

    fn withdrawal_authority_epoch(
        &self,
        authority: Pubkey,
        asset_index: usize,
        insurance: bool,
    ) -> u64 {
        let account = self.svm.get_account(&self.market).expect("market account");
        let (_, group) = state::read_market(&account.data).expect("decode withdrawal market");
        let profile = state::read_asset_oracle_profile(&account.data, asset_index)
            .expect("decode withdrawal authority profile");
        let local_authority = if insurance {
            if group.mode == MarketModeV16::Live {
                profile.insurance_operator
            } else {
                profile.insurance_authority
            }
        } else {
            profile.backing_bucket_authority
        };
        let epoch_asset = if authority.to_bytes() == local_authority {
            asset_index
        } else {
            0
        };
        self.control_sequences(epoch_asset).authority_epoch
    }

    fn withdraw_insurance_asset_instruction(
        &self,
        authority: Pubkey,
        asset_index: u16,
        amount: u128,
    ) -> ProgInstruction {
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index,
            market_id: self.asset_market_id(asset_index),
            authority_epoch: self.withdrawal_authority_epoch(authority, asset_index as usize, true),
            amount,
        }
    }

    fn portfolio_position_epoch(&self, portfolio: Pubkey) -> u64 {
        let account = self.svm.get_account(&portfolio).expect("portfolio account");
        state::read_portfolio_position_epoch(&account.data).unwrap()
    }

    fn deposit_ix(&self, portfolio: Pubkey, amount: u128) -> ProgInstruction {
        ProgInstruction::Deposit {
            portfolio_id: self.portfolio_id(portfolio),
            expected_sequence: self.portfolio_matcher_sequence(portfolio),
            amount,
        }
    }

    fn withdraw_ix(&self, portfolio: Pubkey, amount: u128) -> ProgInstruction {
        ProgInstruction::Withdraw {
            portfolio_id: self.portfolio_id(portfolio),
            expected_sequence: self.portfolio_matcher_sequence(portfolio),
            amount,
        }
    }

    fn close_portfolio_ix(&self, portfolio: Pubkey) -> ProgInstruction {
        ProgInstruction::ClosePortfolio {
            portfolio_id: self.portfolio_id(portfolio),
            expected_sequence: self.portfolio_matcher_sequence(portfolio),
            position_epoch: self.portfolio_position_epoch(portfolio),
        }
    }

    fn convert_released_pnl_ix(&self, portfolio: Pubkey, amount: u128) -> ProgInstruction {
        ProgInstruction::ConvertReleasedPnl {
            portfolio_id: self.portfolio_id(portfolio),
            position_epoch: self.portfolio_position_epoch(portfolio),
            amount,
        }
    }

    fn trade_no_cpi_ix(
        &self,
        account_a: Pubkey,
        account_b: Pubkey,
        asset_index: u16,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> ProgInstruction {
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: self.portfolio_id(account_a),
            account_a_position_epoch: self.portfolio_position_epoch(account_a),
            account_b_portfolio_id: self.portfolio_id(account_b),
            account_b_position_epoch: self.portfolio_position_epoch(account_b),
            asset_index,
            market_id: self.asset_market_id(asset_index),
            size_q,
            exec_price,
            fee_bps,
            backing_fee_cap_bps: 0,
        }
    }

    fn trade_cpi_ix(
        &self,
        account_a: Pubkey,
        account_b: Pubkey,
        asset_index: u16,
        size_q: i128,
        fee_bps: u64,
        limit_price: u64,
    ) -> ProgInstruction {
        ProgInstruction::TradeCpi {
            account_a_portfolio_id: self.portfolio_id(account_a),
            account_a_position_epoch: self.portfolio_position_epoch(account_a),
            account_b_portfolio_id: self.portfolio_id(account_b),
            account_b_position_epoch: self.portfolio_position_epoch(account_b),
            account_b_matcher_sequence: self.portfolio_matcher_sequence(account_b),
            asset_index,
            market_id: self.asset_market_id(asset_index),
            size_q,
            fee_bps,
            limit_price,
            backing_fee_cap_bps: 0,
        }
    }

    fn batch_trade_no_cpi_ix(
        &self,
        account_a: Pubkey,
        account_b: Pubkey,
        legs: Vec<BatchTradeLeg>,
    ) -> ProgInstruction {
        ProgInstruction::BatchTradeNoCpi {
            account_a_portfolio_id: self.portfolio_id(account_a),
            account_a_position_epoch: self.portfolio_position_epoch(account_a),
            account_b_portfolio_id: self.portfolio_id(account_b),
            account_b_position_epoch: self.portfolio_position_epoch(account_b),
            legs,
        }
    }

    fn batch_trade_cpi_ix(
        &self,
        account_a: Pubkey,
        account_b: Pubkey,
        legs: Vec<BatchTradeCpiLeg>,
    ) -> ProgInstruction {
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: self.portfolio_id(account_a),
            account_a_position_epoch: self.portfolio_position_epoch(account_a),
            account_b_portfolio_id: self.portfolio_id(account_b),
            account_b_position_epoch: self.portfolio_position_epoch(account_b),
            account_b_matcher_sequence: self.portfolio_matcher_sequence(account_b),
            legs,
        }
    }

    fn mutate_market<F>(&mut self, f: F)
    where
        F: FnOnce(&mut state::WrapperConfigV16, &mut MarketGroupV16),
    {
        let mut account = self.svm.get_account(&self.market).expect("market account");
        let (mut cfg, mut group) = state::read_market(&account.data).unwrap();
        f(&mut cfg, &mut group);
        state::write_market(&mut account.data, &cfg, &group).unwrap();
        self.svm.set_account(self.market, account).unwrap();
    }

    fn mark_b_stale_gap(&mut self, portfolio: Pubkey, asset_index: usize, target_b: u128) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_account = self.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        let mut marked = false;
        for leg_wire in account.legs.iter_mut() {
            let mut leg = leg_wire.try_to_runtime().unwrap();
            if leg.active && leg.asset_index as usize == asset_index {
                assert!(
                    target_b > leg.b_snap,
                    "B-stale setup must put the market target ahead of the leg snapshot"
                );
                match leg.side {
                    SideV16::Long => group.assets[asset_index].b_long_num = target_b,
                    SideV16::Short => group.assets[asset_index].b_short_num = target_b,
                }
                leg.b_stale = true;
                *leg_wire = percolator::PortfolioLegV16Account::from_runtime(&leg);
                marked = true;
                break;
            }
        }
        assert!(
            marked,
            "portfolio must have an active leg for the B-stale asset"
        );
        if account.b_stale_state == 0 {
            group.b_stale_account_count = group.b_stale_account_count.saturating_add(1);
        }
        account.b_stale_state = 1;
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio, portfolio_account).unwrap();
    }

    fn add_source_positive_pnl(&mut self, portfolio: Pubkey, domain: usize, amount: u128) {
        // Use the engine's canonical, Live-gated, shape-validated grant API (over the same account
        // bytes) rather than a host-side re-implementation, so setups match how the engine actually
        // creates source-backed claims.
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_account = self.svm.get_account(&portfolio).expect("portfolio account");
        let max_slots = state::read_market_config_mode_and_capacity(&market_account.data)
            .unwrap()
            .2;
        {
            let (_cfg, mut group) = state::market_view_mut(&mut market_account.data).unwrap();
            let mut account =
                state::portfolio_view_mut_for_market_slots(&mut portfolio_account.data, max_slots)
                    .unwrap();
            group
                .add_account_source_positive_pnl_not_atomic(&mut account, domain, amount)
                .unwrap();
        }
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio, portfolio_account).unwrap();
    }

    fn seed_cancellable_close_progress(&mut self, portfolio: Pubkey) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_account = self.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        account.close_progress =
            percolator::CloseProgressLedgerV16Account::from_runtime(&CloseProgressLedgerV16 {
                active: true,
                finalized: false,
                canceled: false,
                close_id: 1,
                asset_index: 0,
                market_id: group.assets[0].market_id,
                domain_side: SideV16::Long,
                gross_loss_at_close_start: 10,
                drift_reference_slot: 0,
                max_close_slot: 10,
                residual_remaining: 10,
                ..CloseProgressLedgerV16::EMPTY
            });
        group.pending_domain_loss_barriers[0] = 1;
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio, portfolio_account).unwrap();
    }

    fn activate_permissionless_asset_with_fee(
        &mut self,
        creator: &Keypair,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        insurance_authority: Pubkey,
        insurance_operator: Pubkey,
        backing_bucket_authority: Pubkey,
        oracle_authority: Pubkey,
        fee: u128,
    ) -> (Pubkey, u64) {
        self.ensure_signer_account(creator.pubkey());
        let source = self.token_account(creator.pubkey(), fee as u64);
        let market_id = self.market_state().1.next_market_id;
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index,
                market_id,
                authority_epoch: 0,
                now_slot,
                initial_price,
                max_init_fee: fee,
                insurance_authority: insurance_authority.to_bytes(),
                insurance_operator: insurance_operator.to_bytes(),
                backing_bucket_authority: backing_bucket_authority.to_bytes(),
                oracle_authority: oracle_authority.to_bytes(),
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[creator],
        )
        .expect("permissionless asset activation with fee");
        (source, cu)
    }

    fn deposit_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        amount: u128,
    ) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, owner.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = self
            .send(
                self.deposit_ix(portfolio, amount),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(self.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(source, false),
                    AccountMeta::new(self.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .expect("deposit");
        (source, cu)
    }

    fn trade_with_cu(
        &mut self,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> u64 {
        self.trade_asset_with_cu(
            0, owner_a, account_a, owner_b, account_b, size_q, exec_price, fee_bps,
        )
    }

    fn trade_asset_with_cu(
        &mut self,
        asset_index: u16,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> u64 {
        self.try_trade_asset_with_cu(
            asset_index,
            owner_a,
            account_a,
            owner_b,
            account_b,
            size_q,
            exec_price,
            fee_bps,
        )
        .expect("trade")
    }

    #[allow(clippy::too_many_arguments)]
    fn try_trade_asset_with_cu(
        &mut self,
        asset_index: u16,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> Result<u64, String> {
        self.try_trade_asset_with_backing_fee_cap_with_cu(
            asset_index,
            owner_a,
            account_a,
            owner_b,
            account_b,
            size_q,
            exec_price,
            fee_bps,
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_trade_asset_with_backing_fee_cap_with_cu(
        &mut self,
        asset_index: u16,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
        backing_fee_cap_bps: u16,
    ) -> Result<u64, String> {
        let market_id = self
            .market_state()
            .1
            .assets
            .get(asset_index as usize)
            .map(|asset| asset.market_id)
            .unwrap_or(0);
        self.send(
            ProgInstruction::TradeNoCpi {
                account_a_portfolio_id: self.portfolio_id(account_a),
                account_a_position_epoch: self.portfolio_position_epoch(account_a),
                account_b_portfolio_id: self.portfolio_id(account_b),
                account_b_position_epoch: self.portfolio_position_epoch(account_b),
                asset_index,
                market_id,
                size_q,
                exec_price,
                fee_bps,
                backing_fee_cap_bps,
            },
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[owner_a, owner_b],
        )
    }

    fn update_maintenance_fee_policy_with_cu(&mut self, cranker_share_bps: u16) -> u64 {
        let sequences = self.control_sequences(0);
        let policy_sequence = next_control_sequence(sequences.maintenance_fee);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateMaintenanceFeePolicy {
                cranker_share_bps,
                policy_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update maintenance fee policy")
    }

    fn sync_maintenance_fee_with_cu(
        &mut self,
        portfolio: Pubkey,
        cranker_portfolio: Option<Pubkey>,
        now_slot: u64,
    ) -> u64 {
        self.try_sync_maintenance_fee_with_cu(portfolio, cranker_portfolio, now_slot)
            .expect("sync maintenance fee")
    }

    fn try_sync_maintenance_fee_with_cu(
        &mut self,
        portfolio: Pubkey,
        cranker_portfolio: Option<Pubkey>,
        now_slot: u64,
    ) -> Result<u64, String> {
        let mut accounts = vec![
            AccountMeta::new(self.market, false),
            AccountMeta::new(portfolio, false),
        ];
        if let Some(cranker_portfolio) = cranker_portfolio {
            accounts.push(AccountMeta::new(cranker_portfolio, false));
        }
        self.send(
            ProgInstruction::SyncMaintenanceFee { now_slot },
            accounts,
            &[],
        )
    }

    fn seed_n_leg_position_for_benchmark(
        &mut self,
        long_account: Pubkey,
        short_account: Pubkey,
        n: usize,
    ) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut long_data = self.svm.get_account(&long_account).expect("long account");
        let mut short_data = self.svm.get_account(&short_account).expect("short account");
        let (_, _, max_market_slots, _) =
            state::read_market_config_mode_and_capacity(&market_account.data).unwrap();
        {
            let (_, mut group) = state::market_view_mut(&mut market_account.data).unwrap();
            let mut long =
                state::portfolio_view_mut_for_market_slots(&mut long_data.data, max_market_slots)
                    .unwrap();
            let mut short =
                state::portfolio_view_mut_for_market_slots(&mut short_data.data, max_market_slots)
                    .unwrap();
            for asset_index in 0..n {
                group
                    .execute_trade_with_fee_loss_stale_scoped_not_atomic(
                        &mut long,
                        &mut short,
                        TradeRequestV16 {
                            asset_index,
                            size_q: (10 * POS_SCALE) as i128,
                            exec_price: 100,
                            fee_bps: 0,
                        },
                    )
                    .unwrap();
            }
            for asset_index in 0..n {
                group
                    .accrue_asset_to_not_atomic(asset_index, 16, 95, 0, true)
                    .unwrap();
                group.markets[asset_index]
                    .engine
                    .asset
                    .raw_oracle_target_price = percolator::V16PodU64::new(95);
            }
        }
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(long_account, long_data).unwrap();
        self.svm.set_account(short_account, short_data).unwrap();
    }

    fn seed_current_n_leg_position_for_benchmark(
        &mut self,
        long_account: Pubkey,
        short_account: Pubkey,
        n: usize,
    ) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut long_data = self.svm.get_account(&long_account).expect("long account");
        let mut short_data = self.svm.get_account(&short_account).expect("short account");
        let (_, _, max_market_slots, _) =
            state::read_market_config_mode_and_capacity(&market_account.data).unwrap();
        {
            let (_, mut group) = state::market_view_mut(&mut market_account.data).unwrap();
            let mut long =
                state::portfolio_view_mut_for_market_slots(&mut long_data.data, max_market_slots)
                    .unwrap();
            let mut short =
                state::portfolio_view_mut_for_market_slots(&mut short_data.data, max_market_slots)
                    .unwrap();
            for asset_index in 0..n {
                group
                    .execute_trade_with_fee_loss_stale_scoped_not_atomic(
                        &mut long,
                        &mut short,
                        TradeRequestV16 {
                            asset_index,
                            size_q: (10 * POS_SCALE) as i128,
                            exec_price: 100,
                            fee_bps: 0,
                        },
                    )
                    .unwrap();
            }
        }
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(long_account, long_data).unwrap();
        self.svm.set_account(short_account, short_data).unwrap();
    }

    fn force_portfolio_capital_for_benchmark(&mut self, portfolio_key: Pubkey, new_capital: u128) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_data = self
            .svm
            .get_account(&portfolio_key)
            .expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut portfolio = state::read_portfolio(&portfolio_data.data).unwrap();
        let old_capital = portfolio.capital.get();
        if new_capital < old_capital {
            let delta = old_capital - new_capital;
            group.c_tot -= delta;
            group.vault -= delta;
        } else {
            let delta = new_capital - old_capital;
            group.c_tot += delta;
            group.vault += delta;
        }
        portfolio.capital = percolator::V16PodU128::new(new_capital);
        portfolio.health_cert.valid = 0;
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_data.data, &portfolio).unwrap();
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio_key, portfolio_data).unwrap();
    }

    fn force_portfolio_loss_for_security_test(&mut self, portfolio_key: Pubkey, loss: u128) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_data = self
            .svm
            .get_account(&portfolio_key)
            .expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut portfolio = state::read_portfolio(&portfolio_data.data).unwrap();
        assert!(
            portfolio.capital.get() > loss,
            "loss must remain fully covered by capital"
        );
        assert_eq!(
            portfolio.pnl.get(),
            0,
            "security seed expects neutral starting pnl"
        );
        let loss_i128 = i128::try_from(loss).unwrap();
        portfolio.pnl = percolator::V16PodI128::new(-loss_i128);
        group.negative_pnl_account_count += 1;
        portfolio.health_cert.valid = 0;
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_data.data, &portfolio).unwrap();
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio_key, portfolio_data).unwrap();
    }

    fn set_residual_reward_counters_for_test(
        &mut self,
        portfolio_key: Pubkey,
        crystallized_loss_atoms: u128,
        spent_principal_atoms: u128,
        received_atoms: u128,
    ) {
        let mut portfolio_data = self
            .svm
            .get_account(&portfolio_key)
            .expect("portfolio account");
        let mut portfolio = state::read_portfolio(&portfolio_data.data).unwrap();
        portfolio.residual_crystallized_loss_atoms_total =
            percolator::V16PodU128::new(crystallized_loss_atoms);
        portfolio.residual_spent_principal_atoms_total =
            percolator::V16PodU128::new(spent_principal_atoms);
        portfolio.residual_received_atoms_total = percolator::V16PodU128::new(received_atoms);
        state::write_portfolio(&mut portfolio_data.data, &portfolio).unwrap();
        self.svm.set_account(portfolio_key, portfolio_data).unwrap();
    }

    fn init_matcher_context(
        &mut self,
        matcher_program: Pubkey,
        maker_account: Pubkey,
    ) -> (Pubkey, Pubkey, u64) {
        self.init_matcher_context_with_data(
            matcher_program,
            maker_account,
            encode_matcher_init_passive(u128::MAX),
        )
    }

    fn init_matcher_context_with_passive_spread(
        &mut self,
        matcher_program: Pubkey,
        maker_account: Pubkey,
        base_spread_bps: u32,
        max_total_bps: u32,
    ) -> (Pubkey, Pubkey, u64) {
        self.init_matcher_context_with_data(
            matcher_program,
            maker_account,
            encode_matcher_init_passive_with_spread(u128::MAX, base_spread_bps, max_total_bps),
        )
    }

    fn init_matcher_context_with_data(
        &mut self,
        matcher_program: Pubkey,
        maker_account: Pubkey,
        init_data: Vec<u8>,
    ) -> (Pubkey, Pubkey, u64) {
        let ctx = Pubkey::new_unique();
        let (_, maker_owner) = state::read_portfolio_owner_preflight(
            &self
                .svm
                .get_account(&maker_account)
                .expect("maker portfolio account")
                .data,
        )
        .expect("maker portfolio owner");
        let maker_owner = Pubkey::new_from_array(maker_owner);
        let delegate = matcher_delegate_key(
            &self.program_id,
            &self.market,
            &maker_account,
            &maker_owner,
            &matcher_program,
            &ctx,
        );
        self.svm
            .set_account(
                delegate,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![],
                    owner: Pubkey::default(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        self.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; MATCHER_CONTEXT_LEN],
                    owner: matcher_program,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_raw_tx(
            &mut self.svm,
            &self.payer,
            Instruction {
                program_id: matcher_program,
                accounts: vec![
                    AccountMeta::new_readonly(delegate, false),
                    AccountMeta::new(ctx, false),
                ],
                data: init_data,
            },
            &[],
        )
        .expect("init matcher context");
        (ctx, delegate, cu)
    }

    fn init_matcher_context_authorized(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
    ) -> (Pubkey, Pubkey, u64) {
        let (ctx, delegate, cu) = self.init_matcher_context(matcher_program, maker_account);
        self.set_matcher_config(
            matcher_program,
            maker_owner,
            maker_account,
            ctx,
            delegate,
            1,
        );
        (ctx, delegate, cu)
    }

    fn init_matcher_context_with_passive_spread_authorized(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
        base_spread_bps: u32,
        max_total_bps: u32,
    ) -> (Pubkey, Pubkey, u64) {
        let (ctx, delegate, cu) = self.init_matcher_context_with_passive_spread(
            matcher_program,
            maker_account,
            base_spread_bps,
            max_total_bps,
        );
        self.set_matcher_config(
            matcher_program,
            maker_owner,
            maker_account,
            ctx,
            delegate,
            1,
        );
        (ctx, delegate, cu)
    }

    fn init_matcher_context_with_data_authorized(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
        init_data: Vec<u8>,
    ) -> (Pubkey, Pubkey, u64) {
        let (ctx, delegate, cu) =
            self.init_matcher_context_with_data(matcher_program, maker_account, init_data);
        self.set_matcher_config(
            matcher_program,
            maker_owner,
            maker_account,
            ctx,
            delegate,
            1,
        );
        (ctx, delegate, cu)
    }

    fn init_auth_matcher_context(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
    ) -> (Pubkey, Pubkey, u64) {
        self.init_auth_matcher_context_with_trade_fee_cap(
            matcher_program,
            maker_owner,
            maker_account,
            10_000,
        )
    }

    fn init_auth_matcher_context_with_trade_fee_cap(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
        trade_fee_cap_bps: u16,
    ) -> (Pubkey, Pubkey, u64) {
        let ctx = Pubkey::new_unique();
        let delegate = matcher_delegate_key(
            &self.program_id,
            &self.market,
            &maker_account,
            &maker_owner.pubkey(),
            &matcher_program,
            &ctx,
        );
        let cu = self
            .try_init_auth_matcher_context_with_delegate(
                matcher_program,
                maker_owner,
                maker_account,
                ctx,
                delegate,
            )
            .expect("init auth matcher context");
        self.set_matcher_config_with_trade_fee_cap(
            matcher_program,
            maker_owner,
            maker_account,
            ctx,
            delegate,
            1,
            trade_fee_cap_bps,
        );
        (ctx, delegate, cu)
    }

    fn init_auth_matcher_context_via_system_create(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
    ) -> (Pubkey, Pubkey, u64) {
        self.ensure_signer_account(maker_owner.pubkey());
        let ctx = Keypair::new();
        let create_cu = send_raw_tx(
            &mut self.svm,
            &self.payer,
            system_instruction::create_account(
                &self.payer.pubkey(),
                &ctx.pubkey(),
                1_000_000_000,
                MATCHER_CONTEXT_LEN as u64,
                &matcher_program,
            ),
            &[&ctx],
        )
        .expect("system-create matcher context");
        let delegate = matcher_delegate_key(
            &self.program_id,
            &self.market,
            &maker_account,
            &maker_owner.pubkey(),
            &matcher_program,
            &ctx.pubkey(),
        );
        let init_cu = send_raw_tx(
            &mut self.svm,
            &self.payer,
            Instruction {
                program_id: matcher_program,
                accounts: vec![
                    AccountMeta::new_readonly(maker_owner.pubkey(), true),
                    AccountMeta::new_readonly(delegate, false),
                    AccountMeta::new(ctx.pubkey(), false),
                    AccountMeta::new_readonly(self.program_id, false),
                    AccountMeta::new_readonly(self.market, false),
                    AccountMeta::new_readonly(maker_account, false),
                ],
                data: vec![2],
            },
            &[maker_owner],
        )
        .expect("auth matcher init after system create");
        self.set_matcher_config(
            matcher_program,
            maker_owner,
            maker_account,
            ctx.pubkey(),
            delegate,
            1,
        );
        (ctx.pubkey(), delegate, create_cu + init_cu)
    }

    fn set_matcher_config(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
        matcher_context: Pubkey,
        matcher_delegate: Pubkey,
        enabled: u8,
    ) -> Pubkey {
        self.set_matcher_config_with_trade_fee_cap(
            matcher_program,
            maker_owner,
            maker_account,
            matcher_context,
            matcher_delegate,
            enabled,
            if enabled == 0 { 0 } else { 10_000 },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn set_matcher_config_with_trade_fee_cap(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
        matcher_context: Pubkey,
        matcher_delegate: Pubkey,
        enabled: u8,
        trade_fee_cap_bps: u16,
    ) -> Pubkey {
        self.try_set_matcher_config_with_trade_fee_cap(
            matcher_program,
            maker_owner,
            maker_account,
            matcher_context,
            matcher_delegate,
            enabled,
            trade_fee_cap_bps,
        )
        .expect("set matcher config")
    }

    #[allow(clippy::too_many_arguments)]
    fn try_set_matcher_config_with_trade_fee_cap(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
        matcher_context: Pubkey,
        matcher_delegate: Pubkey,
        enabled: u8,
        trade_fee_cap_bps: u16,
    ) -> Result<Pubkey, String> {
        let portfolio_id = self.portfolio_id(maker_account);
        let expected_sequence = self.portfolio_matcher_sequence(maker_account);
        self.svm.expire_blockhash();
        let mut accounts = vec![
            AccountMeta::new(maker_owner.pubkey(), true),
            AccountMeta::new_readonly(self.market, false),
            AccountMeta::new(maker_account, false),
        ];
        if enabled != 0 {
            accounts.extend([
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new_readonly(matcher_context, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ]);
        }
        self.send(
            ProgInstruction::SetMatcherConfig {
                portfolio_id,
                expected_sequence,
                enabled,
                trade_fee_cap_bps,
            },
            accounts,
            &[maker_owner],
        )?;
        Ok(matcher_delegate)
    }

    fn try_init_auth_matcher_context_with_delegate(
        &mut self,
        matcher_program: Pubkey,
        maker_owner: &Keypair,
        maker_account: Pubkey,
        ctx: Pubkey,
        delegate: Pubkey,
    ) -> Result<u64, String> {
        self.ensure_signer_account(maker_owner.pubkey());
        self.svm
            .set_account(
                delegate,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![],
                    owner: Pubkey::default(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        self.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; MATCHER_CONTEXT_LEN],
                    owner: matcher_program,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        send_raw_tx(
            &mut self.svm,
            &self.payer,
            Instruction {
                program_id: matcher_program,
                accounts: vec![
                    AccountMeta::new_readonly(maker_owner.pubkey(), true),
                    AccountMeta::new_readonly(delegate, false),
                    AccountMeta::new(ctx, false),
                    AccountMeta::new_readonly(self.program_id, false),
                    AccountMeta::new_readonly(self.market, false),
                    AccountMeta::new_readonly(maker_account, false),
                ],
                data: vec![2],
            },
            &[maker_owner],
        )
    }

    fn trade_cpi_with_cu(
        &mut self,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        matcher_program: Pubkey,
        matcher_context: Pubkey,
        matcher_delegate: Pubkey,
        size_q: i128,
        fee_bps: u64,
    ) -> u64 {
        self.trade_cpi_with_cu_on_asset(
            owner_a,
            account_a,
            owner_b,
            account_b,
            matcher_program,
            matcher_context,
            matcher_delegate,
            0,
            size_q,
            fee_bps,
        )
    }

    fn trade_cpi_with_cu_on_asset(
        &mut self,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        matcher_program: Pubkey,
        matcher_context: Pubkey,
        matcher_delegate: Pubkey,
        asset_index: u16,
        size_q: i128,
        fee_bps: u64,
    ) -> u64 {
        self.try_trade_cpi_with_cu_on_asset(
            owner_a,
            account_a,
            owner_b,
            account_b,
            matcher_program,
            matcher_context,
            matcher_delegate,
            asset_index,
            size_q,
            fee_bps,
        )
        .expect("trade cpi")
    }

    #[allow(clippy::too_many_arguments)]
    fn try_trade_cpi_with_cu_on_asset(
        &mut self,
        owner_a: &Keypair,
        account_a: Pubkey,
        _owner_b: &Keypair,
        account_b: Pubkey,
        matcher_program: Pubkey,
        matcher_context: Pubkey,
        matcher_delegate: Pubkey,
        asset_index: u16,
        size_q: i128,
        fee_bps: u64,
    ) -> Result<u64, String> {
        let market_id = self.asset_market_id(asset_index);
        let metas = vec![
            AccountMeta::new(owner_a.pubkey(), true),
            AccountMeta::new(self.market, false),
            AccountMeta::new(account_a, false),
            AccountMeta::new(account_b, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(matcher_context, false),
            AccountMeta::new_readonly(matcher_delegate, false),
        ];
        self.send(
            ProgInstruction::TradeCpi {
                account_a_portfolio_id: self.portfolio_id(account_a),
                account_a_position_epoch: self.portfolio_position_epoch(account_a),
                account_b_portfolio_id: self.portfolio_id(account_b),
                account_b_position_epoch: self.portfolio_position_epoch(account_b),
                account_b_matcher_sequence: self.portfolio_matcher_sequence(account_b),
                asset_index,
                market_id,
                size_q,
                fee_bps,
                limit_price: 0,
                backing_fee_cap_bps: 0,
            },
            metas,
            &[owner_a],
        )
    }

    fn withdraw(&mut self, owner: &Keypair, portfolio: Pubkey, amount: u128) -> Pubkey {
        self.withdraw_with_cu(owner, portfolio, amount).0
    }

    fn close_portfolio_with_cu(&mut self, owner: &Keypair, portfolio: Pubkey) -> u64 {
        self.send(
            self.close_portfolio_ix(portfolio),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("close portfolio")
    }

    fn withdraw_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        amount: u128,
    ) -> (Pubkey, u64) {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, owner.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = self
            .send(
                self.withdraw_ix(portfolio, amount),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(self.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(dest, false),
                    AccountMeta::new(self.vault, false),
                    AccountMeta::new_readonly(self.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .expect("withdraw");
        (dest, cu)
    }

    fn resolve(&mut self) -> u64 {
        let now_slot = self.svm.get_sysvar::<Clock>().slot;
        let market_data = self.svm.get_account(&self.market).unwrap().data;
        let (_, group) = state::read_market(&market_data).unwrap();
        let max_attempts = group
            .assets
            .iter()
            .filter(|asset| asset.oi_eff_long_q != 0 || asset.oi_eff_short_q != 0)
            .map(|asset| {
                let max_dt = group.config.max_accrual_dt_slots.max(1);
                let elapsed = now_slot.saturating_sub(asset.slot_last);
                elapsed / max_dt + u64::from(elapsed % max_dt != 0) + 2
            })
            .sum::<u64>()
            .saturating_add(1)
            .min(16_384) as usize;
        let mut max_cu = 0;
        for _ in 0..max_attempts.max(1) {
            self.svm.expire_blockhash();
            let authority_epoch = self.control_sequences(0).authority_epoch;
            let result = send_tx(
                &mut self.svm,
                self.program_id,
                &self.payer,
                ProgInstruction::ResolveMarket {
                    asset_generation_frontier: 0,
                    authority_epoch,
                },
                vec![
                    AccountMeta::new(self.admin.pubkey(), true),
                    AccountMeta::new(self.market, false),
                ],
                &[&self.admin],
            );
            match result {
                Ok(cu) => return max_cu.max(cu),
                Err(err) if is_engine_stale_error(&err) => {
                    max_cu = max_cu.max(
                        self.public_terminal_accrual_any_exposed(now_slot)
                            .unwrap_or_else(|crank_err| {
                                panic!(
                                    "public accrual before market resolve failed: {crank_err}; resolve error: {err}"
                                )
                            }),
                    );
                }
                Err(err) => panic!("resolve market: {err}"),
            }
        }
        panic!("market resolve remained stale after {max_attempts} bounded public accrual attempts")
    }

    fn close_slab_with_cu(&mut self) -> u64 {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let authority_epoch = self.control_sequences(0).authority_epoch;
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::CloseSlab { authority_epoch },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new(dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(self.mint, false),
            ],
            &[&self.admin],
        )
        .expect("close slab")
    }

    fn configure_permissionless_resolve_with_cu(
        &mut self,
        stale_slots: u64,
        force_close_delay_slots: u64,
    ) -> u64 {
        let sequences = self.control_sequences(0);
        let policy_sequence = next_control_sequence(sequences.permissionless_resolve);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigurePermissionlessResolve {
                asset_generation_frontier: 0,
                stale_slots,
                force_close_delay_slots,
                policy_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("configure permissionless resolve")
    }

    fn enable_live_insurance_withdrawal(&mut self) {
        // Live insurance withdrawal is uniformly asset-scoped now; no global
        // asset-0-only policy must be enabled.
    }

    fn set_pyth_price(
        &mut self,
        feed: &[u8; 32],
        price: i64,
        expo: i32,
        publish_time: i64,
    ) -> Pubkey {
        self.set_pyth_price_with_conf(feed, price, expo, 1, publish_time)
    }

    fn set_pyth_price_with_conf(
        &mut self,
        feed: &[u8; 32],
        price: i64,
        expo: i32,
        conf: u64,
        publish_time: i64,
    ) -> Pubkey {
        let key = Pubkey::new_unique();
        self.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: make_pyth_data(feed, price, expo, conf, publish_time),
                    owner: oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    }

    // Set a Switchboard On-Demand feed account (owned by the Switchboard program). The leg binds by
    // ACCOUNT KEY (read_switchboard_price_e6 checks price_ai.key == expected_feed_key), so the returned
    // pubkey is what goes into oracle_leg_feeds.
    fn set_switchboard_price(&mut self, value: i128, std_dev: i128, publish_time: i64) -> Pubkey {
        let key = Pubkey::new_unique();
        self.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: make_switchboard_data(
                        &[0xABu8; 32],
                        value,
                        std_dev,
                        publish_time,
                        3,
                        1,
                        1,
                    ),
                    owner: oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    }

    fn configure_three_leg_hybrid_with_cu(
        &mut self,
        feeds: [[u8; 32]; 3],
        leg0: Pubkey,
        leg1: Pubkey,
        leg2: Pubkey,
        now_slot: u64,
        now_unix_ts: i64,
    ) -> u64 {
        self.try_configure_three_leg_hybrid(feeds, leg0, leg1, leg2, now_slot, now_unix_ts)
            .expect("configure hybrid oracle")
    }

    fn try_configure_three_leg_hybrid(
        &mut self,
        feeds: [[u8; 32]; 3],
        leg0: Pubkey,
        leg1: Pubkey,
        leg2: Pubkey,
        now_slot: u64,
        now_unix_ts: i64,
    ) -> Result<u64, String> {
        let sequences = self.control_sequences(0);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureHybridOracle {
                market_id: 0,
                asset_index: 0,
                now_slot,
                now_unix_ts,
                oracle_leg_count: 3,
                oracle_leg_flags: ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
                max_staleness_secs: 60,
                hybrid_soft_stale_slots: 3,
                mark_ewma_halflife_slots: 1,
                mark_min_fee: 0,
                invert: 0,
                unit_scale: 0,
                conf_filter_bps: 500,
                oracle_leg_feeds: feeds,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new_readonly(leg0, false),
                AccountMeta::new_readonly(leg1, false),
                AccountMeta::new_readonly(leg2, false),
            ],
            &[&self.admin],
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_configure_hybrid_with_cu(
        &mut self,
        oracle_leg_count: u8,
        oracle_leg_flags: u8,
        feeds: [[u8; 32]; 3],
        oracle_accounts: &[Pubkey],
        now_slot: u64,
        now_unix_ts: i64,
        invert: u8,
        unit_scale: u32,
        hybrid_soft_stale_slots: u64,
    ) -> Result<u64, String> {
        self.try_configure_hybrid_asset_with_cu(
            0,
            oracle_leg_count,
            oracle_leg_flags,
            feeds,
            oracle_accounts,
            now_slot,
            now_unix_ts,
            invert,
            unit_scale,
            hybrid_soft_stale_slots,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_configure_hybrid_asset_with_cu(
        &mut self,
        asset_index: u16,
        oracle_leg_count: u8,
        oracle_leg_flags: u8,
        feeds: [[u8; 32]; 3],
        oracle_accounts: &[Pubkey],
        now_slot: u64,
        now_unix_ts: i64,
        invert: u8,
        unit_scale: u32,
        hybrid_soft_stale_slots: u64,
    ) -> Result<u64, String> {
        self.try_configure_hybrid_asset_with_conf_filter_cu(
            asset_index,
            oracle_leg_count,
            oracle_leg_flags,
            feeds,
            oracle_accounts,
            now_slot,
            now_unix_ts,
            invert,
            unit_scale,
            hybrid_soft_stale_slots,
            500,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_configure_hybrid_asset_with_conf_filter_cu(
        &mut self,
        asset_index: u16,
        oracle_leg_count: u8,
        oracle_leg_flags: u8,
        feeds: [[u8; 32]; 3],
        oracle_accounts: &[Pubkey],
        now_slot: u64,
        now_unix_ts: i64,
        invert: u8,
        unit_scale: u32,
        hybrid_soft_stale_slots: u64,
        conf_filter_bps: u16,
    ) -> Result<u64, String> {
        let sequences = self.control_sequences(asset_index as usize);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        let mut accounts = vec![
            AccountMeta::new(self.admin.pubkey(), true),
            AccountMeta::new(self.market, false),
        ];
        accounts.extend(
            oracle_accounts
                .iter()
                .take(oracle_leg_count as usize)
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureHybridOracle {
                market_id: 0,
                asset_index,
                now_slot,
                now_unix_ts,
                oracle_leg_count,
                oracle_leg_flags,
                max_staleness_secs: 60,
                hybrid_soft_stale_slots,
                mark_ewma_halflife_slots: 1,
                mark_min_fee: 0,
                invert,
                unit_scale,
                conf_filter_bps,
                oracle_leg_feeds: feeds,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            accounts,
            &[&self.admin],
        )
    }

    fn configure_ewma_mark_with_cu(
        &mut self,
        now_slot: u64,
        initial_mark_e6: u64,
        halflife_slots: u64,
        mark_min_fee: u64,
    ) -> u64 {
        let sequences = self.control_sequences(0);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureEwmaMark {
                market_id: 0,
                asset_index: 0,
                now_slot,
                initial_mark_e6,
                mark_ewma_halflife_slots: halflife_slots,
                mark_min_fee,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("configure ewma_mark mark")
    }

    fn push_ewma_mark_with_cu(&mut self, now_slot: u64, mark_e6: u64) -> u64 {
        let sequences = self.control_sequences(0);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::PushEwmaMark {
                market_id: 0,
                asset_index: 0,
                now_slot,
                mark_e6,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("push ewma_mark mark")
    }

    fn configure_auth_mark_with_cu(&mut self, now_slot: u64, initial_mark_e6: u64) -> u64 {
        let sequences = self.control_sequences(0);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureAuthMark {
                market_id: 0,
                asset_index: 0,
                now_slot,
                initial_mark_e6,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("configure auth mark")
    }

    fn push_auth_mark_with_cu(&mut self, now_slot: u64, mark_e6: u64) -> u64 {
        let sequences = self.control_sequences(0);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::PushAuthMark {
                market_id: 0,
                asset_index: 0,
                now_slot,
                mark_e6,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("push auth mark")
    }

    fn configure_auth_mark_for_asset_as_admin(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        initial_mark_e6: u64,
    ) -> u64 {
        let sequences = self.control_sequences(asset_index as usize);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureAuthMark {
                market_id: 0,
                asset_index,
                now_slot,
                initial_mark_e6,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("configure auth mark for asset as admin")
    }

    fn push_auth_mark_for_asset_as_admin(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        mark_e6: u64,
    ) -> u64 {
        let sequences = self.control_sequences(asset_index as usize);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::PushAuthMark {
                market_id: 0,
                asset_index,
                now_slot,
                mark_e6,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("push auth mark for asset as admin")
    }

    fn configure_auth_mark_for_asset_with_authority(
        &mut self,
        asset_index: u16,
        authority: &Keypair,
        now_slot: u64,
        initial_mark_e6: u64,
    ) -> u64 {
        self.ensure_signer_account(authority.pubkey());
        let sequences = self.control_sequences(asset_index as usize);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureAuthMark {
                market_id: 0,
                asset_index,
                now_slot,
                initial_mark_e6,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
        .expect("configure auth mark for asset")
    }

    fn push_auth_mark_for_asset_with_authority(
        &mut self,
        asset_index: u16,
        authority: &Keypair,
        now_slot: u64,
        mark_e6: u64,
    ) -> u64 {
        self.ensure_signer_account(authority.pubkey());
        let sequences = self.control_sequences(asset_index as usize);
        let observation_sequence = next_control_sequence(sequences.oracle_observation);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::PushAuthMark {
                market_id: 0,
                asset_index,
                now_slot,
                mark_e6,
                observation_sequence,
                authority_epoch: sequences.authority_epoch,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
        .expect("push auth mark for asset")
    }

    fn resolve_stale_permissionless_with_cu(&mut self, now_slot: u64) -> u64 {
        self.svm.warp_to_slot(now_slot);
        let market_data = self.svm.get_account(&self.market).unwrap().data;
        let (_, group) = state::read_market(&market_data).unwrap();
        let max_dt = group.config.max_accrual_dt_slots.max(1);
        let max_attempts = group
            .assets
            .iter()
            .filter(|asset| asset.oi_eff_long_q != 0 || asset.oi_eff_short_q != 0)
            .map(|asset| {
                let elapsed = now_slot.saturating_sub(asset.slot_last);
                elapsed / max_dt + u64::from(elapsed % max_dt != 0) + 2
            })
            .sum::<u64>()
            .saturating_add(1)
            .min(16_384) as usize;
        let mut max_cu = 0;
        for _ in 0..max_attempts.max(1) {
            self.svm.expire_blockhash();
            match send_tx(
                &mut self.svm,
                self.program_id,
                &self.payer,
                ProgInstruction::ResolveStalePermissionless { now_slot },
                vec![AccountMeta::new(self.market, false)],
                &[],
            ) {
                Ok(cu) => return max_cu.max(cu),
                Err(err) if is_engine_stale_error(&err) => {
                    max_cu = max_cu.max(
                        self.public_terminal_accrual_any_exposed(now_slot)
                            .unwrap_or_else(|crank_err| {
                                panic!(
                                    "public accrual before permissionless resolve failed: {crank_err}; resolve error: {err}"
                                )
                            }),
                    );
                }
                Err(err) => panic!("resolve stale permissionless: {err}"),
            }
        }
        panic!(
            "permissionless market resolve remained stale after {max_attempts} bounded public accrual attempts"
        )
    }

    fn close_resolved(&mut self, owner: &Keypair, portfolio: Pubkey) -> Pubkey {
        self.close_resolved_with_cu(owner, portfolio).0
    }

    fn close_resolved_with_cu(&mut self, owner: &Keypair, portfolio: Pubkey) -> (Pubkey, u64) {
        let (dest, result) = self.try_close_resolved_with_cu(owner, portfolio);
        (dest, result.expect("close resolved"))
    }

    fn try_close_resolved_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
    ) -> (Pubkey, Result<u64, String>) {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, owner.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let result = self.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        );
        (dest, result)
    }

    fn top_up_insurance(&mut self, amount: u128) -> Pubkey {
        self.top_up_insurance_with_cu(amount).0
    }

    fn top_up_insurance_domain_with_authority(
        &mut self,
        authority: &Keypair,
        domain: u16,
        amount: u128,
    ) -> Pubkey {
        self.top_up_insurance_domain_with_authority_and_cu(authority, domain, amount)
            .0
    }

    fn top_up_backing_bucket(&mut self, domain: u16, amount: u128, expiry_slot: u64) -> Pubkey {
        self.top_up_backing_bucket_with_cu(domain, amount, expiry_slot)
            .0
    }

    fn top_up_insurance_from_admin_token_with_cu(&mut self, source: Pubkey, amount: u128) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpInsurance {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                amount,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("top up insurance from admin token")
    }

    fn top_up_backing_bucket_from_admin_token_with_cu(
        &mut self,
        source: Pubkey,
        domain: u16,
        amount: u128,
        expiry_slot: u64,
    ) -> u64 {
        let (backing_fee_bps, insurance_share_bps) = self.backing_fee_policy(domain);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain,
                backing_fee_bps,
                insurance_share_bps,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("top up backing bucket from admin token")
    }

    fn top_up_insurance_with_cu(&mut self, amount: u128) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpInsurance {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                amount,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("top up insurance");
        (source, cu)
    }

    fn top_up_insurance_with_ledger_with_cu(
        &mut self,
        ledger: Pubkey,
        amount: u128,
    ) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpInsurance {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                amount,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&self.admin],
        )
        .expect("top up insurance with ledger");
        (source, cu)
    }

    fn top_up_insurance_domain_with_authority_and_cu(
        &mut self,
        authority: &Keypair,
        domain: u16,
        amount: u128,
    ) -> (Pubkey, u64) {
        self.ensure_signer_account(authority.pubkey());
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, authority.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpInsuranceDomain {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
        .expect("top up domain insurance");
        (source, cu)
    }

    fn top_up_insurance_domain_with_authority_ledger_and_cu(
        &mut self,
        authority: &Keypair,
        ledger: Pubkey,
        domain: u16,
        amount: u128,
    ) -> (Pubkey, u64) {
        self.ensure_signer_account(authority.pubkey());
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, authority.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpInsuranceDomain {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[authority],
        )
        .expect("top up domain insurance with ledger");
        (source, cu)
    }

    fn top_up_backing_bucket_with_cu(
        &mut self,
        domain: u16,
        amount: u128,
        expiry_slot: u64,
    ) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let (backing_fee_bps, insurance_share_bps) = self.backing_fee_policy(domain);
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain,
                backing_fee_bps,
                insurance_share_bps,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("top up backing bucket");
        (source, cu)
    }

    fn top_up_backing_bucket_with_ledger_with_cu(
        &mut self,
        ledger: Pubkey,
        domain: u16,
        amount: u128,
        expiry_slot: u64,
    ) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let (backing_fee_bps, insurance_share_bps) = self.backing_fee_policy(domain);
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain,
                backing_fee_bps,
                insurance_share_bps,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&self.admin],
        )
        .expect("top up backing bucket with ledger");
        (source, cu)
    }

    fn top_up_backing_bucket_with_authority(
        &mut self,
        authority: &Keypair,
        domain: u16,
        amount: u128,
        expiry_slot: u64,
    ) -> Pubkey {
        self.ensure_signer_account(authority.pubkey());
        let source = self.token_account(authority.pubkey(), amount as u64);
        let (backing_fee_bps, insurance_share_bps) = self.backing_fee_policy(domain);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain,
                backing_fee_bps,
                insurance_share_bps,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
        .expect("top up backing bucket with authority");
        source
    }

    fn withdraw_insurance_with_cu(&mut self, amount: u128) -> (Pubkey, u64) {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let market_id = self.asset_market_id(0);
        let authority_epoch = self.withdrawal_authority_epoch(self.admin.pubkey(), 0, true);
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawInsuranceAsset {
                market_id,
                authority_epoch,
                asset_index: 0,
                amount,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("withdraw insurance");
        (dest, cu)
    }

    fn withdraw_insurance_domain_to_admin_token_with_cu(
        &mut self,
        dest: Pubkey,
        domain: u16,
        amount: u128,
    ) -> u64 {
        let asset_index = (domain / 2) as u16;
        let market_id = self.asset_market_id(asset_index);
        let authority_epoch =
            self.withdrawal_authority_epoch(self.admin.pubkey(), asset_index as usize, true);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawInsuranceAsset {
                market_id,
                authority_epoch,
                asset_index,
                amount,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("withdraw domain insurance to admin token")
    }

    fn withdraw_backing_bucket_to_admin_token_with_cu(
        &mut self,
        dest: Pubkey,
        domain: u16,
        amount: u128,
    ) -> u64 {
        let market_id = self.asset_market_id(domain / 2);
        let authority_epoch =
            self.withdrawal_authority_epoch(self.admin.pubkey(), domain as usize / 2, false);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawBackingBucket {
                domain,
                market_id,
                authority_epoch,
                amount,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("withdraw backing bucket to admin token")
    }

    fn withdraw_backing_bucket_with_ledger_to_admin_token_with_cu(
        &mut self,
        ledger: Pubkey,
        dest: Pubkey,
        domain: u16,
        amount: u128,
    ) -> u64 {
        let market_id = self.asset_market_id(domain / 2);
        let authority_epoch =
            self.withdrawal_authority_epoch(self.admin.pubkey(), domain as usize / 2, false);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawBackingBucket {
                domain,
                market_id,
                authority_epoch,
                amount,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&self.admin],
        )
        .expect("withdraw backing bucket with ledger to admin token")
    }

    fn sync_backing_domain_ledger_with_cu(&mut self, ledger: Pubkey, domain: u16) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::SyncBackingDomainLedger { domain },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(ledger, false),
            ],
            &[&self.admin],
        )
        .expect("sync backing domain ledger")
    }

    fn withdraw_backing_bucket_earnings_to_admin_token_with_cu(
        &mut self,
        ledger: Pubkey,
        dest: Pubkey,
        domain: u16,
        amount: u128,
    ) -> u64 {
        let market_id = self.asset_market_id(domain / 2);
        let authority_epoch =
            self.withdrawal_authority_epoch(self.admin.pubkey(), domain as usize / 2, false);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawBackingBucketEarnings {
                domain,
                market_id,
                authority_epoch,
                amount,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(ledger, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("withdraw backing bucket earnings")
    }

    fn sync_insurance_ledger_with_cu(&mut self, ledger: Pubkey) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::SyncInsuranceLedger,
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(ledger, false),
            ],
            &[&self.admin],
        )
        .expect("sync insurance ledger")
    }

    fn try_withdraw_insurance_domain_with_authority(
        &mut self,
        authority: &Keypair,
        domain: u16,
        amount: u128,
    ) -> Result<(Pubkey, u64), String> {
        self.try_withdraw_insurance_asset_with_authority(authority, (domain / 2) as u16, amount)
    }

    fn try_withdraw_insurance_asset_with_authority(
        &mut self,
        authority: &Keypair,
        asset_index: u16,
        amount: u128,
    ) -> Result<(Pubkey, u64), String> {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, authority.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let market_id = self.asset_market_id(asset_index);
        let authority_epoch =
            self.withdrawal_authority_epoch(authority.pubkey(), asset_index as usize, true);
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawInsuranceAsset {
                market_id,
                authority_epoch,
                asset_index,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )?;
        Ok((dest, cu))
    }

    fn withdraw_terminal_insurance_with_authority(
        &mut self,
        authority: &Keypair,
        asset_index: u16,
        amount: u128,
    ) -> (Pubkey, u64) {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, authority.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let market_id = self.asset_market_id(asset_index);
        let authority_epoch =
            self.withdrawal_authority_epoch(authority.pubkey(), asset_index as usize, true);
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index,
                market_id,
                authority_epoch,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
        .expect("withdraw terminal insurance");
        (dest, cu)
    }

    fn withdraw_terminal_insurance_with_authority_and_ledger(
        &mut self,
        authority: &Keypair,
        asset_index: u16,
        ledger: Pubkey,
        amount: u128,
    ) -> (Pubkey, u64) {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, authority.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let market_id = self.asset_market_id(asset_index);
        let authority_epoch =
            self.withdrawal_authority_epoch(authority.pubkey(), asset_index as usize, true);
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index,
                market_id,
                authority_epoch,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[authority],
        )
        .expect("withdraw terminal insurance with ledger");
        (dest, cu)
    }

    fn token_amount(&self, key: Pubkey) -> u64 {
        let account = self.svm.get_account(&key).expect("token account");
        TokenAccount::unpack(&account.data).unwrap().amount
    }

    fn convert_released_pnl_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        amount: u128,
    ) -> u64 {
        self.send(
            self.convert_released_pnl_ix(portfolio, amount),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("convert released pnl")
    }

    fn cure_and_cancel_close_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        source: Pubkey,
        amount: u128,
    ) -> u64 {
        let portfolio_id = self.portfolio_id(portfolio);
        let position_epoch = self.portfolio_position_epoch(portfolio);
        self.send(
            ProgInstruction::CureAndCancelClose {
                portfolio_id,
                position_epoch,
                optional_deposit: amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
        .expect("cure and cancel close")
    }

    fn forfeit_recovery_leg_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        asset_index: u16,
        b_delta_budget: u128,
    ) -> u64 {
        let portfolio_id = self.portfolio_id(portfolio);
        let position_epoch = self.portfolio_position_epoch(portfolio);
        self.send(
            ProgInstruction::ForfeitRecoveryLeg {
                portfolio_id,
                position_epoch,
                asset_index,
                b_delta_budget,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("forfeit recovery leg")
    }

    fn rebalance_reduce_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        asset_index: u16,
        reduce_q: u128,
    ) -> u64 {
        let portfolio_id = self.portfolio_id(portfolio);
        let position_epoch = self.portfolio_position_epoch(portfolio);
        self.send(
            ProgInstruction::RebalanceReduce {
                portfolio_id,
                position_epoch,
                asset_index,
                reduce_q,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("rebalance reduce")
    }

    fn finalize_reset_side_with_cu(&mut self, asset_index: u16, side: u8) -> u64 {
        self.send(
            ProgInstruction::FinalizeResetSide { asset_index, side },
            vec![AccountMeta::new(self.market, false)],
            &[],
        )
        .expect("finalize reset side")
    }

    fn claim_resolved_payout_topup_with_cu(
        &mut self,
        owner: Pubkey,
        portfolio: Pubkey,
        dest: Pubkey,
    ) -> u64 {
        self.send(
            ProgInstruction::ClaimResolvedPayoutTopup,
            vec![
                AccountMeta::new_readonly(owner, false),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("claim resolved payout topup")
    }

    fn crank(&mut self, portfolio: Pubkey, ix: ProgInstruction) -> u64 {
        self.crank_steps(portfolio, ix, 1)
    }

    fn send_crank_if_actionable(
        &mut self,
        ix: ProgInstruction,
        accounts: Vec<AccountMeta>,
        extra_signers: &[&Keypair],
    ) -> Option<u64> {
        assert!(
            matches!(&ix, ProgInstruction::PermissionlessCrank { .. }),
            "progress-aware sender is only valid for PermissionlessCrank"
        );
        let payer = self.payer.pubkey();
        let writable_before = accounts
            .iter()
            .filter(|account| account.is_writable && account.pubkey != payer)
            .map(|account| (account.pubkey, self.svm.get_account(&account.pubkey)))
            .collect::<Vec<_>>();
        assert!(
            !writable_before.is_empty(),
            "permissionless crank must expose writable economic state"
        );
        let call = format!("{ix:?}");

        self.svm.expire_blockhash();
        match self.send(ix, accounts, extra_signers) {
            Ok(cu) => {
                assert!(
                    writable_before
                        .iter()
                        .any(|(key, before)| self.svm.get_account(key) != *before),
                    "an accepted permissionless crank must mutate writable economic state"
                );
                Some(cu)
            }
            Err(error) if is_engine_non_progress_error(&error) => {
                for (key, before) in writable_before {
                    assert_eq!(
                        self.svm.get_account(&key),
                        before,
                        "EngineNonProgress must roll back writable account {key} exactly"
                    );
                }
                None
            }
            Err(error) => {
                panic!("permissionless crank {call} returned unexpected error: {error}")
            }
        }
    }

    fn crank_if_actionable(&mut self, portfolio: Pubkey, ix: ProgInstruction) -> Option<u64> {
        self.send_crank_if_actionable(
            ix,
            vec![
                AccountMeta::new(self.payer.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
    }

    fn crank_steps(&mut self, portfolio: Pubkey, ix: ProgInstruction, attempts: usize) -> u64 {
        let mut max_cu = 0;
        let mut progressed = false;
        for _ in 0..attempts {
            self.svm.expire_blockhash();
            match self.send(
                ix.clone(),
                vec![
                    AccountMeta::new(self.payer.pubkey(), true),
                    AccountMeta::new(self.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            ) {
                Ok(cu) => {
                    progressed = true;
                    max_cu = max_cu.max(cu);
                }
                Err(err) if progressed && err.contains("Custom(22)") => break,
                Err(err) => panic!("crank: {err}"),
            }
        }
        max_cu
    }

    fn crank_steps_after_market_catchup(
        &mut self,
        portfolio: Pubkey,
        ix: ProgInstruction,
        attempts: usize,
    ) -> u64 {
        let mut max_cu = 0;
        let mut account_steps = 0;
        let mut transactions = 0;
        while account_steps < attempts {
            transactions += 1;
            assert!(
                transactions <= 16_384,
                "crank did not finish bounded market catch-up"
            );
            let Some(cu) = self.crank_if_actionable(portfolio, ix.clone()) else {
                break;
            };
            max_cu = max_cu.max(cu);
            if !self.crank_observations_need_more_catchup(&ix) {
                account_steps += 1;
            }
        }
        max_cu
    }

    fn crank_with_oracle_tail(
        &mut self,
        portfolio: Pubkey,
        ix: ProgInstruction,
        oracle_accounts: &[Pubkey],
    ) -> u64 {
        let ix = match ix {
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations,
            } if observations.len() == 1 && !oracle_accounts.is_empty() => {
                ProgInstruction::PermissionlessCrank {
                    now_slot,
                    observations: crank_observations_with_accounts(
                        observations[0].asset_index,
                        oracle_accounts.len() as u8,
                    ),
                }
            }
            _ => ix,
        };
        let attempts = 1;
        let mut max_cu = 0;
        let mut progressed = false;
        for _ in 0..attempts {
            self.svm.expire_blockhash();
            let mut accounts = vec![
                AccountMeta::new(self.payer.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ];
            accounts.extend(
                oracle_accounts
                    .iter()
                    .copied()
                    .map(|key| AccountMeta::new_readonly(key, false)),
            );
            match self.send(ix.clone(), accounts, &[]) {
                Ok(cu) => {
                    progressed = true;
                    max_cu = max_cu.max(cu);
                }
                Err(err) if progressed && err.contains("Custom(22)") => break,
                Err(err) => panic!("crank with oracle tail: {err}"),
            }
        }
        max_cu
    }

    fn crank_observations_need_more_catchup(&self, ix: &ProgInstruction) -> bool {
        let ProgInstruction::PermissionlessCrank { observations, .. } = ix else {
            return false;
        };
        if observations.is_empty() {
            return false;
        }
        let authenticated_slot = self.svm.get_sysvar::<Clock>().slot;
        let Some(market) = self.svm.get_account(&self.market) else {
            return false;
        };
        let Ok((_, group)) = state::read_market(&market.data) else {
            return false;
        };
        observations.iter().any(|hint| {
            group
                .assets
                .get(hint.asset_index as usize)
                .is_some_and(|asset| asset.slot_last < authenticated_slot)
        })
    }

    fn try_force_close_abandoned_asset_with_cu(
        &mut self,
        cranker: &Keypair,
        account_a: Pubkey,
        account_b: Pubkey,
        asset_index: u16,
        now_slot: u64,
        close_q: u128,
    ) -> Result<u64, String> {
        self.ensure_signer_account(cranker.pubkey());
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ForceCloseAbandonedAsset {
                asset_index,
                now_slot,
                close_q,
            },
            vec![
                AccountMeta::new(cranker.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[cranker],
        )
    }

    fn force_close_abandoned_asset_with_cu(
        &mut self,
        cranker: &Keypair,
        account_a: Pubkey,
        account_b: Pubkey,
        asset_index: u16,
        now_slot: u64,
        close_q: u128,
    ) -> u64 {
        self.try_force_close_abandoned_asset_with_cu(
            cranker,
            account_a,
            account_b,
            asset_index,
            now_slot,
            close_q,
        )
        .expect("force close abandoned asset")
    }

    fn send(
        &mut self,
        ix: ProgInstruction,
        accounts: Vec<AccountMeta>,
        extra_signers: &[&Keypair],
    ) -> Result<u64, String> {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ix,
            accounts,
            extra_signers,
        )
    }
}

struct PublicBackingEarningsFixture {
    env: V16CuEnv,
    ledger: Pubkey,
    domain: u16,
    earnings: u128,
}

fn public_backing_earnings_fixture() -> PublicBackingEarningsFixture {
    const INITIAL_PRICE: u64 = 100;
    const SOURCE_POSITION_Q: i128 = 200 * POS_SCALE as i128;
    const HEDGE_POSITION_Q: i128 = 100 * POS_SCALE as i128;
    const LIEN_GROWTH_Q: i128 = 20 * POS_SCALE as i128;
    const INITIAL_CAPITAL: u128 = 3_130;
    const MAINTENANCE_FEE: u128 = 530;
    const DOMAIN: u16 = 1;

    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        4,
        1_000,
        1_000,
        500,
        MAINTENANCE_FEE,
    );
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);
    env.update_backing_fee_policy_with_cu(DOMAIN, 5_000, 2_500);
    env.svm.expire_blockhash();
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_portfolio = env.create_portfolio(&cross_owner);
    let counterparty_portfolio = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_portfolio, INITIAL_CAPITAL);
    env.deposit(&counterparty_owner, counterparty_portfolio, 10_000);
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, DOMAIN, 1_500, 10);

    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_portfolio,
        &counterparty_owner,
        counterparty_portfolio,
        SOURCE_POSITION_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_portfolio,
        &counterparty_owner,
        counterparty_portfolio,
        HEDGE_POSITION_Q,
        INITIAL_PRICE,
        0,
    );
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for (portfolio, asset_index) in [
        (counterparty_portfolio, 0),
        (cross_portfolio, 0),
        (counterparty_portfolio, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
            },
        );
    }
    assert_eq!(
        env.portfolio_state(cross_portfolio).capital.get(),
        INITIAL_CAPITAL - MAINTENANCE_FEE - 500,
        "the public transition must charge maintenance and capitalize the adverse hedge loss"
    );
    assert_eq!(
        env.portfolio_state(cross_portfolio).pnl.get(),
        1_000,
        "the favorable leg must remain a gross source-attributed claim"
    );
    assert!(
        env.market_state().1.source_credit[DOMAIN as usize].positive_claim_bound_num > 0,
        "the public mark route must create a source claim"
    );

    env.top_up_backing_bucket_with_ledger_with_cu(ledger, DOMAIN, 50_000, 10);
    env.deposit(&cross_owner, cross_portfolio, 500);
    env.deposit(&counterparty_owner, counterparty_portfolio, 500);
    let earnings_before =
        env.market_state().1.source_backing_buckets[DOMAIN as usize].utilization_fee_earnings;
    env.try_trade_asset_with_backing_fee_cap_with_cu(
        1,
        &cross_owner,
        cross_portfolio,
        &counterparty_owner,
        counterparty_portfolio,
        LIEN_GROWTH_Q,
        95,
        0,
        5_000,
    )
    .expect("public risk increase with signed backing-fee cap");
    let earnings = env.market_state().1.source_backing_buckets[DOMAIN as usize]
        .utilization_fee_earnings
        .checked_sub(earnings_before)
        .expect("public risk increase must not reduce provider earnings");
    assert!(
        earnings > 0,
        "the public route must generate provider earnings"
    );

    PublicBackingEarningsFixture {
        env,
        ledger,
        domain: DOMAIN,
        earnings,
    }
}

fn send_tx(
    svm: &mut LiteSVM,
    program_id: Pubkey,
    payer: &Keypair,
    mut ix: ProgInstruction,
    accounts: Vec<AccountMeta>,
    extra_signers: &[&Keypair],
) -> Result<u64, String> {
    bind_current_generation_guards(svm, &accounts, &mut ix);
    let instruction = Instruction {
        program_id,
        accounts,
        data: ix.encode(),
    };
    let mut signer_refs = Vec::with_capacity(1 + extra_signers.len());
    signer_refs.push(payer);
    signer_refs.extend_from_slice(extra_signers);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), instruction],
        Some(&payer.pubkey()),
        &signer_refs,
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .map(|meta| meta.compute_units_consumed)
        .map_err(|e| format!("{e:?}"))
}

fn bind_current_generation_guards(
    svm: &LiteSVM,
    accounts: &[AccountMeta],
    ix: &mut ProgInstruction,
) {
    let top_up_intent = match ix {
        ProgInstruction::TopUpInsurance {
            intent_id,
            authority_epoch,
            ..
        } => Some((0usize, true, intent_id, authority_epoch)),
        ProgInstruction::TopUpInsuranceDomain {
            domain,
            intent_id,
            authority_epoch,
            ..
        } => Some((*domain as usize / 2, true, intent_id, authority_epoch)),
        ProgInstruction::TopUpBackingBucket {
            domain,
            intent_id,
            authority_epoch,
            ..
        } => Some((*domain as usize / 2, false, intent_id, authority_epoch)),
        _ => None,
    };
    if let Some((asset_index, insurance, intent_id, authority_epoch)) = top_up_intent {
        let market_key = accounts.get(1).expect("top-up market account").pubkey;
        let market = svm.get_account(&market_key).expect("top-up market state");
        if let Ok(sequences) = state::read_asset_control_sequences(&market.data, asset_index) {
            *authority_epoch = sequences.authority_epoch;
            if *intent_id == 0 {
                *intent_id = next_control_sequence(if insurance {
                    sequences.insurance_top_up
                } else {
                    sequences.backing_top_up
                });
            }
        }
    }

    let market_wide_binding = match ix {
        ProgInstruction::ResolveMarket {
            asset_generation_frontier,
            authority_epoch,
        } => Some((asset_generation_frontier, Some(authority_epoch))),
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier,
            ..
        } => Some((asset_generation_frontier, None)),
        _ => None,
    };
    if let Some((asset_generation_frontier, authority_epoch)) = market_wide_binding {
        if *asset_generation_frontier != 0 {
            return;
        }
        let market_key = accounts.get(1).expect("market-wide market account").pubkey;
        let market = svm
            .get_account(&market_key)
            .expect("market-wide market state");
        let (_, group) = state::read_market(&market.data).expect("valid market-wide market state");
        *asset_generation_frontier = group.next_market_id;
        assert_ne!(
            *asset_generation_frontier, 0,
            "asset generation frontier must be nonzero"
        );
        if let Some(authority_epoch) = authority_epoch {
            *authority_epoch = state::read_asset_control_sequences(&market.data, 0)
                .expect("valid asset-0 control sequences")
                .authority_epoch;
        }
        return;
    }

    let (asset_index, market_id) = match ix {
        ProgInstruction::ConfigureHybridOracle {
            asset_index,
            market_id,
            ..
        }
        | ProgInstruction::ConfigureEwmaMark {
            asset_index,
            market_id,
            ..
        }
        | ProgInstruction::PushEwmaMark {
            asset_index,
            market_id,
            ..
        }
        | ProgInstruction::ConfigureAuthMark {
            asset_index,
            market_id,
            ..
        }
        | ProgInstruction::PushAuthMark {
            asset_index,
            market_id,
            ..
        }
        | ProgInstruction::RestartAssetOracle {
            asset_index,
            market_id,
            ..
        } => (*asset_index as usize, market_id),
        ProgInstruction::TopUpInsurance { market_id, .. } => (0, market_id),
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index,
            market_id,
            authority_epoch: 0,
            ..
        } => (*asset_index as usize, market_id),
        ProgInstruction::TopUpInsuranceDomain {
            domain, market_id, ..
        }
        | ProgInstruction::TopUpBackingBucket {
            domain, market_id, ..
        }
        | ProgInstruction::UpdateBackingFeePolicy {
            domain, market_id, ..
        } => (*domain as usize / 2, market_id),
        _ => return,
    };
    if *market_id != 0 {
        return;
    }
    let market_key = accounts.get(1).expect("asset-scoped market account").pubkey;
    let market = svm
        .get_account(&market_key)
        .expect("asset-scoped market state");
    let (_, group) = state::read_market(&market.data).expect("valid asset-scoped market state");
    let Some(asset) = group.assets.get(asset_index) else {
        return;
    };
    *market_id = asset.market_id;
    assert_ne!(*market_id, 0, "active asset generation must be nonzero");
}

fn is_engine_stale_error(error: &str) -> bool {
    error.contains("Custom(19)") || error.contains("custom program error: 0x13")
}

fn is_engine_non_progress_error(error: &str) -> bool {
    error.contains("Custom(22)") || error.contains("custom program error: 0x16")
}

fn resolved_portfolio_is_terminal(env: &V16CuEnv, portfolio: Pubkey) -> bool {
    let account = env.portfolio_state(portfolio);
    let receipt = resolved_receipt(&account);
    let (_, group) = env.market_state();
    account.capital.get() == 0
        && account.pnl.get() == 0
        && account.reserved_pnl.get() == 0
        && account.fee_credits.get() == 0
        && account.cancel_deposit_escrow.get() == 0
        && percolator::active_bitmap_is_empty(active_bitmap(&account))
        && account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied())
        && (!receipt.present || receipt.finalized)
        && account.last_fee_slot.get() == group.resolved_slot
}

fn drain_resolved_cohort(
    env: &mut V16CuEnv,
    actors: &[(&Keypair, Pubkey)],
    label: &str,
) -> Vec<u128> {
    drain_resolved_cohort_with_cu_limit(env, actors, label, CUSTODY_CU_LIMIT).0
}

fn drain_resolved_cohort_with_cu_limit(
    env: &mut V16CuEnv,
    actors: &[(&Keypair, Pubkey)],
    label: &str,
    cu_limit: u64,
) -> (Vec<u128>, u64) {
    let mut payouts = vec![0u128; actors.len()];
    let mut max_cu = 0;
    for round in 0..64 {
        if actors
            .iter()
            .all(|(_, portfolio)| resolved_portfolio_is_terminal(env, *portfolio))
        {
            return (payouts, max_cu);
        }

        let mut progressed = false;
        for (index, (owner, portfolio)) in actors.iter().enumerate() {
            if resolved_portfolio_is_terminal(env, *portfolio) {
                continue;
            }
            let market_before = env.svm.get_account(&env.market).unwrap();
            let portfolio_before = env.svm.get_account(portfolio).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            let (destination, result) = env.try_close_resolved_with_cu(owner, *portfolio);
            match result {
                Ok(cu) => {
                    assert_cu_within(label, cu, cu_limit);
                    max_cu = max_cu.max(cu);
                    let paid = env.token_amount(destination) as u128;
                    payouts[index] = payouts[index]
                        .checked_add(paid)
                        .expect("resolved cohort payout overflow");
                    assert!(
                        env.svm.get_account(&env.market).unwrap() != market_before
                            || env.svm.get_account(portfolio).unwrap() != portfolio_before
                            || env.svm.get_account(&env.vault).unwrap() != vault_before
                            || paid != 0,
                        "{label}: successful call made no progress in round {round}"
                    );
                    progressed = true;
                }
                Err(error) if is_engine_non_progress_error(&error) => {
                    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                    assert_eq!(env.svm.get_account(portfolio).unwrap(), portfolio_before);
                    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
                    assert_eq!(env.token_amount(destination), 0);
                }
                Err(error) => panic!("{label}: unexpected resolved crank error: {error}"),
            }
        }
        assert!(
            progressed,
            "{label}: nonterminal cohort reached a fixed point in round {round}"
        );
    }
    panic!("{label}: cohort did not terminate in 64 bounded rounds");
}

fn send_raw_tx(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instruction: Instruction,
    extra_signers: &[&Keypair],
) -> Result<u64, String> {
    let mut signer_refs = Vec::with_capacity(1 + extra_signers.len());
    signer_refs.push(payer);
    signer_refs.extend_from_slice(extra_signers);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), instruction],
        Some(&payer.pubkey()),
        &signer_refs,
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .map(|meta| meta.compute_units_consumed)
        .map_err(|e| format!("{e:?}"))
}

fn send_raw_ixs(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instructions: Vec<Instruction>,
    extra_signers: &[&Keypair],
) -> Result<u64, String> {
    let mut signer_refs = Vec::with_capacity(1 + extra_signers.len());
    signer_refs.push(payer);
    signer_refs.extend_from_slice(extra_signers);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &signer_refs,
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .map(|meta| meta.compute_units_consumed)
        .map_err(|e| format!("{e:?}"))
}

fn system_create_account_for_test(
    svm: &mut LiteSVM,
    payer: &Keypair,
    account: &Keypair,
    data_len: usize,
    owner: Pubkey,
) -> u64 {
    send_raw_tx(
        svm,
        payer,
        system_instruction::create_account(
            &payer.pubkey(),
            &account.pubkey(),
            1_000_000_000,
            data_len as u64,
            &owner,
        ),
        &[account],
    )
    .expect("system create account")
}

fn create_ata_for_test(svm: &mut LiteSVM, payer: &Keypair, wallet: Pubkey, mint: Pubkey) -> Pubkey {
    let ata = Pubkey::find_program_address(
        &[wallet.as_ref(), spl_token::ID.as_ref(), mint.as_ref()],
        &associated_token_program_id(),
    )
    .0;
    send_raw_tx(
        svm,
        payer,
        Instruction {
            program_id: associated_token_program_id(),
            accounts: vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(wallet, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
            ],
            data: vec![],
        },
        &[],
    )
    .expect("create associated token account");
    ata
}

fn assert_cu_within(label: &str, cu: u64, limit: u64) {
    assert!(
        cu <= limit,
        "{label} consumed {cu} CU, above the {limit} CU guardrail"
    );
}
fn init_independent_market_same_mint(
    env: &mut V16CuEnv,
    params: V16CuMarketParams,
) -> (Pubkey, Pubkey, Pubkey) {
    let market = Pubkey::new_unique();
    let vault_authority =
        Pubkey::find_program_address(&[b"vault", market.as_ref()], &env.program_id).0;
    let vault = canonical_vault_ata(vault_authority, env.mint);
    env.svm
        .set_account(
            market,
            Account {
                lamports: 1_000_000_000,
                data: vec![
                    0u8;
                    state::market_account_len_for_capacity(
                        params.max_portfolio_assets as usize
                    )
                    .unwrap()
                ],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitMarket {
            max_portfolio_assets: params.max_portfolio_assets,
            h_min: params.h_min,
            h_max: params.h_max,
            initial_price: params.initial_price,
            min_nonzero_mm_req: params.min_nonzero_mm_req,
            min_nonzero_im_req: params.min_nonzero_im_req,
            maintenance_margin_bps: params.maintenance_margin_bps,
            initial_margin_bps: params.initial_margin_bps,
            max_trading_fee_bps: params.max_trading_fee_bps,
            trade_fee_base_bps: params.trade_fee_base_bps,
            liquidation_fee_bps: params.liquidation_fee_bps,
            liquidation_fee_cap: params.liquidation_fee_cap,
            min_liquidation_abs: params.min_liquidation_abs,
            max_price_move_bps_per_slot: params.max_price_move_bps_per_slot,
            max_accrual_dt_slots: params.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: params.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: params.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: params.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: params.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: params.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: params.public_b_chunk_atoms,
            maintenance_fee_per_slot: params.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&env.admin],
    )
    .expect("init independent market");
    (market, vault_authority, vault)
}

fn init_portfolio_on_market(
    env: &mut V16CuEnv,
    market: Pubkey,
    owner: &Keypair,
    max_market_slots: usize,
) -> Pubkey {
    let portfolio = Pubkey::new_unique();
    env.ensure_signer_account(owner.pubkey());
    env.svm
        .set_account(
            portfolio,
            Account {
                lamports: 1_000_000_000,
                data: vec![
                    0u8;
                    state::portfolio_account_len_for_market_slots(max_market_slots).unwrap()
                ],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[owner],
    )
    .expect("init portfolio on explicit market");
    portfolio
}

fn deposit_to_market(
    env: &mut V16CuEnv,
    market: Pubkey,
    vault: Pubkey,
    owner: &Keypair,
    portfolio: Pubkey,
    amount: u128,
) -> Pubkey {
    let source = env.token_account(owner.pubkey(), amount as u64);
    let portfolio_id = env.portfolio_id(portfolio);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::Deposit {
            portfolio_id,
            expected_sequence: 0,
            amount,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(source, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .expect("deposit to explicit market");
    source
}

fn top_up_backing_bucket_to_market(
    env: &mut V16CuEnv,
    market: Pubkey,
    vault: Pubkey,
    domain: u16,
    amount: u128,
    expiry_slot: u64,
) -> Pubkey {
    let source = env.token_account(env.admin.pubkey(), amount as u64);
    let market_account = env.svm.get_account(&market).expect("market account");
    let profile = state::read_asset_oracle_profile(&market_account.data, domain as usize / 2)
        .expect("decode explicit-market backing fee profile");
    let (backing_fee_bps, insurance_share_bps) = if domain % 2 == 0 {
        (
            profile.backing_trade_fee_bps_long,
            profile.backing_trade_fee_insurance_share_bps_long,
        )
    } else {
        (
            profile.backing_trade_fee_bps_short,
            profile.backing_trade_fee_insurance_share_bps_short,
        )
    };
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            authority_epoch: 0,
            intent_id: 0,
            market_id: 0,
            domain,
            backing_fee_bps,
            insurance_share_bps,
            amount,
            expiry_slot,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin],
    )
    .expect("top up explicit market backing bucket");
    source
}

fn add_source_positive_pnl_to_market(
    env: &mut V16CuEnv,
    market: Pubkey,
    portfolio: Pubkey,
    domain: usize,
    amount: u128,
) {
    let mut market_account = env.svm.get_account(&market).expect("market account");
    let mut portfolio_account = env.svm.get_account(&portfolio).expect("portfolio account");
    let max_slots = state::read_market_config_mode_and_capacity(&market_account.data)
        .unwrap()
        .2;
    {
        let (_cfg, mut group) = state::market_view_mut(&mut market_account.data).unwrap();
        let mut account =
            state::portfolio_view_mut_for_market_slots(&mut portfolio_account.data, max_slots)
                .unwrap();
        group
            .add_account_source_positive_pnl_not_atomic(&mut account, domain, amount)
            .unwrap();
    }
    env.svm.set_account(market, market_account).unwrap();
    env.svm.set_account(portfolio, portfolio_account).unwrap();
}
fn crank_portfolio_on_market(
    env: &mut V16CuEnv,
    market: Pubkey,
    portfolio: Pubkey,
    ix: ProgInstruction,
) -> u64 {
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ix,
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    )
    .expect("crank portfolio on explicit market")
}
#[derive(Clone, Copy, Debug)]
enum AssetGenerationTradePath {
    TradeNoCpi,
    BatchTradeNoCpi,
    TradeCpi,
    BatchTradeCpi,
}

// A transaction signed for one asset generation (for example, with a durable nonce) must not
// execute after public retirement and permissionless slot reuse. Every route rejects before
// mutation; the replacement generation then accepts the same trade when its current market_id is
// signed, so the guard does not block trading.
fn assert_signed_trade_cannot_replay_across_asset_slot_reuse() {
    const ASSET: u16 = 1;
    const OLD_PRICE: u64 = 100;
    const NEW_PRICE: u64 = 250;

    for path in [
        AssetGenerationTradePath::TradeNoCpi,
        AssetGenerationTradePath::BatchTradeNoCpi,
        AssetGenerationTradePath::TradeCpi,
        AssetGenerationTradePath::BatchTradeCpi,
    ] {
        let mut env = V16CuEnv::new();
        env.update_market_init_fee_policy_with_cu(1);
        env.svm.warp_to_slot(1);
        env.activate_asset(ASSET, 1, OLD_PRICE);

        let trader_a = Keypair::new();
        let trader_b = Keypair::new();
        let account_a = env.create_portfolio(&trader_a);
        let account_b = env.create_portfolio(&trader_b);
        env.deposit(&trader_a, account_a, 1_000_000);
        env.deposit(&trader_b, account_b, 1_000_000);
        let (matcher_program, matcher_ctx, matcher_delegate) =
            auth_matcher_for_lp_via_system_create(&mut env, &trader_b, account_b);

        let old_market_id = env.asset_market_id(ASSET);
        let account_a_portfolio_id = env.portfolio_id(account_a);
        let account_b_portfolio_id = env.portfolio_id(account_b);
        let account_b_matcher_sequence = env.portfolio_matcher_sequence(account_b);
        let stale_instruction = match path {
            AssetGenerationTradePath::TradeNoCpi => ProgInstruction::TradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id,
                account_b_position_epoch: 0,
                asset_index: ASSET,
                market_id: old_market_id,
                size_q: POS_SCALE as i128,
                exec_price: OLD_PRICE,
                fee_bps: 0,
                backing_fee_cap_bps: 0,
            },
            AssetGenerationTradePath::BatchTradeNoCpi => ProgInstruction::BatchTradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id,
                account_b_position_epoch: 0,
                legs: vec![BatchTradeLeg {
                    asset_index: ASSET,
                    market_id: old_market_id,
                    size_q: POS_SCALE as i128,
                    exec_price: OLD_PRICE,
                    fee_bps: 0,
                }],
            },
            AssetGenerationTradePath::TradeCpi => ProgInstruction::TradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id,
                account_b_position_epoch: 0,
                account_b_matcher_sequence,
                asset_index: ASSET,
                market_id: old_market_id,
                size_q: POS_SCALE as i128,
                fee_bps: 0,
                limit_price: 0,
                backing_fee_cap_bps: 0,
            },
            AssetGenerationTradePath::BatchTradeCpi => ProgInstruction::BatchTradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id,
                account_b_position_epoch: 0,
                account_b_matcher_sequence,
                legs: vec![BatchTradeCpiLeg {
                    asset_index: ASSET,
                    market_id: old_market_id,
                    size_q: POS_SCALE as i128,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            },
        };
        let cpi = matches!(
            path,
            AssetGenerationTradePath::TradeCpi | AssetGenerationTradePath::BatchTradeCpi
        );
        let stale_accounts = if cpi {
            vec![
                AccountMeta::new(trader_a.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(matcher_ctx, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ]
        } else {
            vec![
                AccountMeta::new(trader_a.pubkey(), true),
                AccountMeta::new(trader_b.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ]
        };
        let stale_trade_ix = Instruction {
            program_id: env.program_id,
            accounts: stale_accounts.clone(),
            data: stale_instruction.encode(),
        };
        let stale_trade = if cpi {
            Transaction::new_signed_with_payer(
                &[heap_ix(), cu_ix(), stale_trade_ix],
                Some(&env.payer.pubkey()),
                &[&env.payer, &trader_a],
                env.svm.latest_blockhash(),
            )
        } else {
            Transaction::new_signed_with_payer(
                &[heap_ix(), cu_ix(), stale_trade_ix],
                Some(&env.payer.pubkey()),
                &[&env.payer, &trader_a, &trader_b],
                env.svm.latest_blockhash(),
            )
        };

        env.svm.warp_to_slot(3);
        env.update_asset_lifecycle_as_admin_with_cu(
            percolator_prog::processor::ASSET_ACTION_RETIRE,
            ASSET,
            3,
            0,
        );
        let replacement_authority = Keypair::new();
        env.svm.warp_to_slot(4);
        env.activate_permissionless_asset_with_fee(
            &replacement_authority,
            ASSET,
            4,
            NEW_PRICE,
            replacement_authority.pubkey(),
            replacement_authority.pubkey(),
            replacement_authority.pubkey(),
            replacement_authority.pubkey(),
            1,
        );
        let new_market_id = env.asset_market_id(ASSET);
        assert_ne!(
            new_market_id, old_market_id,
            "{path:?}: slot reuse creates a new market generation"
        );

        let market_before = env.svm.get_account(&env.market).unwrap();
        let account_a_before = env.svm.get_account(&account_a).unwrap();
        let account_b_before = env.svm.get_account(&account_b).unwrap();
        let matcher_before = env.svm.get_account(&matcher_ctx).unwrap();
        let replay_error = env
            .svm
            .send_transaction(stale_trade)
            .expect_err("old generation must reject");
        let replay_error = format!("{replay_error:?}");
        let expected_error = format!(
            "Custom({})",
            PercolatorError::AssetGenerationMismatch as u32
        );
        assert!(
            replay_error.contains(&expected_error),
            "{path:?}: stale generation must fail with {expected_error}, got {replay_error}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&account_a).unwrap(), account_a_before);
        assert_eq!(env.svm.get_account(&account_b).unwrap(), account_b_before);
        assert_eq!(env.svm.get_account(&matcher_ctx).unwrap(), matcher_before);

        let current_instruction = match path {
            AssetGenerationTradePath::TradeNoCpi => ProgInstruction::TradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id,
                account_b_position_epoch: 0,
                asset_index: ASSET,
                market_id: new_market_id,
                size_q: POS_SCALE as i128,
                exec_price: NEW_PRICE,
                fee_bps: 0,
                backing_fee_cap_bps: 0,
            },
            AssetGenerationTradePath::BatchTradeNoCpi => ProgInstruction::BatchTradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id,
                account_b_position_epoch: 0,
                legs: vec![BatchTradeLeg {
                    asset_index: ASSET,
                    market_id: new_market_id,
                    size_q: POS_SCALE as i128,
                    exec_price: NEW_PRICE,
                    fee_bps: 0,
                }],
            },
            AssetGenerationTradePath::TradeCpi => ProgInstruction::TradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id,
                account_b_position_epoch: 0,
                account_b_matcher_sequence,
                asset_index: ASSET,
                market_id: new_market_id,
                size_q: POS_SCALE as i128,
                fee_bps: 0,
                limit_price: 0,
                backing_fee_cap_bps: 0,
            },
            AssetGenerationTradePath::BatchTradeCpi => ProgInstruction::BatchTradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch: 0,
                account_b_portfolio_id,
                account_b_position_epoch: 0,
                account_b_matcher_sequence,
                legs: vec![BatchTradeCpiLeg {
                    asset_index: ASSET,
                    market_id: new_market_id,
                    size_q: POS_SCALE as i128,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            },
        };
        let current_trade = if cpi {
            env.send(current_instruction, stale_accounts, &[&trader_a])
        } else {
            env.send(current_instruction, stale_accounts, &[&trader_a, &trader_b])
        };
        assert!(
            current_trade.is_ok(),
            "{path:?}: the replacement generation remains tradeable: {current_trade:?}"
        );
        let account_a_after = env.portfolio_state(account_a);
        let account_b_after = env.portfolio_state(account_b);
        let leg_a = active_leg_for_asset(&account_a_after, ASSET as usize);
        let leg_b = active_leg_for_asset(&account_b_after, ASSET as usize);
        assert_eq!(leg_a.market_id, new_market_id);
        assert_eq!(leg_b.market_id, new_market_id);
        assert_eq!(leg_a.basis_pos_q, POS_SCALE as i128);
        assert_eq!(leg_b.basis_pos_q, -(POS_SCALE as i128));
    }
}
#[derive(Clone, Copy, Debug)]
enum SourceCreditWatermarkTradePath {
    NoCpi,
    Cpi,
}

#[derive(Clone, Copy, Debug)]
enum SourceCreditWatermarkDirection {
    PositiveSize,
    NegativeSize,
}

#[allow(clippy::too_many_arguments)]
fn try_source_credit_watermark_trade(
    env: &mut V16CuEnv,
    path: SourceCreditWatermarkTradePath,
    matcher_accounts: Option<(Pubkey, Pubkey, Pubkey)>,
    owner_a: &Keypair,
    account_a: Pubkey,
    owner_b: &Keypair,
    account_b: Pubkey,
    asset_index: u16,
    size_q: i128,
    exec_price: u64,
    fee_bps: u64,
) -> Result<u64, String> {
    match path {
        SourceCreditWatermarkTradePath::NoCpi => env.try_trade_asset_with_cu(
            asset_index,
            owner_a,
            account_a,
            owner_b,
            account_b,
            size_q,
            exec_price,
            fee_bps,
        ),
        SourceCreditWatermarkTradePath::Cpi => {
            let (matcher_program, matcher_ctx, matcher_delegate) =
                matcher_accounts.expect("matcher accounts");
            env.try_trade_cpi_with_cu_on_asset(
                owner_a,
                account_a,
                owner_b,
                account_b,
                matcher_program,
                matcher_ctx,
                matcher_delegate,
                asset_index,
                size_q,
                fee_bps,
            )
        }
    }
}

fn run_source_credit_watermark_trade_case(
    path: SourceCreditWatermarkTradePath,
    direction: SourceCreditWatermarkDirection,
) {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = POS_SCALE as i128;
    const DEPOSIT: u128 = 313;
    const EXPECTED_GROSS_SOURCE_CLAIM: i128 = 100;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    let matcher_program = match path {
        SourceCreditWatermarkTradePath::NoCpi => None,
        SourceCreditWatermarkTradePath::Cpi => {
            let matcher_program = Pubkey::new_unique();
            let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
            env.svm.add_program(matcher_program, &matcher_bytes);
            Some(matcher_program)
        }
    };
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 1_000);
    let (winning_domain, asset0_mark, asset1_mark, side_sign) = match direction {
        SourceCreditWatermarkDirection::PositiveSize => (1usize, 105, 95, 1i128),
        SourceCreditWatermarkDirection::NegativeSize => (0usize, 95, 105, -1i128),
    };
    env.top_up_backing_bucket(winning_domain as u16, 150, 10);

    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        side_sign * ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        side_sign * ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, asset0_mark);
    env.push_auth_mark_for_asset_as_admin(1, 2, asset1_mark);
    for (portfolio, asset_index) in [
        (counterparty_account, 0),
        (cross_account, 0),
        (counterparty_account, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
            },
        );
    }
    let forced_capital = match direction {
        SourceCreditWatermarkDirection::PositiveSize => 260,
        SourceCreditWatermarkDirection::NegativeSize => 100,
    };
    env.force_portfolio_capital_for_benchmark(cross_account, forced_capital);

    let cross_before = env.portfolio_state(cross_account);
    assert_eq!(
        cross_before.pnl.get(),
        EXPECTED_GROSS_SOURCE_CLAIM,
        "{path:?} {direction:?} setup must retain the gross source-attributed claim after complete refresh"
    );
    let (_, before_withdraw_group) = env.market_state();
    assert_eq!(
        before_withdraw_group.source_credit[winning_domain].positive_claim_bound_num,
        EXPECTED_GROSS_SOURCE_CLAIM as u128 * BOUND_SCALE
    );
    let surplus_backing = before_withdraw_group.source_credit[winning_domain]
        .fresh_reserved_backing_num
        .checked_sub(before_withdraw_group.source_credit[winning_domain].positive_claim_bound_num)
        .unwrap()
        / BOUND_SCALE;
    assert!(
        surplus_backing > 0,
        "{path:?} {direction:?} setup must leave withdrawable surplus backing"
    );

    let watermark_withdraw_dest = env.token_account(env.admin.pubkey(), 0);
    env.withdraw_backing_bucket_to_admin_token_with_cu(
        watermark_withdraw_dest,
        winning_domain as u16,
        surplus_backing,
    );
    let (_, exact_watermark_group) = env.market_state();
    assert_eq!(
        exact_watermark_group.source_credit[winning_domain].fresh_reserved_backing_num,
        exact_watermark_group.source_credit[winning_domain].positive_claim_bound_num,
        "{path:?} {direction:?} setup must leave no surplus source-credit backing"
    );
    // The setup positions were created outside the matcher. Authorize the matcher only after those
    // mutations so this route permutation starts from current, explicit LP consent.
    let matcher_accounts = matcher_program.map(|program| {
        let (ctx, delegate, _) =
            env.init_matcher_context_authorized(program, &counterparty_owner, counterparty_account);
        (program, ctx, delegate)
    });

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_cross = env.svm.get_account(&cross_account).unwrap();
    let before_counterparty = env.svm.get_account(&counterparty_account).unwrap();
    let over_watermark = try_source_credit_watermark_trade(
        &mut env,
        path,
        matcher_accounts,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        1,
        side_sign * SAFE_INCREASE_Q,
        asset1_mark,
        0,
    );
    assert!(
        over_watermark.is_err(),
        "{path:?} {direction:?} risk increase must reject at the exact source-credit watermark"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market.data
    );
    assert_eq!(
        env.svm.get_account(&cross_account).unwrap().data,
        before_cross.data
    );
    assert_eq!(
        env.svm.get_account(&counterparty_account).unwrap().data,
        before_counterparty.data
    );

    env.top_up_backing_bucket(winning_domain as u16, 5_000, 10);
    let second_pass_deposit = match direction {
        SourceCreditWatermarkDirection::PositiveSize => 50,
        SourceCreditWatermarkDirection::NegativeSize => 200,
    };
    env.deposit(&cross_owner, cross_account, second_pass_deposit);
    env.deposit(
        &counterparty_owner,
        counterparty_account,
        second_pass_deposit,
    );
    env.svm.warp_to_slot(3);
    let inside_watermark = try_source_credit_watermark_trade(
        &mut env,
        path,
        matcher_accounts,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        1,
        side_sign * SAFE_INCREASE_Q,
        asset1_mark,
        1,
    );
    assert!(
        inside_watermark.is_ok(),
        "{path:?} {direction:?} risk increase inside the source-credit watermark failed: {inside_watermark:?}"
    );

    let (_, after_group) = env.market_state();
    let cross_after = env.portfolio_state(cross_account);
    assert_eq!(
        after_group.source_credit[winning_domain].credit_rate_num,
        percolator::CREDIT_RATE_SCALE,
        "{path:?} {direction:?} must not dilute live positive claims"
    );
    assert!(
        state::portfolio_source_domain(&cross_after, winning_domain)
            .source_lien_effective_reserved
            .get()
            > 0,
        "{path:?} {direction:?} must reserve source credit once surplus backing exists"
    );
}
fn set_test_clock(env: &mut V16CuEnv, slot: u64, unix_timestamp: i64) {
    env.svm.warp_to_slot(slot);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = unix_timestamp;
    env.svm.set_sysvar(&clock);
}

fn run_hybrid_fresh_oracle_trade_case(dt: u64, oracle_leg_count: u8, invert: u8) {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);

    let seed = 0xc0u8
        .wrapping_add((dt as u8) << 4)
        .wrapping_add(oracle_leg_count << 1)
        .wrapping_add(invert);
    let mut feeds = [[0u8; 32]; 3];
    feeds[0] = [seed; 32];
    if oracle_leg_count == 3 {
        feeds[1] = [seed.wrapping_add(1); 32];
        feeds[2] = [seed.wrapping_add(2); 32];
    }
    let oracle_leg_flags = if oracle_leg_count == 3 {
        ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3
    } else {
        0
    };

    let initial_oracles = if oracle_leg_count == 1 {
        vec![env.set_pyth_price(&feeds[0], 200_000, -6, 100)]
    } else {
        vec![
            env.set_pyth_price(&feeds[0], 4_000_000_000, -6, 100),
            env.set_pyth_price(&feeds[1], 150_000_000, -6, 100),
            env.set_pyth_price(&feeds[2], 200_000_000, -6, 100),
        ]
    };
    let configure_cu = env
        .try_configure_hybrid_with_cu(
            oracle_leg_count,
            oracle_leg_flags,
            feeds,
            &initial_oracles,
            1,
            100,
            invert,
            0,
            3,
        )
        .expect("configure hybrid oracle");
    assert_cu_within(
        "HybridMark fresh-trade configure",
        configure_cu,
        CUSTODY_CU_LIMIT,
    );

    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    set_test_clock(&mut env, 2, 101);
    let fresh_oracles = if oracle_leg_count == 1 {
        vec![env.set_pyth_price(&feeds[0], 210_000, -6, 101)]
    } else {
        vec![
            env.set_pyth_price(&feeds[0], 4_200_000_000, -6, 101),
            env.set_pyth_price(&feeds[1], 150_000_000, -6, 101),
            env.set_pyth_price(&feeds[2], 200_000_000, -6, 101),
        ]
    };
    let fresh_crank_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        &fresh_oracles,
    );
    assert_cu_within(
        "HybridMark fresh-trade crank",
        fresh_crank_cu,
        CRANK_CU_LIMIT,
    );

    let (fresh_cfg, fresh_group) = env.market_state();
    let mark = fresh_group.assets[0].effective_price;
    assert!(mark > 0, "fresh HybridMark case produced a zero mark");
    assert_eq!(fresh_cfg.last_good_oracle_slot, 2);
    assert_eq!(fresh_cfg.hybrid_soft_stale_slots, 3);
    assert_eq!(fresh_cfg.mark_ewma_e6, mark);
    assert_eq!(fresh_group.assets[0].raw_oracle_target_price, mark);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000_000);
    env.deposit(&short_owner, short_account, 10_000_000);

    if dt == 1 {
        set_test_clock(&mut env, 3, 102);
    }
    let (before_trade_cfg, before_trade_group) = env.market_state();
    let trade_slot = env.svm.get_sysvar::<Clock>().slot;
    assert_eq!(
        trade_slot - before_trade_cfg.last_good_oracle_slot,
        dt,
        "test case must trade while the hybrid oracle is still fresh"
    );
    assert!(
        dt <= before_trade_cfg.hybrid_soft_stale_slots,
        "test case must remain inside the live-oracle freshness window"
    );
    let insurance_before = before_trade_group.insurance;

    let size_q = POS_SCALE;
    let open_cu = env
        .try_trade_asset_with_cu(
            0,
            &long_owner,
            long_account,
            &short_owner,
            short_account,
            size_q as i128,
            mark,
            0,
        )
        .unwrap_or_else(|err| {
            panic!(
                "fresh HybridMark TradeNoCpi open failed for dt={dt}, legs={oracle_leg_count}, invert={invert}: {err}"
            )
        });
    assert_cu_within("HybridMark fresh open", open_cu, TRADE_CU_LIMIT);
    let (opened_cfg, opened_group) = env.market_state();
    assert_eq!(opened_group.assets[0].oi_eff_long_q, size_q);
    assert_eq!(opened_group.assets[0].oi_eff_short_q, size_q);
    assert_eq!(opened_group.assets[0].effective_price, mark);
    assert_eq!(opened_group.assets[0].raw_oracle_target_price, mark);
    assert_eq!(opened_cfg.mark_ewma_e6, mark);
    assert_eq!(
        opened_group.insurance, insurance_before,
        "fresh HybridMark trade at the live mark must not charge an after-hours movement premium"
    );

    let close_cu = env
        .try_trade_asset_with_cu(
            0,
            &long_owner,
            long_account,
            &short_owner,
            short_account,
            -(size_q as i128),
            mark,
            0,
        )
        .unwrap_or_else(|err| {
            panic!(
                "fresh HybridMark TradeNoCpi close failed for dt={dt}, legs={oracle_leg_count}, invert={invert}: {err}"
            )
        });
    assert_cu_within("HybridMark fresh close", close_cu, TRADE_CU_LIMIT);
    let (_, flat_group) = env.market_state();
    assert_eq!(flat_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(flat_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(flat_group.assets[0].effective_price, mark);
    assert_eq!(flat_group.insurance, insurance_before);
}
fn production_risk_params() -> V16CuMarketParams {
    V16CuMarketParams {
        h_max: 6_480_000,
        initial_price: 1_000_000,
        min_nonzero_mm_req: 599,
        min_nonzero_im_req: 600,
        maintenance_margin_bps: 500,
        initial_margin_bps: 500,
        liquidation_fee_bps: 5,
        liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
        max_price_move_bps_per_slot: 24,
        max_accrual_dt_slots: 20,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 10_000_000,
        ..V16CuMarketParams::default()
    }
}

#[derive(Clone, Copy)]
struct ProductionRiskOraclePrices {
    leg0: i64,
    leg1: i64,
    leg2: i64,
}

impl ProductionRiskOraclePrices {
    fn default_inverted_composite() -> Self {
        Self {
            leg0: 4_200_000_000,
            leg1: 150_000_000,
            leg2: 200_000_000,
        }
    }

    fn sub_one_inverted_composite() -> Self {
        Self {
            leg0: 2_155_172_400,
            leg1: 5_000_000,
            leg2: 5_000_000,
        }
    }
}

#[derive(Clone, Copy)]
struct ProductionRiskTradeCase {
    name: &'static str,
    fixed_deposit: Option<u128>,
    same_owner: bool,
    oracle_prices: ProductionRiskOraclePrices,
    oracle_conf_bps: u16,
    conf_filter_bps: u16,
    size_q_abs: u128,
    assert_sub_one_mark: bool,
}

impl ProductionRiskTradeCase {
    fn baseline() -> Self {
        Self {
            name: "baseline",
            fixed_deposit: None,
            same_owner: false,
            oracle_prices: ProductionRiskOraclePrices::default_inverted_composite(),
            oracle_conf_bps: 0,
            conf_filter_bps: 500,
            size_q_abs: POS_SCALE,
            assert_sub_one_mark: false,
        }
    }

    fn fixed_deposit() -> Self {
        Self {
            name: "fixed-300m-deposit",
            fixed_deposit: Some(300_000_000),
            ..Self::baseline()
        }
    }

    fn same_owner() -> Self {
        Self {
            name: "same-owner-counterparties",
            same_owner: true,
            ..Self::baseline()
        }
    }

    fn sub_one_mark() -> Self {
        Self {
            name: "sub-one-inverted-mark",
            oracle_prices: ProductionRiskOraclePrices::sub_one_inverted_composite(),
            size_q_abs: 10 * POS_SCALE,
            assert_sub_one_mark: true,
            ..Self::baseline()
        }
    }

    fn real_conf_filter() -> Self {
        Self {
            name: "pyth-conf-150bps-filter-200bps",
            oracle_conf_bps: 150,
            conf_filter_bps: 200,
            ..Self::baseline()
        }
    }
}

fn pyth_conf_for_bps(price: i64, conf_bps: u16) -> u64 {
    if conf_bps == 0 {
        return 1;
    }
    ((price as u128) * conf_bps as u128 / 10_000)
        .max(1)
        .try_into()
        .unwrap()
}

fn set_production_risk_oracles(
    env: &mut V16CuEnv,
    feeds: &[[u8; 32]; 3],
    prices: ProductionRiskOraclePrices,
    conf_bps: u16,
    publish_time: i64,
) -> [Pubkey; 3] {
    [
        env.set_pyth_price_with_conf(
            &feeds[0],
            prices.leg0,
            -6,
            pyth_conf_for_bps(prices.leg0, conf_bps),
            publish_time,
        ),
        env.set_pyth_price_with_conf(
            &feeds[1],
            prices.leg1,
            -6,
            pyth_conf_for_bps(prices.leg1, conf_bps),
            publish_time,
        ),
        env.set_pyth_price_with_conf(
            &feeds[2],
            prices.leg2,
            -6,
            pyth_conf_for_bps(prices.leg2, conf_bps),
            publish_time,
        ),
    ]
}

fn run_hybrid_fresh_oracle_production_risk_trade_case(
    asset_index: u16,
    case: ProductionRiskTradeCase,
    direction_sign: i128,
) {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    set_test_clock(&mut env, 1, 100);
    if asset_index != 0 {
        env.activate_asset(asset_index, 1, production_risk_params().initial_price);
    }

    let feed_seed = 0xe0u8.wrapping_add(asset_index as u8 * 3);
    let feeds = [
        [feed_seed.wrapping_add(1); 32],
        [feed_seed.wrapping_add(2); 32],
        [feed_seed.wrapping_add(3); 32],
    ];
    let [initial_leg0, initial_leg1, initial_leg2] = set_production_risk_oracles(
        &mut env,
        &feeds,
        case.oracle_prices,
        case.oracle_conf_bps,
        100,
    );
    let configure_cu = env
        .try_configure_hybrid_asset_with_conf_filter_cu(
            asset_index,
            3,
            ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            feeds,
            &[initial_leg0, initial_leg1, initial_leg2],
            1,
            100,
            1,
            0,
            3,
            case.conf_filter_bps,
        )
        .expect("configure inverted production-risk hybrid oracle");
    assert_cu_within(
        "HybridMark production-risk configure",
        configure_cu,
        CUSTODY_CU_LIMIT,
    );

    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    set_test_clock(&mut env, 2, 101);
    let [fresh_leg0, fresh_leg1, fresh_leg2] = set_production_risk_oracles(
        &mut env,
        &feeds,
        case.oracle_prices,
        case.oracle_conf_bps,
        101,
    );
    let fresh_crank_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(asset_index),
        },
        &[fresh_leg0, fresh_leg1, fresh_leg2],
    );
    assert_cu_within(
        "HybridMark production-risk fresh crank",
        fresh_crank_cu,
        CRANK_CU_LIMIT,
    );
    let (fresh_cfg, fresh_group) = env.market_state();
    let mark = fresh_group.assets[asset_index as usize].effective_price;
    if case.assert_sub_one_mark {
        assert!(
            mark < 1_000_000,
            "{} must exercise an inverted mark below 1.0, got {mark}",
            case.name
        );
    }
    if asset_index == 0 {
        assert_eq!(fresh_cfg.last_good_oracle_slot, 2);
        assert_eq!(fresh_cfg.hybrid_soft_stale_slots, 3);
        assert_eq!(fresh_cfg.mark_ewma_e6, mark);
    } else {
        let market_data = env.svm.get_account(&env.market).unwrap().data;
        let fresh_profile =
            state::read_asset_oracle_profile(&market_data, asset_index as usize).unwrap();
        assert_eq!(fresh_profile.last_good_oracle_slot, 2);
        assert_eq!(fresh_profile.hybrid_soft_stale_slots, 3);
        assert_eq!(fresh_profile.mark_ewma_e6, mark);
    }
    assert_eq!(
        fresh_group.assets[asset_index as usize].raw_oracle_target_price,
        mark
    );

    let long_owner = Keypair::new();
    let short_owner = if case.same_owner {
        None
    } else {
        Some(Keypair::new())
    };
    let short_owner_ref = short_owner.as_ref().unwrap_or(&long_owner);
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(short_owner_ref);
    let size_q = direction_sign
        .checked_mul(case.size_q_abs as i128)
        .expect("signed size");
    let notional = (mark as u128)
        .checked_mul(case.size_q_abs)
        .and_then(|v| v.checked_div(POS_SCALE))
        .expect("notional");
    let exact_im_deposit = notional
        .checked_mul(production_risk_params().initial_margin_bps as u128)
        .and_then(|v| v.checked_add(9_999))
        .and_then(|v| v.checked_div(10_000))
        .expect("deposit");
    let deposit_amount = case.fixed_deposit.unwrap_or(exact_im_deposit);
    env.deposit(&long_owner, long_account, deposit_amount);
    env.deposit(short_owner_ref, short_account, deposit_amount);

    let direction = if size_q > 0 { "long" } else { "short" };
    let open_cu = env
        .try_trade_asset_with_cu(
            asset_index,
            &long_owner,
            long_account,
            short_owner_ref,
            short_account,
            size_q,
            mark,
            0,
        )
        .unwrap_or_else(|err| {
            panic!(
                "production-risk fresh HybridMark {} asset[{asset_index}] {direction}-open failed at mark={mark}, deposit={deposit_amount}: {err}",
                case.name
            )
        });
    assert_cu_within(
        "HybridMark production-risk fresh open",
        open_cu,
        TRADE_CU_LIMIT,
    );
    let (_, opened_group) = env.market_state();
    assert_eq!(
        opened_group.assets[asset_index as usize].oi_eff_long_q,
        size_q.unsigned_abs()
    );
    assert_eq!(
        opened_group.assets[asset_index as usize].oi_eff_short_q,
        size_q.unsigned_abs()
    );

    let close_cu = env
        .try_trade_asset_with_cu(
            asset_index,
            &long_owner,
            long_account,
            short_owner_ref,
            short_account,
            -size_q,
            mark,
            0,
        )
        .unwrap_or_else(|err| {
            panic!(
                "production-risk fresh HybridMark {} asset[{asset_index}] {direction}-close failed at mark={mark}, deposit={deposit_amount}: {err}",
                case.name
            )
        });
    assert_cu_within(
        "HybridMark production-risk fresh close",
        close_cu,
        TRADE_CU_LIMIT,
    );
    let (_, flat_group) = env.market_state();
    assert_eq!(flat_group.assets[asset_index as usize].oi_eff_long_q, 0);
    assert_eq!(flat_group.assets[asset_index as usize].oi_eff_short_q, 0);
}
#[derive(Clone, Copy, Debug)]
enum BackingResidualCounterTradePath {
    TradeNoCpi,
    TradeCpi,
    BatchTradeNoCpi,
    BatchTradeCpi,
}

#[derive(Clone, Copy, Debug)]
enum AccountResidualCounterTradePath {
    TradeNoCpi,
    TradeCpi,
    BatchTradeNoCpi,
    BatchTradeCpi,
}

fn auth_matcher_for_lp_via_system_create(
    env: &mut V16CuEnv,
    lp_owner: &Keypair,
    lp_account: Pubkey,
) -> (Pubkey, Pubkey, Pubkey) {
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (ctx, delegate, _) =
        env.init_auth_matcher_context_via_system_create(matcher_program, lp_owner, lp_account);
    (matcher_program, ctx, delegate)
}

#[allow(clippy::too_many_arguments)]
fn execute_account_residual_counter_trade_path(
    env: &mut V16CuEnv,
    path: AccountResidualCounterTradePath,
    taker_owner: &Keypair,
    taker_account: Pubkey,
    lp_owner: &Keypair,
    lp_account: Pubkey,
    size_q: i128,
    exec_price: u64,
) -> u64 {
    env.svm.expire_blockhash();
    match path {
        AccountResidualCounterTradePath::TradeNoCpi => env.trade_asset_with_cu(
            0,
            taker_owner,
            taker_account,
            lp_owner,
            lp_account,
            size_q,
            exec_price,
            0,
        ),
        AccountResidualCounterTradePath::TradeCpi => {
            let (matcher_program, ctx, delegate) =
                auth_matcher_for_lp_via_system_create(env, lp_owner, lp_account);
            env.trade_cpi_with_cu_on_asset(
                taker_owner,
                taker_account,
                lp_owner,
                lp_account,
                matcher_program,
                ctx,
                delegate,
                0,
                size_q,
                0,
            )
        }
        AccountResidualCounterTradePath::BatchTradeNoCpi => env
            .send(
                env.batch_trade_no_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id((0) as u16),
                        size_q,
                        exec_price,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(lp_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                ],
                &[taker_owner, lp_owner],
            )
            .expect("BatchTradeNoCpi account residual-counter trade"),
        AccountResidualCounterTradePath::BatchTradeCpi => {
            let (matcher_program, ctx, delegate) =
                auth_matcher_for_lp_via_system_create(env, lp_owner, lp_account);
            env.send(
                env.batch_trade_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id((0) as u16),
                        size_q,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                    AccountMeta::new_readonly(matcher_program, false),
                    AccountMeta::new(ctx, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[taker_owner],
            )
            .expect("BatchTradeCpi account residual-counter trade")
        }
    }
}

fn run_account_residual_counter_credit_case(
    path: AccountResidualCounterTradePath,
    size_q: i128,
    crystallized_loss_atoms: u128,
    expected_credit: u128,
) {
    const PRICE: u64 = 1_000;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 500, 500, 24);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker_account = env.create_portfolio(&taker_owner);
    let lp_account = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker_account, 10_000);
    env.deposit(&lp_owner, lp_account, 10_000);

    let taker_initial = env.portfolio_state(taker_account);
    let lp_initial = env.portfolio_state(lp_account);
    assert_eq!(
        taker_initial.residual_crystallized_loss_atoms_total.get(),
        0
    );
    assert_eq!(taker_initial.residual_spent_principal_atoms_total.get(), 0);
    assert_eq!(taker_initial.residual_received_atoms_total.get(), 0);
    assert_eq!(lp_initial.residual_received_atoms_total.get(), 0);

    env.set_residual_reward_counters_for_test(taker_account, crystallized_loss_atoms, 0, 0);
    let cu = execute_account_residual_counter_trade_path(
        &mut env,
        path,
        &taker_owner,
        taker_account,
        &lp_owner,
        lp_account,
        size_q,
        PRICE,
    );
    assert_cu_within(
        &format!("{path:?} account residual-counter trade"),
        cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let taker_after = env.portfolio_state(taker_account);
    let lp_after = env.portfolio_state(lp_account);
    assert_eq!(
        taker_after.residual_crystallized_loss_atoms_total.get(),
        crystallized_loss_atoms,
        "{path:?}: trading consumes reward budget but never reduces crystallized loss"
    );
    assert_eq!(
        taker_after.residual_spent_principal_atoms_total.get(),
        expected_credit,
        "{path:?}: taker spends only real principal from the residual budget"
    );
    assert_eq!(
        taker_after.residual_received_atoms_total.get(),
        0,
        "{path:?}: the source trader does not self-credit LP rewards"
    );
    assert_eq!(
        lp_after.residual_received_atoms_total.get(),
        expected_credit,
        "{path:?}: LP receives the deterministic residual credit"
    );
    assert_eq!(
        lp_after.residual_spent_principal_atoms_total.get(),
        0,
        "{path:?}: passive LP did not spend its own residual budget"
    );
    assert_ne!(
        lp_after.residual_received_atoms_total.get(),
        PRICE as u128,
        "{path:?}: counter must not credit leveraged notional"
    );
}
fn auth_matcher_for_lp(
    env: &mut V16CuEnv,
    lp_owner: &Keypair,
    lp_account: Pubkey,
) -> (Pubkey, Pubkey, Pubkey) {
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, lp_owner, lp_account);
    (matcher_program, ctx, delegate)
}

#[allow(clippy::too_many_arguments)]
fn execute_backing_residual_counter_trade_path(
    env: &mut V16CuEnv,
    path: BackingResidualCounterTradePath,
    taker_owner: &Keypair,
    taker_account: Pubkey,
    lp_owner: &Keypair,
    lp_account: Pubkey,
    size_q: i128,
    exec_price: u64,
) -> u64 {
    match path {
        BackingResidualCounterTradePath::TradeNoCpi => env.trade_asset_with_cu(
            0,
            taker_owner,
            taker_account,
            lp_owner,
            lp_account,
            size_q,
            exec_price,
            0,
        ),
        BackingResidualCounterTradePath::TradeCpi => {
            let (matcher_program, ctx, delegate) = auth_matcher_for_lp(env, lp_owner, lp_account);
            env.trade_cpi_with_cu_on_asset(
                taker_owner,
                taker_account,
                lp_owner,
                lp_account,
                matcher_program,
                ctx,
                delegate,
                0,
                size_q,
                0,
            )
        }
        BackingResidualCounterTradePath::BatchTradeNoCpi => env
            .send(
                env.batch_trade_no_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id((0) as u16),
                        size_q,
                        exec_price,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(lp_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                ],
                &[taker_owner, lp_owner],
            )
            .expect("BatchTradeNoCpi residual-counter trade"),
        BackingResidualCounterTradePath::BatchTradeCpi => {
            let (matcher_program, ctx, delegate) = auth_matcher_for_lp(env, lp_owner, lp_account);
            env.send(
                env.batch_trade_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id((0) as u16),
                        size_q,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                    AccountMeta::new_readonly(matcher_program, false),
                    AccountMeta::new(ctx, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[taker_owner],
            )
            .expect("BatchTradeCpi residual-counter trade")
        }
    }
}

fn run_backing_residual_counter_trade_path_case(path: BackingResidualCounterTradePath) {
    const INITIAL_PRICE: u64 = 100;
    const MARK_AFTER_MOVE: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const EXPECTED_PNL: u128 = 100;
    const WINNING_DOMAIN: u8 = 1;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, WINNING_DOMAIN.into(), 150, 10);

    let read_ledger = |env: &V16CuEnv| {
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap()
    };
    let start = read_ledger(&env).residual_received_atoms();
    assert_eq!(start, 0, "{path:?} farm reward snapshot starts at zero");

    let winner_owner = Keypair::new();
    let loser_owner = Keypair::new();
    let winner_account = env.create_portfolio(&winner_owner);
    let loser_account = env.create_portfolio(&loser_owner);
    env.deposit(&winner_owner, winner_account, 1_000);
    env.deposit(&loser_owner, loser_account, 1_000);

    let open_cu = execute_backing_residual_counter_trade_path(
        &mut env,
        path,
        &winner_owner,
        winner_account,
        &loser_owner,
        loser_account,
        SIZE_Q,
        INITIAL_PRICE,
    );
    assert_cu_within(
        &format!("{path:?} residual-counter open"),
        open_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, MARK_AFTER_MOVE);
    for account in [loser_account, winner_account] {
        env.crank(
            account,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
    }

    let winner_before_convert = env.portfolio_state(winner_account);
    assert_eq!(
        winner_before_convert.pnl.get(),
        EXPECTED_PNL as i128,
        "{path:?} setup must produce the source-backed positive PnL through the trade path"
    );
    assert!(
        has_active_leg_for_asset(&winner_before_convert, 0),
        "{path:?} positive claim must exist while the real trade leg remains open"
    );
    let (_, before_convert_group) = env.market_state();
    assert_eq!(
        before_convert_group.source_credit[WINNING_DOMAIN as usize].positive_claim_bound_num,
        EXPECTED_PNL * BOUND_SCALE,
        "{path:?} setup must reserve the positive claim in the winning backing domain"
    );
    assert_eq!(
        read_ledger(&env).residual_received_atoms(),
        0,
        "{path:?} trade and crank alone must not move the farm counter before ledger sync"
    );

    env.svm.expire_blockhash();
    let close_cu = execute_backing_residual_counter_trade_path(
        &mut env,
        path,
        &winner_owner,
        winner_account,
        &loser_owner,
        loser_account,
        -SIZE_Q,
        MARK_AFTER_MOVE,
    );
    assert_cu_within(
        &format!("{path:?} residual-counter close"),
        close_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    let winner_after_close = env.portfolio_state(winner_account);
    assert!(
        !has_active_leg_for_asset(&winner_after_close, 0),
        "{path:?} close path must release the PnL by flattening the real trade leg"
    );
    assert_eq!(
        winner_after_close.pnl.get(),
        EXPECTED_PNL as i128,
        "{path:?} close path must preserve the source-backed PnL before conversion"
    );

    let convert_cu = env.convert_released_pnl_with_cu(&winner_owner, winner_account, EXPECTED_PNL);
    assert_cu_within(
        &format!("{path:?} residual-counter convert"),
        convert_cu,
        CUSTODY_CU_LIMIT,
    );
    let winner_after_convert = env.portfolio_state(winner_account);
    assert_eq!(
        winner_after_convert.capital.get(),
        1_000 + EXPECTED_PNL,
        "{path:?} backed released PnL converts into senior capital"
    );
    let (_, after_convert_group) = env.market_state();
    assert_eq!(
        after_convert_group.source_backing_buckets[WINNING_DOMAIN as usize]
            .consumed_liened_backing_num,
        EXPECTED_PNL * BOUND_SCALE,
        "{path:?} converted PnL consumes exactly the source backing principal"
    );
    assert_eq!(
        read_ledger(&env).residual_received_atoms(),
        0,
        "{path:?} farm counter remains snapshot-gated until SyncBackingDomainLedger"
    );

    env.sync_backing_domain_ledger_with_cu(ledger, WINNING_DOMAIN.into());
    let synced = read_ledger(&env);
    assert_eq!(
        synced.residual_received_atoms(),
        EXPECTED_PNL,
        "{path:?} residual reward counter tracks the trade-sourced backing loss"
    );
    assert_eq!(
        synced.residual_received_delta_since(start).unwrap(),
        EXPECTED_PNL,
        "{path:?} farm start/end reward delta is deterministic"
    );
    assert_eq!(
        synced.residual_recovered_atoms(),
        0,
        "{path:?} conversion is a rewardable residual receive, not a recovery"
    );

    let (refill_source, refill_cu) = env.top_up_backing_bucket_with_ledger_with_cu(
        ledger,
        WINNING_DOMAIN.into(),
        EXPECTED_PNL,
        10,
    );
    assert_cu_within(
        &format!("{path:?} residual-counter refill"),
        refill_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(refill_source), 0);
    env.svm.expire_blockhash();
    env.sync_backing_domain_ledger_with_cu(ledger, WINNING_DOMAIN.into());
    let recovered = read_ledger(&env);
    assert_eq!(
        recovered.residual_received_atoms(),
        EXPECTED_PNL,
        "{path:?} provider refill cannot erase previously realized loss"
    );
    assert_eq!(
        recovered.residual_recovered_atoms(),
        EXPECTED_PNL,
        "{path:?} provider refill records the exact public recovery delta"
    );
    assert_eq!(
        recovered.last_observed_unavailable_principal_atoms, 0,
        "{path:?} full refill clears the observed unavailable principal"
    );
    let (_, recovered_group) = env.market_state();
    assert_eq!(
        recovered_group.source_backing_buckets[WINNING_DOMAIN as usize].consumed_liened_backing_num,
        0,
        "{path:?} public refill clears the consumed backing receivable"
    );
    assert_eq!(
        recovered_group.source_credit[WINNING_DOMAIN as usize].provider_receivable_num, 0,
        "{path:?} source and provider recovery mirrors stay exact"
    );
}
// security.md sweep — F-TRADENOCPI-FEE fix regression: the TradeNoCpi fee is now billed on the asset
// mark (effective_price), so the caller-supplied exec_price can no longer be gamed to under-pay fees.
#[derive(Clone, Copy, Debug)]
enum NoCpiReportedPricePath {
    Single,
    Batch,
}

#[allow(clippy::too_many_arguments)]
fn try_no_cpi_reported_price_trade_with_cu(
    env: &mut V16CuEnv,
    path: NoCpiReportedPricePath,
    owner_a: &Keypair,
    account_a: Pubkey,
    owner_b: &Keypair,
    account_b: Pubkey,
    size_q: i128,
    exec_price: u64,
    fee_bps: u64,
) -> Result<u64, String> {
    match path {
        NoCpiReportedPricePath::Single => env.try_trade_asset_with_cu(
            0, owner_a, account_a, owner_b, account_b, size_q, exec_price, fee_bps,
        ),
        NoCpiReportedPricePath::Batch => env.send(
            env.batch_trade_no_cpi_ix(
                account_a,
                account_b,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q,
                    exec_price,
                    fee_bps,
                }],
            ),
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[owner_a, owner_b],
        ),
    }
}

fn funded_no_cpi_reported_price_pair(
    env: &mut V16CuEnv,
    deposit: u128,
) -> (Keypair, Pubkey, Keypair, Pubkey) {
    let owner_a = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let owner_b = Keypair::new();
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, deposit);
    env.deposit(&owner_b, account_b, deposit);
    (owner_a, account_a, owner_b, account_b)
}

fn assert_zero_reported_price_rejects_atomically(
    mut env: V16CuEnv,
    path: NoCpiReportedPricePath,
    label: &str,
) {
    let (owner_a, account_a, owner_b, account_b) =
        funded_no_cpi_reported_price_pair(&mut env, 10_000_000_000);
    let (_, group_before) = env.market_state();
    let mark = group_before.assets[0].effective_price;
    assert_eq!(mark, 1_000_000, "{label}: setup must use the intended mark");
    let market_before = env.svm.get_account(&env.market).unwrap();
    let account_a_before = env.svm.get_account(&account_a).unwrap();
    let account_b_before = env.svm.get_account(&account_b).unwrap();

    env.svm.expire_blockhash();
    let rejected = try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        POS_SCALE as i128,
        0,
        0,
    );
    assert!(
        rejected.is_err(),
        "{label} {path:?}: zero reported exec_price must reject before it can drive the mark"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "{label} {path:?}: rejected zero-price trade must leave market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&account_a).unwrap(),
        account_a_before,
        "{label} {path:?}: rejected zero-price trade must leave account_a unchanged"
    );
    assert_eq!(
        env.svm.get_account(&account_b).unwrap(),
        account_b_before,
        "{label} {path:?}: rejected zero-price trade must leave account_b unchanged"
    );

    env.svm.expire_blockhash();
    let control = try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        POS_SCALE as i128,
        mark,
        0,
    );
    assert!(
        control.is_ok(),
        "{label} {path:?}: at-mark no-CPI control trade must remain live: {control:?}"
    );
}

fn zero_reported_price_ewma_env() -> V16CuEnv {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_price_move_bps_per_slot: 50,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, 1_000_000, 1, 0);
    env.svm.warp_to_slot(2);
    env
}

fn zero_reported_price_hybrid_after_hours_env() -> V16CuEnv {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_price_move_bps_per_slot: 50,
        ..V16CuMarketParams::default()
    });
    set_test_clock(&mut env, 1, 100);
    let feed = [0x4du8; 32];
    let pyth = env.set_pyth_price(&feed, 1_000_000, -6, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0u8; 32], [0u8; 32]],
        &[pyth],
        1,
        100,
        0,
        0,
        1,
        0,
    )
    .expect("configure hybrid oracle");
    set_test_clock(&mut env, 2, 101);
    env
}

// No-CPI reported-price manipulation: fees are already pinned to the mark, but the original
// caller-supplied exec_price still feeds EWMA discovery in EWMA and stale-hybrid modes. A zero
fn ewma_no_cpi_fee_and_mark_for_reported_price(
    path: NoCpiReportedPricePath,
    reported_price: u64,
) -> (u128, u64) {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const TRADE_SLOT: u64 = 5;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_price_move_bps_per_slot: CAP_BPS,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        trade_fee_base_bps: 100,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, 0);
    let (_, configured_group) = env.market_state();
    assert_eq!(configured_group.assets[0].effective_price, MARK);
    assert_eq!(configured_group.assets[0].slot_last, 1);

    env.svm.warp_to_slot(TRADE_SLOT);
    let (owner_a, account_a, owner_b, account_b) =
        funded_no_cpi_reported_price_pair(&mut env, 3_000_000_000);
    let insurance_before = env.market_state().1.insurance;
    let size_q = (1000u128 * POS_SCALE) as i128;
    env.svm.expire_blockhash();
    try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        size_q,
        reported_price,
        100,
    )
    .unwrap_or_else(|err| {
        panic!("{path:?}: no-CPI EWMA trade with reported_price={reported_price} failed: {err}")
    });
    let (cfg, group) = env.market_state();
    (group.insurance - insurance_before, cfg.mark_ewma_e6)
}

// No-CPI reported-price manipulation follow-up: an epsilon reported price is valid, so rejecting
// zero is insufficient. The wrapper must first bound the reported print to the same per-asset dt
// envelope the engine will accept, then use that accepted print consistently for fee notional,
// mark-movement fees, and EWMA movement. Otherwise epsilon can either under-pay fees or drive a
fn assert_no_cpi_tiny_exit_accepts_extreme_reported_price(
    path: NoCpiReportedPricePath,
    reported_price: u64,
) {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const OPEN_Q: i128 = (1000u128 * POS_SCALE) as i128;
    const CLOSE_Q: i128 = -1;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_price_move_bps_per_slot: CAP_BPS,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, 0);
    env.svm.warp_to_slot(5);
    let (owner_a, account_a, owner_b, account_b) =
        funded_no_cpi_reported_price_pair(&mut env, 10_000_000_000_000);

    env.svm.expire_blockhash();
    try_no_cpi_reported_price_trade_with_cu(
        &mut env, path, &owner_a, account_a, &owner_b, account_b, OPEN_Q, MARK, 0,
    )
    .unwrap_or_else(|err| panic!("{path:?}: setup open at mark failed: {err}"));
    let (_, opened_group) = env.market_state();
    let long_before = opened_group.assets[0].oi_eff_long_q;
    let short_before = opened_group.assets[0].oi_eff_short_q;
    let insurance_before_exit = opened_group.insurance;
    let mark_before_exit = env.market_state().0.mark_ewma_e6;
    assert_eq!(long_before, OPEN_Q.unsigned_abs());
    assert_eq!(short_before, OPEN_Q.unsigned_abs());

    env.svm.expire_blockhash();
    let exit = try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        CLOSE_Q,
        reported_price,
        0,
    );
    assert!(
        exit.is_ok(),
        "{path:?}: risk-reducing exit must not be DoSed by valid reported_price={reported_price}: {exit:?}"
    );
    let (closed_cfg, closed_group) = env.market_state();
    assert_eq!(
        closed_group.assets[0].oi_eff_long_q,
        long_before - 1,
        "{path:?}: long OI reduced by the close"
    );
    assert_eq!(
        closed_group.assets[0].oi_eff_short_q,
        short_before - 1,
        "{path:?}: short OI reduced by the close"
    );
    let close_fee_paid = closed_group.insurance - insurance_before_exit;
    assert_eq!(
        close_fee_paid, 0,
        "{path:?}: setup must exercise a minimum-quantum close whose fee rounds below one atom"
    );
    let externality_notional = 2u128
        .checked_mul(long_before)
        .and_then(|v| v.checked_mul(MARK as u128))
        .and_then(|v| v.checked_div(POS_SCALE))
        .expect("externality notional");
    let paid_move_bps = close_fee_paid * 10_000 / externality_notional;
    let mark_move_bps =
        percolator_prog::policy_v16::price_move_bps_ceil(mark_before_exit, closed_cfg.mark_ewma_e6)
            .expect("mark move bps");
    assert!(
        mark_move_bps <= paid_move_bps as u64,
        "{path:?}: EWMA close move ({mark_move_bps} bps) must be covered by paid fee ({paid_move_bps} bps)"
    );
}

fn assert_no_cpi_tiny_open_accepts_extreme_reported_price(
    path: NoCpiReportedPricePath,
    reported_price: u64,
) {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const OPEN_Q: i128 = (1000u128 * POS_SCALE) as i128;
    const TINY_Q: i128 = 1;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_price_move_bps_per_slot: CAP_BPS,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, 0);
    env.svm.warp_to_slot(5);
    let (owner_a, account_a, owner_b, account_b) =
        funded_no_cpi_reported_price_pair(&mut env, 10_000_000_000_000);

    env.svm.expire_blockhash();
    try_no_cpi_reported_price_trade_with_cu(
        &mut env, path, &owner_a, account_a, &owner_b, account_b, OPEN_Q, MARK, 0,
    )
    .unwrap_or_else(|err| panic!("{path:?}: setup open at mark failed: {err}"));
    let (_, opened_group) = env.market_state();
    let long_before = opened_group.assets[0].oi_eff_long_q;
    let short_before = opened_group.assets[0].oi_eff_short_q;
    let insurance_before_tiny = opened_group.insurance;
    let mark_before_tiny = env.market_state().0.mark_ewma_e6;

    let (owner_c, account_c, owner_d, account_d) =
        funded_no_cpi_reported_price_pair(&mut env, 10_000_000_000_000);
    env.svm.expire_blockhash();
    let tiny_open = try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &owner_c,
        account_c,
        &owner_d,
        account_d,
        TINY_Q,
        reported_price,
        0,
    );
    assert!(
        tiny_open.is_ok(),
        "{path:?}: tiny open must not be DoSed by valid reported_price={reported_price}: {tiny_open:?}"
    );
    let (final_cfg, final_group) = env.market_state();
    assert_eq!(
        final_group.assets[0].oi_eff_long_q,
        long_before + 1,
        "{path:?}: long OI increased by the tiny open"
    );
    assert_eq!(
        final_group.assets[0].oi_eff_short_q,
        short_before + 1,
        "{path:?}: short OI increased by the tiny open"
    );
    let tiny_fee_paid = final_group.insurance - insurance_before_tiny;
    assert_eq!(
        tiny_fee_paid, 0,
        "{path:?}: setup must exercise a minimum-quantum open whose fee rounds below one atom"
    );
    let externality_notional = 2u128
        .checked_mul(long_before)
        .and_then(|v| v.checked_mul(MARK as u128))
        .and_then(|v| v.checked_div(POS_SCALE))
        .expect("externality notional");
    let paid_move_bps = tiny_fee_paid * 10_000 / externality_notional;
    let mark_move_bps =
        percolator_prog::policy_v16::price_move_bps_ceil(mark_before_tiny, final_cfg.mark_ewma_e6)
            .expect("mark move bps");
    assert!(
        mark_move_bps <= paid_move_bps as u64,
        "{path:?}: EWMA open move ({mark_move_bps} bps) must be covered by paid fee ({paid_move_bps} bps)"
    );
}

// Trade liveness: reported no-CPI prices are adversarial mark-discovery inputs, not a reason to
// reject valid trades. At the minimum representable position quantum, fees round below one base
fn assert_no_cpi_extreme_reported_price_caps_paid_ewma_move(
    path: NoCpiReportedPricePath,
    reported_price: u64,
) {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const MAX_FEE_BPS: u64 = 37;
    const TRADE_SLOT: u64 = 5;
    const SIZE_Q: i128 = (1000u128 * POS_SCALE) as i128;

    let accepted_price = oracle_v16::clamp_toward_engine_dt(MARK, reported_price, CAP_BPS, 4);
    let candidate_mark =
        percolator_prog::policy_v16::ewma_update(MARK, accepted_price, 1, 1, TRADE_SLOT, 0, 0);
    let candidate_move_bps = percolator_prog::policy_v16::price_move_bps_ceil(MARK, candidate_mark)
        .expect("candidate move bps");
    assert!(
        candidate_move_bps > MAX_FEE_BPS,
        "{path:?}: setup must make the unclamped EWMA candidate exceed the market fee cap"
    );

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_trading_fee_bps: MAX_FEE_BPS,
        max_price_move_bps_per_slot: CAP_BPS,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, 0);
    env.svm.warp_to_slot(TRADE_SLOT);
    let (owner_a, account_a, owner_b, account_b) =
        funded_no_cpi_reported_price_pair(&mut env, 3_000_000_000);
    let insurance_before = env.market_state().1.insurance;

    env.svm.expire_blockhash();
    let trade = try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        SIZE_Q,
        reported_price,
        0,
    );
    assert!(
        trade.is_ok(),
        "{path:?}: capped EWMA movement must not reject valid reported_price={reported_price}: {trade:?}"
    );

    let (cfg, group) = env.market_state();
    let mark_move_bps = percolator_prog::policy_v16::price_move_bps_ceil(MARK, cfg.mark_ewma_e6)
        .expect("actual mark move bps");
    assert_eq!(
        mark_move_bps, MAX_FEE_BPS,
        "{path:?}: EWMA movement should bind at the market fee cap, not at the full candidate move"
    );
    assert!(
        group.insurance > insurance_before,
        "{path:?}: capped EWMA movement still charges a fee"
    );
    let trade_notional = SIZE_Q.unsigned_abs() * accepted_price as u128 / POS_SCALE;
    let externality_notional = trade_notional * 2;
    let paid_move_bps = (group.insurance - insurance_before) * 10_000 / externality_notional;
    assert!(
        mark_move_bps <= paid_move_bps as u64,
        "{path:?}: capped EWMA move ({mark_move_bps} bps) must be paid for by fees ({paid_move_bps} bps)"
    );
}
fn grow_market_to_10m_with_high_active_asset(
    env: &mut V16CuEnv,
    n: usize,
    high_asset: usize,
    price: u64,
) -> usize {
    const SOLANA_MAX_ACCOUNT_DATA_LEN: usize = 10 * 1024 * 1024;

    env.configure_auth_mark_with_cu(1, price);
    let (_, g0) = env.market_state();
    assert_eq!(g0.config.max_market_slots, 1, "starts as a 1-asset market");
    assert_eq!(
        g0.assets[0].lifecycle,
        AssetLifecycleV16::Active,
        "asset 0 active after ConfigureAuthMark"
    );
    let template = g0.assets[0];

    let new_len = state::market_account_len_for_capacity(n).unwrap();
    let next_len = state::market_account_len_for_capacity(n + 1).unwrap();
    let small_len = state::market_account_len_for_capacity(1).unwrap();
    assert!(
        n > 5_000 && new_len <= SOLANA_MAX_ACCOUNT_DATA_LEN && next_len > SOLANA_MAX_ACCOUNT_DATA_LEN,
        "10 MiB market capacity should be >5,000 assets and maximal at N={n}: len={new_len}, next={next_len}"
    );
    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        assert_eq!(
            acct.data.len(),
            small_len,
            "market started at the 1-slot length"
        );
        acct.data.resize(new_len, 0u8);
        acct.lamports = acct.lamports.max(new_len as u64 * 10);
        env.svm.set_account(env.market, acct).unwrap();
    }

    let high_market_id = (high_asset as u64) + 1;
    env.mutate_market(|_cfg, group| {
        assert_eq!(group.assets.len(), n, "grown read yields N asset slots");
        assert_eq!(
            group.insurance_domain_budget.len(),
            2 * n,
            "per-domain Vecs sized to 2N"
        );
        group.config.max_market_slots = n as u32;
        group.next_market_id = (n as u64) + 1;

        let mut high = template;
        high.market_id = high_market_id;
        group.assets[high_asset] = high;

        let (ld, sd) = (2 * high_asset, 2 * high_asset + 1);
        group.source_backing_buckets[ld] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
        group.source_backing_buckets[sd] =
            percolator::BackingBucketV16::empty_for_market(high_market_id);
    });

    {
        let mut acct = env.svm.get_account(&env.market).unwrap();
        let profile0 = state::read_asset_oracle_profile(&acct.data, 0).unwrap();
        state::write_asset_oracle_profile(&mut acct.data, high_asset, &profile0).unwrap();
        env.svm.set_account(env.market, acct).unwrap();
    }

    let (_, g) = env.market_state();
    assert_eq!(g.config.max_market_slots as usize, n);
    assert_eq!(g.assets.len(), n);
    assert_eq!(g.assets[high_asset].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(g.assets[high_asset].effective_price, price);
    assert_eq!(g.assets[high_asset].market_id, high_market_id);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data.len(),
        new_len
    );
    new_len
}
fn hostile_matcher_program_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures/hostile_matcher/target/deploy/hostile_matcher.so");
    assert!(
        path.exists(),
        "build the hostile matcher: cargo build-sbf in {path:?}"
    );
    path
}
// LoF/DoS sweep - RebalanceReduce is an owner exit route, not just a recovery helper. A user with
// a high-index position in the largest market account that fits LiteSVM must be able to reduce risk
// under bounded CU, without scanning the whole 10MiB slab or requiring a counterparty signature.

// LoF/DoS sweep (PR135): a permissionless non-base asset can become locally stale while
// the base market remains fresh. That must freeze new risk on the stale asset, but not
// brick cleanup: marketauth must still be able to move the asset to Recovery, and the
// delayed public force-close path must wind down the abandoned exposure.

// Public-interface DoS sweep: after the protected owner exit window, the public crank must be able
// to wind down a resolved account even if the owner no longer has a funded system account. The owner
// key is still the payout identity, but keeper liveness must not depend on that key being a signer or
// rent-funded once the owner-created SPL destination already exists.

// LoF/DoS sweep: after marketauth shuts an asset into Recovery, users still have the
// force-close delay window to exit voluntarily. The no-CPI routes are pinned above; this drives
// the real matcher CPI plumbing too, proving both single and batch CPI fills can close Recovery
// exposure before permissionless force-close matures.

// LoF/DoS sweep: the EWMA fee/externality math must stay live at large public notional,
// not only at small test sizes. A high-notional pair opens a large position, then one side
// exits a small slice with an extreme but valid reported price. The trade may clamp the
// internal mark, but it must not overflow wrapper fee math or block risk reduction.

// CU/DoS sweep: the crank wire decoder admits 16 observation hints, while the normal
// portfolio leg cap is 14. A market can still have more configured assets than any one
// portfolio can hold, so pin the true decode-cap observation-only crank path. This keeps
// the public keeper route bounded when it refreshes market slots beyond the active-leg cap.

// LoF/DoS sweep: public cranks may carry stale liquidation work budgets. If the
// target account no longer has liquidation work by the time the transaction
// lands, the supplied observations are still valid progress; close_q must not
// force a revert or pay a liquidation reward.

// LoF/DoS sweep (cron135): a soft-stale HybridMark crank may see a mixed oracle tail where an
// early leg is fresh but a later leg is stale. The public fallback path can make bounded EWMA
// progress, but it must not poison the oracle profile so a later fully fresh tail is unable to
// restore normal oracle progress.

#[derive(Clone, Copy, Debug)]
enum DrainOnlyCpiExitRoute {
    Single,
    Batch,
}

fn assert_drain_only_existing_risk_can_exit_through_cpi(route: DrainOnlyCpiExitRoute) {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let matcher_program = Pubkey::new_unique();
    env.svm.add_program(
        matcher_program,
        &std::fs::read(auth_matcher_program_path()).unwrap(),
    );
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &taker,
        taker_account,
        &lp,
        lp_account,
        POS_SCALE as i128,
        100,
        0,
    );
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);

    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
        0,
        0,
        0,
    );
    let (_, before) = env.market_state();
    assert_eq!(before.assets[0].lifecycle, AssetLifecycleV16::DrainOnly);
    assert_eq!(before.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(before.assets[0].oi_eff_short_q, POS_SCALE);

    env.svm.expire_blockhash();
    let exit = match route {
        DrainOnlyCpiExitRoute::Single => env.try_trade_cpi_with_cu_on_asset(
            &taker,
            taker_account,
            &lp,
            lp_account,
            matcher_program,
            ctx,
            delegate,
            0,
            -(POS_SCALE as i128),
            0,
        ),
        DrainOnlyCpiExitRoute::Batch => env.send(
            env.batch_trade_cpi_ix(
                taker_account,
                lp_account,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: before.assets[0].market_id,
                    size_q: -(POS_SCALE as i128),
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        ),
    };
    assert!(
        exit.is_ok(),
        "{route:?}: existing DrainOnly risk must be exit-able through CPI: {exit:?}"
    );

    let (_, after) = env.market_state();
    assert_eq!(
        after.assets[0].oi_eff_long_q, 0,
        "{route:?}: CPI exit reduced long OI"
    );
    assert_eq!(
        after.assets[0].oi_eff_short_q, 0,
        "{route:?}: CPI exit reduced short OI"
    );
    assert!(
        after.vault >= after.c_tot + after.insurance,
        "{route:?}: senior conservation after DrainOnly CPI exit"
    );
    assert_eq!(
        after.vault as u64,
        env.token_amount(env.vault),
        "{route:?}: accounting stays tied to SPL custody"
    );
}

// LoF/DoS sweep: BatchTradeNoCpi has its own final-margin engine path, so pin the
// exit-only lifecycle invariant there too. An oversized batch reduction must not
// cross through flat and reopen the opposite side on DrainOnly or Recovery assets,
// while an exact close must remain available under bounded CU.

// LoF/DoS sweep: a mixed BatchTradeNoCpi basket that combines one legal
// lifecycle exit with one illegal fresh open must be all-or-nothing. The valid
// reduction leg cannot be partially committed before the invalid lifecycle leg
// rejects, and the same reduction must remain executable by itself.

// LoF/DoS sweep: a market account can be pre-sized with spare asset-slot capacity while only a
// smaller configured prefix is live. Public routes must treat the spare slots as unconfigured,
// not as inactive tradable assets or oracle/crank targets, and CPI trade routes must reject before
// handing control to a hostile matcher.

// LoF/DoS sweep (PR135): local non-base stale clocks must not trap normal users in already-open
// exposure. New risk on the stale asset is blocked, but the owner-only reduce path should still
// let the user shrink or exit the stale local leg through the public wrapper.

// LoF/DoS sweep: a positive mark_min_fee must not become a liveness gate for otherwise valid
// trade-driven EWMA discovery. If the configured minimum is larger than the fee the trade can pay,
// the trade still lands, but the internal EWMA movement is reduced to the fee-supported amount.

// LoF/DoS sweep: UpdateBaseUnitMints rewires the terminal payout token rails. After a market has
// resolved but before users have closed out, that config write must reject while value is still
// custodied; otherwise the admin could strand CloseResolved on a fresh mint with no matching vault.

// LoF/DoS sweep: auto-crank selection must be overlap-safe at the public wrapper boundary.
// If an account is both B-stale and already certified liquidatable, a hostile keeper should not be
// able to steer the selector into liquidation by supplying a huge close_q and a reward tail. The
// selector must take the higher-priority B-settlement step, make bounded rank progress, and pay no
// liquidation reward.

// LoF/DoS sweep: enabling liquidation reward sharing must not make the public crank depend on a
// keeper-owned reward portfolio. A keeper that omits the optional reward tail should still liquidate
// the account; the whole fee is retained by insurance instead of blocking progress.

// LoF/DoS sweep: batch execution has two independent dimensions that can hide accounting drift:
// signed mixed-direction legs and post-engine per-leg fee reconstruction. The zero-fee mixed-spread
// test pins direction, and same-direction dust tests pin fee rounding; this pins the intersection on
// both public batch surfaces.

// LoF/DoS sweep (PR135): ForceCloseAbandonedAsset intentionally bypasses owner signatures after
// the recovery timeout, and unlike normal TradeNoCpi it does not run wrapper-side backing-fee
// collection. Pin the cross-product with source-backed positive PnL: a permissionless recovery
// force-close of another asset must remain liveness-safe, but it must not grow source-credit liens
// or silently bypass backing-fee accounting.

// LoF/DoS sweep (cron135): SyncMaintenanceFee is permissionless and can debit the target portfolio.
// A caller must not be able to charge or close a portfolio from market A through market B's fee rail,
// even when both markets use the same mint and market B has a valid vault.

// full-interface/CPI sweep: matcher tails are integration accounts, not a way to hand an external
// matcher arbitrary wrapper-owned market/portfolio state. Required-account aliasing is covered above;
// this pins the broader owner boundary for both CPI routes. The clean benign-tail control executes,
// while a wrapper-owned tail account rejects before the matcher can write its context.

#[derive(Clone, Copy, Debug)]
enum CpiEwmaTradePath {
    Single,
    Batch,
}

fn assert_cpi_matcher_price_caps_paid_ewma_move(path: CpiEwmaTradePath, size_q: i128) {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const MAX_FEE_BPS: u64 = 37;
    const TRADE_SLOT: u64 = 5;

    let raw_matcher_price = if size_q > 0 {
        MARK * 19 / 10
    } else {
        MARK / 10
    };
    let accepted_price = oracle_v16::clamp_toward_engine_dt(MARK, raw_matcher_price, CAP_BPS, 4);
    let candidate_mark =
        percolator_prog::policy_v16::ewma_update(MARK, accepted_price, 1, 1, TRADE_SLOT, 0, 0);
    let candidate_move_bps = percolator_prog::policy_v16::price_move_bps_ceil(MARK, candidate_mark)
        .expect("candidate move bps");
    assert!(
        candidate_move_bps > MAX_FEE_BPS,
        "{path:?}: setup must make the raw matcher print want more EWMA movement than the fee cap"
    );

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_trading_fee_bps: MAX_FEE_BPS,
        max_price_move_bps_per_slot: CAP_BPS,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, 0);
    env.svm.warp_to_slot(TRADE_SLOT);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp_owner = Keypair::new();
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 3_000_000_000);
    env.deposit(&lp_owner, lp, 3_000_000_000);
    let (matcher_ctx, matcher_delegate, _) = env
        .init_matcher_context_with_passive_spread_authorized(
            matcher_program,
            &lp_owner,
            lp,
            9_000,
            9_000,
        );
    let (_, market_before) = env.market_state();
    let insurance_before = market_before.insurance;
    let market_id = market_before.assets[0].market_id;

    env.svm.expire_blockhash();
    let trade = match path {
        CpiEwmaTradePath::Single => env.try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            matcher_program,
            matcher_ctx,
            matcher_delegate,
            0,
            size_q,
            0,
        ),
        CpiEwmaTradePath::Batch => env.send(
            env.batch_trade_cpi_ix(
                taker,
                lp,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id,
                    size_q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(matcher_ctx, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ],
            &[&taker_owner],
        ),
    };
    assert!(
        trade.is_ok(),
        "{path:?}: CPI fill with off-oracle matcher price must not DoS the trade: {trade:?}"
    );

    let (cfg, group) = env.market_state();
    let mark_move_bps = percolator_prog::policy_v16::price_move_bps_ceil(MARK, cfg.mark_ewma_e6)
        .expect("actual mark move bps");
    let size_abs = size_q.unsigned_abs();
    let trade_notional = size_abs
        .checked_mul(accepted_price as u128)
        .and_then(|num| num.checked_add(POS_SCALE - 1))
        .expect("trade notional numerator")
        / POS_SCALE;
    let externality_notional = trade_notional * 2;
    let paid_move_bps = (group.insurance - insurance_before) * 10_000 / externality_notional;
    assert!(
        mark_move_bps <= paid_move_bps as u64,
        "{path:?}: CPI EWMA move ({mark_move_bps} bps) must be covered by paid fee ({paid_move_bps} bps)"
    );
    assert_eq!(
        mark_move_bps, MAX_FEE_BPS,
        "{path:?}: CPI EWMA movement should bind at the market fee cap"
    );
    if size_abs == 1 {
        assert_eq!(
            group.insurance - insurance_before,
            2,
            "{path:?}: a minimum-quantum CPI fill must collect the one-atom fee ceiling from each side"
        );
    } else {
        assert!(
            group.insurance > insurance_before,
            "{path:?}: CPI EWMA movement must charge a fee"
        );
    }
    assert_eq!(group.assets[0].oi_eff_long_q, size_abs);
    assert_eq!(group.assets[0].oi_eff_short_q, size_abs);
}

// Public CPI matchers can legally return a full fill at an off-oracle price. That price is the same
// adversarial mark-discovery input as no-CPI exec_price, so the wrapper must clamp it to the engine's
// accepted dt envelope, cap the EWMA move to paid fee headroom, and still let the trade land.

fn assert_underfunded_ewma_exit_uses_collected_fee(path: NoCpiReportedPricePath) {
    const MARK: u64 = 1_000_000;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.configure_ewma_mark_with_cu(0, MARK, 1, 0);
    let (long_owner, long, short_owner, short) =
        funded_no_cpi_reported_price_pair(&mut env, MARK as u128);

    try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &long_owner,
        long,
        &short_owner,
        short,
        SIZE_Q,
        MARK,
        0,
    )
    .unwrap_or_else(|err| panic!("{path:?}: setup open failed: {err}"));

    // A legitimate oracle-authority update and public cranks leave the long healthy at the exact
    // maintenance boundary, but with less capital than the maximum fee on a full exit.
    env.svm.warp_to_slot(10);
    env.push_ewma_mark_with_cu(10, 1);
    env.svm.expire_blockhash();
    env.crank_steps_after_market_catchup(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.svm.expire_blockhash();
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );

    env.svm.warp_to_slot(20);
    let (cfg_before, group_before) = env.market_state();
    let reported_exit_price = group_before.assets[0]
        .effective_price
        .checked_mul(2)
        .expect("one-slot upper price envelope");
    let requested_fee_per_side = reported_exit_price as u128;
    let long_capital = env.portfolio_state(long).capital.get();
    assert!(
        0 < long_capital && long_capital < requested_fee_per_side,
        "{path:?}: setup must make one side's quoted fee partly uncollectible"
    );

    env.svm.expire_blockhash();
    let exit = try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &long_owner,
        long,
        &short_owner,
        short,
        -SIZE_Q,
        reported_exit_price,
        0,
    );
    assert!(
        exit.is_ok(),
        "{path:?}: an underfunded risk-reducing full exit must remain live: {exit:?}"
    );

    let (cfg_after, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].oi_eff_long_q, 0);
    assert_eq!(group_after.assets[0].oi_eff_short_q, 0);
    let collected_fee = group_after.insurance - group_before.insurance;
    let quoted_two_sided_fee = requested_fee_per_side * 2;
    assert!(
        collected_fee < quoted_two_sided_fee,
        "{path:?}: setup must exercise a real engine partial fee charge"
    );

    // The mark externality is priced against max(pre-trade OI notional, trade notional) on both
    // sides. A successful trade may collect less than quoted, but its EWMA move cannot consume the
    // uncollectible part as though it were paid.
    let mark_externality_notional = quoted_two_sided_fee;
    let paid_move_bps = collected_fee * 10_000 / mark_externality_notional;
    let mark_move_bps = percolator_prog::policy_v16::price_move_bps_ceil(
        cfg_before.mark_ewma_e6,
        cfg_after.mark_ewma_e6,
    )
    .expect("mark move bps");
    assert!(mark_move_bps > 0, "{path:?}: control must move the EWMA");
    assert!(
        mark_move_bps <= paid_move_bps as u64,
        "{path:?}: EWMA move {mark_move_bps} bps exceeds collected-fee support {paid_move_bps} bps"
    );
}

// Public-only regression for a fee-backed mark invariant. The engine intentionally allows a full
// risk-reducing exit to charge only available capital. Single TradeNoCpi used to move EWMA from the
// nominal quote anyway; BatchTradeNoCpi instead detected the short aggregate and reverted the exit.
// Both routes must execute, and both may move the mark only as far as their collected fees support.

fn assert_underfunded_cpi_ewma_exit_uses_collected_fee(path: CpiEwmaTradePath) {
    const MARK: u64 = 1_000_000;
    const ADVERSE_MARK: u64 = 1_999_999;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.configure_ewma_mark_with_cu(0, MARK, 1, 0);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp_owner = Keypair::new();
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, MARK as u128);
    env.deposit(&lp_owner, lp, MARK as u128);
    let (open_ctx, open_delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp_owner,
        lp,
        0,
        9_000,
    );

    env.svm.expire_blockhash();
    env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        open_ctx,
        open_delegate,
        0,
        -SIZE_Q,
        0,
    )
    .unwrap_or_else(|err| panic!("{path:?}: setup short open failed: {err}"));

    env.svm.warp_to_slot(10);
    env.push_ewma_mark_with_cu(10, ADVERSE_MARK);
    env.svm.expire_blockhash();
    env.crank_steps_after_market_catchup(
        taker,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.svm.expire_blockhash();
    env.crank(
        lp,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );

    let (exit_ctx, exit_delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp_owner,
        lp,
        9_000,
        9_000,
    );
    env.svm.warp_to_slot(20);
    let (cfg_before, group_before) = env.market_state();
    let expected_matcher_price = group_before.assets[0]
        .effective_price
        .checked_mul(19)
        .expect("matcher ask numerator")
        / 10;
    let accepted_exit_price = oracle_v16::clamp_toward_engine_dt(
        group_before.assets[0].effective_price,
        expected_matcher_price,
        10_000,
        1,
    );
    assert_eq!(
        accepted_exit_price, expected_matcher_price,
        "{path:?}: wide matcher ask must remain inside the one-segment engine envelope"
    );
    let requested_fee_per_side = accepted_exit_price as u128;
    let taker_capital = env.portfolio_state(taker).capital.get();
    assert!(
        0 < taker_capital && taker_capital < requested_fee_per_side,
        "{path:?}: setup must leave the adverse short unable to pay its quoted exit fee"
    );

    env.svm.expire_blockhash();
    let exit = match path {
        CpiEwmaTradePath::Single => env.try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            matcher_program,
            exit_ctx,
            exit_delegate,
            0,
            SIZE_Q,
            0,
        ),
        CpiEwmaTradePath::Batch => env.send(
            env.batch_trade_cpi_ix(
                taker,
                lp,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: group_before.assets[0].market_id,
                    size_q: SIZE_Q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(exit_ctx, false),
                AccountMeta::new_readonly(exit_delegate, false),
            ],
            &[&taker_owner],
        ),
    };
    assert!(
        exit.is_ok(),
        "{path:?}: an underfunded risk-reducing CPI exit must remain live: {exit:?}"
    );

    let (cfg_after, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].oi_eff_long_q, 0);
    assert_eq!(group_after.assets[0].oi_eff_short_q, 0);
    let collected_fee = group_after.insurance - group_before.insurance;
    let quoted_two_sided_fee = requested_fee_per_side * 2;
    assert!(
        collected_fee < quoted_two_sided_fee,
        "{path:?}: setup must exercise a real partial engine fee charge"
    );
    let mark_externality_notional = quoted_two_sided_fee;
    let paid_move_bps = collected_fee * 10_000 / mark_externality_notional;
    let mark_move_bps = percolator_prog::policy_v16::price_move_bps_ceil(
        cfg_before.mark_ewma_e6,
        cfg_after.mark_ewma_e6,
    )
    .expect("mark move bps");
    assert!(mark_move_bps > 0, "{path:?}: control must move the EWMA");
    assert!(
        mark_move_bps <= paid_move_bps as u64,
        "{path:?}: CPI EWMA move {mark_move_bps} bps exceeds collected-fee support {paid_move_bps} bps"
    );
}

// Max-shape market liveness: the admin must be able to enter terminal resolution without a
// whole-slab CU cliff, and the mode transition must not move or reclassify user value.

// Public-interface isolation sweep: a permissionless asset creator can move its own oracle at a
// later slot, which updates the engine's touched-asset slot summary. That must not make an unrelated
// open portfolio fee-current: SyncMaintenanceFee derives its safe anchor from the portfolio's live
// legs, so it leaves capital, insurance, and last_fee_slot unchanged until asset 0 is accrued. The
// affected users must also retain their signed risk-reducing exit path.

// Once an asset enters Recovery, its shutdown slot is the anchor for the bounded
// permissionless force-close deadline. A hostile oracle authority must not be able
// to refresh last_good_oracle_slot and move that deadline after users are trapped
// in the frozen asset.

// A permissionless asset's target update invalidates the market-wide oracle epoch. Even at the
// portfolio leg cap, an unrelated user can atomically refresh both sides and reduce exposure through
// an external matcher before the attacker can invalidate the certificates again.

// LoF/DoS sweep (PR135): shutdown cleanup has to remain permissionlessly live even when an
// abandoned account retains the maximum 2N public source-claim domains. This cross-product pins
// the two-account ForceCloseAbandonedAsset path, not just ordinary owner-signed source-lien trades.

// Terminal liveness/order independence with only public state transitions: two equal winners share
// one loser, all three flatten after an authenticated mark move, and resolution is driven in opposite
// close orders. A premature winner close must not change payouts or strand another account.

const MAX_SOURCE_LIVE_ASSETS: u16 = 14;
const MAX_SOURCE_LIVE_SIZE_Q: i128 = POS_SCALE as i128 * 1_000;

// An LP authorizes a matcher once, then an unrelated taker creates both source domains on every
// asset without the LP signing the fills. All but the final profitable LP leg are closed.
fn setup_max_source_live_pair(
    maintenance_fee_per_slot: u128,
    retained_active_assets: u16,
) -> (V16CuEnv, Keypair, Keypair, Pubkey, Pubkey, u64) {
    setup_max_source_live_pair_with_configured_assets(
        maintenance_fee_per_slot,
        retained_active_assets,
        MAX_SOURCE_LIVE_ASSETS,
        false,
        None,
    )
}

fn setup_max_source_live_pair_with_hybrid_oracles(
    feeds: [[u8; 32]; 3],
) -> (V16CuEnv, Keypair, Keypair, Pubkey, Pubkey, u64) {
    setup_max_source_live_pair_with_configured_assets(
        0,
        MAX_SOURCE_LIVE_ASSETS,
        MAX_SOURCE_LIVE_ASSETS,
        false,
        Some(feeds),
    )
}

fn setup_max_source_live_pair_with_spare_auth_mark_asset(
    maintenance_fee_per_slot: u128,
    retained_active_assets: u16,
) -> (V16CuEnv, Keypair, Keypair, Pubkey, Pubkey, u64) {
    setup_max_source_live_pair_with_configured_assets(
        maintenance_fee_per_slot,
        retained_active_assets,
        MAX_SOURCE_LIVE_ASSETS + 1,
        false,
        None,
    )
}

fn setup_max_source_live_pair_with_seeded_lien() -> (V16CuEnv, Keypair, Keypair, Pubkey, Pubkey, u64)
{
    setup_max_source_live_pair_with_configured_assets(
        0,
        MAX_SOURCE_LIVE_ASSETS,
        MAX_SOURCE_LIVE_ASSETS,
        true,
        None,
    )
}

fn setup_max_source_live_pair_with_configured_assets(
    maintenance_fee_per_slot: u128,
    retained_active_assets: u16,
    configured_auth_mark_assets: u16,
    seed_source_lien: bool,
    hybrid_feeds: Option<[[u8; 32]; 3]>,
) -> (V16CuEnv, Keypair, Keypair, Pubkey, Pubkey, u64) {
    setup_max_source_live_pair_with_configured_assets_and_capacity(
        maintenance_fee_per_slot,
        retained_active_assets,
        configured_auth_mark_assets,
        seed_source_lien,
        hybrid_feeds,
        70,
    )
}

fn setup_max_source_live_pair_with_configured_assets_and_capacity(
    maintenance_fee_per_slot: u128,
    retained_active_assets: u16,
    configured_auth_mark_assets: u16,
    seed_source_lien: bool,
    hybrid_feeds: Option<[[u8; 32]; 3]>,
    market_capacity: usize,
) -> (V16CuEnv, Keypair, Keypair, Pubkey, Pubkey, u64) {
    const ACTIVE_CAP: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const PRICE_LOW: u64 = 100;
    const PRICE_HIGH: u64 = 101;
    const HYBRID_PRICE_HIGH: u64 = 110;
    assert!(
        retained_active_assets > 0 && retained_active_assets <= ACTIVE_CAP,
        "fixture must retain between one and the public active-leg cap"
    );
    assert!(
        configured_auth_mark_assets >= MAX_SOURCE_LIVE_ASSETS,
        "fixture must configure at least the source-domain growth assets"
    );
    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: ACTIVE_CAP,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            maintenance_fee_per_slot,
            ..V16CuMarketParams::default()
        },
        market_capacity,
    );
    for asset_index in ACTIVE_CAP..configured_auth_mark_assets {
        let activation_slot = u64::from(asset_index - ACTIVE_CAP + 1);
        env.activate_asset(asset_index, activation_slot, PRICE_LOW);
    }
    let mut slot = u64::from(configured_auth_mark_assets - ACTIVE_CAP);
    if let Some(feeds) = hybrid_feeds {
        slot = slot.max(1);
        set_test_clock(&mut env, slot, 100 + slot as i64);
        let initial_oracles = [
            env.set_pyth_price(&feeds[0], 3_000_000, -6, 100 + slot as i64),
            env.set_pyth_price(&feeds[1], 150_000_000, -6, 100 + slot as i64),
            env.set_pyth_price(&feeds[2], 200_000_000, -6, 100 + slot as i64),
        ];
        for asset_index in 0..configured_auth_mark_assets {
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                asset_index,
                3,
                ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
                feeds,
                &initial_oracles,
                slot,
                100 + slot as i64,
                0,
                0,
                3,
                500,
            )
            .unwrap_or_else(|error| {
                panic!("configure max-source Hybrid asset {asset_index}: {error}")
            });
        }
    } else {
        env.svm.warp_to_slot(slot);
        for asset_index in 0..configured_auth_mark_assets {
            env.configure_auth_mark_for_asset_as_admin(asset_index, slot, PRICE_LOW);
        }
    }

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 2_000_000);
    env.deposit(&lp_owner, lp, 2_000_000);
    let (matcher_ctx, matcher_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
    if seed_source_lien {
        for domain in [0u16, 1] {
            env.top_up_backing_bucket(domain, 1_000, 1_000);
        }
    }

    let cpi_fill = |env: &mut V16CuEnv, asset_index: u16, size_q: i128| {
        env.svm.expire_blockhash();
        env.try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            matcher_program,
            matcher_ctx,
            matcher_delegate,
            asset_index,
            size_q,
            0,
        )
        .unwrap_or_else(|err| panic!("unsigned-LP asset {asset_index} fill failed: {err}"));
    };
    let cert_current = |env: &V16CuEnv, portfolio: Pubkey| {
        let group = env.market_state().1;
        let state = env.portfolio_state(portfolio);
        let cert = health_cert(&state);
        cert.valid
            && cert.cert_oracle_epoch == group.oracle_epoch
            && cert.cert_funding_epoch == group.funding_epoch
            && cert.cert_risk_epoch == group.risk_epoch
            && cert.cert_asset_set_epoch == group.asset_set_epoch
            && cert.active_bitmap_at_cert == active_bitmap(&state)
    };
    let drive_both_current = |env: &mut V16CuEnv, now_slot: u64| {
        for _ in 0..8 {
            for portfolio in [taker, lp] {
                if !cert_current(env, portfolio) {
                    env.crank(
                        portfolio,
                        ProgInstruction::PermissionlessCrank {
                            now_slot,
                            observations: vec![],
                        },
                    );
                }
            }
            if cert_current(env, taker) && cert_current(env, lp) {
                return;
            }
        }
        panic!("public cranks did not reach a two-account certificate fixed point");
    };
    let settle_both =
        |env: &mut V16CuEnv, asset_index: u16, now_slot: u64, oracle_accounts: &[Pubkey]| {
            if oracle_accounts.is_empty() {
                for account in [taker, lp] {
                    env.svm.expire_blockhash();
                    env.crank(
                        account,
                        ProgInstruction::PermissionlessCrank {
                            now_slot,
                            observations: crank_observations(asset_index),
                        },
                    );
                }
                drive_both_current(env, now_slot);
            } else {
                env.crank_with_oracle_tail(
                    taker,
                    ProgInstruction::PermissionlessCrank {
                        now_slot,
                        observations: crank_observations(asset_index),
                    },
                    oracle_accounts,
                );
                drive_both_current(env, now_slot);
            }
        };

    for asset_index in 0..MAX_SOURCE_LIVE_ASSETS {
        cpi_fill(&mut env, asset_index, -MAX_SOURCE_LIVE_SIZE_Q);
        slot += 1;
        let high_oracles = if let Some(feeds) = hybrid_feeds {
            let publish_time = 100 + slot as i64;
            set_test_clock(&mut env, slot, publish_time);
            vec![
                env.set_pyth_price(&feeds[0], 3_300_000, -6, publish_time),
                env.set_pyth_price(&feeds[1], 150_000_000, -6, publish_time),
                env.set_pyth_price(&feeds[2], 200_000_000, -6, publish_time),
            ]
        } else {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(asset_index, slot, PRICE_HIGH);
            vec![]
        };
        settle_both(&mut env, asset_index, slot, &high_oracles);
        if hybrid_feeds.is_some() {
            let asset = &env.market_state().1.assets[asset_index as usize];
            assert_eq!(asset.raw_oracle_target_price, HYBRID_PRICE_HIGH);
            assert_eq!(asset.effective_price, HYBRID_PRICE_HIGH);
        }
        cpi_fill(&mut env, asset_index, MAX_SOURCE_LIVE_SIZE_Q);

        cpi_fill(&mut env, asset_index, MAX_SOURCE_LIVE_SIZE_Q);
        slot += 1;
        let low_oracles = if let Some(feeds) = hybrid_feeds {
            let publish_time = 100 + slot as i64;
            set_test_clock(&mut env, slot, publish_time);
            vec![
                env.set_pyth_price(&feeds[0], 3_000_000, -6, publish_time),
                env.set_pyth_price(&feeds[1], 150_000_000, -6, publish_time),
                env.set_pyth_price(&feeds[2], 200_000_000, -6, publish_time),
            ]
        } else {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(asset_index, slot, PRICE_LOW);
            vec![]
        };
        settle_both(&mut env, asset_index, slot, &low_oracles);
        if hybrid_feeds.is_some() {
            let asset = &env.market_state().1.assets[asset_index as usize];
            assert_eq!(asset.raw_oracle_target_price, PRICE_LOW);
            assert_eq!(asset.effective_price, PRICE_LOW);
        }
        if asset_index < MAX_SOURCE_LIVE_ASSETS - retained_active_assets {
            cpi_fill(&mut env, asset_index, -MAX_SOURCE_LIVE_SIZE_Q);
        } else if hybrid_feeds.is_none() {
            for active_asset in (MAX_SOURCE_LIVE_ASSETS - retained_active_assets)..=asset_index {
                env.crank_if_actionable(
                    taker,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(active_asset),
                    },
                );
            }
            drive_both_current(&mut env, slot);
        } else {
            assert!(cert_current(&env, taker) && cert_current(&env, lp));
        }
        if seed_source_lien && asset_index == 0 {
            const SEEDED_LIEN_Q: i128 = 20 * POS_SCALE as i128;
            cpi_fill(&mut env, asset_index, -MAX_SOURCE_LIVE_SIZE_Q);
            let capital = env.portfolio_state(lp).capital.get();
            env.withdraw_with_cu(&lp_owner, lp, capital);
            cpi_fill(&mut env, asset_index, SEEDED_LIEN_Q);
            let seeded = env.portfolio_state(lp);
            let seeded_lien_domains = seeded
                .source_domains
                .iter()
                .filter(|source| source.source_claim_liened_num.get() != 0)
                .count();
            assert_eq!(
                seeded_lien_domains, 2,
                "public max-source fixture failed to seed both source-side liens"
            );
            assert_eq!(
                seeded
                    .source_domains
                    .iter()
                    .map(|source| source.source_lien_effective_reserved.get())
                    .sum::<u128>(),
                2_000,
                "two source-side liens must reserve the exact margin credit"
            );
            env.deposit(&lp_owner, lp, capital);
            drive_both_current(&mut env, slot);
        }
    }

    let lp_state = env.portfolio_state(lp);
    assert_eq!(
        lp_state
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        usize::from(MAX_SOURCE_LIVE_ASSETS) * 2
    );
    assert_eq!(
        active_leg_for_asset(&lp_state, usize::from(MAX_SOURCE_LIVE_ASSETS - 1)).basis_pos_q,
        -MAX_SOURCE_LIVE_SIZE_Q
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&lp_state)),
        u32::from(retained_active_assets)
    );
    assert_eq!(
        active_leg_for_asset(
            &env.portfolio_state(taker),
            usize::from(MAX_SOURCE_LIVE_ASSETS - 1)
        )
        .basis_pos_q,
        MAX_SOURCE_LIVE_SIZE_Q
    );
    (env, taker_owner, lp_owner, taker, lp, slot)
}

// Max-source liveness through a wrapper-specific owner route: the owner can still unilaterally
// reduce the final leg at the exact 32-domain public shape.

// Max-source liveness through a wrapper-specific two-account route: shutdown must leave an
// unrelated cranker enough CU to clear the final abandoned pair at the exact 32-domain shape.

// Compose the full active-leg and historical-source caps with asset Recovery. Even if the
// convenience pair force-close is too expensive at this shape, each affected owner must retain a
// bounded unilateral path that clears the shutdown leg without depending on its counterparty.

// Max-source liveness through the permissionless fee-currentness route used by eventual owner
// withdrawal and close. Exercise a real nonzero charge, not only a zero-fee refresh.

// Compose the full public active-leg cap with the larger 32-entry historical source cap. A normal
// bilateral reduction remains the bounded escape even though account-wide optional maintenance
// sync and unilateral convenience routes are more expensive at this shape.

// The unsigned LP can accrue every public source-domain slot and still reclaim principal without
// first converting its source-backed positive PnL. This owner exit must remain account-local and
// bounded independently of the separate source-claim conversion path.

// Adding collateral is the owner's unconditional cure primitive. It must remain executable at the
// full active-leg and historical-source cross-product without requiring account refresh first.

// The wrapper authenticates and accrues every supplied observation before asking the engine to select
// an account action. That means an unrelated empty portfolio can be the target that commits an exposed
// asset's mark. Exercise that wrapper-only ordering and prove it leaves each trader with a bounded
// no-observation refresh and the same principal/PnL/terminal payouts as cranking an exposed account first.

// A trade that lands after the backing provider's signed expiry must not create a
// new source lien and charge the trader for support that has already lapsed.

#[path = "invariants/cu/inv_002_asset_generation_binding.rs"]
mod inv_002_asset_generation_binding;

#[path = "invariants/cu/inv_003_portfolio_incarnation_binding.rs"]
mod inv_003_portfolio_incarnation_binding;

#[path = "invariants/cu/inv_004_position_episode_binding.rs"]
mod inv_004_position_episode_binding;

#[path = "invariants/cu/inv_005_authority_incarnation_binding.rs"]
mod inv_005_authority_incarnation_binding;

#[path = "invariants/cu/inv_007_no_aba_reuse.rs"]
mod inv_007_no_aba_reuse;

#[path = "invariants/cu/inv_008_intent_uniqueness_and_bounded_replay.rs"]
mod inv_008_intent_uniqueness_and_bounded_replay;

#[path = "invariants/cu/inv_009_partial_fill_and_retry_accounting.rs"]
mod inv_009_partial_fill_and_retry_accounting;

#[path = "invariants/cu/inv_010_out_of_order_safety.rs"]
mod inv_010_out_of_order_safety;

#[path = "invariants/cu/inv_011_signed_aggregate_economic_bounds.rs"]
mod inv_011_signed_aggregate_economic_bounds;

#[path = "invariants/cu/inv_012_capability_and_delegate_scope.rs"]
mod inv_012_capability_and_delegate_scope;

#[path = "invariants/cu/inv_013_destructive_consent_scope.rs"]
mod inv_013_destructive_consent_scope;

#[path = "invariants/cu/inv_014_delayed_policy_and_policy_epoch_safety.rs"]
mod inv_014_delayed_policy_and_policy_epoch_safety;

#[path = "invariants/cu/inv_015_account_ownership_layout_discriminator_and_length_validity.rs"]
mod inv_015_account_ownership_layout_discriminator_and_length_validity;

#[path = "invariants/cu/inv_016_canonical_pda_and_seed_binding.rs"]
mod inv_016_canonical_pda_and_seed_binding;

#[path = "invariants/cu/inv_017_signer_writable_role_and_account_alias_safety.rs"]
mod inv_017_signer_writable_role_and_account_alias_safety;

#[path = "invariants/cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs"]
mod inv_018_quote_mint_vault_token_program_and_authority_integrity;

#[path = "invariants/cu/inv_019_cpi_invocation_and_return_data_binding.rs"]
mod inv_019_cpi_invocation_and_return_data_binding;

#[path = "invariants/cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs"]
mod inv_020_authenticated_clock_slot_and_oracle_provenance;

#[path = "invariants/cu/inv_021_account_creation_reallocation_close_rent_and_lamport_safety.rs"]
mod inv_021_account_creation_reallocation_close_rent_and_lamport_safety;

#[path = "invariants/cu/inv_022_instruction_decoding_and_schema_upgrade_safety.rs"]
mod inv_022_instruction_decoding_and_schema_upgrade_safety;

#[path = "invariants/cu/inv_023_caller_input_confinement_for_derived_safety_state.rs"]
mod inv_023_caller_input_confinement_for_derived_safety_state;

#[path = "invariants/cu/inv_024_attributed_quote_value_conservation.rs"]
mod inv_024_attributed_quote_value_conservation;

#[path = "invariants/cu/inv_025_exact_stock_reconciliation.rs"]
mod inv_025_exact_stock_reconciliation;

#[path = "invariants/cu/inv_026_reservation_and_encumbrance_conservation_is_separate_from_token_value.rs"]
mod inv_026_reservation_and_encumbrance_conservation_is_separate_from_token_value;

#[path = "invariants/cu/inv_027_protected_principal_seniority.rs"]
mod inv_027_protected_principal_seniority;

#[path = "invariants/cu/inv_028_source_domain_realizability_cap.rs"]
mod inv_028_source_domain_realizability_cap;

#[path = "invariants/cu/inv_029_positive_claim_bounds_never_understate.rs"]
mod inv_029_positive_claim_bounds_never_understate;

#[path = "invariants/cu/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs"]
mod inv_030_credit_rate_determinism_and_fail_closed_behavior;

#[path = "invariants/cu/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs"]
mod inv_031_no_double_use_of_claim_backing_or_insurance_atoms;

#[path = "invariants/cu/inv_032_exact_counterparty_lien_lifecycle.rs"]
mod inv_032_exact_counterparty_lien_lifecycle;

#[path = "invariants/cu/inv_033_insurance_backed_lien_single_classification.rs"]
mod inv_033_insurance_backed_lien_single_classification;

#[path = "invariants/cu/inv_034_domain_and_instance_isolation.rs"]
mod inv_034_domain_and_instance_isolation;

#[path = "invariants/cu/inv_036_fee_destination_and_policy_version_integrity.rs"]
mod inv_036_fee_destination_and_policy_version_integrity;

#[path = "invariants/cu/inv_037_exact_residual_partition.rs"]
mod inv_037_exact_residual_partition;

#[path = "invariants/cu/inv_038_rounding_and_ratio_conservation.rs"]
mod inv_038_rounding_and_ratio_conservation;

#[path = "invariants/cu/inv_039_pending_loss_obligation_durability.rs"]
mod inv_039_pending_loss_obligation_durability;

#[path = "invariants/cu/inv_040_no_fee_seniority.rs"]
mod inv_040_no_fee_seniority;

#[path = "invariants/cu/inv_041_deterministic_allocation_and_caller_order_independence.rs"]
mod inv_041_deterministic_allocation_and_caller_order_independence;

#[path = "invariants/cu/inv_042_recovery_fallback_envelope.rs"]
mod inv_042_recovery_fallback_envelope;

#[path = "invariants/cu/inv_043_hedge_and_correlation_credit_envelope.rs"]
mod inv_043_hedge_and_correlation_credit_envelope;

#[path = "invariants/cu/inv_044_no_phantom_value_from_indices_certificates_or_labels.rs"]
mod inv_044_no_phantom_value_from_indices_certificates_or_labels;

#[path = "invariants/cu/inv_045_no_free_mark_movement.rs"]
mod inv_045_no_free_mark_movement;

#[path = "invariants/cu/inv_046_trade_availability_without_unsafe_mark_admission.rs"]
mod inv_046_trade_availability_without_unsafe_mark_admission;

#[path = "invariants/cu/inv_047_equivalent_route_semantics.rs"]
mod inv_047_equivalent_route_semantics;

#[path = "invariants/cu/inv_048_matched_trade_and_open_interest_coherence.rs"]
mod inv_048_matched_trade_and_open_interest_coherence;

#[path = "invariants/cu/inv_049_canonical_single_net_leg_per_asset_generation.rs"]
mod inv_049_canonical_single_net_leg_per_asset_generation;

#[path = "invariants/cu/inv_050_cross_zero_decomposition.rs"]
mod inv_050_cross_zero_decomposition;

#[path = "invariants/cu/inv_051_canonical_adl_effective_quantity.rs"]
mod inv_051_canonical_adl_effective_quantity;

#[path = "invariants/cu/inv_052_split_merge_invariance.rs"]
mod inv_052_split_merge_invariance;

#[path = "invariants/cu/inv_053_full_health_recertification_equivalence.rs"]
mod inv_053_full_health_recertification_equivalence;

#[path = "invariants/cu/inv_054_certificate_epoch_completeness.rs"]
mod inv_054_certificate_epoch_completeness;

#[path = "invariants/cu/inv_055_state_indexed_admission.rs"]
mod inv_055_state_indexed_admission;

#[path = "invariants/cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs"]
mod inv_056_hints_are_discovery_only_favorable_actions_fully_refresh;

#[path = "invariants/cu/inv_057_risk_reduction_availability.rs"]
mod inv_057_risk_reduction_availability;

#[path = "invariants/cu/inv_058_cumulative_position_oi_notional_and_rate_limit_integrity.rs"]
mod inv_058_cumulative_position_oi_notional_and_rate_limit_integrity;

#[path = "invariants/cu/inv_059_fee_fragmentation_bound.rs"]
mod inv_059_fee_fragmentation_bound;

#[path = "invariants/cu/inv_060_single_sided_margin_and_penalty_accounting.rs"]
mod inv_060_single_sided_margin_and_penalty_accounting;

#[path = "invariants/cu/inv_061_deterministic_bounded_liquidation.rs"]
mod inv_061_deterministic_bounded_liquidation;

#[path = "invariants/cu/inv_062_no_identity_assumptions_self_trade_containment.rs"]
mod inv_062_no_identity_assumptions_self_trade_containment;

#[path = "invariants/cu/inv_063_backing_expiry_normalization.rs"]
mod inv_063_backing_expiry_normalization;

#[path = "invariants/cu/inv_064_insurance_withdrawal_policy_equivalence.rs"]
mod inv_064_insurance_withdrawal_policy_equivalence;

#[path = "invariants/cu/inv_065_reset_recovery_and_retired_state_isolation.rs"]
mod inv_065_reset_recovery_and_retired_state_isolation;

#[path = "invariants/cu/inv_066_resolved_payout_fairness_and_order_independence.rs"]
mod inv_066_resolved_payout_fairness_and_order_independence;

#[path = "invariants/cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"]
mod inv_067_terminal_payout_completeness_and_exact_once_settlement;

#[path = "invariants/cu/inv_068_receipt_uniqueness_and_monotonic_topups.rs"]
mod inv_068_receipt_uniqueness_and_monotonic_topups;

#[path = "invariants/cu/inv_069_terminal_normalization_and_retirement.rs"]
mod inv_069_terminal_normalization_and_retirement;

#[path = "invariants/cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs"]
mod inv_070_zero_unattributed_terminal_residue_and_close_slab;

#[path = "invariants/cu/inv_071_crank_progress.rs"]
mod inv_071_crank_progress;

#[path = "invariants/cu/inv_072_order_robust_crankability.rs"]
mod inv_072_order_robust_crankability;

#[path = "invariants/cu/inv_073_no_permanent_user_lock.rs"]
mod inv_073_no_permanent_user_lock;

#[path = "invariants/cu/inv_074_scope_locality.rs"]
mod inv_074_scope_locality;

#[path = "invariants/cu/inv_075_close_priority_ownership_and_episode_integrity.rs"]
mod inv_075_close_priority_ownership_and_episode_integrity;

#[path = "invariants/cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs"]
mod inv_076_close_drift_residual_durability_and_finalization_atomicity;

#[path = "invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs"]
mod inv_077_bounded_work_and_maximum_shape_compute;

#[path = "invariants/cu/inv_078_permissionless_recovery_coverage.rs"]
mod inv_078_permissionless_recovery_coverage;

#[path = "invariants/cu/inv_080_error_propagation_and_exact_rollback.rs"]
mod inv_080_error_propagation_and_exact_rollback;

#[path = "invariants/cu/inv_081_success_state_validity_over_complete_public_routes.rs"]
mod inv_081_success_state_validity_over_complete_public_routes;

#[path = "invariants/cu/inv_082_state_indexed_liveness_theorem.rs"]
mod inv_082_state_indexed_liveness_theorem;

#[path = "invariants/cu/inv_083_boundary_completeness.rs"]
mod inv_083_boundary_completeness;

#[path = "invariants/cu/inv_084_proof_assumptions_are_reachable_and_nonvacuous.rs"]
mod inv_084_proof_assumptions_are_reachable_and_nonvacuous;

#[path = "invariants/cu/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs"]
mod inv_085_proven_arithmetic_equals_deployed_arithmetic;

#[path = "invariants/cu/inv_087_no_phantom_controls_or_dead_security_fields.rs"]
mod inv_087_no_phantom_controls_or_dead_security_fields;

#[path = "invariants/cu/inv_088_global_summaries_are_not_account_local_proofs.rs"]
mod inv_088_global_summaries_are_not_account_local_proofs;

#[path = "invariants/cu/inv_089_activation_reactivation_and_initialization_equivalence.rs"]
mod inv_089_activation_reactivation_and_initialization_equivalence;
