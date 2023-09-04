use std::sync::Arc;

use anyhow::{Context, Result};
use containerd_shim as shim;
use containerd_shim_wasm::container::{Engine, Instance, RuntimeContext};
use containerd_shim_wasm::sandbox::stdio::Stdio;
use containerd_shim_wasm::sandbox::ShimCli;
use lunatic_process::env::{Environment, Environments, LunaticEnvironments};
use lunatic_process::runtimes::{self, RawWasm};
use lunatic_process::wasm::spawn_wasm;
use lunatic_process_api::ProcessConfigCtx;
use lunatic_runtime::{DefaultProcessConfig, DefaultProcessState};
use tokio::runtime::Runtime;

#[derive(Clone, Default)]
struct LunaticEngine;

impl Engine for LunaticEngine {
    fn name() -> &'static str {
        "lunatic"
    }

    fn run(&self, ctx: impl RuntimeContext, stdio: Stdio) -> Result<i32> {
        let rt = Runtime::new().context("failed to create runtime")?;
        rt.block_on(async {
            stdio.redirect().context("failed to redirect stdio")?;
            let (path, method) = ctx.module();
            let wasm_args = ctx.args().iter().skip(1).cloned();
            let wasm_envs = ctx
                .envs()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect();

            log::info!(" >>> building lunatic application from binary {path:?}");
            // Create wasmtime runtime
            let config = runtimes::wasmtime::default_config();
            let runtime = runtimes::wasmtime::WasmtimeRuntime::new(&config)?;
            let envs = Arc::new(LunaticEnvironments::default());

            let env = envs.create(1).await;

            let mut config = DefaultProcessConfig::default();
            config.set_can_compile_modules(true);
            config.set_can_create_configs(true);
            config.set_can_spawn_processes(true);

            let filename = path
                .file_name()
                .context("Invalid path")?
                .to_string_lossy()
                .to_string();
            config.set_command_line_arguments([filename].into_iter().chain(wasm_args).collect());
            config.set_environment_variables(wasm_envs);
            config.preopen_dir("/");

            let module: RawWasm = std::fs::read(path)
                .with_context(|| format!("reading module {path:?}"))?
                .into();
            let module = Arc::new(runtime.compile_module::<DefaultProcessState>(module)?);

            let state = DefaultProcessState::new(
                env.clone(),
                None,
                runtime.clone(),
                module.clone(),
                Arc::new(config),
                Default::default(),
            )?;

            env.can_spawn_next_process().await?;
            let (task, _) = spawn_wasm(env, runtime, &module, state, method, Vec::new(), None)
                .await
                .context(format!(
                    "Failed to spawn process from {}::{method}()",
                    path.to_string_lossy()
                ))?;

            task.await?.context("running wasm process")
        })?;
        Ok(0)
    }
}

fn main() {
    shim::run::<ShimCli<Instance<LunaticEngine>>>("io.containerd.lunatic.v1", None);
}
