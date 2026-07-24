//! Governance contract — type definitions and error codes.

use soroban_sdk::{contracterror, contracttype, Address, String, Symbol, Vec, Val};

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    // Initialization
    AlreadyInitialized  = 1,
    NotInitialized      = 2,

    // Proposals
    ProposalNotFound    = 10,
    ProposalNotActive   = 11,
    ProposalNotPassed   = 12,
    InvalidTitle        = 13,
    InvalidDescription  = 14,
    InvalidQuorum       = 15,
    QuorumExceedsSupply = 16,
    InvalidDurationRange = 17,
    InsufficientBalance = 18,
    ProposalCooldown    = 19,
    QuorumBelowFloor    = 20,
    ProposalsStillActive = 21,

    // Voting
    VotingNotStarted    = 22,
    VotingPeriodEnded   = 23,
    VotingStillOpen     = 24,
    AlreadyVoted        = 25,
    NoVotingPower       = 26,
    AdminVoteRestricted = 27,
    VoteNotFound        = 28,
    VoteAlreadySame     = 29,

    // Admin
    NotAdmin            = 30,
    InvalidNewAdmin     = 31,
    QuorumUpdateNotAllowed = 32,
    NoPendingAdmin      = 33,
    NotPendingAdmin     = 34,
    NotProposer         = 35,
    VotesAlreadyCast    = 36,
    InvalidLink         = 37,

    // Contract state
    ContractPaused      = 40,
    NotPaused           = 41,

    // Arithmetic
    ArithmeticOverflow  = 50,

    // Execution
    ExecutionFailed     = 60,

    // Quorum integrity
    /// Raised by update_quorum when new_quorum < total_votes (yes + no + abstain).
    /// Lowering quorum below the current vote count would instantly pass a proposal
    /// without genuine deliberation and is therefore rejected.
    QuorumBelowCurrentVotes = 61,
}

// ---------------------------------------------------------------------------
// Contract state
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContractState {
    Uninitialized,
    Ready,
}

// ---------------------------------------------------------------------------
// Treasury
// ---------------------------------------------------------------------------

/// Asset to disburse — native XLM or a SEP-41 token.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreasuryAsset {
    Native,
    Token(Address),
}

/// Optional payload attached to a proposal for on-execution treasury disbursement.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryAction {
    pub recipient: Address,
    pub amount: i128,
    pub asset: TreasuryAsset,
}

// ---------------------------------------------------------------------------
// Proposal
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalState {
    Active,
    Passed,
    Rejected,
    Executed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ExecutionPayload {
    pub contract: Address,
    pub action: Symbol,
    pub args: Vec<Val>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub title: String,
    pub description: String,
    pub link: Option<String>,
    pub votes_yes: i128,
    pub votes_no: i128,
    pub votes_abstain: i128,
    pub quorum: i128,
    pub start_time: u64,
    pub end_time: u64,
    pub state: ProposalState,
    pub snapshot_ledger: u32,
    pub voter_count: u32,
    /// Optional treasury disbursement to execute on proposal execution.
    pub treasury_action: Vec<TreasuryAction>,
}

// ---------------------------------------------------------------------------
// Voting
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Vote {
    Yes,
    No,
    Abstain,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct VoteRecord {
    pub vote: Vote,
    pub weight: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GovernanceConfig {
    pub admin: Address,
    pub voting_token: Address,
    pub min_proposal_balance: i128,
    pub proposal_cooldown: u64,
    pub restrict_admin_vote: bool,
    pub paused: bool,
}
