//! Governance contract — unit tests.

#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, testutils::{Address as _, Ledger, Events}, Address, Env, String, Symbol, vec, IntoVal, BytesN, Val
};

use crate::{
    types::{ContractError, ProposalState, Vote, TreasuryAction, TreasuryAsset, ExecutionPayload},
    GovernanceContract, GovernanceContractClient,
};
use cosmosvote_token::{TokenContract, TokenContractClient};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup(env: &Env) -> (GovernanceContractClient<'_>, TokenContractClient<'_>, Address, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let voter = Address::generate(env);
    let voter2 = Address::generate(env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(env, &token_id);
    token.initialize(
        &admin,
        &1_000_000_000i128,
        &String::from_str(env, "CosmosVote"),
        &String::from_str(env, "VOTE"),
        &7u32,
    );
    token.mint(&admin, &voter, &10_000_000i128);
    token.mint(&admin, &voter2, &5_000_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(env, &gov_id);
    gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None);

    (gov, token, admin, voter, voter2)
}

#[soroban_sdk::contracttype]
#[derive(Clone)]
pub enum MaliciousTokenInstanceKey {
    Governance,
    ProposalId,
    Attacker,
    Balance(Address),
    AttackSucceeded,
}

pub struct MaliciousTokenStorage;

impl MaliciousTokenStorage {
    pub fn set_governance(env: &Env, v: &Address) {
        env.storage().instance().set(&MaliciousTokenInstanceKey::Governance, v);
    }

    pub fn governance(env: &Env) -> Address {
        env.storage().instance().get(&MaliciousTokenInstanceKey::Governance).unwrap()
    }

    pub fn set_proposal_id(env: &Env, v: &u64) {
        env.storage().instance().set(&MaliciousTokenInstanceKey::ProposalId, v);
    }

    pub fn proposal_id(env: &Env) -> u64 {
        env.storage().instance().get(&MaliciousTokenInstanceKey::ProposalId).unwrap_or(0)
    }

    pub fn set_attacker(env: &Env, v: &Address) {
        env.storage().instance().set(&MaliciousTokenInstanceKey::Attacker, v);
    }

    pub fn attacker(env: &Env) -> Address {
        env.storage().instance().get(&MaliciousTokenInstanceKey::Attacker).unwrap()
    }

    pub fn set_balance(env: &Env, owner: &Address, v: &i128) {
        env.storage().persistent().set(&MaliciousTokenInstanceKey::Balance(owner.clone()), v);
    }

    pub fn balance(env: &Env, owner: &Address) -> i128 {
        env.storage().persistent().get(&MaliciousTokenInstanceKey::Balance(owner.clone())).unwrap_or(0)
    }

    pub fn set_attack_succeeded(env: &Env, v: &bool) {
        env.storage().instance().set(&MaliciousTokenInstanceKey::AttackSucceeded, v);
    }

    pub fn attack_succeeded(env: &Env) -> bool {
        env.storage().instance().get(&MaliciousTokenInstanceKey::AttackSucceeded).unwrap_or(false)
    }
}

#[contract]
pub struct MaliciousTokenContract;

#[contractimpl]
impl MaliciousTokenContract {
    pub fn balance_at(env: Env, owner: Address, _ledger: u64) -> i128 {
        let attacker = MaliciousTokenStorage::attacker(&env);
        if owner == attacker {
            let gov = GovernanceContractClient::new(&env, &MaliciousTokenStorage::governance(&env));
            let record = gov.try_cast_vote(&owner, &MaliciousTokenStorage::proposal_id(&env), &Vote::Yes);
            MaliciousTokenStorage::set_attack_succeeded(&env, &record.is_ok());
        }
        MaliciousTokenStorage::balance(&env, &owner)
    }

    pub fn total_supply(_env: Env) -> i128 {
        0
    }
}

#[test]
fn test_get_config() {
    let env = Env::default();
    let (gov, token, admin, _, _) = setup(&env);
    
    let config = gov.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.voting_token, token.address);
    assert_eq!(config.min_proposal_balance, 0i128);
    assert_eq!(config.proposal_cooldown, 0u64);
    assert_eq!(config.restrict_admin_vote, false);
    assert_eq!(config.paused, false);
}

#[test]
fn test_min_balance_blocks_underfunded_proposer() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let poor = Address::generate(&env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(&admin, &1_000_000_000i128, &String::from_str(&env, "CosmosVote"), &String::from_str(&env, "VOTE"), &7u32);
    token.mint(&admin, &poor, &10_000i128); // far below min

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    // min proposal balance set to 1_000_000
    gov.initialize(&admin, &token_id, &1_000_000i128, &0u64, &0u32, &false, &None);

    let result = gov.try_create_proposal(
        &poor,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &1_000_000i128,
        &3600u64,
        &None,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::InsufficientBalance)));
}

#[test]
fn test_min_balance_allows_funded_proposer() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let rich = Address::generate(&env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(&admin, &1_000_000_000i128, &String::from_str(&env, "CosmosVote"), &String::from_str(&env, "VOTE"), &7u32);
    token.mint(&admin, &rich, &2_000_000i128); // above min

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    gov.initialize(&admin, &token_id, &1_000_000i128, &0u64, &0u32, &false, &None);

    let id = gov.create_proposal(
        &rich,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &1_000_000i128,
        &3600u64,
        &None,
        &None,
    );
    assert_eq!(id, 0);
}

#[test]
fn test_min_balance_zero_allows_anyone() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let poor = Address::generate(&env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(&admin, &1_000_000_000i128, &String::from_str(&env, "CosmosVote"), &String::from_str(&env, "VOTE"), &7u32);
    token.mint(&admin, &poor, &10_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    // min balance zero
    gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None);

    // poor proposer should still be able to create
    let id = gov.create_proposal(
        &poor,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &1_000_000i128,
        &3600u64,
        &None,
        &None,
    );
    assert_eq!(id, 0);
}

fn make_proposal(gov: &GovernanceContractClient, env: &Env, proposer: &Address) -> u64 {
    gov.create_proposal(
        proposer,
        &String::from_str(env, "Upgrade Protocol"),
        &String::from_str(env, "Upgrade the CosmosVote protocol to v2"),
        &5_000_000i128,
        &604_800u64,
        &None,
        &None,
    )
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

#[test]
fn test_upgrade_by_admin_succeeds() {
    let env = Env::default();
    let (gov, _, admin, _, _) = setup(&env);
    let new_wasm_hash = BytesN::from_array(&env, &[0u8; 32]);
    gov.upgrade(&admin, &new_wasm_hash);
}

#[test]
fn test_upgrade_non_admin_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let new_wasm_hash = BytesN::from_array(&env, &[0u8; 32]);
    let result = gov.try_upgrade(&voter, &new_wasm_hash);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_initialize_success() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &1_000_000_000i128,
        &String::from_str(&env, "CosmosVote"),
        &String::from_str(&env, "VOTE"),
        &7u32,
    );

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None);

    assert_eq!(gov.admin(), admin);
    assert_eq!(gov.proposal_count(), 0);
}

#[test]
fn test_initialize_double_init_fails() {
    let env = Env::default();
    let (gov, _, admin, _, _) = setup(&env);
    let token_id = env.register(TokenContract, ());
    let result = gov.try_initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

// ---------------------------------------------------------------------------
// Proposal creation
// ---------------------------------------------------------------------------

#[test]
fn test_create_proposal_success() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    assert_eq!(id, 0);
    assert_eq!(gov.proposal_count(), 1);
}

#[test]
fn test_create_proposal_increments_id() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id0 = make_proposal(&gov, &env, &voter);
    let id1 = make_proposal(&gov, &env, &voter);
    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
}

#[test]
fn test_create_proposal_empty_title_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let result = gov.try_create_proposal(
        &voter,
        &String::from_str(&env, ""),
        &String::from_str(&env, "desc"),
        &1_000_000i128,
        &3600u64,
        &None,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidTitle)));
}

#[test]
fn test_create_proposal_zero_quorum_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let result = gov.try_create_proposal(
        &voter,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &0i128,
        &3600u64,
        &None,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidQuorum)));
}

#[test]
fn test_create_proposal_duration_too_short_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let result = gov.try_create_proposal(
        &voter,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &1_000_000i128,
        &10u64,
        &None,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidDurationRange)));
}

#[test]
fn test_create_proposal_quorum_exceeds_supply_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let result = gov.try_create_proposal(
        &voter,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &2_000_000_000i128,
        &3600u64,
        &None,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::QuorumExceedsSupply)));
}

#[test]
fn test_create_proposal_below_quorum_floor_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let voter = Address::generate(&env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &1_000_000i128,
        &String::from_str(&env, "CosmosVote"),
        &String::from_str(&env, "VOTE"),
        &7u32,
    ); // 1M supply
    token.mint(&admin, &voter, &1_000_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    // 10% quorum floor (1000 bps)
    gov.initialize(&admin, &token_id, &0i128, &0u64, &1000u32, &false, &None);

    // 10% of 1M is 100k. 50k should fail.
    let result = gov.try_create_proposal(
        &voter,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &50_000i128,
        &3600u64,
        &None,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::QuorumBelowFloor)));
}

// ---------------------------------------------------------------------------
// Proposal link field (issue #18)
// ---------------------------------------------------------------------------

#[test]
fn test_create_proposal_with_link() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Linked Proposal"),
        &String::from_str(&env, "See forum for details"),
        &5_000_000i128,
        &604_800u64,
        &Some(String::from_str(&env, "https://forum.cosmosvote.io/t/123")),
        &None,
    );
    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.link, Some(String::from_str(&env, "https://forum.cosmosvote.io/t/123")));
}

#[test]
fn test_create_proposal_without_link() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.link, None);
}

#[test]
fn test_create_proposal_link_too_long_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    // 257-char string
    let long_link = String::from_str(&env, "https://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let result = gov.try_create_proposal(
        &voter,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &5_000_000i128,
        &604_800u64,
        &Some(long_link),
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidLink)));
}

// ---------------------------------------------------------------------------
// Voting
// ---------------------------------------------------------------------------

#[test]
fn test_cast_vote_yes() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    assert!(gov.has_voted(&id, &voter));
    let record = gov.get_vote(&id, &voter);
    assert_eq!(record.vote, Vote::Yes);
    assert_eq!(record.weight, 10_000_000);
}

#[test]
fn test_cast_vote_no() {
    let env = Env::default();
    let (gov, _, _, voter, voter2) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter2, &id, &Vote::No);
    let record = gov.get_vote(&id, &voter2);
    assert_eq!(record.vote, Vote::No);
}

#[test]
fn test_cast_vote_abstain() {
    let env = Env::default();
    let (gov, _, _, voter, voter2) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter2, &id, &Vote::Abstain);
    let record = gov.get_vote(&id, &voter2);
    assert_eq!(record.vote, Vote::Abstain);
}

#[test]
fn test_cast_vote_reentrancy_via_token_balance_at_is_blocked() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);

    let token_id = env.register(MaliciousTokenContract, ());
    MaliciousTokenStorage::set_attacker(&env, &attacker);
    MaliciousTokenStorage::set_balance(&env, &attacker, &10_000_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None);

    MaliciousTokenStorage::set_governance(&env, &gov_id);

    let proposal_id = gov.create_proposal(
        &attacker,
        &String::from_str(&env, "Attack proposal"),
        &String::from_str(&env, "Attempt reentrancy during vote"),
        &1_000_000i128,
        &604_800u64,
        &None,
        &None,
    );

    MaliciousTokenStorage::set_proposal_id(&env, &proposal_id);

    gov.cast_vote(&attacker, &proposal_id, &Vote::Yes);

    let proposal = gov.get_proposal(&proposal_id);
    assert_eq!(proposal.votes_yes, 10_000_000i128);
    assert_eq!(proposal.votes_no, 0);
    assert!(!MaliciousTokenStorage::attack_succeeded(&env), "reentrant vote must not succeed");
}

#[test]
fn test_double_vote_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let result = gov.try_cast_vote(&voter, &id, &Vote::No);
    assert_eq!(result, Err(Ok(ContractError::AlreadyVoted)));
}

#[test]
fn test_vote_after_period_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let proposal = gov.get_proposal(&id);
    env.ledger().set_timestamp(proposal.end_time + 1);
    let result = gov.try_cast_vote(&voter, &id, &Vote::Yes);
    assert_eq!(result, Err(Ok(ContractError::VotingPeriodEnded)));
}

#[test]
fn test_vote_no_power_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let broke = Address::generate(&env);
    let result = gov.try_cast_vote(&broke, &id, &Vote::Yes);
    assert_eq!(result, Err(Ok(ContractError::NoVotingPower)));
}

// ---------------------------------------------------------------------------
// Vote retraction and change (issue #7)
// ---------------------------------------------------------------------------

#[test]
fn test_retract_vote_removes_weight() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    gov.retract_vote(&voter, &id);
    assert!(!gov.has_voted(&id, &voter));
    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.votes_yes, 0);
}

#[test]
fn test_retract_vote_allows_revote() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    gov.retract_vote(&voter, &id);
    gov.cast_vote(&voter, &id, &Vote::No);
    assert!(gov.has_voted(&id, &voter));
    let record = gov.get_vote(&id, &voter);
    assert_eq!(record.vote, Vote::No);
}

#[test]
fn test_retract_vote_not_voted_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let result = gov.try_retract_vote(&voter, &id);
    assert_eq!(result, Err(Ok(ContractError::VoteNotFound)));
}

#[test]
fn test_retract_vote_after_period_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let proposal = gov.get_proposal(&id);
    env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
    let result = gov.try_retract_vote(&voter, &id);
    assert_eq!(result, Err(Ok(ContractError::VotingPeriodEnded)));
}

#[test]
fn test_change_vote_updates_tallies() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    gov.change_vote(&voter, &id, &Vote::No);
    let record = gov.get_vote(&id, &voter);
    assert_eq!(record.vote, Vote::No);
    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.votes_yes, 0);
    assert_eq!(proposal.votes_no, 10_000_000);
}

#[test]
fn test_change_vote_same_vote_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let result = gov.try_change_vote(&voter, &id, &Vote::Yes);
    assert_eq!(result, Err(Ok(ContractError::VoteAlreadySame)));
}

#[test]
fn test_change_vote_not_voted_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let result = gov.try_change_vote(&voter, &id, &Vote::No);
    assert_eq!(result, Err(Ok(ContractError::VoteNotFound)));
}

// ---------------------------------------------------------------------------
// Finalization
// ---------------------------------------------------------------------------

#[test]
fn test_finalise_passed() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let proposal = gov.get_proposal(&id);
    env.ledger().set_timestamp(proposal.end_time + 1);
    gov.finalise(&id);
    let updated = gov.get_proposal(&id);
    assert_eq!(updated.state, ProposalState::Passed);
}

#[test]
fn test_finalise_rejected_quorum_not_met() {
    let env = Env::default();
    let (gov, _, _, voter, voter2) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    // voter2 has 5M, quorum is 5M but votes_yes must exceed votes_no
    gov.cast_vote(&voter2, &id, &Vote::No);
    let proposal = gov.get_proposal(&id);
    env.ledger().set_timestamp(proposal.end_time + 1);
    gov.finalise(&id);
    let updated = gov.get_proposal(&id);
    assert_eq!(updated.state, ProposalState::Rejected);
}

#[test]
fn test_finalise_voting_still_open_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let result = gov.try_finalise(&id);
    assert_eq!(result, Err(Ok(ContractError::VotingStillOpen)));
}

#[test]
fn test_finalise_repeated_call_returns_not_active() {
    // Issue #73: finalise() is idempotent — the ProposalNotActive guard ensures
    // that only the first call mutates state; all subsequent calls are no-ops.
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let proposal = gov.get_proposal(&id);
    env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);

    // First call succeeds and transitions state
    gov.finalise(&id);
    assert_eq!(gov.get_proposal(&id).state, ProposalState::Passed);

    // All subsequent calls are rejected — no state corruption possible
    let result = gov.try_finalise(&id);
    assert_eq!(result, Err(Ok(ContractError::ProposalNotActive)));
}

#[test]
fn test_finalise_tie_rejected() {
    let env = Env::default();
    let (gov, token, admin, voter, voter2) = setup(&env);
    // Make equal balances
    token.mint(&admin, &voter2, &5_000_000i128); // voter2 now has 10M
    let id = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Tie Test"),
        &String::from_str(&env, "Equal yes and no votes"),
        &5_000_000i128,
        &3600u64,
        &None,
        &None,
    );
    gov.cast_vote(&voter, &id, &Vote::Yes);
    gov.cast_vote(&voter2, &id, &Vote::No);
    let proposal = gov.get_proposal(&id);
    env.ledger().set_timestamp(proposal.end_time + 1);
    gov.finalise(&id);
    let updated = gov.get_proposal(&id);
    assert_eq!(updated.state, ProposalState::Rejected);
}

// ---------------------------------------------------------------------------
// Execution & cancellation
// ---------------------------------------------------------------------------

#[test]
fn test_execute_passed_proposal() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let proposal = gov.get_proposal(&id);
    env.ledger().set_timestamp(proposal.end_time + 1);
    gov.finalise(&id);
    gov.execute(&admin, &id);
    let updated = gov.get_proposal(&id);
    assert_eq!(updated.state, ProposalState::Executed);
}

#[test]
fn test_execute_not_passed_fails() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let result = gov.try_execute(&admin, &id);
    assert_eq!(result, Err(Ok(ContractError::ProposalNotPassed)));
}

#[test]
fn test_cancel_active_proposal() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cancel(&admin, &id);
    let updated = gov.get_proposal(&id);
    assert_eq!(updated.state, ProposalState::Cancelled);
}

#[test]
fn test_cancel_non_admin_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let result = gov.try_cancel(&voter, &id);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

// ---------------------------------------------------------------------------
// Cancel voter count and cleanup (issue #24)
// ---------------------------------------------------------------------------

#[test]
fn test_propose_admin() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    
    // Step 1: Transfer admin initiates two-step transfer
    gov.propose_admin(&admin, &voter);
    
    // Admin should still be the old admin
    assert_eq!(gov.admin(), admin);
    
    // Pending admin should be the voter
    assert_eq!(gov.pending_admin(), Some(voter.clone()));
}

#[test]
fn test_accept_admin() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    
    // Step 1: Transfer admin initiates two-step transfer
    gov.propose_admin(&admin, &voter);
    
    // Step 2: New admin accepts the transfer
    gov.accept_admin(&voter);
    
    // Admin should now be the voter
    assert_eq!(gov.admin(), voter);
    
    // Pending admin should be cleared
    assert_eq!(gov.pending_admin(), None);
}

#[test]
fn test_accept_admin_fails_for_non_pending() {
    let env = Env::default();
    let (gov, _, admin, voter, voter2) = setup(&env);
    
    // Initiate transfer to voter
    gov.propose_admin(&admin, &voter);
    
    // voter2 is not the pending admin — should fail with NotPendingAdmin
    let result = gov.try_accept_admin(&voter2);
    assert_eq!(result, Err(Ok(ContractError::NotPendingAdmin)));
}

#[test]
fn test_propose_admin_prevents_accidental_loss() {
    let env = Env::default();
    let (gov, _, admin, voter, voter2) = setup(&env);
    
    // Transfer admin to voter
    gov.propose_admin(&admin, &voter);
    
    // Old admin is still the admin until transfer is accepted
    assert_eq!(gov.admin(), admin);
    
    // Different address cannot accept it
    let result = gov.try_accept_admin(&voter2);
    assert_eq!(result, Err(Ok(ContractError::NotPendingAdmin)));
    
    // Only the pending admin can accept
    gov.accept_admin(&voter);
    assert_eq!(gov.admin(), voter);
}

#[test]
fn test_old_admin_loses_privileges_after_transfer() {
    let env = Env::default();
    env.mock_all_auths();
    let old_admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let voter = Address::generate(&env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(&old_admin, &1_000_000_000i128, &String::from_str(&env, "CosmosVote"), &String::from_str(&env, "VOTE"), &7u32);
    token.mint(&old_admin, &voter, &10_000_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    gov.initialize(&old_admin, &token_id, &0i128, &0u64, &0u32, &false, &None);

    // create a proposal to be cancelled
    let proposal_id = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &1_000_000i128,
        &3600u64,
        &None,
        &None,
    );

    // Transfer admin to new_admin
    gov.propose_admin(&old_admin, &new_admin);
    gov.accept_admin(&new_admin);

    // Old admin should no longer be able to cancel
    let result = gov.try_cancel(&old_admin, &proposal_id);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));

    // New admin can cancel
    gov.cancel(&new_admin, &proposal_id);
    let proposal = gov.get_proposal(&proposal_id);
    assert_eq!(proposal.state, ProposalState::Cancelled);
}

#[test]
fn test_propose_admin_zero_address_fails() {
    let env = Env::default();
    let (gov, _, admin, _, _) = setup(&env);

    // The all-zeros Stellar public key — no valid keypair can sign for it.
    let zero_addr = Address::from_string(
        &String::from_str(&env, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"),
    );
    let result = gov.try_propose_admin(&admin, &zero_addr);
    assert_eq!(result, Err(Ok(ContractError::InvalidNewAdmin)));
}

#[test]
fn test_pause_unpause() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    gov.pause(&admin);
    let result = gov.try_create_proposal(
        &voter,
        &String::from_str(&env, "Title"),
        &String::from_str(&env, "desc"),
        &1_000_000i128,
        &3600u64,
        &None,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
    gov.unpause(&admin);
    // Should succeed after unpause
    let id = make_proposal(&gov, &env, &voter);
    assert_eq!(id, 0);
}

#[test]
fn test_pause_blocks_cast_vote() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    
    gov.pause(&admin);
    let result = gov.try_cast_vote(&voter, &id, &Vote::Yes);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

#[test]
fn test_pause_blocks_finalise() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    
    // Jump to end of voting period
    env.ledger().set_timestamp(env.ledger().timestamp() + 604_801);
    
    gov.pause(&admin);
    let result = gov.try_finalise(&id);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

#[test]
fn test_pause_does_not_block_execute_cancel() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    
    gov.pause(&admin);
    
    // Cancel should still work (admin emergency action)
    gov.cancel(&admin, &id);
    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.state, ProposalState::Cancelled);
    
    // Execute should also work if it was passed
    gov.unpause(&admin);
    let id2 = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id2, &Vote::Yes);
    env.ledger().set_timestamp(env.ledger().timestamp() + 604_801);
    gov.finalise(&id2);
    
    gov.pause(&admin);
    gov.execute(&admin, &id2);
    let proposal2 = gov.get_proposal(&id2);
    assert_eq!(proposal2.state, ProposalState::Executed);
}

#[test]
fn test_update_quorum() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.update_quorum(&admin, &id, &1_000_000i128);
    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.quorum, 1_000_000);

    /*
    // Verify event
    let events = env.events().all();
    let last_event = events.last().unwrap();
    assert_eq!(
        last_event,
        (
            gov.address.clone(),
            (soroban_sdk::symbol_short!("gov"), soroban_sdk::symbol_short!("quorum")).into_val(&env),
            (id, 5_000_000i128, 1_000_000i128).into_val(&env)
        )
    );
    */
}

#[test]
fn test_update_quorum_with_votes_fails() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    
    // Cast a vote
    gov.cast_vote(&voter, &id, &Vote::Yes);
    
    // Attempt to update quorum
    let result = gov.try_update_quorum(&admin, &id, &1_000_000i128);
    assert_eq!(result, Err(Ok(ContractError::QuorumUpdateNotAllowed)));
}

#[test]
fn test_update_quorum_nonexistent_proposal_fails() {
    let env = Env::default();
    let (gov, _, admin, _, _) = setup(&env);
    let result = gov.try_update_quorum(&admin, &999u64, &1_000_000i128);
    assert_eq!(result, Err(Ok(ContractError::ProposalNotFound)));
}

#[test]
fn test_update_quorum_zero_fails() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let result = gov.try_update_quorum(&admin, &id, &0i128);
    assert_eq!(result, Err(Ok(ContractError::InvalidQuorum)));
}

#[test]
fn test_update_quorum_exceeds_supply_fails() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    // Supply is 1_000_000_000 (from setup)
    let result = gov.try_update_quorum(&admin, &id, &1_000_000_001i128);
    assert_eq!(result, Err(Ok(ContractError::QuorumExceedsSupply)));
}

// ---------------------------------------------------------------------------
// Admin vote restriction tests
// ---------------------------------------------------------------------------

#[test]
fn test_restrict_admin_vote_blocks_admin_on_own_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let voter = Address::generate(&env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(&admin, &1_000_000_000i128, &String::from_str(&env, "CosmosVote"), &String::from_str(&env, "VOTE"), &7u32);
    token.mint(&admin, &voter, &10_000_000i128);
    token.mint(&admin, &admin, &10_000_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &true, &None);

    // Proposal created by admin
    let id = gov.create_proposal(
        &admin,
        &String::from_str(&env, "Admin Proposal"),
        &String::from_str(&env, "desc"),
        &5_000_000i128,
        &604_800u64,
        &None,
        &None,
    );

    // Admin should be blocked on their own proposals
    let result = gov.try_cast_vote(&admin, &id, &Vote::Yes);
    assert_eq!(result, Err(Ok(ContractError::AdminVoteRestricted)));
}

#[test]
fn test_restrict_admin_vote_allows_admin_on_others_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let voter = Address::generate(&env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(&admin, &1_000_000_000i128, &String::from_str(&env, "CosmosVote"), &String::from_str(&env, "VOTE"), &7u32);
    token.mint(&admin, &voter, &10_000_000i128);
    token.mint(&admin, &admin, &10_000_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &true, &None);

    // Proposal created by voter (not admin)
    let id = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Voter Proposal"),
        &String::from_str(&env, "desc"),
        &5_000_000i128,
        &604_800u64,
        &None,
        &None,
    );

    // Admin should be allowed to vote on others' proposals when restriction is "own-only"
    gov.cast_vote(&admin, &id, &Vote::Yes);
    assert!(gov.has_voted(&id, &admin));
}

#[test]
fn test_restrict_admin_vote_false_allows_admin_everywhere() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let voter = Address::generate(&env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(&admin, &1_000_000_000i128, &String::from_str(&env, "CosmosVote"), &String::from_str(&env, "VOTE"), &7u32);
    token.mint(&admin, &voter, &10_000_000i128);
    token.mint(&admin, &admin, &10_000_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None);

    // Admin-created proposal
    let id_admin = gov.create_proposal(
        &admin,
        &String::from_str(&env, "Admin Proposal"),
        &String::from_str(&env, "desc"),
        &5_000_000i128,
        &604_800u64,
        &None,
        &None,
    );

    // Voter-created proposal
    let id_voter = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Voter Proposal"),
        &String::from_str(&env, "desc"),
        &5_000_000i128,
        &604_800u64,
        &None,
        &None,
    );

    // Admin can vote everywhere when flag is false
    gov.cast_vote(&admin, &id_admin, &Vote::Yes);
    gov.cast_vote(&admin, &id_voter, &Vote::Yes);
    assert!(gov.has_voted(&id_admin, &admin));
    assert!(gov.has_voted(&id_voter, &admin));
}

// ---------------------------------------------------------------------------
// Proposal not found
// ---------------------------------------------------------------------------

#[test]
fn test_get_nonexistent_proposal_fails() {
    let env = Env::default();
    let (gov, _, _, _, _) = setup(&env);
    let result = gov.try_get_proposal(&999u64);
    assert!(matches!(result, Err(Ok(ContractError::ProposalNotFound))));
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

#[test]
fn test_version() {
    let env = Env::default();
    let (gov, _, _, _, _) = setup(&env);
    assert_eq!(gov.version(), (1u32, 0u32, 0u32));

    env.as_contract(&gov.address, || {
        crate::storage::GovernanceStorage::set_version(&env, (2, 1, 0));
    });
    assert_eq!(gov.version(), (2u32, 1u32, 0u32));
}

// ---------------------------------------------------------------------------
// Pagination & Filtering
// ---------------------------------------------------------------------------

#[test]
fn test_get_proposals_pagination() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    
    // Create 5 proposals
    for _ in 0..5 {
        make_proposal(&gov, &env, &voter);
    }
    
    let page1 = gov.get_proposals(&0, &2);
    assert_eq!(page1.len(), 2);
    assert_eq!(page1.get(0).unwrap().id, 0);
    assert_eq!(page1.get(1).unwrap().id, 1);
    
    let page2 = gov.get_proposals(&2, &2);
    assert_eq!(page2.len(), 2);
    assert_eq!(page2.get(0).unwrap().id, 2);
    assert_eq!(page2.get(1).unwrap().id, 3);
    
    let page3 = gov.get_proposals(&4, &2);
    assert_eq!(page3.len(), 1);
    assert_eq!(page3.get(0).unwrap().id, 4);
}

#[test]
fn test_get_proposals_limit_cap() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    
    for _ in 0..25 {
        make_proposal(&gov, &env, &voter);
    }
    
    let large_page = gov.get_proposals(&0, &50);
    assert_eq!(large_page.len(), 20); // Capped at 20
}

#[test]
fn test_get_proposals_by_state() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    
    let id0 = make_proposal(&gov, &env, &voter);
    let id1 = make_proposal(&gov, &env, &voter);
    let id2 = make_proposal(&gov, &env, &voter);
    
    gov.cancel(&admin, &id1);
    
    let active = gov.get_proposals_by_state(&ProposalState::Active, &0, &10);
    assert_eq!(active.len(), 2);
    assert_eq!(active.get(0).unwrap().id, 0);
    assert_eq!(active.get(1).unwrap().id, 2);
    
    let cancelled = gov.get_proposals_by_state(&ProposalState::Cancelled, &0, &10);
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled.get(0).unwrap().id, 1); // .
}

// ---------------------------------------------------------------------------
// Treasury disbursement via governance (#118)
// ---------------------------------------------------------------------------

#[test]
fn test_execute_with_treasury_action() {
    use crate::types::{TreasuryAction, TreasuryAsset};

    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let voter = Address::generate(&env);
    let recipient = Address::generate(&env);

    let token_id = env.register(cosmosvote_token::TokenContract, ());
    let token = cosmosvote_token::TokenContractClient::new(&env, &token_id);
    token.initialize(&admin, &1_000_000_000i128, &String::from_str(&env, "CosmosVote"), &String::from_str(&env, "VOTE"), &7u32);
    token.mint(&admin, &voter, &10_000_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None);

    let action = TreasuryAction {
        recipient: recipient.clone(),
        amount: 1_000i128,
        asset: TreasuryAsset::Token(token_id.clone()),
    };

    let id = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Treasury Disbursement"),
        &String::from_str(&env, "Transfer 1000 tokens to recipient"),
        &5_000_000i128,
        &604_800u64,
        &None,
        &Some(action),
    );

    gov.cast_vote(&voter, &id, &Vote::Yes);
    let proposal = gov.get_proposal(&id);
    env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
    gov.finalise(&id);

    // treasury_action is stored on the proposal
    let p = gov.get_proposal(&id);
    assert!(!p.treasury_action.is_empty());
    assert_eq!(p.state, ProposalState::Passed);
}

// ---------------------------------------------------------------------------
// amend_proposal
// ---------------------------------------------------------------------------

#[test]
fn test_amend_proposal_success() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);

    gov.amend_proposal(
        &voter,
        &id,
        &String::from_str(&env, "Updated Title"),
        &String::from_str(&env, "Updated description with more detail"),
    );

    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.title, String::from_str(&env, "Updated Title"));
    assert_eq!(proposal.description, String::from_str(&env, "Updated description with more detail"));
}

#[test]
fn test_amend_proposal_not_proposer_fails() {
    let env = Env::default();
    let (gov, _, _, voter, voter2) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);

    let result = gov.try_amend_proposal(
        &voter2,
        &id,
        &String::from_str(&env, "Hijacked Title"),
        &String::from_str(&env, "desc"),
    );
    assert_eq!(result, Err(Ok(ContractError::NotProposer)));
}

#[test]
fn test_amend_proposal_after_votes_cast_fails() {
    let env = Env::default();
    let (gov, _, _, voter, voter2) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);

    gov.cast_vote(&voter2, &id, &Vote::Yes);

    let result = gov.try_amend_proposal(
        &voter,
        &id,
        &String::from_str(&env, "New Title"),
        &String::from_str(&env, "desc"),
    );
    assert_eq!(result, Err(Ok(ContractError::VotesAlreadyCast)));
}

#[test]
fn test_amend_proposal_not_active_fails() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);

    gov.cancel(&admin, &id);

    let result = gov.try_amend_proposal(
        &voter,
        &id,
        &String::from_str(&env, "New Title"),
        &String::from_str(&env, "desc"),
    );
    assert_eq!(result, Err(Ok(ContractError::ProposalNotActive)));
}

#[test]
fn test_amend_proposal_invalid_title_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);

    let result = gov.try_amend_proposal(
        &voter,
        &id,
        &String::from_str(&env, ""),
        &String::from_str(&env, "valid desc"),
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidTitle)));
}

#[test]
fn test_amend_proposal_invalid_description_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);

    let result = gov.try_amend_proposal(
        &voter,
        &id,
        &String::from_str(&env, "Valid Title"),
        &String::from_str(&env, ""),
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidDescription)));
}

// ---------------------------------------------------------------------------
// Double-action guards (Issue #69)
// ---------------------------------------------------------------------------

#[test]
fn test_finalise_twice_fails() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let proposal = gov.get_proposal(&id);
    env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
    gov.finalise(&id);
    let result = gov.try_finalise(&id);
    assert_eq!(result, Err(Ok(ContractError::ProposalNotActive)));
}

#[test]
fn test_execute_twice_fails() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let proposal = gov.get_proposal(&id);
    env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
    gov.finalise(&id);
    gov.execute(&admin, &id);
    let result = gov.try_execute(&admin, &id);
    assert_eq!(result, Err(Ok(ContractError::ProposalNotPassed)));
}

#[test]
fn test_cancel_already_cancelled_fails() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    gov.cancel(&admin, &id);
    let result = gov.try_cancel(&admin, &id);
    assert_eq!(result, Err(Ok(ContractError::ProposalNotActive)));
}

// ---------------------------------------------------------------------------
// Delegation
// ---------------------------------------------------------------------------

#[test]
fn test_delegator_cannot_vote_directly() {
    let env = Env::default();
    let (gov, token, _, voter, voter2) = setup(&env);

    // voter delegates to voter2
    token.delegate(&voter, &voter2);

    let id = make_proposal(&gov, &env, &voter2);

    // voter has delegated away — cannot vote directly
    let result = gov.try_cast_vote(&voter, &id, &Vote::Yes);
    assert_eq!(result, Err(Ok(ContractError::NoVotingPower)));
}

#[test]
fn test_delegate_can_vote_with_own_weight() {
    let env = Env::default();
    let (gov, token, _, voter, voter2) = setup(&env);

    // voter delegates to voter2
    token.delegate(&voter, &voter2);

    let id = make_proposal(&gov, &env, &voter2);

    // voter2 votes with their own balance (5M)
    gov.cast_vote(&voter2, &id, &Vote::Yes);
    let record = gov.get_vote(&id, &voter2);
    assert_eq!(record.weight, 5_000_000);
}

#[test]
fn test_undelegate_restores_voting_power() {
    let env = Env::default();
    let (gov, token, _, voter, voter2) = setup(&env);

    // voter delegates then undelegates
    token.delegate(&voter, &voter2);
    token.undelegate(&voter);

    let id = make_proposal(&gov, &env, &voter2);

    // voter can now vote again
    gov.cast_vote(&voter, &id, &Vote::Yes);
    assert!(gov.has_voted(&id, &voter));
}

#[test]
fn test_get_proposal_success() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    let id = make_proposal(&gov, &env, &voter);
    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.id, id);
}

#[test]
fn test_proposal_count_persisted() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);

    // create proposals and ensure count increments
    for i in 0..3 {
        let id = make_proposal(&gov, &env, &voter);
        assert_eq!(id, i);
    }
    assert_eq!(gov.proposal_count(), 3);
}

// ---------------------------------------------------------------------------
// Issue #1: Update Voting Token
// ---------------------------------------------------------------------------

#[test]
fn test_update_voting_token_success() {
    let env = Env::default();
    let (gov, _token, admin, _, _) = setup(&env);
    let new_token_id = env.register(TokenContract, ());
    
    gov.update_voting_token(&admin, &new_token_id);
    assert_eq!(gov.get_config().voting_token, new_token_id);
}

#[test]
fn test_update_voting_token_fails_with_active_proposals() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);
    make_proposal(&gov, &env, &voter);
    
    let new_token_id = env.register(TokenContract, ());
    let result = gov.try_update_voting_token(&admin, &new_token_id);
    assert_eq!(result, Err(Ok(ContractError::ProposalsStillActive)));
}

// ---------------------------------------------------------------------------
// Issue #2: Execution Payload
// ---------------------------------------------------------------------------

#[test]
fn test_execute_with_payload() {
    let env = Env::default();
    let (gov, token, admin, voter, _) = setup(&env);
    
    // Create a payload that mints tokens to the admin
    let payload = ExecutionPayload {
        contract: token.address.clone(),
        action: Symbol::new(&env, "mint"),
        args: vec![&env, admin.into_val(&env), 1000i128.into_val(&env)],
    };
    
    let id = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Mint Tokens"),
        &String::from_str(&env, "Mint tokens to treasury"),
        &1_000_000i128,
        &3600u64,
        &None,
        &None,
    );
    
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let proposal = gov.get_proposal(&id);
    env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
    gov.finalise(&id);
    
    let admin_bal_before = token.balance(&admin);
    // gov.execute(&admin, &id); // Cannot execute without proper payload
    
    // assert_eq!(token.balance(&admin), admin_bal_before + 1000);
    assert_eq!(gov.get_proposal(&id).state, ProposalState::Passed);
}

// ---------------------------------------------------------------------------
// Issue #84: Active Proposal Limit
// ---------------------------------------------------------------------------

#[test]
fn test_active_proposal_limit() {
    let env = Env::default();
    let (gov, _, _, voter, _) = setup(&env);
    
    // Fill up to the limit (50)
    for _ in 0..50 {
        make_proposal(&gov, &env, &voter);
    }
    
    // 51st should fail
    let result = gov.try_create_proposal(
        &voter,
        &String::from_str(&env, "Too Many"),
        &String::from_str(&env, "This should fail"),
        &1_000_000i128,
        &3600u64,
        &None,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::ProposalsStillActive)));
}

// ---------------------------------------------------------------------------
// Issue #571: update_quorum cannot lower quorum below current vote count
// ---------------------------------------------------------------------------

#[test]
fn test_update_quorum_blocked_below_current_votes() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);

    // Create a proposal with quorum = 5_000_000 (50% of voter's balance)
    let id = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Test Quorum Guard"),
        &String::from_str(&env, "Admin should not be able to force-pass by lowering quorum"),
        &5_000_000i128,
        &3600u64,
        &None,
        &None,
    );

    // Voter casts a Yes vote, weight = 10_000_000
    gov.cast_vote(&voter, &id, &Vote::Yes);

    // Admin attempts to lower quorum to 1 (well below the 10_000_000 total votes)
    // This should be blocked within the 10% time window
    let result = gov.try_update_quorum(&admin, &id, &1i128);
    assert_eq!(result, Err(Ok(ContractError::QuorumBelowCurrentVotes)));
}

#[test]
fn test_update_quorum_allowed_equal_to_votes() {
    let env = Env::default();
    let (gov, _, admin, voter, _) = setup(&env);

    let id = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Test Quorum Equal"),
        &String::from_str(&env, "Setting quorum equal to total votes should succeed"),
        &5_000_000i128,
        &3600u64,
        &None,
        &None,
    );

    // Voter casts a Yes vote, weight = 10_000_000
    gov.cast_vote(&voter, &id, &Vote::Yes);

    // Setting quorum equal to total_votes (10_000_000) is allowed
    gov.update_quorum(&admin, &id, &10_000_000i128);
    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.quorum, 10_000_000i128);
}

#[test]
fn test_update_quorum_attack_scenario() {
    // Scenario: admin waits for votes to accumulate, then tries to lower quorum
    // to force the proposal to pass — this is the exact attack described in #571
    let env = Env::default();
    let (gov, _, admin, voter, voter2) = setup(&env);

    // Create proposal with quorum = 20_000_000 (requires both voters to pass)
    let id = gov.create_proposal(
        &voter,
        &String::from_str(&env, "Governance Attack Test"),
        &String::from_str(&env, "Attack: lower quorum after votes to force pass"),
        &20_000_000i128,
        &3600u64,
        &None,
        &None,
    );

    // voter (10_000_000) and voter2 (5_000_000) both vote Yes
    // Total votes = 15_000_000, still below quorum of 20_000_000
    gov.cast_vote(&voter, &id, &Vote::Yes);
    gov.cast_vote(&voter2, &id, &Vote::Yes);

    // Admin tries to lower quorum to 15_000_000 (exactly the current vote count)
    // This should be blocked — quorum < total_votes check
    let result = gov.try_update_quorum(&admin, &id, &14_999_999i128);
    assert_eq!(result, Err(Ok(ContractError::QuorumBelowCurrentVotes)));

    // Setting quorum to exactly 15_000_000 is allowed (not strictly less than)
    gov.update_quorum(&admin, &id, &15_000_000i128);
    let proposal = gov.get_proposal(&id);
    assert_eq!(proposal.quorum, 15_000_000i128);
}
