use std::env::current_dir;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use rand::distr::{Alphanumeric, SampleString};
use rand::rng;

const VERSION: &str = match std::option_env!("CARGO_PKG_VERSION") {
    Some(version) => version,
    None => "unknown",
};

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    let cfg = Config::from_env_and_cli();

    match cfg {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Error loading configuration: {}", err);
            std::process::exit(1);
        }
    }
});

#[derive(Debug)]
pub struct Config {
    pub listen: SocketAddr,
    pub store_path: PathBuf,
    pub chunk_size: usize,
    pub username: String,
    pub password: String,
    pub secret: String,
    pub token_expiry: u64,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            listen: SocketAddr::V6("[::]:8080".parse().unwrap()),
            store_path: current_dir().unwrap(),
            chunk_size: 1024 * 1024 * 8, // 8MB
            username: "admin".to_string(),
            password: "password".to_string(),
            secret: Alphanumeric.sample_string(&mut rng(), 16),
            token_expiry: 60 * 60 * 24, // 24 hours
        }
    }
}
impl Config {
    /// Get the configuration from the environment variables and command line arguments,
    /// and use default values for any missing configuration.
    pub fn from_env_and_cli() -> Result<Self> {
        let mut config = Self::default();
        let user_config = UserConfig::from_env_and_cli()?;

        if let Some(listen) = user_config.listen {
            config.listen = listen.parse().context("Invalid listen address")?;
        }

        if let Some(store_path_string) = user_config.store_path {
            let store_path = current_dir()?.join(PathBuf::from(store_path_string));
            if !store_path.is_dir() {
                bail!(
                    "Store path `{}` is not a directory.",
                    store_path.to_string_lossy()
                );
            }
            config.store_path = store_path;
        }

        if let Some(chunk_size_string) = user_config.chunk_size {
            let chunk_size = chunk_size_string.parse().context("Invalid chunk size")?;
            config.chunk_size = chunk_size;
        }

        if let Some(username) = user_config.username {
            config.username = username;
        }

        if let Some(password) = user_config.password {
            config.password = password;
        }

        if let Some(secret) = user_config.secret {
            config.secret = secret;
        }

        if let Some(token_expiry_string) = user_config.token_expiry {
            let token_expiry = token_expiry_string
                .parse()
                .context("Invalid token expiry")?;
            config.token_expiry = token_expiry;
        }

        Ok(config)
    }
}

#[derive(Default)]
pub struct UserConfig {
    listen: Option<String>,
    store_path: Option<String>,
    chunk_size: Option<String>,
    username: Option<String>,
    password: Option<String>,
    secret: Option<String>,
    token_expiry: Option<String>,
}
impl UserConfig {
    /// Get the configuration from the environment variables.
    pub fn from_env() -> Self {
        let mut config = UserConfig::default();

        if let Ok(listen) = std::env::var("SFS_LISTEN") {
            config.listen = Some(listen);
        }

        if let Ok(store_path) = std::env::var("SFS_STORE_PATH") {
            config.store_path = Some(store_path);
        }

        if let Ok(chunk_size) = std::env::var("SFS_CHUNK_SIZE") {
            config.chunk_size = Some(chunk_size);
        }

        if let Ok(username) = std::env::var("SFS_USERNAME") {
            config.username = Some(username);
        }

        if let Ok(password) = std::env::var("SFS_PASSWORD") {
            config.password = Some(password);
        }

        if let Ok(secret) = std::env::var("SFS_SECRET") {
            config.secret = Some(secret);
        }

        if let Ok(token_expiry) = std::env::var("SFS_TOKEN_EXP") {
            config.token_expiry = Some(token_expiry);
        }

        config
    }

    /// Get the configuration from the command line arguments.
    /// Also handles the `--help` and the `--version` argument,
    /// which would exits the process immediately.
    pub fn from_cli() -> Result<Self> {
        let mut config = UserConfig::default();

        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    println!(
                        "Usage: simple-file-store [OPTIONS]\n\n\
                        --listen, -l <ADDR>\t\tListen address (default: [::]:8080)\n\
                        --store-path, -p <PATH>\t\tPath to store files (default: current directory)\n\
                        --chunk-size, -s <SIZE>\t\tChunk size in bytes (default: 8MB)\n\
                        --username, -u <USERNAME>\tUsername for authentication (default: admin)\n\
                        --password, -w <PASSWORD>\tPassword for authentication (default: password)\n\
                        --secret, -x <SECRET>\t\tSecret for JWT (default: random 16 characters)\n\
                        --token-exp, -e <SECONDS>\tToken expiry in seconds (default: 24 hours)\n\
                        --version, -v\t\t\tPrint version information\n\
                        --help, -h\t\t\tPrint this help message\n\n\
                        All options are optional, they can also be set using the following environment variables:\n\
                        SFS_LISTEN\t\tListen address\n\
                        SFS_STORE_PATH\t\tPath to store files\n\
                        SFS_CHUNK_SIZE\t\tChunk size in bytes\n\
                        SFS_USERNAME\t\tUsername for authentication\n\
                        SFS_PASSWORD\t\tPassword for authentication\n\
                        SFS_SECRET\t\tSecret for JWT\n\
                        SFS_TOKEN_EXP\t\tToken expiry in seconds\n
                        "
                    );
                    std::process::exit(0);
                }

                "--version" | "-v" => {
                    println!("version {}", VERSION);
                    std::process::exit(0);
                }

                "--listen" | "-l" => {
                    if config.listen.is_some() {
                        bail!("--listen can only be specified once");
                    }
                    let listen = args.next().context("--listen requires an argument")?;
                    config.listen = Some(listen);
                }

                "--store-path" | "-p" => {
                    let store_path = args.next().context("--store-path requires an argument")?;
                    config.store_path = Some(store_path);
                }

                "--chunk-size" | "-s" => {
                    let chunk_size = args.next().context("--chunk-size requires an argument")?;
                    config.chunk_size = Some(chunk_size);
                }

                "--username" | "-u" => {
                    let username = args.next().context("--username requires an argument")?;
                    config.username = Some(username);
                }

                "--password" | "-w" => {
                    let password = args.next().context("--password requires an argument")?;
                    config.password = Some(password);
                }

                "--secret" | "-x" => {
                    let secret = args.next().context("--secret requires an argument")?;
                    config.secret = Some(secret);
                }

                "--token-exp" | "-e" => {
                    let token_expiry = args.next().context("--token-exp requires an argument")?;
                    config.token_expiry = Some(token_expiry);
                }

                _ => bail!("Unknown argument: {}", arg),
            }
        }

        Ok(config)
    }

    /// Merge the environment and command line configurations.
    pub fn from_env_and_cli() -> Result<Self> {
        let mut config = UserConfig::from_env();
        let cli_config = UserConfig::from_cli()?;

        if let Some(listen) = cli_config.listen {
            config.listen = Some(listen);
        }

        if let Some(store_path) = cli_config.store_path {
            config.store_path = Some(store_path);
        }

        if let Some(chunk_size) = cli_config.chunk_size {
            config.chunk_size = Some(chunk_size);
        }

        if let Some(username) = cli_config.username {
            config.username = Some(username);
        }

        if let Some(password) = cli_config.password {
            config.password = Some(password);
        }

        if let Some(secret) = cli_config.secret {
            config.secret = Some(secret);
        }

        if let Some(token_expiry) = cli_config.token_expiry {
            config.token_expiry = Some(token_expiry);
        }

        Ok(config)
    }
}
