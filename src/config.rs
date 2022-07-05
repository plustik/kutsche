use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::{
    server::{ClientHello, ResolvesServerCert, ServerConfig},
    sign::CertifiedKey,
    Certificate, PrivateKey,
};
use rustls_pemfile::{read_all, read_one, Item};
use users::{get_group_by_name, get_user_by_name, Group, User};

use crate::maildest::{EmailDestination, FileDestination, MatrixDestBuilder};
use crate::Error;

pub(crate) struct Config {
    pub(crate) effective_user: Option<User>,
    pub(crate) effective_group: Option<Group>,
    pub(crate) local_addrs: Vec<SocketAddr>,
    default_path: Option<PathBuf>,
    pub(crate) dest_map: HashMap<String, Box<dyn EmailDestination + Send + Sync>>,
    pub(crate) tls_config: Option<Arc<ServerConfig>>,
}

impl Config {
    pub(crate) async fn with_args(mut args: impl Iterator<Item = String>) -> Result<Self, Error> {
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
        let mut cfg_file_buf = String::new();
        let mut cfg_file = File::open(&config_path)?; // TODO: Make async
        cfg_file.read_to_string(&mut cfg_file_buf)?;
        let file_cfg = if let toml::Value::Table(map) = toml::from_str(cfg_file_buf.as_str())
            .map_err(|e| Error::Config(format!("Could not parse config file: {}", e)))?
        {
            map
        } else {
            return Err(Error::Config(
                "Could not parse config file: Root Value not a Table.".to_string(),
            ));
        };

        // Get local socket address or default:
        let local_addrs = match file_cfg.get("bind_addresses") {
            Some(toml::Value::Array(addrs_list)) => {
                let mut local_addrs = vec![];
                for addr in addrs_list.iter() {
                    if let toml::Value::String(addr) = addr {
                        local_addrs.extend(addr.to_socket_addrs().map_err(|_| Error::Config("Could not resolve value of 'bind_address' in main section of config."
                                .to_string()))?);
                    } else {
                        return Err(Error::Config("'bind_addresses' contains a value with wrong type (expected type string).".to_string()));
                    }
                }
                local_addrs
            }
            Some(_) => {
                return Err(Error::Config(
                    "Field 'bind_addresses' has wrong type (should be of type Array).".to_string(),
                ));
            }
            None => vec!["127.0.0.1:25"
                .to_socket_addrs()
                .expect("This should always work.")
                .next()
                .unwrap()],
        };

        // Get new unix user and group:
        let effective_user = if let Some(name_val) = file_cfg.get("unix_user") {
            Some(
                get_user_by_name(name_val.as_str().ok_or_else(|| {
                    Error::Config(
                        "Value of field 'unix_user' has wrong type (expected string).".to_string(),
                    )
                })?)
                .ok_or_else(|| {
                    Error::Config("The user given by 'unix_user' does not exist.".to_string())
                })?,
            )
        } else {
            None
        };
        let effective_group = if let Some(name_val) = file_cfg.get("unix_group") {
            Some(
                get_group_by_name(name_val.as_str().ok_or_else(|| {
                    Error::Config(
                        "Value of field 'unix_group' has wrong type (expected string).".to_string(),
                    )
                })?)
                .ok_or_else(|| {
                    Error::Config("The group given by 'unix_group' does not exist.".to_string())
                })?,
            )
        } else {
            None
        };

        // Get TLS configuration:
        let tls_config = if local_addrs.iter().any(|addr| addr.port() == 465) {
            let cert_section = file_cfg
                .get("certificates")
                .ok_or_else(|| {
                    Error::Config("Missing 'certificates' section in config file.".to_string())
                })?
                .as_table()
                .ok_or_else(|| {
                    Error::Config(
                        "Wrong type of 'certificate' section in config file (expected table)."
                            .to_string(),
                    )
                })?;

            Some(TlsConfig::try_from(cert_section)?.into())
        } else {
            None
        };

        // Get default file destination base directory:
        let default_path: Option<PathBuf> = if let Some(val) = file_cfg.get("default_path") {
            Some(PathBuf::from(val.as_str().ok_or_else(|| {
                Error::Config(
                    "Value of field 'default_path' has wrong type (expected string).".to_string(),
                )
            })?))
        } else {
            None
        };

        Config {
            effective_user,
            effective_group,
            local_addrs,
            default_path,
            dest_map: HashMap::new(),
            tls_config,
        }
        .load_mapping(
            file_cfg
                .get("mappings")
                .ok_or_else(|| {
                    Error::Config("Missing 'mappings' sections in config file.".to_string())
                })?
                .as_table()
                .ok_or_else(|| {
                    Error::Config(
                        "Wrong type of 'mappings' section in config file (expected table)."
                            .to_string(),
                    )
                })?,
        )
        .await
    }

    /// Loads a destination mapping from the given mappings sections from the config file to the own field dest_map.
    async fn load_mapping(
        mut self,
        mapping_sections: &toml::map::Map<String, toml::Value>,
    ) -> Result<Self, Error> {
        for mapping_name in mapping_sections.keys() {
            let map_section = mapping_sections
                .get(mapping_name)
                .unwrap() // Cannor be None, because mapping_name name is in mapping_sections.keys().
                .as_table()
                .ok_or_else(|| {
                    Error::Config(format!(
                        "Section 'mappings.{}' has wrong type (expected table).",
                        mapping_name
                    ))
                })?;

            let addr_key = map_section
                .get("address")
                .ok_or_else(|| Error::Config(format!("Mapping {} is missing 'address' field.", mapping_name)))?
                .as_str()
                .ok_or_else(|| {
                    Error::Config(format!("Field 'address' for mapping '{mapping_name}' has wrong type (expected string)."))
                })?;

            if let Some(matrix_homeserver) = map_section.get("matrix_homeserver") {
                // Create matrix destination:

                let mut dest_builder = MatrixDestBuilder::new(
                    matrix_homeserver.as_str()
                        .ok_or_else(|| Error::Config(format!("Field 'matrix_homeserver' for mapping '{mapping_name}' has wrong type (expected string).")))?
                ).await?;
                // Set session file path, if given:
                if let Some(session_file_path) = map_section.get("matrix_session_file") {
                    dest_builder.set_session_path(
                        Path::new(
                            session_file_path.as_str()
                                .ok_or_else(|| Error::Config(format!("Field 'matrix_session_file' for mapping '{mapping_name}' has wrong type (expected string).")))?
                        )
                    );
                }
                // Set login data, if given:
                if let Some(username) = map_section.get("matrix_username") {
                    let username = username.as_str()
                        .ok_or_else(|| Error::Config(format!("Field 'matrix_username' for mapping '{mapping_name}' has wrong type (expected string).")))?;
                    let password = map_section.get("matrix_password")
                        .ok_or_else(|| Error::Config(format!("Expected a field 'matrix_password', because the field 'matrix_username' was present in mapping '{mapping_name}'.")))?
						.as_str()
                        .ok_or_else(|| Error::Config(format!("Field 'matrix_password' for mapping '{mapping_name}' has wrong type (expected string).")))?;
                    dest_builder.set_login(username, password);
                }
                // Build and insert into dest_map:
                self.dest_map.insert(
                    String::from(addr_key),
                    Box::new(dest_builder.build().await?),
                );
            } else if let Some(path) = map_section.get("dest_path") {
                // Create file destination specific to this mapping:

                let destination = FileDestination::new(
                    path.as_str()
                        .ok_or_else(|| Error::Config(format!("Field 'dest_path' for mapping '{mapping_name}' has wrong type (expected string).")))?
                )?;
                self.dest_map
                    .insert(String::from(addr_key), Box::new(destination));
            } else if let Some(ref base_path) = self.default_path {
                // Create default file destination:

                let mut path = PathBuf::from(base_path);
                path.push(&addr_key);
                self.dest_map.insert(
                    String::from(addr_key),
                    Box::new(FileDestination::new(path)?),
                );
            } else {
                return Err(Error::Config(format!(
                    "Missing destination for mapping '{mapping_name}'."
                )));
            };
        }

        Ok(self)
    }
}

// We only use this struct to circumvent rusts rules for implementing foreign traits on foreign types.
// We cannot directly implement TryFrom<toml::map::Map<String, toml::Value>> for ServerConfig.
struct TlsConfig(ServerConfig);
impl From<TlsConfig> for Arc<ServerConfig> {
    fn from(conf: TlsConfig) -> Self {
        Arc::new(conf.0)
    }
}
impl TryFrom<&toml::map::Map<String, toml::Value>> for TlsConfig {
    type Error = Error;

    fn try_from(cert_section: &toml::map::Map<String, toml::Value>) -> Result<Self, Self::Error> {
        let mut resolver = CertResolver::new();

        for domain in cert_section.keys() {
            // Get configured paths:
            let domain_cert_obj = cert_section[domain]
				.as_table()
				.ok_or_else(|| Error::Config(format!("Value for domain {} in 'certificates' section has wrong type (expected table).", domain)))?;
            let cert_file_path = domain_cert_obj
				.get("cert_file")
				.ok_or_else(|| Error::Config(format!("Missing field 'cert_file' for domain {}.", domain)))?
				.as_str()
				.ok_or_else(|| Error::Config(format!("Value for field 'cert_file' for domain {} in 'certificates' section has wrong type (expected string).", domain)))?;
            let key_file_path = domain_cert_obj
				.get("private_key_file")
				.ok_or_else(|| Error::Config(format!("Missing field 'private_key_file' for domain {}.", domain)))?
				.as_str()
				.ok_or_else(|| Error::Config(format!("Value for field 'private_key_file' for domain {} in 'certificates' section has wrong type (expected string).", domain)))?;

            // Read certificates:
            let cert_file = File::open(cert_file_path)?;
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
            let key_file = File::open(&key_file_path)?;
            let mut reader = BufReader::new(key_file);
            let priv_key_signer =
                if let Some(Item::RSAKey(raw) | Item::PKCS8Key(raw) | Item::ECKey(raw)) =
                    read_one(&mut reader)?
                {
                    rustls::sign::any_supported_type(&PrivateKey(raw)).map_err(|e| {
                        Error::Config(format!(
                            "Could not sign with private key given for domain {}: {}",
                            domain, e
                        ))
                    })?
                } else {
                    return Err(Error::Config(format!(
                        "Could not read key from {} given by 'private_key_file'.",
                        key_file_path
                    )));
                };

            resolver.add_domain(
                domain.to_string(),
                CertifiedKey::new(certs, priv_key_signer),
            );
        }

        Ok(Self(
            ServerConfig::builder()
                .with_safe_defaults()
                .with_no_client_auth()
                .with_cert_resolver(Arc::new(resolver)),
        ))
    }
}

pub(crate) struct CertResolver {
    domain_cert_map: HashMap<String, Arc<CertifiedKey>>,
}

impl CertResolver {
    fn new() -> Self {
        CertResolver {
            domain_cert_map: HashMap::new(),
        }
    }

    fn add_domain(&mut self, domain: String, cert: CertifiedKey) {
        self.domain_cert_map.insert(domain, Arc::new(cert));
    }
}

impl ResolvesServerCert for CertResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        if let Some(domain) = client_hello.server_name() {
            self.domain_cert_map.get(domain).cloned()
        } else {
            None
        }
    }
}

#[cfg(test)]
impl Default for Config {
    fn default() -> Self {
        Config {
            effective_user: None,
            effective_group: None,
            local_addrs: "127.0.0.1:25".to_socket_addrs().unwrap().collect(),
            default_path: None,
            dest_map: HashMap::new(),
            tls_config: None,
        }
    }
}
