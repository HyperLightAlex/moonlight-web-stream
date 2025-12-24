use common::config::Config;
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use std::{io::ErrorKind, path::PathBuf, str::FromStr};
use tokio::fs::{self, File};

use actix_web::{
    App as ActixApp, HttpServer,
    middleware::{self, Logger},
    web::{Data, scope},
};
use log::{Level, error, info};
use simplelog::{ColorChoice, CombinedLogger, SharedLogger, TermLogger, TerminalMode, WriteLogger};

use crate::{
    api::api_service,
    app::App,
    cli::{Cli, Command},
    human_json::preprocess_human_json,
    upnp::{UpnpManager, detect_local_ip},
    web::{web_config_js_service, web_service},
};

mod api;
mod app;
mod web;

mod cli;
mod human_json;
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

    // Initialize UPnP if enabled
    let upnp_manager = if config.upnp.enabled {
        let local_ip = detect_local_ip().unwrap_or_else(|| {
            info!("[UPnP] Could not detect local IP, using bind address");
            match bind_address {
                std::net::SocketAddr::V4(addr) => *addr.ip(),
                std::net::SocketAddr::V6(_) => std::net::Ipv4Addr::UNSPECIFIED,
            }
        });

        let server_port = bind_address.port();
        let manager = UpnpManager::new(config.upnp.clone(), server_port, local_ip);

        match manager.initialize().await {
            Ok(status) => {
                if status.available {
                    if let Some(external_ip) = status.external_ip {
                        info!(
                            "[Server] Remote access available at: {}:{}",
                            external_ip, server_port
                        );
                    }
                }
            }
            Err(e) => {
                info!("[UPnP] UPnP setup failed: {}. Remote streaming may require manual port forwarding.", e);
            }
        }

        Some(Data::new(manager))
    } else {
        None
    };

    let server = HttpServer::new({
        let url_path_prefix = config.web_server.url_path_prefix.clone();
        let app = app.clone();
        let upnp_manager = upnp_manager.clone();

        move || {
            let mut actix_app = ActixApp::new().service(
                scope(&url_path_prefix)
                    .app_data(app.clone())
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

    if let Some(certificate) = app.config().web_server.certificate.as_ref() {
        info!("[Server]: Running Https Server with ssl tls");

        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())
            .expect("failed to create ssl tls acceptor");
        builder
            .set_private_key_file(&certificate.private_key_pem, SslFiletype::PEM)
            .expect("failed to set private key");
        builder
            .set_certificate_chain_file(&certificate.certificate_pem)
            .expect("failed to set certificate");

        server.bind_openssl(bind_address, builder)?.run().await?;
    } else {
        server.bind(bind_address)?.run().await?;
    }

    Ok(())
}
