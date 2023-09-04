use anyhow::Context;
use containerd_shim as shim;
use containerd_shim_wasm::container::{Engine, Instance, RuntimeContext};
use containerd_shim_wasm::sandbox::stdio::Stdio;
use containerd_shim_wasm::sandbox::ShimCli;
use slight_lib::commands::run::{handle_run, RunArgs};
use tokio::runtime::Runtime;

#[derive(Clone, Default)]
struct SlightEngine;

impl Engine for SlightEngine {
    fn name() -> &'static str {
        "slight"
    }

    fn run(&self, _ctx: impl RuntimeContext, stdio: Stdio) -> anyhow::Result<i32> {
        let rt = Runtime::new().context("failed to create runtime")?;
        rt.block_on(async {
            stdio.redirect().context("failed to redirect stdio")?;
            handle_run(RunArgs {
                module: "/app.wasm".into(),
                slightfile: "/slightfile.toml".into(),
                io_redirects: None,
                link_all_capabilities: true,
            })
            .await
        })?;
        Ok(0)
    }
}

fn main() {
    shim::run::<ShimCli<Instance<SlightEngine>>>("io.containerd.slight.v1", None);
}
