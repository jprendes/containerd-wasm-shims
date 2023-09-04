use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use containerd_shim as shim;
use containerd_shim_wasm::container::{Engine, Instance, RuntimeContext};
use containerd_shim_wasm::sandbox::{ShimCli, Stdio};
use serde::de::DeserializeOwned;
use spin_manifest::{Application, ApplicationTrigger};
use spin_redis_engine::RedisTrigger;
use spin_trigger::cli::NoArgs;
use spin_trigger::loader::TriggerLoader;
use spin_trigger::locked::build_locked_app;
use spin_trigger::{RuntimeConfig, TriggerExecutor, TriggerExecutorBuilder};
use spin_trigger_http::{CliArgs, HttpTrigger};
use tokio::runtime::Runtime;
use url::Url;

const SPIN_ADDR: &str = "0.0.0.0:80";

fn parse_addr(addr: &str) -> Result<SocketAddr> {
    addr.to_socket_addrs()?
        .next()
        .with_context(|| format!("could not parse address: {addr}"))
}

#[derive(Clone, Default)]
struct SpinEngine;

impl Engine for SpinEngine {
    fn name() -> &'static str {
        "spin"
    }

    fn run(&self, _ctx: impl RuntimeContext, stdio: Stdio) -> anyhow::Result<i32> {
        let rt = Runtime::new().context("failed to create runtime")?;
        rt.block_on(async {
            stdio.redirect().context("failed to redirect stdio")?;

            log::info!(" >>> building spin application");
            let app = spin_loader::from_file("/spin.toml", Some("/"))
                .await
                .context("failed to build spin application")?;

            match app.info.trigger {
                ApplicationTrigger::Http(_) => {
                    log::info!(" >>> running spin http trigger");
                    build_spin_trigger::<HttpTrigger>("/", app)
                        .await
                        .context("failed to build spin http trigger")?
                        .run(CliArgs {
                            address: parse_addr(SPIN_ADDR).unwrap(),
                            tls_cert: None,
                            tls_key: None,
                        })
                        .await
                }
                ApplicationTrigger::Redis(_) => {
                    log::info!(" >>> running spin redis trigger");
                    build_spin_trigger::<RedisTrigger>("/", app)
                        .await
                        .context("failed to build spin redis trigger")?
                        .run(NoArgs)
                        .await
                }
                _ => todo!("Only Http and Redis triggers are currently supported."),
            }
        })?;
        Ok(0)
    }
}

async fn build_spin_trigger<T>(working_dir: impl AsRef<Path>, app: Application) -> Result<T>
where
    T: spin_trigger::TriggerExecutor,
    T::TriggerConfig: DeserializeOwned,
{
    let working_dir = working_dir.as_ref();

    // Build and write app lock file
    let app = build_locked_app(app, working_dir).context("building licked app")?;
    let path = working_dir.join("spin.lock");
    let buff = serde_json::to_string(&app).context("could not serialize locked app")?;
    std::fs::write(&path, buff).context("could not write locked app")?;
    let uri = Url::from_file_path(&path)
        .map_err(|_| anyhow!("converting {path:?} to file URL"))?
        .to_string();

    // Build trigger config
    let loader = TriggerLoader::new(working_dir, true);
    let mut builder = TriggerExecutorBuilder::new(loader);
    builder
        .wasmtime_config_mut()
        .cranelift_opt_level(wasmtime::OptLevel::Speed);
    let config = RuntimeConfig::new(Some("/".into()));
    builder.build(uri, config, Default::default()).await
}

fn main() {
    shim::run::<ShimCli<Instance<SpinEngine>>>("io.containerd.spin.v1", None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_spin_address() {
        let parsed = parse_addr(SPIN_ADDR).unwrap();
        assert_eq!(parsed.clone().port(), 80);
        assert_eq!(parsed.ip().to_string(), "0.0.0.0");
    }
}
