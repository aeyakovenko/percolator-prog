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
//! insurance/backing ledger tails, plus publicly generated backing-earnings withdrawal, add 126
//! pairwise reserve-custody cases plus 40 required privilege downgrades from value-moving controls.
//! Flat close, unilateral reduction, maintenance
//! sync, and permissionless crank add 13 core-account pairs and 13 required downgrades; accepted
//! self-cranker and unsigned no-reward-crank cases have explicit economic-equivalence controls.
//!
//! Guarantee boundary: this is targeted public-route evidence for the most dangerous alias pairs;
//! it is not an exhaustive pairwise account-meta proof for every instruction.

use super::*;

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
    WithdrawBacking,
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
        ReserveCustodyAliasRoute::WithdrawBacking => {
            env.top_up_backing_bucket(0, 100, 10_000);
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
        ReserveCustodyAliasRoute::WithdrawInsurance => ProgInstruction::WithdrawInsuranceAsset {
            market_id: fixture.env.asset_market_id(0),
            asset_index: 0,
            amount: fixture.amount,
        },
        ReserveCustodyAliasRoute::WithdrawBacking => ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            market_id: fixture.env.asset_market_id(0),
            amount: fixture.amount,
        },
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
        ReserveCustodyAliasRoute::WithdrawBacking,
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
            | ReserveCustodyAliasRoute::WithdrawBacking => &[
                "authority",
                "market",
                "destination_token",
                "vault",
                "vault_authority",
                "token_program",
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
    signer: Option<Keypair>,
    instruction: ProgInstruction,
    accounts: Vec<AccountMeta>,
    tracked_accounts: Vec<Pubkey>,
}

fn core_account_alias_snapshot(fixture: &CoreAccountAliasFixture) -> Vec<Option<Account>> {
    fixture
        .tracked_accounts
        .iter()
        .map(|key| fixture.env.svm.get_account(key))
        .collect()
}

fn send_core_account_alias_fixture(
    fixture: &mut CoreAccountAliasFixture,
    expire_blockhash: bool,
) -> Result<u64, String> {
    let signer = fixture
        .signer
        .as_ref()
        .filter(|signer| {
            fixture
                .accounts
                .iter()
                .any(|account| account.is_signer && account.pubkey == signer.pubkey())
        })
        .map(Keypair::insecure_clone);
    let signers = signer.as_ref().into_iter().collect::<Vec<_>>();
    let instruction = fixture.instruction.clone();
    let accounts = fixture.accounts.clone();
    if expire_blockhash {
        fixture.env.svm.expire_blockhash();
    }
    fixture.env.send(instruction, accounts, &signers)
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
        signer: Some(owner),
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
        signer: Some(owner),
        instruction,
        accounts,
        tracked_accounts,
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
        signer: None,
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
        signer: Some(cranker),
        instruction: ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        accounts,
        tracked_accounts,
    }
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
// security.md sweep — liquidation cranker reward account aliasing (#3/#44): when cranker rewards are
// enabled, the optional reward portfolio must be distinct from the portfolio being liquidated. Otherwise
// a liquidated account could receive part of its own liquidation fee back in the same crank.
#[test]
fn v16_attack_liquidation_cranker_reward_cannot_alias_liquidated_account() {
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
        env.svm.expire_blockhash();
        let _ = env.send(
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

    let market_before = env.svm.get_account(&env.market).unwrap();
    let short_before = env.svm.get_account(&s).unwrap();
    let short_cap_before = env.portfolio_state(s).capital.get();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(so.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(s, false),
        ],
        &[&so],
    );
    assert!(
        rejected.is_err(),
        "liquidation reward portfolio must not alias the liquidated account"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "alias rejection leaves market byte-identical"
    );
    assert_eq!(
        env.svm.get_account(&s).unwrap(),
        short_before,
        "alias rejection leaves liquidated account byte-identical"
    );
    assert_eq!(
        env.portfolio_state(s).capital.get(),
        short_cap_before,
        "no self-reward paid into the victim account"
    );

    env.svm.expire_blockhash();
    let rejected_market_reward = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(so.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(env.market, false),
        ],
        &[&so],
    );
    assert!(
        rejected_market_reward.is_err(),
        "liquidation reward portfolio must not alias the market slab"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "market-slab reward alias rejection leaves market byte-identical"
    );
    assert_eq!(
        env.svm.get_account(&s).unwrap(),
        short_before,
        "market-slab reward alias rejection leaves liquidated account byte-identical"
    );

    let cranker_cap_before = env.portfolio_state(c).capital.get();
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
        "distinct reward portfolio liquidation succeeds: {:?}",
        accepted
    );
    assert!(
        env.portfolio_state(c).capital.get() > cranker_cap_before,
        "positive control: distinct cranker received a real reward"
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
        env.svm.expire_blockhash();
        let _ = env.send(
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
