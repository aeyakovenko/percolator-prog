//! INV-015 - Account ownership, layout, discriminator, and length validity.
//!
//! Normative obligation: public CU/LiteSVM routes reject malformed account classes before
//! interpreting them as market, portfolio, ledger, or token state.
//!
//! Evidence in this file complements the public-SBF INV-015 harness with targeted CU routes that
//! exercise account-type confusion through deployed wrapper instructions. Rejections must be real
//! instruction errors and must leave persistent state unchanged.

use super::*;
use percolator_prog::constants;

#[derive(Clone, Copy)]
enum AuxiliaryLedgerKind {
    Backing,
    Insurance,
}

#[derive(Clone, Copy, Debug)]
enum AuxiliaryMalformedCase {
    WrongOwner,
    TooShort,
    BadMagic,
    BadVersion,
    BadKind,
    TrailingByte,
    InvalidSemanticField,
}

fn inv015_sync_auxiliary_ledger_ix(
    env: &V16CuEnv,
    ledger: Pubkey,
    kind: AuxiliaryLedgerKind,
) -> Instruction {
    let data = match kind {
        AuxiliaryLedgerKind::Backing => {
            ProgInstruction::SyncBackingDomainLedger { domain: 0 }.encode()
        }
        AuxiliaryLedgerKind::Insurance => ProgInstruction::SyncInsuranceLedger.encode(),
    };
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger, false),
        ],
        data,
    }
}

fn inv015_create_and_sync_auxiliary_ledger(
    env: &mut V16CuEnv,
    kind: AuxiliaryLedgerKind,
    data_len: usize,
) -> (Pubkey, Result<u64, String>) {
    let ledger = Keypair::new();
    let rent = env.svm.get_sysvar::<solana_sdk::rent::Rent>();
    let create = system_instruction::create_account(
        &env.payer.pubkey(),
        &ledger.pubkey(),
        rent.minimum_balance(data_len),
        data_len as u64,
        &env.program_id,
    );
    let sync = inv015_sync_auxiliary_ledger_ix(env, ledger.pubkey(), kind);
    env.svm.expire_blockhash();
    let result = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), create, sync],
        &[&env.admin, &ledger],
    );
    (ledger.pubkey(), result)
}

#[test]
fn v16_program_auxiliary_ledgers_require_exact_public_account_length() {
    for (kind, required_len, label) in [
        (
            AuxiliaryLedgerKind::Backing,
            state::backing_domain_ledger_account_len(),
            "backing",
        ),
        (
            AuxiliaryLedgerKind::Insurance,
            state::insurance_ledger_account_len(),
            "insurance",
        ),
    ] {
        let mut oversized_env = V16CuEnv::new();
        let market_before = oversized_env
            .svm
            .get_account(&oversized_env.market)
            .unwrap();
        let (oversized, rejected) =
            inv015_create_and_sync_auxiliary_ledger(&mut oversized_env, kind, required_len + 1);
        assert!(
            rejected.is_err(),
            "{label} ledger with trailing storage must reject"
        );
        assert!(
            oversized_env.svm.get_account(&oversized).is_none(),
            "atomic rejection must roll back the oversized {label} ledger creation"
        );
        assert_eq!(
            oversized_env
                .svm
                .get_account(&oversized_env.market)
                .unwrap(),
            market_before,
            "oversized {label} ledger rejection must not mutate the market"
        );

        let mut exact_env = V16CuEnv::new();
        let (exact, accepted) =
            inv015_create_and_sync_auxiliary_ledger(&mut exact_env, kind, required_len);
        let cu = accepted.expect("canonical auxiliary ledger must remain initializable");
        assert_cu_within(
            "INV-015 canonical auxiliary ledger initialization",
            cu,
            CUSTODY_CU_LIMIT,
        );
        let account = exact_env.svm.get_account(&exact).unwrap();
        assert_eq!(account.data.len(), required_len);
        match kind {
            AuxiliaryLedgerKind::Backing => {
                state::read_backing_domain_ledger(&account.data).unwrap();
            }
            AuxiliaryLedgerKind::Insurance => {
                state::read_insurance_ledger(&account.data).unwrap();
            }
        }
    }
}

#[test]
fn v16_program_initialized_auxiliary_ledger_malformed_matrix_rejects_exactly() {
    for (kind, required_len, label) in [
        (
            AuxiliaryLedgerKind::Backing,
            state::backing_domain_ledger_account_len(),
            "backing",
        ),
        (
            AuxiliaryLedgerKind::Insurance,
            state::insurance_ledger_account_len(),
            "insurance",
        ),
    ] {
        for case in [
            AuxiliaryMalformedCase::WrongOwner,
            AuxiliaryMalformedCase::TooShort,
            AuxiliaryMalformedCase::BadMagic,
            AuxiliaryMalformedCase::BadVersion,
            AuxiliaryMalformedCase::BadKind,
            AuxiliaryMalformedCase::TrailingByte,
            AuxiliaryMalformedCase::InvalidSemanticField,
        ] {
            let mut env = V16CuEnv::new();
            let (ledger, initialized) =
                inv015_create_and_sync_auxiliary_ledger(&mut env, kind, required_len);
            initialized.expect("canonical control must initialize before malformed mutation");

            let mut malformed = env.svm.get_account(&ledger).unwrap();
            match case {
                AuxiliaryMalformedCase::WrongOwner => {
                    malformed.owner = solana_sdk::system_program::ID;
                }
                AuxiliaryMalformedCase::TooShort => {
                    malformed.data.truncate(required_len - 1);
                }
                AuxiliaryMalformedCase::BadMagic => malformed.data[0] ^= 0x80,
                AuxiliaryMalformedCase::BadVersion => malformed.data[8] ^= 0x80,
                AuxiliaryMalformedCase::BadKind => malformed.data[10] ^= 0x7f,
                AuxiliaryMalformedCase::TrailingByte => malformed.data.push(0),
                AuxiliaryMalformedCase::InvalidSemanticField => match kind {
                    AuxiliaryLedgerKind::Backing => {
                        let offset = constants::HEADER_LEN
                            + core::mem::offset_of!(state::BackingDomainLedgerAccountV16, _padding);
                        malformed.data[offset] = 1;
                    }
                    AuxiliaryLedgerKind::Insurance => {
                        malformed.data[constants::HEADER_LEN] = 0;
                        malformed.data[constants::HEADER_LEN + 1..constants::HEADER_LEN + 32]
                            .fill(0);
                    }
                },
            }
            env.svm.set_account(ledger, malformed.clone()).unwrap();
            let market_before = env.svm.get_account(&env.market).unwrap();

            let admin = Keypair::from_bytes(&env.admin.to_bytes()).unwrap();
            env.svm.expire_blockhash();
            let rejected = env.send(
                match kind {
                    AuxiliaryLedgerKind::Backing => {
                        ProgInstruction::SyncBackingDomainLedger { domain: 0 }
                    }
                    AuxiliaryLedgerKind::Insurance => ProgInstruction::SyncInsuranceLedger,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(ledger, false),
                ],
                &[&admin],
            );
            assert!(
                rejected.is_err(),
                "malformed {label} ledger case {case:?} must reject"
            );
            assert_eq!(
                env.svm.get_account(&ledger).unwrap(),
                malformed,
                "malformed {label} ledger rejection must roll back exactly"
            );
            assert_eq!(
                env.svm.get_account(&env.market).unwrap(),
                market_before,
                "malformed {label} ledger rejection must not mutate the market"
            );
        }
    }
}

#[test]
fn v16_program_init_portfolio_canonicalizes_oversized_uninitialized_account() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = Keypair::new();
    env.ensure_signer_account(owner.pubkey());

    let oversized_len = env.portfolio_account_len + 1;
    let rent = env.svm.get_sysvar::<solana_sdk::rent::Rent>();
    let create = system_instruction::create_account(
        &env.payer.pubkey(),
        &portfolio.pubkey(),
        rent.minimum_balance(oversized_len),
        oversized_len as u64,
        &env.program_id,
    );
    let init = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
        ],
        data: ProgInstruction::InitPortfolio.encode(),
    };

    env.svm.expire_blockhash();
    let init_cu = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), create, init],
        &[&owner, &portfolio],
    )
    .expect("public InitPortfolio must canonicalize an oversized uninitialized account");
    assert_cu_within(
        "INV-015 oversized InitPortfolio canonicalization",
        init_cu,
        CUSTODY_CU_LIMIT,
    );

    let initialized = env.svm.get_account(&portfolio.pubkey()).unwrap();
    assert_eq!(
        initialized.data.len(),
        env.portfolio_account_len,
        "public initialization must not preserve ambiguous trailing bytes",
    );
    assert!(state::is_initialized(&initialized.data));
    assert_eq!(env.market_state().1.materialized_portfolio_count, 1);

    env.deposit(&owner, portfolio.pubkey(), 1_000);
    assert_eq!(env.portfolio_state(portfolio.pubkey()).capital.get(), 1_000);
}

// Account-type confusion must reject before state mutation or value movement.
#[test]
fn v16_program_account_type_confusion_rejected() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();

    // 1) withdraw naming the MARKET account as the portfolio.
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, la.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let r1 = env.send(
        ProgInstruction::Withdraw {
            portfolio_id: 0,
            expected_sequence: 0,
            amount: 1,
        },
        vec![
            AccountMeta::new(la.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&la],
    );
    assert!(r1.is_err(), "withdraw with market-as-portfolio must reject");

    // 2) trade naming the VAULT as account_a.
    env.svm.expire_blockhash();
    let r2 = env.send(
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: 0,
            account_a_position_epoch: 0,
            account_b_portfolio_id: env.portfolio_id(pb),
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        vec![
            AccountMeta::new(la.pubkey(), true),
            AccountMeta::new(lb.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(pb, false),
        ],
        &[&la, &lb],
    );
    assert!(r2.is_err(), "trade with vault-as-portfolio must reject");

    // 3) crank naming an uninitialized (system) account as the portfolio.
    let junk = Pubkey::new_unique();
    env.svm
        .set_account(
            junk,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; 64],
                owner: solana_sdk::system_program::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let r3 = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(junk, false),
        ],
        &[],
    );
    assert!(
        r3.is_err(),
        "crank with uninitialized-account-as-portfolio must reject"
    );

    let (_, g1) = env.market_state();
    assert_eq!(
        g1.c_tot, g0.c_tot,
        "no capital moved by confused-account calls"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
}

#[test]
fn v16_attack_sync_maintenance_bad_cranker_rolls_back_fee() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let payer_owner = Keypair::new();
    let payer_portfolio = env.create_portfolio(&payer_owner);
    env.deposit(&payer_owner, payer_portfolio, 100_000_000);
    env.update_maintenance_fee_policy_with_cu(4_000);

    let bad_cranker_portfolio = env.program_account(env.portfolio_account_len);
    let payer_before = env.svm.get_account(&payer_portfolio).unwrap().data;
    let market_before = env.svm.get_account(&env.market).unwrap().data;
    let bad_cranker_before = env.svm.get_account(&bad_cranker_portfolio).unwrap().data;

    env.svm.warp_to_slot(10);
    env.svm.expire_blockhash();
    let err = env
        .try_sync_maintenance_fee_with_cu(payer_portfolio, Some(bad_cranker_portfolio), 10)
        .expect_err("malformed cranker reward account must reject");
    assert!(
        err.contains("TransactionError") || err.contains("InstructionError"),
        "unexpected maintenance bad-cranker error: {err}"
    );

    assert_eq!(
        env.svm.get_account(&payer_portfolio).unwrap().data,
        payer_before,
        "failed cranker maintenance sync must not charge the payer"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before,
        "failed cranker maintenance sync must not credit insurance or mutate market accounting"
    );
    assert_eq!(
        env.svm.get_account(&bad_cranker_portfolio).unwrap().data,
        bad_cranker_before,
        "failed cranker maintenance sync must not mutate the malformed reward account"
    );

    let cranker_owner = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.svm.expire_blockhash();
    env.sync_maintenance_fee_with_cu(payer_portfolio, Some(cranker_portfolio), 10);
    assert_eq!(env.portfolio_state(payer_portfolio).last_fee_slot.get(), 10);
    assert!(
        env.portfolio_state(cranker_portfolio).capital.get() > 0,
        "valid cranker reward path should still pay the cranker share"
    );
}

// security.md sweep — portfolio-as-market confusion (#44/#45): passing a portfolio account where the
// market is expected must reject (the market view decode fails on portfolio-shaped data). No cross-
// type confusion drains funds.
#[test]
fn v16_attack_portfolio_as_market_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    let other = Keypair::new();
    let p2 = env.create_portfolio(&other);
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    // withdraw but pass a PORTFOLIO account (p2) in the MARKET slot.
    env.svm.expire_blockhash();
    let r = env.send(
        env.withdraw_ix(p, 500_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(p2, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "withdraw with a portfolio in the market slot must reject"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no funds drained via type confusion"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "capital intact"
    );
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged");
}

// security.md sweep — deposit into an uninitialized portfolio rejects (#44/#45): an account that was
// never InitPortfolio'd (zeroed data) is not a valid portfolio — its stored id is all-zero, not its key.
// Attacker goal: deposit into a raw program-owned account to corrupt the accounting / create a portfolio
// the engine didn't initialize. Protection: the identity/validation check rejects; vault & source intact.
#[test]
fn v16_attack_deposit_into_uninitialized_portfolio_rejects() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    env.ensure_signer_account(owner.pubkey());
    // a RAW, never-initialized account: program-owned, correct size, all-zero data.
    let raw = Pubkey::new_unique();
    let plen = env.portfolio_account_len;
    env.svm
        .set_account(
            raw,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; plen],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    // a funded source token account.
    let source = env.token_account_for_mint(env.mint, owner.pubkey(), 1_000);
    let (_, g0) = env.market_state();

    env.svm.expire_blockhash();
    let r = env.send(
        ProgInstruction::Deposit {
            portfolio_id: 0,
            expected_sequence: 0,
            amount: 1_000,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(raw, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "deposit into an uninitialized portfolio must reject"
    );

    // nothing moved: source tokens not pulled, vault/c_tot unchanged.
    assert_eq!(
        env.token_amount(source),
        1_000,
        "source tokens not pulled into a non-portfolio"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(
        g1.c_tot, g0.c_tot,
        "c_tot unchanged (no phantom capital from an uninit account)"
    );
}
