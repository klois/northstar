// Copyright (c) 2019 - 2020 ESRLabs
//
//   Licensed under the Apache License, Version 2.0 (the "License");
//   you may not use this file except in compliance with the License.
//   You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//   Unless required by applicable law or agreed to in writing, software
//   distributed under the License is distributed on an "AS IS" BASIS,
//   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//   See the License for the specific language governing permissions and
//   limitations under the License.

//! Controls Northstar runtime instances

use super::{
    logger,
    test_container::{test_container_npk, test_resource_npk},
};
use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use northstar::{
    api::{client::Client, model::Notification},
    runtime::{
        self,
        config::{self, Config},
        Container,
    },
};
use std::{collections::HashMap, convert::TryInto, path::PathBuf, time::Duration};
use tempfile::TempDir;
use tokio::{fs, pin, select, time};

pub struct Northstar {
    config: Config,
    client: northstar::api::client::Client,
    runtime: runtime::Runtime,
    tmpdir: TempDir,
    data_dir: PathBuf,
}

impl std::ops::Deref for Northstar {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl Northstar {
    /// Launches an instance of Northstar with the test container and
    /// resource installed.
    pub async fn launch_install_test_container() -> Result<Northstar> {
        let runtime = Self::launch().await?;
        runtime.install_test_resource().await?;
        runtime.install_test_container().await?;
        Ok(runtime)
    }

    /// Launches an instance of Northstar
    pub async fn launch() -> Result<Northstar> {
        let pid = std::process::id();
        let tmpdir = tempfile::TempDir::new()?;

        let run_dir = tmpdir.path().join("run");
        let data_dir = tmpdir.path().join("data");
        let log_dir = tmpdir.path().join("log");
        let test_repositority = tmpdir.path().join("test");
        let example_key = tmpdir.path().join("key.pub");
        fs::create_dir(&test_repositority).await?;
        fs::write(
            &example_key,
            include_bytes!("../../examples/keys/northstar.pub"),
        )
        .await?;

        let mut repositories = HashMap::new();
        repositories.insert(
            "test".into(),
            config::Repository {
                dir: test_repositority,
                key: Some(example_key.clone()),
            },
        );

        let mut cgroups = HashMap::new();
        cgroups.insert("memory".into(), PathBuf::from(format!("northstar-{}", pid)));
        cgroups.insert("cpu".into(), PathBuf::from(format!("northstar-{}", pid)));

        let console = format!(
            "unix://{}/northstar-{}",
            tmpdir.path().display(),
            std::process::id()
        );
        let console_url = url::Url::parse(&console)?;

        let config = Config {
            console: Some(console_url.clone()),
            run_dir,
            data_dir: data_dir.clone(),
            log_dir,
            repositories,
            cgroups,
            #[cfg(target_os = "android")]
            devices: config::Devices {
                device_mapper: PathBuf::from("/dev/device-mapper"),
                device_mapper_dev: "/dev/block/dm-".into(),
                loop_control: PathBuf::from("/dev/loop-control"),
                loop_dev: "/dev/block/loop".into(),
            },
            #[cfg(not(target_os = "android"))]
            devices: config::Devices {
                device_mapper: PathBuf::from("/dev/mapper/control"),
                device_mapper_dev: "/dev/dm-".into(),
                loop_control: PathBuf::from("/dev/loop-control"),
                loop_dev: "/dev/loop".into(),
            },
            debug: None,
        };

        // Start the runtime
        let runtime = runtime::Runtime::start(config.clone())
            .await
            .context("Failed to start runtime")?;
        // Wait until the console is up and running
        super::logger::assume("Started console on", 5u64).await?;

        // Connect to the runtime
        let client = Client::new(&console_url, Some(1000), time::Duration::from_secs(30)).await?;
        // Wait until a successfull connection
        logger::assume("Client .* connected", 5u64).await?;

        Ok(Northstar {
            config,
            client,
            runtime,
            tmpdir,
            data_dir,
        })
    }

    pub async fn shutdown(self) -> Result<()> {
        // TODO: Stop and disconnect the client

        // Stop the runtime
        self.runtime
            .shutdown()
            .await
            .context("Failed to stop the runtime")?;

        logger::assume("Closed listener", 5u64).await?;

        // Remove the tmpdir
        self.tmpdir.close().context("Failed to remove tmpdir")
    }

    /// Return the runtimes configuration
    pub fn config(&self) -> &config::Config {
        &self.config
    }

    /// Start a container
    pub async fn start(&self, container: &str) -> Result<()> {
        let container: Container = container.try_into().expect("Invalid container str");
        self.client
            .start(container.name(), container.version())
            .await
            .context("Failed to start")
    }

    /// Stop a container
    pub async fn stop(&self, container: &str, timeout: u64) -> Result<()> {
        let container: Container = container.try_into().expect("Invalid container str");
        self.client
            .stop(
                container.name(),
                container.version(),
                Duration::from_secs(timeout),
            )
            .await
            .context("Failed to stop")?;
        Ok(())
    }

    /// Umount
    pub async fn umount(&self, container: &str) -> Result<()> {
        let container: Container = container.try_into().expect("Invalid container str");
        self.client
            .umount(container.name(), container.version())
            .await
            .context("Failed to umount")?;
        Ok(())
    }

    pub async fn install_test_container(&self) -> Result<()> {
        self.client
            .install(test_container_npk().await, "test")
            .await
            .context("Failed to install test container")
    }

    pub async fn uninstall_test_container(&self) -> Result<()> {
        self.client
            .uninstall(
                "test_container",
                &npk::manifest::Version::parse("0.0.1").unwrap(),
            )
            .await
            .context("Failed to uninstall test container")
    }

    pub async fn install_test_resource(&self) -> Result<()> {
        self.client
            .install(test_resource_npk().await, "test")
            .await
            .context("Failed to install test resource")
    }

    pub async fn uninstall_test_resource(&self) -> Result<()> {
        self.client
            .uninstall(
                "test_resource",
                &npk::manifest::Version::parse("0.0.1").unwrap(),
            )
            .await
            .context("Failed to uninstall test resource")
    }

    // TOOD: Queue the notifications in the runtime struct. Currently there's a race
    // if the notification is faster.
    pub async fn assume_notification<F>(&mut self, mut pred: F, timeout: u64) -> Result<()>
    where
        F: FnMut(&Notification) -> bool,
    {
        let timeout = time::sleep(time::Duration::from_secs(timeout));
        pin!(timeout);

        loop {
            select! {
                _ = &mut timeout => break Err(anyhow!("Timeout waiting for notification")),
                notification = self.client.next() => {
                    match notification {
                        Some(Ok(n)) if pred(&n) => break Ok(()),
                        Some(_) => continue,
                        None => break Err(anyhow!("Client connection closed")),
                    }
                }
            }
        }
    }

    pub async fn test_cmds(&self, cmd: &str) {
        let data = self.data_dir.join("test_container");
        fs::create_dir_all(&data)
            .await
            .expect("Failed to create data dir");
        fs::write(&data.join("input.txt"), &cmd)
            .await
            .expect("Failed to write test_container input");
    }
}
