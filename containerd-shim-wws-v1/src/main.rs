use std::path::Path;

use anyhow::{Context, Result};
use containerd_shim as shim;
use containerd_shim_wasm::container::{Engine, Instance, RuntimeContext};
use containerd_shim_wasm::sandbox::stdio::Stdio;
use containerd_shim_wasm::sandbox::ShimCli;
use tokio::runtime::Runtime;
use wws_config::Config;
use wws_router::Routes;
use wws_server::serve;

/// URL to listen to in wws
const WWS_ADDR: &str = "0.0.0.0";
const WWS_PORT: u16 = 3000;

#[derive(Clone, Default)]
struct WwsEngine;

impl Engine for WwsEngine {
    fn name() -> &'static str {
        "wws"
    }

    fn run(&self, _ctx: impl RuntimeContext, stdio: Stdio) -> Result<i32> {
        let rt = Runtime::new().context("failed to create runtime")?;
        rt.block_on(async {
            stdio.stderr.redirect().context("failed to redirect stdio")?;
            let root = Path::new("/");

            let config = Config::load(root).unwrap_or_else(|err| {
                log::error!("[wws] Error reading .wws.toml file. It will be ignored");
                log::error!("[wws] Error: {err}");
                Config::default()
            });

            // Check if there're missing runtimes
            if config.is_missing_any_runtime(root) {
                log::error!("[wws] Required language runtimes are not installed. Some files may not be considered workers");
                log::error!("[wws] You can install the missing runtimes with: wws runtimes install");
            }

            let routes = Routes::new(root, "", Vec::new(), &config);
            let server = serve(root, routes, WWS_ADDR, WWS_PORT, false, None).await?;

            log::info!(" >>> notifying main thread we are about to start");
            server.await.context("running wws server")
        })?;
        Ok(0)
    }
}

fn main() {
    shim::run::<ShimCli<Instance<WwsEngine>>>("io.containerd.wws.v1", None);
}
