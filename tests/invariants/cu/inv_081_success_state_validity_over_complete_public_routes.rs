//! INV-081 - Success-state validity over complete public routes.
//!
//! Normative obligation: every successful public wrapper instruction commits a globally valid
//! post-state and only the economic deltas authorized by the instruction's signer/account set.
//! Every rejected wrapper route must leave the complete tracked public state unchanged.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM scenario uses the shared whole-route
//! public-interface oracle, not a hand-written state mutation. The scenario runs successful
//! deposits, withdrawals, all four trade routes, mark/crank progress, and liquidation coverage
//! through the runner's mandatory prefix, then adds a fixed mixed route with an over-policy CPI
//! trade rejection. After every successful wrapper instruction the oracle checks SPL supply,
//! vault/accounting equality, source-credit attribution, OI/current-leg shape, and authorized
//! token/account frames. Rejected routes are checked by byte-for-byte snapshots of market,
//! portfolio, backing-ledger, matcher-context, and SPL-token state.
//!
//! A separate native-quote round trip checks wrapped-SOL token amounts and backing lamports
//! together, excluding rent and unsynced SOL from capital, through live owner withdrawal and
//! redemption. It consumes the existing stock/encumbrance censuses and engine shape contracts.
//!
//! The source-composition gate in this file closes the route-count dimension without duplicating
//! those scenarios. It joins the complete decoder/route/account/input/admission inventories to the
//! wrapper-to-engine transition, wrapper-field, value, stock, certificate, position/OI, scope,
//! adversarial-role containment, rollback, and independent-model owners. Each owner remains
//! independently executable; this gate fails if one disappears or the 49-route registry acquires
//! an omission.
//!
//! Guarantee boundary: this is a proof-equivalence decomposition for the current deployed surface,
//! not one solver query over arbitrary account bytes. It relies on the exact pinned engine
//! postconditions, the mounted wrapper Kani theorems, standard SVM rollback, and the assumptions
//! named by each composing invariant. A new route, persisted field, engine call, effect class, or
//! engine pin reopens the composition.

use crate::support::fuzz_model::{
    run_scenario, Action, HintMode, Scenario, SmallMarketConfig, TradeRoute,
};

#[path = "inv_081_fee_resolution_atomicity.rs"]
mod fee_resolution_atomicity;

#[derive(Clone, Copy)]
struct Inv081CompositionOwner {
    layer: &'static str,
    path: &'static str,
    test: &'static str,
}

fn inv081_source_defines_test(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    let mut saw_test = false;
    for line in source.lines() {
        let line = line.trim();
        if line == "#[test]" {
            saw_test = true;
        } else if line.starts_with("fn ") {
            if saw_test
                && line
                    .strip_prefix(&marker)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            saw_test = false;
        } else if saw_test && !line.is_empty() && !line.starts_with('#') {
            saw_test = false;
        }
    }
    false
}

fn inv081_source_defines_kani_proof(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    let mut saw_proof = false;
    for line in source.lines() {
        let line = line.trim();
        if line == "#[kani::proof]" {
            saw_proof = true;
        } else if line.starts_with("fn ") {
            if saw_proof
                && line
                    .strip_prefix(&marker)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            saw_proof = false;
        } else if saw_proof && !line.is_empty() && !line.starts_with("#[") {
            saw_proof = false;
        }
    }
    false
}

#[test]
fn v16_program_success_state_validity_composition_is_source_complete() {
    const ENGINE_PIN: &str = "495a5590c97055bd71c6f94d849ff0298f243145";
    const OWNERS: &[Inv081CompositionOwner] = &[
        Inv081CompositionOwner {
            layer: "production instruction and public witness roster",
            path: "tests/invariants/public_sbf/inv_079_public_reachability_evidence.rs",
            test: "v16_public_instruction_coverage_registry_matches_production_roster",
        },
        Inv081CompositionOwner {
            layer: "canonical decoder and schema boundary",
            path: "tests/invariants/cu/inv_022_instruction_decoding_and_schema_upgrade_safety.rs",
            test: "v16_program_encoded_public_instruction_roster_rejects_trailing_bytes",
        },
        Inv081CompositionOwner {
            layer: "transaction signature domain",
            path: "tests/invariants/public_sbf/inv_006_program_chain_message_type_and_version_binding.rs",
            test: "deployed_wrapper_has_no_detached_signature_interpreter",
        },
        Inv081CompositionOwner {
            layer: "program account incarnation",
            path: "tests/invariants/public_sbf/inv_007_no_aba_reuse.rs",
            test: "v16_wrapper_account_incarnation_census_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "asset generation binding",
            path: "tests/invariants/cu/inv_002_asset_generation_binding.rs",
            test: "v16_program_asset_generation_field_and_guard_roster_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "portfolio incarnation binding",
            path: "tests/invariants/cu/inv_003_portfolio_incarnation_binding.rs",
            test: "v16_program_retained_portfolio_binding_roster_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "position episode binding",
            path: "tests/invariants/cu/inv_004_position_episode_binding.rs",
            test: "v16_program_retained_position_binding_and_writer_rosters_are_source_complete",
        },
        Inv081CompositionOwner {
            layer: "configured authority binding",
            path: "tests/invariants/cu/inv_005_authority_incarnation_binding.rs",
            test: "v16_program_configured_authority_route_dispositions_are_source_complete",
        },
        Inv081CompositionOwner {
            layer: "adversarial role economic containment",
            path: "tests/invariants/cu/inv_005_authority_incarnation_binding.rs",
            test: "v16_program_adversarial_role_containment_matrix_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "account roles signers writability and aliases",
            path: "tests/invariants/cu/inv_017_signer_writable_role_and_account_alias_safety.rs",
            test: "v16_program_account_role_matrix_roster_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "canonical PDA and token movement callsites",
            path: "tests/invariants/cu/inv_016_canonical_pda_and_seed_binding.rs",
            test: "v16_program_pda_and_token_move_callsite_roster_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "caller field confinement and boundaries",
            path: "tests/invariants/cu/inv_023_caller_input_confinement_for_derived_safety_state.rs",
            test: "v16_program_caller_input_roster_owns_every_production_field",
        },
        Inv081CompositionOwner {
            layer: "state indexed admission",
            path: "tests/invariants/cu/inv_055_state_indexed_admission.rs",
            test: "v16_program_every_public_instruction_has_a_state_admission_owner",
        },
        Inv081CompositionOwner {
            layer: "engine transition summary and certificate disposition",
            path: "tests/invariants/cu/inv_088_global_summaries_are_not_account_local_proofs.rs",
            test: "v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness",
        },
        Inv081CompositionOwner {
            layer: "wrapper persisted field disposition",
            path: "tests/invariants/cu/inv_087_no_phantom_controls_or_dead_security_fields.rs",
            test: "v16_program_all_wrapper_owned_persisted_structs_have_complete_field_rosters",
        },
        Inv081CompositionOwner {
            layer: "position and effective OI transition induction",
            path: "tests/invariants/cu/inv_048_matched_trade_and_open_interest_coherence.rs",
            test: "v16_program_position_mutation_composition_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "typed matched-book obligation census",
            path: "tests/invariants/cu/inv_048_matched_trade_and_open_interest_coherence.rs",
            test: "v16_program_typed_matched_book_obligation_oracle_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "scope frame induction",
            path: "tests/invariants/cu/inv_074_scope_locality.rs",
            test: "v16_program_scope_locality_composition_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "external token and internal quote delta",
            path: "tests/invariants/cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs",
            test: "v16_primary_quote_routes_match_actual_spl_and_internal_accounting_deltas",
        },
        Inv081CompositionOwner {
            layer: "per-episode entitlement and public value-effect dispositions",
            path: "tests/invariants/cu/inv_024_attributed_quote_value_conservation.rs",
            test: "v16_program_entitlement_effect_roster_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "independent transition model dimensions",
            path: "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
            test: "v16_program_reference_model_dimension_composition_is_source_complete",
        },
        Inv081CompositionOwner {
            layer: "engine error propagation and SVM rollback boundary",
            path: "tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs",
            test: "v16_program_dispatch_and_entrypoints_preserve_every_handler_error",
        },
    ];
    const KANI_OWNERS: &[(&str, &str)] = &[
        (
            "tests/invariants/kani/inv_024_attributed_quote_value_conservation.rs",
            "kani_inv024_engine_flow_validator_equals_wrapper_value_equation",
        ),
        (
            "tests/invariants/kani/inv_024_attributed_quote_value_conservation.rs",
            "kani_inv024_per_episode_entitlement_is_stronger_than_aggregate_conservation",
        ),
        (
            "tests/invariants/kani/inv_025_exact_stock_reconciliation.rs",
            "kani_inv025_engine_partition_composes_with_wrapper_spl_custody",
        ),
        (
            "tests/invariants/kani/inv_053_full_health_recertification_equivalence.rs",
            "kani_inv053_wrapper_commit_is_invalid_or_no_healthier_under_engine_disposition",
        ),
        (
            "tests/invariants/kani/inv_080_error_propagation_and_exact_rollback.rs",
            "kani_v16_inv080_every_engine_error_maps_to_instruction_error",
        ),
    ];

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut layers = std::collections::BTreeSet::new();
    for owner in OWNERS {
        assert!(layers.insert(owner.layer), "duplicate INV-081 layer");
        let source = std::fs::read_to_string(root.join(owner.path))
            .unwrap_or_else(|error| panic!("read {}: {error}", owner.path));
        assert!(
            inv081_source_defines_test(&source, owner.test),
            "INV-081 layer '{}' lacks executable owner {}#{}",
            owner.layer,
            owner.path,
            owner.test,
        );
    }
    assert_eq!(layers.len(), 22, "INV-081 composition layer drift");

    for (path, theorem) in KANI_OWNERS {
        let source = std::fs::read_to_string(root.join(path))
            .unwrap_or_else(|error| panic!("read {path}: {error}"));
        assert!(
            inv081_source_defines_kani_proof(&source, theorem),
            "INV-081 lacks mounted proof owner {path}#{theorem}",
        );
    }

    let registry = include_str!("../public_instruction_coverage.tsv");
    let route_rows = registry
        .lines()
        .filter(|line| {
            !line.is_empty() && !line.starts_with('#') && !line.starts_with("tag\tvariant\t")
        })
        .collect::<Vec<_>>();
    assert_eq!(route_rows.len(), 49, "public instruction roster drift");
    assert!(
        route_rows.iter().all(|line| !line.contains("\tOMITTED\t")),
        "every public route needs executable success and CU evidence",
    );

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-081 composition must be reviewed on every engine pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the exact composed engine revision",
    );
}

#[test]
fn v16_program_public_route_oracle_checks_success_and_reject_frames_fixed_case() {
    let scenario = Scenario {
        seed: [0x81; 32],
        config: SmallMarketConfig::default(),
        actions: vec![
            Action::Deposit {
                actor: 0,
                amount: 17,
            },
            Action::Trade {
                route: TradeRoute::BatchCpi,
                taker: 0,
                maker: 2,
                asset: 2,
                units: 1,
                fee_bps: 17,
                price_move_bps: 4,
                prefer_reduce: false,
            },
            Action::PushMark {
                asset: 2,
                dt: 2,
                move_bps: -75,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Complete,
            },
            Action::Trade {
                route: TradeRoute::Cpi,
                taker: 1,
                maker: 2,
                asset: 0,
                units: 1,
                fee_bps: u16::MAX,
                price_move_bps: 0,
                prefer_reduce: false,
            },
            Action::Withdraw {
                actor: 0,
                amount: 7,
            },
        ],
    };

    let coverage = run_scenario(&scenario).unwrap_or_else(|error| {
        panic!(
            "INV-081 deterministic public-route scenario failed\nscenario={}\n{error}",
            serde_json::to_string_pretty(&scenario).unwrap()
        )
    });

    assert_ne!(
        coverage.loaded_program_hash, [0; 32],
        "the deployed SBF artifact hash must be recorded"
    );
    assert!(
        coverage
            .route_success
            .iter()
            .all(|successes| *successes != 0),
        "all four public trade routes must have successful authorized deltas"
    );
    assert!(
        coverage.route_reject.iter().copied().sum::<u64>() != 0,
        "the over-policy trade rejection path must be exercised with exact rollback"
    );
    assert!(
        coverage.deposits != 0 && coverage.withdrawals != 0 && coverage.token_frame_checks != 0,
        "custody-changing routes must be checked against exact token frames"
    );
    assert!(
        coverage.crank_progress != 0,
        "permissionless crank progress must remain part of the complete public route oracle"
    );
    assert!(
        coverage.liquidation_steps != 0 && coverage.liquidated_abs_q != 0,
        "liquidation progress must remain part of the complete public route oracle"
    );
}

pub(super) fn inv081_public_native_market() -> super::V16CuEnv {
    use super::*;

    let mut svm = LiteSVM::new();
    let program_id = percolator_prog::id();
    for (program, path) in [
        (program_id, program_path()),
        (spl_token::ID, spl_token_program_path()),
        (
            associated_token_program_id(),
            associated_token_program_path(),
        ),
    ] {
        svm.add_program(program, &std::fs::read(path).unwrap());
    }
    let payer = Keypair::new();
    let admin = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();

    // LiteSVM omits the native mint genesis account. All subsequent accounts and economic
    // transitions are created through System, ATA, SPL, or the public wrapper, never byte edits.
    let mint = spl_token::native_mint::ID;
    svm.set_account(
        mint,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(Mint::LEN),
            data: make_mint_data_with_decimals(spl_token::native_mint::DECIMALS),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    let params = V16CuMarketParams::default();
    let market = Keypair::new();
    system_create_account_for_test(
        &mut svm,
        &payer,
        &market,
        state::market_account_len_for_capacity(1).unwrap(),
        program_id,
    );
    let vault_authority =
        Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
    let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint);
    let init_market_cu = send_tx(
        &mut svm,
        program_id,
        &payer,
        init_market_instruction(&params),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new_readonly(mint, false),
        ],
        &[&admin],
    )
    .expect("public native-quote market initialization");
    V16CuEnv {
        svm,
        program_id,
        payer,
        admin,
        init_market_cu,
        market: market.pubkey(),
        mint,
        vault,
        vault_authority,
        portfolio_account_len: state::portfolio_account_len_for_market_slots(1).unwrap(),
        portfolios: Vec::new(),
    }
}

#[test]
fn v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value() {
    use super::*;
    use crate::support::fuzz_model::{
        assert_market_stock_census, assert_reservation_encumbrance_census,
    };

    const WRAPPED: u64 = 137;
    const UNSYNCED: u64 = 19;
    const PARTIAL: u64 = 37;

    let mut env = inv081_public_native_market();
    let owner = Keypair::new();
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let mint = env.mint;
    let vault = env.vault;
    let vault_authority = env.vault_authority;
    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio,
        env.portfolio_account_len,
        env.program_id,
    );
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
        ],
        &[&owner],
    )
    .expect("public portfolio initialization");
    let portfolio = portfolio.pubkey();
    env.portfolios.push(portfolio);
    let user_token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), mint);
    let empty_user = env.svm.get_account(&user_token).unwrap();
    let empty_vault = env.svm.get_account(&vault).unwrap();
    let token_rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    for (account, token_owner) in [
        (&empty_user, owner.pubkey()),
        (&empty_vault, vault_authority),
    ] {
        let token = TokenAccount::unpack(&account.data).unwrap();
        assert_eq!(token.mint, mint);
        assert_eq!(token.owner, token_owner);
        assert_eq!(token.is_native, COption::Some(token_rent));
        assert_eq!(token.amount, 0);
        assert_eq!(account.lamports, token_rent);
    }
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        system_instruction::transfer(&owner.pubkey(), &user_token, WRAPPED),
        &[&owner],
    )
    .unwrap();
    assert_eq!(env.token_amount(user_token), 0);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::sync_native(&spl_token::ID, &user_token).unwrap(),
        &[],
    )
    .unwrap();
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        system_instruction::transfer(&owner.pubkey(), &user_token, UNSYNCED),
        &[&owner],
    )
    .unwrap();

    let config = env.market_state().0;
    assert_eq!(config.collateral_mint, mint.to_bytes());
    let portfolio_id = env.portfolio_id(portfolio);
    let portfolio_rent = env.svm.get_account(&portfolio).unwrap().lamports;
    let market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
    let passive_keys = [
        env.mint,
        env.admin.pubkey(),
        owner.pubkey(),
        vault_authority,
    ];
    let passive_before = passive_keys.map(|key| env.svm.get_account(&key));
    let check = |env: &V16CuEnv, capital: u64, sequence: u64, closed: bool| {
        for (key, initial, amount, unsynced) in [
            (user_token, &empty_user, WRAPPED - capital, UNSYNCED),
            (vault, &empty_vault, capital, 0),
        ] {
            let mut expected = initial.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            expected.lamports += amount + unsynced;
            assert_eq!(
                env.svm.get_account(&key),
                Some(expected),
                "native custody must change only token amount and its backing lamports at {key}"
            );
        }
        assert_eq!(
            passive_keys.map(|key| env.svm.get_account(&key)),
            passive_before
        );
        let mut market_data = env.svm.get_account(&env.market).unwrap().data;
        let (cfg, group) = state::read_market(&market_data).unwrap();
        assert_eq!(cfg, config);
        assert_eq!(
            (group.vault, group.c_tot, group.insurance),
            (capital.into(), capital.into(), 0)
        );
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            market_lamports + if closed { portfolio_rent } else { 0 }
        );
        let portfolios = if closed {
            assert_eq!(capital, 0);
            assert!(env
                .svm
                .get_account(&portfolio)
                .is_none_or(|account| { account.lamports == 0 && account.data.is_empty() }));
            Vec::new()
        } else {
            assert_eq!(env.portfolio_id(portfolio), portfolio_id);
            assert_eq!(env.portfolio_matcher_sequence(portfolio), sequence);
            assert_eq!(env.portfolio_position_epoch(portfolio), 0);
            assert_eq!(
                env.svm.get_account(&portfolio).unwrap().lamports,
                portfolio_rent
            );
            let account = env.portfolio_state(portfolio);
            assert_eq!(account.capital.get(), u128::from(capital));
            assert_eq!(account.owner, owner.pubkey().to_bytes());
            vec![account]
        };
        assert_market_stock_census(
            "INV-081 native quote",
            &group,
            &market_data,
            &portfolios,
            u128::from(env.token_amount(vault)),
        )
        .unwrap();
        assert_reservation_encumbrance_census("INV-081 native quote", &group, &portfolios).unwrap();
        let (_, view) = state::market_view_mut(&mut market_data).unwrap();
        view.validate_shape().unwrap();
        if !closed {
            let mut data = env.svm.get_account(&portfolio).unwrap().data;
            state::portfolio_view_mut_for_market_slots(&mut data, 1)
                .unwrap()
                .validate_with_market(&view.as_view())
                .unwrap();
        }
    };
    check(&env, 0, 0, false);
    let deposit_cu = env
        .send(
            env.deposit_ix(portfolio, WRAPPED.into()),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(user_token, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .expect("deposit native collateral without crediting rent or unsynced SOL");
    assert_cu_within("native deposit", deposit_cu, CUSTODY_CU_LIMIT);
    check(&env, WRAPPED, 1, false);
    for (amount, remaining, sequence) in
        [(PARTIAL, WRAPPED - PARTIAL, 2), (WRAPPED - PARTIAL, 0, 3)]
    {
        let cu = env
            .send(
                env.withdraw_ix(portfolio, amount.into()),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(user_token, false),
                    AccountMeta::new(vault, false),
                    AccountMeta::new_readonly(vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owner],
            )
            .expect("withdraw native token atoms and exactly the same backing lamports");
        assert_cu_within("native withdrawal", cu, CUSTODY_CU_LIMIT);
        check(&env, remaining, sequence, false);
    }
    env.close_portfolio_with_cu(&owner, portfolio);
    check(&env, 0, 3, true);

    let market_before_redemption = env.svm.get_account(&env.market);
    let vault_before_redemption = env.svm.get_account(&vault);
    let mut owner_after_redemption = env.svm.get_account(&owner.pubkey()).unwrap();
    owner_after_redemption.lamports += token_rent + WRAPPED + UNSYNCED;
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::close_account(
            &spl_token::ID,
            &user_token,
            &owner.pubkey(),
            &owner.pubkey(),
            &[],
        )
        .unwrap(),
        &[&owner],
    )
    .expect("redeem wrapped SOL plus unsynced SOL and the user's token-account rent");
    assert_eq!(
        env.svm.get_account(&owner.pubkey()),
        Some(owner_after_redemption)
    );
    assert!(env
        .svm
        .get_account(&user_token)
        .is_none_or(|account| { account.lamports == 0 && account.data.is_empty() }));
    assert_eq!(env.svm.get_account(&env.market), market_before_redemption);
    assert_eq!(env.svm.get_account(&vault), vault_before_redemption);
    assert_eq!(env.svm.get_account(&mint), passive_before[0]);
    println!(
        "INV-081 native roundtrip: {WRAPPED} quote atoms, {UNSYNCED} unbooked lamports, \
         {token_rent} token rent; partial/full payout and owner redemption are exact"
    );
}
