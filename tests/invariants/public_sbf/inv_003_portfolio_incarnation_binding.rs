//! INV-003 - Portfolio incarnation binding.
//!
//! Normative obligation: every retained portfolio-scoped request binds the current
//! program-assigned `portfolio_id`, not only the portfolio pubkey. Closing and
//! recreating the same pubkey must make old consent unusable before any economic
//! or lamport mutation.
//!
//! Evidence in this file (I): one public SBF/LiteSVM matrix builds retained
//! requests for every portfolio-scoped public route, cycles the same portfolio
//! pubkey through owners A -> B -> A, then replays A's original transaction. The assertion is
//! route-local: the stale request must reject, exact tracked state must roll
//! back, SPL supply must stay fixed, and the replacement account's new
//! `portfolio_id` must be larger than both prior incarnations. Each operation is
//! then rebuilt against the current incarnation and must land with an observable
//! economic delta, preventing an always-rejecting implementation from satisfying
//! the matrix. The trace schema additionally proves every lifecycle edge is a
//! real public transaction.
//! A separate retained-deposit control keeps its original signature across another
//! portfolio's A -> B -> A cycle, proving unrelated incarnation allocation does
//! not invalidate consent for an unchanged portfolio.

use crate::support::invariant_discovery::{
    discover_portfolio_incarnation_replays, PortfolioIntentKind,
};
use crate::support::v16_svm::{MarketConfig, V16Svm};

#[test]
fn v16_program_retained_deposit_survives_unrelated_portfolio_recreation() {
    const RETAINED_OWNER: usize = 0;
    const RECREATED_OWNER: usize = 1;
    const INTERMEDIATE_OWNER: usize = 2;
    const AMOUNT: u64 = 1_000;

    let mut env = V16Svm::new([0x3d; 32], MarketConfig::default());
    let retained_id = env.primary_portfolio_id(RETAINED_OWNER);
    let retained_portfolio = env.primary_portfolio_data(RETAINED_OWNER);
    let old_id = env.primary_portfolio_id(RECREATED_OWNER);
    let retained = env.build_retained_deposit(RETAINED_OWNER, u128::from(AMOUNT));

    let old_capital = env.primary_portfolio(RECREATED_OWNER).capital.get();
    env.withdraw_primary(RECREATED_OWNER, old_capital)
        .expect("empty the neighboring portfolio");
    env.close_primary_portfolio(RECREATED_OWNER)
        .expect("close the neighboring portfolio");
    let (intermediate_id, replacement_id) = env
        .cycle_closed_primary_portfolio_through_owner(RECREATED_OWNER, INTERMEDIATE_OWNER)
        .expect("recreate the neighboring portfolio through owners A-B-A");
    assert!(intermediate_id > old_id);
    assert!(replacement_id > intermediate_id);
    assert_eq!(env.primary_portfolio_id(RETAINED_OWNER), retained_id);
    assert_eq!(
        env.primary_portfolio_data(RETAINED_OWNER),
        retained_portfolio,
        "the signed portfolio's identity and replay sequence remain unchanged"
    );

    let source = env.actors[RETAINED_OWNER].source_token;
    let source_before = env.token_amount(source);
    let vault_before = env.token_amount(env.vault);
    let capital_before = env.primary_portfolio(RETAINED_OWNER).capital.get();
    let total_before = env.primary_market_state().1.c_tot;
    let portfolios_before = env.all_primary_portfolio_data();
    let matchers_before = env.all_matcher_context_data();
    let lamports_before = env.all_economic_account_lamports();
    let supply_before = env.token_supply_observed();

    // Submit the original signed request, not a control rebuilt after allocation.
    env.land_retained(retained)
        .expect("unrelated portfolio recreation must preserve retained deposit consent");
    assert_eq!(env.primary_portfolio_id(RETAINED_OWNER), retained_id);
    assert_eq!(env.token_amount(source), source_before - AMOUNT);
    assert_eq!(env.token_amount(env.vault), vault_before + AMOUNT);
    assert_eq!(
        env.primary_portfolio(RETAINED_OWNER).capital.get(),
        capital_before + u128::from(AMOUNT)
    );
    assert_eq!(
        env.primary_market_state().1.c_tot,
        total_before + u128::from(AMOUNT)
    );
    for (actor, before) in portfolios_before.iter().enumerate() {
        if actor != RETAINED_OWNER {
            assert_eq!(&env.primary_portfolio_data(actor), before);
        }
    }
    assert_eq!(env.all_matcher_context_data(), matchers_before);
    assert_eq!(env.all_economic_account_lamports(), lamports_before);
    assert_eq!(env.token_supply_observed(), supply_before);
}

#[test]
fn v16_program_all_retained_portfolio_intents_reject_after_same_pubkey_recreate() {
    let discoveries = discover_portfolio_incarnation_replays([0x03; 32])
        .unwrap_or_else(|error| panic!("INV-003 matrix failed: {error}"));
    assert_eq!(discoveries.len(), PortfolioIntentKind::ALL.len());

    for (expected, discovery) in PortfolioIntentKind::ALL.into_iter().zip(&discoveries) {
        assert_eq!(discovery.kind, expected);
        assert!(
            discovery.intermediate_portfolio_id > discovery.old_portfolio_id
                && discovery.new_portfolio_id > discovery.intermediate_portfolio_id,
            "{expected:?}: portfolio id did not advance across A-B-A recreation: {} -> {} -> {}",
            discovery.old_portfolio_id,
            discovery.intermediate_portfolio_id,
            discovery.new_portfolio_id,
        );
        assert!(
            !discovery.accepted_stale_intent,
            "{expected:?}: stale retained portfolio intent landed on a replacement account"
        );
        assert!(
            !discovery.mutated_economic_state,
            "{expected:?}: rejected stale intent failed exact rollback"
        );
        assert_eq!(
            discovery.compute_units, None,
            "{expected:?}: stale replay should have no successful CU result"
        );
        assert_eq!(
            discovery.public_trace.out_of_band_economic_mutations, 0,
            "{expected:?}: replay evidence must use public transitions only",
        );
        let replay = discovery
            .public_trace
            .steps
            .last()
            .expect("A-B-A trace includes the stale replay");
        assert!(!replay.succeeded, "{expected:?}: stale replay trace");
        assert_eq!(
            replay.rejected_exact_writable_rollback,
            Some(true),
            "{expected:?}: stale replay must roll back every writable account",
        );
        assert!(
            replay.token_deltas.iter().all(|(_, delta)| *delta == 0),
            "{expected:?}: stale replay must move no SPL value",
        );
        assert!(
            discovery.fresh_intent_landed,
            "{expected:?}: current-incarnation control must remain executable: {:?}",
            discovery.fresh_error,
        );
        assert!(
            discovery.fresh_mutated_economic_state,
            "{expected:?}: current-incarnation control must produce a nonvacuous economic delta",
        );
        assert!(
            discovery.fresh_compute_units.is_some(),
            "{expected:?}: current-incarnation control needs a successful CU result",
        );
        let fresh_trace = discovery
            .fresh_public_trace
            .as_ref()
            .unwrap_or_else(|| panic!("{expected:?}: missing fresh trace"));
        assert_eq!(
            fresh_trace.out_of_band_economic_mutations, 0,
            "{expected:?}: current control must use public transitions only",
        );
        let fresh = fresh_trace
            .steps
            .last()
            .unwrap_or_else(|| panic!("{expected:?}: empty fresh trace"));
        assert!(fresh.succeeded, "{expected:?}: current control trace");
    }

    // This fixed-pin mapping is direct certification evidence, not independent discovery.
    // Each finding remains backed by the operation-generic matrix above; trade findings require
    // every route family in both account roles rather than one representative transaction.
    let certifications: &[(u16, &[PortfolioIntentKind])] = &[
        (274, &[PortfolioIntentKind::MatcherEnable]),
        (
            276,
            &[
                PortfolioIntentKind::TradeNoCpiAccountA,
                PortfolioIntentKind::TradeNoCpiAccountB,
                PortfolioIntentKind::TradeCpiAccountA,
                PortfolioIntentKind::TradeCpiAccountB,
                PortfolioIntentKind::BatchTradeNoCpiAccountA,
                PortfolioIntentKind::BatchTradeNoCpiAccountB,
                PortfolioIntentKind::BatchTradeCpiAccountA,
                PortfolioIntentKind::BatchTradeCpiAccountB,
            ],
        ),
        (278, &[PortfolioIntentKind::ForfeitRecoveryLeg]),
        (285, &PortfolioIntentKind::ALL),
        (299, &[PortfolioIntentKind::Withdraw]),
        (301, &[PortfolioIntentKind::ConvertReleasedPnl]),
        (
            303,
            &[
                PortfolioIntentKind::TradeNoCpiAccountA,
                PortfolioIntentKind::TradeNoCpiAccountB,
                PortfolioIntentKind::TradeCpiAccountA,
                PortfolioIntentKind::TradeCpiAccountB,
                PortfolioIntentKind::BatchTradeNoCpiAccountA,
                PortfolioIntentKind::BatchTradeNoCpiAccountB,
                PortfolioIntentKind::BatchTradeCpiAccountA,
                PortfolioIntentKind::BatchTradeCpiAccountB,
            ],
        ),
        (304, &[PortfolioIntentKind::MatcherEnable]),
        (305, &[PortfolioIntentKind::Deposit]),
        (309, &[PortfolioIntentKind::Close]),
    ];
    for (pr, kinds) in certifications {
        for kind in *kinds {
            let evidence = discoveries
                .iter()
                .find(|discovery| discovery.kind == *kind)
                .unwrap_or_else(|| panic!("PR {pr}: missing {kind:?} certification evidence"));
            assert!(
                !evidence.accepted_stale_intent && !evidence.mutated_economic_state,
                "PR {pr}: {kind:?} stale request must reject atomically",
            );
            assert!(
                evidence.fresh_intent_landed && evidence.fresh_mutated_economic_state,
                "PR {pr}: {kind:?} current request must remain live",
            );
        }
    }
}
