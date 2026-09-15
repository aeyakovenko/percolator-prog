//! INV-024/025/031/034/039/041: funding-bearing mixed roles with insurance
//! in the loss domain, opposite side, or foreign asset. Expected debt and owner payouts
//! come from signed inputs, independently of observed balances or close order.

use super::*;

const LOTS: i128 = 4;
const INSURANCE: u128 = 10_000;
const CAPITAL: [u128; 5] = [600_000, 179_985, 300_000, 250_000, 20_001];

fn debt_from_inputs(reverse: bool) -> u128 {
    let sign = if reverse { -1 } else { 1 };
    let funding = funding_from_inputs(sign);
    (LOTS * (SETTLE_MOVE - sign * funding)) as u128
}

fn fund(world: &mut AttributionWorld, domain: usize) {
    let env = &mut world.env;
    let donor = &world.actors[4];
    let donor_key = donor.owner.insecure_clone();
    env.send(
        env.withdraw_ix(donor.portfolio, INSURANCE),
        vec![
            AccountMeta::new(donor.owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(donor.portfolio, false),
            AccountMeta::new(donor.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&donor_key],
    )
    .unwrap();
    let admin = env.admin.insecure_clone();
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&donor_key),
        (domain / 2) as u16,
        processor::ASSET_AUTH_INSURANCE,
        donor.owner.pubkey().to_bytes(),
    )
    .unwrap();
    let seq = env.control_sequences(domain / 2);
    env.send(
        ProgInstruction::TopUpInsuranceDomain {
            domain: domain as u16,
            market_id: env.asset_market_id((domain / 2) as u16),
            authority_epoch: seq.authority_epoch,
            intent_id: next_control_sequence(seq.insurance_top_up),
            amount: INSURANCE,
        },
        vec![
            AccountMeta::new(donor.owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(donor.token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&donor_key],
    )
    .unwrap();
}

#[test]
fn v16_program_mixed_funding_debt_charges_only_its_insurance_domain() {
    let mut peak = 0;
    let mut worlds = 0;
    for reverse in [false, true] {
        let debt = debt_from_inputs(reverse);
        assert_eq!(debt, 239_976);
        for placement in 0..3 {
            let foreign = placement != 0;
            let spent = if foreign { 0 } else { INSURANCE };
            let residual = debt - CAPITAL[1] - spent;
            // K consumes the creditor's principal-backed support and retires
            // its whole face. Insurance pays only the separate B residual;
            // it does not replenish that source's fresh backing.
            let expected = [
                CAPITAL[0] - (debt - CAPITAL[1]) - residual,
                0,
                CAPITAL[2] + debt,
                CAPITAL[3],
                CAPITAL[4] - INSURANCE,
            ];
            let mut outcomes = Vec::new();
            for order in [[0, 2, 1, 3, 4], [2, 0, 3, 1, 4]] {
                let mut world = setup_with_terms(
                    reverse,
                    LOTS * POS_SCALE as i128,
                    LOTS * POS_SCALE as i128,
                    CAPITAL,
                    400,
                    true,
                );
                let side = usize::from(reverse);
                let domain = match placement {
                    0 => 2 + side,
                    1 => 2 + 1 - side,
                    _ => 4 + side,
                };
                let before = world.frame();
                fund(&mut world, domain);
                for (key, account) in before {
                    if ![world.env.market, world.actors[4].portfolio].contains(&key) {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                let outcome = close_all_checked(
                    world,
                    order,
                    &mut peak,
                    INSURANCE - spent,
                    |world, deleted| {
                        let env = &world.env;
                        let group = env.market_state().1;
                        for (i, &is_deleted) in deleted.iter().enumerate() {
                            let paid = env.token_amount(world.actors[i].token) as u128;
                            assert!(paid <= expected[i], "owner {i}: payout {paid} exceeds entitlement {}, reverse={reverse}, domain={domain}, order={order:?}", expected[i]);
                            if is_deleted {
                                assert_eq!(
                                    paid, expected[i],
                                    "owner {i}: exact terminal entitlement"
                                );
                                continue;
                            }
                            let account = env.portfolio_state(world.actors[i].portfolio);
                            let legs: Vec<_> = account
                                .legs
                                .iter()
                                .map(|l| l.try_to_runtime().unwrap())
                                .filter(|l| l.active)
                                .collect();
                            let book_value = match i {
                                0 => {
                                    let debtor = legs.iter().find(|l| l.asset_index == 2);
                                    let settled = debtor.is_none_or(|l| l.k_snap != 0);
                                    let creditor = legs.iter().find(|l| l.asset_index == 1);
                                    let charged = creditor.is_none_or(|l| l.b_snap != 0);
                                    if let Some(leg) = creditor {
                                        assert_eq!(leg.basis_pos_q, 0);
                                        assert_eq!(leg.loss_weight, LOTS as u128 * POS_SCALE);
                                    }
                                    assert!(!charged || settled, "all K/F precedes B settlement");
                                    CAPITAL[0] as i128 + debt as i128
                                        - if settled {
                                            (2 * debt - CAPITAL[1]) as i128
                                        } else {
                                            0
                                        }
                                        - if charged { residual as i128 } else { 0 }
                                }
                                1 => {
                                    if close_progress(&account).finalized {
                                        0
                                    } else {
                                        -((debt - CAPITAL[1]) as i128)
                                    }
                                }
                                2 => {
                                    CAPITAL[2] as i128
                                        + if legs.is_empty() || legs[0].k_snap != 0 {
                                            debt as i128
                                        } else {
                                            0
                                        }
                                }
                                _ => expected[i] as i128,
                            };
                            let receipt = resolved_receipt(&account);
                            let due = if receipt.present {
                                assert_eq!(i, 2, "only the peer owns the residual receipt");
                                receipt
                                    .terminal_positive_claim_face
                                    .checked_sub(receipt.paid_effective)
                                    .unwrap()
                            } else {
                                0
                            };
                            assert_eq!(
                                account.capital.get() as i128
                                    + account.pnl.get()
                                    + due as i128
                                    + paid as i128,
                                book_value,
                                "owner {i}: prefix entitlement, domain={domain}, order={order:?}"
                            );
                            for source in account.source_domains.iter().filter(|s| s.is_occupied())
                            {
                                let expected_domain = match i {
                                    0 => 2 + 1 - side,
                                    2 => 4 + 1 - side,
                                    _ => panic!("unexpected source owner {i}"),
                                };
                                assert_eq!(source.domain.get() as usize, expected_domain);
                            }
                            if i == 1 {
                                let close = close_progress(&account);
                                assert_eq!(close.gross_loss_at_close_start, debt - CAPITAL[1]);
                                if close.finalized {
                                    assert_eq!(close.insurance_spent, spent);
                                    assert_eq!(close.b_loss_booked, residual);
                                    assert_eq!(close.residual_remaining, 0);
                                }
                            }
                        }
                        for d in 0..group.insurance_domain_budget.len() {
                            assert_eq!(
                                group.insurance_domain_budget[d],
                                if d == domain { INSURANCE } else { 0 }
                            );
                            if d != domain || foreign {
                                assert_eq!(group.insurance_domain_spent[d], 0);
                            }
                        }
                        assert_eq!(
                            group.insurance,
                            INSURANCE - group.insurance_domain_spent.iter().sum::<u128>()
                        );
                        let accounts: Vec<_> = world
                            .actors
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| !deleted[*i])
                            .map(|(_, a)| env.portfolio_state(a.portfolio))
                            .collect();
                        assert_market_stock_census(
                            "mixed funded insurance",
                            &group,
                            &env.svm.get_account(&env.market).unwrap().data,
                            &accounts,
                            env.token_amount(env.vault) as u128,
                        )
                        .unwrap();
                        assert_reservation_encumbrance_census(
                            "mixed funded insurance",
                            &group,
                            &accounts,
                        )
                        .unwrap();
                    },
                );
                assert_eq!(outcome.paid, expected);
                assert_eq!(outcome.vault, debt - CAPITAL[1] + INSURANCE - spent);
                outcomes.push(outcome);
                worlds += 1;
            }
            assert_eq!(outcomes[0], outcomes[1]);
        }
    }
    assert_eq!(worlds, 12);
    println!("INV-039 mixed funding/insurance: {worlds} public worlds, exact owner/domain entitlement, peak {peak} CU");
}
