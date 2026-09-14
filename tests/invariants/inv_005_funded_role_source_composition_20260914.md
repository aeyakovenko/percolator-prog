# INV-005 Funded-Role Source Composition

Owner: `cu/inv_005_authority_incarnation_binding.rs`,
`v16_program_funded_role_guard_and_oracle_handoff_are_source_complete`.

This source gate pins the production branch behind `UpdateAssetAuthority`. It
exists because the public row-416 histories are only meaningful if the wrapper's
cold-admin branch continues to derive "funded role" from the same stock classes
that can carry user value.

The gate checks three properties:

- The funded-role predicate includes all five backing stock classes:
  fresh, valid-liened, consumed-liened, impaired-liened and utilization earnings.
- Insurance and insurance-operator protection depends on both live domain
  budgets, while historical spent counters do not become transferrable authority.
- A cold-admin-only funded handoff rejects before authority-epoch advancement and
  before profile persistence; the successful match writes exactly one of the five
  authority fields, including `oracle_authority`.

This composes with the existing public witnesses:

- `cold_admin_handoff_scope` covers cold-admin ABA/burn with funded insurance and
  backing role payouts.
- `funded_role_zero_transition` covers a zero-role suffix after a funded handoff.
- `cold_oracle_funded_containment` covers cold-admin oracle replacement while the
  incumbent also holds funded backing.
- `funded_oracle_succession` and `funded_backing_succession` cover incumbent
  succession preserving exits.

Row 416 remains OPEN. This is a current-route source-composition guard, not a
generic generator over arbitrary funded histories, position/claim states,
coalesced market roles, lifecycle transitions, clock/oracle schedules or future
handler rewrites outside the pinned branch.
