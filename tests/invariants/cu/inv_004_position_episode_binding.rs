//! INV-004 - Position episode binding.
//!
//! Normative obligation: retained close, cure, conversion, reduction, and recovery-forfeit consent binds
//! the exact economic position/recovery episode that existed when the owner
//! signed. Closing or forfeiting an old episode and opening a replacement at
//! the same portfolio/asset must not let the old signed request touch the new
//! exposure.
//!
//! Evidence in this file (I/F plus a production-source roster): this deterministic LiteSVM wrapper witness runs
//! the shared public-route position-episode matrix. For rebalance-reduce and
//! recovery-forfeit, it requires stale retained consent to reject with exact
//! market, portfolio, vault, and SPL-supply rollback; it also requires freshly
//! signed current consent to land and change exposure so the guard is not a
//! blanket risk-reduction DoS.
//!
//! The source roster requires all five single-account and four paired-trade episode-bound
//! instruction families to encode and dispatch their position epochs, consume the shared exact
//! binding predicate before mutation, and advance the epoch after success. It also owns every
//! wrapper callsite that can change a portfolio's position vector: single/batch trades,
//! force-close, auto-crank, and the two shared owner routes. A new field or bump callsite reopens
//! this invariant until classified.

use std::fs;

#[test]
fn v16_program_position_episode_matrix_rejects_stale_consent_fixed_case() {
    let discoveries =
        crate::support::invariant_discovery::discover_position_episode_replays([0x04; 32])
            .expect("position-episode replay discovery");
    assert_eq!(
        discoveries.len(),
        crate::support::invariant_discovery::PositionEpisodeKind::ALL.len()
    );
    for discovery in discoveries {
        assert!(
            discovery.satisfies_invariant(),
            "position-episode binding failed: {discovery:?}"
        );
    }
}

fn function_source<'a>(source: &'a str, name: &str, next_name: &str) -> &'a str {
    let start = source
        .find(&format!("fn {name}"))
        .unwrap_or_else(|| panic!("missing production function {name}"));
    let rest = &source[start..];
    let end = rest.find(&format!("fn {next_name}")).unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn v16_program_retained_position_binding_and_writer_rosters_are_source_complete() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/v16_program.rs"))
        .expect("read deployed wrapper source");
    let production_end = source
        .rfind("    #[cfg(test)]\n    mod tests")
        .expect("production/test boundary");
    let production_source = &source[..production_end];
    let enum_start = source
        .find("pub enum Instruction")
        .expect("instruction enum");
    let enum_end = source[enum_start..]
        .find("impl Instruction")
        .map(|offset| enum_start + offset)
        .expect("instruction enum end");
    let instruction_enum = &source[enum_start..enum_end];
    assert_eq!(
        instruction_enum.matches("position_epoch: u64").count(),
        13,
        "retained position-episode field roster changed without INV-004 review"
    );
    for variant in [
        "ClosePortfolio",
        "ConvertReleasedPnl",
        "CureAndCancelClose",
        "ForfeitRecoveryLeg",
        "RebalanceReduce",
    ] {
        let marker = format!("{variant} {{");
        let start = instruction_enum
            .find(&marker)
            .unwrap_or_else(|| panic!("missing retained episode route {variant}"));
        let body = &instruction_enum[start..];
        let end = body.find("},").expect("variant terminator") + 2;
        assert!(
            body[..end].contains("position_epoch: u64"),
            "{variant} lost its signed episode binding"
        );
    }
    for variant in ["TradeNoCpi", "TradeCpi", "BatchTradeNoCpi", "BatchTradeCpi"] {
        let marker = format!("{variant} {{");
        let start = instruction_enum
            .find(&marker)
            .unwrap_or_else(|| panic!("missing paired episode route {variant}"));
        let body = &instruction_enum[start..];
        let end = body.find("},").expect("variant terminator") + 2;
        assert!(
            body[..end].contains("account_a_position_epoch: u64")
                && body[..end].contains("account_b_position_epoch: u64"),
            "{variant} must bind both counterparties' episodes"
        );
    }

    for name in [
        "handle_trade_nocpi_zero_copy",
        "handle_batch_execute_zero_copy",
        "handle_trade_cpi",
        "handle_batch_trade_cpi",
    ] {
        let start = source
            .find(&format!("fn {name}"))
            .unwrap_or_else(|| panic!("missing paired trade handler {name}"));
        let tail = &source[start..];
        let end = tail.find("\n    fn ").unwrap_or(tail.len());
        let handler = &tail[..end];
        assert!(
            handler
                .matches("expect_portfolio_position_binding(")
                .count()
                >= 2,
            "{name} must consume both exact episode bindings before mutation or CPI"
        );
    }

    let convert = function_source(
        &source,
        "handle_convert_released_pnl",
        "handle_cure_and_cancel_close",
    );
    assert!(convert.contains("Some((expected_portfolio_id, expected_position_epoch))"));
    let cure = function_source(
        &source,
        "handle_cure_and_cancel_close",
        "handle_forfeit_recovery_leg",
    );
    assert!(cure.contains("state::portfolio_position_binding_matches("));
    assert!(cure.contains("state::bump_portfolio_position_epoch(&mut portfolio_data)?;"));
    for (name, next) in [
        ("handle_forfeit_recovery_leg", "handle_rebalance_reduce"),
        ("handle_rebalance_reduce", "handle_sync_maintenance_fee"),
    ] {
        let handler = function_source(&source, name, next);
        assert!(handler.contains("Some((expected_portfolio_id, expected_position_epoch))"));
    }
    let close = function_source(&source, "handle_close_portfolio", "handle_top_up_insurance");
    assert!(close.contains("state::portfolio_close_binding_matches("));

    assert_eq!(
        production_source
            .matches("state::bump_portfolio_position_epoch(")
            .count(),
        11,
        "position-epoch writer roster changed without INV-004 review"
    );
    assert_eq!(
        production_source
            .matches("state::bump_portfolio_position_epoch_after_matcher_fill(")
            .count(),
        2,
        "matcher-synchronized episode writer roster changed without INV-004 review"
    );
    let crank = function_source(&source, "handle_permissionless_crank_zero_copy", "account");
    assert!(crank.contains("let positions_before = portfolio_position_vector_view(&portfolio);"));
    assert!(crank.contains("if position_changed"));
}
