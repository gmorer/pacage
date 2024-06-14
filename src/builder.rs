use crate::conf::Makepkg;
use log::{error, info};
use std::collections::HashSet;
use std::fs::{self};
use std::io;
use std::path::Path;
use thiserror::Error;

use crate::cmd::{command, out_to_file, write_last_lines, CmdError, ExecError, NOENV};
use crate::download::SrcInfo;
use crate::{Conf, DurationPrinter, BUILD_SCRIPT_FILE};

const CONTAINER_NAME: &str = "pacage_builder";

#[derive(Debug, Error)]
pub enum BuilderError {
    #[error("System error: {0}")]
    ExecError(#[from] ExecError),
    #[error("Cmd error: {0}")]
    CmdError(#[from] CmdError),
    #[error("IO error: {0}")]
    IOError(#[from] io::Error),
    // #[error("Patch error: {0}")]
    // PatchError(#[from] PatchError),
}

pub struct Builder {
    container_runner: String,
}

pub fn should_build(pkgbuilds: &HashSet<SrcInfo>) -> bool {
    for pkgbuild in pkgbuilds {
        if pkgbuild.src {
            return true;
        }
    }
    false
}

impl Builder {
    fn stop_builder(container_runner: &str) {
        // Stop previous builds
        command(&[container_runner, "stop", CONTAINER_NAME], "/", NOENV).ok();
        command(&[container_runner, "rm", CONTAINER_NAME], "/", NOENV).ok();
    }

    pub fn new(conf: &Conf) -> Result<Self, BuilderError> {
        info!("Initiating builder container...");
        Self::stop_builder(&conf.container_runner);

        let server_dir = conf.host_server_dir.as_deref();
        let server_dir = String::from_utf8_lossy(
            server_dir
                .unwrap_or(&conf.server_dir)
                .as_os_str()
                .as_encoded_bytes(),
        );

        let (status, out, _) = command(
            &[
                &conf.container_runner,
                "run",
                "--rm",
                "--pids-limit", // got a pid limit :/
                "-1",
                "--name",
                CONTAINER_NAME,
                "-d", // detach
                &format!("-v={}:/build", server_dir),
                "archlinux:base-devel",
                "sh",
                "-c",
                "sleep infinity",
            ],
            &conf.server_dir,
            NOENV,
        )?;
        if !status.success() {
            error!("Fail to spawn builder");
            Err(CmdError::from_output(out))?;
        }
        let (status, out, _) = command(
            &[
                &conf.container_runner,
                "exec",
                "--workdir=/build",
                "--env=HOME=/tmp",
                "--env=CCACHE_DIR=/build/cache/ccache/",
                CONTAINER_NAME,
                "bash",
                &format!("/build/{}", BUILD_SCRIPT_FILE),
                "start",
            ],
            &conf.server_dir,
            NOENV,
        )?;
        match out_to_file(conf, "pacage_builder", "start", &out, status.success()) {
            Ok(Some(file)) => info!("Start logs writed to {}", file),
            Ok(None) => {}
            Err(e) => error!("Failed to write output to logs: {}", e),
        }
        if !status.success() {
            error!("Failed to start builder");
            Err(CmdError::from_output(out))?;
        }
        info!("Builder container initiated");
        Ok(Self {
            container_runner: conf.container_runner.clone(),
        })
    }

    pub fn download_src(
        &self,
        conf: &Conf,
        name: &str,
        makepkgconf: Option<&Makepkg>,
    ) -> Result<(), BuilderError> {
        fs::write(
            Path::new(&conf.server_dir).join("makepkg.conf"),
            Makepkg::get_conf_file(&conf, makepkgconf, name)?,
        )?;
        info!("[{}] downloading the sources...", name);
        let (status, out, _) = command(
            &[
                &conf.container_runner,
                "exec",
                "--workdir=/build",
                "--env=HOME=/tmp",
                "--env=CCACHE_DIR=/build/cache/ccache/",
                CONTAINER_NAME,
                "bash",
                &format!("/build/{}", BUILD_SCRIPT_FILE),
                "get",
                name,
            ],
            &conf.server_dir,
            NOENV,
        )?;
        match out_to_file(conf, name, "get", &out, status.success()) {
            Ok(Some(file)) => info!("[{}] Get logs writed to {}", name, file),
            Ok(None) => {}
            Err(e) => error!("[{}] Failed to write output to logs: {}", name, e),
        }
        if !status.success() {
            error!("[{}] Failed to get sources ", name);
            write_last_lines(&out, 10);
            Err(CmdError::from_output(out))?
        }
        info!("[{}] sources downloaded", name);
        Ok(())
    }

    pub fn build_pkg(
        &self,
        conf: &Conf,
        name: &str,
        makepkgconf: Option<&Makepkg>,
    ) -> Result<(), BuilderError> {
        info!("[{}] Building/packaging the sources...", name);
        fs::write(
            Path::new(&conf.server_dir).join("makepkg.conf"),
            Makepkg::get_conf_file(&conf, makepkgconf, name)?,
        )?;
        let (status, out, elapsed) = command(
            &[
                &conf.container_runner,
                "exec",
                "--workdir=/build",
                "--env=HOME=/tmp",
                "--env=CCACHE_DIR=/build/cache/ccache/",
                CONTAINER_NAME,
                "bash",
                &format!("/build/{}", BUILD_SCRIPT_FILE),
                "build",
                name,
            ],
            &conf.server_dir,
            NOENV,
        )?;
        match out_to_file(conf, name, "build", &out, status.success()) {
            Ok(Some(file)) => info!("[{}] Build logs writed to {}", name, file),
            Ok(None) => {}
            Err(e) => error!("[{}] Failed to write output to logs: {}", name, e),
        }
        if !status.success() {
            error!(
                "[{}] Failed to build in {} ->",
                name,
                DurationPrinter(elapsed)
            );
            write_last_lines(&out, 10);
            Err(CmdError::from_output(out))?
        } else {
            info!(
                "[{}] Build sucessfull in {}",
                name,
                DurationPrinter(elapsed)
            );
            Ok(())
        }
    }
}

impl Drop for Builder {
    fn drop(&mut self) {
        info!("Stoping builder...");
        Self::stop_builder(&self.container_runner);
        info!("Builder stoped");
    }
}
