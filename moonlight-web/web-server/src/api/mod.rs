use actix_web::{
    HttpResponse, delete,
    dev::HttpServiceFactory,
    get,
    middleware::from_fn,
    patch, post, services,
    web::{self, Bytes, Data, Json, Query},
};
use futures::future::try_join_all;
use log::warn;
use moonlight_common::PairPin;
use tokio::spawn;

use crate::{
    api::{
        admin::{add_user, delete_user, list_users, patch_user},
        auth::auth_middleware,
        response_streaming::StreamedResponse,
    },
    app::{
        App, AppError,
        host::{AppId, HostId},
        storage::StorageHostModify,
        user::{AuthenticatedUser, Role, UserId},
    },
    remote_access::RemoteAccessProvider,
};
use common::api_bindings::{
    self, DeleteHostQuery, DetailedHost, DetailedUser, GetAppImageQuery, GetAppsQuery, GetAppsResponse,
    GetHostQuery, GetHostResponse, GetHostsResponse, GetUserQuery, PatchHostRequest,
    PostHostRequest, PostHostResponse, PostPairRequest, PostPairResponse1, PostPairResponse2,
    PostWakeUpRequest, UndetailedHost,
};

pub mod admin;
pub mod auth;
pub mod input;
pub mod network;
pub mod stream;

pub mod response_streaming;

#[get("/user")]
async fn get_user(
    app: Data<App>,
    mut user: AuthenticatedUser,
    Query(query): Query<GetUserQuery>,
) -> Result<Json<DetailedUser>, AppError> {
    match (query.name, query.user_id) {
        (None, None) => {
            let detailed_user = user.detailed_user().await?;

            Ok(Json(detailed_user))
        }
        (None, Some(user_id)) => {
            let target_user_id = UserId(user_id);

            let mut target_user = app.user_by_id(target_user_id).await?;

            let detailed_user = target_user.detailed_user(&mut user).await?;

            Ok(Json(detailed_user))
        }
        (Some(name), None) => {
            let mut target_user = app.user_by_name(&name).await?;

            let detailed_user = target_user.detailed_user(&mut user).await?;

            Ok(Json(detailed_user))
        }
        (Some(_), Some(_)) => Err(AppError::BadRequest),
    }
}

#[get("/hosts")]
async fn list_hosts(
    mut user: AuthenticatedUser,
) -> Result<StreamedResponse<GetHostsResponse, UndetailedHost>, AppError> {
    let (mut stream_response, stream_sender) =
        StreamedResponse::new(GetHostsResponse { hosts: Vec::new() });

    let hosts = user.hosts().await?;

    // Try join all because storage should always work, the actual host info will be send using response streaming
    let undetailed_hosts = try_join_all(hosts.into_iter().map(move |mut host| {
        let mut user = user.clone();
        let stream_sender = stream_sender.clone();

        async move {
            // First query db
            let undetailed_cache = host.undetailed_host_cached(&mut user).await;

            // Then send http request now
            let mut user = user.clone();

            spawn(async move {
                let undetailed = match host.undetailed_host(&mut user).await {
                    Ok(value) => value,
                    Err(err) => {
                        warn!("Failed to get undetailed host of {host:?}: {err:?}");
                        return;
                    }
                };

                if let Err(err) = stream_sender.send(undetailed).await {
                    warn!(
                        "Failed to send back undetailed host data using response streaming: {err:?}"
                    );
                }
            });

            undetailed_cache
        }
    }))
    .await?;

    stream_response.set_initial(GetHostsResponse {
        hosts: undetailed_hosts,
    });

    Ok(stream_response)
}

/// Attach remote access info to a DetailedHost if available.
fn attach_remote_access(
    mut host: DetailedHost,
    remote_provider: &RemoteAccessProvider,
) -> DetailedHost {
    host.remote_access = remote_provider.get_info();
    if let Some(ref ra) = host.remote_access {
        log::info!("[Remote] Attaching remote_access to host response: external_ip={:?}, port={}, nat_type={}", 
            ra.external_ip, ra.port, ra.nat_type);
    } else {
        log::info!("[Remote] No remote_access info available to attach");
    }
    host
}

#[get("/host")]
async fn get_host(
    mut user: AuthenticatedUser,
    Query(query): Query<GetHostQuery>,
    remote_provider: Data<RemoteAccessProvider>,
) -> Result<Json<GetHostResponse>, AppError> {
    let host_id = HostId(query.host_id);

    let mut host = user.host(host_id).await?;

    let detailed = host.detailed_host(&mut user).await?;
    let detailed = attach_remote_access(detailed, &remote_provider);

    Ok(Json(GetHostResponse { host: detailed }))
}

#[post("/host")]
async fn post_host(
    app: Data<App>,
    mut user: AuthenticatedUser,
    Json(request): Json<PostHostRequest>,
    remote_provider: Data<RemoteAccessProvider>,
) -> Result<Json<PostHostResponse>, AppError> {
    let mut host = user
        .host_add(
            request.address,
            request
                .http_port
                .unwrap_or(app.config().moonlight.default_http_port),
        )
        .await?;

    let detailed = host.detailed_host(&mut user).await?;
    let detailed = attach_remote_access(detailed, &remote_provider);

    Ok(Json(PostHostResponse { host: detailed }))
}

#[patch("/host")]
async fn patch_host(
    mut user: AuthenticatedUser,
    Json(request): Json<PatchHostRequest>,
) -> Result<HttpResponse, AppError> {
    let host_id = HostId(request.host_id);

    let mut host = user.host(host_id).await?;

    let mut modify = StorageHostModify::default();

    let role = user.role().await?;
    if request.change_owner {
        match role {
            Role::Admin => {
                modify.owner = Some(request.owner.map(UserId));
            }
            Role::User => {
                return Err(AppError::Forbidden);
            }
        }
    }

    host.modify(&mut user, modify).await?;

    Ok(HttpResponse::Ok().finish())
}

#[delete("/host")]
async fn delete_host(
    mut user: AuthenticatedUser,
    Query(query): Query<DeleteHostQuery>,
) -> Result<HttpResponse, AppError> {
    let host_id = HostId(query.host_id);

    user.host_delete(host_id).await?;

    Ok(HttpResponse::Ok().finish())
}

#[post("/pair")]
async fn pair_host(
    mut user: AuthenticatedUser,
    Json(request): Json<PostPairRequest>,
    remote_provider: Data<RemoteAccessProvider>,
) -> Result<StreamedResponse<PostPairResponse1, PostPairResponse2>, AppError> {
    use common::api_bindings::HostType;

    let host_id = HostId(request.host_id);

    let mut host = user.host(host_id).await?;

    // Detect if this is a Backlight host
    let host_type = host.detect_host_type(&mut user).await.unwrap_or(HostType::Standard);

    // Clone remote access info for use in spawned tasks
    let remote_access_info = remote_provider.get_info();

    match host_type {
        HostType::Backlight => {
            // Backlight host: auto-pair using OTP (no PIN needed)
            let (stream_response, stream_sender) =
                StreamedResponse::new(PostPairResponse1::BacklightAutoPairing);

            let remote_info = remote_access_info.clone();
            spawn(async move {
                let result = host.pair_fuji(&mut user).await;

                let result = match result {
                    Ok(()) => host.detailed_host(&mut user).await,
                    Err(err) => Err(err),
                };

                match result {
                    Ok(mut detailed_host) => {
                        // Attach remote access info for client to use
                        detailed_host.remote_access = remote_info;
                        if let Err(err) = stream_sender
                            .send(PostPairResponse2::Paired(detailed_host))
                            .await
                        {
                            warn!("Failed to send Backlight pair success: {err:?}");
                        }
                    }
                    Err(err) => {
                        warn!("Failed to Backlight auto-pair host: {err}");
                        if let Err(err) = stream_sender.send(PostPairResponse2::PairError).await {
                            warn!("Failed to send Backlight pair failure: {err:?}");
                        }
                    }
                }
            });

            Ok(stream_response)
        }
        HostType::Standard => {
            // Standard Sunshine: use PIN-based pairing
            let pin = PairPin::generate()?;

            let (stream_response, stream_sender) =
                StreamedResponse::new(PostPairResponse1::Pin(pin.to_string()));

            let remote_info = remote_access_info;
            spawn(async move {
                let result = host.pair(&mut user, pin).await;

                let result = match result {
                    Ok(()) => host.detailed_host(&mut user).await,
                    Err(err) => Err(err),
                };

                match result {
                    Ok(mut detailed_host) => {
                        // Attach remote access info for client to use
                        detailed_host.remote_access = remote_info;
                        if let Err(err) = stream_sender
                            .send(PostPairResponse2::Paired(detailed_host))
                            .await
                        {
                            warn!("Failed to send pair success: {err:?}");
                        }
                    }
                    Err(err) => {
                        warn!("Failed to pair host: {err}");
                        if let Err(err) = stream_sender.send(PostPairResponse2::PairError).await {
                            warn!("Failed to send pair failure: {err:?}");
                        }
                    }
                }
            });

            Ok(stream_response)
        }
    }
}

#[post("/host/wake")]
async fn wake_host(
    mut user: AuthenticatedUser,
    Json(request): Json<PostWakeUpRequest>,
) -> Result<HttpResponse, AppError> {
    let host_id = HostId(request.host_id);

    let host = user.host(host_id).await?;

    host.wake(&mut user).await?;

    Ok(HttpResponse::Ok().finish())
}

#[get("/apps")]
async fn get_apps(
    mut user: AuthenticatedUser,
    Query(query): Query<GetAppsQuery>,
) -> Result<Json<GetAppsResponse>, AppError> {
    use crate::app::fuji_internal::{fuji_client, is_embedded_in_fuji};
    use log::info;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let host_id = HostId(query.host_id);

    // When embedded in Fuji, return Fuji's game library (not Sunshine's app list)
    // Fuji has the full scanned game library from all platforms
    if is_embedded_in_fuji().await {
        info!("[Apps]: Embedded in Fuji, getting games from Fuji's library");
        
        match fuji_client().get_games(None, None).await {
            Ok(fuji_games) => {
                info!("[Apps]: Got {} games from Fuji", fuji_games.games.len());
                return Ok(Json(GetAppsResponse {
                    apps: fuji_games.games
                        .into_iter()
                        .map(|game| {
                            // Generate a consistent numeric app_id from the game title
                            // This is used by the client to request streams
                            // The streaming endpoint will match back by title
                            let mut hasher = DefaultHasher::new();
                            game.title.hash(&mut hasher);
                            let app_id = (hasher.finish() & 0x7FFFFFFF) as u32; // Keep positive
                            
                            api_bindings::App {
                                app_id,
                                title: game.title,
                                is_hdr_supported: false, // Fuji doesn't track HDR per-game
                            }
                        })
                        .collect(),
                }));
            }
            Err(e) => {
                warn!("[Apps]: Failed to get games from Fuji: {:?}, falling back to Sunshine", e);
                // Fall through to direct Sunshine query
            }
        }
    }

    // Fallback: query Sunshine directly (non-Fuji mode)
    let mut host = user.host(host_id).await?;
    let apps = host.list_apps(&mut user).await?;

    Ok(Json(GetAppsResponse {
        apps: apps
            .into_iter()
            .map(|app| api_bindings::App {
                app_id: app.id.0,
                title: app.title,
                is_hdr_supported: app.is_hdr_supported,
            })
            .collect(),
    }))
}

#[get("/app/image")]
async fn get_app_image(
    mut user: AuthenticatedUser,
    Query(query): Query<GetAppImageQuery>,
) -> Result<Bytes, AppError> {
    use crate::app::fuji_internal::{fuji_client, is_embedded_in_fuji};
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let host_id = HostId(query.host_id);
    let app_id = AppId(query.app_id);

    // When embedded in Fuji, get box art from Fuji's game library
    if is_embedded_in_fuji().await {
        // Find the game by matching the hashed app_id
        if let Ok(fuji_games) = fuji_client().get_games(None, None).await {
            let found_game = fuji_games.games.into_iter().find(|game| {
                let mut hasher = DefaultHasher::new();
                game.title.hash(&mut hasher);
                let hashed_id = (hasher.finish() & 0x7FFFFFFF) as u32;
                hashed_id == app_id.0
            });

            if let Some(game) = found_game {
                // Get cover from Fuji
                match fuji_client().get_game_cover(&game.id, Some("medium")).await {
                    Ok(image_bytes) => {
                        return Ok(Bytes::from(image_bytes));
                    }
                    Err(e) => {
                        warn!("[AppImage]: Failed to get cover from Fuji for '{}': {:?}", game.title, e);
                        // Fall through to Sunshine lookup
                    }
                }
            }
        }
    }

    // Fallback: get from Sunshine (non-Fuji mode or Fuji lookup failed)
    let mut host = user.host(host_id).await?;

    let image = host
        .app_image(&mut user, app_id, query.force_refresh)
        .await?;

    Ok(image)
}

pub fn api_service() -> impl HttpServiceFactory {
    web::scope("/api")
        .wrap(from_fn(auth_middleware))
        .service(services![
            // -- Auth
            auth::login,
            auth::logout,
            auth::authenticate
        ])
        .service(services![
            // -- Host
            get_user,
            list_hosts,
            get_host,
            post_host,
            patch_host,
            wake_host,
            delete_host,
            pair_host,
            get_apps,
            get_app_image,
        ])
        .service(services![
            // -- Stream
            stream::start_host,
            stream::cancel_host,
            stream::get_session,
            stream::pause_session,
            stream::end_session,
            // -- Input (hybrid mode)
            input::input_connect,
        ])
        .service(services![
            // -- Admin
            add_user,
            patch_user,
            delete_user,
            list_users
        ])
        .service(services![
            // -- Network
            network::get_network_status,
        ])
}
