//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use multiaddr::Multiaddr;
use reqwest::Url;
use tari_dan_wallet_daemon::{cli::Cli, run_tari_dan_wallet_daemon};
use tari_shutdown::Shutdown;
use tari_wallet_daemon_client::WalletDaemonClient;
use tokio::task;

use crate::{
    utils::{helpers::get_os_assigned_ports, logging::get_base_dir},
    TariWorld,
};

#[derive(Debug)]
pub struct DanWalletDaemonProcess {
    pub name: String,
    pub json_rpc_port: u16,
    pub validator_node_jrpc_port: u16,
    pub temp_path_dir: PathBuf,
    pub shutdown: Shutdown,
}

pub async fn spawn_wallet_daemon(world: &mut TariWorld, wallet_daemon_name: String, validator_node_name: String) {
    let (_, json_rpc_port) = get_os_assigned_ports();
    let base_dir = get_base_dir();

    let validator_node_jrpc_port = world.validator_nodes.get(&validator_node_name).unwrap().json_rpc_port;
    let shutdown = Shutdown::new();
    let shutdown_signal = shutdown.to_signal();

    let listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), json_rpc_port);
    let validator_node_endpoint =
        Multiaddr::from_str(&format!("/ip4/127.0.0.1/tcp/{}", validator_node_jrpc_port)).unwrap();

    let cli = Cli {
        listen_addr: Some(listen_addr),
        base_dir: Some(base_dir.clone()),
        validator_node_endpoint: Some(validator_node_endpoint),
    };

    let handle = task::spawn(async move {
        let result = run_tari_dan_wallet_daemon(cli, shutdown_signal).await;
        if let Err(e) = result {
            panic!("{:?}", e);
        }
    });

    // give it some time for the wallet daemon to start
    tokio::time::sleep(Duration::from_secs(10)).await;

    if handle.is_finished() {
        handle.await.unwrap();
        return;
    }

    let wallet_daemon_process = DanWalletDaemonProcess {
        name: wallet_daemon_name.clone(),
        json_rpc_port,
        validator_node_jrpc_port,
        temp_path_dir: base_dir,
        shutdown,
    };

    world.wallet_daemons.insert(wallet_daemon_name, wallet_daemon_process);
}

pub async fn get_walletd_client(port: u16) -> WalletDaemonClient {
    let endpoint: Url = Url::parse(&format!("http://127.0.0.1:{}", port)).unwrap();
    WalletDaemonClient::connect(endpoint).unwrap()
}

impl DanWalletDaemonProcess {
    pub fn stop(&mut self) {
        self.shutdown.trigger();
    }
}
