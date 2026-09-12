//! INV-084: bind the deposit proof's input product and full claim path to public states.
//! This is a focused source contract, not an assumption/trace registry or a Kani run.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const SOURCE: &str = include_str!("../kani/inv_013_destructive_consent_scope.rs");
const FILE: &str = "tests/invariants/kani/inv_013_destructive_consent_scope.rs";

// Lock the whole small module, including imports and attributes: a same-named item
// in a comment, literal, nested module or inactive branch must not stand in for it.
const BINDING_PROOF: &str = r#"
use percolator_prog::state;

#[kani::proof]
fn kani_close_binding_accepts_exactly_one_state_tuple() {
    let current_portfolio_id: u64 = kani::any();
    let current_sequence: u64 = kani::any();
    let current_position_epoch: u64 = kani::any();
    let expected_portfolio_id: u64 = kani::any();
    let expected_sequence: u64 = kani::any();
    let expected_position_epoch: u64 = kani::any();

    let accepted = state::portfolio_close_binding_matches(
        current_portfolio_id,
        current_sequence,
        current_position_epoch,
        expected_portfolio_id,
        expected_sequence,
        expected_position_epoch,
    );
    assert_eq!(
        accepted,
        current_portfolio_id == expected_portfolio_id
            && current_sequence == expected_sequence
            && current_position_epoch == expected_position_epoch
    );
}
"#;

// Generate the source expectation and host replay from the same statements. Only
// symbolic inputs become arguments, and assumes become assertions (never skips).
macro_rules! deposit_contract {
    (
        inputs { $(let $input:ident: u64 = kani::any();)+ }
        assumptions { $(kani::assume($predicate:expr);)+ }
        claims { $($claim:tt)* }
        result { $result:ident }
    ) => {
        const DEPOSIT_PROOF: &str = stringify! {
            #[kani::proof]
            fn kani_successful_deposit_sequence_invalidates_prior_close_binding() {
                $(let $input: u64 = kani::any();)+
                $(kani::assume($predicate);)+
                $($claim)*
            }
        };

        fn replay_deposit_proof($($input: u64),+) -> u64 {
            $(assert!($predicate, "public tuple excluded by {}", stringify!($predicate));)+
            $($claim)*
            $result
        }
    };
}

deposit_contract! {
    inputs {
        let portfolio_id: u64 = kani::any();
        let current_sequence: u64 = kani::any();
        let position_epoch: u64 = kani::any();
    }
    assumptions {
        kani::assume(portfolio_id != 0);
        kani::assume(current_sequence < u64::MAX);
    }
    claims {
        let next_sequence = state::next_portfolio_matcher_sequence(current_sequence, current_sequence)
            .expect("nonmaximal matching sequence advances");
        assert!(state::portfolio_close_binding_matches(
            portfolio_id,
            current_sequence,
            position_epoch,
            portfolio_id,
            current_sequence,
            position_epoch,
        ));
        assert!(!state::portfolio_close_binding_matches(
            portfolio_id,
            next_sequence,
            position_epoch,
            portfolio_id,
            current_sequence,
            position_epoch,
        ));
        assert!(state::portfolio_close_binding_matches(
            portfolio_id,
            next_sequence,
            position_epoch,
            portfolio_id,
            next_sequence,
            position_epoch,
        ));
    }
    result { next_sequence }
}

// Same lexical boundary as the recent liveness guard: ignore comments/spacing,
// keep literals opaque. This does not resolve Rust names or expand macros.
fn tokens(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else if bytes[i..].starts_with(b"//") {
            i += bytes[i..]
                .iter()
                .position(|b| *b == b'\n')
                .unwrap_or(bytes.len() - i);
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while depth != 0 {
                assert!(i < bytes.len(), "unterminated source comment");
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else {
            let mut quote = i + usize::from(bytes[i] == b'r');
            while bytes.get(quote) == Some(&b'#') {
                quote += 1;
            }
            if bytes[i] == b'r' && bytes.get(quote) == Some(&b'"') {
                let end = format!("\"{}", "#".repeat(quote - i - 1));
                i = quote + 1;
                i += source[i..].find(&end).expect("unterminated raw string") + end.len();
            } else if bytes[i] == b'"'
                || (bytes[i] == b'\''
                    && (bytes.get(i + 2) == Some(&b'\'') || bytes.get(i + 1) == Some(&b'\\')))
            {
                let delimiter = bytes[i];
                i += 1;
                loop {
                    assert!(i < bytes.len(), "unterminated literal");
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == delimiter {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            } else if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
            } else {
                i += 1;
            }
            result.push(&source[start..i]);
        }
    }
    result
}

fn contract_matches(source: &str) -> bool {
    let expected: Vec<_> = tokens(BINDING_PROOF)
        .into_iter()
        .chain(tokens(DEPOSIT_PROOF))
        .collect();
    tokens(source) == expected
}

fn check_source_and_mutations() {
    assert!(inv_084_mounted_kani_files().contains(FILE));
    assert!(
        contract_matches(SOURCE),
        "deposit proof input/claim contract drift"
    );

    let signature =
        "#[kani::proof]\nfn kani_successful_deposit_sequence_invalidates_prior_close_binding()";
    let (prefix, body) = SOURCE.split_once(signature).unwrap();
    let proof = format!("{signature}{body}");
    let replace_once = |from: &str, to: &str| {
        assert_eq!(proof.matches(from).count(), 1, "unique mutation anchor");
        format!("{prefix}{}", proof.replacen(from, to, 1))
    };
    let mut mutations = Vec::new();
    for input in ["portfolio_id", "current_sequence", "position_epoch"] {
        let value = if input == "portfolio_id" { 1 } else { 0 };
        mutations.push((
            format!("concrete {input}"),
            replace_once(
                &format!("let {input}: u64 = kani::any();"),
                &format!("let {input}: u64 = {value};"),
            ),
        ));
    }
    let assumption = "kani::assume(current_sequence < u64::MAX);";
    for (label, replacement) in [
        ("joint epoch restriction", "kani::assume(current_sequence < u64::MAX && position_epoch == 0);"),
        ("joint sequence restriction", "kani::assume(current_sequence < u64::MAX && current_sequence == position_epoch);"),
        ("masked symbolic epoch", "kani::assume(current_sequence < u64::MAX); let position_epoch = position_epoch & 0;"),
        ("early return with unrelated cover", "kani::assume(current_sequence < u64::MAX); kani::cover!(true, \"entry\"); if position_epoch != 0 { return; }"),
        ("dropped assumption", "kani::assume(true);"),
    ] {
        mutations.push((label.to_owned(), replace_once(assumption, replacement)));
    }
    mutations.push((
        "inactive proof".to_owned(),
        replace_once(signature, &format!("#[cfg(any())]\n{signature}")),
    ));
    mutations.push((
        "vacuous claim".to_owned(),
        replace_once(
            "assert!(!state::portfolio_close_binding_matches(",
            "assert!(true || !state::portfolio_close_binding_matches(",
        ),
    ));
    mutations.push((
        "comment-only proof".to_owned(),
        format!("{prefix}/* {proof} */"),
    ));
    mutations.push((
        "literal-only proof".to_owned(),
        format!("{prefix}const DOCUMENTATION: &str = r###\"{proof}\"###;"),
    ));
    assert_eq!(mutations.len(), 12);
    let inventory = inv_084_source_assumptions(FILE, SOURCE);
    let old_facts = |source: &str| {
        let functions = inv_084_top_level_functions(FILE, source);
        let body =
            &functions["kani_successful_deposit_sequence_invalidates_prior_close_binding"].body;
        let facts = inv_084_direct_function_facts(body);
        [
            facts.symbolic,
            facts.assumption,
            facts.claim,
            facts.cover,
            facts.branch_limited_claim,
        ]
    };
    let mut inventory_preserving = 0;
    for (label, source) in mutations {
        assert!(
            !contract_matches(&source),
            "source mutation accepted: {label}"
        );
        if label.starts_with("concrete ")
            || matches!(label.as_str(), "masked symbolic epoch" | "vacuous claim")
        {
            assert_eq!(
                inv_084_source_assumptions(FILE, &source),
                inventory,
                "{label}"
            );
            assert_eq!(old_facts(&source), old_facts(SOURCE), "{label}");
            inventory_preserving += 1;
        }
    }
    assert_eq!(
        inventory_preserving, 5,
        "net-new checks beyond inventory/category facts"
    );
    let positive = SOURCE.replace(
        assumption,
        &format!("/* outer {{ /* nested */ }} */\n  {assumption} // spacing only"),
    );
    assert!(
        contract_matches(&positive),
        "comments/spacing are not semantics"
    );
}

#[test]
fn v16_program_deposit_proof_contract_admits_public_epoch_sequence_product() {
    check_source_and_mutations();
    let mut env = inv018_public_spl_market_with_params(6, V16CuMarketParams::default());
    env.configure_auth_mark_with_cu(0, 100);
    let admin = env.admin.insecure_clone();
    let owners: [Keypair; 2] = std::array::from_fn(|_| Keypair::new());
    let mut portfolios = [Pubkey::default(); 2];
    let mut sources = [Pubkey::default(); 2];
    const FUNDING: u64 = 1_000_051;
    for (actor, owner) in owners.iter().enumerate() {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            system_instruction::transfer(&env.payer.pubkey(), &owner.pubkey(), 1_000_000_000),
            &[],
        )
        .unwrap();
        let key = Keypair::new();
        portfolios[actor] = key.pubkey();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &key,
            env.portfolio_account_len,
            env.program_id,
        );
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(key.pubkey(), false),
            ],
            &[owner],
        )
        .unwrap();
        env.portfolios.push(key.pubkey());
        sources[actor] = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &sources[actor],
                &admin.pubkey(),
                &[],
                FUNDING,
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();
    }
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::set_authority(
            &spl_token::ID,
            &env.mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &admin.pubkey(),
            &[],
        )
        .unwrap(),
        &[&admin],
    )
    .unwrap();

    let mut tuples = std::collections::BTreeSet::new();
    let mut peak_cu = 0;
    for phase in 0u64..4 {
        if phase >= 2 {
            let size = if phase == 2 {
                POS_SCALE as i128
            } else {
                -(POS_SCALE as i128)
            };
            let cu = env.trade_asset_with_cu(
                0,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                size,
                100,
                0,
            );
            assert_cu_within("nonvacuity trade", cu, MULTI_ASSET_OPEN_TRADE_CU_LIMIT);
            peak_cu = peak_cu.max(cu);
        }
        for actor in 0..2 {
            let portfolio = portfolios[actor];
            let id = env.portfolio_id(portfolio);
            let sequence = env.portfolio_matcher_sequence(portfolio);
            let epoch = env.portfolio_position_epoch(portfolio);
            assert_eq!(id, actor as u64 + 1);
            assert_eq!(epoch, phase.saturating_sub(1));
            assert_eq!(sequence == 0, phase == 0);
            assert_eq!(
                has_active_leg_for_asset(&env.portfolio_state(portfolio), 0),
                phase == 2
            );
            assert!(tuples.insert((id, sequence, epoch)));

            let expected_sequence = replay_deposit_proof(id, sequence, epoch);
            let amount = if phase == 0 { 1_000_000 } else { 17 };
            let capital = env.portfolio_state(portfolio).capital.get();
            let source = env.token_amount(sources[actor]);
            let vault = env.token_amount(env.vault);
            let cu = env
                .send(
                    env.deposit_ix(portfolio, amount),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(sources[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[actor]],
                )
                .expect("public deposit must inhabit the replayed proof transition");
            assert_cu_within("nonvacuity deposit", cu, CUSTODY_CU_LIMIT);
            peak_cu = peak_cu.max(cu);
            assert_eq!(env.portfolio_matcher_sequence(portfolio), expected_sequence);
            assert_eq!(env.portfolio_id(portfolio), id);
            assert_eq!(env.portfolio_position_epoch(portfolio), epoch);
            assert_eq!(
                env.portfolio_state(portfolio).capital.get(),
                capital + amount
            );
            assert_eq!(env.token_amount(sources[actor]), source - amount as u64);
            assert_eq!(env.token_amount(env.vault), vault + amount as u64);
        }
    }
    assert_eq!(tuples.len(), 8);
    for actor in 0..2 {
        assert_eq!(env.token_amount(sources[actor]), 0);
        let cu = env
            .send(
                env.withdraw_ix(portfolios[actor], u128::from(FUNDING)),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(sources[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .expect("return public witness collateral");
        assert_cu_within("nonvacuity withdrawal", cu, CUSTODY_CU_LIMIT);
        peak_cu = peak_cu.max(cu);
        assert_eq!(env.token_amount(sources[actor]), FUNDING);
        assert_eq!(env.portfolio_state(portfolios[actor]).capital.get(), 0);
    }
    assert_eq!(env.token_amount(env.vault), 0);
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(
        (mint.supply, mint.mint_authority),
        (2 * FUNDING, COption::None)
    );
    println!("INV-084 deposit contract: 2 complete proof bodies; 12/12 mutations rejected (5 preserve inventory/category facts); lexical control passed; 8 public input tuples/deposits across fresh, live long/short and flattened epochs; peak checked CU {peak_cu}");
}
