//! Governance contract — property-based tests.

#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};
use cosmosvote_token::{TokenContract, TokenContractClient};
use crate::{types::Vote, GovernanceContract, GovernanceContractClient};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    #[test]
    fn prop_no_double_vote(yes_weight in 1..100_000_000i128) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let voter = Address::generate(&env);
        let token_id = env.register(TokenContract, ());
        let token = TokenContractClient::new(&env, &token_id);
        
        token.initialize(
            &admin, 
            &1_000_000_000i128, 
            &String::from_str(&env, "CosmosVote"), 
            &String::from_str(&env, "VOTE"), 
            &7u32
        );
        token.mint(&admin, &voter, &yes_weight);

        let gov_id = env.register(GovernanceContract, ());
        let gov = GovernanceContractClient::new(&env, &gov_id);
        gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

        let id = gov.create_proposal(
            &voter,
            &String::from_str(&env, "Prop"),
            &String::from_str(&env, "Description"),
            &1i128,
            &3600u64,
            &None,
            &None,
        );

        gov.cast_vote(&voter, &id, &Vote::Yes);
        prop_assert!(gov.has_voted(&id, &voter));

        let result = gov.try_cast_vote(&voter, &id, &Vote::No);
        prop_assert!(result.is_err());
    }

    #[test]
    fn prop_vote_arithmetic_overflow_prevented(
        yes_weight in 100_000_000_000_000_000_000_000_000_000_000i128..i128::MAX,
        no_weight in 100_000_000_000_000_000_000_000_000_000_000i128..i128::MAX
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let voter_a = Address::generate(&env);
        let voter_b = Address::generate(&env);
        
        let token_id = env.register(TokenContract, ());
        let token = TokenContractClient::new(&env, &token_id);
        token.initialize(
            &admin, 
            &i128::MAX, 
            &String::from_str(&env, "CosmosVote"), 
            &String::from_str(&env, "VOTE"), 
            &7u32
        );
        token.mint(&admin, &voter_a, &yes_weight);
        token.mint(&admin, &voter_b, &no_weight);

        let gov_id = env.register(GovernanceContract, ());
        let gov = GovernanceContractClient::new(&env, &gov_id);
        gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

        let id = gov.create_proposal(
            &voter_a,
            &String::from_str(&env, "Overflow Test"),
            &String::from_str(&env, "Total votes must not exceed i128::MAX"),
            &1i128,
            &3600u64,
            &None,
            &None,
        );

        gov.cast_vote(&voter_a, &id, &Vote::Yes);
        
        // This should fail with ArithmeticOverflow if yes + no > i128::MAX
        // (but minting already might have failed if supply > i128::MAX, 
        //  so we use weights that sum to > i128::MAX but are individually < i128::MAX)
        let result = gov.try_cast_vote(&voter_b, &id, &Vote::No);
        if yes_weight.checked_add(no_weight).is_none() {
            prop_assert!(result.is_err());
        } else {
            prop_assert!(result.is_ok());
        }
    }

    /// A tied vote (yes == no) is always rejected.
    #[test]
    fn prop_tie_results_in_rejection(
        tie_weight in 1_000_000i128..10_000_000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let voter_yes = Address::generate(&env);
        let voter_no = Address::generate(&env);

        let token_id = env.register(TokenContract, ());
        let token = TokenContractClient::new(&env, &token_id);
        token.initialize(&admin, &1_000_000_000i128);
        token.mint(&admin, &voter_yes, &tie_weight);
        token.mint(&admin, &voter_no, &tie_weight);

        let gov_id = env.register(GovernanceContract, ());
        let gov = GovernanceContractClient::new(&env, &gov_id);
        gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

        let quorum = tie_weight; // total will be 2*tie_weight >= quorum
        let id = gov.create_proposal(
            &voter_yes,
            &String::from_str(&env, "Tie Test"),
            &String::from_str(&env, "Should be rejected on tie"),
            &quorum,
            &3600u64,
        );

        gov.cast_vote(&voter_yes, &id, &Vote::Yes);
        gov.cast_vote(&voter_no, &id, &Vote::No);

        let proposal = gov.get_proposal(&id);
        env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
        gov.finalise(&id);

        prop_assert_eq!(gov.get_proposal(&id).state, ProposalState::Rejected);
    }

    /// Abstain votes count toward quorum: yes+no+abstain >= quorum with yes > no passes.
    #[test]
    fn prop_abstain_counts_toward_quorum(
        yes_weight in 4_000_000i128..8_000_000i128,
        no_weight in 1_000_000i128..3_000_000i128,
        abstain_weight in 1_000_000i128..5_000_000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let voter_yes = Address::generate(&env);
        let voter_no = Address::generate(&env);
        let voter_abs = Address::generate(&env);

        let token_id = env.register(TokenContract, ());
        let token = TokenContractClient::new(&env, &token_id);
        token.initialize(&admin, &1_000_000_000i128);
        token.mint(&admin, &voter_yes, &yes_weight);
        token.mint(&admin, &voter_no, &no_weight);
        token.mint(&admin, &voter_abs, &abstain_weight);

        let gov_id = env.register(GovernanceContract, ());
        let gov = GovernanceContractClient::new(&env, &gov_id);
        gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

        // Set quorum so abstain is needed to push total over quorum
        let total = yes_weight + no_weight + abstain_weight;
        let quorum = yes_weight + no_weight + 1; // requires abstain to meet quorum
        prop_assume!(total >= quorum);

        let id = gov.create_proposal(
            &voter_yes,
            &String::from_str(&env, "Abstain Quorum Test"),
            &String::from_str(&env, "Abstain pushes over quorum"),
            &quorum,
            &3600u64,
        );

        gov.cast_vote(&voter_yes, &id, &Vote::Yes);
        gov.cast_vote(&voter_no, &id, &Vote::No);
        gov.cast_vote(&voter_abs, &id, &Vote::Abstain);

        let proposal = gov.get_proposal(&id);
        env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
        gov.finalise(&id);

        // yes > no and total >= quorum => Passed
        prop_assert_eq!(gov.get_proposal(&id).state, ProposalState::Passed);
    }

    /// When total votes < quorum, proposal is always rejected regardless of yes/no split.
    #[test]
    fn prop_quorum_not_met_always_rejected(
        yes_weight in 1_000_000i128..10_000_000i128,
        no_weight in 1_000_000i128..10_000_000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let voter_yes = Address::generate(&env);
        let voter_no = Address::generate(&env);

        let token_id = env.register(TokenContract, ());
        let token = TokenContractClient::new(&env, &token_id);
        token.initialize(&admin, &1_000_000_000i128);
        token.mint(&admin, &voter_yes, &yes_weight);
        token.mint(&admin, &voter_no, &no_weight);

        let gov_id = env.register(GovernanceContract, ());
        let gov = GovernanceContractClient::new(&env, &gov_id);
        gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

        let total = yes_weight + no_weight;
        let quorum = total + 1; // always one more than total votes cast

        let id = gov.create_proposal(
            &voter_yes,
            &String::from_str(&env, "Quorum Miss Test"),
            &String::from_str(&env, "Quorum not met"),
            &quorum,
            &3600u64,
        );

        gov.cast_vote(&voter_yes, &id, &Vote::Yes);
        gov.cast_vote(&voter_no, &id, &Vote::No);

        let proposal = gov.get_proposal(&id);
        env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
        gov.finalise(&id);

        prop_assert_eq!(gov.get_proposal(&id).state, ProposalState::Rejected);
    }

    /// A tied vote (yes == no) is always rejected.
    #[test]
    fn prop_tie_results_in_rejection(
        tie_weight in 1_000_000i128..10_000_000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let voter_yes = Address::generate(&env);
        let voter_no = Address::generate(&env);

        let token_id = env.register(TokenContract, ());
        let token = TokenContractClient::new(&env, &token_id);
        token.initialize(&admin, &1_000_000_000i128);
        token.mint(&admin, &voter_yes, &tie_weight);
        token.mint(&admin, &voter_no, &tie_weight);

        let gov_id = env.register(GovernanceContract, ());
        let gov = GovernanceContractClient::new(&env, &gov_id);
        gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

        let quorum = tie_weight; // total will be 2*tie_weight >= quorum
        let id = gov.create_proposal(
            &voter_yes,
            &String::from_str(&env, "Tie Test"),
            &String::from_str(&env, "Should be rejected on tie"),
            &quorum,
            &3600u64,
        );

        gov.cast_vote(&voter_yes, &id, &Vote::Yes);
        gov.cast_vote(&voter_no, &id, &Vote::No);

        let proposal = gov.get_proposal(&id);
        env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
        gov.finalise(&id);

        prop_assert_eq!(gov.get_proposal(&id).state, ProposalState::Rejected);
    }

    /// Abstain votes count toward quorum: yes+no+abstain >= quorum with yes > no passes.
    #[test]
    fn prop_abstain_counts_toward_quorum(
        yes_weight in 4_000_000i128..8_000_000i128,
        no_weight in 1_000_000i128..3_000_000i128,
        abstain_weight in 1_000_000i128..5_000_000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let voter_yes = Address::generate(&env);
        let voter_no = Address::generate(&env);
        let voter_abs = Address::generate(&env);

        let token_id = env.register(TokenContract, ());
        let token = TokenContractClient::new(&env, &token_id);
        token.initialize(&admin, &1_000_000_000i128);
        token.mint(&admin, &voter_yes, &yes_weight);
        token.mint(&admin, &voter_no, &no_weight);
        token.mint(&admin, &voter_abs, &abstain_weight);

        let gov_id = env.register(GovernanceContract, ());
        let gov = GovernanceContractClient::new(&env, &gov_id);
        gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

        // Set quorum so abstain is needed to push total over quorum
        let total = yes_weight + no_weight + abstain_weight;
        let quorum = yes_weight + no_weight + 1; // requires abstain to meet quorum
        prop_assume!(total >= quorum);

        let id = gov.create_proposal(
            &voter_yes,
            &String::from_str(&env, "Abstain Quorum Test"),
            &String::from_str(&env, "Abstain pushes over quorum"),
            &quorum,
            &3600u64,
        );

        gov.cast_vote(&voter_yes, &id, &Vote::Yes);
        gov.cast_vote(&voter_no, &id, &Vote::No);
        gov.cast_vote(&voter_abs, &id, &Vote::Abstain);

        let proposal = gov.get_proposal(&id);
        env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
        gov.finalise(&id);

        // yes > no and total >= quorum => Passed
        prop_assert_eq!(gov.get_proposal(&id).state, ProposalState::Passed);
    }

    /// When total votes < quorum, proposal is always rejected regardless of yes/no split.
    #[test]
    fn prop_quorum_not_met_always_rejected(
        yes_weight in 1_000_000i128..10_000_000i128,
        no_weight in 1_000_000i128..10_000_000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let voter_yes = Address::generate(&env);
        let voter_no = Address::generate(&env);

        let token_id = env.register(TokenContract, ());
        let token = TokenContractClient::new(&env, &token_id);
        token.initialize(&admin, &1_000_000_000i128);
        token.mint(&admin, &voter_yes, &yes_weight);
        token.mint(&admin, &voter_no, &no_weight);

        let gov_id = env.register(GovernanceContract, ());
        let gov = GovernanceContractClient::new(&env, &gov_id);
        gov.initialize(&admin, &token_id, &0i128, &0u64, &0u32, &false, &None, &0u64);

        let total = yes_weight + no_weight;
        let quorum = total + 1; // always one more than total votes cast

        let id = gov.create_proposal(
            &voter_yes,
            &String::from_str(&env, "Quorum Miss Test"),
            &String::from_str(&env, "Quorum not met"),
            &quorum,
            &3600u64,
        );

        gov.cast_vote(&voter_yes, &id, &Vote::Yes);
        gov.cast_vote(&voter_no, &id, &Vote::No);

        let proposal = gov.get_proposal(&id);
        env.ledger().with_mut(|l| l.timestamp = proposal.end_time + 1);
        gov.finalise(&id);

        prop_assert_eq!(gov.get_proposal(&id).state, ProposalState::Rejected);
    }
}
