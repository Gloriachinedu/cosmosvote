//! Governance contract — instruction count benchmarks.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, String};
use cosmosvote_token::TokenContractClient;
use crate::{types::Vote, GovernanceContract, GovernanceContractClient};
use cosmosvote_token::TokenContract;

// ---------------------------------------------------------------------------
// Baselines (Target instruction counts)
// ---------------------------------------------------------------------------

const BASELINE_CREATE_PROPOSAL: u64 = 5_000_000;
const BASELINE_CAST_VOTE: u64 = 2_000_000;

// ---------------------------------------------------------------------------
// Benchmark runner
// ---------------------------------------------------------------------------

fn setup_env() -> (Env, GovernanceContractClient<'static>, TokenContractClient<'static>, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let proposer = Address::generate(&env);

    let token_id = env.register(TokenContract, ());
    let token = TokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin, 
        &1_000_000_000_000i128, 
        &String::from_str(&env, "CosmosVote"), 
        &String::from_str(&env, "VOTE"), 
        &7u32
    );
    token.mint(&admin, &proposer, &10_000_000_000i128);

    let gov_id = env.register(GovernanceContract, ());
    let gov = GovernanceContractClient::new(&env, &gov_id);
    gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

    (env, gov, token, admin, proposer)
}

fn threshold(baseline: u64) -> u64 {
    baseline + (baseline / 10)
}

#[test]
fn bench_create_proposal() {
    let (env, gov, _token, _admin, proposer) = setup_env();

    env.as_contract(&gov.address, || {
        env.budget().reset_unlimited();
    });
    
    let instructions_before = env.budget().cpu_instruction_cost();
    gov.create_proposal(
        &proposer,
        &String::from_str(&env, "Vote Benchmark"),
        &String::from_str(&env, "Measuring instruction count for cast_vote"),
        &1_000_000i128,
        &604_800u64,
        &None,
        &None,
    );
    let instructions = env.budget().cpu_instruction_cost() - instructions_before;

    assert!(
        instructions <= threshold(BASELINE_CREATE_PROPOSAL),
        "create_proposal used {} instructions, exceeds 10% over baseline {}",
        instructions,
        BASELINE_CREATE_PROPOSAL
    );
}

#[test]
fn bench_cast_vote() {
    let (env, gov, token, admin, proposer) = setup_env();
    
    let id = gov.create_proposal(
        &proposer,
        &String::from_str(&env, "Vote Benchmark"),
        &String::from_str(&env, "Measuring instruction count for cast_vote"),
        &1_000_000i128,
        &604_800u64,
        &None,
        &None,
    );

    let voter = Address::generate(&env);
    token.mint(&admin, &voter, &1_000i128);

    env.as_contract(&gov.address, || {
        env.budget().reset_unlimited();
    });
    let instructions_before = env.budget().cpu_instruction_cost();
    gov.cast_vote(&voter, &id, &Vote::Yes);
    let instructions = env.budget().cpu_instruction_cost() - instructions_before;

    assert!(
        instructions <= threshold(BASELINE_CAST_VOTE),
        "cast_vote used {} instructions, exceeds 10% over baseline {}",
        instructions,
        BASELINE_CAST_VOTE
    );
}
