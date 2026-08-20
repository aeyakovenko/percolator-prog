//! INV-056 - hints are discovery only; favorable actions fully refresh.
//!
//! Normative obligation: a user-favorable route must not trust a caller-provided
//! subset of work or omit an active stale liability. Before authorizing a
//! favorable new position, it must fully discover the bounded active portfolio
//! state or use a proven-equivalent exact certificate.
//!
//! Evidence in this file (I/C plus invariant-specific route assertions): a source-complete caller
//! input roster guard proves only PermissionlessCrank exposes discovery hints. Matched forward and
//! reverse two-asset Pyth hint/account-tail orders normalize identically; mismatched tails reject
//! with exact rollback and a live canonical retry. BatchTradeNoCpi and BatchTradeCpi open a new
//! asset-0 leg for an account that already has a stale active asset-1 leg, and must discover and
//! refresh that stale leg before admitting the new favorable leg. INV-053 owns the single-leg
//! TradeNoCpi/TradeCpi variants and every single-omitted max-shape refresh case.

use super::*;

#[test]
fn v16_program_discovery_hint_surface_is_permissionless_crank_only() {
    const CALLER_INPUT_ROSTER: &str = include_str!("../inv_023_caller_input_roster.tsv");
    let mut hint_fields = std::collections::BTreeSet::new();

    for line in CALLER_INPUT_ROSTER.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with("type\t") {
            continue;
        }
        let columns: Vec<_> = line.split('\t').collect();
        assert_eq!(
            columns.len(),
            4,
            "malformed caller-input roster row: {line}"
        );
        if columns[2] == "DISCOVERY_HINT" {
            for field in columns[1].split(',') {
                assert!(
                    hint_fields.insert((columns[0].to_owned(), field.to_owned())),
                    "duplicate discovery-hint field {}.{field}",
                    columns[0]
                );
            }
        }
    }

    let expected = [
        ("CrankObservationHint".to_owned(), "asset_index".to_owned()),
        (
            "CrankObservationHint".to_owned(),
            "oracle_accounts".to_owned(),
        ),
        ("PermissionlessCrank".to_owned(), "observations".to_owned()),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        hint_fields, expected,
        "a new caller-controlled discovery hint requires an INV-056 public omission/order matrix"
    );
}

const INV056_EXTERNAL_SLOT: u64 = 2;
const INV056_EXTERNAL_TIME: i64 = 101;
const INV056_EXTERNAL_FEEDS: [[u8; 32]; 2] = [[0x56; 32], [0x57; 32]];
const INV056_EXTERNAL_PRICES: [u64; 2] = [1_100_000, 900_000];

#[derive(Debug, PartialEq, Eq)]
struct Inv056ExternalOrderSnapshot {
    current_slot: u64,
    oracle_epoch: u64,
    funding_epoch: u64,
    effective_prices: [u64; 2],
    raw_targets: [u64; 2],
    asset_slots: [u64; 2],
    profile_prices: [u64; 2],
    profile_publish_times: [i64; 2],
    vault: u128,
    c_tot: u128,
    insurance: u128,
}

fn inv056_external_oracle_world() -> (V16CuEnv, Pubkey, [Pubkey; 2]) {
    const INITIAL_SLOT: u64 = 1;
    const INITIAL_TIME: i64 = 100;
    const INITIAL_PRICE: i64 = 1_000_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    set_test_clock(&mut env, INITIAL_SLOT, INITIAL_TIME);
    for (asset_index, feed) in INV056_EXTERNAL_FEEDS.iter().enumerate() {
        let initial = env.set_pyth_price_with_conf(feed, INITIAL_PRICE, -6, 0, INITIAL_TIME);
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            asset_index as u16,
            1,
            0,
            [*feed, [0; 32], [0; 32]],
            &[initial],
            INITIAL_SLOT,
            INITIAL_TIME,
            0,
            0,
            10,
            0,
        )
        .unwrap_or_else(|error| panic!("configure external asset {asset_index}: {error}"));
    }

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    set_test_clock(&mut env, INV056_EXTERNAL_SLOT, INV056_EXTERNAL_TIME);
    let fresh = std::array::from_fn(|asset_index| {
        env.set_pyth_price_with_conf(
            &INV056_EXTERNAL_FEEDS[asset_index],
            INV056_EXTERNAL_PRICES[asset_index] as i64,
            -6,
            0,
            INV056_EXTERNAL_TIME,
        )
    });
    (env, portfolio, fresh)
}

fn inv056_external_order_snapshot(env: &V16CuEnv) -> Inv056ExternalOrderSnapshot {
    let market = env.svm.get_account(&env.market).unwrap();
    let (_, group) = env.market_state();
    let profiles = [
        state::read_asset_oracle_profile(&market.data, 0).unwrap(),
        state::read_asset_oracle_profile(&market.data, 1).unwrap(),
    ];
    Inv056ExternalOrderSnapshot {
        current_slot: group.current_slot,
        oracle_epoch: group.oracle_epoch,
        funding_epoch: group.funding_epoch,
        effective_prices: [
            group.assets[0].effective_price,
            group.assets[1].effective_price,
        ],
        raw_targets: [
            group.assets[0].raw_oracle_target_price,
            group.assets[1].raw_oracle_target_price,
        ],
        asset_slots: [group.assets[0].slot_last, group.assets[1].slot_last],
        profile_prices: [
            profiles[0].oracle_leg_prices_e6[0],
            profiles[1].oracle_leg_prices_e6[0],
        ],
        profile_publish_times: [
            profiles[0].oracle_leg_publish_times[0],
            profiles[1].oracle_leg_publish_times[0],
        ],
        vault: group.vault,
        c_tot: group.c_tot,
        insurance: group.insurance,
    }
}

fn inv056_run_external_hint_order(order: [usize; 2]) -> Inv056ExternalOrderSnapshot {
    let (mut env, portfolio, fresh) = inv056_external_oracle_world();
    let observations = order
        .iter()
        .map(|asset_index| CrankObservationHint {
            asset_index: *asset_index as u16,
            oracle_accounts: 1,
        })
        .collect();
    let accounts = vec![
        AccountMeta::new(env.payer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
        AccountMeta::new_readonly(fresh[order[0]], false),
        AccountMeta::new_readonly(fresh[order[1]], false),
    ];
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations,
            },
            accounts,
            &[],
        )
        .expect("matched external-oracle hint order must progress");
    assert_cu_within("INV-056 external-oracle hint order", cu, CRANK_CU_LIMIT);

    let snapshot = inv056_external_order_snapshot(&env);
    assert_eq!(snapshot.effective_prices, INV056_EXTERNAL_PRICES);
    assert_eq!(snapshot.raw_targets, INV056_EXTERNAL_PRICES);
    assert_eq!(snapshot.profile_prices, INV056_EXTERNAL_PRICES);
    assert_eq!(snapshot.profile_publish_times, [INV056_EXTERNAL_TIME; 2]);
    snapshot
}

#[test]
fn v16_program_external_oracle_hint_and_account_order_is_normalized_or_atomic() {
    let forward = inv056_run_external_hint_order([0, 1]);
    let reverse = inv056_run_external_hint_order([1, 0]);
    assert_eq!(
        forward, reverse,
        "matching hint/account permutations must produce one normalized market result"
    );

    let (mut env, portfolio, fresh) = inv056_external_oracle_world();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let mismatched = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![
                CrankObservationHint {
                    asset_index: 1,
                    oracle_accounts: 1,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 1,
                },
            ],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new_readonly(fresh[0], false),
            AccountMeta::new_readonly(fresh[1], false),
        ],
        &[],
    );
    assert!(
        mismatched.is_err(),
        "a feed tail that does not match hint order must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    env.svm.expire_blockhash();
    let retry = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 1,
                },
                CrankObservationHint {
                    asset_index: 1,
                    oracle_accounts: 1,
                },
            ],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new_readonly(fresh[0], false),
            AccountMeta::new_readonly(fresh[1], false),
        ],
        &[],
    );
    assert!(
        retry.is_ok(),
        "a canonical retry must remain live after mismatched-tail rollback: {retry:?}"
    );
    assert_eq!(
        inv056_external_order_snapshot(&env),
        forward,
        "retry after hostile ordering must reach the canonical normalized state"
    );
}

#[derive(Clone, Copy, Debug)]
enum Inv056BatchRoute {
    NoCpi,
    Cpi,
}

fn run_batch_route_with_stale_related_leg(route: Inv056BatchRoute) {
    const PRICE: u64 = 100;
    const MOVED_PRICE: u64 = 105;
    const STALE_SIZE_Q: i128 = (10 * POS_SCALE) as i128;
    const NEW_SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(1, 0, PRICE);

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000_000);
    env.deposit(&lp_owner, lp, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        STALE_SIZE_Q,
        PRICE,
        0,
    );

    let crank_long_owner = Keypair::new();
    let crank_short_owner = Keypair::new();
    let crank_long = env.create_portfolio(&crank_long_owner);
    let crank_short = env.create_portfolio(&crank_short_owner);
    env.deposit(&crank_long_owner, crank_long, 1_000_000_000);
    env.deposit(&crank_short_owner, crank_short, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &crank_long_owner,
        crank_long,
        &crank_short_owner,
        crank_short,
        POS_SCALE as i128,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(1, 1, MOVED_PRICE);
    env.crank(
        crank_long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(1),
        },
    );
    let (_, stale_group) = env.market_state();
    let taker_stale = env.portfolio_state(taker);
    let lp_stale = env.portfolio_state(lp);
    assert_eq!(stale_group.assets[1].effective_price, MOVED_PRICE);
    assert!(
        health_cert(&taker_stale).cert_oracle_epoch < stale_group.oracle_epoch,
        "{route:?}: taker certificate must be stale before the batch route"
    );
    assert!(
        health_cert(&lp_stale).cert_oracle_epoch < stale_group.oracle_epoch,
        "{route:?}: LP certificate must be stale before the batch route"
    );
    assert_ne!(
        active_leg_for_asset(&taker_stale, 1).k_snap,
        stale_group.assets[1].k_long,
        "{route:?}: taker stale leg snapshot must differ from current market K"
    );
    assert_ne!(
        active_leg_for_asset(&lp_stale, 1).k_snap,
        stale_group.assets[1].k_short,
        "{route:?}: LP stale leg snapshot must differ from current market K"
    );

    let cu = match route {
        Inv056BatchRoute::NoCpi => env
            .send(
                env.batch_trade_no_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: NEW_SIZE_Q,
                        exec_price: PRICE,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(lp_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(lp, false),
                ],
                &[&taker_owner, &lp_owner],
            )
            .expect("BatchTradeNoCpi must refresh stale related legs before admitting asset-0"),
        Inv056BatchRoute::Cpi => {
            let matcher_program = Pubkey::new_unique();
            let matcher_bytes =
                std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
            env.svm.add_program(matcher_program, &matcher_bytes);
            let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
            env.send(
                env.batch_trade_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: NEW_SIZE_Q,
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
                    AccountMeta::new(ctx, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[&taker_owner],
            )
            .expect("BatchTradeCpi must refresh stale related legs before admitting asset-0")
        }
    };
    assert_cu_within(
        &format!("INV-056 {route:?} stale related-leg batch refresh"),
        cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let (_, group_after) = env.market_state();
    let taker_after = env.portfolio_state(taker);
    let lp_after = env.portfolio_state(lp);
    assert_eq!(
        health_cert(&taker_after).cert_oracle_epoch,
        group_after.oracle_epoch,
        "{route:?}: taker is recertified against the full market epoch"
    );
    assert_eq!(
        health_cert(&lp_after).cert_oracle_epoch,
        group_after.oracle_epoch,
        "{route:?}: LP is recertified against the full market epoch"
    );
    assert_eq!(
        active_leg_for_asset(&taker_after, 1).k_snap,
        group_after.assets[1].k_long,
        "{route:?}: taker stale asset-1 leg was refreshed in-place"
    );
    assert_eq!(
        active_leg_for_asset(&lp_after, 1).k_snap,
        group_after.assets[1].k_short,
        "{route:?}: LP stale asset-1 leg was refreshed in-place"
    );
    assert!(has_active_leg_for_asset(&taker_after, 0));
    assert!(has_active_leg_for_asset(&lp_after, 0));
    assert!(has_active_leg_for_asset(&taker_after, 1));
    assert!(has_active_leg_for_asset(&lp_after, 1));
}

#[test]
fn v16_program_batch_routes_refresh_stale_related_legs_before_favorable_trade() {
    run_batch_route_with_stale_related_leg(Inv056BatchRoute::NoCpi);
    run_batch_route_with_stale_related_leg(Inv056BatchRoute::Cpi);
}
