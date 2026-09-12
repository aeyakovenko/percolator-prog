//! Row 417: Clock expiry is not stock normalization. Retained claims can run on either
//! side of the actual reclassification, including within one atomic transaction.
//! Terminal disposition and shared-owner receipts are covered by sibling modules.

use super::{late_expiry::World, *};

const CLAIMANTS: [usize; 2] = [0, 4];
const FACES: [u128; 2] = [700, 1_300];
const TOTAL_FACE: u128 = 3_000;
const INITIAL_RESIDUAL: u128 = 501;
const RELEASE: u128 = 100 + 250;
const NORMALIZE: usize = 2;
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, NORMALIZE],
    [1, 0, NORMALIZE],
    [0, NORMALIZE, 1],
    [1, NORMALIZE, 0],
    [NORMALIZE, 0, 1],
    [NORMALIZE, 1, 0],
];

fn entitlement(claimant: usize, residual: u128) -> u128 {
    FACES[claimant] * residual / TOTAL_FACE
}

struct ClaimIdentity {
    receipt: ResolvedPayoutReceiptV16,
    portfolio_id: u64,
    position_epoch: u64,
    provenance: (percolator::ProvenanceHeaderV16, [u8; 32]),
}

struct Prefix {
    identities: [ClaimIdentity; 2],
    paid: [u128; 2],
    normalized: bool,
}

impl Prefix {
    fn new(world: &World) -> Self {
        Self {
            identities: CLAIMANTS.map(|actor| {
                let portfolio = world.actors[actor].portfolio;
                ClaimIdentity {
                    receipt: world.receipt(actor),
                    portfolio_id: world.env.portfolio_id(portfolio),
                    position_epoch: world.env.portfolio_position_epoch(portfolio),
                    provenance: state::read_portfolio_owner_preflight(
                        &world.env.svm.get_account(&portfolio).unwrap().data,
                    )
                    .unwrap(),
                }
            }),
            paid: [0, 1].map(|index| entitlement(index, INITIAL_RESIDUAL)),
            normalized: false,
        }
    }

    fn residual(&self) -> u128 {
        INITIAL_RESIDUAL + if self.normalized { RELEASE } else { 0 }
    }

    // The oracle consumes instruction order and fixed public inputs, never an observed rate.
    fn apply(&mut self, action: usize) -> u128 {
        if action == NORMALIZE {
            assert!(!self.normalized);
            self.normalized = true;
            return 0;
        }
        let target = entitlement(action, self.residual());
        let due = target.checked_sub(self.paid[action]).unwrap();
        self.paid[action] = target;
        due
    }

    fn check(&self, world: &World) {
        let group = world.env.market_state().1;
        let ledger = group.resolved_payout_ledger;
        assert_eq!(ledger.snapshot_slot, 12);
        assert_eq!(ledger.snapshot_residual, self.residual());
        assert_eq!(
            ledger.current_payout_rate_num,
            self.residual() * BOUND_SCALE
        );
        assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num,
            2_000 * BOUND_SCALE
        );
        assert_eq!(
            ledger.terminal_claim_bound_unreceipted_num,
            1_000 * BOUND_SCALE
        );
        assert!(!ledger.payout_halted && !ledger.finalized);
        assert_eq!(
            group.source_credit[3].fresh_reserved_backing_num,
            if self.normalized {
                0
            } else {
                RELEASE * BOUND_SCALE
            }
        );
        assert_eq!(
            group.source_backing_buckets[3].status,
            if self.normalized {
                BackingBucketStatusV16::Expired
            } else {
                BackingBucketStatusV16::Fresh
            }
        );
        // All other owners and the provider retain their exact setup-derived balances.
        // The remaining winner's 1,000 capital plus 851 terminal stock stays in custody,
        // less only the independently computed payments to these two receipt holders.
        assert_eq!(group.c_tot, 1_000);
        assert_eq!(group.insurance, 0);
        assert_eq!(
            group.vault,
            1_000 + INITIAL_RESIDUAL + RELEASE - self.paid.iter().sum::<u128>()
        );
        assert_eq!(group.materialized_portfolio_count, 5);
        for actor in [1, 2, 3] {
            assert_eq!(world.env.token_amount(world.actors[actor].token), 0);
        }
        assert_eq!(world.env.token_amount(world.provider_token), 1);
        assert!(!world.receipt(2).present);
        assert_eq!(
            world
                .env
                .portfolio_state(world.actors[2].portfolio)
                .pnl
                .get(),
            1_000
        );
        for (index, actor) in CLAIMANTS.into_iter().enumerate() {
            let identity = &self.identities[index];
            let mut receipt = identity.receipt;
            receipt.paid_effective = self.paid[index];
            assert_eq!(world.receipt(actor), receipt, "claim identity {actor}");
            assert!(receipt.present && !receipt.finalized);
            assert_eq!(receipt.terminal_positive_claim_face, FACES[index]);
            assert_eq!(
                receipt.prior_bound_contribution_num,
                FACES[index] * BOUND_SCALE
            );
            assert_eq!(receipt.live_released_face_at_receipt, 0);
            let portfolio = world.actors[actor].portfolio;
            assert_eq!(world.env.portfolio_id(portfolio), identity.portfolio_id);
            assert_eq!(
                world.env.portfolio_position_epoch(portfolio),
                identity.position_epoch
            );
            assert_eq!(
                state::read_portfolio_owner_preflight(
                    &world.env.svm.get_account(&portfolio).unwrap().data
                )
                .unwrap(),
                identity.provenance
            );
            assert_eq!(
                world.env.portfolio_state(portfolio).owner,
                world.actors[actor].owner.pubkey().to_bytes()
            );
            assert_eq!(
                u128::from(world.env.token_amount(world.actors[actor].token)),
                1_000 + self.paid[index]
            );
        }
        world.custody();
    }
}

fn land(world: &mut World, oracle: &mut Prefix, retained: &[Instruction; 3], actions: &[usize]) {
    let before = world.frame();
    let mut allowed = vec![world.env.market];
    let mut transfers = 0;
    for &action in actions {
        if oracle.apply(action) != 0 {
            transfers += 1;
            allowed.extend([world.env.vault, world.actors[CLAIMANTS[action]].token]);
        }
        allowed.push(
            world.actors[if action == NORMALIZE {
                2
            } else {
                CLAIMANTS[action]
            }]
            .portfolio,
        );
    }
    let instructions: Vec<_> = actions
        .iter()
        .map(|&action| retained[action].clone())
        .collect();
    let meta = world
        .land(&instructions, false)
        .expect("public expiry/claim ordering");
    for (program, successes) in [
        (world.env.program_id, actions.len()),
        (spl_token::ID, transfers),
    ] {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            successes
        );
    }
    world.assert_frame_except(&before, &allowed);
    oracle.check(world);
}

#[test]
fn v16_program_retained_claims_commute_with_late_expiry_normalization_after_catchup() {
    let mut peak_cu = 0;
    for landing in [13, 15] {
        for atomic in [false, true] {
            for order in ORDERS {
                let mut world = World::new();
                let mut oracle = Prefix::new(&world);
                oracle.check(&world);
                let retained = [
                    world.payout(0, true),
                    world.payout(4, true),
                    world.payout(2, false),
                ];
                world.peak_cu = 0;
                world.env.svm.warp_to_slot(landing);
                oracle.check(&world);
                for actions in order.chunks(if atomic { 3 } else { 1 }) {
                    land(&mut world, &mut oracle, &retained, actions);
                    assert_eq!(world.env.market_state().1.current_slot, landing);
                }

                // Earlier zero-due claims remain pending through the later normalization.
                // Reverse their original priority for catch-up; retain the same wire bytes.
                for claimant in order
                    .into_iter()
                    .rev()
                    .filter(|&action| action != NORMALIZE)
                {
                    land(&mut world, &mut oracle, &retained, &[claimant]);
                }
                assert_eq!(oracle.paid, [198, 368]);
                let before = world.frame();
                land(&mut world, &mut oracle, &retained, &[0, 1, 1, 0]);
                assert_eq!(
                    world.frame(),
                    before,
                    "fresh-blockhash duplicates pay nothing"
                );
                assert_cu_within("expiry/claim interleavings", world.peak_cu, 500_000);
                peak_cu = peak_cu.max(world.peak_cu);
            }
        }
    }
    println!("INV-067 row 417: 24 worlds, 6 expiry/claim orders, atomic/split catch-up; peak {peak_cu} CU");
}
