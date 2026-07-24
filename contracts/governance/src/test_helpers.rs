//! Governance contract — test helpers.

#![cfg(test)]

use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, String};

use crate::{GovernanceContract, GovernanceContractClient};

// Re-export token client for tests
use cosmosvote_token::{TokenContract, TokenContractClient};

pub struct TestEnv<'a> {
    pub env: &'a Env,
    pub governance: GovernanceContractClient<'a>,
    pub token: TokenContractClient<'a>,
    pub admin: Address,
    pub voter_a: Address,
    pub voter_b: Address,
    pub voter_c: Address,
}

/// Set up a complete test environment with deployed contracts and funded voters.
pub fn setup<'a>(env: &'a Env) -> TestEnv<'a> {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let voter_a = Address::generate(env);
    let voter_b = Address::generate(env);
    let voter_c = Address::generate(env);

    // Deploy token
    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(env, &token_id);
    token.initialize(
        &admin,
        &1_000_000_000i128,
        &soroban_sdk::String::from_str(env, "CosmosVote"),
        &soroban_sdk::String::from_str(env, "VOTE"),
        &7u32,
    );

    // Fund voters
    token.mint(&admin, &voter_a, &10_000_000i128);
    token.mint(&admin, &voter_b, &5_000_000i128);
    token.mint(&admin, &voter_c, &3_000_000i128);

    // Deploy governance
    let gov_id = env.register(GovernanceContract, ());
    let governance = GovernanceContractClient::new(env, &gov_id);
    governance.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

    TestEnv { env, governance, token, admin, voter_a, voter_b, voter_c }
}

/// Create a standard test proposal.
pub fn create_proposal(t: &TestEnv, proposer: &Address) -> u64 {
    t.governance.create_proposal(
        proposer,
        &String::from_str(t.env, "Test Proposal"),
        &String::from_str(t.env, "A test governance proposal for CosmosVote"),
        &5_000_000i128,
        &604_800u64,
        &None,
        &None,
    )
}

/// Advance ledger past the proposal end time.
pub fn advance_past_end(env: &Env, end_time: u64) {
    env.ledger().with_mut(|l| l.timestamp = end_time + 1);
}
