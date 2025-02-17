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

use ::npk::npk::{open_zip, Npk, MANIFEST_NAME, SIGNATURE_NAME, UNSQUASHFS};
use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use npk::npk::FS_IMG_NAME;
use std::{
    fs::File,
    io::{self, BufReader, Read},
    path::Path,
    process::Command,
};

pub fn inspect(npk: &Path, short: bool) -> Result<()> {
    if short {
        inspect_short(&npk)
    } else {
        inspect_long(&npk)
    }
}

pub fn inspect_short(npk: &Path) -> Result<()> {
    let npk = Npk::<BufReader<File>>::from_path(npk, None)?;
    let manifest = npk.manifest();
    let name = manifest.name.to_string();
    let version = manifest.version.to_string();
    let npk_version = npk.version();
    let is_resource_container = manifest.init.as_ref().map_or("yes", |_| "no");
    println!(
        "name: {}, version: {}, NPK version: {}, resource container: {}",
        name, version, npk_version, is_resource_container
    );

    Ok(())
}

pub fn inspect_long(npk: &Path) -> Result<()> {
    let mut zip = open_zip(&npk)?;
    let mut print_buf: String = String::new();
    println!(
        "{}",
        format!("# inspection of '{}'", &npk.display()).green()
    );
    println!("{}", "## NPK Content".to_string().green());
    zip.file_names().for_each(|f| println!("{}", f));
    println!();

    // print manifest
    let mut man = zip
        .by_name(MANIFEST_NAME)
        .context("Failed to find manifest in NPK")?;
    println!("{}", format!("## {}", MANIFEST_NAME).green());
    man.read_to_string(&mut print_buf)
        .with_context(|| "Failed to read manifest")?;
    println!("{}", &print_buf);
    print!("\n\n");
    print_buf.clear();
    drop(man);

    // print signature
    match zip.by_name(SIGNATURE_NAME) {
        Ok(mut sig) => {
            println!("{}", format!("## {}", SIGNATURE_NAME).green());
            sig.read_to_string(&mut print_buf)
                .with_context(|| "Failed to read signature")?;
            println!("{}", &print_buf);
            print!("\n\n");
            print_buf.clear();
            drop(sig);
        }
        _ => println!("No signature found"),
    }

    // print squashfs listing
    println!("{}", "## SquashFS listing".green());
    let mut dest_fsimage = tempfile::NamedTempFile::new().context("Failed to create tmp file")?;
    let mut src_fsimage = zip
        .by_name(FS_IMG_NAME)
        .context("Failed to find filesystem image in NPK")?;
    io::copy(&mut src_fsimage, &mut dest_fsimage)?;
    let path = dest_fsimage.path();
    print_squashfs(&path)?;

    Ok(())
}

fn print_squashfs(fsimg_path: &Path) -> Result<()> {
    which::which(&UNSQUASHFS).with_context(|| anyhow!("Failed to find '{}'", &UNSQUASHFS))?;

    let mut cmd = Command::new(&UNSQUASHFS);
    cmd.arg("-ll").arg(fsimg_path.display().to_string());

    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute '{}'", &UNSQUASHFS))?;

    println!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}

#[cfg(test)]
mod test {
    use super::inspect;
    use npk::npk::{gen_key, pack};
    use std::{
        fs::File,
        io::Write,
        path::{Path, PathBuf},
    };

    const TEST_KEY_NAME: &str = "test_key";
    const TEST_MANIFEST: &str = "name: hello
version: 0.0.2
init: /hello
env:
  HELLO: north
# autostart: true
uid: 1000
gid: 1000
mounts:
    /lib:
      type: bind
      host: /lib
    /lib64:
      type: bind
      host: /lib64
    /system:
      type: bind
      host: /system";

    fn create_test_npk(dest: &Path) -> PathBuf {
        let src = create_tmp_dir();
        let key_dir = create_tmp_dir();
        let manifest = create_test_manifest(&src);
        let (_pub_key, prv_key) = gen_test_key(&key_dir);
        pack(&manifest, &src, &dest, Some(&prv_key)).expect("Pack NPK");
        dest.join("hello-0.0.2.npk")
    }

    fn create_test_manifest(src: &PathBuf) -> PathBuf {
        let manifest = src.join("manifest").with_extension("yaml");
        File::create(&manifest)
            .expect("Create manifest.yaml")
            .write_all(TEST_MANIFEST.as_ref())
            .expect("Write test manifest");
        manifest
    }

    fn create_tmp_dir() -> PathBuf {
        tempfile::TempDir::new()
            .expect("Create tmp dir")
            .into_path()
    }

    fn gen_test_key(key_dir: &Path) -> (PathBuf, PathBuf) {
        gen_key(&TEST_KEY_NAME, &key_dir).expect("Generate key pair");
        let prv_key = key_dir.join(&TEST_KEY_NAME).with_extension("key");
        let pub_key = key_dir.join(&TEST_KEY_NAME).with_extension("pub");
        assert!(prv_key.exists());
        assert!(pub_key.exists());
        (pub_key, prv_key)
    }

    #[test]
    fn inspect_npk() {
        let npk = create_test_npk(&create_tmp_dir());
        assert!(npk.exists());
        inspect(&npk, true).expect("Inspect NPK");
        inspect(&npk, false).expect("Inspect NPK");
    }

    #[test]
    fn inspect_npk_no_file() {
        inspect(&Path::new("invalid"), true).expect_err("Invalid NPK");
        inspect(&Path::new("invalid"), false).expect_err("Invalid NPK");
    }
}
