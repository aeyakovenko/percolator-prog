//! Row 421 / INV-073: partial insurance recredit after keeper-only expiry with
//! unfinished user liabilities and missing beneficiary custody. No reserve or
//! admin signature is available in this continuation; owners delete empty accounts.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const LIMIT: u64 = 600_000;
const DEFICIT: u64 = 100;
const PAYOUTS: [u64; 3] = [1_200, 0, 137];

fn land(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    signers: &[&Keypair],
    tracked: &[Pubkey],
    allowed: &[Pubkey],
    rent: u64,
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    env.svm.expire_blockhash();
    let instructions = [
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
    ]
    .into_iter()
    .chain(ixs.iter().cloned())
    .collect::<Vec<_>>();
    let signatures = [&env.payer]
        .into_iter()
        .chain(signers.iter().copied())
        .collect::<Vec<_>>();
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signatures,
        env.svm.latest_blockhash(),
    );
    assert_eq!(
        usize::from(tx.message.header.num_required_signatures),
        signatures.len()
    );
    assert_eq!(tx.message.account_keys[0], env.payer.pubkey());
    assert!(!tx.message.account_keys[..signatures.len()].contains(&env.admin.pubkey()));
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    let before = keys
        .iter()
        .map(|key| env.svm.get_account(key))
        .collect::<Vec<_>>();
    let rejected = rejection.is_some();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, error)) = rejection {
        let failure = result.expect_err("terminal prerequisite remains protected");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
        );
        for program in [env.program_id, associated_token_program_id()] {
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                ixs[..usize::from(index) - 2]
                    .iter()
                    .filter(|ix| ix.program_id == program)
                    .count(),
                "every intended repair/settlement/payout prefix completes before rollback"
            );
        }
        failure.meta
    } else {
        result.expect("bounded terminal continuation without reserve or admin signatures")
    };
    for (key, mut expected) in keys.into_iter().zip(before) {
        if key == env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= signatures.len() as u64
                * FeeStructure::default().lamports_per_signature
                + if rejected { 0 } else { rent };
        }
        if rejected || !allowed.contains(&key) {
            assert_eq!(
                env.svm.get_account(&key),
                expected,
                "complete Account frame {key}"
            );
        }
    }
    meta.compute_units_consumed
}

#[test]
fn v16_program_partial_insurance_recredit_crosses_active_liabilities_and_custody_repair() {
    for backing in [37, 73] {
        for before_loss in [false, true] {
            for repair_early in [false, true] {
                absent_reserve_progress(
                    ProviderHistory::PartialRecredit {
                        backing,
                        before_loss,
                        repair_early,
                    },
                    None,
                );
            }
        }
    }
}

pub(super) fn finish(
    env: &mut V16CuEnv,
    owners: &[Keypair; 3],
    portfolios: [Pubkey; 3],
    tokens: [Pubkey; 3],
    absent: [Pubkey; 3],
    reserves: [Pubkey; 3],
    asset: usize,
    remainder: u64,
    backing: u64,
    before_loss: bool,
    repair_early: bool,
) {
    let [destination, admin_token, provider_token] = reserves;
    let domain = 2 * asset;
    let market = env.market;
    let vault = env.vault;
    let program_id = env.program_id;
    let admin = env.admin.pubkey();
    let mint_frame = env.svm.get_account(&env.mint);
    let role_frames = absent.map(|key| env.svm.get_account(&key));
    let admin_frame = env.svm.get_account(&admin);
    let profiles = [0, 1].map(|index| {
        state::read_asset_oracle_profile(&env.svm.get_account(&market).unwrap().data, index)
            .unwrap()
    });
    let sequences = [0, 1].map(|index| env.control_sequences(index));
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let initial_market_rent = env.svm.get_account(&market).unwrap().lamports;
    let portfolio_rents = portfolios.map(|key| env.svm.get_account(&key).unwrap().lamports);
    let tracked = [market, vault, env.mint, env.vault_authority, admin]
        .into_iter()
        .chain(portfolios)
        .chain(tokens)
        .chain(reserves)
        .chain(absent)
        .chain(owners.each_ref().map(Signer::pubkey))
        .collect::<Vec<_>>();
    assert!(backing > 0 && backing < DEFICIT);
    assert!(!absent.contains(&admin));
    assert!(!tracked.contains(&env.payer.pubkey()));
    assert!(env
        .svm
        .get_account(&destination)
        .is_none_or(|a| a.lamports == 0));

    let wrap = |data: ProgInstruction, accounts| Instruction {
        program_id,
        accounts,
        data: data.encode(),
    };
    let repair = Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(absent[0], false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: vec![1],
    };
    let insurance = |env: &V16CuEnv, amount: u64, epoch: u64| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(absent[0], false),
            AccountMeta::new(market, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: epoch,
            amount: amount.into(),
        }
        .encode(),
    };
    // This envelope is retained across debt settlement, expiry and portfolio deletion.
    let retained = insurance(env, 17, sequences[asset].authority_epoch);
    let payouts = std::array::from_fn::<_, 3, _>(|actor| {
        wrap(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owners[actor].pubkey(), false),
                AccountMeta::new(market, false),
                AccountMeta::new(portfolios[actor], false),
                AccountMeta::new(tokens[actor], false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        )
    });
    let unsigned_close = wrap(
        ProgInstruction::CloseSlab {
            authority_epoch: sequences[0].authority_epoch,
        },
        vec![
            AccountMeta::new(admin, false),
            AccountMeta::new(market, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(admin_token, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ],
    );
    let mut peak = 0;
    let mut rollbacks = 0;
    let mut run = |env: &mut V16CuEnv,
                   ixs: &[Instruction],
                   signers: &[&Keypair],
                   allowed: &[Pubkey],
                   creation_rent: u64,
                   rejection: Option<(u8, PercolatorError)>| {
        for meta in ixs
            .iter()
            .flat_map(|ix| &ix.accounts)
            .filter(|meta| meta.is_signer)
        {
            assert!(
                meta.pubkey == env.payer.pubkey()
                    || signers.iter().any(|signer| signer.pubkey() == meta.pubkey)
            );
            assert!(meta.pubkey != admin && !absent.contains(&meta.pubkey));
        }
        assert!(signers
            .iter()
            .all(|signer| owners.iter().any(|owner| owner.pubkey() == signer.pubkey())));
        rollbacks += usize::from(rejection.is_some());
        let cu = land(
            env,
            ixs,
            signers,
            &tracked,
            allowed,
            creation_rent,
            rejection,
        );
        peak = peak.max(cu);
        assert_cu_within("partial recredit and active liabilities", cu, LIMIT);
        assert_eq!(env.svm.get_account(&admin), admin_frame);
        assert_eq!(absent.map(|key| env.svm.get_account(&key)), role_frames);
        assert_eq!(env.svm.get_account(&env.mint), mint_frame);
        assert_eq!(env.token_amount(admin_token), 0);
        assert_eq!(env.token_amount(provider_token), 0);
        let group = env.market_state().1;
        let paid = tokens.map(|key| env.token_amount(key));
        let insurance_paid = env
            .svm
            .get_account(&destination)
            .filter(|account| account.owner == spl_token::ID)
            .map_or(0, |account| {
                TokenAccount::unpack(&account.data).unwrap().amount
            });
        assert_eq!(env.token_amount(vault), u64::try_from(group.vault).unwrap());
        assert_eq!(
            env.token_amount(vault) + paid.iter().sum::<u64>() + insurance_paid,
            PAYOUTS.iter().sum::<u64>() + backing + remainder
        );
        assert!(paid
            .into_iter()
            .zip(PAYOUTS)
            .all(|(actual, bound)| actual <= bound));
        let accounts: Vec<_> = portfolios
            .iter()
            .filter_map(|key| {
                env.svm
                    .get_account(key)
                    .filter(|a| a.owner == env.program_id && !a.data.is_empty())
                    .map(|_| env.portfolio_state(*key))
            })
            .collect();
        let market_account = env.svm.get_account(&market).unwrap();
        assert_market_stock_census(
            "partial recredit",
            &group,
            &market_account.data,
            &accounts,
            group.vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("partial recredit", &group, &accounts).unwrap();
        for index in [0, 1] {
            assert_eq!(
                state::read_asset_oracle_profile(&market_account.data, index).unwrap(),
                profiles[index]
            );
        }
    };

    env.svm.warp_to_slot(40);
    run(
        env,
        &[wrap(
            ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
            vec![AccountMeta::new(market, false)],
        )],
        &[],
        &[market],
        0,
        None,
    );
    env.svm.warp_to_slot(42);
    run(
        env,
        &[payouts[1].clone()],
        &[],
        &[],
        0,
        Some((2, PercolatorError::ExpectedSigner)),
    );
    env.svm.warp_to_slot(43);
    assert_eq!(
        env.portfolio_state(portfolios[1]).pnl.get(),
        -i128::from(DEFICIT)
    );
    assert!(env.market_state().1.source_claim_bound_total_num > 0);
    run(
        env,
        &[repair.clone(), retained.clone()],
        &[],
        &[],
        0,
        Some((3, PercolatorError::EngineLockActive)),
    );
    if repair_early {
        run(env, &[repair.clone()], &[], &[destination], rent, None);
    }

    // Expiry is discovered by an active resolved leg, without CloseSlab/admin help.
    let rank = |env: &V16CuEnv, actor: usize| {
        let account = env.portfolio_state(portfolios[actor]);
        (
            usize::from(
                env.market_state().1.source_backing_buckets[domain].status
                    == BackingBucketStatusV16::Fresh,
            ),
            account.pnl.get().min(0).unsigned_abs(),
            account.legs.iter().filter(|leg| leg.active != 0).count(),
            account
                .source_domains
                .iter()
                .filter(|source| source.is_occupied())
                .count(),
            account.capital.get(),
            account.pnl.get().max(0) as u128,
            !resolved_portfolio_is_terminal(env, portfolios[actor]),
        )
    };
    let mut calls = [0; 3];
    let mut expirations = 0;
    for actor in [1, 0, 2] {
        if (before_loss && actor == 1) || (!before_loss && actor == 0) {
            env.svm.warp_to_slot(if before_loss { 44 } else { 45 });
        }
        while !resolved_portfolio_is_terminal(env, portfolios[actor]) {
            assert!(calls[actor] < 8);
            let before = rank(env, actor);
            let mut batch = vec![payouts[actor].clone()];
            if !repair_early {
                batch.push(repair.clone());
            }
            batch.push(retained.clone());
            let rejected_index = 1 + batch.len() as u8;
            run(
                env,
                &batch,
                &[],
                &[],
                0,
                Some((rejected_index, PercolatorError::EngineLockActive)),
            );
            assert_eq!(rank(env, actor), before);
            run(
                env,
                &[payouts[actor].clone()],
                &[],
                &[market, vault, portfolios[actor], tokens[actor]],
                0,
                None,
            );
            let after = rank(env, actor);
            assert!(
                after < before,
                "bounded liability continuation {before:?} -> {after:?}"
            );
            if after.0 < before.0 {
                expirations += 1;
                assert!(
                    env.market_state().1.source_claim_bound_total_num > 0,
                    "expiry must occur with the profitable peer's claim unfinished"
                );
                assert_eq!(
                    env.market_state().1.insurance_domain_spent[domain],
                    if before_loss { 0 } else { u128::from(DEFICIT) }
                );
            }
            calls[actor] += 1;
        }
        assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
        assert_eq!(
            env.market_state().1.insurance_domain_spent[domain],
            u128::from(DEFICIT)
        );
    }
    assert_eq!(expirations, 1);
    let settled = env.market_state().1;
    assert_eq!(
        (
            settled.c_tot,
            settled.pnl_pos_tot,
            settled.source_claim_bound_total_num
        ),
        (0, 0, 0)
    );
    assert_eq!(settled.materialized_portfolio_count, 3);
    assert_eq!(settled.vault, u128::from(backing + remainder));
    assert_eq!(settled.insurance, u128::from(remainder));
    assert_eq!(
        settled.source_credit[domain + 1].provider_receivable_num,
        u128::from(DEFICIT) * BOUND_SCALE
    );
    assert_eq!(
        settled.source_backing_buckets[domain].status,
        BackingBucketStatusV16::Expired
    );
    assert_eq!(
        settled.source_backing_buckets[domain].fresh_unliened_backing_num,
        0
    );

    let mut refunded = 0;
    for (deleted, actor) in [1, 0, 2].into_iter().enumerate() {
        let mut blocked = if repair_early {
            vec![]
        } else {
            vec![repair.clone()]
        };
        blocked.push(retained.clone());
        run(
            env,
            &blocked,
            &[],
            &[],
            0,
            Some((1 + blocked.len() as u8, PercolatorError::EngineLockActive)),
        );
        let delete = wrap(
            env.close_portfolio_ix(portfolios[actor]),
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(portfolios[actor], false),
            ],
        );
        if deleted == 2 {
            // Once the last owner deletes, the identical insurance envelope can pay.
            let mut batch = vec![delete.clone()];
            if !repair_early {
                batch.push(repair.clone());
            }
            batch.extend([retained.clone(), unsigned_close.clone()]);
            run(
                env,
                &batch,
                &[&owners[actor]],
                &[],
                0,
                Some((1 + batch.len() as u8, PercolatorError::ExpectedSigner)),
            );
        }
        run(
            env,
            &[delete],
            &[&owners[actor]],
            &[market, portfolios[actor]],
            0,
            None,
        );
        refunded += portfolio_rents[actor];
        assert_eq!(
            env.svm.get_account(&market).unwrap().lamports,
            initial_market_rent + refunded
        );
        assert_eq!(
            env.market_state().1.materialized_portfolio_count,
            (2 - deleted) as u64
        );
    }

    let terminal = env.market_state();
    let vault_frame = env.svm.get_account(&vault).unwrap();
    let mut batch = if repair_early {
        vec![]
    } else {
        vec![repair.clone()]
    };
    batch.push(retained.clone());
    // Both requests fit custody, but the paid prefix consumes the retained epoch.
    let mut rejected = batch.clone();
    rejected.push(retained.clone());
    run(
        env,
        &rejected,
        &[],
        &[],
        0,
        Some((1 + rejected.len() as u8, PercolatorError::EngineStale)),
    );
    run(
        env,
        &batch,
        &[],
        &[market, vault, destination],
        if repair_early { 0 } else { rent },
        None,
    );
    let mut paid = 17;
    let mut expected = terminal.clone();
    let check_payment = |env: &V16CuEnv,
                         expected: &mut (state::WrapperConfigV16, MarketGroupV16),
                         paid: u64,
                         debits: u64| {
        let remaining = backing + remainder - paid;
        expected.1.vault = u128::from(remaining);
        expected.1.insurance = u128::from(remaining);
        expected.1.insurance_domain_spent[domain] = u128::from(DEFICIT - backing);
        expected.1.insurance_domain_budget[domain] = u128::from(DEFICIT + remainder - paid);
        expected.1.insurance_domain_budget_remaining_total = u128::from(remaining);
        assert_eq!(
            env.market_state(),
            *expected,
            "partial recredit preserves unrecovered spend"
        );
        let mut expected_vault = vault_frame.clone();
        let mut token = TokenAccount::unpack(&expected_vault.data).unwrap();
        token.amount -= paid;
        TokenAccount::pack(token, &mut expected_vault.data).unwrap();
        assert_eq!(env.svm.get_account(&vault), Some(expected_vault));
        let custody = env.svm.get_account(&destination).unwrap();
        assert_eq!(custody.lamports, rent);
        assert_eq!(custody.owner, spl_token::ID);
        assert!(!custody.executable);
        assert_eq!(
            TokenAccount::unpack(&custody.data).unwrap(),
            TokenAccount {
                mint: env.mint,
                owner: absent[0],
                amount: paid,
                delegate: COption::None,
                state: spl_token::state::AccountState::Initialized,
                is_native: COption::None,
                delegated_amount: 0,
                close_authority: COption::None,
            }
        );
        for index in [0, 1] {
            let mut sequence = sequences[index];
            if index == asset {
                sequence.authority_epoch += debits;
            }
            assert_eq!(env.control_sequences(index), sequence);
        }
    };
    check_payment(env, &mut expected, paid, 1);
    run(
        env,
        &[retained],
        &[],
        &[],
        0,
        Some((2, PercolatorError::EngineStale)),
    );
    let final_payment = insurance(
        env,
        backing + remainder - paid,
        sequences[asset].authority_epoch + 1,
    );
    run(
        env,
        &[final_payment.clone(), unsigned_close.clone()],
        &[],
        &[],
        0,
        Some((3, PercolatorError::ExpectedSigner)),
    );
    run(
        env,
        &[final_payment],
        &[],
        &[market, vault, destination],
        0,
        None,
    );
    paid = backing + remainder;
    check_payment(env, &mut expected, paid, 2);
    run(
        env,
        &[unsigned_close],
        &[],
        &[],
        0,
        Some((2, PercolatorError::ExpectedSigner)),
    );
    assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
    assert_eq!(env.token_amount(destination), backing + remainder);
    assert_eq!(env.token_amount(vault), 0);
    drop(run);
    eprintln!("lane22 partial recredit: asset={asset}, remainder={remainder}, backing={backing}, before_loss={before_loss}, repair_early={repair_early}, calls={calls:?}, rollbacks={rollbacks}, spent={}, paid={paid}, peak={peak} CU", DEFICIT - backing);
}
