//! INV-021: public allocation and repeated reincarnation composed with funded SPL exits.
//! A late SPL rejection restores the entire successful lifecycle prefix, including rent.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn withdraw_ix(
    env: &V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    token: Pubkey,
    amount: u128,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: env.withdraw_ix(portfolio, amount).encode(),
    }
}

fn reject_suffix_then_commit(
    env: &mut V16CuEnv,
    prefix: Vec<Instruction>,
    suffix: Instruction,
    signers: &[&Keypair],
    fixture_keys: &[Pubkey],
) -> u64 {
    env.svm.expire_blockhash();
    let mut signing = vec![&env.payer];
    signing.extend_from_slice(signers);
    let positive = Transaction::new_signed_with_payer(
        &prefix,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    let mut instructions = prefix.clone();
    instructions.push(suffix);
    let rejected_tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signing,
        env.svm.latest_blockhash(),
    );
    let mut keys = fixture_keys.to_vec();
    keys.extend(rejected_tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    keys.retain(|key| *key != env.payer.pubkey());
    let snapshot = |env: &V16CuEnv| {
        keys.iter()
            .map(|key| env.svm.get_account(key))
            .collect::<Vec<_>>()
    };
    let before = snapshot(env);
    let payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
    env.svm
        .simulate_transaction(positive.clone().into())
        .expect(
            "the complete lifecycle/SPL-paying prefix must succeed before testing its rollback",
        );
    assert_eq!(snapshot(env), before, "simulation cannot commit the prefix");
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        payer_before
    );

    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(rejected_tx.message.header.num_required_signatures);
    let rejected = env
        .svm
        .send_transaction(rejected_tx)
        .expect_err("late SPL rejection");
    assert_eq!(
        rejected.err,
        TransactionError::InstructionError(
            prefix.len() as u8,
            InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32),
        ),
        "rejection must come from the appended SPL transfer, after every lifecycle instruction"
    );
    assert_eq!(
        snapshot(env),
        before,
        "all account bytes, lengths, rent and metadata roll back"
    );
    let mut expected_payer = payer_before;
    expected_payer.lamports -= fee;
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        expected_payer
    );

    let accepted = env
        .svm
        .send_transaction(positive)
        .expect("identical prefix remains live on retry");
    expected_payer.lamports -= fee;
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        expected_payer
    );
    let peak = rejected
        .meta
        .compute_units_consumed
        .max(accepted.compute_units_consumed);
    assert_cu_within(
        "INV-021 composed lifecycle and exit",
        peak,
        2 * CUSTODY_CU_LIMIT,
    );
    peak
}

fn assert_token_frame(env: &V16CuEnv, key: Pubkey, before: &Account, amount: u64) {
    let after = env.svm.get_account(&key).unwrap();
    assert_eq!(after.lamports, before.lamports);
    assert_eq!(after.owner, before.owner);
    assert_eq!(after.executable, before.executable);
    assert_eq!(after.rent_epoch, before.rent_epoch);
    assert_eq!(after.data.len(), before.data.len());
    let mut expected = TokenAccount::unpack(&before.data).unwrap();
    expected.amount = amount;
    assert_eq!(TokenAccount::unpack(&after.data).unwrap(), expected);
}

fn assert_clean_incarnation(env: &V16CuEnv, key: Pubkey, owner: Pubkey, id: u64, rent: u64) {
    let account = env.svm.get_account(&key).unwrap();
    assert_eq!(account.owner, env.program_id);
    assert!(!account.executable);
    assert_eq!(account.data.len(), env.portfolio_account_len);
    assert_eq!(account.lamports, rent);
    assert!(env
        .svm
        .get_sysvar::<solana_sdk::rent::Rent>()
        .is_exempt(rent, account.data.len()));
    assert_eq!(env.portfolio_id(key), id);
    assert_eq!(env.portfolio_matcher_sequence(key), 0);
    assert_eq!(env.portfolio_position_epoch(key), 0);
    assert_eq!(
        state::read_portfolio_matcher_expiry(&account.data).unwrap(),
        0
    );
    assert!(bytemuck::bytes_of(&env.portfolio_matcher_config(key))
        .iter()
        .all(|byte| *byte == 0));
    let portfolio = env.portfolio_state(key);
    assert_eq!(portfolio.owner, owner.to_bytes());
    assert_eq!(portfolio.capital.get(), 0);
    assert_eq!(portfolio.pnl.get(), 0);
    assert_eq!(portfolio.reserved_pnl.get(), 0);
    assert_eq!(portfolio.fee_credits.get(), 0);
    assert_eq!(portfolio.cancel_deposit_escrow.get(), 0);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &portfolio
    )));
    assert!(portfolio
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));
    assert!(!resolved_receipt(&portfolio).present);
    assert!(close_progress(&portfolio).is_empty());
    for slot in 0..portfolio.legs.len() {
        assert_eq!(leg(&portfolio, slot), percolator::PortfolioLegV16::EMPTY);
    }
}

#[test]
fn v16_program_funded_lifecycle_spl_suffix_rollback_preserves_claim_and_rent() {
    const CAPITAL: u128 = 1_000;
    const OPEN: u64 = 100;
    const MARK: u64 = 105;
    const LOTS: u128 = 10;
    const FACE: u128 = LOTS * (MARK - OPEN) as u128;
    const BACKING: u128 = 17;
    const FUNDS: [u128; 5] = [CAPITAL, CAPITAL, 7, 11, 0];
    const SUPPLY: u128 = 2 * CAPITAL + BACKING + FUNDS[2] + FUNDS[3];
    let mut peak_cu = 0;
    for surplus in [0, 1] {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN);
        let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
        for owner in &owners {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        }
        let tokens = owners
            .each_ref()
            .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
        let provider = create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
        for (token, amount) in tokens.into_iter().zip(FUNDS).chain([(provider, BACKING)]) {
            if amount != 0 {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &token,
                        &env.admin.pubkey(),
                        &[],
                        amount as u64,
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .unwrap();
            }
        }
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        let peers = [0, 1].map(|actor| {
            let key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                env.portfolio_account_len,
                env.program_id,
            );
            let init = inv021_init_portfolio_ix(&env, owners[actor].pubkey(), key.pubkey());
            send_raw_tx(&mut env.svm, &env.payer, init, &[&owners[actor]]).unwrap();
            key.pubkey()
        });
        let deposit = |env: &mut V16CuEnv, actor: usize, portfolio: Pubkey| {
            env.send(
                env.deposit_ix(portfolio, FUNDS[actor]),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .expect("deposit finite publicly minted funds");
        };
        deposit(&mut env, 0, peers[0]);
        deposit(&mut env, 1, peers[1]);
        env.top_up_backing_bucket_from_admin_token_with_cu(provider, 1, BACKING, 20);
        env.trade_with_cu(
            &owners[0],
            peers[0],
            &owners[1],
            peers[1],
            (LOTS * POS_SCALE) as i128,
            OPEN,
            0,
        );
        env.svm.warp_to_slot(2);
        env.push_auth_mark_for_asset_as_admin(0, 2, MARK);
        for portfolio in [peers[1], peers[0]] {
            env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations(0),
                },
            );
        }
        env.trade_with_cu(
            &owners[0],
            peers[0],
            &owners[1],
            peers[1],
            -((LOTS * POS_SCALE) as i128),
            MARK,
            0,
        );
        assert_eq!(env.portfolio_state(peers[0]).pnl.get(), FACE as i128);
        assert_eq!(env.portfolio_state(peers[1]).capital.get(), CAPITAL - FACE);
        let (initial_config, initial) = env.market_state();
        assert_eq!(initial.source_claim_bound_total_num, FACE * BOUND_SCALE);
        assert_eq!(
            initial.source_backing_buckets[1].fresh_unliened_backing_num,
            (BACKING + FACE) * BOUND_SCALE
        );
        let claimant = env.svm.get_account(&peers[0]).unwrap();
        let claim = env.portfolio_state(peers[0]);
        assert_eq!(claim.capital.get(), CAPITAL);
        assert!(percolator::active_bitmap_is_empty(active_bitmap(&claim)));
        let sources: Vec<_> = claim
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .collect();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].domain.get(), 1);
        assert_eq!(sources[0].source_claim_bound_num.get(), FACE * BOUND_SCALE);
        let mint = env.svm.get_account(&env.mint).unwrap();
        assert_eq!(Mint::unpack(&mint.data).unwrap().supply as u128, SUPPLY);
        assert_eq!(
            Mint::unpack(&mint.data).unwrap().mint_authority,
            COption::None
        );
        let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
        let vault_frame = env.svm.get_account(&env.vault).unwrap();
        let provider_frame = env.svm.get_account(&provider).unwrap();
        let owner_frames = owners
            .each_ref()
            .map(|owner| env.svm.get_account(&owner.pubkey()).unwrap());
        let admin_frame = env.svm.get_account(&env.admin.pubkey()).unwrap();
        let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
        let exact_rent = env
            .svm
            .minimum_balance_for_rent_exemption(env.portfolio_account_len);
        let key = Keypair::new();
        let portfolio = key.pubkey();
        let mut keys = vec![
            env.market,
            env.mint,
            env.vault,
            env.vault_authority,
            provider,
            env.admin.pubkey(),
            portfolio,
        ];
        keys.extend(peers);
        keys.extend(tokens);
        keys.extend(owners.each_ref().map(|owner| owner.pubkey()));
        let overspend = |actor: usize, amount: u128| {
            spl_token::instruction::transfer(
                &spl_token::ID,
                &tokens[actor],
                &provider,
                &owners[actor].pubkey(),
                &[],
                (amount + 1) as u64,
            )
            .unwrap()
        };
        let assert_survivor =
            |env: &V16CuEnv, live: u64, capital: u128, swept: u64, creates: u64| {
                assert_eq!(env.svm.get_account(&peers[0]).unwrap(), claimant);
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint);
                assert_eq!(
                    env.svm.get_account(&env.admin.pubkey()).unwrap(),
                    admin_frame
                );
                let mut expected = initial.clone();
                expected.materialized_portfolio_count = 2 + live;
                expected.c_tot = CAPITAL + capital;
                expected.vault = CAPITAL + FACE + BACKING + capital;
                assert_eq!(
                    env.market_state().1,
                    expected,
                    "only count, principal and custody may change around the surviving claim"
                );
                let mut expected_config = initial_config;
                expected_config.next_portfolio_id += creates;
                assert_eq!(env.market_state().0, expected_config);
                assert_eq!(
                    env.svm.get_account(&env.market).unwrap().lamports,
                    market_rent + swept
                );
                assert_token_frame(env, env.vault, &vault_frame, expected.vault as u64);
                assert_token_frame(env, provider, &provider_frame, 0);
                assert_eq!(
                    env.token_amount(env.vault) as u128
                        + tokens
                            .iter()
                            .map(|key| env.token_amount(*key) as u128)
                            .sum::<u128>(),
                    SUPPLY
                );
            };

        // Growth and a peer's real funded exit both precede the failed SPL suffix. Rent is
        // sponsored by a nonpayer so the exact fee exception cannot hide a wrong destination.
        let initial_id = initial_config.next_portfolio_id;
        let mut portfolio_rent = exact_rent + surplus;
        let prefix = vec![
            heap_ix(),
            cu_ix(),
            system_instruction::create_account(
                &owners[2].pubkey(),
                &portfolio,
                portfolio_rent,
                (env.portfolio_account_len / 3) as u64,
                &env.program_id,
            ),
            inv021_init_portfolio_ix(&env, owners[2].pubkey(), portfolio),
            withdraw_ix(
                &env,
                owners[1].pubkey(),
                peers[1],
                tokens[1],
                CAPITAL - FACE,
            ),
        ];
        peak_cu = peak_cu.max(reject_suffix_then_commit(
            &mut env,
            prefix,
            overspend(1, CAPITAL - FACE),
            &[&owners[2], &key, &owners[1]],
            &keys,
        ));
        assert_clean_incarnation(
            &env,
            portfolio,
            owners[2].pubkey(),
            initial_id,
            portfolio_rent,
        );
        assert_eq!(env.market_state().0.next_portfolio_id, initial_id + 1);
        assert_eq!(env.portfolio_state(peers[1]).capital.get(), 0);
        assert_token_frame(&env, tokens[1], &token_frames[1], (CAPITAL - FACE) as u64);
        let exited_peer = env.svm.get_account(&peers[1]).unwrap();
        let mut rent_debits = [0u64; 5];
        rent_debits[2] = portfolio_rent;
        let assert_owners = |env: &V16CuEnv, debits: [u64; 5]| {
            for actor in 0..5 {
                let mut expected = owner_frames[actor].clone();
                expected.lamports -= debits[actor];
                assert_eq!(
                    env.svm.get_account(&owners[actor].pubkey()).unwrap(),
                    expected,
                    "rent never pays a portfolio owner"
                );
            }
        };
        assert_survivor(&env, 1, 0, 0, 1);
        assert_owners(&env, rent_debits);

        let mut swept = 0;
        for actor in [2, 3] {
            deposit(&mut env, actor, portfolio);
            assert_survivor(&env, 1, FUNDS[actor], swept, (actor - 1) as u64);
            let next_id = initial_id + (actor - 1) as u64;
            let replacement_rent = exact_rent + (1 - surplus);
            let prefix = vec![
                heap_ix(),
                cu_ix(),
                withdraw_ix(
                    &env,
                    owners[actor].pubkey(),
                    portfolio,
                    tokens[actor],
                    FUNDS[actor],
                ),
                Instruction {
                    program_id: env.program_id,
                    accounts: inv021_init_portfolio_ix(&env, owners[actor].pubkey(), portfolio)
                        .accounts,
                    data: ProgInstruction::ClosePortfolio {
                        portfolio_id: env.portfolio_id(portfolio),
                        expected_sequence: env.portfolio_matcher_sequence(portfolio) + 1,
                        position_epoch: env.portfolio_position_epoch(portfolio),
                    }
                    .encode(),
                },
                system_instruction::transfer(
                    &owners[actor + 1].pubkey(),
                    &portfolio,
                    replacement_rent,
                ),
                inv021_init_portfolio_ix(&env, owners[actor + 1].pubkey(), portfolio),
            ];
            peak_cu = peak_cu.max(reject_suffix_then_commit(
                &mut env,
                prefix,
                overspend(actor, FUNDS[actor]),
                &[&owners[actor], &owners[actor + 1]],
                &keys,
            ));
            swept += portfolio_rent;
            portfolio_rent = replacement_rent;
            rent_debits[actor + 1] += replacement_rent;
            assert_clean_incarnation(
                &env,
                portfolio,
                owners[actor + 1].pubkey(),
                next_id,
                portfolio_rent,
            );
            assert_eq!(env.market_state().0.next_portfolio_id, next_id + 1);
            assert_survivor(&env, 1, 0, swept, actor as u64);
            assert_owners(&env, rent_debits);
            assert_eq!(env.svm.get_account(&peers[1]).unwrap(), exited_peer);
            for index in 0..5 {
                let amount = if index == 1 {
                    CAPITAL - FACE
                } else if index == 0 {
                    0
                } else {
                    FUNDS[index]
                };
                assert_token_frame(&env, tokens[index], &token_frames[index], amount as u64);
            }
        }
        env.close_portfolio_with_cu(&owners[4], portfolio);
        swept += portfolio_rent;
        let closed = env.svm.get_account(&portfolio).unwrap();
        assert_eq!(closed.lamports, 0);
        assert!(closed.data.is_empty());
        assert_survivor(&env, 0, 0, swept, 3);
        assert_owners(&env, rent_debits);

        env.convert_released_pnl_with_cu(&owners[0], peers[0], FACE);
        let exit = withdraw_ix(
            &env,
            owners[0].pubkey(),
            peers[0],
            tokens[0],
            CAPITAL + FACE,
        );
        send_raw_tx(&mut env.svm, &env.payer, exit, &[&owners[0]])
            .expect("surviving principal and claim exit in full");
        assert_token_frame(&env, tokens[0], &token_frames[0], (CAPITAL + FACE) as u64);
        assert_token_frame(&env, env.vault, &vault_frame, BACKING as u64);
        assert_eq!(env.svm.get_account(&peers[1]).unwrap(), exited_peer);
        assert_eq!(env.svm.get_account(&portfolio).unwrap(), closed);
        let paid = env.portfolio_state(peers[0]);
        assert_eq!(paid.capital.get(), 0);
        assert_eq!(paid.pnl.get(), 0);
        assert!(paid
            .source_domains
            .iter()
            .all(|source| !source.is_occupied()));
        let group = env.market_state().1;
        assert_eq!(group.c_tot, 0);
        assert_eq!(group.pnl_pos_tot, 0);
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert_eq!(group.vault, BACKING);
        assert_eq!(group.materialized_portfolio_count, 2);
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().lamports,
            market_rent + swept
        );
        assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint);
        assert_owners(&env, rent_debits);
        assert_eq!(
            tokens.map(|key| env.token_amount(key)),
            [1_050, 950, 7, 11, 0]
        );
        assert_eq!(
            env.token_amount(env.vault) as u128
                + tokens
                    .iter()
                    .map(|key| env.token_amount(*key) as u128)
                    .sum::<u128>(),
            SUPPLY
        );
    }
    println!("INV-021: 2 rent worlds / 6 late SPL rejections / 6 identical-prefix retries; payouts=[1050,950,7,11,0], residual=17; peak composed CU={peak_cu}");
}
