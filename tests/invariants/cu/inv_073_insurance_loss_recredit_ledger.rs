//! Row 421: unsigned insurance telemetry spans a realized loss and later recredit.
//! A funded ledger observes the loss before backing expiry, then records recovery
//! without erasing the loss or counting the earlier payment twice. Public setup only.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use crate::inv_071_crank_progress::terminal_prefix_recredit::{land, wrap};
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::instruction::InstructionError;

const CAPITAL: [u64; 3] = [1_000, 100, 137];
const LOSS: u64 = 100;
const FIRST: u64 = 17;
const FUNDED: u64 = LOSS + FIRST;
const BACKING: u64 = 307;
const EXPIRY: u64 = 44;
const PAYOUTS: [u64; 3] = [1_200, 0, 137];
const SUPPLY: u64 = 1_000 + 100 + 137 + FUNDED + BACKING;

fn payment(
    env: &V16CuEnv,
    beneficiary: Pubkey,
    token: Pubkey,
    ledger: Pubkey,
    amount: u64,
) -> Instruction {
    wrap(
        env,
        env.withdraw_insurance_asset_instruction(beneficiary, 0, amount.into()),
        vec![
            AccountMeta::new_readonly(beneficiary, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
    )
}

fn close(env: &V16CuEnv, destination: Pubkey) -> Instruction {
    wrap(
        env,
        ProgInstruction::CloseSlab {
            authority_epoch: env.control_sequences(0).authority_epoch,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ],
    )
}

#[test]
fn v16_program_unsigned_insurance_ledger_preserves_loss_and_recredit_after_cleanup() {
    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 1,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
    );
    let admin = env.admin.insecure_clone();
    let beneficiary = Keypair::new();
    let operator = Keypair::new();
    let beneficiary_key = beneficiary.pubkey();
    let operator_key = operator.pubkey();
    assert_ne!(beneficiary_key, operator_key);
    for role in [beneficiary_key, operator_key, admin.pubkey()] {
        assert_ne!(role, env.payer.pubkey());
    }
    for (kind, signer) in [
        (processor::ASSET_AUTH_INSURANCE, &beneficiary),
        (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
    ] {
        env.svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
        env.try_update_per_asset_authority_with_cu(
            &admin,
            Some(signer),
            0,
            kind,
            signer.pubkey().to_bytes(),
        )
        .unwrap();
    }
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_permissionless_resolve_with_cu(20, 3);
    let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
    let portfolios = owners.each_ref().map(|owner| {
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let key = Keypair::new();
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
        key.pubkey()
    });
    let tokens = owners
        .each_ref()
        .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
    let reserve = create_ata_for_test(&mut env.svm, &env.payer, beneficiary_key, env.mint);
    let destination = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    let ledger_key = Keypair::new();
    let ledger = ledger_key.pubkey();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &ledger_key,
        state::insurance_ledger_account_len(),
        env.program_id,
    );
    for (token, amount) in tokens
        .into_iter()
        .zip(CAPITAL)
        .chain([(reserve, FUNDED), (destination, BACKING)])
    {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &admin.pubkey(),
                &[],
                amount,
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
    for actor in 0..3 {
        env.send(
            env.deposit_ix(portfolios[actor], CAPITAL[actor].into()),
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[actor], false),
                AccountMeta::new(tokens[actor], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[actor]],
        )
        .unwrap();
    }
    env.send(
        ProgInstruction::TopUpInsuranceDomain {
            domain: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            intent_id: 0,
            amount: FUNDED.into(),
        },
        vec![
            AccountMeta::new(beneficiary_key, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(reserve, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&beneficiary],
    )
    .unwrap();
    env.send(
        ProgInstruction::TopUpBackingBucket {
            domain: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            intent_id: 0,
            backing_fee_bps: 0,
            insurance_share_bps: 0,
            amount: BACKING.into(),
            expiry_slot: EXPIRY,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    )
    .unwrap();
    let funded_ledger = env.svm.get_account(&ledger).unwrap();
    let funded_record = state::InsuranceLedgerAccountV16 {
        market_group: env.market.to_bytes(),
        authority: beneficiary_key.to_bytes(),
        total_principal_atoms: FUNDED.into(),
        total_deposited_atoms: FUNDED.into(),
        total_withdrawn_atoms: 0,
        cumulative_profit_atoms: 0,
        cumulative_loss_atoms: 0,
        last_observed_insurance_atoms: FUNDED.into(),
    };
    assert_eq!(
        state::read_insurance_ledger(&funded_ledger.data).unwrap(),
        funded_record
    );
    drop((beneficiary, operator, ledger_key));
    env.trade_asset_with_cu(
        0,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    for (offset, mark) in (105..=120).step_by(5).enumerate() {
        let slot = offset as u64 + 2;
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(0, slot, mark);
        env.crank(
            portfolios[2],
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    for actor in [0, 1] {
        env.crank(
            portfolios[actor],
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(
        env.portfolio_state(portfolios[1]).pnl.get(),
        -i128::from(LOSS)
    );
    assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), 200);
    assert_eq!(env.svm.get_account(&ledger), Some(funded_ledger.clone()));
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let mut tracked = vec![
        env.market,
        env.vault,
        env.mint,
        ledger,
        reserve,
        destination,
        beneficiary_key,
        operator_key,
        admin.pubkey(),
    ];
    tracked.extend(portfolios);
    tracked.extend(tokens);
    tracked.extend(owners.each_ref().map(Signer::pubkey));
    let mut peaks = [0u64; 4]; // user progress, rejection, insurance payment, cleanup
    env.svm.warp_to_slot(40);
    let resolve = wrap(
        &env,
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
    );
    let market = env.market;
    peaks[0] = land(&mut env, &[resolve], &[], &tracked, &[market], None, (1, 0));
    env.svm.warp_to_slot(43);
    let first = payment(&env, beneficiary_key, reserve, ledger, FIRST);
    let locked = Some((
        2,
        InstructionError::Custom(PercolatorError::EngineLockActive as u32),
    ));
    assert!(env.market_state().1.c_tot > 0);
    peaks[1] = land(
        &mut env,
        &[first.clone()],
        &[],
        &tracked,
        &[],
        locked.clone(),
        (0, 0),
    );
    for actor in [1, 0, 2] {
        let mut calls = 0;
        while !resolved_portfolio_is_terminal(&env, portfolios[actor]) {
            assert!(calls < 8);
            let payout = wrap(
                &env,
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(owners[actor].pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            // User settlement may be a bookkeeping step or a real token payment.
            let cu = env
                .send(
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    payout.accounts,
                    &[],
                )
                .unwrap();
            assert_cu_within("row421 user settlement", cu, 400_000);
            peaks[0] = peaks[0].max(cu);
            calls += 1;
            assert_eq!(env.svm.get_account(&ledger), Some(funded_ledger.clone()));
        }
        assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
    }
    assert_eq!(env.market_state().1.c_tot, 0);
    assert_eq!(env.market_state().1.materialized_portfolio_count, 3);
    assert_eq!(env.market_state().1.insurance_domain_spent[0], LOSS.into());
    peaks[1] = peaks[1].max(land(
        &mut env,
        &[first.clone()],
        &[],
        &tracked,
        &[],
        locked,
        (0, 0),
    ));
    for actor in 0..3 {
        let cu = env.close_portfolio_with_cu(&owners[actor], portfolios[actor]);
        assert_cu_within("row421 portfolio cleanup", cu, 400_000);
        peaks[3] = peaks[3].max(cu);
    }
    drop(owners);
    let initial_sequences = env.control_sequences(0);
    let check = |env: &V16CuEnv, paid: u64, recovered: u64, payments: u64| {
        let (_, group) = env.market_state();
        let insurance = FIRST + recovered - paid;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(
            (
                group.c_tot,
                group.pnl_pos_tot,
                group.materialized_portfolio_count
            ),
            (0, 0, 0)
        );
        assert_eq!(group.insurance, insurance.into());
        assert_eq!(
            group.insurance_domain_budget,
            vec![u128::from(FUNDED - paid), 0]
        );
        assert_eq!(
            group.insurance_domain_spent,
            vec![u128::from(LOSS - recovered), 0]
        );
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            insurance.into()
        );
        assert_eq!(group.vault, u128::from(BACKING + FIRST - paid));
        assert_eq!(env.token_amount(env.vault) as u128, group.vault);
        assert_eq!(env.token_amount(reserve), paid);
        assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
        assert_eq!(env.token_amount(destination), 0);
        assert_eq!(
            env.token_amount(env.vault) + paid + PAYOUTS.iter().sum::<u64>(),
            SUPPLY
        );
        let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
        assert_eq!((mint.supply, mint.mint_authority), (SUPPLY, COption::None));
        assert_eq!(
            group.source_credit[1].provider_receivable_num,
            u128::from(LOSS) * BOUND_SCALE
        );
        let mut market = env.svm.get_account(&env.market).unwrap();
        assert_eq!(
            state::read_asset_oracle_profile(&market.data, 0).unwrap(),
            profile
        );
        let mut sequences = initial_sequences;
        sequences.authority_epoch += payments;
        assert_eq!(env.control_sequences(0), sequences);
        let account = env.svm.get_account(&ledger).unwrap();
        let expected = state::InsuranceLedgerAccountV16 {
            total_principal_atoms: u128::from(FUNDED - paid),
            total_withdrawn_atoms: paid.into(),
            cumulative_loss_atoms: LOSS.into(),
            cumulative_profit_atoms: recovered.into(),
            last_observed_insurance_atoms: insurance.into(),
            ..funded_record
        };
        assert_eq!(
            state::read_insurance_ledger(&account.data).unwrap(),
            expected
        );
        let mut metadata = account;
        metadata.data.clone_from(&funded_ledger.data);
        assert_eq!(metadata, funded_ledger);
        assert_market_stock_census(
            "row421 loss/recredit ledger",
            &group,
            &market.data,
            &[],
            group.vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("row421 loss/recredit ledger", &group, &[]).unwrap();
        let (_, view) = state::market_view_mut(&mut market.data).unwrap();
        view.validate_shape().unwrap();
    };
    let changed = [env.market, env.vault, reserve, ledger];
    peaks[2] = land(&mut env, &[first], &[], &tracked, &changed, None, (1, 1));
    check(&env, FIRST, 0, 1);
    assert_eq!(
        env.market_state().1.source_backing_buckets[0].status,
        BackingBucketStatusV16::Fresh
    );
    let premature = payment(&env, beneficiary_key, reserve, ledger, 1);
    peaks[1] = peaks[1].max(land(
        &mut env,
        &[premature],
        &[],
        &tracked,
        &[],
        Some((
            2,
            InstructionError::Custom(PercolatorError::EngineLockActive as u32),
        )),
        (0, 0),
    ));
    check(&env, FIRST, 0, 1);
    env.svm.warp_to_slot(EXPIRY);
    let normalize = close(&env, destination);
    peaks[3] = peaks[3].max(land(
        &mut env,
        &[normalize],
        &[&admin],
        &tracked,
        &[market],
        None,
        (1, 0),
    ));
    check(&env, FIRST, 0, 1);
    assert_eq!(
        env.market_state().1.source_backing_buckets[0].status,
        BackingBucketStatusV16::Expired
    );
    assert_eq!(
        env.market_state().1.source_credit[0].fresh_reserved_backing_num,
        0
    );
    // Recredit and the ledger's profit observation occur in the same unsigned call.
    // A denied administrative suffix must restore both, preserving the earlier loss/payment.
    let recovered_prefix = payment(&env, beneficiary_key, reserve, ledger, 41);
    let mut denied_close = close(&env, destination);
    denied_close.accounts[0].is_signer = false;
    peaks[1] = peaks[1].max(land(
        &mut env,
        &[recovered_prefix.clone(), denied_close],
        &[],
        &tracked,
        &[],
        Some((
            3,
            InstructionError::Custom(PercolatorError::ExpectedSigner as u32),
        )),
        (1, 1),
    ));
    check(&env, FIRST, 0, 1);
    peaks[2] = peaks[2].max(land(
        &mut env,
        &[recovered_prefix],
        &[],
        &tracked,
        &changed,
        None,
        (1, 1),
    ));
    check(&env, FIRST + 41, LOSS, 2);
    let tail = payment(&env, beneficiary_key, reserve, ledger, LOSS - 41);
    peaks[2] = peaks[2].max(land(
        &mut env,
        &[tail],
        &[],
        &tracked,
        &changed,
        None,
        (1, 1),
    ));
    check(&env, FUNDED, LOSS, 3);
    let overclaim = payment(&env, beneficiary_key, reserve, ledger, 1);
    peaks[1] = peaks[1].max(land(
        &mut env,
        &[overclaim],
        &[],
        &tracked,
        &[],
        Some((
            2,
            InstructionError::Custom(PercolatorError::EngineLockActive as u32),
        )),
        (0, 0),
    ));
    check(&env, FUNDED, LOSS, 3);
    let paid_ledger = env.svm.get_account(&ledger);
    let paid_reserve = env.svm.get_account(&reserve);
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
    expected_admin.lamports += env.svm.get_account(&env.market).unwrap().lamports
        + env.svm.get_account(&env.vault).unwrap().lamports
        - tombstone_rent;
    let retire = close(&env, destination);
    let close_changes = [env.market, env.vault, env.mint, admin.pubkey()];
    peaks[3] = peaks[3].max(land(
        &mut env,
        &[retire],
        &[&admin],
        &tracked,
        &close_changes,
        None,
        (1, 2),
    ));
    assert_eq!(env.svm.get_account(&ledger), paid_ledger);
    assert_eq!(env.svm.get_account(&reserve), paid_reserve);
    assert!(env
        .svm
        .get_account(&env.vault)
        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply,
        SUPPLY - (BACKING - LOSS)
    );
    println!("row421 loss/recredit ledger: paid={FUNDED}, loss={LOSS}, profit={LOSS}, rollbacks=5, peak_CU(user,rejection,payment,cleanup)={peaks:?}");
}
