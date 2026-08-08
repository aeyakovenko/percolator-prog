//! INV-015 - Account ownership, layout, discriminator, and length validity.
//!
//! Public wrappers must reject malformed program-owned accounts before any zero-copy view or
//! mutation can commit. This module uses a retained, otherwise-valid deposit as the public route,
//! then supplies malformed market/portfolio account state at execution time. Every case must return
//! an instruction error and leave the complete persistent state unchanged.
//!
//! Guarantee boundary: these are malformed-input validation checks, not accepted LoF/DoS
//! counterexamples. The test intentionally creates malformed account fixtures to prove the wrapper
//! fails closed and relies on SVM rollback only after a real transaction error is returned.

use super::support::v16_svm::{MarketConfig, V16Svm};
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
    MarketBadKind,
    PortfolioWrongOwner,
    PortfolioTooShort,
    PortfolioBadMagic,
    PortfolioBadKind,
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
        MalformedCase::MarketBadKind => {
            replace_account(env, env.market, |account| account.data[10] ^= 0x7f);
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
        MalformedCase::PortfolioBadKind => {
            replace_account(env, env.actors[0].portfolio, |account| {
                account.data[10] ^= 0x7f
            });
        }
    }
}

#[test]
fn malformed_program_accounts_reject_before_mutation_and_roll_back_exactly() {
    for case in [
        MalformedCase::MarketWrongOwner,
        MalformedCase::MarketTooShort,
        MalformedCase::MarketBadMagic,
        MalformedCase::MarketBadKind,
        MalformedCase::PortfolioWrongOwner,
        MalformedCase::PortfolioTooShort,
        MalformedCase::PortfolioBadMagic,
        MalformedCase::PortfolioBadKind,
    ] {
        let mut env = V16Svm::new([case as u8; 32], MarketConfig::default());
        let retained = env.build_retained_deposit(0, 1_337);
        apply_malformed_case(&mut env, case);
        let before = snapshot(&env);

        let error = match env.land_retained(retained) {
            Ok(_) => panic!("{case:?}: malformed account was accepted"),
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
}
