use common::config::Config;
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use std::{io::ErrorKind, net::SocketAddr, path::PathBuf, str::FromStr};
use tokio::fs::{self, File};

use actix_web::{
    App as ActixApp, HttpServer,
    middleware::{self, Logger, NormalizePath, TrailingSlash},
    web::{Data, scope},
};
use log::{Level, error, info};
use simplelog::{ColorChoice, CombinedLogger, SharedLogger, TermLogger, TerminalMode, WriteLogger};

use crate::{
    api::api_service,
    app::App,
    cli::{Cli, Command},
    human_json::preprocess_human_json,
    remote_access::RemoteAccessProvider,
    upnp::{UpnpManager, detect_local_ip},
    web::{web_config_js_service, web_service},
};

mod api;
mod app;
mod web;

mod cli;
mod human_json;
mod remote_access;
mod stun;
mod upnp;

#[actix_web::main]
async fn main() {
    let cli = Cli::load();

    // Load Config
    let config_path = PathBuf::from_str(&cli.config_path).expect("invalid config file path");
    let config = match fs::read_to_string(&config_path).await {
        Ok(mut value) => {
            value = preprocess_human_json(value);

            let mut config = serde_json::from_str(&value).expect("invalid file");
            cli.options.apply(&mut config);
            config
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            let mut new_config = Config::default();
            cli.options.apply(&mut new_config);

            let value_str =
                serde_json::to_string_pretty(&new_config).expect("failed to serialize file");

            if let Some(parent) = config_path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .expect("failed to create directories to file");
            }
            fs::write(config_path, value_str)
                .await
                .expect("failed to write default file");

            new_config
        }
        Err(err) => panic!("failed to read file: {err}"),
    };

    match cli.command {
        Some(Command::PrintConfig) => {
            let json =
                serde_json::to_string_pretty(&config).expect("failed to serialize config to json");
            println!("{json}");
            return;
        }
        None | Some(Command::Run) => {
            // Fallthrough
        }
    }

    // TODO: log config: anonymize ips when enabled in file
    // TODO: https://www.reddit.com/r/csharp/comments/166xgcl/comment/jynybpe/

    let mut log_config = simplelog::ConfigBuilder::default();

    let mut loggers: Vec<Box<dyn SharedLogger>> = vec![TermLogger::new(
        config.log.level_filter,
        log_config.build(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )];

    if let Some(file_path) = &config.log.file_path {
        if fs::try_exists(file_path)
            .await
            .expect("failed to check if log file exists")
        {
            // TODO: should we rename?
        }

        let file = File::create(file_path)
            .await
            .expect("failed to open log file");

        loggers.push(WriteLogger::new(
            config.log.level_filter,
            log_config.build(),
            file.try_into_std()
                .expect("failed to cast tokio file into std file"),
        ));
    }

    CombinedLogger::init(loggers).expect("failed to init combined logger");

    if let Err(err) = start(config).await {
        error!("{err:?}");
    }
}

async fn start(config: Config) -> Result<(), anyhow::Error> {
    let app = App::new(config.clone()).await?;
    let app = Data::new(app);

    let bind_address = app.config().web_server.bind_address;

    // When Fuji-style TLS config is set (file paths + HTTPS bind), listen HTTPS-only on bind_address_https
    let use_tls_paths = app.config().web_server.tls_cert_path.as_ref().and_then(|c| {
        app.config()
            .web_server
            .tls_key_path
            .as_ref()
            .and_then(|k| app.config().web_server.bind_address_https.as_ref().map(|b| (c, k, b)))
    });

    let (listen_address, use_https) = if let Some((cert_path, key_path, bind_https)) = use_tls_paths {
        let addr: SocketAddr = bind_https
            .parse()
            .expect("invalid bind_address_https: must be host:port (e.g. 0.0.0.0:8443)");
        info!(
            "[Server] TLS mode: listening HTTPS-only on {} (cert: {}, key: {})",
            addr, cert_path, key_path
        );
        (addr, true)
    } else {
        (bind_address, app.config().web_server.certificate.is_some())
    };

    // Initialize UPnP if enabled (use the port we're actually listening on)
    let (upnp_manager, upnp_status) = if config.upnp.enabled {
        let local_ip = detect_local_ip().unwrap_or_else(|| {
            info!("[UPnP] Could not detect local IP, using bind address");
            match listen_address {
                std::net::SocketAddr::V4(addr) => *addr.ip(),
                std::net::SocketAddr::V6(_) => std::net::Ipv4Addr::UNSPECIFIED,
            }
        });

        let server_port = listen_address.port();
        let manager = UpnpManager::new(config.upnp.clone(), server_port, local_ip);

        let status = match manager.initialize().await {
            Ok(status) => {
                if status.available {
                    if let Some(external_ip) = status.external_ip {
                        info!(
                            "[Server] Remote access available at: {}:{}",
                            external_ip, server_port
                        );
                    }
                }
                Some(status)
            }
            Err(e) => {
                info!("[UPnP] UPnP setup failed: {}. Remote streaming may require manual port forwarding.", e);
                None
            }
        };

        (Some(Data::new(manager)), status)
    } else {
        (None, None)
    };

    // Initialize remote access provider (discovers external IP, NAT type, etc.)
    let remote_access_provider = Data::new(RemoteAccessProvider::new(
        &config,
        upnp_status.as_ref(),
    ));

    let server = HttpServer::new({
        let url_path_prefix = config.web_server.url_path_prefix.clone();
        let app = app.clone();
        let upnp_manager = upnp_manager.clone();
        let remote_access_provider = remote_access_provider.clone();

        move || {
            let mut actix_app = ActixApp::new().service(
                scope(&url_path_prefix)
                    .wrap(NormalizePath::new(TrailingSlash::Trim))
                    .app_data(app.clone())
                    .app_data(remote_access_provider.clone())
                    .wrap(
                        Logger::new("%r took %D ms")
                            .log_target("http_server")
                            .log_level(Level::Debug),
                    )
                    .wrap(
                        // TODO: maybe only re cache when required?
                        middleware::DefaultHeaders::new()
                            .add((
                                "Cache-Control",
                                "no-store, no-cache, must-revalidate, private",
                            ))
                            .add(("Pragma", "no-cache"))
                            .add(("Expires", "0")),
                    )
                    .service(api_service())
                    .service(web_config_js_service())
                    .service(web_service()),
            );

            // Add UPnP manager if available
            if let Some(ref upnp) = upnp_manager {
                actix_app = actix_app.app_data(upnp.clone());
            }

            actix_app
        }
    });

    if use_https {
        info!("[Server] Running HTTPS server with TLS");

        let (cert_path, key_path) = if let Some((c, k, _)) = use_tls_paths {
            (c.clone(), k.clone())
        } else if let Some(certificate) = app.config().web_server.certificate.as_ref() {
            // Legacy: certificate holds paths in private_key_pem / certificate_pem fields
            (
                certificate.certificate_pem.clone(),
                certificate.private_key_pem.clone(),
            )
        } else {
            unreachable!("use_https but no TLS config");
        };

        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())
            .expect("failed to create SSL TLS acceptor");

        if use_tls_paths.is_some() {
            builder
                .set_private_key_file(&key_path, SslFiletype::PEM)
                .expect("failed to set private key from tls_key_path");
            builder
                .set_certificate_chain_file(&cert_path)
                .expect("failed to set certificate from tls_cert_path");
        } else {
            builder
                .set_private_key_file(&key_path, SslFiletype::PEM)
                .expect("failed to set private key");
            builder
                .set_certificate_chain_file(&cert_path)
                .expect("failed to set certificate");
        }

        server.bind_openssl(listen_address, builder)?.run().await?;
    } else {
        server.bind(listen_address)?.run().await?;
    }

    Ok(())
}
