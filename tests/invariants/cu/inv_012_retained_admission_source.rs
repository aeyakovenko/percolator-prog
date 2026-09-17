//! INV-012/004/014: executable retained matcher admission, from dispatch to effects.
//!
//! The older rosters count/check source strings and behavioral tests sample histories.
//! This guard discovers references to the grant writer, capability consumer and CPI
//! sinks in parsed production Rust. Every reference must have a reviewed owner;
//! dispatch must forward the signed fields unchanged, and admission statements must
//! execute unconditionally before storage, portfolio refresh or matcher CPI.
//! This is a source contract, not a general Rust control-flow proof or new LoF finding.

use std::collections::BTreeMap;
use syn::{visit::Visit, Block, Expr, ItemFn, Stmt};

const SOURCE: &str = include_str!("../../../src/v16_program.rs");
const GRANT: &str = "handle_set_matcher_config";
const CONSUMER: &str = "matcher_tail_start_or_verify_lp_config";
const SINGLE: &str = "handle_trade_cpi";
const BATCH: &str = "handle_batch_trade_cpi";
const SENSITIVE: &[&str] = &[
    GRANT,
    CONSUMER,
    SINGLE,
    BATCH,
    "write_portfolio_matcher_config",
    "invoke_matcher",
    "invoke_matcher_batch",
];

// This prefix ends at the first potentially mutating storage helper. In particular,
// the market-wide generation frontier and the portfolio-local episode are separate.
const GRANT_PREFLIGHT: &str = r#"{
    let current_slot = Clock::get()?.slot;
    if !state::matcher_capability_config_is_valid(
        enabled, trade_fee_cap_bps, expiry_slot, current_slot,
    ) {
        return Err(PercolatorError::InvalidInstruction.into());
    }
    let lp_owner = account(accounts, 0)?;
    let market_ai = account(accounts, 1)?;
    let lp_portfolio_ai = account(accounts, 2)?;
    expect_signer(lp_owner)?;
    expect_writable(lp_portfolio_ai)?;
    expect_owner(market_ai, program_id)?;
    expect_owner(lp_portfolio_ai, program_id)?;
    let (header, owner) =
        state::read_portfolio_owner_preflight(&lp_portfolio_ai.try_borrow_data()?)?;
    if header.market_group_id != market_ai.key.to_bytes()
        || header.portfolio_account_id != lp_portfolio_ai.key.to_bytes()
        || owner != lp_owner.key.to_bytes()
    {
        return Err(PercolatorError::Unauthorized.into());
    }
    {
        let data = market_ai.try_borrow_data()?;
        let (_, next_market_id) =
            state::read_asset_lifecycle_generation_preflight(&data, 0, true)?;
        if next_market_id != asset_generation_frontier {
            return Err(PercolatorError::EngineStale.into());
        }
    }
    let (current_portfolio_id, current_sequence, current_position_epoch) = {
        let data = lp_portfolio_ai.try_borrow_data()?;
        (
            state::read_portfolio_id(&data)?,
            state::read_portfolio_matcher_sequence(&data)?,
            state::read_portfolio_position_epoch(&data)?,
        )
    };
    if portfolio_id != current_portfolio_id
        || expected_sequence != current_sequence
        || position_epoch != current_position_epoch
    {
        return Err(PercolatorError::EngineStale.into());
    }
    state::next_portfolio_matcher_sequence(current_sequence, expected_sequence)?;
}"#;

const GRANT_COMMIT: &str = r#"{
    cfg.set_enabled(enabled)?;
    cfg.set_position_epoch(current_position_epoch)?;
    cfg.set_trade_fee_cap_bps(trade_fee_cap_bps)?;
    let mut data = lp_portfolio_ai.try_borrow_mut_data()?;
    state::write_portfolio_matcher_config(&mut data, &cfg)?;
    state::write_portfolio_matcher_expiry(&mut data, expiry_slot)?;
    state::advance_portfolio_matcher_sequence(&mut data, expected_sequence)?;
    Ok(())
}"#;

const CAPABILITY_CHECK: &str = r#"{
    let account_b_data = account_b_ai.try_borrow_data()?;
    if state::read_portfolio_matcher_sequence(&account_b_data)? != expected_matcher_sequence {
        return Err(PercolatorError::EngineStale.into());
    }
    let cfg = state::read_portfolio_matcher_config(&account_b_data)?;
    if !cfg.authorizes_matcher_tuple(
        &matcher_prog_key.to_bytes(), &matcher_ctx_key.to_bytes(),
        &matcher_delegate_key.to_bytes(),
    ) {
        return Err(PercolatorError::Unauthorized.into());
    }
    let expiry_slot = state::read_portfolio_matcher_expiry(&account_b_data)?;
    if !state::matcher_capability_is_live(expiry_slot, Clock::get()?.slot) {
        return Err(PercolatorError::Unauthorized.into());
    }
    Ok((7, cfg.trade_fee_cap_bps()))
}"#;

#[derive(Default)]
struct Production {
    scope: Vec<String>,
    owner: String,
    functions: BTreeMap<String, ItemFn>,
    references: BTreeMap<(String, String), usize>,
    opaque_references: Vec<String>,
}

impl<'ast> Visit<'ast> for Production {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if node.attrs.iter().any(|a| {
            a.path().is_ident("cfg")
                && a.parse_args::<syn::Path>()
                    .is_ok_and(|p| p.is_ident("test"))
        }) {
            return;
        }
        self.scope.push(node.ident.to_string());
        syn::visit::visit_item_mod(self, node);
        self.scope.pop();
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let name = format!("{}::{}", self.scope.join("::"), node.sig.ident);
        let previous = std::mem::replace(&mut self.owner, name.clone());
        if self.scope == ["processor"] {
            assert!(self.functions.insert(name, node.clone()).is_none());
        }
        syn::visit::visit_item_fn(self, node);
        self.owner = previous;
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        let name = node.path.segments.last().unwrap().ident.to_string();
        if SENSITIVE.contains(&name.as_str()) {
            *self
                .references
                .entry((self.owner.clone(), name))
                .or_default() += 1;
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        if (self.scope == ["processor"] && *node == syn::parse_str("use super::*;").unwrap())
            || (self.scope == ["risk"]
                && *node == syn::parse_str("pub use percolator::*;").unwrap())
        {
            return;
        }
        // Aliases/globs cannot silently hide references from the reviewed call graph.
        fn inspect(tree: &syn::UseTree, bad: &mut Vec<String>) {
            match tree {
                syn::UseTree::Path(p) => inspect(&p.tree, bad),
                syn::UseTree::Group(g) => g.items.iter().for_each(|t| inspect(t, bad)),
                syn::UseTree::Name(n) if SENSITIVE.contains(&n.ident.to_string().as_str()) => {
                    bad.push(n.ident.to_string());
                }
                syn::UseTree::Rename(n) if SENSITIVE.contains(&n.ident.to_string().as_str()) => {
                    bad.push(n.ident.to_string());
                }
                syn::UseTree::Glob(_) => bad.push("glob import".into()),
                _ => {}
            }
        }
        inspect(&node.tree, &mut self.opaque_references);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        let tokens = node.tokens.to_string();
        for name in SENSITIVE {
            if tokens
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|t| t == *name)
            {
                self.opaque_references
                    .push(format!("macro reference to {name}"));
            }
        }
    }
}

fn check(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn block(source: &str) -> Block {
    syn::parse_str(source).expect("valid reviewed source contract")
}

fn function<'a>(production: &'a Production, name: &str) -> Result<&'a ItemFn, String> {
    let function = production
        .functions
        .get(&format!("processor::{name}"))
        .ok_or_else(|| format!("missing processor::{name}"))?;
    check(
        function.attrs.iter().all(|a| {
            ["doc", "inline", "allow"]
                .iter()
                .any(|n| a.path().is_ident(n))
        }),
        format!("conditional/transformed function {name}"),
    )?;
    Ok(function)
}

fn statement_index(body: &Block, source: &str, owner: &str) -> Result<usize, String> {
    let expected = block(&format!("{{ {source} }}"));
    check(
        expected.stmts.len() == 1,
        "contract must name one statement",
    )?;
    let found: Vec<_> = body
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| (s == &expected.stmts[0]).then_some(i))
        .collect();
    check(
        found.len() == 1,
        format!("{owner}: missing/conditional/duplicate admission: {source}"),
    )?;
    Ok(found[0])
}

#[derive(Default)]
struct Effects {
    found: bool,
    early_success: bool,
}

impl<'ast> Visit<'ast> for Effects {
    fn visit_expr_return(&mut self, node: &'ast syn::ExprReturn) {
        self.early_success |= !matches!(&node.expr, Some(expr)
            if matches!(&**expr, Expr::Call(call)
                if matches!(&*call.func, Expr::Path(path) if path.path.is_ident("Err"))));
        syn::visit::visit_expr_return(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let Expr::Path(path) = &*node.func {
            let name = path.path.segments.last().unwrap().ident.to_string();
            self.found |= [
                "invoke_matcher",
                "invoke_matcher_batch",
                "bump_matcher_req_seq",
                "accrue_zero_move_funding_before_matcher_view",
                "ensure_cpi_trade_portfolios_current_before_matcher",
                "handle_trade_nocpi_zero_copy",
                "handle_batch_execute_zero_copy",
            ]
            .contains(&name.as_str());
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        self.found |= ["try_borrow_mut_data", "try_borrow_mut_lamports", "realloc"]
            .iter()
            .any(|n| node.method == *n);
        syn::visit::visit_expr_method_call(self, node);
    }
}

fn validate(source: &str) -> Result<(), String> {
    let parsed = syn::parse_file(source).map_err(|e| e.to_string())?;
    validate_file(&parsed)
}

fn validate_file(parsed: &syn::File) -> Result<(), String> {
    let mut production = Production::default();
    production.visit_file(parsed);
    check(
        production.opaque_references.is_empty(),
        format!("unreviewed reference: {:?}", production.opaque_references),
    )?;
    let expected = [
        ("process_instruction", GRANT),
        ("process_instruction", SINGLE),
        ("process_instruction", BATCH),
        (GRANT, "write_portfolio_matcher_config"),
        (SINGLE, CONSUMER),
        (BATCH, CONSUMER),
        (SINGLE, "invoke_matcher"),
        (BATCH, "invoke_matcher_batch"),
    ]
    .into_iter()
    .map(|(owner, target)| ((format!("processor::{owner}"), target.into()), 1))
    .collect();
    check(
        production.references == expected,
        format!(
            "unreviewed retained matcher reference roster: {:?}",
            production.references
        ),
    )?;

    let dispatch = function(&production, "process_instruction")?;
    let [Stmt::Expr(Expr::Match(dispatch), None)] = dispatch.block.stmts.as_slice() else {
        return Err("public dispatcher is no longer one returned match".into());
    };
    for (variant, handler, fields, parameters) in [
        ("SetMatcherConfig", GRANT,
         "portfolio_id, expected_sequence, position_epoch, asset_generation_frontier, enabled, trade_fee_cap_bps, expiry_slot",
         "portfolio_id, expected_sequence, position_epoch, asset_generation_frontier, enabled, trade_fee_cap_bps, expiry_slot"),
        ("TradeCpi", SINGLE,
         "account_a_portfolio_id, account_a_position_epoch, account_b_portfolio_id, account_b_position_epoch, account_b_matcher_sequence, asset_index, market_id, size_q, fee_bps, limit_price, backing_fee_cap_bps",
         "account_a_portfolio_id, account_a_position_epoch, account_b_portfolio_id, account_b_position_epoch, account_b_matcher_sequence, asset_index, expected_market_id, size_q, fee_bps, limit_price, backing_fee_cap_bps"),
        ("BatchTradeCpi", BATCH,
         "account_a_portfolio_id, account_a_position_epoch, account_b_portfolio_id, account_b_position_epoch, account_b_matcher_sequence, max_slippage_atoms, max_fee_atoms, legs",
         "account_a_portfolio_id, account_a_position_epoch, account_b_portfolio_id, account_b_position_epoch, account_b_matcher_sequence, max_slippage_atoms, max_fee_atoms, legs"),
    ] {
        let args = if handler == BATCH { fields.replace(", legs", ", &legs") } else { fields.into() };
        let reviewed: syn::ExprMatch = syn::parse_str(&format!(
            "match instruction {{ Instruction::{variant} {{ {fields}, }} => {handler}(program_id, accounts, {args},), }}"
        )).unwrap();
        check(dispatch.arms.iter().filter(|arm| *arm == &reviewed.arms[0]).count() == 1,
            format!("{variant}: signed dispatch arguments changed"))?;
        let actual: Vec<_> = function(&production, handler)?.sig.inputs.iter().map(|arg| {
            match arg {
                syn::FnArg::Typed(arg) => match &*arg.pat {
                    syn::Pat::Ident(p) => p.ident.to_string(),
                    _ => "unreviewed pattern".into(),
                },
                _ => "unreviewed receiver".into(),
            }
        }).collect();
        let expected: Vec<_> = format!("program_id, accounts, {parameters}")
            .split(", ").map(str::to_owned).collect();
        check(actual == expected, format!("{handler}: signed parameter order changed"))?;
    }

    let grant = &function(&production, GRANT)?.block;
    let prefix = block(GRANT_PREFLIGHT);
    check(
        grant.stmts.starts_with(&prefix.stmts),
        "grant: executable identity/consent preflight changed",
    )?;
    let storage = statement_index(
        grant,
        "ensure_portfolio_storage_for_market_slots(lp_portfolio_ai, 0)?;",
        GRANT,
    )?;
    check(
        storage == prefix.stmts.len(),
        "grant: effect before complete preflight",
    )?;
    check(
        grant.stmts.ends_with(&block(GRANT_COMMIT).stmts),
        "grant: atomic consent commit changed",
    )?;
    check(
        *function(&production, CONSUMER)?.block == block(CAPABILITY_CHECK),
        "consumer: incarnation/tuple/expiry/fee-cap contract changed",
    )?;

    for route in [SINGLE, BATCH] {
        let body = &function(&production, route)?.block;
        let first_effect = body
            .stmts
            .iter()
            .position(|stmt| {
                let mut effects = Effects::default();
                effects.visit_stmt(stmt);
                effects.found
            })
            .ok_or_else(|| format!("{route}: no reviewed economic effect"))?;
        let mut prefix = Effects::default();
        for stmt in &body.stmts[..first_effect] {
            prefix.visit_stmt(stmt);
        }
        check(
            !prefix.early_success,
            format!("{route}: early success before admission"),
        )?;
        for side in ["a", "b"] {
            let binding = statement_index(body, &format!(
                "expect_portfolio_position_binding(&account_{side}_ai.try_borrow_data()?, account_{side}_portfolio_id, account_{side}_position_epoch,)?;"
            ), route)?;
            check(
                binding < first_effect,
                format!("{route}: episode check after effect"),
            )?;
        }
        let consume = statement_index(
            body,
            r#"
            let (tail_start, lp_trade_fee_cap_bps) = matcher_tail_start_or_verify_lp_config(
                account_b_ai, account_b_matcher_sequence, matcher_prog.key,
                matcher_ctx.key, matcher_delegate.key,
            )?;
        "#,
            route,
        )?;
        let fee = if route == SINGLE {
            "cfg_pre.trade_fee_base_bps"
        } else {
            "cpi_fee_bps"
        };
        let fee_check = statement_index(body, &format!(
            "if {fee} > u64::from(lp_trade_fee_cap_bps) {{ return Err(PercolatorError::InvalidInstruction.into()); }}"
        ), route)?;
        check(
            consume < fee_check && fee_check < first_effect,
            format!("{route}: capability/LP consent must precede effects"),
        )?;
        if route == SINGLE {
            let taker = statement_index(
                body,
                r#"
                if cfg_pre.trade_fee_base_bps > fee_bps {
                    return Err(PercolatorError::InvalidInstruction.into());
                }
            "#,
                route,
            )?;
            check(
                taker < first_effect,
                "single CPI: taker consent after effect",
            )?;
        }
    }
    Ok(())
}

#[test]
fn v16_retained_matcher_admission_is_source_complete_before_effects() {
    validate(SOURCE).unwrap();
}

fn replace_once(source: &str, old: &str, new: &str) -> String {
    assert_eq!(
        source.matches(old).count(),
        1,
        "unique mutation target: {old}"
    );
    source.replacen(old, new, 1)
}

fn processor_items(file: &mut syn::File) -> &mut Vec<syn::Item> {
    file.items
        .iter_mut()
        .find_map(|item| match item {
            syn::Item::Mod(module) if module.ident == "processor" => {
                Some(&mut module.content.as_mut().unwrap().1)
            }
            _ => None,
        })
        .unwrap()
}

fn mutate_function(name: &str, mutate: impl FnOnce(&mut ItemFn)) -> syn::File {
    let mut parsed = syn::parse_file(SOURCE).unwrap();
    let function = processor_items(&mut parsed)
        .iter_mut()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == name => Some(f),
            _ => None,
        })
        .unwrap();
    mutate(function);
    parsed
}

#[test]
fn v16_retained_matcher_admission_guard_rejects_bypasses() {
    validate(SOURCE).unwrap();
    let mutations = [
        ("generation block skipped", "        {\n            let data = market_ai.try_borrow_data()?;\n            let (_, next_market_id) =\n                state::read_asset_lifecycle_generation_preflight(&data, 0, true)?;", "        if false {\n            let data = market_ai.try_borrow_data()?;\n            let (_, next_market_id) =\n                state::read_asset_lifecycle_generation_preflight(&data, 0, true)?;"),
        ("episode condition", "|| position_epoch != current_position_epoch", "|| false"),
        ("generation condition", "if next_market_id != asset_generation_frontier {", "if false && next_market_id != asset_generation_frontier {"),
        ("generation error", "if next_market_id != asset_generation_frontier {\n                return Err(PercolatorError::EngineStale.into());", "if next_market_id != asset_generation_frontier {\n                return Ok(());"),
        ("grant epoch reset", "cfg.set_position_epoch(current_position_epoch)?;", "cfg.set_position_epoch(0)?;"),
        ("grant sequence consumption", "state::advance_portfolio_matcher_sequence(&mut data, expected_sequence)?;", "let _ = state::advance_portfolio_matcher_sequence(&mut data, expected_sequence);"),
        ("consumer epoch bypass", "if state::read_portfolio_matcher_sequence(&account_b_data)? != expected_matcher_sequence {", "if false && state::read_portfolio_matcher_sequence(&account_b_data)? != expected_matcher_sequence {"),
        ("consumer expiry bypass", "if !state::matcher_capability_is_live(expiry_slot, Clock::get()?.slot) {", "if false && !state::matcher_capability_is_live(expiry_slot, Clock::get()?.slot) {"),
        ("consumer cap widened", "Ok((7, cfg.trade_fee_cap_bps()))", "Ok((7, 10_000))"),
        ("taker cap disabled", "// The taker signs fee_bps independently of the LP's matcher capability cap.\n        if cfg_pre.trade_fee_base_bps > fee_bps {", "if false && cfg_pre.trade_fee_base_bps > fee_bps {"),
        ("single LP cap disabled", "if cfg_pre.trade_fee_base_bps > u64::from(lp_trade_fee_cap_bps) {", "if false && cfg_pre.trade_fee_base_bps > u64::from(lp_trade_fee_cap_bps) {"),
        ("batch LP cap disabled", "if cpi_fee_bps > u64::from(lp_trade_fee_cap_bps) {", "if false && cpi_fee_bps > u64::from(lp_trade_fee_cap_bps) {"),
        ("conditional grant handler", "fn handle_set_matcher_config<'a>(", "#[cfg(any())]\n    fn handle_set_matcher_config<'a>("),
    ];
    for (label, old, new) in mutations {
        let mutant = replace_once(SOURCE, old, new);
        syn::parse_file(&mutant).expect("negative control remains valid Rust syntax");
        assert!(validate(&mutant).is_err(), "accepted {label}");
    }
    let mut count = mutations.len();
    for route in [SINGLE, BATCH] {
        let fee = if route == SINGLE {
            "cfg_pre.trade_fee_base_bps"
        } else {
            "cpi_fee_bps"
        };
        let guard = format!("if {fee} > u64::from(lp_trade_fee_cap_bps) {{ return Err(PercolatorError::InvalidInstruction.into()); }}");
        let late = mutate_function(route, |function| {
            let index = statement_index(&function.block, &guard, route).unwrap();
            let statement = function.block.stmts.remove(index);
            let after_effect = function
                .block
                .stmts
                .iter()
                .position(|stmt| {
                    let mut effects = Effects::default();
                    effects.visit_stmt(stmt);
                    effects.found
                })
                .unwrap()
                + 1;
            function.block.stmts.insert(after_effect, statement);
        });
        assert!(validate_file(&late)
            .unwrap_err()
            .contains("consent must precede effects"));
        let early = mutate_function(route, |function| {
            function
                .block
                .stmts
                .insert(0, syn::parse_str("return Ok(());").unwrap());
        });
        assert!(validate_file(&early).unwrap_err().contains("early success"));
        let swapped = mutate_function(route, |function| {
            let argument = function.sig.inputs[2].clone();
            function.sig.inputs[2] = function.sig.inputs[4].clone();
            function.sig.inputs[4] = argument;
        });
        assert!(validate_file(&swapped)
            .unwrap_err()
            .contains("parameter order"));
        count += 3;
    }
    for variant in ["SetMatcherConfig", "TradeCpi", "BatchTradeCpi"] {
        let forwarded = mutate_function("process_instruction", |function| {
            let Stmt::Expr(Expr::Match(dispatch), _) = &mut function.block.stmts[0] else {
                unreachable!()
            };
            let arm = dispatch.arms.iter_mut().find(|arm| {
                matches!(&arm.pat, syn::Pat::Struct(pat) if pat.path.segments.last().unwrap().ident == variant)
            }).unwrap();
            let Expr::Call(call) = &mut *arm.body else {
                unreachable!()
            };
            call.args[4] = syn::parse_str("0").unwrap();
        });
        assert!(validate_file(&forwarded)
            .unwrap_err()
            .contains("signed dispatch"));
        count += 1;
    }
    for added in [
        "fn unchecked_grant(data: &mut [u8], cfg: &state::PortfolioMatcherConfigV16) { let _ = state::write_portfolio_matcher_config(data, cfg); }",
        "fn hidden_consumer() { let callable = matcher_tail_start_or_verify_lp_config; }",
        "use self::matcher_tail_start_or_verify_lp_config as unchecked_consumer;",
        "fn hidden_in_macro() { opaque!(matcher_tail_start_or_verify_lp_config); }",
    ] {
        let mut file = syn::parse_file(SOURCE).unwrap();
        processor_items(&mut file).push(syn::parse_str(added).unwrap());
        assert!(validate_file(&file).unwrap_err().contains("reference"));
        count += 1;
    }
    // Comments and an unrelated production function are not obligations.
    validate(&format!(
        "// source contract positive control\n{SOURCE}\nfn unrelated_control() {{}}\n"
    ))
    .unwrap();
    eprintln!("retained matcher admission: {count} rejected mutations, 2 accepted controls");
}
