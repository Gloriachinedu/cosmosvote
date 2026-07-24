# Threat Model

## Threat Actors

### T1 — Malicious Voter
**Goal:** Cast more votes than balance entitles, or vote multiple times.  
**Mitigations:** `has_voted` guard, zero-balance check, vote weight captured immutably at vote time, flash-loan mitigation (loan repaid before tx ends → zero balance).  
**Residual Risk:** None

### T2 — Malicious Proposer
**Goal:** Create proposals that pass without genuine support, or spam the system with low-quality proposals.  
**Mitigations:** Quorum enforcement, `min_proposal_balance`, `proposal_cooldown`, admin cancel.  
**Production Recommendation:** Always set `min_proposal_balance` to a non-zero value in production (e.g., 1% of total supply) to prevent sybil spam.  
**Residual Risk:** Low

### T3 — Malicious Admin
**Goal:** Abuse privileged functions to manipulate outcomes.  
**Mitigations:** Admin can only cancel/execute — cannot alter votes or tallies. Admin vote restriction option. Events make all admin actions auditable.  
**Residual Risk:** Medium (by design — admin is a trusted role; use multisig in production)

### T3a — Quorum Manipulation via update_quorum
**Goal:** Admin waits for votes to accumulate, then lowers quorum to exactly the current vote count to force a proposal to pass without genuine participation.  
**Attack Scenario:** Admin creates or monitors a proposal. Enough votes accumulate to meet a lowered quorum but not the original. Admin calls `update_quorum` within the 10% time window to lower the quorum floor to the current `total_votes`, making the proposal instantly eligible for finalization.  
**Mitigations:** `update_quorum` rejects any `new_quorum` that is strictly less than the proposal's current `total_votes` (sum of yes + no + abstain) with `ContractError::QuorumBelowCurrentVotes` (code 61). This guard applies even within the permitted 10% update window.  
**Residual Risk:** Low — the guard makes it impossible to lower quorum below accumulated votes.

### T4 — External Attacker (No Tokens)
**Goal:** Disrupt governance without holding tokens.  
**Mitigations:** `require_auth()` on all state-changing ops, initialization guard, state machine checks.  
**Residual Risk:** None

### T5 — Compromised Token Contract
**Goal:** Return inflated balances to favored voters.  
**Mitigations:** Admin must deploy a trustworthy token; token address is immutable after init.  
**Residual Risk:** High (external dependency — accepted)

### Reentrancy and Cross-Contract Calls
**Goal:** Prevent malicious token callbacks from manipulating governance tallies during vote casting or finalization.  
**Mitigations:** Soroban executes contract calls in a single deterministic transaction frame and enforces call stack isolation. Cross-contract callbacks during `cast_vote` are executed without allowing unsafe reentrant state mutations in the governance contract.  
**Residual Risk:** Low (verified by regression tests that deploy a malicious token contract and attempt to re-enter `cast_vote` through `balance_at`).

## RPC CORS Requirements
The frontend connects directly to a Soroban RPC endpoint. A browser-accessible RPC must expose a narrow CORS policy to avoid breaking the app while also preventing broad abuse.

Required headers for the RPC endpoint:
- `Access-Control-Allow-Origin: <frontend origin>` (do not use `*` for production)
- `Access-Control-Allow-Methods: POST, OPTIONS`
- `Access-Control-Allow-Headers: Content-Type`
- `Access-Control-Allow-Credentials: false`

For local Docker development, the repository uses a proxy service so the frontend can access the RPC at `http://localhost:8000` with explicit CORS headers while the underlying Soroban node remains isolated.

## Security Properties

| Property | Implementation |
|----------|---------------|
| Vote integrity | One vote per address; weight = live balance at vote time |
| Admin confinement | Admin cannot alter votes or tallies |
| Initialization safety | One-time init guard; admin/token immutable after |
| Arithmetic safety | `checked_add` on all vote accumulation |
| Finalization correctness | Pass condition evaluated atomically |
| Emergency response | Admin pause blocks all state-changing ops |
.
