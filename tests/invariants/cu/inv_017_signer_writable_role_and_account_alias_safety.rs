//! INV-017 - Signer, writable-role, and account-alias safety.
//!
//! Public wrappers must reject duplicate mutable accounts in roles where aliasing would merge
//! independent economic meanings. These tests exercise real SBF/LiteSVM account metas for custody
//! trade, ledger, helper, and optional-ledger routes. A rejected alias attempt must leave program
//! accounts and SPL custody bytes unchanged exactly. The trade matrices exhaust all ten direct
//! and all 21 CPI semantic account pairs, plus every required signer/writable downgrade, for both
//! single and batch routes from otherwise-valid public fixtures. Deposit, withdraw, and resolved
//! close separately exhaust their 15-, 21-, and 21-pair SPL custody spaces and privileges. Backing
//! top-up, live insurance withdrawal, and backing-principal withdrawal, including optional
//! insurance/backing ledger tails, terminal insurance with and without its ledger, plus publicly
//! generated backing-earnings withdrawal, add 204 pairwise reserve-custody cases plus 59 required
//! privilege downgrades from value-moving controls.
//! Flat close, unilateral reduction, public Recovery forfeit, public bankrupt-close cure,
//! released-PnL conversion, maintenance sync, and both permissionless-crank account shapes add 40
//! core-account pairs and 28 required downgrades. The conversion and cure fixtures reach their
//! favorable states through public trade, mark, crank, and reduction transitions; cure then
//! exercises its SPL deposit tail. Initial and replacement base-unit mint shapes add 41 pair
//! aliases and eight downgrades; the dual-vault swap adds 28 pairs and five downgrades from a real
//! two-token transfer. All legal one-, two-, and three-provider Hybrid-oracle tails add 19 pair
//! aliases and six required downgrades from
//! coherent authenticated controls. Permissionless crank additionally exhausts every one-, two-,
//! and three-provider tail both with and without its optional reward portfolio, and proves that a
//! reward-enabled caller may omit that tail. `CloseSlab` covers primary/secondary collateral with
//! both ordinary dust and publicly generated unbudgeted insurance; abandoned-asset force close is
//! reached through public Recovery. Every lifecycle action plus permissionless append/reuse fee
//! tails are covered. Both market and asset-oracle authority
//! handoffs add all six account-pair aliases, four required-signature downgrades, and two required-
//! writable downgrades from valid two-signer mutation controls. Portfolio initialization and both
//! matcher-configuration shapes add 21 account-pair aliases and seven required privilege
//! downgrades. Fresh market initialization adds all three aliases and both required privilege
//! downgrades. Both ledger synchronizers add six pair aliases and six privilege downgrades; all
//! seven two-account fee/resolve policy routes add seven pair aliases and fourteen downgrades.
//! Four managed-mark routes and two authority/lifecycle routes add six pair aliases and twelve
//! downgrades; permissionless stale resolution adds its sole writable-role downgrade. Accepted
//! self-cranker, unsigned no-reward-crank, and readonly reward-cranker cases have explicit economic
//! controls.
//!
//! Guarantee boundary: this exhausts pairwise aliases and required privilege downgrades for every
//! current successful public account shape. It does not prove higher-arity alias combinations or
//! the instructions' non-account-role economic invariants.

use super::*;

fn inv017_braced_block_after<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing source marker {marker}"));
    let open = start
        + source[start..]
            .find('{')
            .unwrap_or_else(|| panic!("missing opening brace after {marker}"));
    let mut depth = 0i32;
    for (offset, character) in source[open..].char_indices() {
        match character {
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

fn inv017_instruction_variants(source: &str) -> std::collections::BTreeSet<String> {
    inv017_braced_block_after(source, "pub enum Instruction")
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.as_bytes().first()?.is_ascii_uppercase() {
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

#[test]
fn v16_program_account_role_matrix_roster_is_source_complete() {
    let production = include_str!("../../../src/v16_program.rs");
    let source_variants = inv017_instruction_variants(production);
    assert_eq!(
        source_variants.len(),
        50,
        "production instruction roster drift"
    );

    let public_registry = include_str!("../public_instruction_coverage.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty() && !line.starts_with("tag\t"))
        .map(|line| {
            let fields = line.splitn(5, '\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 5, "malformed public registry row: {line}");
            (fields[0].parse::<u8>().expect("numeric tag"), fields[1])
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let test_source = include_str!("inv_017_signer_writable_role_and_account_alias_safety.rs");
    let mut roster = std::collections::BTreeMap::new();
    let mut status_counts = std::collections::BTreeMap::<&str, usize>::new();
    for line in include_str!("../inv_017_account_role_coverage.tsv").lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("tag\t") {
            continue;
        }
        let fields = line.splitn(5, '\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 5, "malformed INV-017 roster row: {line}");
        let tag = fields[0].parse::<u8>().expect("numeric tag");
        let variant = fields[1];
        let status = fields[2];
        let evidence = fields[3];
        let gap = fields[4];
        assert_eq!(public_registry.get(&tag), Some(&variant));
        assert!(
            roster.insert(variant, status).is_none(),
            "duplicate {variant}"
        );
        *status_counts.entry(status).or_default() += 1;
        match status {
            "EXHAUSTIVE" => {
                assert_eq!(gap, "-", "closed matrix {variant} must have no gap");
                assert!(
                    test_source.contains(&format!("fn {evidence}")),
                    "closed matrix {variant} lacks executable evidence {evidence}"
                );
            }
            "PARTIAL" => {
                assert_ne!(evidence, "-");
                assert_ne!(gap, "-");
                assert!(test_source.contains(&format!("fn {evidence}")));
            }
            "OPEN" => {
                assert_eq!(evidence, "-");
                assert_ne!(gap, "-");
            }
            other => panic!("unknown INV-017 matrix status {other}"),
        }
    }
    assert_eq!(
        roster
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        source_variants.iter().map(String::as_str).collect(),
        "every production instruction needs an INV-017 matrix disposition"
    );
    assert_eq!(status_counts.get("EXHAUSTIVE"), Some(&50));
    assert_eq!(status_counts.get("PARTIAL"), None);
    assert_eq!(status_counts.get("OPEN"), None);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InitMarketRoleSnapshot {
    admin: Account,
    market: Account,
    mint: Account,
}

struct InitMarketRoleFixture {
    env: V16CuEnv,
    admin: Keypair,
    market: Keypair,
}

fn init_market_role_fixture() -> InitMarketRoleFixture {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let market = Keypair::new();
    let market_len = state::market_account_len_for_capacity(
        V16CuMarketParams::default().max_portfolio_assets as usize,
    )
    .expect("market account length");
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &market,
        market_len,
        env.program_id,
    );
    InitMarketRoleFixture { env, admin, market }
}

fn init_market_role_accounts(fixture: &InitMarketRoleFixture) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(fixture.admin.pubkey(), true),
        AccountMeta::new(fixture.market.pubkey(), false),
        AccountMeta::new_readonly(fixture.env.mint, false),
    ]
}

fn init_market_role_snapshot(fixture: &InitMarketRoleFixture) -> InitMarketRoleSnapshot {
    InitMarketRoleSnapshot {
        admin: fixture
            .env
            .svm
            .get_account(&fixture.admin.pubkey())
            .expect("admin account"),
        market: fixture
            .env
            .svm
            .get_account(&fixture.market.pubkey())
            .expect("market account"),
        mint: fixture
            .env
            .svm
            .get_account(&fixture.env.mint)
            .expect("mint account"),
    }
}

fn assert_init_market_role_rejects_atomically(
    label: &str,
    mutate: impl FnOnce(&mut [AccountMeta]),
) {
    let mut fixture = init_market_role_fixture();
    let mut accounts = init_market_role_accounts(&fixture);
    mutate(&mut accounts);
    let before = init_market_role_snapshot(&fixture);
    let admin_signature_required = accounts
        .iter()
        .any(|account| account.is_signer && account.pubkey == fixture.admin.pubkey());
    let signers = admin_signature_required
        .then_some(&fixture.admin)
        .into_iter()
        .collect::<Vec<_>>();

    fixture.env.svm.expire_blockhash();
    let rejected = fixture.env.send(
        init_market_instruction(&V16CuMarketParams::default()),
        accounts,
        &signers,
    );
    assert!(
        rejected.is_err(),
        "{label}: aliased or underprivileged InitMarket unexpectedly succeeded"
    );
    assert_eq!(
        init_market_role_snapshot(&fixture),
        before,
        "{label}: rejected InitMarket must roll back all supplied account state exactly"
    );
}

#[test]
fn v16_program_init_market_account_roles_are_exhaustive() {
    const ROLE_NAMES: [&str; 3] = ["admin", "market", "mint"];

    let mut control = init_market_role_fixture();
    let before = init_market_role_snapshot(&control);
    control.env.svm.expire_blockhash();
    control
        .env
        .send(
            init_market_instruction(&V16CuMarketParams::default()),
            init_market_role_accounts(&control),
            &[&control.admin],
        )
        .expect("canonical InitMarket control");
    assert_ne!(
        init_market_role_snapshot(&control).market,
        before.market,
        "canonical control must initialize the fresh market account"
    );
    let initialized = control
        .env
        .svm
        .get_account(&control.market.pubkey())
        .expect("initialized market");
    let (wrapper, group) = state::read_market(&initialized.data).expect("valid initialized market");
    assert_eq!(wrapper.marketauth, control.admin.pubkey().to_bytes());
    assert_eq!(wrapper.collateral_mint, control.env.mint.to_bytes());
    assert_eq!(
        group.assets[0].effective_price,
        V16CuMarketParams::default().initial_price
    );

    let mut pair_count = 0usize;
    for first in 0..ROLE_NAMES.len() {
        for second in (first + 1)..ROLE_NAMES.len() {
            pair_count += 1;
            assert_init_market_role_rejects_atomically(
                &format!("alias {} with {}", ROLE_NAMES[first], ROLE_NAMES[second]),
                |accounts| accounts[second].pubkey = accounts[first].pubkey,
            );
        }
    }
    assert_eq!(
        pair_count, 3,
        "three InitMarket roles have exactly three pairs"
    );

    assert_init_market_role_rejects_atomically("admin signer downgrade", |accounts| {
        accounts[0].is_signer = false;
    });
    assert_init_market_role_rejects_atomically("market writable downgrade", |accounts| {
        accounts[1].is_writable = false;
    });
}

#[test]
fn v16_program_finalize_reset_side_account_roles_are_exhaustive() {
    let super::inv_065_reset_recovery_and_retired_state_isolation::PublicEmptyLongResetPendingFixture {
        mut env,
        ..
    } = super::inv_065_reset_recovery_and_retired_state_isolation::public_empty_long_reset_pending_fixture();
    let before = env.svm.get_account(&env.market).expect("market account");
    let risk_epoch_before = env.market_state().1.risk_epoch;

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        vec![AccountMeta::new_readonly(env.market, false)],
        &[],
    );
    assert!(
        rejected.is_err(),
        "FinalizeResetSide must reject its sole market role when readonly"
    );
    assert_eq!(
        env.svm.get_account(&env.market).expect("market account"),
        before,
        "readonly rejection must preserve the publicly reached ResetPending state exactly"
    );

    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        vec![AccountMeta::new(env.market, false)],
        &[],
    )
    .expect("permissionless writable finalizer control");
    let (_, finalized) = env.market_state();
    assert_eq!(finalized.assets[0].mode_long, SideModeV16::Normal);
    assert_eq!(finalized.risk_epoch, risk_epoch_before + 1);
}

struct RestartAssetOracleRoleFixture {
    env: V16CuEnv,
    authority: Keypair,
    observation_sequence: u64,
}

fn restart_asset_oracle_role_fixture() -> RestartAssetOracleRoleFixture {
    let mut env = V16CuEnv::new();
    let authority = env.admin.insecure_clone();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_with_cu(0, 100);
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    env.try_shutdown_asset_with_authority(&authority, 0, 2)
        .expect("asset admin publicly shuts down empty asset");
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );
    let observation_sequence = next_control_sequence(env.control_sequences(0).oracle_observation);
    env.svm.warp_to_slot(3);
    RestartAssetOracleRoleFixture {
        env,
        authority,
        observation_sequence,
    }
}

fn restart_asset_oracle_role_instruction(
    fixture: &RestartAssetOracleRoleFixture,
) -> ProgInstruction {
    ProgInstruction::RestartAssetOracle {
        market_id: fixture.env.asset_market_id(0),
        asset_index: 0,
        now_slot: 3,
        initial_price: 111,
        observation_sequence: fixture.observation_sequence,
    }
}

fn restart_asset_oracle_role_accounts(fixture: &RestartAssetOracleRoleFixture) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(fixture.authority.pubkey(), true),
        AccountMeta::new(fixture.env.market, false),
    ]
}

fn assert_restart_asset_oracle_role_rejects_atomically(
    label: &str,
    mutate: impl FnOnce(&mut [AccountMeta]),
) {
    let mut fixture = restart_asset_oracle_role_fixture();
    let mut accounts = restart_asset_oracle_role_accounts(&fixture);
    mutate(&mut accounts);
    let market_before = fixture
        .env
        .svm
        .get_account(&fixture.env.market)
        .expect("market account");
    let authority_before = fixture
        .env
        .svm
        .get_account(&fixture.authority.pubkey())
        .expect("authority account");
    let authority_signature_required = accounts
        .iter()
        .any(|account| account.is_signer && account.pubkey == fixture.authority.pubkey());
    let signers = authority_signature_required
        .then_some(&fixture.authority)
        .into_iter()
        .collect::<Vec<_>>();

    fixture.env.svm.expire_blockhash();
    let rejected = fixture.env.send(
        restart_asset_oracle_role_instruction(&fixture),
        accounts,
        &signers,
    );
    assert!(
        rejected.is_err(),
        "{label}: aliased or underprivileged RestartAssetOracle unexpectedly succeeded"
    );
    assert_eq!(
        fixture
            .env
            .svm
            .get_account(&fixture.env.market)
            .expect("market account"),
        market_before,
        "{label}: restart rejection must preserve the Recovery market exactly"
    );
    assert_eq!(
        fixture
            .env
            .svm
            .get_account(&fixture.authority.pubkey())
            .expect("authority account"),
        authority_before,
        "{label}: restart rejection must preserve the authority account exactly"
    );
}

#[test]
fn v16_program_restart_asset_oracle_account_roles_are_exhaustive() {
    let mut control = restart_asset_oracle_role_fixture();
    let market_before = control
        .env
        .svm
        .get_account(&control.env.market)
        .expect("market account");
    control.env.svm.expire_blockhash();
    control
        .env
        .send(
            restart_asset_oracle_role_instruction(&control),
            restart_asset_oracle_role_accounts(&control),
            &[&control.authority],
        )
        .expect("canonical RestartAssetOracle control");
    assert_ne!(
        control
            .env
            .svm
            .get_account(&control.env.market)
            .expect("market account"),
        market_before,
        "canonical restart must mutate the Recovery market"
    );
    let (_, restarted) = control.env.market_state();
    assert_eq!(restarted.assets[0].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(restarted.assets[0].effective_price, 111);

    assert_restart_asset_oracle_role_rejects_atomically(
        "alias authority with market",
        |accounts| accounts[1].pubkey = accounts[0].pubkey,
    );
    assert_restart_asset_oracle_role_rejects_atomically("authority signer downgrade", |accounts| {
        accounts[0].is_signer = false
    });
    assert_restart_asset_oracle_role_rejects_atomically("market writable downgrade", |accounts| {
        accounts[1].is_writable = false
    });
}

struct ConfigureHybridRoleFixture {
    env: V16CuEnv,
    authority: Keypair,
    feeds: [[u8; 32]; 3],
    oracle_accounts: Vec<Pubkey>,
    observation_sequence: u64,
}

fn configure_hybrid_role_fixture(leg_count: usize) -> ConfigureHybridRoleFixture {
    assert!((1..=3).contains(&leg_count));
    let mut env = V16CuEnv::new();
    let authority = env.admin.insecure_clone();
    set_test_clock(&mut env, 1, 100);
    let mut feeds = [[0u8; 32]; 3];
    let mut oracle_accounts = Vec::with_capacity(leg_count);
    for index in 0..leg_count {
        feeds[index] = [0x91 + index as u8; 32];
        oracle_accounts.push(env.set_pyth_price_with_conf(&feeds[index], 1_000_000, -6, 0, 100));
    }
    let observation_sequence = next_control_sequence(env.control_sequences(0).oracle_observation);
    ConfigureHybridRoleFixture {
        env,
        authority,
        feeds,
        oracle_accounts,
        observation_sequence,
    }
}

fn configure_hybrid_role_instruction(fixture: &ConfigureHybridRoleFixture) -> ProgInstruction {
    ProgInstruction::ConfigureHybridOracle {
        market_id: fixture.env.asset_market_id(0),
        asset_index: 0,
        now_slot: 1,
        now_unix_ts: 100,
        oracle_leg_count: fixture.oracle_accounts.len() as u8,
        oracle_leg_flags: 0,
        max_staleness_secs: 60,
        hybrid_soft_stale_slots: 3,
        mark_ewma_halflife_slots: 1,
        mark_min_fee: 0,
        invert: 0,
        unit_scale: 0,
        conf_filter_bps: 0,
        oracle_leg_feeds: fixture.feeds,
        observation_sequence: fixture.observation_sequence,
    }
}

fn configure_hybrid_role_accounts(fixture: &ConfigureHybridRoleFixture) -> Vec<AccountMeta> {
    let mut accounts = vec![
        AccountMeta::new(fixture.authority.pubkey(), true),
        AccountMeta::new(fixture.env.market, false),
    ];
    accounts.extend(
        fixture
            .oracle_accounts
            .iter()
            .copied()
            .map(|key| AccountMeta::new_readonly(key, false)),
    );
    accounts
}

fn configure_hybrid_role_snapshot(fixture: &ConfigureHybridRoleFixture) -> Vec<Account> {
    std::iter::once(fixture.authority.pubkey())
        .chain(std::iter::once(fixture.env.market))
        .chain(fixture.oracle_accounts.iter().copied())
        .map(|key| fixture.env.svm.get_account(&key).expect("tracked account"))
        .collect()
}

fn assert_configure_hybrid_role_rejects_atomically(
    leg_count: usize,
    label: &str,
    mutate: impl FnOnce(&mut [AccountMeta]),
) {
    let mut fixture = configure_hybrid_role_fixture(leg_count);
    let mut accounts = configure_hybrid_role_accounts(&fixture);
    mutate(&mut accounts);
    let before = configure_hybrid_role_snapshot(&fixture);
    let authority_signature_required = accounts
        .iter()
        .any(|account| account.is_signer && account.pubkey == fixture.authority.pubkey());
    let signers = authority_signature_required
        .then_some(&fixture.authority)
        .into_iter()
        .collect::<Vec<_>>();

    fixture.env.svm.expire_blockhash();
    let rejected = fixture.env.send(
        configure_hybrid_role_instruction(&fixture),
        accounts,
        &signers,
    );
    assert!(
        rejected.is_err(),
        "{leg_count}-leg {label}: hostile hybrid configuration unexpectedly succeeded"
    );
    assert_eq!(
        configure_hybrid_role_snapshot(&fixture),
        before,
        "{leg_count}-leg {label}: rejection must preserve market, authority, and feeds exactly"
    );
}

#[test]
fn v16_program_configure_hybrid_oracle_account_roles_are_exhaustive() {
    let mut total_pairs = 0usize;
    for leg_count in 1..=3 {
        let mut control = configure_hybrid_role_fixture(leg_count);
        let before = control
            .env
            .svm
            .get_account(&control.env.market)
            .expect("market account");
        control.env.svm.expire_blockhash();
        control
            .env
            .send(
                configure_hybrid_role_instruction(&control),
                configure_hybrid_role_accounts(&control),
                &[&control.authority],
            )
            .expect("canonical hybrid configuration control");
        assert_ne!(
            control
                .env
                .svm
                .get_account(&control.env.market)
                .expect("market account"),
            before,
            "{leg_count}-leg control must install a hybrid profile"
        );
        let profile = state::read_asset_oracle_profile(
            &control
                .env
                .svm
                .get_account(&control.env.market)
                .expect("market account")
                .data,
            0,
        )
        .expect("hybrid profile");
        assert_eq!(profile.oracle_leg_count as usize, leg_count);
        assert_eq!(profile.oracle_target_price_e6, 1_000_000);

        let role_names = std::iter::once("authority".to_owned())
            .chain(std::iter::once("market".to_owned()))
            .chain((0..leg_count).map(|index| format!("oracle_{index}")))
            .collect::<Vec<_>>();
        for first in 0..role_names.len() {
            for second in (first + 1)..role_names.len() {
                total_pairs += 1;
                assert_configure_hybrid_role_rejects_atomically(
                    leg_count,
                    &format!("alias {} with {}", role_names[first], role_names[second]),
                    |accounts| accounts[second].pubkey = accounts[first].pubkey,
                );
            }
        }
        assert_configure_hybrid_role_rejects_atomically(
            leg_count,
            "authority signer downgrade",
            |accounts| accounts[0].is_signer = false,
        );
        assert_configure_hybrid_role_rejects_atomically(
            leg_count,
            "market writable downgrade",
            |accounts| accounts[1].is_writable = false,
        );
    }
    assert_eq!(total_pairs, 19);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AliasSnapshot {
    market: Account,
    portfolio_a: Account,
    portfolio_b: Option<Account>,
    vault: Account,
    vault_atoms: u64,
}

fn snapshot(env: &V16CuEnv, portfolio_a: Pubkey, portfolio_b: Option<Pubkey>) -> AliasSnapshot {
    AliasSnapshot {
        market: env.svm.get_account(&env.market).unwrap(),
        portfolio_a: env.svm.get_account(&portfolio_a).unwrap(),
        portfolio_b: portfolio_b.map(|key| env.svm.get_account(&key).unwrap()),
        vault: env.svm.get_account(&env.vault).unwrap(),
        vault_atoms: env.token_amount(env.vault),
    }
}

#[derive(Clone, Copy, Debug)]
enum DirectTradeAliasRoute {
    Single,
    Batch,
}

fn direct_trade_alias_fixture() -> (V16CuEnv, Keypair, Pubkey, Keypair, Pubkey) {
    let mut env = V16CuEnv::new();
    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let portfolio_a = env.create_portfolio(&owner_a);
    let portfolio_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, portfolio_a, 1_000_000);
    env.deposit(&owner_b, portfolio_b, 1_000_000);
    (env, owner_a, portfolio_a, owner_b, portfolio_b)
}

fn direct_trade_alias_instruction(
    env: &V16CuEnv,
    route: DirectTradeAliasRoute,
    portfolio_a: Pubkey,
    portfolio_b: Pubkey,
) -> ProgInstruction {
    match route {
        DirectTradeAliasRoute::Single => {
            env.trade_no_cpi_ix(portfolio_a, portfolio_b, 0, POS_SCALE as i128, 100, 0)
        }
        DirectTradeAliasRoute::Batch => env.batch_trade_no_cpi_ix(
            portfolio_a,
            portfolio_b,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                size_q: POS_SCALE as i128,
                exec_price: 100,
                fee_bps: 0,
            }],
        ),
    }
}

fn direct_trade_alias_accounts(
    env: &V16CuEnv,
    owner_a: &Keypair,
    portfolio_a: Pubkey,
    owner_b: &Keypair,
    portfolio_b: Pubkey,
) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(owner_a.pubkey(), true),
        AccountMeta::new(owner_b.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio_a, false),
        AccountMeta::new(portfolio_b, false),
    ]
}

fn assert_direct_trade_alias_rejects_atomically(
    route: DirectTradeAliasRoute,
    label: &str,
    mutate: impl FnOnce(&mut [AccountMeta]),
) {
    let (mut env, owner_a, portfolio_a, owner_b, portfolio_b) = direct_trade_alias_fixture();
    let instruction = direct_trade_alias_instruction(&env, route, portfolio_a, portfolio_b);
    let mut accounts =
        direct_trade_alias_accounts(&env, &owner_a, portfolio_a, &owner_b, portfolio_b);
    mutate(&mut accounts);
    let before = snapshot(&env, portfolio_a, Some(portfolio_b));
    let owner_a_required = accounts
        .iter()
        .any(|account| account.is_signer && account.pubkey == owner_a.pubkey());
    let owner_b_required = accounts
        .iter()
        .any(|account| account.is_signer && account.pubkey == owner_b.pubkey());
    let mut signers = Vec::with_capacity(2);
    if owner_a_required {
        signers.push(&owner_a);
    }
    if owner_b_required {
        signers.push(&owner_b);
    }

    env.svm.expire_blockhash();
    let rejected = env.send(instruction, accounts, &signers);
    assert!(
        rejected.is_err(),
        "{route:?} {label}: aliased or underprivileged route unexpectedly succeeded",
    );
    assert_eq!(
        snapshot(&env, portfolio_a, Some(portfolio_b)),
        before,
        "{route:?} {label}: rejection must roll back both portfolios, market, and vault exactly",
    );
}

#[test]
fn v16_program_direct_trade_account_pairs_and_required_privileges_are_exhaustive() {
    const ROLE_NAMES: [&str; 5] = ["owner_a", "owner_b", "market", "portfolio_a", "portfolio_b"];

    for route in [DirectTradeAliasRoute::Single, DirectTradeAliasRoute::Batch] {
        let (mut env, owner_a, portfolio_a, owner_b, portfolio_b) = direct_trade_alias_fixture();
        let before = snapshot(&env, portfolio_a, Some(portfolio_b));
        let control = env.send(
            direct_trade_alias_instruction(&env, route, portfolio_a, portfolio_b),
            direct_trade_alias_accounts(&env, &owner_a, portfolio_a, &owner_b, portfolio_b),
            &[&owner_a, &owner_b],
        );
        assert!(
            control.is_ok(),
            "{route:?}: canonical control must prove the fixture reaches trade mutation: {control:?}",
        );
        assert_ne!(
            snapshot(&env, portfolio_a, Some(portfolio_b)),
            before,
            "{route:?}: canonical control must change economic state",
        );

        let mut pair_count = 0usize;
        for first in 0..ROLE_NAMES.len() {
            for second in (first + 1)..ROLE_NAMES.len() {
                pair_count += 1;
                let label = format!("alias {} with {}", ROLE_NAMES[first], ROLE_NAMES[second]);
                assert_direct_trade_alias_rejects_atomically(route, &label, |accounts| {
                    accounts[second].pubkey = accounts[first].pubkey;
                });
            }
        }
        assert_eq!(pair_count, 10, "five semantic roles have exactly ten pairs");

        for (role, role_name) in ROLE_NAMES.iter().enumerate().take(2) {
            assert_direct_trade_alias_rejects_atomically(
                route,
                &format!("{role_name} signer downgrade"),
                |accounts| accounts[role].is_signer = false,
            );
        }
        for (role, role_name) in ROLE_NAMES.iter().enumerate().skip(2) {
            assert_direct_trade_alias_rejects_atomically(
                route,
                &format!("{role_name} writable downgrade"),
                |accounts| accounts[role].is_writable = false,
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CpiTradeAliasRoute {
    Single,
    Batch,
}

struct CpiTradeAliasFixture {
    env: V16CuEnv,
    taker_owner: Keypair,
    taker: Pubkey,
    lp: Pubkey,
    matcher_program: Pubkey,
    matcher_context: Pubkey,
    matcher_delegate: Pubkey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CpiAliasSnapshot {
    core: AliasSnapshot,
    matcher_context: Account,
}

fn cpi_trade_alias_fixture() -> CpiTradeAliasFixture {
    let mut env = V16CuEnv::new();
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    let (matcher_program, matcher_context, matcher_delegate) =
        auth_matcher_for_lp(&mut env, &lp_owner, lp);
    CpiTradeAliasFixture {
        env,
        taker_owner,
        taker,
        lp,
        matcher_program,
        matcher_context,
        matcher_delegate,
    }
}

fn cpi_alias_snapshot(fixture: &CpiTradeAliasFixture) -> CpiAliasSnapshot {
    CpiAliasSnapshot {
        core: snapshot(&fixture.env, fixture.taker, Some(fixture.lp)),
        matcher_context: fixture
            .env
            .svm
            .get_account(&fixture.matcher_context)
            .unwrap(),
    }
}

fn cpi_trade_alias_instruction(
    fixture: &CpiTradeAliasFixture,
    route: CpiTradeAliasRoute,
) -> ProgInstruction {
    match route {
        CpiTradeAliasRoute::Single => {
            fixture
                .env
                .trade_cpi_ix(fixture.taker, fixture.lp, 0, POS_SCALE as i128, 0, 0)
        }
        CpiTradeAliasRoute::Batch => fixture.env.batch_trade_cpi_ix(
            fixture.taker,
            fixture.lp,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: fixture.env.asset_market_id(0),
                size_q: POS_SCALE as i128,
                fee_bps: 0,
                limit_price: 0,
            }],
        ),
    }
}

fn cpi_trade_alias_accounts(fixture: &CpiTradeAliasFixture) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(fixture.taker_owner.pubkey(), true),
        AccountMeta::new(fixture.env.market, false),
        AccountMeta::new(fixture.taker, false),
        AccountMeta::new(fixture.lp, false),
        AccountMeta::new_readonly(fixture.matcher_program, false),
        AccountMeta::new(fixture.matcher_context, false),
        AccountMeta::new_readonly(fixture.matcher_delegate, false),
    ]
}

fn assert_cpi_trade_alias_rejects_atomically(
    route: CpiTradeAliasRoute,
    label: &str,
    mutate: impl FnOnce(&mut [AccountMeta]),
) {
    let mut fixture = cpi_trade_alias_fixture();
    let instruction = cpi_trade_alias_instruction(&fixture, route);
    let mut accounts = cpi_trade_alias_accounts(&fixture);
    mutate(&mut accounts);
    let before = cpi_alias_snapshot(&fixture);
    let taker_signature_required = accounts
        .iter()
        .any(|account| account.is_signer && account.pubkey == fixture.taker_owner.pubkey());
    let signers = taker_signature_required
        .then_some(&fixture.taker_owner)
        .into_iter()
        .collect::<Vec<_>>();

    fixture.env.svm.expire_blockhash();
    let rejected = fixture.env.send(instruction, accounts, &signers);
    assert!(
        rejected.is_err(),
        "{route:?} {label}: aliased or underprivileged CPI route unexpectedly succeeded",
    );
    assert_eq!(
        cpi_alias_snapshot(&fixture),
        before,
        "{route:?} {label}: CPI rejection must roll back matcher and protocol state exactly",
    );
}

#[test]
fn v16_program_cpi_trade_account_pairs_and_required_privileges_are_exhaustive() {
    const ROLE_NAMES: [&str; 7] = [
        "taker_owner",
        "market",
        "taker_portfolio",
        "lp_portfolio",
        "matcher_program",
        "matcher_context",
        "matcher_delegate",
    ];
    const REQUIRED_WRITABLE_ROLES: [usize; 4] = [1, 2, 3, 5];

    for route in [CpiTradeAliasRoute::Single, CpiTradeAliasRoute::Batch] {
        let mut control = cpi_trade_alias_fixture();
        let before = cpi_alias_snapshot(&control);
        let accepted = control.env.send(
            cpi_trade_alias_instruction(&control, route),
            cpi_trade_alias_accounts(&control),
            &[&control.taker_owner],
        );
        assert!(
            accepted.is_ok(),
            "{route:?}: canonical CPI control must reach matcher-backed mutation: {accepted:?}",
        );
        assert_ne!(
            cpi_alias_snapshot(&control),
            before,
            "{route:?}: canonical CPI control must change matcher or economic state",
        );

        let mut pair_count = 0usize;
        for first in 0..ROLE_NAMES.len() {
            for second in (first + 1)..ROLE_NAMES.len() {
                pair_count += 1;
                let label = format!("alias {} with {}", ROLE_NAMES[first], ROLE_NAMES[second]);
                assert_cpi_trade_alias_rejects_atomically(route, &label, |accounts| {
                    accounts[second].pubkey = accounts[first].pubkey;
                });
            }
        }
        assert_eq!(pair_count, 21, "seven semantic roles have exactly 21 pairs");

        assert_cpi_trade_alias_rejects_atomically(
            route,
            "taker owner signer downgrade",
            |accounts| accounts[0].is_signer = false,
        );
        for role in REQUIRED_WRITABLE_ROLES {
            assert_cpi_trade_alias_rejects_atomically(
                route,
                &format!("{} writable downgrade", ROLE_NAMES[role]),
                |accounts| accounts[role].is_writable = false,
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CustodyAliasRoute {
    Deposit,
    Withdraw,
    CloseResolved,
}

struct CustodyAliasFixture {
    env: V16CuEnv,
    owner: Keypair,
    portfolio: Pubkey,
    user_token: Pubkey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CustodyAliasSnapshot {
    core: AliasSnapshot,
    user_token: Account,
}

fn custody_alias_fixture(route: CustodyAliasRoute) -> CustodyAliasFixture {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let user_token = match route {
        CustodyAliasRoute::Deposit => env.token_account(owner.pubkey(), 100),
        CustodyAliasRoute::Withdraw => {
            env.deposit(&owner, portfolio, 100);
            env.token_account(owner.pubkey(), 0)
        }
        CustodyAliasRoute::CloseResolved => {
            env.deposit(&owner, portfolio, 100);
            env.resolve();
            env.token_account(owner.pubkey(), 0)
        }
    };
    CustodyAliasFixture {
        env,
        owner,
        portfolio,
        user_token,
    }
}

fn custody_alias_snapshot(fixture: &CustodyAliasFixture) -> CustodyAliasSnapshot {
    CustodyAliasSnapshot {
        core: snapshot(&fixture.env, fixture.portfolio, None),
        user_token: fixture.env.svm.get_account(&fixture.user_token).unwrap(),
    }
}

fn custody_alias_instruction(
    fixture: &CustodyAliasFixture,
    route: CustodyAliasRoute,
) -> ProgInstruction {
    match route {
        CustodyAliasRoute::Deposit => fixture.env.deposit_ix(fixture.portfolio, 10),
        CustodyAliasRoute::Withdraw => fixture.env.withdraw_ix(fixture.portfolio, 10),
        CustodyAliasRoute::CloseResolved => ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
    }
}

fn custody_alias_accounts(
    fixture: &CustodyAliasFixture,
    route: CustodyAliasRoute,
) -> Vec<AccountMeta> {
    let mut accounts = vec![
        AccountMeta::new(fixture.owner.pubkey(), true),
        AccountMeta::new(fixture.env.market, false),
        AccountMeta::new(fixture.portfolio, false),
        AccountMeta::new(fixture.user_token, false),
        AccountMeta::new(fixture.env.vault, false),
    ];
    if matches!(
        route,
        CustodyAliasRoute::Withdraw | CustodyAliasRoute::CloseResolved
    ) {
        accounts.push(AccountMeta::new_readonly(
            fixture.env.vault_authority,
            false,
        ));
    }
    if matches!(route, CustodyAliasRoute::CloseResolved) {
        accounts[0].is_signer = false;
        accounts[0].is_writable = false;
    }
    accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
    accounts
}

fn assert_custody_alias_rejects_atomically(
    route: CustodyAliasRoute,
    label: &str,
    mutate: impl FnOnce(&mut [AccountMeta]),
) {
    let mut fixture = custody_alias_fixture(route);
    let instruction = custody_alias_instruction(&fixture, route);
    let mut accounts = custody_alias_accounts(&fixture, route);
    mutate(&mut accounts);
    let before = custody_alias_snapshot(&fixture);
    let owner_signature_required = accounts
        .iter()
        .any(|account| account.is_signer && account.pubkey == fixture.owner.pubkey());
    let signers = owner_signature_required
        .then_some(&fixture.owner)
        .into_iter()
        .collect::<Vec<_>>();

    fixture.env.svm.expire_blockhash();
    let rejected = fixture.env.send(instruction, accounts, &signers);
    assert!(
        rejected.is_err(),
        "{route:?} {label}: aliased or underprivileged custody route unexpectedly succeeded",
    );
    assert_eq!(
        custody_alias_snapshot(&fixture),
        before,
        "{route:?} {label}: custody rejection must roll back accounting and SPL bytes exactly",
    );
}

#[test]
fn v16_program_custody_account_pairs_and_required_privileges_are_exhaustive() {
    for route in [
        CustodyAliasRoute::Deposit,
        CustodyAliasRoute::Withdraw,
        CustodyAliasRoute::CloseResolved,
    ] {
        let role_names: &[&str] = match route {
            CustodyAliasRoute::Deposit => &[
                "owner",
                "market",
                "portfolio",
                "source_token",
                "vault",
                "token_program",
            ],
            CustodyAliasRoute::Withdraw | CustodyAliasRoute::CloseResolved => &[
                "owner",
                "market",
                "portfolio",
                "destination_token",
                "vault",
                "vault_authority",
                "token_program",
            ],
        };

        let mut control = custody_alias_fixture(route);
        let before = custody_alias_snapshot(&control);
        let control_signers = custody_alias_accounts(&control, route)
            .first()
            .is_some_and(|account| account.is_signer)
            .then_some(&control.owner)
            .into_iter()
            .collect::<Vec<_>>();
        let accepted = control.env.send(
            custody_alias_instruction(&control, route),
            custody_alias_accounts(&control, route),
            &control_signers,
        );
        assert!(
            accepted.is_ok(),
            "{route:?}: canonical custody control must execute: {accepted:?}",
        );
        assert_ne!(
            custody_alias_snapshot(&control),
            before,
            "{route:?}: canonical custody control must move tokens and accounting",
        );

        let mut pair_count = 0usize;
        for first in 0..role_names.len() {
            for second in (first + 1)..role_names.len() {
                pair_count += 1;
                let label = format!("alias {} with {}", role_names[first], role_names[second]);
                assert_custody_alias_rejects_atomically(route, &label, |accounts| {
                    accounts[second].pubkey = accounts[first].pubkey;
                });
            }
        }
        assert_eq!(
            pair_count,
            role_names.len() * (role_names.len() - 1) / 2,
            "pair matrix must be complete",
        );

        if !matches!(route, CustodyAliasRoute::CloseResolved) {
            assert_custody_alias_rejects_atomically(route, "owner signer downgrade", |accounts| {
                accounts[0].is_signer = false;
            });
        }
        for role in 1..=4 {
            assert_custody_alias_rejects_atomically(
                route,
                &format!("{} writable downgrade", role_names[role]),
                |accounts| accounts[role].is_writable = false,
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ReserveCustodyAliasRoute {
    TopUpInsurance,
    TopUpInsuranceWithLedger,
    TopUpInsuranceDomain,
    TopUpInsuranceDomainWithLedger,
    TopUpBacking,
    TopUpBackingWithLedger,
    WithdrawInsurance,
    WithdrawInsuranceWithLedger,
    WithdrawTerminalInsurance,
    WithdrawTerminalInsuranceWithLedger,
    WithdrawBacking,
    WithdrawBackingWithLedger,
    WithdrawBackingEarnings,
}

struct ReserveCustodyAliasFixture {
    env: V16CuEnv,
    authority: Keypair,
    user_token: Pubkey,
    ledger: Option<Pubkey>,
    amount: u128,
    domain: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReserveCustodyAliasSnapshot {
    market: Account,
    user_token: Account,
    vault: Account,
    ledger: Option<Account>,
    user_atoms: u64,
    vault_atoms: u64,
}

fn reserve_custody_alias_fixture(route: ReserveCustodyAliasRoute) -> ReserveCustodyAliasFixture {
    let (mut env, ledger, amount, domain) = match route {
        ReserveCustodyAliasRoute::WithdrawBackingEarnings => {
            let fixture = public_backing_earnings_fixture();
            (
                fixture.env,
                Some(fixture.ledger),
                fixture.earnings,
                fixture.domain,
            )
        }
        _ => {
            let mut env = V16CuEnv::new();
            let ledger = match route {
                ReserveCustodyAliasRoute::TopUpInsuranceWithLedger
                | ReserveCustodyAliasRoute::TopUpInsuranceDomainWithLedger => {
                    Some(env.insurance_ledger_account())
                }
                ReserveCustodyAliasRoute::TopUpBackingWithLedger => {
                    Some(env.backing_domain_ledger_account())
                }
                ReserveCustodyAliasRoute::WithdrawInsuranceWithLedger => {
                    Some(env.insurance_ledger_account())
                }
                ReserveCustodyAliasRoute::WithdrawTerminalInsuranceWithLedger => {
                    Some(env.insurance_ledger_account())
                }
                ReserveCustodyAliasRoute::WithdrawBackingWithLedger => {
                    Some(env.backing_domain_ledger_account())
                }
                _ => None,
            };
            (env, ledger, 10, 0)
        }
    };
    let authority = env.admin.insecure_clone();
    let user_token = match route {
        ReserveCustodyAliasRoute::TopUpInsurance
        | ReserveCustodyAliasRoute::TopUpInsuranceWithLedger
        | ReserveCustodyAliasRoute::TopUpInsuranceDomain
        | ReserveCustodyAliasRoute::TopUpInsuranceDomainWithLedger
        | ReserveCustodyAliasRoute::TopUpBacking
        | ReserveCustodyAliasRoute::TopUpBackingWithLedger => {
            env.token_account(authority.pubkey(), 100)
        }
        ReserveCustodyAliasRoute::WithdrawInsurance => {
            env.enable_live_insurance_withdrawal();
            env.top_up_insurance(100);
            env.token_account(authority.pubkey(), 0)
        }
        ReserveCustodyAliasRoute::WithdrawInsuranceWithLedger => {
            env.enable_live_insurance_withdrawal();
            env.top_up_insurance_with_ledger_with_cu(
                ledger.expect("insurance ledger fixture"),
                100,
            );
            env.token_account(authority.pubkey(), 0)
        }
        ReserveCustodyAliasRoute::WithdrawTerminalInsurance => {
            env.top_up_insurance(100);
            env.resolve();
            env.token_account(authority.pubkey(), 0)
        }
        ReserveCustodyAliasRoute::WithdrawTerminalInsuranceWithLedger => {
            env.top_up_insurance_with_ledger_with_cu(
                ledger.expect("terminal insurance ledger fixture"),
                100,
            );
            env.resolve();
            env.token_account(authority.pubkey(), 0)
        }
        ReserveCustodyAliasRoute::WithdrawBacking => {
            env.top_up_backing_bucket(0, 100, 10_000);
            env.token_account(authority.pubkey(), 0)
        }
        ReserveCustodyAliasRoute::WithdrawBackingWithLedger => {
            env.top_up_backing_bucket_with_ledger_with_cu(
                ledger.expect("backing ledger fixture"),
                0,
                100,
                10_000,
            );
            env.token_account(authority.pubkey(), 0)
        }
        ReserveCustodyAliasRoute::WithdrawBackingEarnings => {
            env.token_account(authority.pubkey(), 0)
        }
    };
    ReserveCustodyAliasFixture {
        env,
        authority,
        user_token,
        ledger,
        amount,
        domain,
    }
}

fn reserve_custody_alias_snapshot(
    fixture: &ReserveCustodyAliasFixture,
) -> ReserveCustodyAliasSnapshot {
    ReserveCustodyAliasSnapshot {
        market: fixture.env.svm.get_account(&fixture.env.market).unwrap(),
        user_token: fixture.env.svm.get_account(&fixture.user_token).unwrap(),
        vault: fixture.env.svm.get_account(&fixture.env.vault).unwrap(),
        ledger: fixture
            .ledger
            .map(|ledger| fixture.env.svm.get_account(&ledger).unwrap()),
        user_atoms: fixture.env.token_amount(fixture.user_token),
        vault_atoms: fixture.env.token_amount(fixture.env.vault),
    }
}

fn reserve_custody_alias_instruction(
    fixture: &ReserveCustodyAliasFixture,
    route: ReserveCustodyAliasRoute,
) -> ProgInstruction {
    match route {
        ReserveCustodyAliasRoute::TopUpInsurance
        | ReserveCustodyAliasRoute::TopUpInsuranceWithLedger => ProgInstruction::TopUpInsurance {
            intent_id: 0,
            market_id: fixture.env.asset_market_id(0),
            amount: fixture.amount,
        },
        ReserveCustodyAliasRoute::TopUpInsuranceDomain
        | ReserveCustodyAliasRoute::TopUpInsuranceDomainWithLedger => {
            ProgInstruction::TopUpInsuranceDomain {
                intent_id: 0,
                market_id: fixture.env.asset_market_id(0),
                domain: 0,
                amount: fixture.amount,
            }
        }
        ReserveCustodyAliasRoute::TopUpBacking
        | ReserveCustodyAliasRoute::TopUpBackingWithLedger => ProgInstruction::TopUpBackingBucket {
            intent_id: 0,
            market_id: fixture.env.asset_market_id(0),
            domain: 0,
            amount: fixture.amount,
            expiry_slot: 10_000,
        },
        ReserveCustodyAliasRoute::WithdrawInsurance
        | ReserveCustodyAliasRoute::WithdrawInsuranceWithLedger => {
            ProgInstruction::WithdrawInsuranceAsset {
                market_id: fixture.env.asset_market_id(0),
                asset_index: 0,
                amount: fixture.amount,
            }
        }
        ReserveCustodyAliasRoute::WithdrawTerminalInsurance
        | ReserveCustodyAliasRoute::WithdrawTerminalInsuranceWithLedger => {
            ProgInstruction::WithdrawInsurance {
                amount: fixture.amount,
            }
        }
        ReserveCustodyAliasRoute::WithdrawBacking
        | ReserveCustodyAliasRoute::WithdrawBackingWithLedger => {
            ProgInstruction::WithdrawBackingBucket {
                domain: 0,
                market_id: fixture.env.asset_market_id(0),
                amount: fixture.amount,
            }
        }
        ReserveCustodyAliasRoute::WithdrawBackingEarnings => {
            ProgInstruction::WithdrawBackingBucketEarnings {
                domain: fixture.domain,
                market_id: fixture.env.asset_market_id(fixture.domain / 2),
                amount: fixture.amount,
            }
        }
    }
}

fn reserve_custody_alias_accounts(
    fixture: &ReserveCustodyAliasFixture,
    route: ReserveCustodyAliasRoute,
) -> Vec<AccountMeta> {
    if matches!(route, ReserveCustodyAliasRoute::WithdrawBackingEarnings) {
        return vec![
            AccountMeta::new(fixture.authority.pubkey(), true),
            AccountMeta::new(fixture.env.market, false),
            AccountMeta::new(fixture.ledger.expect("backing earnings ledger"), false),
            AccountMeta::new(fixture.user_token, false),
            AccountMeta::new(fixture.env.vault, false),
            AccountMeta::new_readonly(fixture.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ];
    }
    let mut accounts = vec![
        AccountMeta::new(fixture.authority.pubkey(), true),
        AccountMeta::new(fixture.env.market, false),
        AccountMeta::new(fixture.user_token, false),
        AccountMeta::new(fixture.env.vault, false),
    ];
    let top_up = matches!(
        route,
        ReserveCustodyAliasRoute::TopUpInsurance
            | ReserveCustodyAliasRoute::TopUpInsuranceWithLedger
            | ReserveCustodyAliasRoute::TopUpInsuranceDomain
            | ReserveCustodyAliasRoute::TopUpInsuranceDomainWithLedger
            | ReserveCustodyAliasRoute::TopUpBacking
            | ReserveCustodyAliasRoute::TopUpBackingWithLedger
    );
    if !top_up {
        accounts.push(AccountMeta::new_readonly(
            fixture.env.vault_authority,
            false,
        ));
    }
    accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
    if let Some(ledger) = fixture.ledger {
        accounts.push(AccountMeta::new(ledger, false));
    }
    accounts
}

fn assert_reserve_custody_alias_rejects_atomically(
    route: ReserveCustodyAliasRoute,
    label: &str,
    mutate: impl FnOnce(&mut [AccountMeta]),
) {
    let mut fixture = reserve_custody_alias_fixture(route);
    let instruction = reserve_custody_alias_instruction(&fixture, route);
    let mut accounts = reserve_custody_alias_accounts(&fixture, route);
    mutate(&mut accounts);
    let before = reserve_custody_alias_snapshot(&fixture);
    let authority_signature_required = accounts
        .iter()
        .any(|account| account.is_signer && account.pubkey == fixture.authority.pubkey());
    let signers = authority_signature_required
        .then_some(&fixture.authority)
        .into_iter()
        .collect::<Vec<_>>();

    fixture.env.svm.expire_blockhash();
    let rejected = fixture.env.send(instruction, accounts, &signers);
    assert!(
        rejected.is_err(),
        "{route:?} {label}: aliased or underprivileged reserve route unexpectedly succeeded"
    );
    assert_eq!(
        reserve_custody_alias_snapshot(&fixture),
        before,
        "{route:?} {label}: rejection must roll back market and SPL custody exactly"
    );
}

#[test]
fn v16_program_reserve_custody_account_pairs_and_required_privileges_are_exhaustive() {
    for route in [
        ReserveCustodyAliasRoute::TopUpInsurance,
        ReserveCustodyAliasRoute::TopUpInsuranceWithLedger,
        ReserveCustodyAliasRoute::TopUpInsuranceDomain,
        ReserveCustodyAliasRoute::TopUpInsuranceDomainWithLedger,
        ReserveCustodyAliasRoute::TopUpBacking,
        ReserveCustodyAliasRoute::TopUpBackingWithLedger,
        ReserveCustodyAliasRoute::WithdrawInsurance,
        ReserveCustodyAliasRoute::WithdrawInsuranceWithLedger,
        ReserveCustodyAliasRoute::WithdrawTerminalInsurance,
        ReserveCustodyAliasRoute::WithdrawTerminalInsuranceWithLedger,
        ReserveCustodyAliasRoute::WithdrawBacking,
        ReserveCustodyAliasRoute::WithdrawBackingWithLedger,
        ReserveCustodyAliasRoute::WithdrawBackingEarnings,
    ] {
        let role_names: &[&str] = match route {
            ReserveCustodyAliasRoute::TopUpInsurance
            | ReserveCustodyAliasRoute::TopUpInsuranceDomain
            | ReserveCustodyAliasRoute::TopUpBacking => &[
                "authority",
                "market",
                "source_token",
                "vault",
                "token_program",
            ],
            ReserveCustodyAliasRoute::TopUpInsuranceWithLedger
            | ReserveCustodyAliasRoute::TopUpInsuranceDomainWithLedger
            | ReserveCustodyAliasRoute::TopUpBackingWithLedger => &[
                "authority",
                "market",
                "source_token",
                "vault",
                "token_program",
                "ledger",
            ],
            ReserveCustodyAliasRoute::WithdrawInsurance
            | ReserveCustodyAliasRoute::WithdrawTerminalInsurance
            | ReserveCustodyAliasRoute::WithdrawBacking => &[
                "authority",
                "market",
                "destination_token",
                "vault",
                "vault_authority",
                "token_program",
            ],
            ReserveCustodyAliasRoute::WithdrawInsuranceWithLedger
            | ReserveCustodyAliasRoute::WithdrawTerminalInsuranceWithLedger
            | ReserveCustodyAliasRoute::WithdrawBackingWithLedger => &[
                "authority",
                "market",
                "destination_token",
                "vault",
                "vault_authority",
                "token_program",
                "ledger",
            ],
            ReserveCustodyAliasRoute::WithdrawBackingEarnings => &[
                "authority",
                "market",
                "ledger",
                "destination_token",
                "vault",
                "vault_authority",
                "token_program",
            ],
        };

        let mut control = reserve_custody_alias_fixture(route);
        let before = reserve_custody_alias_snapshot(&control);
        let accepted = control.env.send(
            reserve_custody_alias_instruction(&control, route),
            reserve_custody_alias_accounts(&control, route),
            &[&control.authority],
        );
        assert!(
            accepted.is_ok(),
            "{route:?}: canonical reserve custody control must execute: {accepted:?}"
        );
        let after = reserve_custody_alias_snapshot(&control);
        assert_eq!(
            u128::from(before.vault_atoms.abs_diff(after.vault_atoms)),
            control.amount,
        );
        assert_eq!(
            u128::from(before.user_atoms.abs_diff(after.user_atoms)),
            control.amount,
        );
        assert_ne!(
            after.market, before.market,
            "control must update accounting"
        );

        let mut pair_count = 0usize;
        for first in 0..role_names.len() {
            for second in (first + 1)..role_names.len() {
                pair_count += 1;
                let label = format!("alias {} with {}", role_names[first], role_names[second]);
                assert_reserve_custody_alias_rejects_atomically(route, &label, |accounts| {
                    accounts[second].pubkey = accounts[first].pubkey;
                });
            }
        }
        assert_eq!(
            pair_count,
            role_names.len() * (role_names.len() - 1) / 2,
            "pair matrix must be complete"
        );

        assert_reserve_custody_alias_rejects_atomically(
            route,
            "authority signer downgrade",
            |accounts| accounts[0].is_signer = false,
        );
        let required_writable_roles: &[usize] = match route {
            ReserveCustodyAliasRoute::TopUpInsuranceWithLedger
            | ReserveCustodyAliasRoute::TopUpInsuranceDomainWithLedger
            | ReserveCustodyAliasRoute::TopUpBackingWithLedger => &[1, 2, 3, 5],
            ReserveCustodyAliasRoute::WithdrawInsuranceWithLedger
            | ReserveCustodyAliasRoute::WithdrawTerminalInsuranceWithLedger
            | ReserveCustodyAliasRoute::WithdrawBackingWithLedger => &[1, 2, 3, 6],
            ReserveCustodyAliasRoute::WithdrawBackingEarnings => &[1, 2, 3, 4],
            _ => &[1, 2, 3],
        };
        for &role in required_writable_roles {
            assert_reserve_custody_alias_rejects_atomically(
                route,
                &format!("{} writable downgrade", role_names[role]),
                |accounts| accounts[role].is_writable = false,
            );
        }
    }
}

struct CoreAccountAliasFixture {
    env: V16CuEnv,
    signers: Vec<Keypair>,
    instruction: ProgInstruction,
    accounts: Vec<AccountMeta>,
    tracked_accounts: Vec<Pubkey>,
}

fn account_alias_snapshot(env: &V16CuEnv, tracked_accounts: &[Pubkey]) -> Vec<Option<Account>> {
    tracked_accounts
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect()
}

fn core_account_alias_snapshot(fixture: &CoreAccountAliasFixture) -> Vec<Option<Account>> {
    account_alias_snapshot(&fixture.env, &fixture.tracked_accounts)
}

fn send_core_account_alias_fixture(
    fixture: &mut CoreAccountAliasFixture,
    expire_blockhash: bool,
) -> Result<u64, String> {
    let signers = fixture
        .signers
        .iter()
        .filter(|signer| {
            fixture
                .accounts
                .iter()
                .any(|account| account.is_signer && account.pubkey == signer.pubkey())
        })
        .map(Keypair::insecure_clone)
        .collect::<Vec<_>>();
    let signer_refs = signers.iter().collect::<Vec<_>>();
    let instruction = fixture.instruction.clone();
    let accounts = fixture.accounts.clone();
    if expire_blockhash {
        fixture.env.svm.expire_blockhash();
    }
    fixture.env.send(instruction, accounts, &signer_refs)
}

fn assert_core_account_alias_matrix(
    label: &str,
    role_names: &[&str],
    required_signer_roles: &[usize],
    required_writable_roles: &[usize],
    allowed_aliases: &[(usize, usize)],
    build: impl Fn() -> CoreAccountAliasFixture,
) {
    let mut control = build();
    assert_eq!(control.accounts.len(), role_names.len());
    let before = core_account_alias_snapshot(&control);
    let accepted = send_core_account_alias_fixture(&mut control, false);
    assert!(
        accepted.is_ok(),
        "{label}: canonical control must execute: {accepted:?}"
    );
    assert_ne!(
        core_account_alias_snapshot(&control),
        before,
        "{label}: canonical control must mutate tracked state"
    );

    let mut pair_count = 0usize;
    for first in 0..role_names.len() {
        for second in (first + 1)..role_names.len() {
            pair_count += 1;
            if allowed_aliases.contains(&(first, second)) {
                continue;
            }
            let mut fixture = build();
            fixture.accounts[second].pubkey = fixture.accounts[first].pubkey;
            let before = core_account_alias_snapshot(&fixture);
            let rejected = send_core_account_alias_fixture(&mut fixture, true);
            assert!(
                rejected.is_err(),
                "{label}: alias {} with {} unexpectedly succeeded",
                role_names[first],
                role_names[second]
            );
            assert_eq!(
                core_account_alias_snapshot(&fixture),
                before,
                "{label}: alias {} with {} did not roll back exactly",
                role_names[first],
                role_names[second]
            );
        }
    }
    assert_eq!(
        pair_count,
        role_names.len() * (role_names.len() - 1) / 2,
        "{label}: pair matrix must be complete"
    );

    for &role in required_signer_roles {
        let mut fixture = build();
        fixture.accounts[role].is_signer = false;
        let before = core_account_alias_snapshot(&fixture);
        let rejected = send_core_account_alias_fixture(&mut fixture, true);
        assert!(
            rejected.is_err(),
            "{label}: {} signer downgrade unexpectedly succeeded",
            role_names[role]
        );
        assert_eq!(core_account_alias_snapshot(&fixture), before);
    }
    for &role in required_writable_roles {
        let mut fixture = build();
        fixture.accounts[role].is_writable = false;
        let before = core_account_alias_snapshot(&fixture);
        let rejected = send_core_account_alias_fixture(&mut fixture, true);
        assert!(
            rejected.is_err(),
            "{label}: {} writable downgrade unexpectedly succeeded",
            role_names[role]
        );
        assert_eq!(core_account_alias_snapshot(&fixture), before);
    }
}

fn close_portfolio_alias_fixture() -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let instruction = env.close_portfolio_ix(portfolio);
    let accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    let tracked_accounts = vec![env.market, portfolio];
    CoreAccountAliasFixture {
        env,
        signers: vec![owner],
        instruction,
        accounts,
        tracked_accounts,
    }
}

fn rebalance_reduce_alias_fixture() -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&owner, portfolio, 1_000_000);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);
    env.trade_with_cu(
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        2 * POS_SCALE as i128,
        100,
        0,
    );
    let instruction = ProgInstruction::RebalanceReduce {
        portfolio_id: env.portfolio_id(portfolio),
        position_epoch: env.portfolio_position_epoch(portfolio),
        asset_index: 0,
        reduce_q: POS_SCALE,
    };
    let accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    let tracked_accounts = vec![env.market, portfolio, counterparty, env.vault];
    CoreAccountAliasFixture {
        env,
        signers: vec![owner],
        instruction,
        accounts,
        tracked_accounts,
    }
}

fn convert_released_pnl_alias_fixture() -> CoreAccountAliasFixture {
    let PublicReleasedPnlFixture {
        env,
        winner_owner,
        winner,
        loser,
    } = public_released_pnl_fixture();
    let instruction = env.convert_released_pnl_ix(winner, PUBLIC_RELEASED_PNL_FIXTURE_AMOUNT);
    let accounts = vec![
        AccountMeta::new(winner_owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(winner, false),
    ];
    let tracked_accounts = vec![env.market, winner, loser, env.vault];
    CoreAccountAliasFixture {
        env,
        signers: vec![winner_owner],
        instruction,
        accounts,
        tracked_accounts,
    }
}

fn forfeit_recovery_alias_fixture() -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(100, 1);
    env.configure_auth_mark_with_cu(0, 100);
    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&owner, portfolio, 1_000_000);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);
    env.trade_with_cu(
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        POS_SCALE as i128,
        100,
        0,
    );
    env.svm.warp_to_slot(1);
    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 1, 0);
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Recovery,
        "public shutdown must establish the Recovery fixture"
    );
    let instruction = ProgInstruction::ForfeitRecoveryLeg {
        portfolio_id: env.portfolio_id(portfolio),
        position_epoch: env.portfolio_position_epoch(portfolio),
        asset_index: 0,
        b_delta_budget: u128::MAX,
    };
    let accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    let tracked_accounts = vec![env.market, portfolio, counterparty, env.vault];
    CoreAccountAliasFixture {
        env,
        signers: vec![owner],
        instruction,
        accounts,
        tracked_accounts,
    }
}

fn cure_and_cancel_close_alias_fixture() -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: 1_000_000,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let winner_owner = Keypair::new();
    let loss_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loss = env.create_portfolio(&loss_owner);
    let cranker = env.create_portfolio(&cranker_owner);
    env.deposit(&winner_owner, winner, 1_000_000);
    env.deposit(&loss_owner, loss, 161_600);
    env.deposit(&cranker_owner, cranker, 1);
    let position_q = (POS_SCALE as i128) * 3 / 4;
    env.trade_with_cu(
        &winner_owner,
        winner,
        &loss_owner,
        loss,
        position_q,
        1_000_000,
        0,
    );
    let mut mark = 1_000_000u64;
    for slot in 1..=20 {
        mark = mark * 10_500 / 10_000;
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, mark);
        env.crank_if_actionable(
            cranker,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    let effective_price = env.market_state().1.assets[0].effective_price;
    env.trade_with_cu(
        &winner_owner,
        winner,
        &loss_owner,
        loss,
        -position_q,
        effective_price,
        0,
    );
    let close = close_progress(&env.portfolio_state(loss));
    assert!(
        close.active
            && !close.canceled
            && !close.finalized
            && close.support_consumed == 0
            && close.junior_face_burned == 0
            && close.insurance_spent == 0
            && close.b_loss_booked == 0
            && close.explicit_loss_assigned == 0
            && close.quantity_adl_applied_q == 0
            && close.drift_consumed == 0
            && close.residual_remaining != 0
            && close.residual_remaining == close.gross_loss_at_close_start,
        "public trade reduction must create a cancellable close: {close:?}"
    );
    let deposit = 100_000_000_000u128;
    let source = env.token_account_for_mint(
        env.mint,
        loss_owner.pubkey(),
        u64::try_from(deposit).expect("public close residual fits SPL atoms"),
    );
    let instruction = ProgInstruction::CureAndCancelClose {
        portfolio_id: env.portfolio_id(loss),
        position_epoch: env.portfolio_position_epoch(loss),
        optional_deposit: deposit,
    };
    let accounts = vec![
        AccountMeta::new(loss_owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(loss, false),
        AccountMeta::new(source, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    let tracked_accounts = vec![env.market, loss, winner, cranker, source, env.vault];
    CoreAccountAliasFixture {
        env,
        signers: vec![loss_owner],
        instruction,
        accounts,
        tracked_accounts,
    }
}

#[derive(Clone, Copy, Debug)]
enum BaseUnitMintAliasShape {
    InitialPair,
    ReplacePrimary,
    ReplaceSecondary,
    ReplaceBoth,
}

impl BaseUnitMintAliasShape {
    fn role_names(self) -> &'static [&'static str] {
        match self {
            Self::InitialPair => &["authority", "market", "primary_mint", "secondary_mint"],
            Self::ReplacePrimary => &[
                "authority",
                "market",
                "primary_mint",
                "secondary_mint",
                "old_primary_vault",
            ],
            Self::ReplaceSecondary => &[
                "authority",
                "market",
                "primary_mint",
                "secondary_mint",
                "old_secondary_vault",
            ],
            Self::ReplaceBoth => &[
                "authority",
                "market",
                "primary_mint",
                "secondary_mint",
                "old_primary_vault",
                "old_secondary_vault",
            ],
        }
    }
}

fn base_unit_mint_alias_fixture(shape: BaseUnitMintAliasShape) -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let authority = env.admin.insecure_clone();
    let old_secondary = env.create_mint();
    let replacing_existing = !matches!(shape, BaseUnitMintAliasShape::InitialPair);
    if replacing_existing {
        env.update_base_unit_mints_with_cu(env.mint, old_secondary);
    }
    let old_secondary_vault = canonical_vault_ata(env.vault_authority, old_secondary);
    if replacing_existing {
        env.svm
            .set_account(
                old_secondary_vault,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(old_secondary, env.vault_authority, 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .expect("create empty old secondary vault");
    }

    let replace_primary = matches!(
        shape,
        BaseUnitMintAliasShape::ReplacePrimary | BaseUnitMintAliasShape::ReplaceBoth
    );
    let replace_secondary = matches!(
        shape,
        BaseUnitMintAliasShape::ReplaceSecondary | BaseUnitMintAliasShape::ReplaceBoth
    );
    let primary_mint = if replace_primary {
        env.create_mint()
    } else {
        env.mint
    };
    let secondary_mint = if !replacing_existing || replace_secondary {
        env.create_mint()
    } else {
        old_secondary
    };
    let instruction = ProgInstruction::UpdateBaseUnitMints {
        primary_mint: primary_mint.to_bytes(),
        secondary_mint: secondary_mint.to_bytes(),
    };
    let mut accounts = vec![
        AccountMeta::new_readonly(authority.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new_readonly(primary_mint, false),
        AccountMeta::new_readonly(secondary_mint, false),
    ];
    if replace_primary {
        accounts.push(AccountMeta::new_readonly(env.vault, false));
    }
    if replacing_existing && replace_secondary {
        accounts.push(AccountMeta::new_readonly(old_secondary_vault, false));
    }
    assert_eq!(accounts.len(), shape.role_names().len());
    let tracked_accounts = std::iter::once(env.market)
        .chain(accounts.iter().map(|account| account.pubkey))
        .collect();
    CoreAccountAliasFixture {
        env,
        signers: vec![authority],
        instruction,
        accounts,
        tracked_accounts,
    }
}

fn swap_secondary_alias_fixture() -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let authority = env.admin.insecure_clone();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    let primary_source = env.token_account_for_mint(env.mint, authority.pubkey(), 10);
    let secondary_destination = env.token_account_for_mint(secondary_mint, authority.pubkey(), 0);
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 10),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .expect("create funded secondary vault");
    let accounts = vec![
        AccountMeta::new_readonly(authority.pubkey(), true),
        AccountMeta::new_readonly(env.market, false),
        AccountMeta::new(primary_source, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new(secondary_destination, false),
        AccountMeta::new(secondary_vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    let tracked_accounts = std::iter::once(env.market)
        .chain(accounts.iter().map(|account| account.pubkey))
        .collect();
    CoreAccountAliasFixture {
        env,
        signers: vec![authority],
        instruction: ProgInstruction::SwapSecondaryForPrimary { amount: 10 },
        accounts,
        tracked_accounts,
    }
}

#[derive(Clone, Copy, Debug)]
enum CloseSlabAliasShape {
    PrimaryDust,
    SecondaryDust,
    PrimaryUnbudgetedInsurance,
    SecondaryUnbudgetedInsurance,
}

impl CloseSlabAliasShape {
    fn has_secondary(self) -> bool {
        matches!(
            self,
            Self::SecondaryDust | Self::SecondaryUnbudgetedInsurance
        )
    }

    fn has_unbudgeted_insurance(self) -> bool {
        matches!(
            self,
            Self::PrimaryUnbudgetedInsurance | Self::SecondaryUnbudgetedInsurance
        )
    }

    fn role_names(self) -> &'static [&'static str] {
        match self {
            Self::PrimaryDust => &[
                "authority_destination",
                "market",
                "primary_vault",
                "vault_authority",
                "primary_destination",
                "token_program",
            ],
            Self::SecondaryDust => &[
                "authority_destination",
                "market",
                "primary_vault",
                "vault_authority",
                "primary_destination",
                "token_program",
                "secondary_vault",
                "secondary_destination",
            ],
            Self::PrimaryUnbudgetedInsurance => &[
                "authority_destination",
                "market",
                "primary_vault",
                "vault_authority",
                "primary_destination",
                "token_program",
                "primary_mint",
            ],
            Self::SecondaryUnbudgetedInsurance => &[
                "authority_destination",
                "market",
                "primary_vault",
                "vault_authority",
                "primary_destination",
                "token_program",
                "secondary_vault",
                "secondary_destination",
                "primary_mint",
            ],
        }
    }

    fn required_writable_roles(self) -> &'static [usize] {
        match self {
            Self::PrimaryDust => &[0, 1, 2, 4],
            Self::SecondaryDust => &[0, 1, 2, 4, 6, 7],
            Self::PrimaryUnbudgetedInsurance => &[0, 1, 2, 4, 6],
            Self::SecondaryUnbudgetedInsurance => &[0, 1, 2, 4, 6, 7, 8],
        }
    }
}

fn inv017_public_token_transfer(
    env: &mut V16CuEnv,
    authority: &Keypair,
    source: Pubkey,
    destination: Pubkey,
    amount: u64,
) {
    let payer = env.payer.insecure_clone();
    send_raw_tx(
        &mut env.svm,
        &payer,
        spl_token::instruction::transfer(
            &spl_token::ID,
            &source,
            &destination,
            &authority.pubkey(),
            &[],
            amount,
        )
        .expect("build public SPL transfer"),
        &[authority],
    )
    .expect("public SPL transfer");
}

fn inv017_certificate_is_current(env: &V16CuEnv, portfolio: Pubkey) -> bool {
    let group = env.market_state().1;
    let account = env.portfolio_state(portfolio);
    let cert = health_cert(&account);
    cert.valid
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == active_bitmap(&account)
}

fn inv017_set_initial_mint_supply(env: &mut V16CuEnv, supply: u64) {
    let mut account = env.svm.get_account(&env.mint).expect("primary mint");
    let mut mint = Mint::unpack(&account.data).expect("valid primary mint");
    mint.supply = supply;
    Mint::pack(mint, &mut account.data).expect("write primary mint supply");
    env.svm
        .set_account(env.mint, account)
        .expect("seed externally minted fixture supply");
}

fn inv017_create_unbudgeted_insurance(env: &mut V16CuEnv) {
    const MARK: u64 = 1_000_000;
    const RAW_UP: u64 = 2_000_000;
    const DEPOSIT: u128 = 25_000_000;
    const TERMINAL_DUST: u128 = 7;

    // The shared LiteSVM mint normally has zero synthetic supply even though deposit fixtures
    // construct funded SPL accounts. This route reaches CloseSlab's real burn, so give the mint
    // the exact pre-existing supply represented by the two public deposit sources.
    inv017_set_initial_mint_supply(env, (2 * DEPOSIT + TERMINAL_DUST) as u64);
    env.configure_ewma_mark_with_cu(0, MARK, 1, 0);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, DEPOSIT);
    env.deposit(&short_owner, short, DEPOSIT);
    env.svm.warp_to_slot(1);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        RAW_UP,
        0,
    );
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_DRAIN_ONLY, 0, 0, 0);
    for _ in 0..6 {
        for portfolio in [long, short] {
            if !inv017_certificate_is_current(env, portfolio) {
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 1,
                        observations: vec![],
                    },
                );
            }
        }
        if inv017_certificate_is_current(env, long) && inv017_certificate_is_current(env, short) {
            break;
        }
    }
    assert!(
        inv017_certificate_is_current(env, long) && inv017_certificate_is_current(env, short),
        "public DrainOnly fixture must reach current certificates"
    );
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        1,
        0,
    );
    let released = env.portfolio_state(long).pnl.get();
    if released > 0 {
        env.convert_released_pnl_with_cu(&long_owner, long, released as u128);
    }
    for (owner, portfolio) in [(&long_owner, long), (&short_owner, short)] {
        let capital = env.portfolio_state(portfolio).capital.get();
        env.withdraw(owner, portfolio, capital);
        env.close_portfolio_with_cu(owner, portfolio);
    }
    let terminal = env.market_state().1;
    assert!(
        terminal.insurance > 0,
        "paid mark movement must create insurance"
    );
    assert_eq!(
        terminal.insurance_domain_budget.iter().sum::<u128>(),
        0,
        "mark-movement insurance must remain outside withdrawable budgets"
    );
    assert_eq!(terminal.vault, terminal.insurance);
    assert_eq!(terminal.c_tot, 0);
    assert_eq!(terminal.materialized_portfolio_count, 0);
}

fn close_slab_alias_fixture(shape: CloseSlabAliasShape) -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: 1_000_000,
        max_trading_fee_bps: 100,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 1,
        ..V16CuMarketParams::default()
    });
    let authority = env.admin.insecure_clone();
    let secondary = shape.has_secondary().then(|| env.create_mint());
    if let Some(secondary_mint) = secondary {
        env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    }
    if shape.has_unbudgeted_insurance() {
        inv017_create_unbudgeted_insurance(&mut env);
    }
    let source = env.token_account_for_mint(env.mint, authority.pubkey(), 7);
    let primary_vault = env.vault;
    inv017_public_token_transfer(&mut env, &authority, source, primary_vault, 7);

    let secondary_vault = secondary.map(|secondary_mint| {
        let vault = canonical_vault_ata(env.vault_authority, secondary_mint);
        env.svm
            .set_account(
                vault,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(secondary_mint, env.vault_authority, 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .expect("create empty secondary vault");
        let source = env.token_account_for_mint(secondary_mint, authority.pubkey(), 11);
        inv017_public_token_transfer(&mut env, &authority, source, vault, 11);
        vault
    });
    env.resolve();
    let primary_destination = env.token_account(authority.pubkey(), 0);
    let secondary_destination =
        secondary.map(|mint| env.token_account_for_mint(mint, authority.pubkey(), 0));
    let mut accounts = vec![
        AccountMeta::new(authority.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new(primary_destination, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    if let (Some(vault), Some(destination)) = (secondary_vault, secondary_destination) {
        accounts.push(AccountMeta::new(vault, false));
        accounts.push(AccountMeta::new(destination, false));
    }
    if shape.has_unbudgeted_insurance() {
        accounts.push(AccountMeta::new(env.mint, false));
    }
    assert_eq!(accounts.len(), shape.role_names().len());
    let tracked_accounts = std::iter::once(env.market)
        .chain(accounts.iter().map(|account| account.pubkey))
        .collect();
    CoreAccountAliasFixture {
        env,
        signers: vec![authority],
        instruction: ProgInstruction::CloseSlab,
        accounts,
        tracked_accounts,
    }
}

fn force_close_abandoned_asset_alias_fixture() -> CoreAccountAliasFixture {
    const MARK: u64 = 100;
    const MARK_SLOT: u64 = 1;
    const SHUTDOWN_SLOT: u64 = 2;
    const FORCE_CLOSE_SLOT: u64 = 3;

    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, MARK);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000);
    env.deposit(&short_owner, short, 10_000);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        MARK,
        0,
    );
    env.configure_permissionless_resolve_with_cu(100, 1);
    env.svm.warp_to_slot(MARK_SLOT);
    env.push_auth_mark_with_cu(MARK_SLOT, MARK);
    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        0,
        SHUTDOWN_SLOT,
        0,
    );
    env.svm.warp_to_slot(FORCE_CLOSE_SLOT);

    let cranker = Keypair::new();
    env.ensure_signer_account(cranker.pubkey());
    let accounts = vec![
        AccountMeta::new_readonly(cranker.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(long, false),
        AccountMeta::new(short, false),
    ];
    CoreAccountAliasFixture {
        tracked_accounts: vec![env.market, long, short],
        env,
        signers: vec![cranker],
        instruction: ProgInstruction::ForceCloseAbandonedAsset {
            asset_index: 0,
            now_slot: FORCE_CLOSE_SLOT,
            close_q: POS_SCALE,
        },
        accounts,
    }
}

fn sync_maintenance_alias_fixture(with_cranker: bool) -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 25,
    );
    env.update_maintenance_fee_policy_with_cu(4_000);
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    let cranker = with_cranker.then(|| {
        let cranker_owner = Keypair::new();
        env.create_portfolio(&cranker_owner)
    });
    env.svm.warp_to_slot(10);
    let mut accounts = vec![
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    let mut tracked_accounts = vec![env.market, portfolio, env.vault];
    if let Some(cranker) = cranker {
        accounts.push(AccountMeta::new(cranker, false));
        tracked_accounts.push(cranker);
    }
    CoreAccountAliasFixture {
        env,
        signers: vec![],
        instruction: ProgInstruction::SyncMaintenanceFee { now_slot: 10 },
        accounts,
        tracked_accounts,
    }
}

fn permissionless_crank_alias_fixture() -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);
    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&owner, portfolio, 1_000_000);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);
    env.trade_with_cu(
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        POS_SCALE as i128,
        100,
        0,
    );
    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 110);
    let cranker = Keypair::new();
    env.ensure_signer_account(cranker.pubkey());
    let accounts = vec![
        AccountMeta::new(cranker.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    let tracked_accounts = vec![env.market, portfolio, counterparty, env.vault];
    CoreAccountAliasFixture {
        env,
        signers: vec![cranker],
        instruction: ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        accounts,
        tracked_accounts,
    }
}

fn permissionless_crank_oracle_alias_fixture(
    leg_count: usize,
    reward_enabled: bool,
    include_reward: bool,
) -> CoreAccountAliasFixture {
    assert!((1..=3).contains(&leg_count));
    assert!(!include_reward || reward_enabled);

    let mut configured = configure_hybrid_role_fixture(leg_count);
    let configure_instruction = configure_hybrid_role_instruction(&configured);
    let configure_accounts = configure_hybrid_role_accounts(&configured);
    configured
        .env
        .send(
            configure_instruction,
            configure_accounts,
            &[&configured.authority],
        )
        .expect("configure coherent hybrid oracle for crank matrix");

    let feeds = configured.feeds;
    let mut env = configured.env;
    if reward_enabled {
        env.update_liquidation_fee_policy_with_cu(5_000);
    }
    let target_owner = Keypair::new();
    let target = env.create_portfolio(&target_owner);
    let cranker = Keypair::new();
    env.ensure_signer_account(cranker.pubkey());
    let reward = include_reward.then(|| env.create_portfolio(&cranker));

    set_test_clock(&mut env, 2, 101);
    let oracle_accounts = feeds[..leg_count]
        .iter()
        .map(|feed| env.set_pyth_price_with_conf(feed, 1_000_000, -6, 0, 101))
        .collect::<Vec<_>>();
    let mut accounts = vec![
        AccountMeta::new_readonly(cranker.pubkey(), include_reward),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    accounts.extend(
        oracle_accounts
            .iter()
            .copied()
            .map(|key| AccountMeta::new_readonly(key, false)),
    );
    if let Some(reward) = reward {
        accounts.push(AccountMeta::new(reward, false));
    }
    let tracked_accounts = std::iter::once(env.market)
        .chain(accounts.iter().map(|account| account.pubkey))
        .collect();
    CoreAccountAliasFixture {
        env,
        signers: include_reward.then_some(cranker).into_iter().collect(),
        instruction: ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations_with_accounts(0, leg_count as u8),
        },
        accounts,
        tracked_accounts,
    }
}

#[derive(Clone, Copy)]
enum AuthorityHandoffAliasRoute {
    Market,
    AssetOracle,
}

impl AuthorityHandoffAliasRoute {
    fn label(self) -> &'static str {
        match self {
            Self::Market => "UpdateAuthority",
            Self::AssetOracle => "UpdateAssetAuthority oracle",
        }
    }
}

fn authority_handoff_alias_fixture(route: AuthorityHandoffAliasRoute) -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let current = env.admin.insecure_clone();
    let incoming = Keypair::new();
    env.ensure_signer_account(incoming.pubkey());
    let instruction = match route {
        AuthorityHandoffAliasRoute::Market => ProgInstruction::UpdateAuthority {
            new_pubkey: incoming.pubkey().to_bytes(),
        },
        AuthorityHandoffAliasRoute::AssetOracle => ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            kind: processor::ASSET_AUTH_ORACLE,
            new_pubkey: incoming.pubkey().to_bytes(),
        },
    };
    let accounts = vec![
        AccountMeta::new(current.pubkey(), true),
        AccountMeta::new(incoming.pubkey(), true),
        AccountMeta::new(env.market, false),
    ];
    let tracked_accounts = vec![env.market];
    CoreAccountAliasFixture {
        env,
        signers: vec![current, incoming],
        instruction,
        accounts,
        tracked_accounts,
    }
}

fn init_portfolio_alias_fixture() -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    env.ensure_signer_account(owner.pubkey());
    let portfolio = env.program_account(env.portfolio_account_len);
    let accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    let tracked_accounts = vec![env.market, portfolio];
    CoreAccountAliasFixture {
        env,
        signers: vec![owner],
        instruction: ProgInstruction::InitPortfolio,
        accounts,
        tracked_accounts,
    }
}

#[derive(Clone, Copy)]
enum MatcherConfigAliasRoute {
    Disabled,
    Enabled,
}

impl MatcherConfigAliasRoute {
    fn label(self) -> &'static str {
        match self {
            Self::Disabled => "SetMatcherConfig disabled",
            Self::Enabled => "SetMatcherConfig enabled",
        }
    }
}

fn matcher_config_alias_fixture(route: MatcherConfigAliasRoute) -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let mut accounts = vec![
        AccountMeta::new(owner.pubkey(), true),
        AccountMeta::new_readonly(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    let enabled = match route {
        MatcherConfigAliasRoute::Disabled => 0,
        MatcherConfigAliasRoute::Enabled => {
            let matcher_program = Pubkey::new_unique();
            let matcher_bytes =
                std::fs::read(matcher_program_path()).expect("read matcher program");
            env.svm.add_program(matcher_program, &matcher_bytes);
            let (matcher_context, matcher_delegate, _) =
                env.init_matcher_context(matcher_program, portfolio);
            accounts.extend([
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new_readonly(matcher_context, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ]);
            1
        }
    };
    let instruction = ProgInstruction::SetMatcherConfig {
        portfolio_id: env.portfolio_id(portfolio),
        expected_sequence: env.portfolio_matcher_sequence(portfolio),
        enabled,
        trade_fee_cap_bps: if enabled == 0 { 0 } else { 10_000 },
    };
    let mut tracked_accounts = vec![env.market, portfolio];
    tracked_accounts.extend(accounts.iter().skip(3).map(|account| account.pubkey));
    CoreAccountAliasFixture {
        env,
        signers: vec![owner],
        instruction,
        accounts,
        tracked_accounts,
    }
}

#[derive(Clone, Copy)]
enum LedgerSyncAliasRoute {
    Backing,
    Insurance,
}

impl LedgerSyncAliasRoute {
    fn label(self) -> &'static str {
        match self {
            Self::Backing => "SyncBackingDomainLedger",
            Self::Insurance => "SyncInsuranceLedger",
        }
    }
}

fn ledger_sync_alias_fixture(route: LedgerSyncAliasRoute) -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let authority = env.admin.insecure_clone();
    let ledger = match route {
        LedgerSyncAliasRoute::Backing => env.backing_domain_ledger_account(),
        LedgerSyncAliasRoute::Insurance => env.insurance_ledger_account(),
    };
    let instruction = match route {
        LedgerSyncAliasRoute::Backing => ProgInstruction::SyncBackingDomainLedger { domain: 0 },
        LedgerSyncAliasRoute::Insurance => ProgInstruction::SyncInsuranceLedger,
    };
    let accounts = vec![
        AccountMeta::new(authority.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(ledger, false),
    ];
    let tracked_accounts = vec![env.market, ledger];
    CoreAccountAliasFixture {
        env,
        signers: vec![authority],
        instruction,
        accounts,
        tracked_accounts,
    }
}

#[derive(Clone, Copy)]
enum TwoAccountPolicyAliasRoute {
    LiquidationFee,
    MaintenanceFee,
    TradeFee,
    FeeRedirect,
    MarketInitFee,
    BackingFee,
    PermissionlessResolve,
}

impl TwoAccountPolicyAliasRoute {
    const ALL: [Self; 7] = [
        Self::LiquidationFee,
        Self::MaintenanceFee,
        Self::TradeFee,
        Self::FeeRedirect,
        Self::MarketInitFee,
        Self::BackingFee,
        Self::PermissionlessResolve,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::LiquidationFee => "UpdateLiquidationFeePolicy",
            Self::MaintenanceFee => "UpdateMaintenanceFeePolicy",
            Self::TradeFee => "UpdateTradeFeePolicy",
            Self::FeeRedirect => "UpdateFeeRedirectPolicy",
            Self::MarketInitFee => "UpdateMarketInitFeePolicy",
            Self::BackingFee => "UpdateBackingFeePolicy",
            Self::PermissionlessResolve => "ConfigurePermissionlessResolve",
        }
    }
}

fn two_account_policy_alias_fixture(route: TwoAccountPolicyAliasRoute) -> CoreAccountAliasFixture {
    let env = V16CuEnv::new();
    let authority = env.admin.insecure_clone();
    let sequences = env.control_sequences(0);
    let instruction = match route {
        TwoAccountPolicyAliasRoute::LiquidationFee => ProgInstruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 1,
            policy_sequence: next_control_sequence(sequences.liquidation_fee),
        },
        TwoAccountPolicyAliasRoute::MaintenanceFee => ProgInstruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 1,
            policy_sequence: next_control_sequence(sequences.maintenance_fee),
        },
        TwoAccountPolicyAliasRoute::TradeFee => ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 1,
            policy_sequence: next_control_sequence(sequences.trade_fee),
        },
        TwoAccountPolicyAliasRoute::FeeRedirect => ProgInstruction::UpdateFeeRedirectPolicy {
            redirect_bps: 1,
            policy_sequence: next_control_sequence(sequences.fee_redirect),
        },
        TwoAccountPolicyAliasRoute::MarketInitFee => ProgInstruction::UpdateMarketInitFeePolicy {
            min_init_fee: 1,
            policy_sequence: next_control_sequence(sequences.market_init_fee),
        },
        TwoAccountPolicyAliasRoute::BackingFee => ProgInstruction::UpdateBackingFeePolicy {
            domain: 0,
            market_id: env.asset_market_id(0),
            fee_bps: 1,
            insurance_share_bps: 0,
            policy_sequence: next_control_sequence(sequences.backing_fee_long),
        },
        TwoAccountPolicyAliasRoute::PermissionlessResolve => {
            ProgInstruction::ConfigurePermissionlessResolve {
                asset_generation_frontier: env.market_state().1.next_market_id,
                stale_slots: 100,
                force_close_delay_slots: 100,
                policy_sequence: next_control_sequence(sequences.permissionless_resolve),
            }
        }
    };
    let accounts = vec![
        AccountMeta::new(authority.pubkey(), true),
        AccountMeta::new(env.market, false),
    ];
    let tracked_accounts = vec![env.market];
    CoreAccountAliasFixture {
        env,
        signers: vec![authority],
        instruction,
        accounts,
        tracked_accounts,
    }
}

#[derive(Clone, Copy)]
enum ManagedMarkAliasRoute {
    ConfigureEwma,
    PushEwma,
    ConfigureAuthority,
    PushAuthority,
}

impl ManagedMarkAliasRoute {
    const ALL: [Self; 4] = [
        Self::ConfigureEwma,
        Self::PushEwma,
        Self::ConfigureAuthority,
        Self::PushAuthority,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ConfigureEwma => "ConfigureEwmaMark",
            Self::PushEwma => "PushEwmaMark",
            Self::ConfigureAuthority => "ConfigureAuthMark",
            Self::PushAuthority => "PushAuthMark",
        }
    }
}

fn managed_mark_alias_fixture(route: ManagedMarkAliasRoute) -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let authority = env.admin.insecure_clone();
    if matches!(route, ManagedMarkAliasRoute::PushEwma) {
        env.configure_ewma_mark_with_cu(0, 100, 1, 0);
        env.svm.warp_to_slot(1);
    } else if matches!(route, ManagedMarkAliasRoute::PushAuthority) {
        env.configure_auth_mark_with_cu(0, 100);
        env.svm.warp_to_slot(1);
    }
    let now_slot = env.svm.get_sysvar::<Clock>().slot;
    let observation_sequence = next_control_sequence(env.control_sequences(0).oracle_observation);
    let market_id = env.asset_market_id(0);
    let instruction = match route {
        ManagedMarkAliasRoute::ConfigureEwma => ProgInstruction::ConfigureEwmaMark {
            asset_index: 0,
            market_id,
            now_slot,
            initial_mark_e6: 101,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            observation_sequence,
        },
        ManagedMarkAliasRoute::PushEwma => ProgInstruction::PushEwmaMark {
            asset_index: 0,
            market_id,
            now_slot,
            mark_e6: 101,
            observation_sequence,
        },
        ManagedMarkAliasRoute::ConfigureAuthority => ProgInstruction::ConfigureAuthMark {
            asset_index: 0,
            market_id,
            now_slot,
            initial_mark_e6: 101,
            observation_sequence,
        },
        ManagedMarkAliasRoute::PushAuthority => ProgInstruction::PushAuthMark {
            asset_index: 0,
            market_id,
            now_slot,
            mark_e6: 101,
            observation_sequence,
        },
    };
    let accounts = vec![
        AccountMeta::new(authority.pubkey(), true),
        AccountMeta::new(env.market, false),
    ];
    let tracked_accounts = vec![env.market];
    CoreAccountAliasFixture {
        env,
        signers: vec![authority],
        instruction,
        accounts,
        tracked_accounts,
    }
}

#[derive(Clone, Copy)]
enum TwoAccountLifecycleAliasRoute {
    ResolveMarket,
    AdminAppendActivate,
    AdminReuseActivate,
    DrainOnlyAsset,
    ShutdownAsset,
    RetireAsset,
}

impl TwoAccountLifecycleAliasRoute {
    const ALL: [Self; 6] = [
        Self::ResolveMarket,
        Self::AdminAppendActivate,
        Self::AdminReuseActivate,
        Self::DrainOnlyAsset,
        Self::ShutdownAsset,
        Self::RetireAsset,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ResolveMarket => "ResolveMarket",
            Self::AdminAppendActivate => "UpdateAssetLifecycle admin append activate",
            Self::AdminReuseActivate => "UpdateAssetLifecycle admin reuse activate",
            Self::DrainOnlyAsset => "UpdateAssetLifecycle drain-only",
            Self::ShutdownAsset => "UpdateAssetLifecycle shutdown",
            Self::RetireAsset => "UpdateAssetLifecycle retire",
        }
    }
}

fn two_account_lifecycle_alias_fixture(
    route: TwoAccountLifecycleAliasRoute,
) -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    let authority = env.admin.insecure_clone();
    match route {
        TwoAccountLifecycleAliasRoute::AdminAppendActivate => {
            env.svm.warp_to_slot(1);
        }
        TwoAccountLifecycleAliasRoute::AdminReuseActivate => {
            env.svm.warp_to_slot(1);
            env.activate_asset(1, 1, 100);
            env.svm.warp_to_slot(2);
            env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_RETIRE, 1, 2, 0);
            env.svm.warp_to_slot(3);
        }
        TwoAccountLifecycleAliasRoute::ShutdownAsset => {
            env.configure_permissionless_resolve_with_cu(100, 1);
            env.svm.warp_to_slot(1);
        }
        TwoAccountLifecycleAliasRoute::RetireAsset => {
            env.svm.warp_to_slot(1);
            env.activate_asset(1, 1, 100);
            env.svm.warp_to_slot(2);
        }
        TwoAccountLifecycleAliasRoute::ResolveMarket
        | TwoAccountLifecycleAliasRoute::DrainOnlyAsset => {}
    }
    let instruction = match route {
        TwoAccountLifecycleAliasRoute::ResolveMarket => ProgInstruction::ResolveMarket {
            asset_generation_frontier: env.market_state().1.next_market_id,
        },
        TwoAccountLifecycleAliasRoute::AdminAppendActivate => {
            ProgInstruction::UpdateAssetLifecycle {
                action: processor::ASSET_ACTION_ACTIVATE,
                asset_index: 1,
                market_id: env.market_state().1.next_market_id,
                now_slot: 1,
                initial_price: 100,
                max_init_fee: u128::MAX,
                insurance_authority: authority.pubkey().to_bytes(),
                insurance_operator: authority.pubkey().to_bytes(),
                backing_bucket_authority: authority.pubkey().to_bytes(),
                oracle_authority: authority.pubkey().to_bytes(),
            }
        }
        TwoAccountLifecycleAliasRoute::AdminReuseActivate => {
            ProgInstruction::UpdateAssetLifecycle {
                action: processor::ASSET_ACTION_ACTIVATE,
                asset_index: 1,
                market_id: env.market_state().1.next_market_id,
                now_slot: 3,
                initial_price: 101,
                max_init_fee: u128::MAX,
                insurance_authority: authority.pubkey().to_bytes(),
                insurance_operator: authority.pubkey().to_bytes(),
                backing_bucket_authority: authority.pubkey().to_bytes(),
                oracle_authority: authority.pubkey().to_bytes(),
            }
        }
        TwoAccountLifecycleAliasRoute::DrainOnlyAsset => ProgInstruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_DRAIN_ONLY,
            asset_index: 0,
            market_id: env.asset_market_id(0),
            now_slot: 0,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: authority.pubkey().to_bytes(),
            insurance_operator: authority.pubkey().to_bytes(),
            backing_bucket_authority: authority.pubkey().to_bytes(),
            oracle_authority: authority.pubkey().to_bytes(),
        },
        TwoAccountLifecycleAliasRoute::ShutdownAsset => ProgInstruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_SHUTDOWN,
            asset_index: 0,
            market_id: env.asset_market_id(0),
            now_slot: 1,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: authority.pubkey().to_bytes(),
            insurance_operator: authority.pubkey().to_bytes(),
            backing_bucket_authority: authority.pubkey().to_bytes(),
            oracle_authority: authority.pubkey().to_bytes(),
        },
        TwoAccountLifecycleAliasRoute::RetireAsset => ProgInstruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_RETIRE,
            asset_index: 1,
            market_id: env.asset_market_id(1),
            now_slot: 2,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: authority.pubkey().to_bytes(),
            insurance_operator: authority.pubkey().to_bytes(),
            backing_bucket_authority: authority.pubkey().to_bytes(),
            oracle_authority: authority.pubkey().to_bytes(),
        },
    };
    let accounts = vec![
        AccountMeta::new(authority.pubkey(), true),
        AccountMeta::new(env.market, false),
    ];
    let tracked_accounts = vec![env.market];
    CoreAccountAliasFixture {
        env,
        signers: vec![authority],
        instruction,
        accounts,
        tracked_accounts,
    }
}

#[derive(Clone, Copy, Debug)]
enum PermissionlessActivationAliasShape {
    Append,
    Reuse,
}

fn permissionless_activation_alias_fixture(
    shape: PermissionlessActivationAliasShape,
) -> CoreAccountAliasFixture {
    const FEE: u128 = 10;

    let mut env = V16CuEnv::new();
    if matches!(shape, PermissionlessActivationAliasShape::Reuse) {
        env.svm.warp_to_slot(1);
        env.activate_asset(1, 1, 100);
        env.svm.warp_to_slot(2);
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_RETIRE, 1, 2, 0);
    }
    env.update_market_init_fee_policy_with_cu(FEE);
    let now_slot = match shape {
        PermissionlessActivationAliasShape::Append => 1,
        PermissionlessActivationAliasShape::Reuse => 3,
    };
    env.svm.warp_to_slot(now_slot);
    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    let source = env.token_account(creator.pubkey(), FEE as u64);
    let instruction = ProgInstruction::UpdateAssetLifecycle {
        action: processor::ASSET_ACTION_ACTIVATE,
        asset_index: 1,
        market_id: env.market_state().1.next_market_id,
        now_slot,
        initial_price: 101,
        max_init_fee: FEE,
        insurance_authority: creator.pubkey().to_bytes(),
        insurance_operator: creator.pubkey().to_bytes(),
        backing_bucket_authority: creator.pubkey().to_bytes(),
        oracle_authority: creator.pubkey().to_bytes(),
    };
    let accounts = vec![
        AccountMeta::new(creator.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(source, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    let tracked_accounts = std::iter::once(env.market)
        .chain(accounts.iter().map(|account| account.pubkey))
        .collect();
    CoreAccountAliasFixture {
        env,
        signers: vec![creator],
        instruction,
        accounts,
        tracked_accounts,
    }
}

fn permissionless_resolve_alias_fixture() -> CoreAccountAliasFixture {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(1, 1);
    env.svm.warp_to_slot(2);
    let accounts = vec![AccountMeta::new(env.market, false)];
    CoreAccountAliasFixture {
        tracked_accounts: vec![env.market],
        env,
        signers: vec![],
        instruction: ProgInstruction::ResolveStalePermissionless { now_slot: 2 },
        accounts,
    }
}

#[test]
fn v16_program_authority_handoff_account_pairs_and_privileges_are_exhaustive() {
    for route in [
        AuthorityHandoffAliasRoute::Market,
        AuthorityHandoffAliasRoute::AssetOracle,
    ] {
        assert_core_account_alias_matrix(
            route.label(),
            &["current_authority", "incoming_authority", "market"],
            &[0, 1],
            &[2],
            &[],
            || authority_handoff_alias_fixture(route),
        );
    }
}

#[test]
fn v16_program_ledger_sync_account_roles_are_exhaustive() {
    for route in [
        LedgerSyncAliasRoute::Backing,
        LedgerSyncAliasRoute::Insurance,
    ] {
        assert_core_account_alias_matrix(
            route.label(),
            &["authority", "market", "ledger"],
            &[0],
            &[1, 2],
            &[],
            || ledger_sync_alias_fixture(route),
        );
    }
}

#[test]
fn v16_program_two_account_policy_roles_are_exhaustive() {
    for route in TwoAccountPolicyAliasRoute::ALL {
        assert_core_account_alias_matrix(
            route.label(),
            &["authority", "market"],
            &[0],
            &[1],
            &[],
            || two_account_policy_alias_fixture(route),
        );
    }
}

#[test]
fn v16_program_managed_mark_account_roles_are_exhaustive() {
    for route in ManagedMarkAliasRoute::ALL {
        assert_core_account_alias_matrix(
            route.label(),
            &["oracle_authority", "market"],
            &[0],
            &[1],
            &[],
            || managed_mark_alias_fixture(route),
        );
    }
}

#[test]
fn v16_program_two_account_lifecycle_roles_are_exhaustive() {
    for route in TwoAccountLifecycleAliasRoute::ALL {
        assert_core_account_alias_matrix(
            route.label(),
            &["authority", "market"],
            &[0],
            &[1],
            &[],
            || two_account_lifecycle_alias_fixture(route),
        );
    }
    for shape in [
        PermissionlessActivationAliasShape::Append,
        PermissionlessActivationAliasShape::Reuse,
    ] {
        assert_core_account_alias_matrix(
            &format!("UpdateAssetLifecycle permissionless {shape:?}"),
            &[
                "creator",
                "market",
                "source_token",
                "vault",
                "token_program",
            ],
            &[0],
            &[1, 2, 3],
            &[],
            || permissionless_activation_alias_fixture(shape),
        );
    }
}

#[test]
fn v16_program_permissionless_resolve_required_privileges_are_exhaustive() {
    assert_core_account_alias_matrix(
        "ResolveStalePermissionless",
        &["market"],
        &[],
        &[0],
        &[],
        permissionless_resolve_alias_fixture,
    );
}

#[test]
fn v16_program_initialization_and_matcher_config_account_roles_are_exhaustive() {
    assert_core_account_alias_matrix(
        "InitPortfolio",
        &["owner", "market", "portfolio"],
        &[0],
        &[1, 2],
        &[],
        init_portfolio_alias_fixture,
    );
    assert_core_account_alias_matrix(
        MatcherConfigAliasRoute::Disabled.label(),
        &["owner", "market", "portfolio"],
        &[0],
        &[2],
        &[],
        || matcher_config_alias_fixture(MatcherConfigAliasRoute::Disabled),
    );
    assert_core_account_alias_matrix(
        MatcherConfigAliasRoute::Enabled.label(),
        &[
            "owner",
            "market",
            "portfolio",
            "matcher_program",
            "matcher_context",
            "matcher_delegate",
        ],
        &[0],
        &[2],
        &[],
        || matcher_config_alias_fixture(MatcherConfigAliasRoute::Enabled),
    );
}

#[test]
fn v16_program_exit_and_maintenance_core_account_pairs_are_exhaustive() {
    assert_core_account_alias_matrix(
        "ClosePortfolio",
        &["owner", "market", "portfolio"],
        &[0],
        &[1, 2],
        &[],
        close_portfolio_alias_fixture,
    );
    assert_core_account_alias_matrix(
        "RebalanceReduce",
        &["owner", "market", "portfolio"],
        &[0],
        &[1, 2],
        &[],
        rebalance_reduce_alias_fixture,
    );
    assert_core_account_alias_matrix(
        "ConvertReleasedPnl",
        &["owner", "market", "portfolio"],
        &[0],
        &[1, 2],
        &[],
        convert_released_pnl_alias_fixture,
    );
    assert_core_account_alias_matrix(
        "ForfeitRecoveryLeg",
        &["owner", "market", "portfolio"],
        &[0],
        &[1, 2],
        &[],
        forfeit_recovery_alias_fixture,
    );
    assert_core_account_alias_matrix(
        "CureAndCancelClose with public residual deposit",
        &[
            "owner",
            "market",
            "portfolio",
            "source_token",
            "vault",
            "token_program",
        ],
        &[0],
        &[1, 2, 3, 4],
        &[],
        cure_and_cancel_close_alias_fixture,
    );
    assert_core_account_alias_matrix(
        "SyncMaintenanceFee",
        &["market", "portfolio"],
        &[],
        &[0, 1],
        &[],
        || sync_maintenance_alias_fixture(false),
    );
    assert_core_account_alias_matrix(
        "SyncMaintenanceFee with cranker",
        &["market", "portfolio", "cranker_portfolio"],
        &[],
        &[0, 1, 2],
        &[(1, 2)],
        || sync_maintenance_alias_fixture(true),
    );
    assert_core_account_alias_matrix(
        "PermissionlessCrank",
        &["cranker", "market", "portfolio"],
        &[],
        &[1, 2],
        &[],
        permissionless_crank_alias_fixture,
    );
}

#[test]
fn v16_program_base_unit_and_swap_account_roles_are_exhaustive() {
    for shape in [
        BaseUnitMintAliasShape::InitialPair,
        BaseUnitMintAliasShape::ReplacePrimary,
        BaseUnitMintAliasShape::ReplaceSecondary,
        BaseUnitMintAliasShape::ReplaceBoth,
    ] {
        assert_core_account_alias_matrix(
            &format!("UpdateBaseUnitMints {shape:?}"),
            shape.role_names(),
            &[0],
            &[1],
            &[],
            || base_unit_mint_alias_fixture(shape),
        );
    }
    assert_core_account_alias_matrix(
        "SwapSecondaryForPrimary",
        &[
            "authority",
            "market",
            "primary_source",
            "primary_vault",
            "secondary_destination",
            "secondary_vault",
            "vault_authority",
            "token_program",
        ],
        &[0],
        &[2, 3, 4, 5],
        &[],
        swap_secondary_alias_fixture,
    );
}

#[test]
fn v16_program_close_slab_account_roles_are_exhaustive() {
    for shape in [
        CloseSlabAliasShape::PrimaryDust,
        CloseSlabAliasShape::SecondaryDust,
        CloseSlabAliasShape::PrimaryUnbudgetedInsurance,
        CloseSlabAliasShape::SecondaryUnbudgetedInsurance,
    ] {
        assert_core_account_alias_matrix(
            &format!("CloseSlab {shape:?}"),
            shape.role_names(),
            &[0],
            shape.required_writable_roles(),
            &[],
            || close_slab_alias_fixture(shape),
        );
    }
}

#[test]
fn v16_program_force_close_abandoned_asset_account_roles_are_exhaustive() {
    assert_core_account_alias_matrix(
        "ForceCloseAbandonedAsset",
        &["cranker", "market", "account_a", "account_b"],
        &[0],
        &[1, 2, 3],
        &[],
        force_close_abandoned_asset_alias_fixture,
    );
}

#[test]
fn v16_program_crank_authenticated_oracle_account_roles_are_exhaustive() {
    const ORACLE_ROLES: [&str; 3] = ["oracle_1", "oracle_2", "oracle_3"];

    for leg_count in 1..=3 {
        let mut roles = vec!["cranker", "market", "portfolio"];
        roles.extend_from_slice(&ORACLE_ROLES[..leg_count]);
        assert_core_account_alias_matrix(
            &format!("PermissionlessCrank {leg_count}-provider tail"),
            &roles,
            &[],
            &[1, 2],
            &[],
            || permissionless_crank_oracle_alias_fixture(leg_count, false, false),
        );

        let mut reward_roles = roles.clone();
        reward_roles.push("reward_portfolio");
        let reward_index = reward_roles.len() - 1;
        assert_core_account_alias_matrix(
            &format!("PermissionlessCrank {leg_count}-provider reward tail"),
            &reward_roles,
            &[0],
            &[1, 2, reward_index],
            &[],
            || permissionless_crank_oracle_alias_fixture(leg_count, true, true),
        );

        let mut omitted = permissionless_crank_oracle_alias_fixture(leg_count, true, false);
        let before = core_account_alias_snapshot(&omitted);
        send_core_account_alias_fixture(&mut omitted, false).unwrap_or_else(|error| {
            panic!(
                "PermissionlessCrank {leg_count}-provider tail must remain live when the optional reward portfolio is omitted: {error}"
            )
        });
        assert_ne!(
            core_account_alias_snapshot(&omitted),
            before,
            "reward-enabled omitted-tail control must perform authenticated oracle progress"
        );
    }
}

#[test]
fn v16_program_crank_without_reward_tail_is_signature_independent() {
    let mut signed = permissionless_crank_alias_fixture();
    send_core_account_alias_fixture(&mut signed, false).expect("signed crank control succeeds");
    let signed_group = signed.env.market_state().1;
    let signed_portfolio = signed.env.portfolio_state(signed.accounts[2].pubkey);

    let mut unsigned = permissionless_crank_alias_fixture();
    unsigned.accounts[0].is_signer = false;
    send_core_account_alias_fixture(&mut unsigned, false)
        .expect("permissionless crank without a reward tail needs no caller signature");
    let unsigned_group = unsigned.env.market_state().1;
    let unsigned_portfolio = unsigned.env.portfolio_state(unsigned.accounts[2].pubkey);

    assert_eq!(
        unsigned_group.assets[0].effective_price,
        signed_group.assets[0].effective_price
    );
    assert_eq!(unsigned_group.oracle_epoch, signed_group.oracle_epoch);
    assert_eq!(unsigned_group.funding_epoch, signed_group.funding_epoch);
    assert_eq!(
        unsigned_portfolio.capital.get(),
        signed_portfolio.capital.get()
    );
    assert_eq!(unsigned_portfolio.pnl.get(), signed_portfolio.pnl.get());
    assert_eq!(unsigned_portfolio.health_cert, signed_portfolio.health_cert);
    assert_eq!(unsigned_portfolio.legs, signed_portfolio.legs);
    assert_eq!(
        unsigned.env.token_amount(unsigned.env.vault),
        signed.env.token_amount(signed.env.vault)
    );
}

#[test]
fn v16_program_self_cranker_alias_matches_distinct_cranker_aggregate() {
    let mut distinct = sync_maintenance_alias_fixture(true);
    let distinct_charged = distinct.accounts[1].pubkey;
    let distinct_cranker = distinct.accounts[2].pubkey;
    send_core_account_alias_fixture(&mut distinct, false)
        .expect("distinct maintenance cranker control succeeds");
    let distinct_total_capital = distinct.env.portfolio_state(distinct_charged).capital.get()
        + distinct.env.portfolio_state(distinct_cranker).capital.get();
    let distinct_group = distinct.env.market_state().1;

    let mut aliased = sync_maintenance_alias_fixture(true);
    let aliased_charged = aliased.accounts[1].pubkey;
    let unused_cranker = aliased.accounts[2].pubkey;
    aliased.accounts[2].pubkey = aliased_charged;
    send_core_account_alias_fixture(&mut aliased, false)
        .expect("the charged portfolio may collect its permissionless cranker share");
    let aliased_total_capital = aliased.env.portfolio_state(aliased_charged).capital.get()
        + aliased.env.portfolio_state(unused_cranker).capital.get();
    let aliased_group = aliased.env.market_state().1;

    assert_eq!(aliased_total_capital, distinct_total_capital);
    assert_eq!(aliased_group.c_tot, distinct_group.c_tot);
    assert_eq!(aliased_group.insurance, distinct_group.insurance);
    assert_eq!(
        aliased_group.insurance_domain_budget,
        distinct_group.insurance_domain_budget
    );
    assert_eq!(aliased_group.vault, distinct_group.vault);
    assert_eq!(
        aliased.env.token_amount(aliased.env.vault),
        distinct.env.token_amount(distinct.env.vault)
    );
    assert_eq!(
        aliased.env.portfolio_state(unused_cranker).capital.get(),
        0,
        "the omitted second portfolio receives no hidden credit"
    );
}

#[test]
fn v16_program_custody_token_aliases_reject_without_mutation() {
    for withdraw_path in [false, true] {
        let mut env = V16CuEnv::new();
        let owner = Keypair::new();
        let portfolio = env.create_portfolio(&owner);
        if withdraw_path {
            env.deposit(&owner, portfolio, 1_000);
        }
        let before = snapshot(&env, portfolio, None);

        env.svm.expire_blockhash();
        let result = if withdraw_path {
            env.send(
                env.withdraw_ix(portfolio, 1),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owner],
            )
        } else {
            env.send(
                env.deposit_ix(portfolio, 1),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owner],
            )
        };

        let err = result.expect_err("source/destination token alias with vault must reject");
        assert!(
            err.contains("Custom") || err.contains("InstructionError"),
            "alias rejection should be surfaced as an instruction error, got {err}"
        );
        assert_eq!(
            snapshot(&env, portfolio, None),
            before,
            "custody token alias rejection must roll back exactly"
        );
    }
}

#[test]
fn v16_program_same_portfolio_trade_alias_rejects_without_mutation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 10_000);
    let before = snapshot(&env, portfolio, None);

    env.svm.expire_blockhash();
    let result = env.send(
        env.trade_no_cpi_ix(portfolio, portfolio, 0, POS_SCALE as i128, 100, 0),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );

    let err = result.expect_err("same portfolio cannot occupy both trade roles");
    assert!(
        err.contains("AccountBorrowFailed")
            || err.contains("Custom")
            || err.contains("InstructionError"),
        "same-portfolio alias should reject at the public instruction boundary, got {err}"
    );
    assert_eq!(
        snapshot(&env, portfolio, None),
        before,
        "same-portfolio trade alias rejection must roll back exactly"
    );
}

// large enough to pass shallow storage checks; using it as the portfolio slot must reject atomically.
#[test]
fn v16_program_public_helpers_cannot_use_market_as_portfolio_alias() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 5_000, 10_000, 1_000, 25,
    );
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_portfolio = env.create_portfolio(&long_owner);
    let short_portfolio = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_portfolio, 1_000_000);
    env.deposit(&short_owner, short_portfolio, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long_portfolio,
        &short_owner,
        short_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );
    env.svm.warp_to_slot(10);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let long_before = env.svm.get_account(&long_portfolio).unwrap();
    let short_before = env.svm.get_account(&short_portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    let assert_unchanged = |env: &V16CuEnv, label: &str| {
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label}: market slab unchanged"
        );
        assert_eq!(
            env.svm.get_account(&long_portfolio).unwrap(),
            long_before,
            "{label}: real long portfolio unchanged"
        );
        assert_eq!(
            env.svm.get_account(&short_portfolio).unwrap(),
            short_before,
            "{label}: real short portfolio unchanged"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "{label}: vault custody unchanged"
        );
    };

    env.svm.expire_blockhash();
    let convert = env.send(
        ProgInstruction::ConvertReleasedPnl {
            portfolio_id: 0,
            position_epoch: 0,
            amount: 1,
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
        ],
        &[&long_owner],
    );
    assert!(
        convert.is_err(),
        "ConvertReleasedPnl must reject market-as-portfolio alias"
    );
    assert_unchanged(&env, "ConvertReleasedPnl alias rejection");

    env.svm.expire_blockhash();
    let reduce = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: 1,
            position_epoch: 0,
            asset_index: 0,
            reduce_q: POS_SCALE,
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
        ],
        &[&long_owner],
    );
    assert!(
        reduce.is_err(),
        "RebalanceReduce must reject market-as-portfolio alias"
    );
    assert_unchanged(&env, "RebalanceReduce alias rejection");

    env.svm.expire_blockhash();
    let forfeit = env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: 1,
            position_epoch: 0,
            asset_index: 0,
            b_delta_budget: 1,
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
        ],
        &[&long_owner],
    );
    assert!(
        forfeit.is_err(),
        "ForfeitRecoveryLeg must reject market-as-portfolio alias"
    );
    assert_unchanged(&env, "ForfeitRecoveryLeg alias rejection");

    env.svm.expire_blockhash();
    let close = env.send(
        ProgInstruction::ClosePortfolio {
            portfolio_id: 0,
            expected_sequence: 0,
            position_epoch: 0,
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
        ],
        &[&long_owner],
    );
    assert!(
        close.is_err(),
        "ClosePortfolio must reject market-as-portfolio alias"
    );
    assert_unchanged(&env, "ClosePortfolio alias rejection");

    env.svm.expire_blockhash();
    let fee_sync = env.send(
        ProgInstruction::SyncMaintenanceFee { now_slot: 10 },
        vec![
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
        ],
        &[],
    );
    assert!(
        fee_sync.is_err(),
        "SyncMaintenanceFee must reject market-as-portfolio alias"
    );
    assert_unchanged(&env, "SyncMaintenanceFee alias rejection");

    env.svm.expire_blockhash();
    let crank = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
        ],
        &[],
    );
    assert!(
        crank.is_err(),
        "PermissionlessCrank must reject market-as-portfolio alias"
    );
    assert_unchanged(&env, "PermissionlessCrank alias rejection");
}

// authorized operator could overwrite a funded user portfolio as a ledger and strand vault funds.
#[test]
fn v16_program_sync_ledgers_cannot_overwrite_portfolio_accounts() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.top_up_backing_bucket(1, 100, 10);
    env.top_up_insurance_domain_with_authority(&admin, 0, 100);

    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let backing_sync = env.send(
        ProgInstruction::SyncBackingDomainLedger { domain: 1 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        backing_sync.is_err(),
        "SyncBackingDomainLedger must reject a portfolio account as the ledger"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "backing-ledger sync must not rewrite the portfolio bytes or lamports"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "backing-ledger sync rejection must leave market state unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "backing-ledger sync rejection must not touch vault custody"
    );

    env.svm.expire_blockhash();
    let insurance_sync = env.send(
        ProgInstruction::SyncInsuranceLedger,
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        insurance_sync.is_err(),
        "SyncInsuranceLedger must reject a portfolio account as the ledger"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "insurance-ledger sync must not rewrite the portfolio bytes or lamports"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "insurance-ledger sync rejection must leave market state unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "insurance-ledger sync rejection must not touch vault custody"
    );
}

// must reject atomically, or a sync could rewrite the market slab as a ledger and brick/strand funds.
#[test]
fn v16_program_sync_ledgers_cannot_overwrite_market_account() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.top_up_backing_bucket(1, 100, 10);
    env.top_up_insurance_domain_with_authority(&admin, 0, 100);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let backing_sync = env.send(
        ProgInstruction::SyncBackingDomainLedger { domain: 1 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        backing_sync.is_err(),
        "SyncBackingDomainLedger must reject the market account as the ledger"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "backing-ledger market alias rejection must leave market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "backing-ledger market alias rejection must not rewrite portfolios"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "backing-ledger market alias rejection must not touch vault custody"
    );

    env.svm.expire_blockhash();
    let insurance_sync = env.send(
        ProgInstruction::SyncInsuranceLedger,
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        insurance_sync.is_err(),
        "SyncInsuranceLedger must reject the market account as the ledger"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "insurance-ledger market alias rejection must leave market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "insurance-ledger market alias rejection must not rewrite portfolios"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "insurance-ledger market alias rejection must not touch vault custody"
    );
}

// portfolio as the optional ledger must reject before any market, vault, source, or destination move.
#[test]
fn v16_program_value_paths_cannot_use_portfolio_as_optional_ledger() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.top_up_insurance_domain_with_authority(&admin, 0, 100);
    env.top_up_backing_bucket(1, 100, 10);
    env.enable_live_insurance_withdrawal();
    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].utilization_fee_earnings += 20;
        group.vault += 20;
    });
    let vault_with_earnings = env.token_amount(env.vault) + 20;
    env.set_token_account_amount(
        env.vault,
        env.mint,
        env.vault_authority,
        vault_with_earnings,
    );

    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let assert_core_unchanged = |env: &V16CuEnv| {
        assert_eq!(
            env.svm.get_account(&portfolio).unwrap(),
            portfolio_before,
            "wrong-kind ledger must not rewrite the funded portfolio"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "wrong-kind ledger rejection must leave market accounting unchanged"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "wrong-kind ledger rejection must leave vault custody unchanged"
        );
    };

    let top_up_insurance_source = env.token_account(admin.pubkey(), 25);
    env.svm.expire_blockhash();
    let top_up_insurance = env.send(
        ProgInstruction::TopUpInsurance {
            intent_id: 0,
            market_id: 0,
            amount: 25,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(top_up_insurance_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        top_up_insurance.is_err(),
        "TopUpInsurance must reject a portfolio account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.token_amount(top_up_insurance_source),
        25,
        "wrong-kind insurance ledger must reject before pulling source tokens"
    );

    let top_up_domain_source = env.token_account(admin.pubkey(), 20);
    env.svm.expire_blockhash();
    let top_up_domain = env.send(
        ProgInstruction::TopUpInsuranceDomain {
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 20,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(top_up_domain_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        top_up_domain.is_err(),
        "TopUpInsuranceDomain must reject a portfolio account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.token_amount(top_up_domain_source),
        20,
        "wrong-kind domain insurance ledger must reject before pulling source tokens"
    );

    let top_up_backing_source = env.token_account(admin.pubkey(), 30);
    env.svm.expire_blockhash();
    let top_up_backing = env.send(
        ProgInstruction::TopUpBackingBucket {
            intent_id: 0,
            market_id: 0,
            domain: 1,
            amount: 30,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(top_up_backing_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        top_up_backing.is_err(),
        "TopUpBackingBucket must reject a portfolio account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.token_amount(top_up_backing_source),
        30,
        "wrong-kind backing ledger must reject before pulling source tokens"
    );

    let insurance_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let withdraw_insurance = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 0,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(insurance_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_insurance.is_err(),
        "WithdrawInsuranceAsset must reject a portfolio account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.token_amount(insurance_dest),
        0,
        "wrong-kind insurance withdraw ledger must reject before paying destination"
    );

    let backing_dest = env.token_account(admin.pubkey(), 0);
    let market_id = env.asset_market_id(0);
    env.svm.expire_blockhash();
    let withdraw_backing = env.send(
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_backing.is_err(),
        "WithdrawBackingBucket must reject a portfolio account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.token_amount(backing_dest),
        0,
        "wrong-kind backing withdraw ledger must reject before paying destination"
    );

    let earnings_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let withdraw_earnings = env.send(
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(earnings_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_earnings.is_err(),
        "WithdrawBackingBucketEarnings must reject a portfolio account as the ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(
        env.token_amount(earnings_dest),
        0,
        "wrong-kind backing earnings ledger must reject before paying destination"
    );
}

// as a ledger or partially move SPL custody before failing. Every path must reject atomically.
#[test]
fn v16_program_value_paths_cannot_use_market_as_optional_ledger() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.top_up_insurance_domain_with_authority(&admin, 0, 100);
    env.top_up_backing_bucket(1, 100, 10);
    env.enable_live_insurance_withdrawal();
    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].utilization_fee_earnings += 20;
        group.vault += 20;
    });
    let vault_with_earnings = env.token_amount(env.vault) + 20;
    env.set_token_account_amount(
        env.vault,
        env.mint,
        env.vault_authority,
        vault_with_earnings,
    );

    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let assert_core_unchanged = |env: &V16CuEnv| {
        assert_eq!(
            env.svm.get_account(&portfolio).unwrap(),
            portfolio_before,
            "market-alias ledger rejection must not rewrite the funded portfolio"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "market-alias ledger rejection must leave market bytes unchanged"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "market-alias ledger rejection must leave vault custody unchanged"
        );
    };

    let top_up_insurance_source = env.token_account(admin.pubkey(), 25);
    env.svm.expire_blockhash();
    let top_up_insurance = env.send(
        ProgInstruction::TopUpInsurance {
            intent_id: 0,
            market_id: 0,
            amount: 25,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(top_up_insurance_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        top_up_insurance.is_err(),
        "TopUpInsurance must reject the market account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(env.token_amount(top_up_insurance_source), 25);

    let top_up_domain_source = env.token_account(admin.pubkey(), 20);
    env.svm.expire_blockhash();
    let top_up_domain = env.send(
        ProgInstruction::TopUpInsuranceDomain {
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 20,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(top_up_domain_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        top_up_domain.is_err(),
        "TopUpInsuranceDomain must reject the market account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(env.token_amount(top_up_domain_source), 20);

    let top_up_backing_source = env.token_account(admin.pubkey(), 30);
    env.svm.expire_blockhash();
    let top_up_backing = env.send(
        ProgInstruction::TopUpBackingBucket {
            intent_id: 0,
            market_id: 0,
            domain: 1,
            amount: 30,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(top_up_backing_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        top_up_backing.is_err(),
        "TopUpBackingBucket must reject the market account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(env.token_amount(top_up_backing_source), 30);

    let insurance_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let withdraw_insurance = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 0,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(insurance_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_insurance.is_err(),
        "WithdrawInsuranceAsset must reject the market account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(env.token_amount(insurance_dest), 0);

    let backing_dest = env.token_account(admin.pubkey(), 0);
    let market_id = env.asset_market_id(0);
    env.svm.expire_blockhash();
    let withdraw_backing = env.send(
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_backing.is_err(),
        "WithdrawBackingBucket must reject the market account as the optional ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(env.token_amount(backing_dest), 0);

    let earnings_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let withdraw_earnings = env.send(
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id,
            amount: 10,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(earnings_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        withdraw_earnings.is_err(),
        "WithdrawBackingBucketEarnings must reject the market account as the ledger"
    );
    assert_core_unchanged(&env);
    assert_eq!(env.token_amount(earnings_dest), 0);
}

// security.md sweep — withdraw/trade authorization (#6): only a portfolio's OWNER may withdraw from
// it or trade it. A non-owner signer must be rejected — no fund theft, no unauthorized position.
#[test]
fn v16_attack_non_owner_cannot_withdraw_or_trade() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();
    let pa_id = env.portfolio_id(pa);
    let pb_id = env.portfolio_id(pb);
    let mut legacy_pa = env.svm.get_account(&pa).unwrap();
    legacy_pa.data.truncate(PORTFOLIO_ENGINE_ACCOUNT_LEN);
    env.svm.set_account(pa, legacy_pa).unwrap();
    let pa_legacy_before = env.svm.get_account(&pa).unwrap();
    assert_eq!(
        pa_legacy_before.data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "test setup uses a legacy victim portfolio"
    );

    // Mallory tries to withdraw from pa (owned by la).
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, mallory.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let r_wd = env.send(
        ProgInstruction::Withdraw {
            portfolio_id: pa_id,
            expected_sequence: 0,
            amount: 500_000,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&mallory],
    );
    assert!(r_wd.is_err(), "non-owner withdraw must reject");
    assert_eq!(
        env.svm.get_account(&pa).unwrap(),
        pa_legacy_before,
        "non-owner withdraw rolls back the pre-owner-check legacy realloc"
    );
    assert_eq!(env.token_amount(dest), 0, "no funds stolen by non-owner");
    assert_eq!(
        env.portfolio_state(pa).capital.get(),
        1_000_000,
        "pa capital intact"
    );

    // Mallory tries to trade pa against pb (signing as the account_a owner).
    let pa_before_trade = env.svm.get_account(&pa).unwrap();
    let pb_before_trade = env.svm.get_account(&pb).unwrap();
    env.svm.expire_blockhash();
    let r_tr = env.send(
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: pa_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: pb_id,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(lb.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
            AccountMeta::new(pb, false),
        ],
        &[&mallory, &lb],
    );
    assert!(r_tr.is_err(), "non-owner trade of pa must reject");
    assert_eq!(
        env.svm.get_account(&pa).unwrap(),
        pa_before_trade,
        "non-owner trade rolls back the victim account realloc"
    );
    assert_eq!(
        env.svm.get_account(&pb).unwrap(),
        pb_before_trade,
        "non-owner trade leaves the honest counterparty untouched"
    );
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        0,
        "no unauthorized position opened on pa"
    );

    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
}

// security.md sweep — withdraw dest-owner binding (#44): withdraw must deliver only to a dest token
// account owned by the withdrawing portfolio's owner. A dest owned by a third party must reject
// (verify_withdrawable_token_accounts: dest.owner == expected_dest_owner).
#[test]
fn v16_attack_withdraw_to_third_party_dest_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    let other = Keypair::new();
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    // a dest token account owned by SOMEONE ELSE (correct mint).
    let other_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            other_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, other.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let r = env.send(
        env.withdraw_ix(p, 500_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(other_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw to a third-party-owned dest must reject"
    );
    assert_eq!(
        env.token_amount(other_dest),
        0,
        "no funds delivered to a third-party dest"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "capital not debited on rejected withdraw"
    );
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged");
    // own-dest withdraw works.
    env.svm.expire_blockhash();
    let (own, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(env.token_amount(own), 500_000, "withdraw to own dest works");
}

// security.md sweep - resolved payout account aliasing (#26/#44/#48): CloseResolved and the unsigned
// ClaimResolvedPayoutTopup are value-moving wind-down paths. Passing the market slab as the portfolio
// account must reject atomically and must not burn the real user's payout state.
#[test]
fn v16_attack_resolved_payout_paths_cannot_use_market_as_portfolio_alias() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);
    env.resolve();

    let close_dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let close_alias = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(close_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        close_alias.is_err(),
        "CloseResolved must reject market-as-portfolio alias"
    );
    assert_eq!(
        env.token_amount(close_dest),
        0,
        "no payout to alias close dest"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "alias CloseResolved must not rewrite the market slab"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "alias CloseResolved must not burn the real user's payout state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "alias CloseResolved must not move custody"
    );

    let good_close = env.close_resolved(&owner, portfolio);
    assert_eq!(
        env.token_amount(good_close),
        1_000_000,
        "real CloseResolved still pays after rejected alias attempt"
    );

    let mut topup_env = V16CuEnv::new();
    let topup_owner = Keypair::new();
    let topup_portfolio = topup_env.create_portfolio(&topup_owner);
    {
        let mut market_account = topup_env
            .svm
            .get_account(&topup_env.market)
            .expect("market account");
        let mut portfolio_account = topup_env
            .svm
            .get_account(&topup_portfolio)
            .expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        topup_env
            .svm
            .set_account(topup_env.market, market_account)
            .unwrap();
        topup_env
            .svm
            .set_account(topup_portfolio, portfolio_account)
            .unwrap();
    }
    topup_env.set_token_account_amount(
        topup_env.vault,
        topup_env.mint,
        topup_env.vault_authority,
        60,
    );

    let topup_dest = topup_env.token_account_for_mint(topup_env.mint, topup_owner.pubkey(), 0);
    let topup_market_before = topup_env.svm.get_account(&topup_env.market).unwrap();
    let topup_portfolio_before = topup_env.svm.get_account(&topup_portfolio).unwrap();
    let topup_vault_before = topup_env.svm.get_account(&topup_env.vault).unwrap();

    topup_env.svm.expire_blockhash();
    let topup_alias = topup_env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(topup_owner.pubkey(), false),
            AccountMeta::new(topup_env.market, false),
            AccountMeta::new(topup_env.market, false),
            AccountMeta::new(topup_dest, false),
            AccountMeta::new(topup_env.vault, false),
            AccountMeta::new_readonly(topup_env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        topup_alias.is_err(),
        "ClaimResolvedPayoutTopup must reject market-as-portfolio alias"
    );
    assert_eq!(
        topup_env.token_amount(topup_dest),
        0,
        "no payout to alias top-up dest"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_env.market).unwrap(),
        topup_market_before,
        "alias top-up must not rewrite the market slab"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_portfolio).unwrap(),
        topup_portfolio_before,
        "alias top-up must not burn the real pending receipt"
    );
    assert_eq!(
        topup_env.svm.get_account(&topup_env.vault).unwrap(),
        topup_vault_before,
        "alias top-up must not move custody"
    );

    let topup_cu = topup_env.claim_resolved_payout_topup_with_cu(
        topup_owner.pubkey(),
        topup_portfolio,
        topup_dest,
    );
    assert_cu_within(
        "ClaimResolvedPayoutTopup alias regression control",
        topup_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        topup_env.token_amount(topup_dest),
        60,
        "real top-up claim still pays after rejected alias attempt"
    );
}

// security.md sweep — RebalanceReduce owner gating (#6/#46): RebalanceReduce is OWNER-gated
// self-service risk reduction (with_one_portfolio_view enforces owner signs + matches the portfolio).
// A non-owner must NOT be able to force-reduce a victim's position (griefing); the owner may reduce
// their own. Verifies no permissionless force-close.
#[test]
fn v16_attack_rebalance_reduce_owner_gated() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q.get();
    assert!(basis0 != 0, "la opened a position");
    let (_, g0) = env.market_state();

    // ATTACK: a non-owner tries to force-reduce la's position -> reject (owner mismatch).
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.svm.expire_blockhash();
    let r_grief = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: env.portfolio_id(pa),
            position_epoch: env.portfolio_position_epoch(pa),
            asset_index: 0,
            reduce_q: POS_SCALE,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[&mallory],
    );
    assert!(
        r_grief.is_err(),
        "non-owner force-reduce of a victim's position must reject"
    );
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        basis0,
        "victim's position not reduced by attacker"
    );
    assert_eq!(
        env.market_state().1.vault,
        g0.vault,
        "vault unchanged by rejected griefing reduce"
    );

    // LEGITIMATE: the OWNER may reduce their own position (self-service risk reduction).
    env.svm.expire_blockhash();
    let r_owner = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: env.portfolio_id(pa),
            position_epoch: env.portfolio_position_epoch(pa),
            asset_index: 0,
            reduce_q: POS_SCALE,
        },
        vec![
            AccountMeta::new(la.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[&la],
    );
    assert!(
        r_owner.is_ok(),
        "owner self-reduce should succeed: {:?}",
        r_owner
    );
    assert!(
        env.portfolio_state(pa).legs[0]
            .basis_pos_q
            .get()
            .unsigned_abs()
            < basis0.unsigned_abs(),
        "owner reduced their own position"
    );
    let (_, g1) = env.market_state();
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
    assert_eq!(
        g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q,
        "OI still balanced"
    );
}

// security.md sweep — liquidation cranker reward bounded by the fee (#3): with a NONZERO liquidation
// fee configured, a cranker is paid cranker_share_bps of the fee. Attacker goal: self-liquidate (control
// both the liquidated account AND the cranker) to net-profit, i.e. cranker reward > fee paid. Protection:
// reward == cranker_share% of the fee (≤ fee), the fee is internal (vault unminted), and the remainder
// goes to insurance — so a self-liquidator nets ≤ 0 (here −fee + 50%·fee < 0). First BPF test to drive a
// INV-017 whole-route reward-tail matrix: a nonzero liquidation reward makes all four roles
// semantically distinct. Exhaust every pair and every required signer/writable bit from one publicly
// reached liquidatable state. Each malformed call must reject without changing the market, either
// trading portfolio, the reward portfolio, or SPL custody. A readonly cranker signer remains the
// positive control because the signer account itself is authenticated but never mutated.
#[test]
fn v16_program_liquidation_reward_tail_account_pairs_and_privileges_are_exhaustive() {
    const ROLE_NAMES: [&str; 4] = [
        "cranker",
        "market",
        "liquidated_portfolio",
        "reward_portfolio",
    ];

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let co = Keypair::new();
    let c = env.create_portfolio(&co);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000);
    env.deposit(&co, c, 1_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }

    assert!(
        health_cert(&env.portfolio_state(s)).certified_liq_deficit != 0,
        "short is liquidatable before probing the reward-tail account matrix"
    );

    let market = env.market;
    let canonical_accounts = || {
        vec![
            AccountMeta::new_readonly(co.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ]
    };
    let tracked_accounts = [market, l, s, c, env.vault];
    let instruction = ProgInstruction::PermissionlessCrank {
        now_slot: 30,
        observations: crank_observations(0),
    };

    let assert_rejects_atomically =
        |env: &mut V16CuEnv, accounts: Vec<AccountMeta>, label: &str| {
            let before = account_alias_snapshot(env, &tracked_accounts);
            let cranker_signer_required = accounts
                .iter()
                .any(|account| account.is_signer && account.pubkey == co.pubkey());
            let signers = cranker_signer_required
                .then_some(&co)
                .into_iter()
                .collect::<Vec<_>>();
            env.svm.expire_blockhash();
            let rejected = env.send(instruction.clone(), accounts, &signers);
            assert!(
                rejected.is_err(),
                "PermissionlessCrank reward tail {label} unexpectedly succeeded"
            );
            assert_eq!(
                account_alias_snapshot(env, &tracked_accounts),
                before,
                "PermissionlessCrank reward tail {label} must roll back all economic state"
            );
        };

    let mut pair_count = 0usize;
    for first in 0..ROLE_NAMES.len() {
        for second in (first + 1)..ROLE_NAMES.len() {
            pair_count += 1;
            let mut accounts = canonical_accounts();
            accounts[second].pubkey = accounts[first].pubkey;
            assert_rejects_atomically(
                &mut env,
                accounts,
                &format!("alias {} with {}", ROLE_NAMES[first], ROLE_NAMES[second]),
            );
        }
    }
    assert_eq!(pair_count, 6, "four reward-tail roles have six pairs");

    let mut accounts = canonical_accounts();
    accounts[0].is_signer = false;
    assert_rejects_atomically(&mut env, accounts, "cranker signer downgrade");

    for role in 1..ROLE_NAMES.len() {
        let mut accounts = canonical_accounts();
        accounts[role].is_writable = false;
        assert_rejects_atomically(
            &mut env,
            accounts,
            &format!("{} writable downgrade", ROLE_NAMES[role]),
        );
    }

    let cranker_cap_before = env.portfolio_state(c).capital.get();
    let before = account_alias_snapshot(&env, &tracked_accounts);
    env.svm.expire_blockhash();
    let accepted = env.send(instruction, canonical_accounts(), &[&co]);
    assert!(
        accepted.is_ok(),
        "distinct reward portfolio liquidation succeeds: {:?}",
        accepted
    );
    assert!(
        env.portfolio_state(c).capital.get() > cranker_cap_before,
        "positive control: distinct cranker received a real reward"
    );
    assert_ne!(
        account_alias_snapshot(&env, &tracked_accounts),
        before,
        "canonical reward-tail control must mutate economic state"
    );
    let (_, group) = env.market_state();
    assert_eq!(
        group.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        group.vault >= group.c_tot + group.insurance,
        "senior conservation"
    );
}

// security.md sweep - liquidation cranker reward owner binding (#6/#35/#44): the optional reward
// portfolio is validated after the crank path has already refreshed oracle/profile state. A same-market
// reward portfolio owned by a different user must reject transaction-atomically, or any signer could
// mutate another user's portfolio by sending them liquidation rewards without authorization.
#[test]
fn v16_attack_liquidation_cranker_reward_rejects_wrong_owner() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let co = Keypair::new();
    let c = env.create_portfolio(&co);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000);
    env.deposit(&co, c, 1_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    assert!(
        health_cert(&env.portfolio_state(s)).certified_liq_deficit != 0,
        "short is liquidatable before probing the reward-owner gate"
    );

    let wrong_owner = Keypair::new();
    env.ensure_signer_account(wrong_owner.pubkey());
    let market_before = env.svm.get_account(&env.market).unwrap();
    let short_before = env.svm.get_account(&s).unwrap();
    let cranker_before = env.svm.get_account(&c).unwrap();
    let cranker_cap_before = env.portfolio_state(c).capital.get();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(wrong_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&wrong_owner],
    );
    assert!(
        rejected.is_err(),
        "wrong signer must not direct a liquidation reward into another user's portfolio"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "wrong-owner reward rejection rolls back the pre-validation oracle/profile write"
    );
    assert_eq!(
        env.svm.get_account(&s).unwrap(),
        short_before,
        "wrong-owner reward rejection leaves the liquidated portfolio byte-identical"
    );
    assert_eq!(
        env.svm.get_account(&c).unwrap(),
        cranker_before,
        "wrong-owner reward rejection does not mutate the cranker portfolio"
    );
    assert_eq!(
        env.portfolio_state(c).capital.get(),
        cranker_cap_before,
        "no unauthorized reward is credited"
    );

    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(co.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&co],
    );
    assert!(
        accepted.is_ok(),
        "the actual reward portfolio owner can still claim the cranker reward: {accepted:?}"
    );
    assert!(
        env.portfolio_state(c).capital.get() > cranker_cap_before,
        "positive control: the authorized cranker receives a real reward"
    );
}

// full-interface sweep: a real market account can be a signing system-created keypair. If marketauth is
// rotated to that key, CloseSlab must still reject using the market slab itself as the lamport destination;
// otherwise the final reclaim can zero the data while leaving rent on a program-owned, closed slab.
#[test]
fn v16_attack_close_slab_rejects_market_as_lamport_destination() {
    let mut svm = LiteSVM::new();
    let program_id = percolator_prog::id();
    svm.add_program(
        program_id,
        &std::fs::read(program_path()).expect("read BPF"),
    );
    svm.add_program(
        spl_token::ID,
        &std::fs::read(spl_token_program_path()).expect("read token BPF"),
    );

    let payer = Keypair::new();
    let admin = Keypair::new();
    let market = Keypair::new();
    let mint = Pubkey::new_unique();
    let params = V16CuMarketParams::default();
    let vault_authority =
        Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
    let vault = canonical_vault_ata(vault_authority, mint);
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
    svm.set_account(
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
    svm.set_account(
        market.pubkey(),
        Account {
            lamports: 1_000_000_000,
            data: vec![0u8; state::market_account_len_for_capacity(1).unwrap()],
            owner: program_id,
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

    send_tx(
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
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new_readonly(mint, false),
        ],
        &[&admin],
    )
    .expect("init market");

    svm.expire_blockhash();
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::UpdateAuthority {
            new_pubkey: market.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
        ],
        &[&admin, &market],
    )
    .expect("rotate marketauth to signing market key");

    svm.expire_blockhash();
    send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::ResolveMarket {
            asset_generation_frontier: 0,
        },
        vec![
            AccountMeta::new(market.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
        ],
        &[&market],
    )
    .expect("market key can resolve after handoff");

    let dest = Pubkey::new_unique();
    svm.set_account(
        dest,
        Account {
            lamports: 1_000_000_000,
            data: make_token_data(mint, market.pubkey(), 0),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    let market_before = svm.get_account(&market.pubkey()).unwrap();
    let vault_before = svm.get_account(&vault).unwrap();
    let dest_before = svm.get_account(&dest).unwrap();

    svm.expire_blockhash();
    let rejected = send_tx(
        &mut svm,
        program_id,
        &payer,
        ProgInstruction::CloseSlab,
        vec![
            AccountMeta::new(market.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new(dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&market],
    );
    assert!(
        rejected.is_err(),
        "CloseSlab must reject market-as-destination alias"
    );
    assert_eq!(
        svm.get_account(&market.pubkey()).unwrap(),
        market_before,
        "market-as-destination rejection leaves the slab initialized"
    );
    assert_eq!(
        svm.get_account(&vault).unwrap(),
        vault_before,
        "market-as-destination rejection leaves the vault open"
    );
    assert_eq!(
        svm.get_account(&dest).unwrap(),
        dest_before,
        "market-as-destination rejection pays no dust"
    );
}

// security.md sweep — SwapSecondaryForPrimary authority + balance bounds (#6/#33/#44): the 1:1 par
// collateral swap is base_unit_authority-gated and bounded by the secondary vault's balance. Attacker
// goals: (a) a non-authority drains the secondary reserve, (b) the authority over-swaps beyond the
// reserve to print/underflow. Must reject both; a valid swap conserves value exactly 1:1.
#[test]
fn v16_attack_swap_secondary_unauthorized_and_bounded() {
    let mut env = V16CuEnv::new();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 50),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let swap = |env: &mut V16CuEnv,
                signer: &Keypair,
                primary_source: Pubkey,
                secondary_dest: Pubkey,
                amount: u128|
     -> Result<u64, String> {
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::SwapSecondaryForPrimary { amount },
            vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new(primary_source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new(secondary_dest, false),
                AccountMeta::new(secondary_vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[signer],
        )
    };

    // (a) ATTACK: a non-base_unit_authority signer (mallory) tries to swap and drain the secondary reserve.
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    let m_primary = env.token_account_for_mint(env.mint, mallory.pubkey(), 50);
    let m_secondary = env.token_account_for_mint(secondary_mint, mallory.pubkey(), 0);
    assert!(
        swap(&mut env, &mallory, m_primary, m_secondary, 50).is_err(),
        "non-authority swap must reject"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        50,
        "secondary reserve untouched by unauthorized swap"
    );
    assert_eq!(
        env.token_amount(m_secondary),
        0,
        "no secondary drained to attacker"
    );
    assert_eq!(
        env.token_amount(m_primary),
        50,
        "attacker's primary not pulled"
    );

    // (b) ATTACK: the legit authority over-swaps beyond the secondary reserve (51 > 50) -> reject.
    let admin = env.admin.insecure_clone();
    let a_primary = env.token_account_for_mint(env.mint, admin.pubkey(), 100);
    let a_secondary = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    assert!(
        swap(&mut env, &admin, a_primary, a_secondary, 51).is_err(),
        "over-swap beyond secondary reserve must reject"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        50,
        "reserve untouched by rejected over-swap"
    );
    assert_eq!(
        env.token_amount(a_primary),
        100,
        "no primary pulled on rejected over-swap"
    );

    // (c) zero amount rejects.
    assert!(
        swap(&mut env, &admin, a_primary, a_secondary, 0).is_err(),
        "zero-amount swap rejects"
    );

    // (c2) ATTACK: even the legit authority cannot route the secondary payout to a third party.
    // The rejected swap must not pull primary first or mutate either vault.
    let foreign_secondary = env.token_account_for_mint(secondary_mint, mallory.pubkey(), 0);
    assert!(
        swap(&mut env, &admin, a_primary, foreign_secondary, 10).is_err(),
        "swap to a third-party secondary destination must reject",
    );
    assert_eq!(
        env.token_amount(a_primary),
        100,
        "primary not pulled on bad-dest swap"
    );
    assert_eq!(
        env.token_amount(foreign_secondary),
        0,
        "no secondary paid to third party"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        50,
        "secondary reserve untouched by bad-dest swap"
    );

    // (d) VALID: authority swaps exactly the reserve (50) -> 1:1, value-conserving.
    let vault_primary_before = env.token_amount(env.vault);
    assert!(
        swap(&mut env, &admin, a_primary, a_secondary, 50).is_ok(),
        "authorized in-bounds swap ok"
    );
    assert_eq!(env.token_amount(a_primary), 50, "exactly 50 primary pulled");
    assert_eq!(
        env.token_amount(env.vault),
        vault_primary_before + 50,
        "primary vault gained exactly 50"
    );
    assert_eq!(
        env.token_amount(a_secondary),
        50,
        "exactly 50 secondary delivered 1:1"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        0,
        "secondary reserve fully drained, not more"
    );
}

// security.md sweep - SwapSecondaryForPrimary account aliasing (#26/#35/#44): the primary source must
// be an authority-owned token account and the secondary destination must be authority-owned. Otherwise
// the authority could pass the primary vault as both source and destination for a no-op primary transfer
// that drains secondary, or burn primary into the vault while receiving no secondary.
#[test]
fn v16_attack_swap_secondary_rejects_vault_source_or_dest_aliases() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);

    let depositor = Keypair::new();
    let portfolio = env.create_portfolio(&depositor);
    env.deposit(&depositor, portfolio, 1_000);

    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 50),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let admin_secondary = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let primary_vault_before = env.svm.get_account(&env.vault).unwrap();
    let secondary_vault_before = env.svm.get_account(&secondary_vault).unwrap();
    let admin_secondary_before = env.svm.get_account(&admin_secondary).unwrap();
    env.svm.expire_blockhash();
    let vault_source = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SwapSecondaryForPrimary { amount: 10 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(admin_secondary, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        vault_source.is_err(),
        "SwapSecondaryForPrimary must reject the primary vault as the source"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        primary_vault_before,
        "primary vault not used as a no-op source"
    );
    assert_eq!(
        env.svm.get_account(&secondary_vault).unwrap(),
        secondary_vault_before,
        "secondary reserve not drained by vault-source alias"
    );
    assert_eq!(
        env.svm.get_account(&admin_secondary).unwrap(),
        admin_secondary_before,
        "attacker receives no secondary on rejected vault-source alias"
    );

    let admin_primary = env.token_account_for_mint(env.mint, admin.pubkey(), 10);
    let admin_primary_before = env.svm.get_account(&admin_primary).unwrap();
    let primary_vault_before = env.svm.get_account(&env.vault).unwrap();
    let secondary_vault_before = env.svm.get_account(&secondary_vault).unwrap();
    env.svm.expire_blockhash();
    let vault_dest = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SwapSecondaryForPrimary { amount: 10 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(admin_primary, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        vault_dest.is_err(),
        "SwapSecondaryForPrimary must reject the secondary vault as the user destination"
    );
    assert_eq!(
        env.svm.get_account(&admin_primary).unwrap(),
        admin_primary_before
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        primary_vault_before
    );
    assert_eq!(
        env.svm.get_account(&secondary_vault).unwrap(),
        secondary_vault_before,
        "secondary vault not self-paid by bad-destination alias"
    );

    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SwapSecondaryForPrimary { amount: 10 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(admin_primary, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(admin_secondary, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        ok.is_ok(),
        "same accounts without aliasing swap cleanly: {ok:?}"
    );
    assert_eq!(env.token_amount(admin_primary), 0);
    assert_eq!(env.token_amount(admin_secondary), 10);
    assert_eq!(env.token_amount(secondary_vault), 40);
}

// security.md sweep — ConvertReleasedPnl is owner-gated (#6/#33): the convert moves a portfolio's backed
// junior pnl into senior capital; with_one_portfolio_view(...,true,...) requires the OWNER to sign and
// match the portfolio. Attacker goal: force a VICTIM's conversion (premature junior→senior move, changing
// their haircut exposure) without their consent. Protection: a non-owner signer, or the owner as a
// non-signer, both reject; only the genuine owner-signed call converts.
#[test]
fn v16_attack_convert_released_pnl_owner_gated() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 40, 10_000);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    env.add_source_positive_pnl(p, 1, 40);
    env.crank(
        p,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let cap0 = env.portfolio_state(p).capital.get();
    let pnl0 = env.portfolio_state(p).pnl.get();
    assert!(pnl0 > 0, "victim has backed junior pnl");
    let portfolio_id = env.portfolio_id(p);
    let position_epoch = env.portfolio_position_epoch(p);
    let portfolio_before = env.svm.get_account(&p).unwrap();

    // ATTACK 1: a NON-OWNER (mallory) signs a convert on the victim's portfolio -> reject (owner mismatch).
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.svm.expire_blockhash();
    let r1 = env.send(
        ProgInstruction::ConvertReleasedPnl {
            portfolio_id,
            position_epoch,
            amount: 1_000_000_000,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&mallory],
    );
    assert!(r1.is_err(), "non-owner convert must reject");
    assert_eq!(
        env.svm.get_account(&p).unwrap(),
        portfolio_before,
        "non-owner convert leaves the victim portfolio byte-identical"
    );

    // ATTACK 2: the owner's pubkey is passed but NOT as a signer -> reject (expect_signer).
    env.svm.expire_blockhash();
    let r2 = env.send(
        ProgInstruction::ConvertReleasedPnl {
            portfolio_id,
            position_epoch,
            amount: 1_000_000_000,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[],
    );
    assert!(
        r2.is_err(),
        "convert with the owner as a non-signer must reject"
    );
    assert_eq!(
        env.svm.get_account(&p).unwrap(),
        portfolio_before,
        "non-signer convert leaves the portfolio byte-identical"
    );

    // neither attempt converted anything.
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        cap0,
        "capital unchanged by rejected converts"
    );
    assert_eq!(
        env.portfolio_state(p).pnl.get(),
        pnl0,
        "junior pnl not converted by an unauthorized caller"
    );

    // CONTROL: the genuine OWNER-signed convert works.
    env.svm.expire_blockhash();
    let ok = env.send(
        ProgInstruction::ConvertReleasedPnl {
            portfolio_id,
            position_epoch,
            amount: 1_000_000_000,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&owner],
    );
    assert!(ok.is_ok(), "owner-signed convert works: {:?}", ok);
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        cap0 + 40,
        "owner converts the backed 40 to capital"
    );
}
