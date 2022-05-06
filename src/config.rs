use std::collections::HashMap;
use std::env::Args;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;

use configparser::ini::Ini;

use crate::maildest::{EmailDestination, FileDestination};
use crate::Error;

pub(crate) struct Config {
    pub(crate) local_addr: SocketAddr,
    default_path: Option<PathBuf>,
    pub(crate) dest_map: HashMap<String, Box<dyn EmailDestination>>,
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

        let mut main_section = file_cfg
            .remove_section("KUTSCHE")
            .ok_or_else(|| Error::Config("Missing section 'KUTSCHE'.".to_string()))?;

        // Get local socket address or default:
        let local_addr = main_section
            .remove("bind_address")
            .flatten()
            .unwrap_or_else(|| "127.0.0.1:25".to_string())
            .to_socket_addrs()
            .map_err(|_| Error::Config("Could not resolve value of 'bind_address'.".to_string()))?
            .next()
            .ok_or_else(|| {
                Error::Config("Could not resolve value of 'bind_address'.".to_string())
            })?;

        // Get default file destination base directory:
        let default_path: Option<PathBuf> = main_section
            .remove("default_path")
            .flatten()
            .map(PathBuf::from);

        Config {
            local_addr,
            default_path,
            dest_map: HashMap::new(),
        }
        .load_mapping(file_cfg)
    }

    /// Loads a destination mapping from the given INI file representation to the own field dest_map.
    fn load_mapping(mut self, mapping_config: Ini) -> Result<Self, Error> {
        for mapping_name in mapping_config.sections() {
            let addr_key = mapping_config
                .get(mapping_name.as_str(), "address")
                .ok_or_else(|| {
                    Error::Config(format!("Missing 'address' for mapping '{mapping_name}'."))
                })?;
            let dest_value =
                if let Some(path) = mapping_config.get(mapping_name.as_str(), "dest_path") {
                    FileDestination::new(path)
                } else if let Some(ref base_path) = self.default_path {
                    let mut path = PathBuf::from(base_path);
                    path.push(&addr_key);
                    FileDestination::new(path)
                } else {
                    Err(Error::Config(format!(
                        "Missing destination for mapping '{mapping_name}'."
                    )))
                }?;

            self.dest_map.insert(addr_key, Box::new(dest_value));
        }

        Ok(self)
    }
}
