use std::{collections::HashSet, time::Duration};

use eyre::bail;
use malachitebft_test_framework::{HandlerResult, TestParams};

use crate::TestBuilder;

#[tokio::test]
pub async fn equivocation_two_vals_same_pk() {
    // Nodes 1 and 2 share a validator key to induce equivocation
    let params = TestParams {
        shared_key_group: HashSet::from([0, 3]),
        ..Default::default()
    };

    // State: count of decide events seen
    let mut test = TestBuilder::<u32>::new();

    // Node 0
    test.add_node().start().success();

    // Node 1 (shares validator key with node 2)
    test.add_node().start().success();

    // Node 2 (shares validator key with node 1)
    test.add_node().start().success();

    // Node 3 -- checking equivocation evidence at decide time
    test.add_node()
        .start()
        .on_decided(|_certificate, evidence, decide_count| {
            *decide_count += 1;

            let has_proposal_evidence = !evidence.proposals.is_empty();
            let has_vote_evidence = !evidence.votes.is_empty();

            // Pass test once we have both proposal and vote equivocation evidence
            if has_proposal_evidence && has_vote_evidence {
                Ok(HandlerResult::ContinueTest)
            } else if *decide_count > 3 {
                // Evidence should appear in first heights, fail after 3 decides
                bail!(
                    "Decided {} times. proposal_evidence={}, vote_evidence={}",
                    decide_count,
                    has_proposal_evidence,
                    has_vote_evidence
                )
            } else {
                Ok(HandlerResult::WaitForNextEvent)
            }
        })
        .success();

    test.build()
        .run_with_params(Duration::from_secs(30), params)
        .await;
}
