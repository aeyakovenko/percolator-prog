//! INV-082: missing payout custody is a keeper-constructible environmental prerequisite.
//! Public SPL closure removes the destination; no owner signs the recovery suffix.

use super::super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

#[path = "inv_082_shared_destination_recovery.rs"]
mod shared_destination_recovery;

const DEPOSITS: [u64; 2] = [101, 37];
const RESOLVE_SLOT: u64 = 100;
const EXIT_DELAY: u64 = 5;

fn account_is_closed(env: &V16CuEnv, key: Pubkey) -> bool {
    env.svm.get_account(&key).map_or(true, |account| {
        account.lamports == 0
            && account.data.is_empty()
            && account.owner == solana_sdk::system_program::ID
    })
}

fn keeper_step(
    env: &mut V16CuEnv,
    ixs: &[Instruction],
    tracked: &[Pubkey],
    allowed: &[Pubkey],
    rent: u64,
    rejection: Option<(u8, PercolatorError)>,
) -> u64 {
    env.svm.expire_blockhash();
    let mut instructions = vec![heap_ix(), cu_ix()];
    instructions.extend_from_slice(ixs);
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    assert_eq!(tx.message.header.num_required_signatures, 1);
    assert_eq!(tx.message.account_keys[0], env.payer.pubkey());
    let mut keys = tx.message.account_keys.clone();
    keys.extend_from_slice(tracked);
    keys.sort_unstable();
    keys.dedup();
    keys.retain(|key| *key != env.payer.pubkey());
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= solana_sdk::fee::FeeStructure::default().lamports_per_signature;
    let result = env.svm.send_transaction(tx);
    let rejected = rejection.is_some();
    let meta = if let Some((index, error)) = rejection {
        let failed = result.expect_err("environmental barrier must reject atomically");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
        );
        for ix in &ixs[..usize::from(index) - 2] {
            assert!(failed
                .meta
                .logs
                .contains(&format!("Program {} success", ix.program_id)));
        }
        failed.meta
    } else {
        payer.lamports -= rent;
        result.expect("keeper-only environmental continuation")
    };
    for (key, account) in keys.iter().zip(before) {
        if rejected || !allowed.contains(key) {
            assert_eq!(env.svm.get_account(key), account, "exact frame: {key}");
        }
    }
    assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer));
    assert_cu_within(
        "INV-082 destination recovery",
        meta.compute_units_consumed,
        300_000,
    );
    meta.compute_units_consumed
}

fn assert_entitlements(env: &V16CuEnv, portfolios: [Pubkey; 2], destinations: [Pubkey; 2]) -> u128 {
    let market = env.market_state().1;
    let mut unpaid = 0;
    for i in 0..2 {
        let account = env.portfolio_state(portfolios[i]);
        let paid = env
            .svm
            .get_account(&destinations[i])
            .filter(|token| token.lamports != 0)
            .map_or(0, |token| TokenAccount::unpack(&token.data).unwrap().amount);
        assert_eq!(
            account.capital.get() + u128::from(paid),
            u128::from(DEPOSITS[i])
        );
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(account.reserved_pnl.get(), 0);
        assert_eq!(account.fee_credits.get(), 0);
        assert_eq!(account.cancel_deposit_escrow.get(), 0);
        assert_eq!(account.stale_state, 0);
        assert_eq!(account.b_stale_state, 0);
        assert_eq!(account.rebalance_lock, 0);
        assert_eq!(account.liquidation_lock, 0);
        assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
        assert!(account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied()));
        assert!(!close_progress(&account).active);
        assert!(!resolved_receipt(&account).present);
        if paid != 0 {
            assert!(resolved_portfolio_is_terminal(env, portfolios[i]));
        }
        unpaid += account.capital.get();
    }
    assert_eq!(market.materialized_portfolio_count, 2);
    assert_eq!(market.c_tot, unpaid);
    assert_eq!(market.vault, unpaid);
    assert_eq!(u128::from(env.token_amount(env.vault)), unpaid);
    assert_eq!(market.insurance, 0);
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply,
        138
    );
    unpaid
}

#[test]
fn v16_program_missing_destination_has_keeper_only_terminal_recovery() {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;

    let mut max_cu = 0;
    for use_crank in [false, true] {
        let mut env = inv018_public_spl_market(6);
        env.configure_permissionless_resolve_with_cu(RESOLVE_SLOT, EXIT_DELAY);
        let owners = [Keypair::new(), Keypair::new()];
        let mut portfolios = [Pubkey::default(); 2];
        let mut destinations = [Pubkey::default(); 2];
        let portfolio_len = state::portfolio_account_len_for_market_slots(
            env.market_state().1.config.max_market_slots as usize,
        )
        .unwrap();
        for i in 0..2 {
            env.svm.airdrop(&owners[i].pubkey(), 1_000_000_000).unwrap();
            let key = Keypair::new();
            portfolios[i] = key.pubkey();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                portfolio_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                ],
                &[&owners[i]],
            )
            .unwrap();
            env.portfolios.push(portfolios[i]);
            destinations[i] =
                create_ata_for_test(&mut env.svm, &env.payer, owners[i].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &destinations[i],
                    &env.admin.pubkey(),
                    &[],
                    DEPOSITS[i],
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[i], DEPOSITS[i].into()),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(destinations[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .unwrap();
        }
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::close_account(
                &spl_token::ID,
                &destinations[0],
                &owners[0].pubkey(),
                &owners[0].pubkey(),
                &[],
            )
            .unwrap(),
            &[&owners[0]],
        )
        .expect("owner publicly closes the now-empty deposit ATA");
        for owner in &owners {
            let lamports = env.svm.get_account(&owner.pubkey()).unwrap().lamports;
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                system_instruction::transfer(&owner.pubkey(), &env.payer.pubkey(), lamports),
                &[owner],
            )
            .unwrap();
            assert!(account_is_closed(&env, owner.pubkey()));
        }
        let owner_keys = owners.map(|owner| owner.pubkey());
        assert!(!owner_keys.contains(&env.payer.pubkey()));
        assert_ne!(env.payer.pubkey(), env.admin.pubkey());
        assert!(account_is_closed(&env, destinations[0]));
        let tracked = [
            env.market,
            env.mint,
            env.vault,
            env.admin.pubkey(),
            owner_keys[0],
            owner_keys[1],
            portfolios[0],
            portfolios[1],
            destinations[0],
            destinations[1],
        ];
        let payouts: [Instruction; 2] = std::array::from_fn(|i| Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(owner_keys[i], false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[i], false),
                AccountMeta::new(destinations[i], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: if use_crank == (i == 0) {
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: vec![],
                }
            } else {
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
            }
            .encode(),
        });
        let create = Instruction {
            program_id: associated_token_program_id(),
            accounts: vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(destinations[0], false),
                AccountMeta::new_readonly(owner_keys[0], false),
                AccountMeta::new_readonly(env.mint, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
            ],
            data: vec![],
        };
        let resolve = Instruction {
            program_id: env.program_id,
            accounts: vec![AccountMeta::new(env.market, false)],
            data: ProgInstruction::ResolveStalePermissionless { now_slot: 0 }.encode(),
        };
        env.svm.warp_to_slot(RESOLVE_SLOT);
        let market_key = env.market;
        assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
        max_cu = max_cu.max(keeper_step(
            &mut env,
            &[resolve],
            &tracked,
            &[market_key],
            0,
            None,
        ));
        assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
        assert_eq!(env.market_state().1.resolved_slot, RESOLVE_SLOT);
        assert_eq!(assert_entitlements(&env, portfolios, destinations), 138);

        // The owner-window error occurs after ATA creation, so even its rent must roll back.
        env.svm.warp_to_slot(RESOLVE_SLOT + EXIT_DELAY - 1);
        let repair_and_pay = [create, payouts[0].clone()];
        max_cu = max_cu.max(keeper_step(
            &mut env,
            &repair_and_pay,
            &tracked,
            &[],
            0,
            Some((3, PercolatorError::ExpectedSigner)),
        ));
        assert!(account_is_closed(&env, destinations[0]));
        env.svm.warp_to_slot(RESOLVE_SLOT + EXIT_DELAY);
        max_cu = max_cu.max(keeper_step(
            &mut env,
            &payouts[..1],
            &tracked,
            &[],
            0,
            Some((2, PercolatorError::InvalidTokenAccount)),
        ));
        assert_eq!(assert_entitlements(&env, portfolios, destinations), 138);

        let vault = env.vault;
        max_cu = max_cu.max(keeper_step(
            &mut env,
            &payouts[1..],
            &tracked,
            &[market_key, vault, portfolios[1], destinations[1]],
            0,
            None,
        ));
        assert_eq!(assert_entitlements(&env, portfolios, destinations), 101);
        assert!(!resolved_portfolio_is_terminal(&env, portfolios[0]));
        assert!(account_is_closed(&env, destinations[0]));
        let rent = env
            .svm
            .minimum_balance_for_rent_exemption(TokenAccount::LEN);
        max_cu = max_cu.max(keeper_step(
            &mut env,
            &repair_and_pay,
            &tracked,
            &[market_key, vault, portfolios[0], destinations[0]],
            rent,
            None,
        ));
        assert_eq!(assert_entitlements(&env, portfolios, destinations), 0);
        let repaired = env.svm.get_account(&destinations[0]).unwrap();
        assert_eq!(repaired.lamports, rent);
        assert_eq!(repaired.owner, spl_token::ID);
        let token = TokenAccount::unpack(&repaired.data).unwrap();
        assert_eq!(token.owner, owner_keys[0]);
        assert_eq!(token.mint, env.mint);
        assert_eq!(token.delegate, COption::None);
        assert_eq!(token.close_authority, COption::None);
        assert!(owner_keys.iter().all(|key| account_is_closed(&env, *key)));
        for payout in &payouts {
            max_cu = max_cu.max(keeper_step(
                &mut env,
                std::slice::from_ref(payout),
                &tracked,
                &[],
                0,
                Some((2, PercolatorError::EngineNonProgress)),
            ));
        }
        assert_eq!(assert_entitlements(&env, portfolios, destinations), 0);
    }
    eprintln!("INV-082 missing destination: 2 worlds, 14 keeper transactions, 6 successes, 8 exact rollbacks, 4 exact owner payouts; max CU={max_cu}");
}
