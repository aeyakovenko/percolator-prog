//! INV-056 - hints are discovery only; favorable actions fully refresh.
//!
//! Normative obligation: a user-favorable route must not trust a caller-provided
//! subset of work or omit an active stale liability. Before authorizing a
//! favorable new position, it must fully discover the bounded active portfolio
//! state or use a proven-equivalent exact certificate.
//!
//! Evidence in this file (I/C plus invariant-specific route assertions): a source-complete caller
//! input roster guard proves only PermissionlessCrank exposes discovery hints. A second gate
//! classifies all 49 canonical public instructions and requires executable public witnesses for
//! every current-certificate/full-refresh route, flat-only value exit, immutable terminal payout,
//! refreshing cure, and stale-safe risk reduction; a new instruction cannot silently inherit a
//! favorable-action exemption. Matched forward and
//! reverse two-asset Pyth hint/account-tail orders normalize identically; mismatched tails reject
//! with exact rollback and a live canonical retry. BatchTradeNoCpi and BatchTradeCpi open a new
//! asset-0 leg for an account that already has a stale active asset-1 leg, and must discover and
//! refresh that stale leg before admitting the new favorable leg. Pending-close and Recovery
//! selector traces prove hostile hints roll back before an honest retry progresses; once Resolved,
//! duplicate hints are economically inert and produce the same payout as an empty-hint close.
//! A public two-atom bankruptcy/forfeit trace proves B settlement budgets are collateral atoms,
//! not B-index ticks: one owner step exposes the SettleB selector, duplicate external hints roll
//! back, and one authenticated-tail crank consumes the remaining atom at bounded CU. INV-077
//! composes a three-feed tail with selected max-shape liquidation after exact duplicate/order
//! rollback.
//! INV-053 owns the single-leg TradeNoCpi/TradeCpi variants and every single-omitted max-shape
//! refresh case.

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Inv056PublicRouteDisposition {
    DiscoveryHint,
    CurrentCertificateOrFullRefresh,
    FlatOnlyValueExit,
    ImmutableTerminalPayout,
    RefreshingCure,
    StaleSafeRiskReduction,
    InboundValueOnly,
    ScopedNonPortfolioValue,
    ControlOrBookkeeping,
}

#[derive(Clone, Copy)]
struct Inv056RouteEvidence {
    disposition: Inv056PublicRouteDisposition,
    witness_path: Option<&'static str>,
    witness_test: Option<&'static str>,
}

fn inv056_evidence(
    disposition: Inv056PublicRouteDisposition,
    witness: Option<(&'static str, &'static str)>,
) -> Inv056RouteEvidence {
    Inv056RouteEvidence {
        disposition,
        witness_path: witness.map(|(path, _)| path),
        witness_test: witness.map(|(_, test)| test),
    }
}

fn inv056_public_route_evidence(variant: &str) -> Option<Inv056RouteEvidence> {
    use Inv056PublicRouteDisposition::*;

    let evidence = match variant {
        "PermissionlessCrank" => inv056_evidence(
            DiscoveryHint,
            Some((
                "tests/invariants/cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs",
                "v16_program_external_oracle_hint_and_account_order_is_normalized_or_atomic",
            )),
        ),
        "TradeNoCpi" => inv056_evidence(
            CurrentCertificateOrFullRefresh,
            Some((
                "tests/invariants/cu/inv_053_full_health_recertification_equivalence.rs",
                "v16_bpf_trade_refreshes_stale_related_portfolio_leg_on_demand",
            )),
        ),
        "TradeCpi" => inv056_evidence(
            CurrentCertificateOrFullRefresh,
            Some((
                "tests/invariants/cu/inv_053_full_health_recertification_equivalence.rs",
                "v16_bpf_tradecpi_refreshes_stale_traded_portfolio_leg_on_demand",
            )),
        ),
        "BatchTradeNoCpi" | "BatchTradeCpi" => inv056_evidence(
            CurrentCertificateOrFullRefresh,
            Some((
                "tests/invariants/cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs",
                "v16_program_batch_routes_refresh_stale_related_legs_before_favorable_trade",
            )),
        ),
        "ConvertReleasedPnl" => inv056_evidence(
            CurrentCertificateOrFullRefresh,
            Some((
                "tests/invariants/cu/inv_054_certificate_epoch_completeness.rs",
                "v16_attack_convert_released_pnl_requires_current_cert_and_public_refresh",
            )),
        ),
        "Withdraw" => inv056_evidence(
            FlatOnlyValueExit,
            Some((
                "tests/invariants/cu/inv_055_state_indexed_admission.rs",
                "v16_attack_withdraw_requires_flat_regardless_of_size",
            )),
        ),
        "CloseResolved" | "ClaimResolvedPayoutTopup" => inv056_evidence(
            ImmutableTerminalPayout,
            Some((
                "tests/invariants/cu/inv_068_receipt_uniqueness_and_monotonic_topups.rs",
                "v16_program_resolved_receipt_replays_extract_no_value_on_any_public_rail",
            )),
        ),
        "CureAndCancelClose" => inv056_evidence(
            RefreshingCure,
            Some((
                "tests/invariants/stateful/inv_037_exact_residual_partition.rs",
                "inv037_public_cure_preserves_exact_partition_across_routes_and_sides",
            )),
        ),
        "RebalanceReduce" => inv056_evidence(
            StaleSafeRiskReduction,
            Some((
                "tests/invariants/cu/inv_057_risk_reduction_availability.rs",
                "v16_attack_non_base_local_stale_owner_reduce_remains_live",
            )),
        ),
        "ForfeitRecoveryLeg" => inv056_evidence(
            StaleSafeRiskReduction,
            Some((
                "tests/invariants/stateful/inv_081_success_state_validity_over_complete_public_routes.rs",
                "v16_program_owner_recovery_forfeit_strictly_reduces_each_position_episode",
            )),
        ),
        "ForceCloseAbandonedAsset" => inv056_evidence(
            StaleSafeRiskReduction,
            Some((
                "tests/invariants/cu/inv_078_permissionless_recovery_coverage.rs",
                "v16_attack_locally_stale_permissionless_asset_can_shutdown_and_force_close",
            )),
        ),
        "Deposit" | "TopUpInsurance" | "TopUpBackingBucket" | "TopUpInsuranceDomain" => {
            inv056_evidence(InboundValueOnly, None)
        }
        "WithdrawBackingBucket"
        | "WithdrawBackingBucketEarnings"
        | "WithdrawInsuranceAsset"
        | "SwapSecondaryForPrimary" => inv056_evidence(ScopedNonPortfolioValue, None),
        "InitMarket"
        | "InitPortfolio"
        | "ClosePortfolio"
        | "CloseSlab"
        | "ResolveMarket"
        | "UpdateAuthority"
        | "ConfigureHybridOracle"
        | "ConfigureEwmaMark"
        | "PushEwmaMark"
        | "UpdateLiquidationFeePolicy"
        | "ConfigurePermissionlessResolve"
        | "ResolveStalePermissionless"
        | "UpdateAssetLifecycle"
        | "FinalizeResetSide"
        | "SyncMaintenanceFee"
        | "UpdateMaintenanceFeePolicy"
        | "UpdateBackingFeePolicy"
        | "SyncBackingDomainLedger"
        | "SyncInsuranceLedger"
        | "UpdateTradeFeePolicy"
        | "UpdateFeeRedirectPolicy"
        | "UpdateMarketInitFeePolicy"
        | "UpdateBaseUnitMints"
        | "ConfigureAuthMark"
        | "PushAuthMark"
        | "UpdateAssetAuthority"
        | "SetMatcherConfig"
        | "RestartAssetOracle" => inv056_evidence(ControlOrBookkeeping, None),
        _ => return None,
    };
    Some(evidence)
}

fn inv056_source_defines_test(source: &str, function: &str) -> bool {
    let expected = format!("fn {function}");
    let mut test_attribute = false;
    for line in source.lines() {
        let line = line.trim();
        if line == "#[test]" {
            test_attribute = true;
        } else if line.starts_with("fn ") {
            if test_attribute
                && line
                    .strip_prefix(&expected)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            test_attribute = false;
        } else if test_attribute && !line.is_empty() && !line.starts_with('#') {
            test_attribute = false;
        }
    }
    false
}

#[test]
fn v16_program_no_hint_favorable_route_roster_is_source_complete() {
    const REGISTRY: &str = include_str!("../public_instruction_coverage.tsv");
    use Inv056PublicRouteDisposition::*;

    let mut variants = std::collections::BTreeSet::new();
    let mut dispositions = std::collections::BTreeMap::<_, usize>::new();
    let mut witnessed = 0usize;

    for line in REGISTRY.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with("tag\t") {
            continue;
        }
        let columns = line.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 5, "malformed public-route row: {line}");
        let variant = columns[1];
        assert!(
            variants.insert(variant),
            "duplicate public variant {variant}"
        );
        let evidence = inv056_public_route_evidence(variant).unwrap_or_else(|| {
            panic!(
                "public instruction {variant} lacks an INV-056 stale-state/favorable-route disposition"
            )
        });
        *dispositions.entry(evidence.disposition).or_default() += 1;

        match (evidence.witness_path, evidence.witness_test) {
            (Some(path), Some(test)) => {
                let source = std::fs::read_to_string(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path),
                )
                .unwrap_or_else(|error| panic!("read INV-056 witness {path}: {error}"));
                assert!(
                    inv056_source_defines_test(&source, test),
                    "{variant} points to missing executable witness {path}#{test}"
                );
                witnessed += 1;
            }
            (None, None) => assert!(
                matches!(
                    evidence.disposition,
                    InboundValueOnly | ScopedNonPortfolioValue | ControlOrBookkeeping
                ),
                "{variant} has a security-relevant disposition without executable evidence"
            ),
            _ => panic!("{variant} has an incomplete INV-056 witness"),
        }
    }

    assert_eq!(
        variants.len(),
        49,
        "the canonical public route count changed"
    );
    assert_eq!(dispositions.get(&DiscoveryHint), Some(&1));
    assert_eq!(dispositions.get(&CurrentCertificateOrFullRefresh), Some(&5));
    assert_eq!(dispositions.get(&FlatOnlyValueExit), Some(&1));
    assert_eq!(dispositions.get(&ImmutableTerminalPayout), Some(&2));
    assert_eq!(dispositions.get(&RefreshingCure), Some(&1));
    assert_eq!(dispositions.get(&StaleSafeRiskReduction), Some(&3));
    assert_eq!(witnessed, 13);
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

#[test]
fn v16_program_pending_close_bad_hints_roll_back_then_canonical_crank_progresses() {
    let PublicActiveCloseFixture {
        mut env,
        loss,
        live_counterparty,
        live_peer,
        ..
    } = public_asset1_bankrupt_close_fixture();
    let close_before = close_progress(&env.portfolio_state(loss));
    assert!(close_before.active && close_before.residual_remaining > 0);
    assert!(
        env.svm.get_sysvar::<Clock>().slot <= close_before.max_close_slot,
        "fixture must exercise live close advancement rather than expiry recovery"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let loss_before = env.svm.get_account(&loss).unwrap();
    let counterparty_before = env.svm.get_account(&live_counterparty).unwrap();
    let peer_before = env.svm.get_account(&live_peer).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let hostile = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
            ],
        },
        vec![
            AccountMeta::new_readonly(env.payer.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
        ],
        &[],
    );
    assert!(
        hostile.is_err(),
        "a duplicate late hint must reject before close advancement commits"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&loss).unwrap(), loss_before);
    assert_eq!(
        env.svm.get_account(&live_counterparty).unwrap(),
        counterparty_before
    );
    assert_eq!(env.svm.get_account(&live_peer).unwrap(), peer_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    env.svm.expire_blockhash();
    let progress_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("canonical empty-hint crank must advance the pending close");
    assert_cu_within(
        "INV-056 pending-close canonical retry",
        progress_cu,
        CRANK_CU_LIMIT,
    );
    let mode_after = env.market_state().1.mode;
    let close_after = close_progress(&env.portfolio_state(loss));
    assert!(
        mode_after != MarketModeV16::Live
            || close_after.residual_remaining < close_before.residual_remaining
            || close_after.finalized,
        "canonical retry must lower close rank or enter a terminal mode"
    );
    assert_eq!(
        env.svm.get_account(&live_counterparty).unwrap(),
        counterparty_before,
        "pending-close progress must not rewrite an unrelated portfolio"
    );
    assert_eq!(
        env.svm.get_account(&live_peer).unwrap(),
        peer_before,
        "pending-close progress must not rewrite an unrelated peer"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "pending-close bookkeeping must not move SPL custody"
    );
}

#[test]
fn v16_program_public_b_stale_atom_budget_is_hint_independent_and_bounded() {
    let (fixture, settle_b_oracle) =
        public_asset1_bankrupt_close_fixture_with_asset0_external_oracle();
    let PublicActiveCloseFixture {
        mut env,
        loss,
        asset1_counterparty_owner,
        asset1_counterparty,
        live_peer,
        ..
    } = fixture;

    // The fixture's owner forfeit books the first loss atom. A permissionless
    // close continuation books the second, entirely through public routes.
    let close_before = close_progress(&env.portfolio_state(loss));
    assert!(close_before.active && close_before.residual_remaining > 0);
    env.svm.expire_blockhash();
    let booking_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("public close continuation must book the second B loss atom");
    assert_cu_within("INV-056 public B booking", booking_cu, CRANK_CU_LIMIT);
    let target_b = env.market_state().1.assets[1].b_long_num;
    let before_discovery = active_leg_for_asset(&env.portfolio_state(asset1_counterparty), 1);
    assert!(target_b > before_discovery.b_snap);
    assert!(
        target_b > 2,
        "the B-index representation must make an index-tick budget observably wrong"
    );
    assert!(!before_discovery.b_stale);

    // The owner route discovers the committed gap and may consume only one
    // loss atom. Exactly one atom therefore remains for permissionless SettleB.
    let discovery_cu =
        env.forfeit_recovery_leg_with_cu(&asset1_counterparty_owner, asset1_counterparty, 1, 1);
    assert_cu_within("INV-056 public B discovery", discovery_cu, CUSTODY_CU_LIMIT);
    let discovered = active_leg_for_asset(&env.portfolio_state(asset1_counterparty), 1);
    assert!(
        discovered.b_snap > before_discovery.b_snap.saturating_add(1),
        "one collateral-atom budget must not be interpreted as one B-index tick"
    );
    assert!(discovered.b_snap < target_b && discovered.b_stale);

    // Compose the selected SettleB step with a real external-oracle tail for
    // the unrelated live asset. The observation may update authenticated
    // market state, but it must not suppress or replace higher-priority B
    // progress on the target account.
    for _ in 0..3 {
        let catchup_cu = env.crank_with_oracle_tail(
            live_peer,
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: crank_observations(0),
            },
            &[settle_b_oracle],
        );
        assert_cu_within(
            "INV-056 external-tail bounded market catch-up",
            catchup_cu,
            CRANK_CU_LIMIT,
        );
    }
    assert_eq!(
        env.market_state().1.assets[0].slot_last,
        4,
        "the external asset must be current before measuring composed account progress"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&asset1_counterparty).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let hostile = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 1,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 1,
                },
            ],
        },
        vec![
            AccountMeta::new_readonly(env.payer.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(asset1_counterparty, false),
            AccountMeta::new_readonly(settle_b_oracle, false),
            AccountMeta::new_readonly(settle_b_oracle, false),
        ],
        &[],
    );
    assert!(hostile.is_err(), "duplicate hints must reject atomically");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&asset1_counterparty).unwrap(),
        portfolio_before
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    env.svm.expire_blockhash();
    let settle_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 1,
                }],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(asset1_counterparty, false),
                AccountMeta::new_readonly(settle_b_oracle, false),
            ],
            &[],
        )
        .expect("SettleB must compose with an authenticated unrelated oracle tail");
    assert_cu_within(
        "INV-056 external-tail plus public SettleB",
        settle_cu,
        CRANK_CU_LIMIT,
    );
    let settled_account = env.portfolio_state(asset1_counterparty);
    let settled = active_leg_for_asset(&settled_account, 1);
    assert_eq!(
        settled.b_snap, target_b,
        "the second atom must settle in one call"
    );
    assert!(!settled.b_stale);
    assert_eq!(settled_account.b_stale_state, 0);
}

#[test]
fn v16_program_recovery_and_resolved_dispatch_treat_hints_as_discovery_only() {
    let PublicActiveCloseFixture {
        mut env,
        loss,
        live_counterparty_owner,
        live_counterparty,
        live_peer_owner,
        live_peer,
        ..
    } = public_asset1_bankrupt_close_fixture();
    let close = close_progress(&env.portfolio_state(loss));
    env.svm.warp_to_slot(close.max_close_slot + 1);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![],
        },
        vec![
            AccountMeta::new_readonly(env.payer.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
        ],
        &[],
    )
    .expect("expired close must enter permissionless Recovery");
    assert_eq!(
        env.market_state().1.mode,
        MarketModeV16::Recovery,
        "fixture must expose the distinct FinalizeRecovery selector class"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let loss_before = env.svm.get_account(&loss).unwrap();
    let counterparty_before = env.svm.get_account(&live_counterparty).unwrap();
    let peer_before = env.svm.get_account(&live_peer).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let hostile = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
            ],
        },
        vec![
            AccountMeta::new_readonly(env.payer.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
        ],
        &[],
    );
    assert!(
        hostile.is_err(),
        "duplicate Recovery hints must reject atomically"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&loss).unwrap(), loss_before);
    assert_eq!(
        env.svm.get_account(&live_counterparty).unwrap(),
        counterparty_before
    );
    assert_eq!(env.svm.get_account(&live_peer).unwrap(), peer_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    env.svm.expire_blockhash();
    let finalize_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("empty-hint Recovery crank must finalize to Resolved");
    assert_cu_within(
        "INV-056 Recovery finalization retry",
        finalize_cu,
        CRANK_CU_LIMIT,
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
    assert_eq!(
        env.svm.get_account(&live_counterparty).unwrap(),
        counterparty_before,
        "Recovery finalization must not rewrite a claimant portfolio"
    );
    assert_eq!(
        env.svm.get_account(&live_peer).unwrap(),
        peer_before,
        "Recovery finalization must not rewrite a claimant peer"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "Recovery finalization snapshots accounting without moving SPL custody"
    );

    let duplicate_hint_dest = env.token_account(live_counterparty_owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let duplicate_hint_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![
                    CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    },
                    CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    },
                ],
            },
            vec![
                AccountMeta::new(live_counterparty_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(live_counterparty, false),
                AccountMeta::new(duplicate_hint_dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&live_counterparty_owner],
        )
        .expect("Resolved dispatch must ignore economically irrelevant duplicate hints");
    assert_cu_within(
        "INV-056 Resolved duplicate-hint close",
        duplicate_hint_cu,
        CRANK_CU_LIMIT,
    );

    let empty_hint_dest = env.token_account(live_peer_owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let empty_hint_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new(live_peer_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(live_peer, false),
                AccountMeta::new(empty_hint_dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&live_peer_owner],
        )
        .expect("Resolved empty-hint close must remain live");
    assert_cu_within(
        "INV-056 Resolved empty-hint close",
        empty_hint_cu,
        CRANK_CU_LIMIT,
    );
    let duplicate_hint_payout = env.token_amount(duplicate_hint_dest);
    let empty_hint_payout = env.token_amount(empty_hint_dest);
    assert!(duplicate_hint_payout > 0);
    assert_eq!(
        duplicate_hint_payout, empty_hint_payout,
        "Resolved hints must not alter symmetric claimant economics"
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault),
        "Resolved hint-insensitive closes preserve engine/SPL vault parity"
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
