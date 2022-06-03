use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::Arc;

use configparser::ini::Ini;
use rustls::{server::ServerConfig, Certificate, PrivateKey};
use rustls_pemfile::{read_all, read_one, Item};
use users::{get_group_by_name, get_user_by_name, Group, User};

use crate::maildest::{EmailDestination, FileDestination};
use crate::Error;

pub(crate) struct Config {
    pub(crate) effective_user: Option<User>,
    pub(crate) effective_group: Option<Group>,
    pub(crate) local_addr: SocketAddr, // TODO: multiple addresses
    default_path: Option<PathBuf>,
    pub(crate) dest_map: HashMap<String, Box<dyn EmailDestination>>,
    pub(crate) tls_config: Option<Arc<ServerConfig>>,
}

impl Config {
    pub(crate) fn with_args(mut args: impl Iterator<Item = String>) -> Result<Self, Error> {
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

        // Get new unix user and group:
        let effective_user = if let Some(name) = main_section.remove("unix_user").flatten() {
            Some(get_user_by_name(&name).ok_or_else(|| {
                Error::Config("The user given by 'unix_user' does not exist.".to_string())
            })?)
        } else {
            None
        };
        let effective_group = if let Some(name) = main_section.remove("unix_group").flatten() {
            Some(get_group_by_name(&name).ok_or_else(|| {
                Error::Config("The group given by 'unix_group' does not exist.".to_string())
            })?)
        } else {
            None
        };

        // Get TLS configuration:
        let tls_config = if local_addr.port() == 465 {
            // Read certificates:
            let cert_file = File::open(
                main_section
                    .remove("cert_file")
                    .flatten()
                    .ok_or_else(|| Error::Config("Missing key 'cert_file'.".to_string()))?,
            )?;
            let mut reader = BufReader::new(cert_file);
            let certs = read_all(&mut reader)?
                .into_iter()
                .filter_map(|item| {
                    if let Item::X509Certificate(raw) = item {
                        Some(Certificate(raw))
                    } else {
                        None
                    }
                })
                .collect();

            // Read private key:
            let key_file = File::open(
                main_section
                    .remove("private_key_file")
                    .flatten()
                    .ok_or_else(|| Error::Config("Missing key 'private_key_file'.".to_string()))?,
            )?;
            let mut reader = BufReader::new(key_file);
            let priv_key = if let Some(Item::RSAKey(raw) | Item::PKCS8Key(raw) | Item::ECKey(raw)) =
                read_one(&mut reader)?
            {
                PrivateKey(raw)
            } else {
                return Err(Error::Config(
                    "Could not read key from 'private_key_file'.".to_string(),
                ));
            };

            Some(Arc::new(
                ServerConfig::builder()
                    .with_safe_defaults()
                    .with_no_client_auth()
                    .with_single_cert(certs, priv_key)?,
            ))
        } else {
            None
        };

        // Get default file destination base directory:
        let default_path: Option<PathBuf> = main_section
            .remove("default_path")
            .flatten()
            .map(PathBuf::from);

        Config {
            effective_user,
            effective_group,
            local_addr,
            default_path,
            dest_map: HashMap::new(),
            tls_config,
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

#[cfg(test)]
impl Default for Config {
    fn default() -> Self {
        Config {
            effective_user: None,
            effective_group: None,
            local_addr: "127.0.0.1:25".to_socket_addrs().unwrap().next().unwrap(),
            default_path: None,
            dest_map: HashMap::new(),
            tls_config: None,
        }
    }
}
