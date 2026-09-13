//! INV-014 / row 432: retained single-CPI close consent with one underfunded role.
//! Public withdrawals create the shortfall before signing. Policy changes cannot
//! bypass the taker's rate cap, and an authorized full close drops only the
//! uncollectible fee. Complete rollback includes a successfully collected close.
//! Bounded Live/base-fee histories with fixed SPL supply and owner payouts.

use super::*;

const CAP_BPS: u64 = 99;

fn withdraw(w: &World, actor: usize, amount: u128) -> Instruction {
    Instruction {
        program_id: w.env.program_id,
        accounts: vec![
            AccountMeta::new(w.owners[actor].pubkey(), true),
            AccountMeta::new(w.env.market, false),
            AccountMeta::new(w.portfolios[actor], false),
            AccountMeta::new(w.sources[actor], false),
            AccountMeta::new(w.env.vault, false),
            AccountMeta::new_readonly(w.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: w.env.withdraw_ix(w.portfolios[actor], amount).encode(),
    }
}

fn check_budget(
    w: &World,
    size: i128,
    capital: [u128; 2],
    tokens: [u64; 2],
    fees_by_role: [u128; 2],
    opening_direction: i128,
) {
    let accounts = w.portfolios.map(|key| w.env.portfolio_state(key));
    let (_, group) = w.env.market_state();
    for actor in 0..2 {
        assert_eq!(accounts[actor].owner, w.owners[actor].pubkey().to_bytes());
        assert_eq!(accounts[actor].capital.get(), capital[actor]);
        assert_eq!(accounts[actor].pnl.get(), 0);
        assert_eq!(accounts[actor].fee_credits.get(), 0);
        assert_eq!(w.env.token_amount(w.sources[actor]), tokens[actor]);
        if size == 0 {
            assert!(!has_active_leg_for_asset(&accounts[actor], 0));
        } else {
            assert_eq!(
                active_leg_for_asset(&accounts[actor], 0).basis_pos_q,
                if actor == 0 { size } else { -size }
            );
        }
    }
    let insurance = fees_by_role.iter().sum::<u128>();
    // Opening fees are equal; asymmetric close fees follow the closing trade's
    // signed side, which is the opposite of the owner's opening exposure.
    let domains = if opening_direction < 0 {
        fees_by_role
    } else {
        [fees_by_role[1], fees_by_role[0]]
    };
    assert_eq!(&group.insurance_domain_budget[..2], &domains);
    assert_eq!(group.insurance, insurance);
    assert_eq!(group.c_tot, capital.iter().sum::<u128>());
    assert_eq!(group.vault, group.c_tot + insurance);
    assert_eq!(group.assets[0].effective_price, PRICE);
    assert_eq!(group.assets[0].oi_eff_long_q, size.unsigned_abs());
    assert_eq!(group.assets[0].oi_eff_short_q, size.unsigned_abs());
    let vault = w.env.token_amount(w.env.vault);
    assert_eq!(u128::from(vault), group.vault);
    let supply = DEPOSITS.iter().sum::<u64>() + PREFIX;
    assert_eq!(vault + tokens.iter().sum::<u64>(), supply);
    let mint = Mint::unpack(&w.env.svm.get_account(&w.env.mint).unwrap().data).unwrap();
    assert_eq!(mint.supply, supply);
    assert_eq!(mint.mint_authority, COption::None);
    assert_market_stock_census(
        "retained underfunded close",
        &group,
        &w.env.svm.get_account(&w.env.market).unwrap().data,
        &accounts,
        vault.into(),
    )
    .unwrap();
    assert_reservation_encumbrance_census("retained underfunded close", &group, &accounts).unwrap();
}

#[test]
fn v16_retained_single_cpi_underfunded_close_preserves_consent_and_actual_fee_budget() {
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).unwrap();
    let opening_fee = fee(OLD_BPS);
    let quoted_close_fee = fee(CAP_BPS);
    assert_eq!((opening_fee, quoted_close_fee), (49, 253));
    assert!(CAP_BPS + 1 < u64::from(LP_CAP_BPS));
    let mut peaks = [0; 4]; // Simulation, public setup/control, rejection, close/payout.
    let (mut worlds, mut simulations, mut rejections, mut closes, mut payouts) = (0, 0, 0, 0, 0);

    for direction in [-1, 1] {
        for depleted_actor in 0..2 {
            for remaining in [31, quoted_close_fee - u128::from(PREFIX) - 1] {
                let mut w = World::with_params(
                    &matcher_bytes,
                    V16CuMarketParams {
                        maintenance_margin_bps: 10,
                        initial_margin_bps: 10,
                        max_price_move_bps_per_slot: 1,
                        trade_fee_base_bps: OLD_BPS,
                        ..V16CuMarketParams::default()
                    },
                );
                let size = direction * QUANTITY;
                let mut capital = DEPOSITS.map(u128::from);
                let mut tokens = [PREFIX, 0];
                let mut paid = [0; 2];
                check_budget(&w, 0, capital, tokens, paid, direction);

                let amount = capital[depleted_actor] - opening_fee - remaining;
                let tx = w.sign(&[withdraw(&w, depleted_actor, amount)], 0);
                peaks[1] = peaks[1].max(w.deliver(
                    tx,
                    false,
                    &[
                        w.env.market,
                        w.portfolios[depleted_actor],
                        w.sources[depleted_actor],
                        w.env.vault,
                    ],
                    [1, 1, 0],
                ));
                capital[depleted_actor] -= amount;
                tokens[depleted_actor] += u64::try_from(amount).unwrap();
                check_budget(&w, 0, capital, tokens, paid, direction);
                let open =
                    w.env
                        .trade_cpi_ix(w.portfolios[0], w.portfolios[1], 0, size, OLD_BPS, PRICE);
                let tx = w.sign(&w.bundle_instructions(&open)[1..], 1);
                peaks[1] = peaks[1].max(w.deliver(
                    tx,
                    false,
                    &[w.env.market, w.portfolios[0], w.portfolios[1], w.context],
                    [1, 0, 1],
                ));
                capital = capital.map(|amount| amount - opening_fee);
                paid = [opening_fee; 2];
                assert_eq!(capital[depleted_actor], remaining);
                check_budget(&w, size, capital, tokens, paid, direction);
                let mut available = capital;
                available[0] += u128::from(PREFIX);
                let collected = available.map(|amount| amount.min(quoted_close_fee));
                assert!(
                    0 < collected[depleted_actor] && collected[depleted_actor] < quoted_close_fee
                );
                assert_eq!(collected[1 - depleted_actor], quoted_close_fee);

                let close =
                    w.env
                        .trade_cpi_ix(w.portfolios[0], w.portfolios[1], 0, -size, CAP_BPS, PRICE);
                let initial = w.env.control_sequences(0);
                let old_policy = Instruction {
                    program_id: w.env.program_id,
                    accounts: vec![
                        AccountMeta::new(w.env.admin.pubkey(), true),
                        AccountMeta::new(w.env.market, false),
                    ],
                    data: ProgInstruction::UpdateTradeFeePolicy {
                        trade_fee_base_bps: OLD_BPS,
                        policy_sequence: initial.trade_fee + 1,
                        authority_epoch: initial.authority_epoch,
                    }
                    .encode(),
                };
                let [deposit, close_ix] = w.bundle_instructions(&close);
                let retained = [
                    w.bundle(&close, 2),
                    w.sign(&[deposit, close_ix, old_policy], 3),
                    w.bundle(&close, 4),
                ];
                let bytes = retained
                    .each_ref()
                    .map(|tx| bincode::serialize(tx).unwrap());
                for (tx, signatures) in retained.iter().zip([2, 3, 2]) {
                    assert_eq!(tx.message.header.num_required_signatures, signatures);
                    assert!(!tx.message.account_keys[..signatures as usize]
                        .contains(&w.owners[1].pubkey()));
                    peaks[0] = peaks[0].max(w.simulate(tx));
                    simulations += 1;
                }

                // Low collectible capital does not relax the signed rate boundary.
                peaks[1] = peaks[1].max(w.policy(CAP_BPS + 1, 10));
                peaks[2] = peaks[2].max(w.deliver(retained[0].clone(), true, &[], [1, 1, 0]));
                rejections += 1;
                check_budget(&w, size, capital, tokens, paid, direction);
                peaks[1] = peaks[1].max(w.policy(CAP_BPS, 11));
                let controls = w.env.control_sequences(0);
                assert_eq!(controls.trade_fee, initial.trade_fee + 2);

                // The underfunded close succeeds before the stale policy suffix:
                // rollback restores dropped fees, positions, SPL and matcher data.
                peaks[2] = peaks[2].max(w.deliver_with_error(
                    retained[1].clone(),
                    Some((
                        4,
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    )),
                    &[],
                    [2, 1, 1],
                ));
                rejections += 1;
                check_budget(&w, size, capital, tokens, paid, direction);
                assert_eq!(w.env.control_sequences(0), controls);
                for (tx, bytes) in retained.iter().zip(&bytes) {
                    tx.verify().unwrap();
                    assert_eq!(&bincode::serialize(tx).unwrap(), bytes);
                }
                assert_eq!(w.bundle(&close, 4), retained[2]);
                peaks[0] = peaks[0].max(w.simulate(&retained[2]));
                simulations += 1;
                peaks[3] = peaks[3].max(w.deliver(
                    retained[2].clone(),
                    false,
                    &[
                        w.env.market,
                        w.portfolios[0],
                        w.portfolios[1],
                        w.sources[0],
                        w.env.vault,
                        w.context,
                    ],
                    [2, 1, 1],
                ));
                closes += 1;
                let fill = percolator_prog::matcher_abi::read_matcher_return(
                    &w.env.svm.get_account(&w.context).unwrap().data,
                )
                .unwrap();
                assert_eq!((fill.exec_size, fill.exec_price_e6), (-size, PRICE));
                tokens[0] -= PREFIX;
                for actor in 0..2 {
                    capital[actor] = available[actor] - collected[actor];
                    paid[actor] += collected[actor];
                }
                assert_eq!(capital[depleted_actor], 0);
                check_budget(&w, 0, capital, tokens, paid, direction);

                let actor = 1 - depleted_actor;
                let amount = capital[actor];
                assert!(amount > 0);
                let tx = w.sign(&[withdraw(&w, actor, amount)], 12);
                peaks[3] = peaks[3].max(w.deliver(
                    tx,
                    false,
                    &[
                        w.env.market,
                        w.portfolios[actor],
                        w.sources[actor],
                        w.env.vault,
                    ],
                    [1, 1, 0],
                ));
                payouts += 1;
                capital[actor] = 0;
                tokens[actor] += u64::try_from(amount).unwrap();
                check_budget(&w, 0, capital, tokens, paid, direction);
                for actor in 0..2 {
                    assert_eq!(
                        u128::from(tokens[actor]) + paid[actor],
                        u128::from(DEPOSITS[actor] + if actor == 0 { PREFIX } else { 0 }),
                        "only actually collected fees reduce the owner's final SPL entitlement",
                    );
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(
        (worlds, simulations, rejections, closes, payouts),
        (8, 32, 16, 8, 8)
    );
    println!("row432 underfunded close: worlds={worlds}, simulations={simulations}, rejections={rejections}, closes={closes}, payouts={payouts}, peak_cu={peaks:?}");
}
