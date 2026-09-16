//! Scope I / rows 422 and 426: trade-origin fees across report handoff and catchup.
//! An exposed AuthMark recipient keeps its own K value separate from reward stock.

use super::*;

pub(super) fn discover(
    env: &mut V16CuEnv,
    owners: &[Keypair; 5],
    portfolios: [Pubkey; 5],
    tracked: &mut Vec<Pubkey>,
    batch: bool,
    cpi: bool,
) -> u64 {
    // Bid-side CPI quotes and bilateral requests give the same signed positions.
    let a = portfolios[3];
    let b = portfolios[2];
    let size = -(POS_SCALE as i128);
    let mut accounts = vec![AccountMeta::new(owners[3].pubkey(), true)];
    if !cpi {
        accounts.push(AccountMeta::new(owners[2].pubkey(), true));
    }
    accounts.extend([
        AccountMeta::new(env.market, false),
        AccountMeta::new(a, false),
        AccountMeta::new(b, false),
    ]);
    if cpi {
        let (program, context, delegate) =
            auth_matcher_for_lp_via_system_create(env, &owners[2], b);
        let mut data = vec![4];
        data.extend_from_slice(&1_000u64.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            Instruction {
                program_id: program,
                accounts: vec![
                    AccountMeta::new_readonly(owners[2].pubkey(), true),
                    AccountMeta::new(context, false),
                ],
                data,
            },
            &[&owners[2]],
        )
        .unwrap();
        env.set_matcher_config_with_trade_fee_cap(
            program, &owners[2], b, context, delegate, 1, 10_000,
        );
        tracked.extend([program, context, delegate]);
        accounts.extend([
            AccountMeta::new_readonly(program, false),
            AccountMeta::new(context, false),
            AccountMeta::new_readonly(delegate, false),
        ]);
    }
    let data = match (batch, cpi) {
        (false, false) => env.trade_no_cpi_ix(a, b, 0, size, 900_000, 0),
        (false, true) => env.trade_cpi_ix(a, b, 0, size, 0, 900_000),
        (true, false) => env.batch_trade_no_cpi_ix(
            a,
            b,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                size_q: size,
                exec_price: 900_000,
                fee_bps: 0,
            }],
        ),
        (true, true) => env.batch_trade_cpi_ix(
            a,
            b,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: env.asset_market_id(0),
                size_q: size,
                limit_price: 900_000,
                fee_bps: 0,
            }],
        ),
    };
    env.svm.expire_blockhash();
    let signers = if cpi {
        vec![&owners[3]]
    } else {
        vec![&owners[3], &owners[2]]
    };
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: env.program_id,
            accounts,
            data: data.encode(),
        },
        &signers,
    )
    .expect("authorized paid discovery")
}

pub(super) fn refresh(
    env: &V16CuEnv,
    portfolio: Pubkey,
    signer: Pubkey,
    report: Pubkey,
) -> Instruction {
    let mut ix = observe(env, portfolio, signer, Some(report), None);
    let mut observations = crank_observations_with_accounts(0, 1);
    observations.extend(crank_observations(1));
    ix.data = ProgInstruction::PermissionlessCrank {
        now_slot: u64::MAX,
        observations,
    }
    .encode();
    ix
}

#[test]
fn v16_program_paid_origin_routes_preserve_old_penalty_and_block_trade_origin_rewards() {
    let mut reference = None;
    let mut peak = 0;
    let mut worlds = 0;
    for cpi in [false, true] {
        for batch in [false, true] {
            for publish_first in [false, true] {
                let (values, budgets, fees, cu) =
                    run_retained_handoff(Some((batch, cpi, publish_first)));
                let outcome = (values, budgets, fees);
                if let Some(expected) = &reference {
                    assert_eq!(&outcome, expected);
                } else {
                    reference = Some(outcome);
                }
                peak = peak.max(cu);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    eprintln!(
        "Scope I paid origin: worlds={worlds}, liquidations=16, exact_payouts=8, peak_cu={peak}"
    );
}
