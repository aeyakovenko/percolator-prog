//! INV-015 - Account ownership, layout, discriminator, and length validity.
//!
//! Public wrappers must reject malformed program-owned accounts before any zero-copy view or
//! mutation can commit. This module retains an otherwise-valid public transaction before supplying
//! malformed market/portfolio account state at execution time. Nested fields are exercised through
//! a route that consumes their exact scope; unrelated asset slots are not globally rescanned by
//! account-local operations. Every case must return an instruction error and leave the complete
//! persistent state unchanged.
//!
//! Guarantee boundary: these are malformed-input validation checks, not accepted LoF/DoS
//! counterexamples. The test intentionally creates malformed account fixtures to prove the wrapper
//! fails closed and relies on SVM rollback only after a real transaction error is returned.

use super::support::v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE};
use percolator::POS_SCALE;
use percolator_prog::{constants, state};
use solana_sdk::{account::Account, pubkey::Pubkey, system_program};

#[derive(Clone, Debug, PartialEq, Eq)]
struct AccountSnapshot {
    lamports: u64,
    data: Vec<u8>,
    owner: Pubkey,
    executable: bool,
    rent_epoch: u64,
}

impl From<Account> for AccountSnapshot {
    fn from(account: Account) -> Self {
        Self {
            lamports: account.lamports,
            data: account.data,
            owner: account.owner,
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PersistentSnapshot {
    market: AccountSnapshot,
    portfolio: AccountSnapshot,
    source_token: AccountSnapshot,
    vault_token: AccountSnapshot,
    all_token_accounts: Vec<(Pubkey, Vec<u8>)>,
}

fn account_snapshot(env: &V16Svm, key: Pubkey) -> AccountSnapshot {
    env.svm
        .get_account(&key)
        .unwrap_or_else(|| panic!("missing account {key}"))
        .into()
}

fn snapshot(env: &V16Svm) -> PersistentSnapshot {
    PersistentSnapshot {
        market: account_snapshot(env, env.market),
        portfolio: account_snapshot(env, env.actors[0].portfolio),
        source_token: account_snapshot(env, env.actors[0].source_token),
        vault_token: account_snapshot(env, env.vault),
        all_token_accounts: env.all_token_account_data(),
    }
}

fn mutation_snapshot(env: &V16Svm) -> Vec<(Pubkey, Option<AccountSnapshot>)> {
    let mut keys = vec![
        env.market,
        env.vault,
        env.provider_source_token,
        env.backing_domain_ledger,
    ];
    for actor in &env.actors {
        keys.extend([actor.portfolio, actor.source_token, actor.destination_token]);
    }
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .map(|key| {
            let account = env.svm.get_account(&key).map(AccountSnapshot::from);
            (key, account)
        })
        .collect()
}

fn replace_account(env: &mut V16Svm, key: Pubkey, mutate: impl FnOnce(&mut Account)) {
    let mut account = env
        .svm
        .get_account(&key)
        .unwrap_or_else(|| panic!("missing account {key}"));
    mutate(&mut account);
    env.svm
        .set_account(key, account)
        .expect("install malformed account fixture");
}

#[derive(Clone, Copy, Debug)]
enum MalformedCase {
    MarketWrongOwner,
    MarketTooShort,
    MarketBadMagic,
    MarketBadVersion,
    MarketBadKind,
    MarketNonzeroWrapperPadding,
    MarketTrailingByte,
    PortfolioWrongOwner,
    PortfolioTooShort,
    PortfolioBadMagic,
    PortfolioBadVersion,
    PortfolioBadKind,
    PortfolioTrailingByte,
}

fn market_header_offset(field_offset: usize) -> usize {
    constants::MARKET_GROUP_OFF + field_offset
}

fn asset_zero_engine_offset() -> usize {
    constants::MARKET_GROUP_OFF
        + percolator::MarketGroupV16HeaderAccount::dynamic_asset_slot_offset::<
            state::AssetOracleStorageV16,
        >(0)
        .expect("asset-zero slot offset")
        + core::mem::offset_of!(percolator::Market<state::AssetOracleStorageV16>, engine)
}

fn portfolio_engine_offset(field_offset: usize) -> usize {
    constants::HEADER_LEN + field_offset
}

fn apply_malformed_case(env: &mut V16Svm, case: MalformedCase) {
    match case {
        MalformedCase::MarketWrongOwner => {
            replace_account(env, env.market, |account| {
                account.owner = system_program::ID
            });
        }
        MalformedCase::MarketTooShort => {
            replace_account(env, env.market, |account| account.data.truncate(7));
        }
        MalformedCase::MarketBadMagic => {
            replace_account(env, env.market, |account| account.data[0] ^= 0x80);
        }
        MalformedCase::MarketBadVersion => {
            replace_account(env, env.market, |account| account.data[8] ^= 0x80);
        }
        MalformedCase::MarketBadKind => {
            replace_account(env, env.market, |account| account.data[10] ^= 0x7f);
        }
        MalformedCase::MarketNonzeroWrapperPadding => {
            let padding_offset =
                constants::HEADER_LEN + core::mem::offset_of!(state::WrapperConfigV16, _padding0);
            replace_account(env, env.market, |account| {
                account.data[padding_offset] = 1;
            });
        }
        MalformedCase::MarketTrailingByte => {
            replace_account(env, env.market, |account| account.data.push(0));
        }
        MalformedCase::PortfolioWrongOwner => {
            replace_account(env, env.actors[0].portfolio, |account| {
                account.owner = system_program::ID
            });
        }
        MalformedCase::PortfolioTooShort => {
            replace_account(env, env.actors[0].portfolio, |account| {
                account.data.truncate(7)
            });
        }
        MalformedCase::PortfolioBadMagic => {
            replace_account(env, env.actors[0].portfolio, |account| {
                account.data[0] ^= 0x80
            });
        }
        MalformedCase::PortfolioBadVersion => {
            replace_account(env, env.actors[0].portfolio, |account| {
                account.data[8] ^= 0x80
            });
        }
        MalformedCase::PortfolioBadKind => {
            replace_account(env, env.actors[0].portfolio, |account| {
                account.data[10] ^= 0x7f
            });
        }
        MalformedCase::PortfolioTrailingByte => {
            replace_account(env, env.actors[0].portfolio, |account| account.data.push(0));
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum NestedAccount {
    Market,
    Portfolio,
}

#[derive(Clone, Copy, Debug)]
enum ConsumingRoute {
    Deposit,
    Trade,
    CertifiedTrade,
    Shutdown,
    BackingTopUpLong,
    BackingTopUpShort,
    ClosePortfolio,
}

#[derive(Clone, Copy, Debug)]
struct NestedByteCase {
    name: &'static str,
    account: NestedAccount,
    offset: usize,
    route: ConsumingRoute,
}

fn nested_byte_cases() -> Vec<NestedByteCase> {
    macro_rules! market_config {
        ($field:ident) => {
            NestedByteCase {
                name: concat!("config.", stringify!($field)),
                account: NestedAccount::Market,
                offset: market_header_offset(
                    core::mem::offset_of!(percolator::MarketGroupV16HeaderAccount, config)
                        + core::mem::offset_of!(percolator::V16ConfigAccount, $field),
                ),
                route: ConsumingRoute::Deposit,
            }
        };
    }
    macro_rules! market_field {
        ($field:ident) => {
            NestedByteCase {
                name: concat!("market.", stringify!($field)),
                account: NestedAccount::Market,
                offset: market_header_offset(core::mem::offset_of!(
                    percolator::MarketGroupV16HeaderAccount,
                    $field
                )),
                route: ConsumingRoute::Deposit,
            }
        };
    }
    macro_rules! asset_field {
        ($field:ident) => {
            NestedByteCase {
                name: concat!("asset.", stringify!($field)),
                account: NestedAccount::Market,
                offset: asset_zero_engine_offset()
                    + core::mem::offset_of!(percolator::EngineAssetSlotV16Account, asset)
                    + core::mem::offset_of!(percolator::AssetStateV16Account, $field),
                route: ConsumingRoute::Shutdown,
            }
        };
    }
    macro_rules! portfolio_field {
        ($field:ident, $route:expr) => {
            NestedByteCase {
                name: concat!("portfolio.", stringify!($field)),
                account: NestedAccount::Portfolio,
                offset: portfolio_engine_offset(core::mem::offset_of!(
                    percolator::PortfolioAccountV16Account,
                    $field
                )),
                route: $route,
            }
        };
    }
    macro_rules! leg_field {
        ($field:ident) => {
            NestedByteCase {
                name: concat!("leg.", stringify!($field)),
                account: NestedAccount::Portfolio,
                offset: portfolio_engine_offset(
                    core::mem::offset_of!(percolator::PortfolioAccountV16Account, legs)
                        + core::mem::offset_of!(percolator::PortfolioLegV16Account, $field),
                ),
                route: ConsumingRoute::Deposit,
            }
        };
    }
    macro_rules! close_field {
        ($field:ident) => {
            NestedByteCase {
                name: concat!("close.", stringify!($field)),
                account: NestedAccount::Portfolio,
                offset: portfolio_engine_offset(
                    core::mem::offset_of!(percolator::PortfolioAccountV16Account, close_progress)
                        + core::mem::offset_of!(percolator::CloseProgressLedgerV16Account, $field),
                ),
                route: ConsumingRoute::Deposit,
            }
        };
    }
    macro_rules! receipt_field {
        ($field:ident) => {
            NestedByteCase {
                name: concat!("receipt.", stringify!($field)),
                account: NestedAccount::Portfolio,
                offset: portfolio_engine_offset(
                    core::mem::offset_of!(
                        percolator::PortfolioAccountV16Account,
                        resolved_payout_receipt
                    ) + core::mem::offset_of!(percolator::ResolvedPayoutReceiptV16Account, $field),
                ),
                route: ConsumingRoute::Deposit,
            }
        };
    }

    let cases = vec![
        market_config!(backing_freshness_buckets),
        market_config!(margin_mode_realizable_full_shared_cross_margin),
        market_config!(source_credit_lien_required),
        market_config!(insurance_credit_reservation_required),
        market_config!(permissionless_recovery_enabled),
        market_config!(recovery_fallback_price_enabled),
        market_config!(recovery_fallback_envelope_enabled),
        market_config!(credit_lien_revalidation_required),
        market_config!(stale_certificate_penalty_enabled),
        market_config!(full_refresh_required_for_favorable_actions),
        market_config!(public_liveness_profile_crank_forward),
        NestedByteCase {
            name: "market.recovery_reason.present",
            account: NestedAccount::Market,
            offset: market_header_offset(
                core::mem::offset_of!(percolator::MarketGroupV16HeaderAccount, recovery_reason)
                    + core::mem::offset_of!(percolator::V16OptionalRecoveryReasonAccount, present),
            ),
            route: ConsumingRoute::Deposit,
        },
        NestedByteCase {
            name: "market.recovery_reason.value",
            account: NestedAccount::Market,
            offset: market_header_offset(
                core::mem::offset_of!(percolator::MarketGroupV16HeaderAccount, recovery_reason)
                    + core::mem::offset_of!(percolator::V16OptionalRecoveryReasonAccount, value),
            ),
            route: ConsumingRoute::Deposit,
        },
        market_field!(bankruptcy_hlock_active),
        market_field!(threshold_stress_active),
        market_field!(loss_stale_active),
        market_field!(mode),
        market_field!(payout_snapshot_captured),
        NestedByteCase {
            name: "payout_ledger.payout_halted",
            account: NestedAccount::Market,
            offset: market_header_offset(
                core::mem::offset_of!(
                    percolator::MarketGroupV16HeaderAccount,
                    resolved_payout_ledger
                ) + core::mem::offset_of!(
                    percolator::ResolvedPayoutLedgerV16Account,
                    payout_halted
                ),
            ),
            route: ConsumingRoute::Deposit,
        },
        NestedByteCase {
            name: "payout_ledger.finalized",
            account: NestedAccount::Market,
            offset: market_header_offset(
                core::mem::offset_of!(
                    percolator::MarketGroupV16HeaderAccount,
                    resolved_payout_ledger
                ) + core::mem::offset_of!(percolator::ResolvedPayoutLedgerV16Account, finalized),
            ),
            route: ConsumingRoute::Deposit,
        },
        asset_field!(lifecycle),
        asset_field!(mode_long),
        asset_field!(mode_short),
        NestedByteCase {
            name: "asset.backing_long.status",
            account: NestedAccount::Market,
            offset: asset_zero_engine_offset()
                + core::mem::offset_of!(percolator::EngineAssetSlotV16Account, backing_long)
                + core::mem::offset_of!(percolator::BackingBucketV16Account, status),
            route: ConsumingRoute::BackingTopUpLong,
        },
        NestedByteCase {
            name: "asset.backing_short.status",
            account: NestedAccount::Market,
            offset: asset_zero_engine_offset()
                + core::mem::offset_of!(percolator::EngineAssetSlotV16Account, backing_short)
                + core::mem::offset_of!(percolator::BackingBucketV16Account, status),
            route: ConsumingRoute::BackingTopUpShort,
        },
        leg_field!(active),
        leg_field!(side),
        leg_field!(b_stale),
        leg_field!(stale),
        NestedByteCase {
            name: "portfolio.health_cert.valid",
            account: NestedAccount::Portfolio,
            offset: portfolio_engine_offset(
                core::mem::offset_of!(percolator::PortfolioAccountV16Account, health_cert)
                    + core::mem::offset_of!(percolator::HealthCertV16Account, valid),
            ),
            route: ConsumingRoute::CertifiedTrade,
        },
        portfolio_field!(stale_state, ConsumingRoute::Trade),
        portfolio_field!(b_stale_state, ConsumingRoute::Trade),
        portfolio_field!(rebalance_lock, ConsumingRoute::ClosePortfolio),
        portfolio_field!(liquidation_lock, ConsumingRoute::ClosePortfolio),
        close_field!(active),
        close_field!(finalized),
        close_field!(canceled),
        close_field!(domain_side),
        receipt_field!(present),
        receipt_field!(finalized),
    ];
    assert_eq!(cases.len(), 40, "review every persisted engine byte domain");
    cases
}

fn nested_case_environment(seed: [u8; 32], route: ConsumingRoute) -> V16Svm {
    let mut config = MarketConfig::default();
    if matches!(route, ConsumingRoute::ClosePortfolio) {
        config.actor_deposits[0] = 0;
    }
    let mut env = V16Svm::new(seed, config);
    if matches!(route, ConsumingRoute::Shutdown) {
        env.configure_permissionless_resolve(100, 10)
            .expect("enable the public Recovery shutdown control");
        env.warp_to_slot(1);
    }
    if matches!(route, ConsumingRoute::CertifiedTrade) {
        env.trade_no_cpi(0, 1, 0, POS_SCALE as i128 / 4, INITIAL_PRICE, 0)
            .expect("create the live certificate consumed by the retained trade");
    }
    env
}

fn build_nested_case_transaction(
    env: &mut V16Svm,
    route: ConsumingRoute,
) -> solana_sdk::transaction::Transaction {
    match route {
        ConsumingRoute::Deposit => env.build_retained_deposit(0, 1_337),
        ConsumingRoute::Trade => {
            env.build_retained_no_cpi_trade(0, 1, 0, POS_SCALE as i128 / 4, INITIAL_PRICE)
        }
        ConsumingRoute::CertifiedTrade => {
            env.build_retained_no_cpi_trade(0, 1, 0, POS_SCALE as i128 / 8, INITIAL_PRICE)
        }
        ConsumingRoute::Shutdown => env.build_retained_shutdown_asset(0, env.current_slot()),
        ConsumingRoute::BackingTopUpLong | ConsumingRoute::BackingTopUpShort => {
            let domain = u16::from(matches!(route, ConsumingRoute::BackingTopUpShort));
            env.build_retained_backing_bucket_top_up(
                domain,
                1,
                env.current_slot().checked_add(10).expect("expiry slot"),
            )
        }
        ConsumingRoute::ClosePortfolio => env.build_retained_close_primary_portfolio(0),
    }
}

fn apply_nested_byte_case(env: &mut V16Svm, case: NestedByteCase) {
    let key = match case.account {
        NestedAccount::Market => env.market,
        NestedAccount::Portfolio => env.actors[0].portfolio,
    };
    replace_account(env, key, |account| account.data[case.offset] = u8::MAX);
}

#[test]
fn malformed_program_accounts_reject_before_mutation_and_roll_back_exactly() {
    let mut accepted = Vec::new();
    for case in [
        MalformedCase::MarketWrongOwner,
        MalformedCase::MarketTooShort,
        MalformedCase::MarketBadMagic,
        MalformedCase::MarketBadVersion,
        MalformedCase::MarketBadKind,
        MalformedCase::MarketNonzeroWrapperPadding,
        MalformedCase::MarketTrailingByte,
        MalformedCase::PortfolioWrongOwner,
        MalformedCase::PortfolioTooShort,
        MalformedCase::PortfolioBadMagic,
        MalformedCase::PortfolioBadVersion,
        MalformedCase::PortfolioBadKind,
        MalformedCase::PortfolioTrailingByte,
    ] {
        let mut env = V16Svm::new([case as u8; 32], MarketConfig::default());
        let retained = env.build_retained_deposit(0, 1_337);
        apply_malformed_case(&mut env, case);
        let before = snapshot(&env);

        let error = match env.land_retained(retained) {
            Ok(_) => {
                accepted.push(case);
                continue;
            }
            Err(error) => error,
        };

        assert!(
            !error.is_empty(),
            "{case:?}: rejected transaction must expose an error"
        );
        assert_eq!(
            snapshot(&env),
            before,
            "{case:?}: malformed rejected input must roll back exactly"
        );
    }
    assert!(
        accepted.is_empty(),
        "nested malformed account states were accepted: {accepted:?}"
    );
}

#[test]
fn every_wrapper_config_byte_domain_rejects_before_engine_borrow() {
    let cases = [
        (
            "reserved insurance flag",
            core::mem::offset_of!(
                state::WrapperConfigV16,
                _reserved_insurance_withdraw_deposits_only
            ),
            1,
        ),
        (
            "oracle mode",
            core::mem::offset_of!(state::WrapperConfigV16, oracle_mode),
            u8::MAX,
        ),
        (
            "oracle leg count",
            core::mem::offset_of!(state::WrapperConfigV16, oracle_leg_count),
            u8::MAX,
        ),
        (
            "oracle leg flags",
            core::mem::offset_of!(state::WrapperConfigV16, oracle_leg_flags),
            u8::MAX,
        ),
        (
            "oracle invert",
            core::mem::offset_of!(state::WrapperConfigV16, invert),
            2,
        ),
        (
            "wrapper padding",
            core::mem::offset_of!(state::WrapperConfigV16, _padding0),
            1,
        ),
    ];

    for (case_index, (name, field_offset, invalid)) in cases.into_iter().enumerate() {
        let mut env = V16Svm::new([0xd0 + case_index as u8; 32], MarketConfig::default());
        let retained = env.build_retained_deposit(0, 1_337);
        let offset = constants::HEADER_LEN + field_offset;
        let market = env.market;
        replace_account(&mut env, market, |account| account.data[offset] = invalid);
        let before = snapshot(&env);
        let error = match env.land_retained(retained) {
            Ok(_) => panic!("invalid {name} must reject"),
            Err(error) => error,
        };
        assert!(!error.is_empty());
        assert_eq!(snapshot(&env), before, "invalid {name} must roll back");
    }
}

#[test]
fn every_persisted_engine_byte_domain_rejects_on_its_consuming_public_route() {
    for (route_index, route) in [
        ConsumingRoute::Deposit,
        ConsumingRoute::Trade,
        ConsumingRoute::CertifiedTrade,
        ConsumingRoute::Shutdown,
        ConsumingRoute::BackingTopUpLong,
        ConsumingRoute::BackingTopUpShort,
        ConsumingRoute::ClosePortfolio,
    ]
    .into_iter()
    .enumerate()
    {
        let mut control = nested_case_environment([0x70 + route_index as u8; 32], route);
        let before = mutation_snapshot(&control);
        let retained = build_nested_case_transaction(&mut control, route);
        control
            .land_retained(retained)
            .unwrap_or_else(|error| panic!("{route:?} control must mutate successfully: {error}"));
        assert_ne!(
            mutation_snapshot(&control),
            before,
            "{route:?} control must change persistent state"
        );
    }

    let mut accepted = Vec::new();
    for (case_index, case) in nested_byte_cases().into_iter().enumerate() {
        let mut env = nested_case_environment([0x90 + case_index as u8; 32], case.route);
        let retained = build_nested_case_transaction(&mut env, case.route);
        apply_nested_byte_case(&mut env, case);
        let before = snapshot(&env);

        let error = match env.land_retained(retained) {
            Ok(_) => {
                accepted.push(case.name);
                continue;
            }
            Err(error) => error,
        };
        assert!(!error.is_empty(), "{} must expose an error", case.name);
        assert_eq!(
            snapshot(&env),
            before,
            "{} must roll back every persistent effect",
            case.name
        );
    }
    assert!(
        accepted.is_empty(),
        "invalid persisted engine byte domains were accepted: {accepted:?}"
    );
}

#[test]
fn zero_copy_engine_views_are_byte_aligned_and_wrapper_reads_are_unaligned_safe() {
    assert_eq!(
        core::mem::align_of::<percolator::MarketGroupV16HeaderAccount>(),
        1,
        "the borrowed market header must remain byte-aligned"
    );
    assert_eq!(
        core::mem::align_of::<percolator::Market<state::AssetOracleStorageV16>>(),
        1,
        "the borrowed dynamic asset slot must remain byte-aligned"
    );
    assert_eq!(
        core::mem::align_of::<percolator::PortfolioAccountV16Account>(),
        1,
        "the borrowed portfolio engine state must remain byte-aligned"
    );

    let env = V16Svm::new([0x15; 32], MarketConfig::default());
    let market = env.market_data(false);
    let portfolio = env.primary_portfolio_data(0);
    let expected_market = state::read_market(&market).expect("canonical market");
    let expected_portfolio = state::read_portfolio(&portfolio).expect("canonical portfolio");

    let mut shifted_market = vec![0u8; market.len() + 1];
    shifted_market[1..].copy_from_slice(&market);
    let shifted_market_decoded =
        state::read_market(&shifted_market[1..]).expect("unaligned-safe market decode");
    assert_eq!(shifted_market_decoded, expected_market);
    state::market_view_mut(&mut shifted_market[1..]).expect("byte-aligned market zero-copy view");

    let mut shifted_portfolio = vec![0u8; portfolio.len() + 1];
    shifted_portfolio[1..].copy_from_slice(&portfolio);
    let shifted_portfolio_decoded =
        state::read_portfolio(&shifted_portfolio[1..]).expect("unaligned-safe portfolio decode");
    assert_eq!(shifted_portfolio_decoded, expected_portfolio);
    state::portfolio_view_mut_for_market_slots(
        &mut shifted_portfolio[1..],
        expected_market.1.config.max_market_slots as usize,
    )
    .expect("byte-aligned portfolio zero-copy view");
}
