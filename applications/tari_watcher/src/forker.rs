// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{env, net::IpAddr, path::Path, process::Stdio};

use tokio::process::{Child, Command};

use crate::config::{ExecutableConfig, InstanceType};

pub struct Forker {
    // The Tari L2 validator instance
    validator: Option<Instance>,
    // Child process of the forked validator instance.
    // Includes PID and a handle to the process.
    child: Option<Child>,
}

impl Forker {
    pub fn new() -> Self {
        Self {
            validator: None,
            child: None,
        }
    }

    pub async fn start_validator(&mut self, config: ExecutableConfig) -> anyhow::Result<()> {
        let instance = Instance::new(InstanceType::TariValidatorNode, config.clone());
        self.validator = Some(instance.clone());

        let mut cmd = Command::new(
            config
                .executable_path
                .unwrap_or_else(|| Path::new("tari_validator_node").to_path_buf()),
        );

        // TODO: stdout logs
        // let process_dir = self.base_dir.join("processes").join("TariValidatorNode");
        // let stdout_log_path = process_dir.join("stdout.log");
        // let stderr_log_path = process_dir.join("stderr.log");
        cmd.envs(env::vars())
            //.arg(format!("--config={validator_node_config_path}"))
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let child = cmd.spawn()?;
        self.child = Some(child);

        Ok(())
    }
}

#[allow(dead_code)]
#[derive(Clone)]
struct Instance {
    app: InstanceType,
    config: ExecutableConfig,
    listen_ip: Option<IpAddr>,
}

impl Instance {
    pub fn new(app: InstanceType, config: ExecutableConfig) -> Self {
        Self {
            app,
            config,
            listen_ip: None,
        }
    }
}