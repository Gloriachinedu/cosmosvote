//! Integration tests for CosmosVote contracts
//! Tests the full lifecycle of proposals with real contract interactions

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, String};
use cosmosvote_token::{TokenContract, TokenContractClient};
use cosmosvote_governance::{GovernanceContract, GovernanceContractClient};
use cosmosvote_governance::types::{Vote, ProposalState};

// ---------------------------------------------------------------------------
// Setup Helpers
// ---------------------------------------------------------------------------

/// Initialize both contracts and return clients
fn setup_contracts(env: &Env) -> (GovernanceContractClient<'_>, TokenContractClient<'_>, Address, Address, Address, Address) {
    env.mock_all_auths();
    
    let admin = Address::generate(env);
    let voter1 = Address::generate(env);
    let voter2 = Address::generate(env);
    let voter3 = Address::generate(env);

    // Initialize token contract
    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(env, &token_id);
    token.initialize(&admin, &10_000_000i128);
    
    // Distribute tokens to voters
    token.mint(&admin, &voter1, &3_000_000i128); // 30%
    token.mint(&admin, &voter2, &2_000_000i128); // 20%
    token.mint(&admin, &voter3, &1_000_000i128); // 10%
    
    // Initialize governance contract
    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(env, &gov_id);
    gov.initialize(
        &admin,
        &token_id,
        &1000000i128, // min_proposal_balance
        &0u64,       // proposal_cooldown
        &100u32,     // min_quorum_bps (1%)
        &false,      // restrict_admin_vote
        &None,       // treasury contract
        &0u64,       // timelock_seconds (disabled)
    );

    (gov, token, admin, voter1, voter2, voter3)
}

// ---------------------------------------------------------------------------
// Full Pass Lifecycle Tests
// ---------------------------------------------------------------------------

#[test]
fn test_full_lifecycle_pass_and_execute() {
    let env = Env::default();
    let (gov, token, _admin, voter1, voter2, voter3) = setup_contracts(&env);
    
    // 1. Create proposal
    let id = gov.create_proposal(
        &voter1,
        &String::from_str(&env, "Upgrade to v2"),
        &String::from_str(&env, "Upgrade the protocol to version 2"),
        &5_000_000i128,  // quorum: 5M tokens
        &604_800u64,     // 7 days
        &None,           // execution_payload
    ).expect("should create proposal");
    
    assert_eq!(id, 0);
    
    // 2. Verify proposal is Active
    let proposal = gov.get_proposal(id).expect("should get proposal");
    assert_eq!(proposal.state, ProposalState::Active);
    assert_eq!(proposal.votes_yes, 0);
    assert_eq!(proposal.votes_no, 0);
    assert_eq!(proposal.votes_abstain, 0);
    
    // 3. Cast votes (yes majority)
    gov.cast_vote(&voter1, &id, &Vote::Yes).expect("voter1 votes yes");
    gov.cast_vote(&voter2, &id, &Vote::Yes).expect("voter2 votes yes");
    gov.cast_vote(&voter3, &id, &Vote::No).expect("voter3 votes no");
    
    let proposal = gov.get_proposal(id).expect("should get updated proposal");
    assert_eq!(proposal.votes_yes, 5_000_000i128); // voter1: 3M + voter2: 2M
    assert_eq!(proposal.votes_no, 1_000_000i128);
    
    // 4. Finalize proposal (should pass)
    // Fast-forward time to after voting period
    env.ledger().with_mut(|ledger| {
        ledger.set_timestamp(proposal.end_time + 1);
    });
    
    gov.finalize_proposal(&id).expect("should finalize proposal");
    
    let proposal = gov.get_proposal(id).expect("should get finalized proposal");
    assert_eq!(proposal.state, ProposalState::Passed);
    
    // 5. Execute proposal
    gov.execute_proposal(&id).expect("should execute proposal");
    
    let proposal = gov.get_proposal(id).expect("should get executed proposal");
    assert_eq!(proposal.state, ProposalState::Executed);
}

#[test]
fn test_full_lifecycle_reject() {
    let env = Env::default();
    let (gov, _token, _admin, voter1, voter2, voter3) = setup_contracts(&env);
    
    // 1. Create proposal
    let id = gov.create_proposal(
        &voter1,
        &String::from_str(&env, "Reduce rewards"),
        &String::from_str(&env, "Reduce validator rewards"),
        &5_000_000i128,
        &604_800u64,
        &None,
    ).expect("should create proposal");
    
    // 2. Cast votes (no majority)
    gov.cast_vote(&voter1, &id, &Vote::No).expect("voter1 votes no");
    gov.cast_vote(&voter2, &id, &Vote::No).expect("voter2 votes no");
    gov.cast_vote(&voter3, &id, &Vote::Yes).expect("voter3 votes yes");
    
    let proposal = gov.get_proposal(id).expect("should get proposal");
    assert_eq!(proposal.votes_yes, 1_000_000i128);
    assert_eq!(proposal.votes_no, 5_000_000i128);
    
    // 3. Finalize proposal (should reject)
    env.ledger().with_mut(|ledger| {
        ledger.set_timestamp(proposal.end_time + 1);
    });
    
    gov.finalize_proposal(&id).expect("should finalize proposal");
    
    let proposal = gov.get_proposal(id).expect("should get finalized proposal");
    assert_eq!(proposal.state, ProposalState::Rejected);
}

#[test]
fn test_full_lifecycle_cancel() {
    let env = Env::default();
    let (gov, _token, admin, voter1, _voter2, _voter3) = setup_contracts(&env);
    
    // 1. Create proposal
    let id = gov.create_proposal(
        &voter1,
        &String::from_str(&env, "Test proposal"),
        &String::from_str(&env, "A test proposal"),
        &5_000_000i128,
        &604_800u64,
        &None,
    ).expect("should create proposal");
    
    // 2. Cancel proposal (admin only)
    gov.cancel_proposal(&admin, &id).expect("should cancel proposal");
    
    let proposal = gov.get_proposal(id).expect("should get finalized proposal");
    assert_eq!(proposal.state, ProposalState::Cancelled);
}

// ---------------------------------------------------------------------------
// Integration with Token Contract
// ---------------------------------------------------------------------------

#[test]
fn test_voting_power_from_token_contract() {
    let env = Env::default();
    let (gov, token, admin, voter1, _voter2, _voter3) = setup_contracts(&env);
    
    // Verify token balances are correct
    assert_eq!(token.balance(&voter1), 3_000_000i128);
    
    // Create and vote on proposal
    let id = gov.create_proposal(
        &voter1,
        &String::from_str(&env, "Test voting power"),
        &String::from_str(&env, "Verify voting power"),
        &1_000_000i128,
        &604_800u64,
        &None,
    ).expect("should create proposal");
    
    // Cast vote
    gov.cast_vote(&voter1, &id, &Vote::Yes).expect("voter1 should vote");
    
    // Verify voting power matches token balance
    let proposal = gov.get_proposal(id).expect("should get proposal");
    assert_eq!(proposal.votes_yes, 3_000_000i128);
}

#[test]
fn test_proposal_requires_quorum() {
    let env = Env::default();
    let (gov, _token, _admin, voter1, _voter2, _voter3) = setup_contracts(&env);
    
    // Create proposal with high quorum
    let id = gov.create_proposal(
        &voter1,
        &String::from_str(&env, "High quorum proposal"),
        &String::from_str(&env, "A proposal with high quorum"),
        &9_000_000i128,  // 9M quorum (higher than any individual voter)
        &604_800u64,
    ).expect("should create proposal");
    
    // Cast vote
    gov.cast_vote(&voter1, &id, &Vote::Yes).expect("voter1 should vote");
    
    // Finalize - should reject because quorum not met
    let proposal = gov.get_proposal(id).expect("should get proposal");
    env.ledger().with_mut(|ledger| {
        ledger.set_timestamp(proposal.end_time + 1);
    });
    
    gov.finalize_proposal(&id).expect("should finalize proposal");
    
    let proposal = gov.get_proposal(id).expect("should get finalized proposal");
    assert_eq!(proposal.state, ProposalState::Rejected);
}

// ---------------------------------------------------------------------------
// Multiple Proposals Lifecycle
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_proposals_independent() {
    let env = Env::default();
    let (gov, _token, _admin, voter1, voter2, _voter3) = setup_contracts(&env);
    
    // Create first proposal
    let id1 = gov.create_proposal(
        &voter1,
        &String::from_str(&env, "Proposal 1"),
        &String::from_str(&env, "First proposal"),
        &1_000_000i128,
        &604_800u64,
    ).expect("should create proposal 1");
    
    // Create second proposal
    let id2 = gov.create_proposal(
        &voter2,
        &String::from_str(&env, "Proposal 2"),
        &String::from_str(&env, "Second proposal"),
        &1_000_000i128,
        &604_800u64,
    ).expect("should create proposal 2");
    
    assert_eq!(id1, 0);
    assert_eq!(id2, 1);
    
    // Vote on both independently
    gov.cast_vote(&voter1, &id1, &Vote::Yes).expect("voter1 votes on proposal 1");
    gov.cast_vote(&voter2, &id2, &Vote::Yes).expect("voter2 votes on proposal 2");
    
    // Verify votes are independent
    let proposal1 = gov.get_proposal(id1).expect("should get proposal 1");
    let proposal2 = gov.get_proposal(id2).expect("should get proposal 2");
    
    assert_eq!(proposal1.votes_yes, 3_000_000i128);
    assert_eq!(proposal2.votes_yes, 2_000_000i128);
}

// ---------------------------------------------------------------------------
// Snapshot Voting Tests (Issue #23)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_voting_prevents_post_creation_accumulation() {
    let env = Env::default();
    let (gov, token, admin, voter1, _voter2, _voter3) = setup_contracts(&env);
    
    // Create proposal at ledger sequence S
    let id = gov.create_proposal(
        &voter1,
        &String::from_str(&env, "Snapshot test"),
        &String::from_str(&env, "Test snapshot voting"),
        &1_000_000i128,
        &604_800u64,
    ).expect("should create proposal");
    
    let proposal = gov.get_proposal(id).expect("should get proposal");
    let snapshot_ledger = proposal.snapshot_ledger;
    
    // Verify voter1 has 3M tokens at snapshot
    assert_eq!(token.balance(&voter1), 3_000_000i128);
    
    // Now mint new tokens to voter1 (simulating post-creation accumulation)
    token.mint(&admin, &voter1, &5_000_000i128);
    assert_eq!(token.balance(&voter1), 8_000_000i128);
    
    // Vote should only count 3M (balance at snapshot), not 8M
    gov.cast_vote(&voter1, &id, &Vote::Yes).expect("voter1 votes");
    
    let proposal = gov.get_proposal(id).expect("should get proposal");
    // Voting power should be from snapshot (3M), not current balance (8M)
    assert_eq!(proposal.votes_yes, 3_000_000i128);
}

#[test]
fn test_snapshot_voting_fixed_at_proposal_creation() {
    let env = Env::default();
    let (gov, token, admin, voter1, voter2, _voter3) = setup_contracts(&env);
    
    // Initial state: voter1 has 3M, voter2 has 2M
    assert_eq!(token.balance(&voter1), 3_000_000i128);
    assert_eq!(token.balance(&voter2), 2_000_000i128);
    
    // Create proposal
    let id = gov.create_proposal(
        &voter1,
        &String::from_str(&env, "Snapshot fixed test"),
        &String::from_str(&env, "Voting power should be fixed"),
        &1_000_000i128,
        &604_800u64,
        &None,
    ).expect("should create proposal");
    
    // Transfer tokens from voter1 to voter2
    token.transfer(&voter1, &voter2, &2_000_000i128).expect("transfer tokens");
    assert_eq!(token.balance(&voter1), 1_000_000i128);
    assert_eq!(token.balance(&voter2), 4_000_000i128);
    
    // voter1 votes with the old balance (3M from snapshot, not 1M current)
    gov.cast_vote(&voter1, &id, &Vote::Yes).expect("voter1 votes");
    
    // voter2 votes with the old balance (2M from snapshot, not 4M current)
    gov.cast_vote(&voter2, &id, &Vote::Yes).expect("voter2 votes");
    
    let proposal = gov.get_proposal(id).expect("should get proposal");
    // Total votes should be 3M + 2M (snapshot), not 1M + 4M (current)
    assert_eq!(proposal.votes_yes, 5_000_000i128);
}
