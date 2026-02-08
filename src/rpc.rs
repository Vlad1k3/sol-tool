use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::time::Duration;

pub fn client(url: &str) -> RpcClient {
    RpcClient::new_with_timeout_and_commitment(
        url.to_string(),
        Duration::from_secs(30),
        CommitmentConfig::confirmed(),
    )
}

pub fn client_with_timeout(url: &str, timeout: Duration) -> RpcClient {
    RpcClient::new_with_timeout_and_commitment(
        url.to_string(),
        timeout,
        CommitmentConfig::confirmed(),
    )
}
