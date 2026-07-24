//! Governance contract — storage accessors with tiered storage strategy.
//!
//! * Instance storage  — contract-wide config (cheap, loaded once per call)
//! * Persistent storage — per-proposal / per-voter data (survives ledger expiry)

use soroban_sdk::{Address, Env};

use crate::types::{ContractState, Proposal, ProposalState, VoteRecord};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[soroban_sdk::contracttype]
#[derive(Clone)]
pub enum InstanceKey {
    Admin,
    PendingAdmin,
    VotingToken,
    TreasuryContract,
    ProposalCount,
    ActiveProposalCount,
    MinProposalBalance,
    ProposalCooldown,
    MinQuorumBps,
    RestrictAdminVote,
    Version,
    ContractState,
    Paused,
    /// Seconds that must elapse after finalization before execute() is callable.
    /// 0 = disabled.
    TimelockSeconds,
}

#[soroban_sdk::contracttype]
#[derive(Clone)]
pub enum PersistentKey {
    Proposal(u64),
    HasVoted(u64, Address),
    VoteRecord(u64, Address),
    ProposalCount,
    LastProposal(Address),
}

// ---------------------------------------------------------------------------
// Storage accessors
// ---------------------------------------------------------------------------

pub struct GovernanceStorage;

impl GovernanceStorage {
    // Amount of ledgers to extend persistent entries by (30 days)
    pub const PERSISTENT_BUMP_AMOUNT: u32 = 518_400; // ~30 days @ 5s/ledger
    pub const PERSISTENT_THRESHOLD: u32 = 17_280;   // ~1 day @ 5s/ledger

    fn bump_persistent_ttl(env: &Env, key: &PersistentKey) {
        // best-effort: extend the TTL for the provided persistent key
        env.storage()
            .persistent()
            .extend_ttl(key, Self::PERSISTENT_THRESHOLD, Self::PERSISTENT_BUMP_AMOUNT);
    }
    // --- Instance ---

    pub fn admin(env: &Env) -> Address {
        env.storage().instance().get(&InstanceKey::Admin).unwrap()
    }
    pub fn set_admin(env: &Env, v: &Address) {
        env.storage().instance().set(&InstanceKey::Admin, v);
    }

    pub fn pending_admin(env: &Env) -> Option<Address> {
        env.storage().instance().get(&InstanceKey::PendingAdmin)
    }
    pub fn set_pending_admin(env: &Env, v: Option<&Address>) {
        match v {
            Some(addr) => env.storage().instance().set(&InstanceKey::PendingAdmin, addr),
            None => env.storage().instance().remove(&InstanceKey::PendingAdmin),
        }
    }

    pub fn voting_token(env: &Env) -> Address {
        env.storage().instance().get(&InstanceKey::VotingToken).unwrap()
    }
    pub fn set_voting_token(env: &Env, v: &Address) {
        env.storage().instance().set(&InstanceKey::VotingToken, v);
    }

    pub fn treasury_contract(env: &Env) -> Option<Address> {
        env.storage().instance().get(&InstanceKey::TreasuryContract)
    }
    pub fn set_treasury_contract(env: &Env, v: &Address) {
        env.storage().instance().set(&InstanceKey::TreasuryContract, v);
    }

    pub fn proposal_count(env: &Env) -> u64 {
        env.storage().instance().get(&InstanceKey::ProposalCount).unwrap_or(0)
    }
    pub fn set_proposal_count(env: &Env, v: u64) {
        env.storage().instance().set(&InstanceKey::ProposalCount, &v);
    }

    pub fn active_proposal_count(env: &Env) -> u64 {
        env.storage().instance().get(&InstanceKey::ActiveProposalCount).unwrap_or(0)
    }
    pub fn set_active_proposal_count(env: &Env, v: u64) {
        env.storage().instance().set(&InstanceKey::ActiveProposalCount, &v);
    }

    pub fn min_proposal_balance(env: &Env) -> i128 {
        env.storage().instance().get(&InstanceKey::MinProposalBalance).unwrap_or(0)
    }
    pub fn set_min_proposal_balance(env: &Env, v: i128) {
        env.storage().instance().set(&InstanceKey::MinProposalBalance, &v);
    }

    pub fn proposal_cooldown(env: &Env) -> u64 {
        env.storage().instance().get(&InstanceKey::ProposalCooldown).unwrap_or(0)
    }
    pub fn set_proposal_cooldown(env: &Env, v: u64) {
        env.storage().instance().set(&InstanceKey::ProposalCooldown, &v);
    }

    pub fn min_quorum_bps(env: &Env) -> u32 {
        env.storage().instance().get(&InstanceKey::MinQuorumBps).unwrap_or(0)
    }
    pub fn set_min_quorum_bps(env: &Env, v: u32) {
        env.storage().instance().set(&InstanceKey::MinQuorumBps, &v);
    }

    pub fn restrict_admin_vote(env: &Env) -> bool {
        env.storage().instance().get(&InstanceKey::RestrictAdminVote).unwrap_or(false)
    }
    pub fn set_restrict_admin_vote(env: &Env, v: bool) {
        env.storage().instance().set(&InstanceKey::RestrictAdminVote, &v);
    }

    pub fn paused(env: &Env) -> bool {
        env.storage().instance().get(&InstanceKey::Paused).unwrap_or(false)
    }
    pub fn set_paused(env: &Env, v: bool) {
        env.storage().instance().set(&InstanceKey::Paused, &v);
    }

    pub fn timelock_seconds(env: &Env) -> u64 {
        env.storage().instance().get(&InstanceKey::TimelockSeconds).unwrap_or(0)
    }
    pub fn set_timelock_seconds(env: &Env, v: u64) {
        env.storage().instance().set(&InstanceKey::TimelockSeconds, &v);
    }

    pub fn contract_state(env: &Env) -> ContractState {
        env.storage()
            .instance()
            .get(&InstanceKey::ContractState)
            .unwrap_or(ContractState::Uninitialized)
    }
    pub fn set_contract_state(env: &Env, v: ContractState) {
        env.storage().instance().set(&InstanceKey::ContractState, &v);
    }

    pub fn version(env: &Env) -> (u32, u32, u32) {
        env.storage().instance().get(&InstanceKey::Version).unwrap_or((1, 0, 0))
    }
    pub fn set_version(env: &Env, v: (u32, u32, u32)) {
        env.storage().instance().set(&InstanceKey::Version, &v);
    }

    // --- Persistent ---

    pub fn proposal(env: &Env, id: u64) -> Option<Proposal> {
        let key = PersistentKey::Proposal(id);
        let v = env.storage().persistent().get(&key);
        if v.is_some() {
            Self::bump_persistent_ttl(env, &key);
        }
        v
    }
    pub fn set_proposal(env: &Env, id: u64, v: &Proposal) {
        let key = PersistentKey::Proposal(id);
        env.storage().persistent().set(&key, v);
        Self::bump_persistent_ttl(env, &key);
    }

    pub fn has_voted(env: &Env, proposal_id: u64, voter: &Address) -> bool {
        let key = PersistentKey::HasVoted(proposal_id, voter.clone());
        let v = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(false);
        // bump TTL on read
        Self::bump_persistent_ttl(env, &key);
        v
    }
    pub fn set_has_voted(env: &Env, proposal_id: u64, voter: &Address, v: bool) {
        let key = PersistentKey::HasVoted(proposal_id, voter.clone());
        env.storage().persistent().set(&key, &v);
        Self::bump_persistent_ttl(env, &key);
    }

    pub fn vote_record(env: &Env, proposal_id: u64, voter: &Address) -> Option<VoteRecord> {
        let key = PersistentKey::VoteRecord(proposal_id, voter.clone());
        let v = env.storage().persistent().get(&key);
        if v.is_some() {
            Self::bump_persistent_ttl(env, &key);
        }
        v
    }
    pub fn set_vote_record(env: &Env, proposal_id: u64, voter: &Address, v: &VoteRecord) {
        let key = PersistentKey::VoteRecord(proposal_id, voter.clone());
        env.storage().persistent().set(&key, v);
        Self::bump_persistent_ttl(env, &key);
    }

    pub fn last_proposal_time(env: &Env, proposer: &Address) -> Option<u64> {
        let key = PersistentKey::LastProposal(proposer.clone());
        let v = env.storage().persistent().get(&key);
        if v.is_some() {
            Self::bump_persistent_ttl(env, &key);
        }
        v
    }
    pub fn set_last_proposal_time(env: &Env, proposer: &Address, v: u64) {
        let key = PersistentKey::LastProposal(proposer.clone());
        env.storage().persistent().set(&key, &v);
        Self::bump_persistent_ttl(env, &key);
    }

    // Proposal count persisted to persistent storage to avoid instance write contention
    /// Convenience: check if a proposal is in a terminal state.
    pub fn is_terminal(state: &ProposalState) -> bool {
        matches!(
            state,
            ProposalState::Executed | ProposalState::Cancelled | ProposalState::Rejected
        )
    }
}
