// Copyright © Aptos Foundation
// Parts of the project are originally copyright © Meta Platforms, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    smoke_test_environment::{new_local_swarm_with_aptos, SwarmBuilder},
    test_utils::{
        create_and_fund_account, transfer_and_maybe_reconfig, transfer_coins,
        MAX_CATCH_UP_WAIT_SECS, MAX_HEALTHY_WAIT_SECS,
    },
};
use aptos_config::config::{BootstrappingMode, ContinuousSyncingMode, NodeConfig};
use aptos_forge::{LocalSwarm, Node, NodeExt, Swarm, SwarmExt};
use aptos_inspection_service::inspection_client::InspectionClient;
use aptos_rest_client::Client as RestClient;
use aptos_sdk::types::LocalAccount;
use aptos_types::{account_address::AccountAddress, PeerId};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[tokio::test]
async fn test_full_node_bootstrap_state_snapshot() {
    // Test snapshot (fast) syncing in the presence of epoch changes
    perform_full_node_bootstrap_state_snapshot(true).await;
}

#[tokio::test]
async fn test_full_node_bootstrap_state_snapshot_genesis() {
    // Test snapshot (fast) syncing without epoch changes
    perform_full_node_bootstrap_state_snapshot(false).await;
}

#[tokio::test]
async fn test_full_node_bootstrap_outputs() {
    // Create a validator swarm of 1 validator node
    let mut swarm = new_local_swarm_with_aptos(1).await;

    // Create a fullnode config that uses transaction outputs to sync
    let mut vfn_config = NodeConfig::get_default_vfn_config();
    vfn_config.state_sync.state_sync_driver.bootstrapping_mode =
        BootstrappingMode::ApplyTransactionOutputsFromGenesis;
    vfn_config
        .state_sync
        .state_sync_driver
        .continuous_syncing_mode = ContinuousSyncingMode::ApplyTransactionOutputs;
    vfn_config.state_sync.aptos_data_client.use_compression = true;

    // Create the fullnode
    let vfn_peer_id = create_full_node(vfn_config, &mut swarm).await;

    // Test the ability of the fullnode to sync
    test_full_node_sync(vfn_peer_id, &mut swarm, true, false).await;
}

#[tokio::test]
async fn test_full_node_bootstrap_outputs_no_compression() {
    // Create a validator swarm of 1 validator node
    let mut swarm = new_local_swarm_with_aptos(1).await;

    // Create a fullnode config that uses transaction outputs to sync (without compression)
    let mut vfn_config = NodeConfig::get_default_vfn_config();
    vfn_config.state_sync.state_sync_driver.bootstrapping_mode =
        BootstrappingMode::ApplyTransactionOutputsFromGenesis;
    vfn_config
        .state_sync
        .state_sync_driver
        .continuous_syncing_mode = ContinuousSyncingMode::ApplyTransactionOutputs;
    vfn_config.state_sync.aptos_data_client.use_compression = false;

    // Create the fullnode
    let vfn_peer_id = create_full_node(vfn_config, &mut swarm).await;

    // Test the ability of the fullnode to sync
    test_full_node_sync(vfn_peer_id, &mut swarm, true, false).await;
}

#[tokio::test]
async fn test_full_node_bootstrap_outputs_exponential_backoff() {
    // Create a validator swarm of 1 validator node
    let mut swarm = new_local_swarm_with_aptos(1).await;

    // Create a fullnode config that uses transaction outputs to sync with a small timeout
    let mut vfn_config = NodeConfig::get_default_vfn_config();
    vfn_config.state_sync.state_sync_driver.bootstrapping_mode =
        BootstrappingMode::ApplyTransactionOutputsFromGenesis;
    vfn_config
        .state_sync
        .state_sync_driver
        .continuous_syncing_mode = ContinuousSyncingMode::ApplyTransactionOutputs;
    vfn_config.state_sync.aptos_data_client.response_timeout_ms = 1;

    // Create the fullnode
    let vfn_peer_id = create_full_node(vfn_config, &mut swarm).await;

    // Test the ability of the fullnode to sync
    test_full_node_sync(vfn_peer_id, &mut swarm, true, false).await;
}

#[tokio::test]
async fn test_full_node_bootstrap_transactions_or_outputs() {
    // Create a validator swarm of 1 validator node with a small network limit
    let mut swarm = SwarmBuilder::new_local(1)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.storage_service.max_network_chunk_bytes = 5 * 1024;
        }))
        .build()
        .await;

    // Create a fullnode config that uses transactions or outputs to sync
    let mut vfn_config = NodeConfig::get_default_vfn_config();
    vfn_config.state_sync.state_sync_driver.bootstrapping_mode =
        BootstrappingMode::ExecuteOrApplyFromGenesis;
    vfn_config
        .state_sync
        .state_sync_driver
        .continuous_syncing_mode = ContinuousSyncingMode::ExecuteTransactionsOrApplyOutputs;
    vfn_config
        .state_sync
        .aptos_data_client
        .max_num_output_reductions = 1;
    vfn_config.state_sync.aptos_data_client.response_timeout_ms = 1;

    // Create the fullnode
    let vfn_peer_id = create_full_node(vfn_config, &mut swarm).await;

    // Test the ability of the fullnode to sync
    test_full_node_sync(vfn_peer_id, &mut swarm, true, false).await;
}

#[tokio::test]
async fn test_full_node_bootstrap_snapshot_transactions_or_outputs() {
    // Create a validator swarm of 1 validator node with a small network limit
    let mut swarm = SwarmBuilder::new_local(1)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.storage_service.max_network_chunk_bytes = 500 * 1024;
        }))
        .build()
        .await;

    // Create a fullnode config that uses snapshot syncing and transactions or outputs
    let mut vfn_config = NodeConfig::get_default_vfn_config();
    vfn_config.state_sync.state_sync_driver.bootstrapping_mode =
        BootstrappingMode::DownloadLatestStates;
    vfn_config
        .state_sync
        .state_sync_driver
        .continuous_syncing_mode = ContinuousSyncingMode::ExecuteTransactionsOrApplyOutputs;
    vfn_config
        .state_sync
        .aptos_data_client
        .max_num_output_reductions = 2;
    vfn_config.state_sync.aptos_data_client.response_timeout_ms = 1;

    // Create the fullnode
    let vfn_peer_id = create_full_node(vfn_config, &mut swarm).await;

    // Test the ability of the fullnode to sync
    test_full_node_sync(vfn_peer_id, &mut swarm, true, false).await;
}

#[tokio::test]
async fn test_full_node_bootstrap_transactions() {
    // Create a validator swarm of 1 validator node
    let mut swarm = new_local_swarm_with_aptos(1).await;

    // Create a fullnode config that uses transactions to sync
    let mut vfn_config = NodeConfig::get_default_vfn_config();
    vfn_config.state_sync.state_sync_driver.bootstrapping_mode =
        BootstrappingMode::ExecuteTransactionsFromGenesis;
    vfn_config
        .state_sync
        .state_sync_driver
        .continuous_syncing_mode = ContinuousSyncingMode::ExecuteTransactions;
    vfn_config.state_sync.aptos_data_client.use_compression = true;

    // Create the fullnode
    let vfn_peer_id = create_full_node(vfn_config, &mut swarm).await;

    // Test the ability of the fullnode to sync
    test_full_node_sync(vfn_peer_id, &mut swarm, true, false).await;
}

#[tokio::test]
async fn test_full_node_continuous_sync_outputs() {
    // Create a validator swarm of 1 validator node
    let mut swarm = new_local_swarm_with_aptos(1).await;

    // Create a fullnode config that uses transaction outputs to sync
    let mut vfn_config = NodeConfig::get_default_vfn_config();
    vfn_config
        .state_sync
        .state_sync_driver
        .continuous_syncing_mode = ContinuousSyncingMode::ApplyTransactionOutputs;

    // Create the fullnode
    let vfn_peer_id = create_full_node(vfn_config, &mut swarm).await;

    // Test the ability of the fullnode to sync
    test_full_node_sync(vfn_peer_id, &mut swarm, false, false).await;
}

#[tokio::test]
async fn test_full_node_continuous_sync_transactions() {
    // Create a validator swarm of 1 validator node
    let mut swarm = new_local_swarm_with_aptos(1).await;

    // Create a fullnode config that uses transactions to sync
    let mut vfn_config = NodeConfig::get_default_vfn_config();
    vfn_config
        .state_sync
        .state_sync_driver
        .continuous_syncing_mode = ContinuousSyncingMode::ExecuteTransactions;

    // Create the fullnode
    let vfn_peer_id = create_full_node(vfn_config, &mut swarm).await;

    // Test the ability of the fullnode to sync
    test_full_node_sync(vfn_peer_id, &mut swarm, false, false).await;
}

/// Creates a new full node using the given config and swarm
async fn create_full_node(full_node_config: NodeConfig, swarm: &mut LocalSwarm) -> PeerId {
    let validator_peer_id = swarm.validators().next().unwrap().peer_id();
    let vfn_peer_id = swarm
        .add_validator_fullnode(
            &swarm.versions().max().unwrap(),
            full_node_config,
            validator_peer_id,
        )
        .unwrap();
    for fullnode in swarm.full_nodes_mut() {
        fullnode
            .wait_until_healthy(Instant::now() + Duration::from_secs(MAX_HEALTHY_WAIT_SECS))
            .await
            .unwrap();
    }
    vfn_peer_id
}

/// A helper method that tests that a full node can sync from a validator after
/// a failure and continue to stay up-to-date.
async fn test_full_node_sync(
    vfn_peer_id: PeerId,
    swarm: &mut LocalSwarm,
    epoch_changes: bool,
    clear_storage: bool,
) {
    // Stop the fullnode and potentially clear storage
    if clear_storage {
        stop_fullnode_and_delete_storage(swarm, vfn_peer_id).await;
    } else {
        swarm.fullnode_mut(vfn_peer_id).unwrap().stop();
    }

    // Execute a number of transactions on the validator
    let validator_peer_id = swarm.validators().next().unwrap().peer_id();
    let validator_client = swarm.validator(validator_peer_id).unwrap().rest_client();
    let (mut account_0, mut account_1) = create_test_accounts(swarm).await;
    execute_transactions(
        swarm,
        &validator_client,
        &mut account_0,
        &account_1,
        epoch_changes,
    )
    .await;

    // Restart the fullnode and verify it can sync
    swarm
        .fullnode_mut(vfn_peer_id)
        .unwrap()
        .restart()
        .await
        .unwrap();
    wait_for_all_nodes(swarm).await;

    // Execute more transactions on the validator and verify the fullnode catches up
    execute_transactions_and_wait(
        swarm,
        &validator_client,
        &mut account_1,
        &account_0,
        epoch_changes,
    )
    .await;
}

#[tokio::test]
async fn test_validator_bootstrap_outputs() {
    // Create a swarm of 4 validators using output syncing
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::ApplyTransactionOutputsFromGenesis;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ApplyTransactionOutputs;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[tokio::test]
async fn test_validator_bootstrap_state_snapshot() {
    // Test snapshot (fast) syncing in the presence of epoch changes
    perform_validator_bootstrap_state_snapshot(true).await;
}

#[tokio::test]
async fn test_validator_bootstrap_state_snapshot_genesis() {
    // Test snapshot (fast) syncing without epoch changes
    perform_validator_bootstrap_state_snapshot(false).await;
}

#[ignore] // We ignore this test because it takes a long time. But, it works, so it shouldn't be removed.
#[tokio::test]
async fn test_validator_bootstrap_outputs_network_limit() {
    // Create a swarm of 4 validators using output syncing and an aggressive network limit
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::ApplyTransactionOutputsFromGenesis;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ApplyTransactionOutputs;
            config.state_sync.storage_service.max_network_chunk_bytes = 100 * 1024;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[ignore] // We ignore this test because it takes a long time. But, it works, so it shouldn't be removed.
#[tokio::test]
async fn test_validator_bootstrap_outputs_network_limit_tiny() {
    // Create a swarm of 4 validators using output syncing and an unrealistic network limit.
    // This forces all chunks to be of size 1.
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::ApplyTransactionOutputsFromGenesis;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ApplyTransactionOutputs;
            config.state_sync.storage_service.max_network_chunk_bytes = 1;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[tokio::test]
async fn test_validator_bootstrap_state_snapshot_no_compression() {
    // Create a swarm of 4 validators using state snapshot syncing
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::DownloadLatestStates;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ApplyTransactionOutputs;
            config.state_sync.aptos_data_client.use_compression = false;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[ignore] // We ignore this test because it takes a long time. But, it works, so it shouldn't be removed.
#[tokio::test]
async fn test_validator_bootstrap_state_snapshot_network_limit() {
    // Create a swarm of 4 validators using state snapshot syncing and an aggressive network limit
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::DownloadLatestStates;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ExecuteTransactions;
            config.state_sync.storage_service.max_network_chunk_bytes = 200 * 1024;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[ignore] // We ignore this test because it takes a long time. But, it works, so it shouldn't be removed.
#[tokio::test]
async fn test_validator_bootstrap_state_snapshot_network_limit_tiny() {
    // Create a swarm of 4 validators using state snapshot syncing and an unrealistic network limit.
    // This forces all chunks to be of size 1.
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::DownloadLatestStates;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ExecuteTransactions;
            config.state_sync.storage_service.max_network_chunk_bytes = 1;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[tokio::test]
async fn test_validator_bootstrap_state_snapshot_exponential_backoff() {
    // Test snapshot (fast) syncing in the presence of
    // exponential backoff and epoch changes.
    perform_validator_bootstrap_state_snapshot_exponential_backoff(true).await;
}

#[tokio::test]
async fn test_validator_bootstrap_state_snapshot_exponential_backoff_genesis() {
    // Test snapshot (fast) syncing in the presence of
    // exponential backoff without epoch changes.
    perform_validator_bootstrap_state_snapshot_exponential_backoff(false).await;
}

#[tokio::test]
async fn test_validator_bootstrap_transactions() {
    // Create a swarm of 4 validators using transaction syncing
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::ExecuteTransactionsFromGenesis;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ExecuteTransactions;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[tokio::test]
async fn test_validator_bootstrap_transactions_or_outputs() {
    // Create a swarm of 4 validators using transaction or output syncing
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::ExecuteOrApplyFromGenesis;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ExecuteTransactionsOrApplyOutputs;
            config.state_sync.storage_service.max_network_chunk_bytes = 10 * 1024;
            config
                .state_sync
                .aptos_data_client
                .max_num_output_reductions = 1;
            config.state_sync.aptos_data_client.response_timeout_ms = 1;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[ignore] // We ignore this test because it takes a long time. But, it works, so it shouldn't be removed.
#[tokio::test]
async fn test_validator_bootstrap_transactions_network_limit() {
    // Create a swarm of 4 validators using transaction syncing and an aggressive network limit
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::ExecuteTransactionsFromGenesis;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ExecuteTransactions;
            config.state_sync.storage_service.max_network_chunk_bytes = 100 * 1024;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[ignore] // We ignore this test because it takes a long time. But, it works, so it shouldn't be removed.
#[tokio::test]
async fn test_validator_bootstrap_transactions_network_limit_tiny() {
    // Create a swarm of 4 validators using transaction syncing and an unrealistic network limit.
    // This forces all chunks to be of size 1.
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::ExecuteTransactionsFromGenesis;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ExecuteTransactions;
            config.state_sync.storage_service.max_network_chunk_bytes = 1;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[tokio::test]
async fn test_validator_bootstrap_outputs_network_exponential_backoff() {
    // Create a swarm of 4 validators using output syncing and a small response timeout
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::ApplyTransactionOutputsFromGenesis;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ApplyTransactionOutputs;
            config.state_sync.aptos_data_client.response_timeout_ms = 1;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

#[tokio::test]
async fn test_validator_bootstrap_transactions_no_compression() {
    // Create a swarm of 4 validators using transaction syncing and no compression
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::ExecuteTransactionsFromGenesis;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ExecuteTransactions;
            config.state_sync.aptos_data_client.use_compression = false;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, true).await;
}

/// A test method that verifies that a fullnode can fast sync from
/// a validator after a data wipe. If `epoch_changes` are enabled
/// then epoch changes can occur during test execution and fullnode syncing.
async fn perform_full_node_bootstrap_state_snapshot(epoch_changes: bool) {
    // Create a swarm with 2 validators
    let mut swarm = SwarmBuilder::new_local(2)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::DownloadLatestStates;
        }))
        .with_init_genesis_config(Arc::new(|genesis_config| {
            genesis_config.epoch_duration_secs = 10_000; // Prevent epoch changes from occurring unnecessarily
        }))
        .build()
        .await;

    // Verify the oldest ledger info and pruning metrics for the validators
    for validator in swarm.validators() {
        // Verify the oldest ledger info
        let validator_rest_client = validator.rest_client();
        verify_oldest_version_after_fast_sync(validator_rest_client, true).await;

        // Verify the pruning metrics
        let inspection_client = validator.inspection_client();
        verify_pruning_metrics_after_fast_sync(inspection_client, true).await;
    }

    // Create a fullnode config that uses snapshot syncing
    let mut vfn_config = NodeConfig::get_default_vfn_config();
    vfn_config.state_sync.state_sync_driver.bootstrapping_mode =
        BootstrappingMode::DownloadLatestStates;

    // Test the ability of a fullnode to sync
    let vfn_peer_id = create_full_node(vfn_config, &mut swarm).await;
    test_full_node_sync(vfn_peer_id, &mut swarm, epoch_changes, true).await;

    // Verify the oldest ledger info and pruning metrics for the fullnode
    if epoch_changes {
        // Verify the oldest ledger info
        let vfn_rest_client = swarm.fullnode_mut(vfn_peer_id).unwrap().rest_client();
        verify_oldest_version_after_fast_sync(vfn_rest_client, false).await;

        // Verify the fullnode pruning metrics
        let inspection_client = swarm.fullnode(vfn_peer_id).unwrap().inspection_client();
        verify_pruning_metrics_after_fast_sync(inspection_client, false).await;
    }
}

/// A test method that verifies that a validator can fast sync from
/// other validators after a data wipe. If `epoch_changes` are enabled
/// then epoch changes can occur during test execution and validator syncing.
async fn perform_validator_bootstrap_state_snapshot(epoch_changes: bool) {
    // Create a swarm of 4 validators using snapshot (fast) syncing and a chunk size = 30
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::DownloadLatestStates;
            config.state_sync.storage_service.max_state_chunk_size = 30;
        }))
        .with_init_genesis_config(Arc::new(|genesis_config| {
            genesis_config.epoch_duration_secs = 10_000; // Prevent epoch changes from occurring unnecessarily
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    let validator_index_to_test = 1;
    test_validator_sync(&mut swarm, validator_index_to_test, epoch_changes).await;

    // Verify the oldest ledger info and pruning metrics for the validator
    if epoch_changes {
        // Verify the oldest ledger info for the validator
        let bootstrapped_validator =
            swarm.validators().collect::<Vec<_>>()[validator_index_to_test];
        let validator_client = bootstrapped_validator.rest_client();
        verify_oldest_version_after_fast_sync(validator_client, false).await;

        // Verify the validator pruning metrics
        let inspection_client = bootstrapped_validator.inspection_client();
        verify_pruning_metrics_after_fast_sync(inspection_client, false).await;
    }
}

/// A test method that verifies that a validator can fast sync from other
/// validators after a data wipe and while requiring exponential backoff.
/// If `epoch_changes` are enabled then epoch changes can occur during
/// test execution and validator syncing.
async fn perform_validator_bootstrap_state_snapshot_exponential_backoff(epoch_changes: bool) {
    // Create a swarm of 4 validators using state snapshot syncing and a small response timeout
    let mut swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::DownloadLatestStates;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ApplyTransactionOutputs;
            config.state_sync.aptos_data_client.use_compression = false;
            config.state_sync.aptos_data_client.response_timeout_ms = 1;
        }))
        .with_init_genesis_config(Arc::new(|genesis_config| {
            genesis_config.epoch_duration_secs = 10_000; // Prevent epoch changes from occurring unnecessarily
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_validator_sync(&mut swarm, 1, epoch_changes).await;
}

/// A helper method that tests that a validator can sync after a failure and
/// continue to stay up-to-date.
async fn test_validator_sync(
    swarm: &mut LocalSwarm,
    validator_index_to_test: usize,
    epoch_changes: bool,
) {
    // Execute multiple transactions through validator 0
    let validator_peer_ids = swarm.validators().map(|v| v.peer_id()).collect::<Vec<_>>();
    let validator_client_0 = swarm
        .validator(validator_peer_ids[0])
        .unwrap()
        .rest_client();
    let (mut account_0, mut account_1) = create_test_accounts(swarm).await;
    execute_transactions_and_wait(
        swarm,
        &validator_client_0,
        &mut account_0,
        &account_1,
        epoch_changes,
    )
    .await;

    // Stop the validator and delete the storage
    let validator = validator_peer_ids[validator_index_to_test];
    stop_validator_and_delete_storage(swarm, validator).await;

    // Execute more transactions
    execute_transactions(
        swarm,
        &validator_client_0,
        &mut account_1,
        &account_0,
        epoch_changes,
    )
    .await;

    // Restart the validator and wait for all nodes to catchup
    swarm.validator_mut(validator).unwrap().start().unwrap();
    wait_for_all_nodes(swarm).await;

    // Execute multiple transactions and verify the validator can sync
    execute_transactions_and_wait(
        swarm,
        &validator_client_0,
        &mut account_0,
        &account_1,
        epoch_changes,
    )
    .await;
}

// Ignore this test because it's become increasingly flaky recently
// and needs to be debugged.
#[ignore]
#[tokio::test]
async fn test_validator_failure_bootstrap_outputs() {
    // Create a swarm of 4 validators with state snapshot bootstrapping and output syncing
    let swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::DownloadLatestStates;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ApplyTransactionOutputs;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_all_validator_failures(swarm).await;
}

// Ignore this test because it's become increasingly flaky recently
// and needs to be debugged.
#[ignore]
#[tokio::test]
async fn test_validator_failure_bootstrap_execution() {
    // Create a swarm of 4 validators with state snapshot bootstrapping and transaction syncing
    let swarm = SwarmBuilder::new_local(4)
        .with_aptos()
        .with_init_config(Arc::new(|_, config, _| {
            config.state_sync.state_sync_driver.bootstrapping_mode =
                BootstrappingMode::DownloadLatestStates;
            config.state_sync.state_sync_driver.continuous_syncing_mode =
                ContinuousSyncingMode::ExecuteTransactions;
        }))
        .build()
        .await;

    // Test the ability of the validators to sync
    test_all_validator_failures(swarm).await;
}

/// A helper method that tests that all validators can sync after a failure and
/// continue to stay up-to-date.
pub async fn test_all_validator_failures(mut swarm: LocalSwarm) {
    // Execute multiple transactions through validator 0
    let validator_peer_ids = swarm.validators().map(|v| v.peer_id()).collect::<Vec<_>>();
    let validator_0 = validator_peer_ids[0];
    let validator_client_0 = swarm.validator(validator_0).unwrap().rest_client();
    let (mut account_0, mut account_1) = create_test_accounts(&mut swarm).await;
    execute_transactions_and_wait(
        &mut swarm,
        &validator_client_0,
        &mut account_0,
        &account_1,
        true,
    )
    .await;

    // Go through each validator, stop the node, delete the storage and wait for it to come back
    for validator in validator_peer_ids.clone() {
        stop_validator_and_delete_storage(&mut swarm, validator).await;
        swarm.validator_mut(validator).unwrap().start().unwrap();
        wait_for_all_nodes(&mut swarm).await;
    }

    // Execute multiple transactions (no epoch changes) and verify validator 0 can sync
    execute_transactions_and_wait(
        &mut swarm,
        &validator_client_0,
        &mut account_1,
        &account_0,
        false,
    )
    .await;

    // Go through each validator, stop the node, delete the storage and wait for it to come back
    for validator in validator_peer_ids.clone() {
        stop_validator_and_delete_storage(&mut swarm, validator).await;
        swarm.validator_mut(validator).unwrap().start().unwrap();
        wait_for_all_nodes(&mut swarm).await;
    }

    // Execute multiple transactions (with epoch changes) and verify validator 0 can sync
    let validator_client_1 = swarm
        .validator(validator_peer_ids[0])
        .unwrap()
        .rest_client();
    execute_transactions_and_wait(
        &mut swarm,
        &validator_client_1,
        &mut account_1,
        &account_0,
        true,
    )
    .await;
}

#[tokio::test]
#[ignore]
async fn test_single_validator_failure() {
    // Create a swarm of 1 validator
    let mut swarm = new_local_swarm_with_aptos(1).await;

    // Execute multiple transactions
    let validator = swarm.validators_mut().next().unwrap();
    let validator_client = validator.rest_client();
    let (mut account_0, mut account_1) = create_test_accounts(&mut swarm).await;
    execute_transactions(
        &mut swarm,
        &validator_client,
        &mut account_0,
        &account_1,
        true,
    )
    .await;

    // Restart the validator
    let validator = swarm.validators_mut().next().unwrap();
    validator.stop();
    validator.start().unwrap();
    swarm.wait_all_alive(Duration::from_secs(20)).await.unwrap();

    // Execute more transactions
    execute_transactions(
        &mut swarm,
        &validator_client,
        &mut account_1,
        &account_0,
        true,
    )
    .await;
}

/// Executes transactions using the given transaction factory, client and
/// accounts. If `execute_epoch_changes` is true, also execute transactions to
/// force reconfigurations.
async fn execute_transactions(
    swarm: &mut LocalSwarm,
    client: &RestClient,
    sender: &mut LocalAccount,
    receiver: &LocalAccount,
    execute_epoch_changes: bool,
) {
    // Execute several transactions
    let num_transfers = 10;
    let transaction_factory = swarm.chain_info().transaction_factory();
    if execute_epoch_changes {
        transfer_and_maybe_reconfig(
            client,
            &transaction_factory,
            swarm.chain_info().root_account,
            sender,
            receiver,
            num_transfers,
        )
        .await;
    } else {
        for _ in 0..num_transfers {
            // Execute simple transfer transactions
            transfer_coins(client, &transaction_factory, sender, receiver, 1).await;
        }
    }

    // Always ensure that at least one reconfiguration transaction is executed
    if !execute_epoch_changes {
        aptos_forge::reconfig(
            client,
            &transaction_factory,
            swarm.chain_info().root_account,
        )
        .await;
    }
}

/// Executes transactions and waits for all nodes to catch up
async fn execute_transactions_and_wait(
    swarm: &mut LocalSwarm,
    client: &RestClient,
    sender: &mut LocalAccount,
    receiver: &LocalAccount,
    epoch_changes: bool,
) {
    execute_transactions(swarm, client, sender, receiver, epoch_changes).await;
    wait_for_all_nodes(swarm).await;
}

/// Waits for all nodes to catch up
async fn wait_for_all_nodes(swarm: &mut LocalSwarm) {
    swarm
        .wait_for_all_nodes_to_catchup(Duration::from_secs(MAX_CATCH_UP_WAIT_SECS))
        .await
        .unwrap();
}

/// Creates and funds two test accounts
async fn create_test_accounts(swarm: &mut LocalSwarm) -> (LocalAccount, LocalAccount) {
    let token_amount = 1000;
    let account_0 = create_and_fund_account(swarm, token_amount).await;
    let account_1 = create_and_fund_account(swarm, token_amount).await;
    (account_0, account_1)
}

/// Stops the specified fullnode and deletes storage
async fn stop_fullnode_and_delete_storage(swarm: &mut LocalSwarm, fullnode: AccountAddress) {
    let fullnode = swarm.full_node_mut(fullnode).unwrap();

    // The fullnode is stopped during the clear_storage() call
    fullnode.clear_storage().await.unwrap();
}

/// Stops the specified validator and deletes storage
async fn stop_validator_and_delete_storage(swarm: &mut LocalSwarm, validator: AccountAddress) {
    let validator = swarm.validator_mut(validator).unwrap();

    // The validator is stopped during the clear_storage() call
    validator.clear_storage().await.unwrap();
}

/// Verifies the oldest ledger version on a node after fast syncing
async fn verify_oldest_version_after_fast_sync(
    node_rest_client: RestClient,
    sync_to_genesis: bool,
) {
    // Fetch the oldest ledger version from the node
    let ledger_information = node_rest_client.get_ledger_information().await.unwrap();
    let oldest_ledger_version = ledger_information.inner().oldest_ledger_version;

    // Verify the oldest ledger version after fast syncing
    if sync_to_genesis {
        // The node should have fast synced to genesis
        assert_eq!(oldest_ledger_version, 0);
    } else {
        // The node should have fast synced to the latest epoch
        assert!(oldest_ledger_version > 0);
    }
}

/// Verifies the pruning metrics on a node after fast syncing
async fn verify_pruning_metrics_after_fast_sync(
    node_inspection_client: InspectionClient,
    sync_to_genesis: bool,
) {
    // Fetch the pruning metrics from the node
    let state_merkle_pruner_version = node_inspection_client
        .get_node_metric_i64(
            "aptos_pruner_versions{pruner_name=state_merkle_pruner,tag=min_readable}",
        )
        .await
        .unwrap()
        .unwrap();
    let epoch_snapshot_pruner_version = node_inspection_client
        .get_node_metric_i64(
            "aptos_pruner_versions{pruner_name=epoch_snapshot_pruner,tag=min_readable}",
        )
        .await
        .unwrap()
        .unwrap();
    let ledger_pruner_version = node_inspection_client
        .get_node_metric_i64("aptos_pruner_versions{pruner_name=ledger_pruner,tag=min_readable}")
        .await
        .unwrap()
        .unwrap();

    // Verify that the pruning metrics are valid
    if sync_to_genesis {
        // The node should have fast synced to genesis
        assert_eq!(state_merkle_pruner_version, 0);
        assert_eq!(epoch_snapshot_pruner_version, 0);
        assert_eq!(ledger_pruner_version, 0);
    } else {
        // The node should have fast synced to the latest epoch
        assert!(state_merkle_pruner_version > 0);
        assert!(epoch_snapshot_pruner_version > 0);
        assert!(ledger_pruner_version > 0);
    }
}
