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

use anyhow::{Context, Result};
use nix::unistd::{self, Gid};
use std::{
    env, fs,
    io::{self, Write},
    iter, mem,
    path::{Path, PathBuf},
    process, thread, time,
};
use structopt::StructOpt;

#[derive(StructOpt)]
enum TestCommands {
    Cat {
        #[structopt(parse(from_os_str))]
        path: PathBuf,
    },
    Crash,
    Echo {
        message: Vec<String>,
    },
    Inspect,
    LeakMemory,
    Touch {
        path: PathBuf,
    },
    Sleep {
        seconds: u64,
    },
    Write {
        message: String,
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    let input = Path::new("/data").join("input.txt");
    if input.exists() {
        println!("Reading {}", input.display());
        let commands = fs::read_to_string(&input)?;

        println!("Removing {}", input.display());
        fs::remove_file(&input)?;

        for line in commands.lines() {
            println!("Executing \"{}\"", line);
            let command = iter::once("test_container").chain(line.split_whitespace());
            match TestCommands::from_iter(command) {
                TestCommands::Cat { path } => cat(&path)?,
                TestCommands::Crash => crash(),
                TestCommands::Echo { message } => echo(&message),
                TestCommands::Inspect => inspect(),
                TestCommands::LeakMemory => leak_memory(),
                TestCommands::Touch { path } => touch(&path)?,
                TestCommands::Sleep { seconds } => sleep(seconds),
                TestCommands::Write { message, path } => write(&message, path.as_path())?,
            };
        }
    }

    println!("Sleeping...");
    thread::sleep(time::Duration::from_secs(u64::MAX));

    Ok(())
}

fn dump(file: &str) {
    println!("{}:", file);
    fs::read_to_string(file)
        .unwrap_or_else(|_| panic!("dump {}", file))
        .lines()
        .for_each(|l| println!("  {}", l));
}

fn cat(path: &Path) -> Result<()> {
    let mut input =
        fs::File::open(&path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut output = std::io::stdout();
    io::copy(&mut input, &mut output)
        .map(drop)
        .with_context(|| format!("Failed to cat {}", path.display()))?;
    writeln!(&mut output).context("Failed to write to stdout")
}

fn crash() {
    panic!("witness me!");
}

fn echo(message: &[String]) {
    println!("{}", message.join(" "));
}

fn write(input: &str, path: &Path) -> Result<()> {
    fs::write(path, input)
        .with_context(|| format!("Failed to write \"{}\" to {}", input, path.display()))
}

fn touch(path: &Path) -> Result<()> {
    fs::File::create(path)?;
    Ok(())
}

fn leak_memory() {
    for _ in 0..9_999_999 {
        println!("Eating a Megabyte...");
        let chunk: Vec<u8> = (0..1_000_000).map(|n| (n % 8) as u8).collect();
        mem::forget(chunk);
        thread::sleep(time::Duration::from_millis(400));
    }
}

fn inspect() {
    println!("getpid: {}", unistd::getpid());
    println!("getppid: {}", unistd::getppid());
    println!("getuid: {}", unistd::getuid());
    println!("getgid: {}", unistd::getgid());
    println!("getsid: {}", unistd::getsid(None).unwrap());
    println!("getpgid: {}", unistd::getpgid(None).unwrap());
    println!(
        "getgroups: {:?}",
        unistd::getgroups()
            .expect("getgroups")
            .iter()
            .cloned()
            .map(Gid::as_raw)
            .collect::<Vec<_>>()
    );
    println!(
        "pwd: {}",
        env::current_dir().expect("current_dir").display()
    );
    println!(
        "exe: {}",
        env::current_exe().expect("current_exe").display()
    );

    for set in &[
        caps::CapSet::Ambient,
        caps::CapSet::Bounding,
        caps::CapSet::Effective,
        caps::CapSet::Inheritable,
        caps::CapSet::Permitted,
    ] {
        println!(
            "caps {}: {:?}",
            format!("{:?}", set).as_str().to_lowercase(),
            caps::read(None, *set).expect("Failed to read caps")
        );
    }

    println!("/proc/self/fd:");
    fs::read_dir("/proc/self/fd")
        .expect("read_dir /proc/self/fd")
        .map(|e| e.unwrap().path())
        .map(|p| (p.clone(), fs::read_link(p).expect("readlink entry")))
        .filter(|(_, l)| l != &PathBuf::from(format!("/proc/{}/fd", std::process::id())))
        .for_each(|(p, l)| {
            println!("    {}: {}", p.display(), l.display());
        });
    // Substract the ReadDir fd
    println!(
        "    total: {}",
        fs::read_dir("/proc/self/fd").unwrap().count() - 1
    );

    dump("/proc/self/mounts");
}

fn sleep(seconds: u64) {
    thread::sleep(time::Duration::from_secs(seconds));
    println!("Exiting after {} seconds sleep", seconds);
    process::exit(0);
}
