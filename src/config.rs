use clap::Parser;
use jsonrpsee::core::JsonValue;
use serde::Deserialize;
use std::{fs, num::NonZeroUsize};

const SUBSTRATE_CONFIG: &str = include_str!("../rpc_configs/substrate.yml");
const ETHEREUM_CONFIG: &str = include_str!("../rpc_configs/ethereum.yml");

#[derive(Parser, Debug)]
#[command(version, about)]
struct Command {
    /// The config file to use
    #[arg(short, long, default_value = "./config.yml")]
    config: String,
}

#[derive(Deserialize, Debug)]
pub struct ServerConfig {
    pub listen_address: String,
    pub port: u16,
    pub max_connections: u32,
}

#[derive(Deserialize, Debug)]
pub struct RpcMethod {
    pub method: String,
    #[serde(default)]
    pub params: Vec<MethodParam>,

    /// if cache_ttl_seconds is defined, cache default will be 1
    #[serde(default)]
    pub cache: usize,

    // None means use global cache_ttl_seconds
    #[serde(default)]
    pub cache_ttl_seconds: Option<u64>,

    #[serde(default)]
    pub response: Option<JsonValue>,
}

impl RpcMethod {
    pub fn cache_size(&self) -> Option<NonZeroUsize> {
        match (self.cache, self.cache_ttl_seconds) {
            (0, None) => None,
            (0, Some(0)) => None,
            (0, Some(_)) => NonZeroUsize::new(1),
            (cache, _) => NonZeroUsize::new(cache),
        }
    }
}

#[derive(Clone, Deserialize, Debug, Eq, PartialEq)]
pub struct MethodParam {
    pub name: String,
    #[serde(default)]
    pub ty: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub inject: bool,
}

#[derive(Copy, Clone, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    // Replace old value with new value
    Replace,
    // Merge old storage changes with new changes
    MergeStorageChanges,
}

#[derive(Deserialize, Debug)]
pub struct RpcSubscription {
    pub subscribe: String,
    pub unsubscribe: String,
    pub name: String,

    #[serde(default)]
    pub merge_strategy: Option<MergeStrategy>,
}

#[derive(Deserialize, Debug)]
pub struct RpcDefinitions {
    pub methods: Vec<RpcMethod>,
    #[serde(default)]
    pub subscriptions: Vec<RpcSubscription>,
    #[serde(default)]
    pub aliases: Vec<(String, String)>,
}

#[derive(Deserialize, Debug)]
pub struct RpcDefinitionsWithBase {
    #[serde(default)]
    pub base: Option<RpcOptions>,
    #[serde(default)]
    pub methods: Vec<RpcMethod>,
    #[serde(default)]
    pub subscriptions: Vec<RpcSubscription>,
    #[serde(default)]
    pub aliases: Vec<(String, String)>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum RpcOptions {
    Path(String),
    Custom(Box<RpcDefinitionsWithBase>),
}

impl From<RpcDefinitionsWithBase> for RpcDefinitions {
    fn from(defs: RpcDefinitionsWithBase) -> Self {
        if let Some(base) = defs.base {
            let base: RpcDefinitions = base.into();

            let mut methods = base.methods;
            methods.sort_by(|a, b| a.method.cmp(&b.method));
            for m in defs.methods {
                let idx = methods.binary_search_by(|probe| probe.method.cmp(&m.method));
                match idx {
                    Ok(i) => {
                        methods[i] = m;
                    }
                    Err(i) => {
                        methods.insert(i, m);
                    }
                }
            }

            let mut subscriptions = base.subscriptions;
            subscriptions.sort_by(|a, b| a.name.cmp(&b.name));
            for s in defs.subscriptions {
                let idx = subscriptions.binary_search_by(|probe| probe.name.cmp(&s.name));
                match idx {
                    Ok(i) => {
                        subscriptions[i] = s;
                    }
                    Err(i) => {
                        subscriptions.insert(i, s);
                    }
                }
            }

            let mut aliases = base.aliases;
            aliases.sort_by(|a, b| a.0.cmp(&b.0));
            for a in defs.aliases {
                let idx = aliases.binary_search_by(|probe| probe.0.cmp(&a.0));
                match idx {
                    Ok(i) => {
                        aliases[i] = a;
                    }
                    Err(i) => {
                        aliases.insert(i, a);
                    }
                }
            }

            return RpcDefinitions {
                methods,
                subscriptions,
                aliases,
            };
        }
        RpcDefinitions {
            methods: defs.methods,
            subscriptions: defs.subscriptions,
            aliases: defs.aliases,
        }
    }
}

impl From<RpcOptions> for RpcDefinitions {
    fn from(val: RpcOptions) -> Self {
        match val {
            RpcOptions::Path(path) => match path.to_lowercase().as_str() {
                "sub" | "substrate" => serde_yaml::from_str(SUBSTRATE_CONFIG).unwrap(),
                "eth" | "ethereum" => serde_yaml::from_str(ETHEREUM_CONFIG).unwrap(),
                _ => {
                    let file = fs::File::open(path).expect("Invalid rpc config path");
                    let defs: RpcDefinitionsWithBase =
                        serde_yaml::from_reader(file).expect("Invalid rpc config file");
                    defs.into()
                }
            },
            RpcOptions::Custom(defs) => (*defs).into(),
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryProvider {
    None,
    Datadog,
    Jaeger,
}

#[derive(Deserialize, Debug)]
pub struct TelemetryOptions {
    pub provider: TelemetryProvider,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub agent_endpoint: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct HealthConfig {
    pub path: String,
    pub method: String,
}

#[derive(Debug)]
pub struct Config {
    pub endpoints: Vec<String>,
    pub stale_timeout_seconds: u64,
    // None means no cache expiration
    pub cache_ttl_seconds: Option<u64>,
    pub merge_subscription_keep_alive_seconds: Option<u64>,
    pub server: ServerConfig,
    pub rpcs: RpcDefinitions,
    pub telemetry: Option<TelemetryOptions>,
    pub health: Option<HealthConfig>,
}

#[derive(Deserialize, Debug)]
pub struct ParseConfig {
    pub endpoints: Vec<String>,
    pub stale_timeout_seconds: u64,
    #[serde(default)]
    pub cache_ttl_seconds: Option<u64>,
    pub merge_subscription_keep_alive_seconds: Option<u64>,
    pub server: ServerConfig,
    pub rpcs: RpcOptions,
    #[serde(default)]
    pub telemetry: Option<TelemetryOptions>,
    #[serde(default)]
    pub health: Option<HealthConfig>,
}

impl From<ParseConfig> for Config {
    fn from(val: ParseConfig) -> Self {
        Config {
            endpoints: val.endpoints,
            stale_timeout_seconds: val.stale_timeout_seconds,
            cache_ttl_seconds: val.cache_ttl_seconds,
            merge_subscription_keep_alive_seconds: val.merge_subscription_keep_alive_seconds,
            server: val.server,
            rpcs: val.rpcs.into(),
            telemetry: val.telemetry,
            health: val.health,
        }
    }
}

pub fn read_config() -> Result<Config, String> {
    let cmd = Command::parse();

    let config =
        fs::File::open(cmd.config).map_err(|e| format!("Unable to open config file: {e}"))?;
    let config: ParseConfig = serde_yaml::from_reader(&config)
        .map_err(|e| format!("Unable to parse config file: {e}"))?;
    let mut config: Config = config.into();

    if let Ok(endpoints) = std::env::var("ENDPOINTS") {
        log::debug!("Override endpoints with env.ENDPOINTS");
        config.endpoints = endpoints
            .split(',')
            .map(|x| x.trim().to_string())
            .collect::<Vec<_>>();
    }

    if let Ok(env_port) = std::env::var("PORT") {
        log::debug!("Override port with env.PORT");
        let port = env_port.parse::<u16>();
        if let Ok(port) = port {
            config.server.port = port;
        }
    }

    validate_config(&config)?;

    Ok(config)
}

fn validate_config(config: &Config) -> Result<(), String> {
    // validate endpoints
    for endpoint in &config.endpoints {
        if endpoint
            .parse::<jsonrpsee::client_transport::ws::Uri>()
            .is_err()
        {
            return Err(format!("Invalid endpoint {}", endpoint));
        }
    }

    // ensure each method has only one param with inject=true
    for method in &config.rpcs.methods {
        if method.params.iter().filter(|x| x.inject).count() > 1 {
            return Err(format!(
                "Method {} has more than one inject param",
                method.method
            ));
        }
    }

    // ensure there is no required param after optional param
    for method in &config.rpcs.methods {
        let mut has_optional = false;
        for param in &method.params {
            if param.optional {
                has_optional = true;
            } else if has_optional {
                return Err(format!(
                    "Method {} has required param after optional param",
                    method.method
                ));
            }
        }
    }

    Ok(())
}
