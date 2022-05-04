use std::env::Args;
use std::net::{SocketAddr, ToSocketAddrs};

use configparser::ini::Ini;

use crate::Error;

pub(crate) struct Config {
    pub(crate) local_addr: SocketAddr,
}

impl Config {
    pub(crate) fn with_args(mut args: Args) -> Result<Self, Error> {
        // Select path of config file from arguments or default:
        let config_path = if let Some(arg) = args.next() {
            if arg != "-c" && arg != "--config-file" {
                panic!("Unknown argument."); // TODO
            }
            if let Some(p_arg) = args.next() {
                p_arg
            } else {
                panic!("Missing argument: config-path"); // TODO
            }
        } else {
            "/etc/kutsche.config".to_string()
        };

        // Load config file:
        let mut file_cfg = Ini::new();
        file_cfg.load(config_path)?;

        let local_addr = file_cfg
            .get("KUTSCHE", "bind_address")
            .unwrap_or_else(|| "127.0.0.1:25".to_string())
            .to_socket_addrs()
            .map_err(|_| Error::Config("Could not resolve value of 'bind_address'.".to_string()))?
            .next()
            .ok_or_else(|| {
                Error::Config("Could not resolve value of 'bind_address'.".to_string())
            })?;

        Ok(Config { local_addr })
    }
}
