//! INV-014/005/010/024/036/047/081: retained trade consent and retained payout
//! consent have different lifetimes across a funded Live recipient handoff.

use super::*;

fn sign(f: &mut Fixture, successor: &Keypair, instructions: &[Instruction]) -> Transaction {
    f.nonce += 1;
    let mut ixs = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(1_400_000 - f.nonce),
    ];
    ixs.extend_from_slice(instructions);
    let mut signers = vec![&f.env.payer];
    for signer in [&f.env.admin, successor]
        .into_iter()
        .chain(&f.owners)
        .chain(&f.recipients)
    {
        if instructions
            .iter()
            .flat_map(|ix| &ix.accounts)
            .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
        {
            signers.push(signer);
        }
    }
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&f.env.payer.pubkey()),
        &signers,
        f.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn frame(
    f: &Fixture,
    tx: &Transaction,
    successor: &Keypair,
    destination: Pubkey,
) -> Vec<(Pubkey, Option<Account>)> {
    let mut accounts = f.frame(tx);
    accounts
        .extend([successor.pubkey(), destination].map(|key| (key, f.env.svm.get_account(&key))));
    accounts.sort_by_key(|(key, _)| *key);
    accounts.dedup_by_key(|(key, _)| *key);
    accounts
}

fn land(
    f: &mut Fixture,
    successor: &Keypair,
    destination: Pubkey,
    tx: Transaction,
    error: Option<PercolatorError>,
    successes: [usize; 3],
) -> u64 {
    let before = frame(f, &tx, successor, destination);
    let mut payer = f.env.svm.get_account(&f.env.payer.pubkey()).unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = f.env.svm.send_transaction(tx);
    let rejected = error.is_some();
    let meta = match error {
        Some(error) => {
            let failure =
                result.expect_err("retained payout requires the current recipient incarnation");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(3, InstructionError::Custom(error as u32))
            );
            failure.meta
        }
        None => result.expect("current consent realizes the attributed value"),
    };
    for (key, old) in before {
        if key == f.env.payer.pubkey() {
            continue;
        }
        let actual = f.env.svm.get_account(&key);
        let mutable = [f.env.market, f.env.vault, f.context, destination].contains(&key)
            || f.portfolios.contains(&key)
            || f.tokens.contains(&key)
            || f.destinations.contains(&key);
        if rejected || !mutable {
            assert_eq!(actual, old, "complete Account frame: {key}");
        } else {
            let mut expected = old.unwrap();
            expected.data = actual.as_ref().unwrap().data.clone();
            assert_eq!(
                actual,
                Some(expected),
                "only account data may change: {key}"
            );
        }
    }
    assert_eq!(f.env.svm.get_account(&f.env.payer.pubkey()), Some(payer));
    for (program, count) in [f.env.program_id, spl_token::ID, f.matcher]
        .into_iter()
        .zip(successes)
    {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count
        );
    }
    assert!(meta.compute_units_consumed > 0 && meta.compute_units_consumed <= 1_400_000);
    meta.compute_units_consumed
}

fn handoff(f: &Fixture, asset: usize, from: Pubkey, to: Pubkey) -> Instruction {
    Instruction {
        program_id: f.env.program_id,
        accounts: vec![
            AccountMeta::new(from, true),
            AccountMeta::new_readonly(to, true),
            AccountMeta::new(f.env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: asset as u16,
            market_id: f.env.asset_market_id(asset as u16),
            authority_epoch: f.env.control_sequences(asset).authority_epoch,
            kind: processor::ASSET_AUTH_INSURANCE_OPERATOR,
            new_pubkey: to.to_bytes(),
        }
        .encode(),
    }
}

fn payout(
    f: &Fixture,
    asset: usize,
    operator: Pubkey,
    destination: Pubkey,
    amount: u128,
) -> Instruction {
    let mut ix = f.insurance(asset, amount);
    ix.accounts[0].pubkey = operator;
    ix.accounts[2].pubkey = destination;
    ix.data = f
        .env
        .withdraw_insurance_asset_instruction(operator, asset as u16, amount)
        .encode();
    ix
}

fn credit(budgets: &mut [u128; 4], amount: u128, redirect_bps: u16) {
    let redirected = amount * u128::from(redirect_bps) / 10_000;
    for (budget, earned) in budgets.iter_mut().zip([
        2 * (redirected / 2),
        2 * (redirected - redirected / 2),
        amount - redirected,
        amount - redirected,
    ]) {
        *budget += earned;
    }
}

fn debit(budgets: &mut [u128; 4], asset: usize, amount: u128) {
    let long = amount.min(budgets[2 * asset]);
    budgets[2 * asset] -= long;
    budgets[2 * asset + 1] -= amount - long;
}

fn check(
    f: &Fixture,
    successor: &Keypair,
    destination: Pubkey,
    quantity: i128,
    fees: u128,
    budgets: [u128; 4],
    paid: [u128; 3],
    owner_paid: [u128; 2],
) {
    let accounts = f.portfolios.map(|key| f.env.portfolio_state(key));
    let (_, group) = f.env.market_state();
    assert_eq!(&group.insurance_domain_budget[..4], &budgets);
    assert!(group.insurance_domain_budget[4..].iter().all(|v| *v == 0));
    assert_eq!(group.insurance, budgets.iter().sum::<u128>());
    assert_eq!(group.insurance, 2 * fees - paid.iter().sum::<u128>());
    assert_eq!(
        group.c_tot,
        DEPOSITS.iter().sum::<u128>() - 2 * fees - owner_paid.iter().sum::<u128>()
    );
    assert_eq!(group.vault, group.c_tot + group.insurance);
    assert_eq!(group.vault, u128::from(f.env.token_amount(f.env.vault)));
    assert_eq!(
        group.assets[0].oi_eff_long_q + group.assets[0].oi_eff_short_q,
        0
    );
    assert_eq!(group.assets[1].oi_eff_long_q, quantity.unsigned_abs());
    assert_eq!(group.assets[1].oi_eff_short_q, quantity.unsigned_abs());
    for actor in 0..2 {
        assert_eq!(accounts[actor].owner, f.owners[actor].pubkey().to_bytes());
        assert_eq!(
            accounts[actor].capital.get(),
            DEPOSITS[actor] - fees - owner_paid[actor]
        );
        assert_eq!(accounts[actor].pnl.get(), 0);
        if quantity == 0 {
            assert!(!has_active_leg_for_asset(&accounts[actor], 1));
        } else {
            assert_eq!(
                active_leg_for_asset(&accounts[actor], 1).basis_pos_q,
                quantity * if actor == 0 { 1 } else { -1 }
            );
        }
    }
    let tokens = f
        .tokens
        .into_iter()
        .chain(f.destinations)
        .chain([destination]);
    let owners = f.owners.iter().chain(&f.recipients).chain([successor]);
    for ((token, owner), amount) in tokens.zip(owners).zip(owner_paid.into_iter().chain(paid)) {
        let raw = f.env.svm.get_account(&token).unwrap();
        assert_eq!(raw.owner, spl_token::ID);
        let token = TokenAccount::unpack(&raw.data).unwrap();
        assert_eq!(
            (token.owner, token.mint, u128::from(token.amount)),
            (owner.pubkey(), f.env.mint, amount)
        );
    }
    let mint = Mint::unpack(&f.env.svm.get_account(&f.env.mint).unwrap().data).unwrap();
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(u128::from(mint.supply), DEPOSITS.iter().sum::<u128>());
    assert_eq!(
        group.vault + paid.iter().sum::<u128>() + owner_paid.iter().sum::<u128>(),
        u128::from(mint.supply)
    );
    assert_market_stock_census(
        "retained recipient succession",
        &group,
        &f.env.svm.get_account(&f.env.market).unwrap().data,
        &accounts,
        group.vault,
    )
    .unwrap();
    assert_reservation_encumbrance_census("retained recipient succession", &group, &accounts)
        .unwrap();
}

#[test]
fn v16_retained_trade_and_payout_consent_diverge_across_live_recipient_succession() {
    let mut peaks = [0; 4]; // Successful delivery, rejected delivery, policy, handoff.
    let mut worlds = 0;
    let mut rejections = 0;
    for asset in 0..2 {
        for aba in [false, true] {
            let mut f = Fixture::new();
            let successor = Keypair::new();
            send_raw_tx(
                &mut f.env.svm,
                &f.env.payer,
                system_instruction::transfer(&f.env.payer.pubkey(), &successor.pubkey(), 1_000_000),
                &[],
            )
            .unwrap();
            let destination =
                create_ata_for_test(&mut f.env.svm, &f.env.payer, successor.pubkey(), f.env.mint);
            peaks[2] = peaks[2].max(f.env.update_trade_fee_policy_with_cu(CAP_BPS));
            peaks[2] = peaks[2].max(f.env.update_fee_redirect_policy_with_cu(REDIRECTS[0]));
            let opening = f.trade(0, FILLED);
            let tx = sign(&mut f, &successor, &[opening]);
            peaks[0] = peaks[0].max(land(&mut f, &successor, destination, tx, None, [1, 0, 0]));
            let mut budgets = credits(REDIRECTS[0]);
            let mut paid = [0; 3];
            let mut owner_paid = [0; 2];
            for recipient in 0..2 {
                let amount = 5 + 2 * recipient as u128;
                let ix = f.insurance(recipient, amount);
                let tx = sign(&mut f, &successor, &[ix]);
                peaks[0] = peaks[0].max(land(&mut f, &successor, destination, tx, None, [1, 1, 0]));
                debit(&mut budgets, recipient, amount);
                paid[recipient] += amount;
                check(
                    &f,
                    &successor,
                    destination,
                    FILLED,
                    fee(FILLED),
                    budgets,
                    paid,
                    owner_paid,
                );
            }
            f.env.set_matcher_config_with_trade_fee_cap(
                f.matcher,
                &f.owners[1],
                f.portfolios[1],
                f.context,
                f.delegate,
                1,
                CAP_BPS as u16,
            );
            peaks[2] = peaks[2].max(f.env.update_trade_fee_policy_with_cu(OLD_BPS));
            let trade = f.trade(2, FILLED);
            let retained = sign(&mut f, &successor, &[trade.clone()]);
            let retained_bytes = bincode::serialize(&retained).unwrap();
            let stale_ixs = [trade, f.insurance(asset, 1)];
            let stale = sign(&mut f, &successor, &stale_ixs);
            let stale_aba = aba.then(|| sign(&mut f, &successor, &stale_ixs));
            for tx in [&retained, &stale].into_iter().chain(stale_aba.iter()) {
                let before = frame(&f, tx, &successor, destination);
                let meta = f.env.svm.simulate_transaction(tx.clone().into()).unwrap();
                peaks[0] = peaks[0].max(meta.compute_units_consumed);
                assert_eq!(frame(&f, tx, &successor, destination), before);
            }
            let original_epoch = f.env.control_sequences(asset).authority_epoch;
            let ix = handoff(&f, asset, f.recipients[asset].pubkey(), successor.pubkey());
            let tx = sign(&mut f, &successor, &[ix]);
            peaks[3] = peaks[3].max(land(&mut f, &successor, destination, tx, None, [1, 0, 0]));
            assert_eq!(
                f.env.control_sequences(asset).authority_epoch,
                original_epoch + 1
            );
            peaks[2] = peaks[2].max(f.env.update_trade_fee_policy_with_cu(CAP_BPS));
            peaks[2] = peaks[2].max(f.env.update_fee_redirect_policy_with_cu(REDIRECTS[1]));
            let cfg = f.env.market_state().0;
            assert_eq!(
                (cfg.trade_fee_base_bps, cfg.fee_redirect_to_market_0_bps),
                (CAP_BPS, REDIRECTS[1])
            );
            // A paid legacy prefix belongs to A's existing SPL account. Only the
            // unpaid domain stock is now spendable by the consensual successor.
            let ix = payout(&f, asset, successor.pubkey(), destination, 3);
            let tx = sign(&mut f, &successor, &[ix]);
            peaks[0] = peaks[0].max(land(&mut f, &successor, destination, tx, None, [1, 1, 0]));
            debit(&mut budgets, asset, 3);
            paid[2] += 3;
            peaks[1] = peaks[1].max(land(
                &mut f,
                &successor,
                destination,
                stale,
                Some(PercolatorError::Unauthorized),
                [1, 0, 1],
            ));
            rejections += 1;
            check(
                &f,
                &successor,
                destination,
                FILLED,
                fee(FILLED),
                budgets,
                paid,
                owner_paid,
            );
            if let Some(stale_aba) = stale_aba {
                let ix = handoff(&f, asset, successor.pubkey(), f.recipients[asset].pubkey());
                let tx = sign(&mut f, &successor, &[ix]);
                peaks[3] = peaks[3].max(land(&mut f, &successor, destination, tx, None, [1, 0, 0]));
                assert_eq!(
                    f.env.control_sequences(asset).authority_epoch,
                    original_epoch + 2
                );
                peaks[1] = peaks[1].max(land(
                    &mut f,
                    &successor,
                    destination,
                    stale_aba,
                    Some(PercolatorError::EngineStale),
                    [1, 0, 1],
                ));
                rejections += 1;
                check(
                    &f,
                    &successor,
                    destination,
                    FILLED,
                    fee(FILLED),
                    budgets,
                    paid,
                    owner_paid,
                );
            }
            assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
            peaks[0] = peaks[0].max(land(
                &mut f,
                &successor,
                destination,
                retained,
                None,
                [1, 0, 1],
            ));
            credit(&mut budgets, fee(FILLED), REDIRECTS[1]);
            check(
                &f,
                &successor,
                destination,
                2 * FILLED,
                2 * fee(FILLED),
                budgets,
                paid,
                owner_paid,
            );
            let close = f.trade(0, -2 * FILLED);
            let tx = sign(&mut f, &successor, &[close]);
            peaks[0] = peaks[0].max(land(&mut f, &successor, destination, tx, None, [1, 0, 0]));
            let fees = 2 * fee(FILLED) + fee(2 * FILLED);
            assert_eq!(fees, 191);
            credit(&mut budgets, fee(2 * FILLED), REDIRECTS[1]);
            check(
                &f,
                &successor,
                destination,
                0,
                fees,
                budgets,
                paid,
                owner_paid,
            );
            for recipient in 0..2 {
                let successor_owns = recipient == asset && !aba;
                let operator = if successor_owns {
                    successor.pubkey()
                } else {
                    f.recipients[recipient].pubkey()
                };
                let token = if successor_owns {
                    destination
                } else {
                    f.destinations[recipient]
                };
                let amount = budgets[2 * recipient] + budgets[2 * recipient + 1];
                let ix = payout(&f, recipient, operator, token, amount);
                let tx = sign(&mut f, &successor, &[ix]);
                peaks[0] = peaks[0].max(land(&mut f, &successor, destination, tx, None, [1, 1, 0]));
                debit(&mut budgets, recipient, amount);
                paid[if successor_owns { 2 } else { recipient }] += amount;
                check(
                    &f,
                    &successor,
                    destination,
                    0,
                    fees,
                    budgets,
                    paid,
                    owner_paid,
                );
            }
            for actor in 0..2 {
                let amount = DEPOSITS[actor] - fees;
                let ix = Instruction {
                    program_id: f.env.program_id,
                    accounts: vec![
                        AccountMeta::new(f.owners[actor].pubkey(), true),
                        AccountMeta::new(f.env.market, false),
                        AccountMeta::new(f.portfolios[actor], false),
                        AccountMeta::new(f.tokens[actor], false),
                        AccountMeta::new(f.env.vault, false),
                        AccountMeta::new_readonly(f.env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: f.env.withdraw_ix(f.portfolios[actor], amount).encode(),
                };
                let tx = sign(&mut f, &successor, &[ix]);
                peaks[0] = peaks[0].max(land(&mut f, &successor, destination, tx, None, [1, 1, 0]));
                owner_paid[actor] = amount;
                check(
                    &f,
                    &successor,
                    destination,
                    0,
                    fees,
                    budgets,
                    paid,
                    owner_paid,
                );
            }
            // Input-priced lifetime fees total 382 atoms, attributed 220/162
            // between the redirect and source assets, independently of succession.
            let mut expected_paid = [220, 162, 0];
            expected_paid[2] = if aba {
                3
            } else {
                expected_paid[asset] - (5 + 2 * asset as u128)
            };
            expected_paid[asset] -= expected_paid[2];
            assert_eq!(paid, expected_paid);
            assert_eq!(f.env.token_amount(f.env.vault), 0);
            worlds += 1;
        }
    }
    assert_eq!((worlds, rejections), (4, 6));
    println!("INV-014 retained recipient succession: {worlds} worlds, 10 initial simulations, {rejections} exact trade/matcher rollbacks; peak CU [success/simulation, rejection, policy, handoff]={peaks:?}");
}
