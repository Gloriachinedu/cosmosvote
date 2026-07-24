//! CosmosVote Governance Contract
//! CosmosVote Governance Contract
//! Manages the full proposal lifecycle: creation, voting, finalization,
//! execution, and cancellation. Enforces quorum, prevents double-voting,
//! and emits on-chain events for every state transition.
//! .

#![no_std]

mod events;
mod storage;
mod types;

#[cfg(test)]
mod test;
#[cfg(test)]
mod test_helpers;
#[cfg(test)]
mod prop_tests;
#[cfg(test)]
mod benchmarks;
#[cfg(test)]
mod event_tests;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use events::GovernanceEvents;
use storage::GovernanceStorage;
use types::{ContractError, ContractState, GovernanceConfig, Proposal, ProposalState, TreasuryAction, Vote, VoteRecord};

// ---------------------------------------------------------------------------
// Token interface (cross-contract call)
// ---------------------------------------------------------------------------

mod token_interface {
    use soroban_sdk::{contractclient, Address, Env, Vec};

    #[contractclient(name = "TokenClient")]
    pub trait TokenInterface {
        fn balance(env: Env, owner: Address) -> i128;
        fn balance_at(env: Env, owner: Address, ledger: u64) -> i128;
        fn total_supply(env: Env) -> i128;
        fn get_delegation(env: Env, owner: Address) -> Option<Address>;
        fn get_delegated_weight(env: Env, voter: Address, delegators: Vec<Address>) -> i128;
    }
}

pub(crate) use token_interface::TokenClient;

// ---------------------------------------------------------------------------
// Treasury interface (cross-contract call)
// ---------------------------------------------------------------------------

mod treasury_interface {
    use soroban_sdk::{contractclient, Env};
    use crate::types::TreasuryAction;

    #[contractclient(name = "TreasuryClient")]
    pub trait TreasuryInterface {
        fn disburse(env: Env, action: TreasuryAction);
    }
}

use treasury_interface::TreasuryClient;

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize the governance contract.
    ///
    /// * `admin`                – privileged address (execute / cancel / pause)
    /// * `voting_token`         – SEP-41 governance token address
    /// * `min_proposal_balance` – minimum token balance to create proposals (0 = none)
    /// * `proposal_cooldown`    – seconds between proposals per address (0 = none)
    /// * `min_quorum_bps`       – minimum quorum floor in basis points (100 = 1%)
    /// * `restrict_admin_vote`  – if true, admin cannot vote on own proposals
    /// * `treasury`             – optional treasury contract address
    pub fn initialize(
        env: Env,
        admin: Address,
        voting_token: Address,
        min_proposal_balance: i128,
        proposal_cooldown: u64,
        min_quorum_bps: u32,
        restrict_admin_vote: bool,
        treasury: Option<Address>,
    ) -> Result<(), ContractError> {
        if GovernanceStorage::contract_state(&env) != ContractState::Uninitialized {
            return Err(ContractError::AlreadyInitialized);
        }

        GovernanceStorage::set_admin(&env, &admin);
        GovernanceStorage::set_voting_token(&env, &voting_token);
        if let Some(t) = &treasury {
            GovernanceStorage::set_treasury_contract(&env, t);
        }
        GovernanceStorage::set_proposal_count(&env, 0);
        GovernanceStorage::set_min_proposal_balance(&env, min_proposal_balance);
        GovernanceStorage::set_proposal_cooldown(&env, proposal_cooldown);
        GovernanceStorage::set_min_quorum_bps(&env, min_quorum_bps);
        GovernanceStorage::set_restrict_admin_vote(&env, restrict_admin_vote);
        GovernanceStorage::set_paused(&env, false);
        GovernanceStorage::set_contract_state(&env, ContractState::Ready);
        GovernanceStorage::set_version(&env, (1, 0, 0));

        GovernanceEvents::initialized(&env, &admin, &voting_token);
        Ok(())
    }

    /// Retrieve the current governance configuration.
    pub fn get_config(env: Env) -> GovernanceConfig {
        GovernanceConfig {
            admin: GovernanceStorage::admin(&env),
            voting_token: GovernanceStorage::voting_token(&env),
            min_proposal_balance: GovernanceStorage::min_proposal_balance(&env),
            proposal_cooldown: GovernanceStorage::proposal_cooldown(&env),
            restrict_admin_vote: GovernanceStorage::restrict_admin_vote(&env),
            paused: GovernanceStorage::paused(&env),
        }
    }

    // -----------------------------------------------------------------------
    // Proposal management
    // -----------------------------------------------------------------------

    /// Create a new governance proposal.
    ///
    /// Returns the new proposal ID.
    pub fn create_proposal(
        env: Env,
        proposer: Address,
        title: String,
        description: String,
        quorum: i128,
        duration: u64,
        link: Option<String>,
        treasury_action: Option<TreasuryAction>,
    ) -> Result<u64, ContractError> {
        proposer.require_auth();
        Self::assert_ready(&env)?;

        // Validate inputs
        if title.len() == 0 || title.len() > 128 {
            return Err(ContractError::InvalidTitle);
        }
        if description.len() == 0 || description.len() > 1024 {
            return Err(ContractError::InvalidDescription);
        }
        if let Some(ref l) = link {
            if l.len() == 0 || l.len() > 256 {
                return Err(ContractError::InvalidLink);
            }
        }
        if quorum <= 0 {
            return Err(ContractError::InvalidQuorum);
        }
        if duration < 60 || duration > 2_592_000 {
            return Err(ContractError::InvalidDurationRange);
        }

        // Global rate limit: max 50 active proposals
        let active_count = GovernanceStorage::active_proposal_count(&env);
        if active_count >= 50 {
            return Err(ContractError::ProposalsStillActive);
        }

        let token = GovernanceStorage::voting_token(&env);
        let token_client = TokenClient::new(&env, &token);
        let total_supply = token_client.total_supply();

        if quorum > total_supply {
            return Err(ContractError::QuorumExceedsSupply);
        }

        // Quorum floor check
        let min_quorum_bps = GovernanceStorage::min_quorum_bps(&env);
        if min_quorum_bps > 0 {
            let floor = total_supply
                .checked_mul(min_quorum_bps as i128)
                .and_then(|v| v.checked_div(10_000))
                .ok_or(ContractError::ArithmeticOverflow)?;
            
            if quorum < floor {
                return Err(ContractError::QuorumBelowFloor);
            }
        }

        // Balance check
        let min_balance = GovernanceStorage::min_proposal_balance(&env);
        if min_balance > 0 {
            let bal = token_client.balance(&proposer);
            if bal < min_balance {
                return Err(ContractError::InsufficientBalance);
            }
        }

        // Cooldown check
        let cooldown = GovernanceStorage::proposal_cooldown(&env);
        if cooldown > 0 {
            let now = env.ledger().timestamp();
            if let Some(last) = GovernanceStorage::last_proposal_time(&env, &proposer) {
                if now < last + cooldown {
                    return Err(ContractError::ProposalCooldown);
                }
            }
        }

        let now = env.ledger().timestamp();
        let snapshot_ledger = env.ledger().sequence();
        let id = GovernanceStorage::proposal_count(&env);
        let proposal = Proposal {
            id,
            proposer: proposer.clone(),
            title: title.clone(),
            description: description.clone(),
            link,
            votes_yes: 0,
            votes_no: 0,
            votes_abstain: 0,
            quorum,
            start_time: now,
            end_time: now + duration,
            state: ProposalState::Active,
            snapshot_ledger,
            voter_count: 0,
            treasury_action: match treasury_action {
                Some(a) => Vec::from_array(&env, [a]),
                None => Vec::new(&env),
            },
        };

        GovernanceStorage::set_proposal(&env, id, &proposal);
        GovernanceStorage::set_proposal_count(&env, id + 1);
        GovernanceStorage::set_active_proposal_count(&env, active_count + 1);
        GovernanceStorage::set_last_proposal_time(&env, &proposer, now);

        GovernanceEvents::proposal_created(&env, id, &proposer, &title, quorum, now + duration);
        Ok(id)
    }

    /// Retrieve a proposal by ID.
    pub fn get_proposal(env: Env, proposal_id: u64) -> Result<Proposal, ContractError> {
        GovernanceStorage::proposal(&env, proposal_id)
            .ok_or(ContractError::ProposalNotFound)
    }

    /// Total number of proposals created.
    pub fn proposal_count(env: Env) -> u64 {
        GovernanceStorage::proposal_count(&env)
    }

    /// Current number of proposals in a non-terminal state (Active).
    ///
    /// Off-chain dashboards and the frontend can use this to display
    /// "X active proposals" without paginating through all proposals.
    pub fn active_proposal_count(env: Env) -> u64 {
        GovernanceStorage::active_proposal_count(&env)
    }

    /// Paginated list of proposals.
    pub fn get_proposals(env: Env, from_id: u64, limit: u32) -> Vec<Proposal> {
        let count = GovernanceStorage::proposal_count(&env);
        let limit = if limit > 20 { 20 } else { limit };
        let mut proposals = Vec::new(&env);

        let end = (from_id + limit as u64).min(count);
        for id in from_id..end {
            if let Some(proposal) = GovernanceStorage::proposal(&env, id) {
                proposals.push_back(proposal);
            }
        }
        proposals
    }

    /// Paginated list of proposals filtered by state.
    pub fn get_proposals_by_state(
        env: Env,
        state: ProposalState,
        from_id: u64,
        limit: u32,
    ) -> Vec<Proposal> {
        let count = GovernanceStorage::proposal_count(&env);
        let limit = if limit > 20 { 20 } else { limit };
        let mut proposals = Vec::new(&env);

        let mut current_id = from_id;
        while proposals.len() < limit && current_id < count {
            if let Some(proposal) = GovernanceStorage::proposal(&env, current_id) {
                if proposal.state == state {
                    proposals.push_back(proposal);
                }
            }
            current_id += 1;
        }
        proposals
    }

    /// Amend the title and description of an active proposal before any votes are cast.
    /// Only callable by the original proposer.
    pub fn amend_proposal(
        env: Env,
        proposer: Address,
        proposal_id: u64,
        new_title: String,
        new_description: String,
    ) -> Result<(), ContractError> {
        proposer.require_auth();
        Self::assert_ready(&env)?;

        let mut proposal = GovernanceStorage::proposal(&env, proposal_id)
            .ok_or(ContractError::ProposalNotFound)?;

        if proposal.proposer != proposer {
            return Err(ContractError::NotProposer);
        }

        if proposal.state != ProposalState::Active {
            return Err(ContractError::ProposalNotActive);
        }

        let total_votes = proposal.votes_yes
            .checked_add(proposal.votes_no)
            .and_then(|v| v.checked_add(proposal.votes_abstain))
            .ok_or(ContractError::ArithmeticOverflow)?;

        if total_votes > 0 {
            return Err(ContractError::VotesAlreadyCast);
        }

        if new_title.len() == 0 || new_title.len() > 128 {
            return Err(ContractError::InvalidTitle);
        }
        if new_description.len() == 0 || new_description.len() > 1024 {
            return Err(ContractError::InvalidDescription);
        }

        let old_title = proposal.title.clone();
        let old_description = proposal.description.clone();
        proposal.title = new_title.clone();
        proposal.description = new_description.clone();
        GovernanceStorage::set_proposal(&env, proposal_id, &proposal);

        GovernanceEvents::proposal_amended(
            &env,
            proposal_id,
            &proposer,
            &old_title,
            &new_title,
            &old_description,
            &new_description,
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Voting
    // -----------------------------------------------------------------------

    /// Cast a vote on an active proposal.
    pub fn cast_vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        vote: Vote,
    ) -> Result<(), ContractError> {
        voter.require_auth();
        Self::assert_ready(&env)?;

        let mut proposal = GovernanceStorage::proposal(&env, proposal_id)
            .ok_or(ContractError::ProposalNotFound)?;

        if proposal.state != ProposalState::Active {
            return Err(ContractError::ProposalNotActive);
        }

        let now = env.ledger().timestamp();
        if now < proposal.start_time {
            return Err(ContractError::VotingNotStarted);
        }
        if now > proposal.end_time {
            return Err(ContractError::VotingPeriodEnded);
        }

        if GovernanceStorage::has_voted(&env, proposal_id, &voter) {
            return Err(ContractError::AlreadyVoted);
        }

        // Admin vote restriction: when enabled, prevent the admin from voting
        // on proposals that *they created* but allow the admin to vote on
        // proposals created by others. This avoids absolute admin lockout
        // while still preventing self-voting on owned proposals.
        if GovernanceStorage::restrict_admin_vote(&env) {
            let admin = GovernanceStorage::admin(&env);
            if voter == admin && proposal.proposer == admin {
                return Err(ContractError::AdminVoteRestricted);
            }
        }

        let token = GovernanceStorage::voting_token(&env);
        let token_client = TokenClient::new(&env, &token);

        // If the voter has delegated their power away, they cannot vote directly.
        // Their delegate will vote with the accumulated weight.
        if token_client.get_delegation(&voter).is_some() {
            return Err(ContractError::NoVotingPower);
        }

        // Weight = voter's own balance (delegators' balances are added via get_delegated_weight).
        // Since we cannot enumerate all delegators on-chain, governance uses an empty list here;
        // the voter's own balance is always included. Delegators who want their weight counted
        // must have their delegate cast the vote on their behalf.
        let weight = token_client.get_delegated_weight(&voter, &Vec::new(&env));
        if weight <= 0 {
            return Err(ContractError::NoVotingPower);
        }

        // Overflow is theoretically impossible: each voter's weight comes from their token
        // balance, and the sum of all balances equals total_supply which is bounded by
        // i128::MAX (170_141_183_460_469_231_731_687_303_715_884_105_727). The token contract
        // enforces this via checked_add on mint, so total votes can never exceed total_supply.
        // checked_add is retained as a defence-in-depth guard.
        match vote {
            Vote::Yes => proposal.votes_yes = proposal.votes_yes.checked_add(weight)
                .ok_or(ContractError::ArithmeticOverflow)?,
            Vote::No => proposal.votes_no = proposal.votes_no.checked_add(weight)
                .ok_or(ContractError::ArithmeticOverflow)?,
            Vote::Abstain => proposal.votes_abstain = proposal.votes_abstain.checked_add(weight)
                .ok_or(ContractError::ArithmeticOverflow)?,
        }

        GovernanceStorage::set_has_voted(&env, proposal_id, &voter, true);
        GovernanceStorage::set_vote_record(&env, proposal_id, &voter, &VoteRecord { vote: vote.clone(), weight });
        proposal.voter_count += 1;
        GovernanceStorage::set_proposal(&env, proposal_id, &proposal);

        GovernanceEvents::vote_cast(&env, proposal_id, &voter, &vote, weight);
        Ok(())
    }

    /// Returns true if the voter has already voted on this proposal.
    pub fn has_voted(env: Env, proposal_id: u64, voter: Address) -> bool {
        GovernanceStorage::has_voted(&env, proposal_id, &voter)
    }

    /// Retrieve the vote record for a voter on a proposal.
    pub fn get_vote(
        env: Env,
        proposal_id: u64,
        voter: Address,
    ) -> Result<VoteRecord, ContractError> {
        GovernanceStorage::vote_record(&env, proposal_id, &voter)
            .ok_or(ContractError::VoteNotFound)
    }

    /// Retract a previously cast vote on an active proposal.
    pub fn retract_vote(env: Env, voter: Address, proposal_id: u64) -> Result<(), ContractError> {
        voter.require_auth();
        Self::assert_ready(&env)?;

        let mut proposal = GovernanceStorage::proposal(&env, proposal_id)
            .ok_or(ContractError::ProposalNotFound)?;

        if proposal.state != ProposalState::Active {
            return Err(ContractError::ProposalNotActive);
        }
        let now = env.ledger().timestamp();
        if now > proposal.end_time {
            return Err(ContractError::VotingPeriodEnded);
        }
        if !GovernanceStorage::has_voted(&env, proposal_id, &voter) {
            return Err(ContractError::VoteNotFound);
        }

        let record = GovernanceStorage::vote_record(&env, proposal_id, &voter)
            .ok_or(ContractError::VoteNotFound)?;

        match record.vote {
            Vote::Yes => proposal.votes_yes -= record.weight,
            Vote::No => proposal.votes_no -= record.weight,
            Vote::Abstain => proposal.votes_abstain -= record.weight,
        }

        GovernanceStorage::set_has_voted(&env, proposal_id, &voter, false);
        env.storage().persistent().remove(&storage::PersistentKey::VoteRecord(proposal_id, voter.clone()));
        GovernanceStorage::set_proposal(&env, proposal_id, &proposal);

        GovernanceEvents::vote_retracted(&env, proposal_id, &voter, record.weight);
        Ok(())
    }

    /// Change a previously cast vote on an active proposal.
    pub fn change_vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        new_vote: Vote,
    ) -> Result<(), ContractError> {
        voter.require_auth();
        Self::assert_ready(&env)?;

        let mut proposal = GovernanceStorage::proposal(&env, proposal_id)
            .ok_or(ContractError::ProposalNotFound)?;

        if proposal.state != ProposalState::Active {
            return Err(ContractError::ProposalNotActive);
        }
        let now = env.ledger().timestamp();
        if now > proposal.end_time {
            return Err(ContractError::VotingPeriodEnded);
        }
        if !GovernanceStorage::has_voted(&env, proposal_id, &voter) {
            return Err(ContractError::VoteNotFound);
        }

        let record = GovernanceStorage::vote_record(&env, proposal_id, &voter)
            .ok_or(ContractError::VoteNotFound)?;

        if record.vote == new_vote {
            return Err(ContractError::VoteAlreadySame);
        }

        match record.vote {
            Vote::Yes => proposal.votes_yes -= record.weight,
            Vote::No => proposal.votes_no -= record.weight,
            Vote::Abstain => proposal.votes_abstain -= record.weight,
        }
        match new_vote {
            Vote::Yes => proposal.votes_yes = proposal.votes_yes.checked_add(record.weight)
                .ok_or(ContractError::ArithmeticOverflow)?,
            Vote::No => proposal.votes_no = proposal.votes_no.checked_add(record.weight)
                .ok_or(ContractError::ArithmeticOverflow)?,
            Vote::Abstain => proposal.votes_abstain = proposal.votes_abstain.checked_add(record.weight)
                .ok_or(ContractError::ArithmeticOverflow)?,
        }

        let old_vote = record.vote.clone();
        GovernanceStorage::set_vote_record(&env, proposal_id, &voter, &VoteRecord { vote: new_vote.clone(), weight: record.weight });
        GovernanceStorage::set_proposal(&env, proposal_id, &proposal);

        GovernanceEvents::vote_changed(&env, proposal_id, &voter, &old_vote, &new_vote, record.weight);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Finalization & execution
    // -----------------------------------------------------------------------

    /// Finalize a proposal after its voting period ends.
    ///
    /// This function is permissionless — anyone may call it. It is safe against
    /// spam because the `ProposalNotActive` guard makes every call after the
    /// first a cheap no-op: the proposal state is read, the non-`Active` branch
    /// is taken, and `Err(ProposalNotActive)` is returned without writing any
    /// storage. No state corruption is possible.
    ///
    /// **Idempotency guarantee:** only the *first* successful call transitions
    /// the proposal to `Passed` or `Rejected` and emits the `proposal_finalized`
    /// event. Subsequent calls return `Err(ProposalNotActive)` immediately.
    pub fn finalise(env: Env, proposal_id: u64) -> Result<(), ContractError> {
        Self::assert_ready(&env)?;

        let mut proposal = GovernanceStorage::proposal(&env, proposal_id)
            .ok_or(ContractError::ProposalNotFound)?;

        if proposal.state != ProposalState::Active {
            return Err(ContractError::ProposalNotActive);
        }

        let now = env.ledger().timestamp();
        if now <= proposal.end_time {
            return Err(ContractError::VotingStillOpen);
        }

        let total_votes = proposal.votes_yes
            .checked_add(proposal.votes_no)
            .and_then(|v| v.checked_add(proposal.votes_abstain))
            .ok_or(ContractError::ArithmeticOverflow)?;

        let passed = total_votes >= proposal.quorum && proposal.votes_yes > proposal.votes_no;
        proposal.state = if passed { ProposalState::Passed } else { ProposalState::Rejected };

        GovernanceStorage::set_proposal(&env, proposal_id, &proposal);
        let active_count = GovernanceStorage::active_proposal_count(&env);
        if active_count > 0 {
            GovernanceStorage::set_active_proposal_count(&env, active_count - 1);
        }
        GovernanceEvents::proposal_finalized(&env, proposal_id, &proposal.state);
        Ok(())
    }

    /// Execute a passed proposal. Admin only.
    pub fn execute(env: Env, admin: Address, proposal_id: u64) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let mut proposal = GovernanceStorage::proposal(&env, proposal_id)
            .ok_or(ContractError::ProposalNotFound)?;

        if proposal.state != ProposalState::Passed {
            return Err(ContractError::ProposalNotPassed);
        }

        // Invoke treasury disbursement if a payload is attached
        if let Some(action) = proposal.treasury_action.get(0) {
            if let Some(treasury_addr) = GovernanceStorage::treasury_contract(&env) {
                let treasury = TreasuryClient::new(&env, &treasury_addr);
                treasury.disburse(&action);
            }
        }

        proposal.state = ProposalState::Executed;
        GovernanceStorage::set_proposal(&env, proposal_id, &proposal);
        GovernanceEvents::proposal_executed(&env, proposal_id, &admin);
        Ok(())
    }

    /// Cancel an active proposal. Admin only.
    pub fn cancel(env: Env, admin: Address, proposal_id: u64) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let mut proposal = GovernanceStorage::proposal(&env, proposal_id)
            .ok_or(ContractError::ProposalNotFound)?;

        if proposal.state != ProposalState::Active {
            return Err(ContractError::ProposalNotActive);
        }

        proposal.state = ProposalState::Cancelled;
        GovernanceStorage::set_proposal(&env, proposal_id, &proposal);
        let active_count = GovernanceStorage::active_proposal_count(&env);
        if active_count > 0 {
            GovernanceStorage::set_active_proposal_count(&env, active_count - 1);
        }
        GovernanceEvents::proposal_cancelled(&env, proposal_id, &admin, proposal.voter_count);
        Ok(())
    }

    /// Upgrade the governance contract WASM. Admin only.
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        GovernanceEvents::contract_upgraded(&env, &new_wasm_hash);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin operations
    // -----------------------------------------------------------------------

    /// Update the quorum on an active proposal. Admin only.
    pub fn update_quorum(
        env: Env,
        admin: Address,
        proposal_id: u64,
        new_quorum: i128,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        if new_quorum <= 0 {
            return Err(ContractError::InvalidQuorum);
        }

        let token = GovernanceStorage::voting_token(&env);
        let supply = TokenClient::new(&env, &token).total_supply();
        if new_quorum > supply {
            return Err(ContractError::QuorumExceedsSupply);
        }

        let mut proposal = GovernanceStorage::proposal(&env, proposal_id)
            .ok_or(ContractError::ProposalNotFound)?;

        if proposal.state != ProposalState::Active {
            return Err(ContractError::ProposalNotActive);
        }

        // Time-lock: only allowed within the first 10% of the voting period.
        let now = env.ledger().timestamp();
        let duration = proposal.end_time.saturating_sub(proposal.start_time);
        let window = duration / 10;
        if now > proposal.start_time + window {
            return Err(ContractError::QuorumUpdateNotAllowed);
        }

        let old_quorum = proposal.quorum;
        proposal.quorum = new_quorum;
        GovernanceStorage::set_proposal(&env, proposal_id, &proposal);
        GovernanceEvents::quorum_updated(&env, proposal_id, old_quorum, new_quorum);
        Ok(())
    }

    /// Initiate a two-step admin transfer. Current admin only.
    /// The new admin must call `accept_admin` to complete the transfer.
    /// This pattern prevents accidental admin loss and supports multisig accounts.
    pub fn propose_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        // Guard: reject the all-zeros Stellar account (zero-address equivalent).
        // GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF is the strkey
        // encoding of a 32-byte zeroed public key — no valid keypair can sign for it.
        let zero_addr = Address::from_string(
            &soroban_sdk::String::from_str(
                &env,
                "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
            ),
        );
        if new_admin == zero_addr {
            return Err(ContractError::InvalidNewAdmin);
        }

        GovernanceStorage::set_pending_admin(&env, Some(&new_admin));
        GovernanceEvents::admin_transfer_proposed(&env, &admin, &new_admin);
        Ok(())
    }

    /// Update the voting token address. Admin only.
    /// Only allowed when no proposals are currently Active.
    pub fn update_voting_token(
        env: Env,
        admin: Address,
        new_token: Address,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        let active_count = GovernanceStorage::active_proposal_count(&env);
        if active_count > 0 {
            return Err(ContractError::ProposalsStillActive);
        }

        let old_token = GovernanceStorage::voting_token(&env);
        GovernanceStorage::set_voting_token(&env, &new_token);
        GovernanceEvents::voting_token_updated(&env, &old_token, &new_token);
        Ok(())
    }

    /// Accept admin privileges. Called by the pending admin to complete the transfer.
    pub fn accept_admin(env: Env, pending_admin: Address) -> Result<(), ContractError> {
        pending_admin.require_auth();

        let current_pending = GovernanceStorage::pending_admin(&env)
            .ok_or(ContractError::NoPendingAdmin)?;

        if pending_admin != current_pending {
            return Err(ContractError::NotPendingAdmin);
        }

        let previous_admin = GovernanceStorage::admin(&env);
        GovernanceStorage::set_admin(&env, &pending_admin);
        GovernanceStorage::set_pending_admin(&env, None);
        GovernanceEvents::admin_transfer_accepted(&env, &previous_admin, &pending_admin);
        Ok(())
    }

    /// Pause all state-changing operations. Admin only.
    pub fn pause(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        if GovernanceStorage::paused(&env) {
            return Err(ContractError::ContractPaused);
        }
        GovernanceStorage::set_paused(&env, true);
        GovernanceEvents::paused(&env, &admin);
        Ok(())
    }

    /// Unpause the contract. Admin only.
    pub fn unpause(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        Self::assert_admin(&env, &admin)?;

        if !GovernanceStorage::paused(&env) {
            return Err(ContractError::NotPaused);
        }
        GovernanceStorage::set_paused(&env, false);
        GovernanceEvents::unpaused(&env, &admin);
        Ok(())
    }

    /// Return the current admin address.
    pub fn admin(env: Env) -> Address {
        GovernanceStorage::admin(&env)
    }

    /// Return the pending admin address, if any.
    pub fn pending_admin(env: Env) -> Option<Address> {
        GovernanceStorage::pending_admin(&env)
    }

    /// Return the contract version.
    pub fn version(env: Env) -> (u32, u32, u32) {
        GovernanceStorage::version(&env)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn assert_ready(env: &Env) -> Result<(), ContractError> {
        if GovernanceStorage::contract_state(env) != ContractState::Ready {
            return Err(ContractError::NotInitialized);
        }
        if GovernanceStorage::paused(env) {
            return Err(ContractError::ContractPaused);
        }
        Ok(())
    }

    fn assert_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        if GovernanceStorage::admin(env) != *caller {
            return Err(ContractError::NotAdmin);
        }
        Ok(())
    }
}
