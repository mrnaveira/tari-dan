use std::{thread, time::Duration};

use axum::http::HeaderMap;
use reqwest::{header, Response, Url};
use serde_json::json;
use tari_common::configuration::CommonConfig;
use tari_p2p::{Network, PeerSeedsConfig};
use tari_validator_node::{run_node, ApplicationConfig, ValidatorNodeConfig};
use tempfile::tempdir;
use tokio::runtime;

pub fn spawn_validator_node() {
    thread::spawn(move || {
        // TODO: store the VN in the world by the name
        let mut config = ApplicationConfig {
            common: CommonConfig::default(),
            validator_node: ValidatorNodeConfig::default(),
            peer_seeds: PeerSeedsConfig::default(),
            network: Network::LocalNet,
        };

        // temporal folder for the VN files (e.g. sqlite file, json files, etc.)
        let temp_dir = tempdir().unwrap();
        println!("Using validator_node temp_dir: {}", temp_dir.path().display());
        config.validator_node.data_dir = temp_dir.path().to_path_buf();
        config.validator_node.shard_key_file = temp_dir.path().join("shard_key.json");
        config.validator_node.identity_file = temp_dir.path().join("validator_node_id.json");
        config.validator_node.tor_identity_file = temp_dir.path().join("validator_node_tor_id.json");

        // TODO: use a spawned base node instead of a real one
        config.validator_node.base_node_grpc_address = "127.0.0.1:18152".parse().unwrap();

        // TODO: use a spawned wallet instead of a real one
        config.validator_node.wallet_grpc_address = "127.0.0.1:18153".parse().unwrap();

        let mut builder = runtime::Builder::new_multi_thread();
        let rt = builder.enable_all().build().unwrap();
        rt.block_on(run_node(&config))
    });

    // We need to give it time for the VN to startup
    // TODO: it would be better to scan the VN to detect when it has started
    thread::sleep(Duration::from_secs(2));
}

pub async fn send_vn_json_rpc_request<T: Into<serde_json::Value>>(port: u64, method: String, body: T) -> Response {
    let endpoint: Url = Url::parse(&format!("http://localhost:{}", port)).unwrap();
    let client = reqwest::Client::builder()
        .default_headers({
            let mut headers = HeaderMap::with_capacity(1);
            headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
            headers
        })
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let request_json = json!(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": body.into(),
        }
    );
    client
        .post(endpoint.clone())
        .body(request_json.to_string())
        .send()
        .await
        .unwrap()
}
