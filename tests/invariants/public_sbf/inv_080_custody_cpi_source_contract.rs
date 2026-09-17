//! INV-018/021/080: custody CPI results must reach the public handler unchanged.
//!
//! Existing INV-016/018 rosters bind custody validators and token-moving handlers;
//! INV-080's engine-result and dispatch guards do not inspect SPL-result consumers.
//! Public rollback witnesses sample reachable failures (notably Deposit and backing
//! funding). None enforces error propagation at every transfer/burn/close callsite.
//! This parsed-source contract fills that composition gap, including zero-amount
//! helpers and both vault-close CPIs before the slab's realloc and rent refund.
//!
//! This is a contract for the current Rust source shape, not general control-flow
//! analysis or a new runtime rollback proof. Solana/SPL remain platform assumptions;
//! account admission, entitlement, matcher execution and dispatch have other owners.

use std::collections::BTreeMap;
use syn::{visit::Visit, Block, Expr, ItemFn, Stmt};

const SOURCE: &str = include_str!("../../../src/v16_program.rs");
const HELPERS: &[&str] = &[
    "transfer_tokens",
    "transfer_tokens_signed",
    "burn_tokens_signed",
];

type Calls = BTreeMap<(String, String), usize>;

#[derive(Default)]
struct Custody {
    scope: Vec<String>,
    owner: String,
    nested_result: usize,
    references: Calls,
    propagated: Calls,
    invocations: Calls,
    functions: BTreeMap<String, ItemFn>,
    opaque: Vec<String>,
}

fn target(path: &syn::Path) -> Option<String> {
    let name = path.segments.last()?.ident.to_string();
    if HELPERS.contains(&name.as_str()) {
        return Some(name);
    }
    let names: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    (names.len() == 3 && names[0] == "spl_token" && names[1] == "instruction")
        .then(|| names.join("::"))
}

impl Custody {
    fn record(map: &mut Calls, owner: &str, target: String) {
        *map.entry((owner.into(), target)).or_default() += 1;
    }
}

impl<'ast> Visit<'ast> for Custody {
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
        if self.scope == ["processor"]
            && self.functions.insert(name.clone(), node.clone()).is_some()
        {
            self.opaque.push(format!("duplicate function {name}"));
        }
        if let Some(Stmt::Expr(Expr::Call(call), None)) = node.block.stmts.last() {
            if let Expr::Path(path) = &*call.func {
                if let Some(name) = target(&path.path) {
                    Self::record(&mut self.propagated, &self.owner, name);
                }
            }
        }
        syn::visit::visit_item_fn(self, node);
        self.owner = previous;
    }

    fn visit_expr(&mut self, node: &'ast Expr) {
        // A question mark in a locally caught closure/async/try result does not
        // propagate to the public handler. Do not count it as an accepted edge.
        let nested = matches!(node, Expr::Closure(_) | Expr::Async(_) | Expr::TryBlock(_));
        self.nested_result += usize::from(nested);
        if self.nested_result == 0 {
            if let Expr::Try(question) = node {
                if let Expr::Call(call) = &*question.expr {
                    if let Expr::Path(path) = &*call.func {
                        if let Some(name) = target(&path.path) {
                            Self::record(&mut self.propagated, &self.owner, name);
                        }
                    }
                }
            }
        }
        syn::visit::visit_expr(self, node);
        self.nested_result -= usize::from(nested);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if let Some(name) = target(&node.path) {
            Self::record(&mut self.references, &self.owner, name);
        }
        let name = node.path.segments.last().unwrap().ident.to_string();
        let owner = self.owner.strip_prefix("processor::").unwrap_or("");
        if (HELPERS.contains(&owner) || owner == "handle_close_slab")
            && ["invoke", "invoke_signed"].contains(&name.as_str())
        {
            Self::record(&mut self.invocations, &self.owner, name);
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        fn inspect(tree: &syn::UseTree, opaque: &mut Vec<String>) {
            match tree {
                syn::UseTree::Path(p) if p.ident == "spl_token" => {
                    opaque.push("imported SPL instruction alias".into());
                }
                syn::UseTree::Path(p) => inspect(&p.tree, opaque),
                syn::UseTree::Group(g) => g.items.iter().for_each(|t| inspect(t, opaque)),
                syn::UseTree::Name(n) if HELPERS.contains(&n.ident.to_string().as_str()) => {
                    opaque.push(n.ident.to_string());
                }
                syn::UseTree::Rename(n) if HELPERS.contains(&n.ident.to_string().as_str()) => {
                    opaque.push(n.ident.to_string());
                }
                _ => {}
            }
        }
        inspect(&node.tree, &mut self.opaque);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        // Macro token streams are opaque to syn's expression visitor. Fail closed
        // on custody references instead of letting a macro hide a result consumer.
        let tokens = node.tokens.to_string();
        if tokens
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|s| {
                HELPERS.contains(&s) || s == "spl_token" || s == "invoke" || s == "invoke_signed"
            })
        {
            self.opaque.push(format!("custody macro in {}", self.owner));
        }
    }
}

fn check(condition: bool, message: impl Into<String>) -> Result<(), String> {
    condition.then_some(()).ok_or_else(|| message.into())
}

fn block(source: &str) -> Block {
    syn::parse_str(source).expect("reviewed source contract parses")
}

fn validate(source: &str) -> Result<usize, String> {
    let parsed = syn::parse_file(source).map_err(|e| e.to_string())?;
    let mut custody = Custody::default();
    custody.visit_file(&parsed);
    check(
        custody.opaque.is_empty(),
        format!("opaque custody: {:?}", custody.opaque),
    )?;

    let rows = [
        ("handle_deposit", "transfer_tokens", 1),
        ("handle_top_up_insurance", "transfer_tokens", 1),
        ("handle_top_up_backing_bucket", "transfer_tokens", 1),
        ("handle_cure_and_cancel_close", "transfer_tokens", 1),
        ("handle_swap_secondary_for_primary", "transfer_tokens", 1),
        ("handle_update_asset_lifecycle", "transfer_tokens", 1),
        ("handle_withdraw", "transfer_tokens_signed", 1),
        (
            "handle_withdraw_backing_bucket",
            "transfer_tokens_signed",
            1,
        ),
        (
            "handle_withdraw_backing_bucket_earnings",
            "transfer_tokens_signed",
            1,
        ),
        (
            "handle_withdraw_insurance_asset",
            "transfer_tokens_signed",
            1,
        ),
        ("handle_close_slab", "transfer_tokens_signed", 3),
        (
            "handle_swap_secondary_for_primary",
            "transfer_tokens_signed",
            1,
        ),
        ("handle_close_resolved", "transfer_tokens_signed", 1),
        (
            "handle_claim_resolved_payout_topup",
            "transfer_tokens_signed",
            1,
        ),
        ("handle_close_slab", "burn_tokens_signed", 1),
        ("transfer_tokens", "spl_token::instruction::transfer", 1),
        (
            "transfer_tokens_signed",
            "spl_token::instruction::transfer",
            1,
        ),
        ("burn_tokens_signed", "spl_token::instruction::burn", 1),
        (
            "handle_close_slab",
            "spl_token::instruction::close_account",
            2,
        ),
    ];
    let expected: Calls = rows
        .into_iter()
        .map(|(owner, callee, count)| ((format!("processor::{owner}"), callee.into()), count))
        .collect();
    check(
        custody.references == expected,
        format!("custody callsite roster changed: {:?}", custody.references),
    )?;
    check(
        custody.propagated == custody.references,
        format!(
            "every custody result must propagate: {:?}",
            custody.propagated
        ),
    )?;
    let expected_invocations = [
        ("transfer_tokens", "invoke", 1),
        ("transfer_tokens_signed", "invoke_signed", 1),
        ("burn_tokens_signed", "invoke_signed", 1),
        ("handle_close_slab", "invoke_signed", 2),
    ]
    .into_iter()
    .map(|(owner, callee, count)| ((format!("processor::{owner}"), callee.into()), count))
    .collect();
    check(
        custody.invocations == expected_invocations,
        "unreviewed custody CPI invocation/alias",
    )?;

    for (name, operation, destination, invocation, seeds) in [
        ("transfer_tokens", "transfer", "dest", "invoke", ""),
        (
            "transfer_tokens_signed",
            "transfer",
            "dest",
            "invoke_signed",
            ", signer_seeds",
        ),
        (
            "burn_tokens_signed",
            "burn",
            "mint",
            "invoke_signed",
            ", signer_seeds",
        ),
    ] {
        let expected = block(&format!(
            r#"{{
            if amount == 0 {{ return Ok(()); }}
            let ix = spl_token::instruction::{operation}(
                token_program.key, source.key, {destination}.key, authority.key, &[], amount,
            )?;
            {invocation}(&ix, &[
                source.clone(), {destination}.clone(), authority.clone(), token_program.clone(),
            ]{seeds},)
        }}"#
        ));
        let function = &custody.functions[&format!("processor::{name}")];
        check(
            *function.block == expected,
            format!("{name}: constructor/account binding or exact CPI result changed"),
        )?;
    }

    // CloseSlab bypasses the shared helpers for vault closure. Require each exact
    // constructor/invocation pair in its real block, preceding realloc/refunds.
    let close = &custody.functions["processor::handle_close_slab"].block;
    let primary = close_pair("close_ix", "vault_token");
    let secondary = close_pair("close_secondary_ix", "secondary_vault_token");
    let primary_index = close
        .stmts
        .windows(2)
        .position(|w| w == primary.stmts)
        .ok_or("missing primary close CPI binding/propagation")?;
    let secondary_branch = match close.stmts.get(primary_index + 2) {
        Some(Stmt::Expr(Expr::If(branch), None)) => branch,
        _ => return Err("secondary close must follow primary close".into()),
    };
    let secondary_condition: Expr = syn::parse_str(
        "if let Some((secondary_vault_token, secondary_dest_token, secondary_amount)) = secondary_close {}"
    ).unwrap();
    let Expr::If(expected_branch) = secondary_condition else {
        unreachable!()
    };
    check(
        secondary_branch.cond == expected_branch.cond && secondary_branch.else_branch.is_none(),
        "secondary close must consume the validated optional custody tuple",
    )?;
    check(
        secondary_branch
            .then_branch
            .stmts
            .ends_with(&secondary.stmts),
        "missing secondary close CPI binding/propagation",
    )?;
    let realloc = block("{ market_ai.realloc(constants::HEADER_LEN, false)?; }");
    check(
        close.stmts.get(primary_index + 3) == realloc.stmts.first(),
        "slab realloc/refund must follow both propagated closes",
    )?;
    Ok(expected.values().sum())
}

fn close_pair(instruction: &str, vault: &str) -> Block {
    block(&format!(
        r#"{{
        let {instruction} = spl_token::instruction::close_account(
            token_program.key, {vault}.key, admin_dest.key, vault_authority_ai.key, &[],
        )?;
        invoke_signed(&{instruction}, &[
            {vault}.clone(), admin_dest.clone(), vault_authority_ai.clone(), token_program.clone(),
        ], signer_seeds,)?;
    }}"#
    ))
}

#[test]
fn v16_custody_cpi_results_and_account_bindings_are_source_complete() {
    let sites = validate(SOURCE).expect("all current custody CPI errors propagate unchanged");
    assert_eq!(sites, 22);
    println!("INV-080 custody source contract: {sites} result edges, 3 CPI helpers, 2 direct vault closes");
}

#[test]
fn v16_custody_cpi_contract_rejects_result_and_account_binding_mutations() {
    validate(SOURCE).expect("the unmodified source must pass before mutation controls");
    let deposit = "transfer_tokens(token_program, source_token, vault_token, owner, amount_u64)?;";
    let direct = deposit.trim_end_matches("?;");
    let mut mutations = vec![
        ("discard result", deposit.into(), format!("let _ = {direct};")),
        ("optional result", deposit.into(), format!("{direct}.ok();")),
        ("remap error", deposit.into(), format!("{direct}.map_err(|_| ProgramError::InvalidArgument)?;")),
        ("local catch", deposit.into(), format!("let _ = (|| -> ProgramResult {{ {deposit} Ok(()) }})();")),
        ("alias result", deposit.into(), "let send = transfer_tokens; send(token_program, source_token, vault_token, owner, amount_u64)?;".into()),
        ("macro result", deposit.into(), format!("discard_custody!({deposit});")),
        ("new caller", deposit.into(), format!("{deposit} {deposit}")),
    ];
    for (label, old, new) in [
        (
            "unsigned authority",
            "authority.clone(),\n                token_program.clone(),",
            "source.clone(),\n                token_program.clone(),",
        ),
        (
            "close refund key",
            "admin_dest.key,\n            vault_authority_ai.key,",
            "vault_token.key,\n            vault_authority_ai.key,",
        ),
        (
            "secondary close refund key",
            "admin_dest.key,\n                vault_authority_ai.key,",
            "secondary_vault_token.key,\n                vault_authority_ai.key,",
        ),
        (
            "close CPI instruction",
            "&close_ix,",
            "&close_secondary_ix,",
        ),
        (
            "secondary CPI instruction",
            "&close_secondary_ix,",
            "&close_ix,",
        ),
        (
            "close signer seeds",
            "            signer_seeds,\n        )?;\n\n        if let Some((secondary_vault_token",
            "            &[],\n        )?;\n\n        if let Some((secondary_vault_token",
        ),
        (
            "discard primary close error",
            "            signer_seeds,\n        )?;\n\n        if let Some((secondary_vault_token",
            "            signer_seeds,\n        ).ok();\n\n        if let Some((secondary_vault_token",
        ),
        (
            "discard secondary close error",
            "                signer_seeds,\n            )?;\n        }\n\n        market_ai.realloc",
            "                signer_seeds,\n            ).ok();\n        }\n\n        market_ai.realloc",
        ),
        (
            "skip secondary close",
            "            secondary_close\n        {",
            "            None::<(&AccountInfo, &AccountInfo, u64)>\n        {",
        ),
        (
            "duplicate close invocation",
            "        if let Some((secondary_vault_token, secondary_dest_token, secondary_amount)) =",
            "        invoke_signed(&close_ix, &[], signer_seeds)?;\n        if let Some((secondary_vault_token, secondary_dest_token, secondary_amount)) =",
        ),
    ] {
        mutations.push((label, old.into(), new.into()));
    }
    // Check all three CPI helper tails and both public handlers which return a
    // signed transfer directly instead of applying a question mark.
    for helper in HELPERS
        .iter()
        .copied()
        .chain(["handle_withdraw", "handle_swap_secondary_for_primary"])
    {
        let start = SOURCE.find(&format!("fn {helper}<'a>(")).unwrap();
        let body = &SOURCE[start..SOURCE[start..].find("\n    }\n").unwrap() + start + 7];
        let old = body.to_string();
        let end = old.rfind("\n        )").unwrap();
        let mut new = old.clone();
        new.insert_str(
            end + "\n        )".len(),
            ".map_err(|_| ProgramError::InvalidArgument)",
        );
        mutations.push((helper, old, new));
    }
    for (label, old, new) in &mutations {
        assert!(SOURCE.contains(old), "mutation anchor: {label}");
        let changed = SOURCE.replacen(old, new, 1);
        assert!(
            syn::parse_file(&changed).is_ok(),
            "valid Rust mutation: {label}"
        );
        assert!(
            validate(&changed).is_err(),
            "undetected custody mutation: {label}"
        );
    }
    assert!(validate(SOURCE).is_ok());
    assert!(validate(&format!("// {deposit}\n{SOURCE}")).is_ok());
    assert!(validate(&format!(
        "{SOURCE}\nfn unrelated() {{ let _ = Ok::<(), ()>(()); }}"
    ))
    .is_ok());
    println!(
        "INV-080 custody controls: {} rejected mutations, 3 accepted controls",
        mutations.len()
    );
}
