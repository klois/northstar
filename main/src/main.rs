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

#![deny(clippy::all)]

use anyhow::{Context, Error};
use log::{debug, info, warn};
use northstar::runtime;
use runtime::config::Config;
use std::{fs::read_to_string, path::PathBuf, process::exit};
use structopt::StructOpt;
use tokio::{select, signal::unix::SignalKind};

mod logger;

#[derive(Debug, StructOpt)]
#[structopt(name = "northstar", about = "Northstar")]
struct Opt {
    /// File that contains the northstar configuration
    #[structopt(short, long, default_value = "northstar.toml")]
    pub config: PathBuf,

    /// Do not enter a mount namespace if this option is set Be aware that in
    /// case of a non normal termination of the runtime the images mounted in
    /// `run_dir` have to be umounted manually before starting the runtime again.
    #[structopt(short, long)]
    pub disable_mount_namespace: bool,
}

fn main() -> Result<(), Error> {
    let opt = Opt::from_args();
    let config = read_to_string(&opt.config)
        .with_context(|| format!("Failed to read configuration file {}", opt.config.display()))?;
    let config: Config = toml::from_str(&config)
        .with_context(|| format!("Failed to read configuration file {}", opt.config.display()))?;

    logger::init();

    // Skip mount namespace setup in case it's disabled for debugging purposes
    if !opt.disable_mount_namespace {
        // Enter a mount namespace. This needs to be done before spawning
        // the tokio threadpool.
        debug!("Entering mount namespace");
        nix::sched::unshare(nix::sched::CloneFlags::CLONE_NEWNS)?;
    } else {
        warn!("Mount namespace is disabled");
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("northstar")
        .build()?;
    runtime.block_on(run(config))
}

async fn run(config: Config) -> Result<(), Error> {
    let mut runtime = runtime::Runtime::start(config)
        .await
        .context("Failed to start runtime")?;
    let mut sigint = tokio::signal::unix::signal(SignalKind::interrupt())
        .context("Failed to install sigint handler")?;
    let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())
        .context("Failed to install sigterm handler")?;

    let status = select! {
        _ = sigint.recv() => {
            info!("Received SIGINT. Stopping Northstar runtime");
            runtime.shutdown().await
        }
        _ = sigterm.recv() => {
            info!("Received SIGTERM. Stopping Northstar runtime");
            runtime.shutdown().await
        }
        status = &mut runtime => status,
    };
    match status {
        Ok(_) => exit(0),
        Err(e) => {
            warn!("Runtime exited with {:?}", e);
            exit(1);
        }
    }
}
