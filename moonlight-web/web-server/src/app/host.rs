use std::{
    fmt::{Debug, Formatter},
    str::FromStr,
};

use actix_web::web::Bytes;
use common::api_bindings::{self, DetailedHost, HostOwner, HostState, HostType, PairStatus, UndetailedHost};
use log::{debug, warn};
use moonlight_common::{
    PairPin, ServerState,
    high::broadcast_magic_packet,
    network::{
        self, ApiError, ClientAppBoxArtRequest, ClientInfo, HostInfo, host_app_box_art,
        host_app_list, host_cancel, host_info,
        request_client::{RequestClient, RequestError},
    },
    pair::{PairSuccess, generate_new_client, host_pair, host_pair_with_otp, OtpCredentials},
};
use uuid::Uuid;

use crate::app::{
    AppError, AppInner, AppRef, MoonlightClient,
    fuji::request_fuji_otp,
    storage::{StorageHost, StorageHostModify, StorageHostPairInfo},
    user::{AuthenticatedUser, Role, UserId},
};

/// Parse a 4-digit PIN string (e.g., "1234") into a PairPin
fn parse_pin_string(pin_str: &str) -> Option<PairPin> {
    let digits: Vec<u8> = pin_str
        .chars()
        .filter_map(|c| c.to_digit(10).map(|d| d as u8))
        .collect();

    if digits.len() == 4 {
        PairPin::from_array([digits[0], digits[1], digits[2], digits[3]])
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostId(pub u32);

pub struct Host {
    pub(super) app: AppRef,
    pub(super) id: HostId,
    pub(super) cache_storage: Option<StorageHost>,
    pub(super) cache_host_info: Option<(UserId, HostInfo)>,
}

impl Debug for Host {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppId(pub u32);

pub struct App {
    pub id: AppId,
    pub title: String,
    pub is_hdr_supported: bool,
}

impl From<network::App> for App {
    fn from(value: network::App) -> Self {
        Self {
            id: AppId(value.id),
            title: value.title,
            is_hdr_supported: value.is_hdr_supported,
        }
    }
}
impl From<App> for api_bindings::App {
    fn from(value: App) -> Self {
        Self {
            app_id: value.id.0,
            title: value.title,
            is_hdr_supported: value.is_hdr_supported,
        }
    }
}

impl Host {
    pub fn id(&self) -> HostId {
        self.id
    }

    async fn can_use(&self, user: &mut AuthenticatedUser) -> Result<(), AppError> {
        let owner = self.owner().await?;
        if owner.is_none() || owner == Some(user.id()) || matches!(user.role().await?, Role::Admin)
        {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }

    pub async fn modify(
        &mut self,
        user: &mut AuthenticatedUser,
        modify: StorageHostModify,
    ) -> Result<(), AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        self.cache_storage = None;

        app.storage.modify_host(self.id, modify).await?;

        Ok(())
    }

    pub async fn owner(&self) -> Result<Option<UserId>, AppError> {
        let app = self.app.access()?;

        let host = self.storage_host(&app).await?;

        Ok(host.owner)
    }
    async fn owner_info(
        &self,
        user: &AuthenticatedUser,
        this: &StorageHost,
    ) -> Result<HostOwner, AppError> {
        Ok(match this.owner {
            None => HostOwner::Global,
            Some(user_id) if user.id() == user_id => HostOwner::ThisUser,
            _ => unreachable!(),
        })
    }

    pub async fn undetailed_host_cached(
        &self,
        user: &mut AuthenticatedUser,
    ) -> Result<UndetailedHost, AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        let storage = self.storage_host(&app).await?;
        let owner = self.owner_info(user, &storage).await?;

        Ok(UndetailedHost {
            host_id: storage.id.0,
            name: storage.cache.name,
            owner,
            paired: if storage.pair_info.is_some() {
                PairStatus::Paired
            } else {
                PairStatus::NotPaired
            },
            server_state: None,
        })
    }

    async fn use_client<R>(
        &mut self,
        app: &AppInner,
        user: &mut AuthenticatedUser,
        pairing: bool,
        // app, https_capable, client, host, port, client_info
        f: impl AsyncFnOnce(&mut Self, bool, &mut MoonlightClient, &str, u16, ClientInfo) -> R,
    ) -> Result<R, AppError> {
        let user_unique_id = user.host_unique_id().await?;
        let host_data = self.storage_host(app).await?;

        let (mut client, https_capable) = if pairing {
            (
                MoonlightClient::with_defaults_long_timeout().map_err(ApiError::RequestClient)?,
                false,
            )
        } else if let Some(pair_info) = host_data.pair_info {
            (
                MoonlightClient::with_certificates(
                    &pair_info.client_private_key,
                    &pair_info.client_certificate,
                    &pair_info.server_certificate,
                )
                .map_err(ApiError::RequestClient)?,
                true,
            )
        } else {
            (
                MoonlightClient::with_defaults().map_err(ApiError::RequestClient)?,
                false,
            )
        };

        let info = ClientInfo {
            unique_id: &user_unique_id,
            uuid: Uuid::new_v4(),
        };

        Ok(f(
            self,
            https_capable,
            &mut client,
            &host_data.address,
            host_data.http_port,
            info,
        )
        .await)
    }
    fn build_hostport(host: &str, port: u16) -> String {
        format!("{host}:{port}")
    }

    async fn storage_host(&self, app: &AppInner) -> Result<StorageHost, AppError> {
        if let Some(host) = self.cache_storage.as_ref() {
            return Ok(host.clone());
        }

        app.storage.get_host(self.id).await
    }

    pub async fn address_port(
        &self,
        user: &mut AuthenticatedUser,
    ) -> Result<(String, u16), AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        let host = app.storage.get_host(self.id).await?;

        Ok((host.address, host.http_port))
    }

    pub async fn pair_info(
        &self,
        user: &mut AuthenticatedUser,
    ) -> Result<StorageHostPairInfo, AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        let host = app.storage.get_host(self.id).await?;

        host.pair_info.ok_or(AppError::HostNotPaired)
    }

    fn is_offline<T>(
        &self,
        result: Result<T, ApiError<<MoonlightClient as RequestClient>::Error>>,
    ) -> Result<Option<T>, AppError> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(ApiError::RequestClient(err)) if err.is_connect() => Ok(None),
            Err(err) => Err(AppError::MoonlightApi(err)),
        }
    }
    // None = Offline
    async fn host_info(
        &mut self,
        app: &AppInner,
        user: &mut AuthenticatedUser,
    ) -> Result<Option<HostInfo>, AppError> {
        let user_id = user.id();

        if let Some((cache_user_id, cache)) = self.cache_host_info.as_ref()
            && *cache_user_id == user_id
        {
            return Ok(Some(cache.clone()));
        }

        self.use_client(
            app,
            user,
            false,
            async |this, https_capable, client, host, port, client_info| {
                let mut info = match this.is_offline(
                    host_info(
                        client,
                        false,
                        &Self::build_hostport(host, port),
                        Some(client_info),
                    )
                    .await,
                ) {
                    Ok(Some(value)) => value,
                    err => return err,
                };

                if https_capable {
                    match host_info(
                        client,
                        true,
                        &Self::build_hostport(host, info.https_port),
                        Some(client_info),
                    )
                    .await
                    {
                        Ok(new_info) => {
                            info = new_info;
                        }
                        Err(ApiError::InvalidXmlStatusCode { message: Some(message) })
                            if message.contains("Certificate") =>
                        {
                            // The host likely removed our paired certificate
                            warn!("Host {this:?} has an error related to certificates. This likely happened because the device was removed from sunshine.");
                        }
                        Err(ApiError::RequestClient(err)) if err.is_encryption()  => {
                            // The host likely removed our paired certificate
                            warn!("Host {this:?} has an error related to certificates. This likely happened because the device was removed from sunshine.");
                        }
                        Err(err) => return Err(err.into()),
                    }
                }

                this.cache_host_info = Some((user_id, info.clone()));

                Ok(Some(info))
            },
        )
        .await?
    }

    pub async fn undetailed_host(
        &mut self,
        user: &mut AuthenticatedUser,
    ) -> Result<UndetailedHost, AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        let storage = self.storage_host(&app).await?;
        let owner = self.owner_info(user, &storage).await?;

        match self.host_info(&app, user).await {
            Ok(Some(info)) => {
                let server_state = match ServerState::from_str(&info.state_string) {
                    Ok(state) => Some(state),
                    Err(err) => {
                        warn!(
                            "failed to parse server state of host {self:?}: {:?}, {}",
                            err, info.state_string
                        );

                        None
                    }
                };

                Ok(UndetailedHost {
                    host_id: self.id.0,
                    name: info.host_name,
                    owner,
                    paired: info.pair_status.into(),
                    server_state: server_state.map(HostState::from),
                })
            }
            Ok(None) => {
                let host = self.storage_host(&app).await?;

                let paired = if host.pair_info.is_some() {
                    PairStatus::Paired
                } else {
                    PairStatus::NotPaired
                };

                Ok(UndetailedHost {
                    host_id: self.id.0,
                    name: host.cache.name,
                    owner,
                    paired,
                    server_state: None,
                })
            }
            Err(err) => Err(err),
        }
    }
    pub async fn detailed_host(
        &mut self,
        user: &mut AuthenticatedUser,
    ) -> Result<DetailedHost, AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        let storage = self.storage_host(&app).await?;

        let owner = self.owner_info(user, &storage).await?;

        match self.host_info(&app, user).await {
            Ok(Some(info)) => {
                let server_state = match ServerState::from_str(&info.state_string) {
                    Ok(state) => Some(state),
                    Err(err) => {
                        warn!(
                            "failed to parse server state of host {self:?}: {:?}, {}",
                            err, info.state_string
                        );

                        None
                    }
                };

                // Detect host type (Backlight vs Standard Sunshine) from serverinfo tags
                let host_type = if info.is_backlight {
                    Some(HostType::Backlight)
                } else {
                    Some(HostType::Standard)
                };

                Ok(DetailedHost {
                    host_id: self.id.0,
                    owner,
                    name: info.host_name,
                    paired: info.pair_status.into(),
                    server_state: server_state.map(HostState::from),
                    host_type,
                    address: storage.address,
                    http_port: storage.http_port,
                    https_port: info.https_port,
                    external_port: info.external_port,
                    version: info.app_version.to_string(),
                    gfe_version: info.gfe_version,
                    unique_id: info.unique_id.to_string(),
                    mac: info.mac.map(|mac| mac.to_string()),
                    local_ip: info.local_ip,
                    current_game: info.current_game,
                    max_luma_pixels_hevc: info.max_luma_pixels_hevc,
                    server_codec_mode_support: info.server_codec_mode_support,
                    remote_access: None, // Populated by API layer
                })
            }
            Ok(None) => {
                let paired = if storage.pair_info.is_some() {
                    PairStatus::Paired
                } else {
                    PairStatus::NotPaired
                };

                Ok(DetailedHost {
                    host_id: self.id.0,
                    owner,
                    name: storage.cache.name,
                    paired,
                    server_state: None,
                    host_type: None,
                    address: storage.address,
                    http_port: storage.http_port,
                    https_port: 0,
                    external_port: 0,
                    version: "Offline".to_string(),
                    gfe_version: "Offline".to_string(),
                    unique_id: "Offline".to_string(),
                    mac: storage.cache.mac.map(|mac| mac.to_string()),
                    local_ip: "Offline".to_string(),
                    current_game: 0,
                    max_luma_pixels_hevc: 0,
                    server_codec_mode_support: 0,
                    remote_access: None, // Populated by API layer
                })
            }
            Err(err) => Err(err),
        }
    }

    pub async fn is_paired(
        &mut self,
        user: &mut AuthenticatedUser,
    ) -> Result<PairStatus, AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        match self.host_info(&app, user).await? {
            Some(info) => Ok(info.pair_status.into()),
            None => Ok(PairStatus::NotPaired),
        }
    }

    pub async fn pair(
        &mut self,
        user: &mut AuthenticatedUser,
        pin: PairPin,
    ) -> Result<(), AppError> {
        self.can_use(user).await?;

        let user_id = user.id();
        let app = self.app.access()?;

        let info = self
            .host_info(&app, user)
            .await?
            .ok_or(AppError::HostNotFound)?;

        if matches!(info.pair_status.into(), PairStatus::Paired) {
            return Err(AppError::HostPaired);
        }

        let modify = self
            .use_client(
                &app,
                user,
                true,
                async |this,_https_capable, client, host, port, client_info| {
                    let auth = generate_new_client()?;

                    let https_address = Self::build_hostport(host, info.https_port);

                    let PairSuccess { server_certificate, mut client } =host_pair(
                        client,
                        &Self::build_hostport(host, port),
                        &https_address,
                        client_info,
                        &auth.private_key,
                        &auth.certificate,
                        &app.config.moonlight.pair_device_name,
                        info.app_version,
                        pin,
                    )
                    .await?;


                    // Store pair info
                    let (name, mac) = match host_info(
                        &mut client,
                        true,
                        &Self::build_hostport(host, info.https_port),
                        Some(client_info),
                    )
                    .await
                    {
                        Ok(info) => {
                            this.cache_host_info = Some((user_id, info.clone()));

                            (Some(info.host_name), Some(info.mac))
                        },
                        Err(err) => {
                            warn!("Failed to make https request to host {this:?} after pairing completed: {err}");
                            (None, None)
                        },
                    };

                    Ok::<_, AppError>(StorageHostModify {
                        pair_info: Some(Some(StorageHostPairInfo {
                            client_private_key: auth.private_key,
                            client_certificate: auth.certificate,
                            server_certificate,
                        })),
                        cache_name: name,
                        cache_mac: mac,
                        ..Default::default()
                    })
                },
            )
            .await??;

        self.modify(user, modify).await
    }

    pub async fn unpair(&self, user: &mut AuthenticatedUser) -> Result<Host, AppError> {
        self.can_use(user).await?;

        todo!()
    }

    /// Detect if this host is a Backlight host (supports OTP auto-pairing)
    pub async fn detect_host_type(&mut self, user: &mut AuthenticatedUser) -> Result<HostType, AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;
        let info = self
            .host_info(&app, user)
            .await?
            .ok_or(AppError::HostOffline)?;

        debug!("Detecting host type, is_backlight={}", info.is_backlight);

        if info.is_backlight {
            debug!("Host detected as Backlight");
            Ok(HostType::Backlight)
        } else {
            debug!("Host detected as Standard Sunshine");
            Ok(HostType::Standard)
        }
    }

    /// Auto-pair with a Backlight host using OTP
    ///
    /// This requests an OTP from the Backlight host and uses it to complete pairing
    /// without requiring manual PIN entry.
    pub async fn pair_fuji(&mut self, user: &mut AuthenticatedUser) -> Result<(), AppError> {
        self.can_use(user).await?;

        let user_id = user.id();
        let app = self.app.access()?;

        let info = self
            .host_info(&app, user)
            .await?
            .ok_or(AppError::HostNotFound)?;

        if matches!(info.pair_status.into(), PairStatus::Paired) {
            return Err(AppError::HostPaired);
        }

        let storage = self.storage_host(&app).await?;
        
        // Backlight OTP endpoint is on HTTP port + 1 (e.g., 48989 + 1 = 48990)
        let otp_port = storage.http_port + 1;
        let otp_hostport = Self::build_hostport(&storage.address, otp_port);
        debug!("Requesting OTP from Backlight at: {}", otp_hostport);

        // Request OTP from Backlight
        let passphrase = Uuid::new_v4().to_string();
        let device_name = &app.config.moonlight.pair_device_name;

        let otp = request_fuji_otp(&otp_hostport, &passphrase, device_name)
            .await
            .map_err(|e| {
                warn!("Failed to request Backlight OTP: {e}");
                AppError::FujiPairingFailed(format!("OTP request failed: {e}"))
            })?;

        // Store the PIN string for API submission
        let pin_string = otp.pin.clone();

        // Parse the OTP PIN as a PairPin (expects 4 digit string like "1234")
        let pin = parse_pin_string(&otp.pin).ok_or_else(|| {
            warn!("Invalid OTP PIN format from Backlight: {}", otp.pin);
            AppError::FujiPairingFailed(format!("Invalid PIN format: {}", otp.pin))
        })?;

        debug!("Received Backlight OTP (PIN: {}), proceeding with auto-pairing", pin_string);
        log::info!("Backlight auto-pairing: PIN={}, passphrase={}", pin_string, passphrase);

        // Now use the OTP pairing flow which uses /autopair endpoint with otpauth hash
        let modify = self
            .use_client(
                &app,
                user,
                true,
                async |this, _https_capable, client, host, port, client_info| {
                    let auth = generate_new_client()?;

                    let https_address = Self::build_hostport(host, info.https_port);
                    let http_address = Self::build_hostport(host, port);
                    
                    log::info!("Backlight pairing to http={} https={}", http_address, https_address);

                    // Use OTP credentials for auto-pairing
                    let otp_creds = OtpCredentials {
                        pin: &pin_string,
                        passphrase: &passphrase,
                    };

                    log::info!("Calling host_pair_with_otp with OTP credentials...");
                    let result = host_pair_with_otp(
                        client,
                        &http_address,
                        &https_address,
                        client_info,
                        &auth.private_key,
                        &auth.certificate,
                        &app.config.moonlight.pair_device_name,
                        info.app_version,
                        pin,
                        Some(otp_creds),
                    )
                    .await;
                    
                    let PairSuccess { server_certificate, mut client } = match result {
                        Ok(success) => {
                            log::info!("Backlight pairing succeeded!");
                            success
                        }
                        Err(e) => {
                            log::error!("Backlight pairing failed: {:?}", e);
                            return Err(e.into());
                        }
                    };

                    // Store pair info
                    let (name, mac) = match host_info(
                        &mut client,
                        true,
                        &Self::build_hostport(host, info.https_port),
                        Some(client_info),
                    )
                    .await
                    {
                        Ok(info) => {
                            this.cache_host_info = Some((user_id, info.clone()));
                            (Some(info.host_name), Some(info.mac))
                        }
                        Err(err) => {
                            warn!("Failed to make https request to host {this:?} after Backlight pairing completed: {err}");
                            (None, None)
                        }
                    };

                    Ok::<_, AppError>(StorageHostModify {
                        pair_info: Some(Some(StorageHostPairInfo {
                            client_private_key: auth.private_key,
                            client_certificate: auth.certificate,
                            server_certificate,
                        })),
                        cache_name: name,
                        cache_mac: mac,
                        ..Default::default()
                    })
                },
            )
            .await??;

        self.modify(user, modify).await
    }

    pub async fn wake(&self, user: &mut AuthenticatedUser) -> Result<(), AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        let storage = self.storage_host(&app).await?;

        if let Some(mac) = storage.cache.mac {
            broadcast_magic_packet(mac).await?;
            Ok(())
        } else {
            Err(AppError::HostNotFound)
        }
    }

    pub async fn list_apps(&mut self, user: &mut AuthenticatedUser) -> Result<Vec<App>, AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        let info = self
            .host_info(&app, user)
            .await?
            .ok_or(AppError::HostOffline)?;

        self.use_client(
            &app,
            user,
            false,
            async |_this, https_capable, client, host, _port, client_info| {
                if !https_capable {
                    return Err(AppError::HostNotPaired);
                }

                let apps = host_app_list(
                    client,
                    &Self::build_hostport(host, info.https_port),
                    client_info,
                )
                .await?;

                let apps = apps.apps.into_iter().map(App::from).collect::<Vec<_>>();

                Ok(apps)
            },
        )
        .await?
    }
    pub async fn app_image(
        &mut self,
        user: &mut AuthenticatedUser,
        app_id: AppId,
        force_refresh: bool,
    ) -> Result<Bytes, AppError> {
        use crate::app::fuji_internal::{fuji_client, is_embedded_in_fuji};
        use log::debug;

        self.can_use(user).await?;

        // TODO: apollo doesn't like this for some reason
        let app = self.app.access()?;

        let info = self
            .host_info(&app, user)
            .await?
            .ok_or(AppError::HostOffline)?;

        let cache_key = (user.id(), self.id, app_id);
        if !force_refresh {
            {
                let app_images = app.app_image_cache.read().await;
                if let Some(app_image) = app_images.get(&cache_key) {
                    return Ok(app_image.clone());
                }
            }
        }

        // Try Fuji's internal API first when embedded in Fuji
        // This provides access to IGDB-enriched cover art
        if is_embedded_in_fuji().await {
            debug!("Embedded in Fuji, trying internal API for app image");

            // First get the app list to find the app name for this app_id
            let apps_result = self
                .use_client(
                    &app,
                    user,
                    false,
                    async |_this, https_capable, client, host, _port, client_info| {
                        if !https_capable {
                            return Err(AppError::HostNotPaired);
                        }

                        let apps = host_app_list(
                            client,
                            &Self::build_hostport(host, info.https_port),
                            client_info,
                        )
                        .await?;

                        Ok(apps)
                    },
                )
                .await?;

            if let Ok(apps) = apps_result {
                // Find the app by ID to get its name
                if let Some(sunshine_app) = apps.apps.iter().find(|a| a.id == app_id.0) {
                    let app_name = &sunshine_app.name;
                    debug!("Found app name '{}' for app_id {}", app_name, app_id.0);

                    // Get games from Fuji and find by name
                    if let Ok(fuji_games) = fuji_client().get_games(None, None).await {
                        // Find matching game by name (case-insensitive)
                        let matching_game = fuji_games.games.iter().find(|g| {
                            g.title.to_lowercase() == app_name.to_lowercase()
                        });

                        if let Some(game) = matching_game {
                            debug!("Found matching Fuji game: {} -> {}", app_name, game.id);

                            // Fetch cover from Fuji's internal API
                            if let Ok(cover_bytes) = fuji_client().get_game_cover(&game.id, Some("large")).await {
                                let app_image = Bytes::from(cover_bytes);

                                // Cache it
                                {
                                    let mut app_images = app.app_image_cache.write().await;
                                    app_images.insert(cache_key, app_image.clone());
                                }

                                return Ok(app_image);
                            } else {
                                debug!("Fuji internal API returned no cover for game {}", game.id);
                            }
                        } else {
                            debug!("No matching Fuji game found for '{}'", app_name);
                        }
                    }
                }
            }

            debug!("Fuji internal API didn't provide cover, falling back to Sunshine");
        }

        // Fall back to Sunshine's box art endpoint
        let app_image = self
            .use_client(
                &app,
                user,
                false,
                async |_this, https_capable, client, host, _port, client_info| {
                    if !https_capable {
                        return Err(AppError::HostNotPaired);
                    }

                    let image = host_app_box_art(
                        client,
                        &Self::build_hostport(host, info.https_port),
                        client_info,
                        ClientAppBoxArtRequest { app_id: app_id.0 },
                    )
                    .await?;

                    Ok(image)
                },
            )
            .await??;
        let app_image = Bytes::from_owner(app_image);

        {
            let mut app_images = app.app_image_cache.write().await;
            app_images.insert(cache_key, app_image.clone());
        }

        Ok(app_image)
    }

    pub async fn cancel_app(&mut self, user: &mut AuthenticatedUser) -> Result<bool, AppError> {
        self.can_use(user).await?;

        let app = self.app.access()?;

        let info = self
            .host_info(&app, user)
            .await?
            .ok_or(AppError::HostOffline)?;

        self.use_client(
            &app,
            user,
            false,
            async |_this, https_capable, client, host, _port, client_info| {
                if !https_capable {
                    return Err(AppError::Forbidden);
                }

                let success = host_cancel(
                    client,
                    &Self::build_hostport(host, info.https_port),
                    client_info,
                )
                .await?;

                Ok(success)
            },
        )
        .await?
    }

    pub async fn delete(self, user: &mut AuthenticatedUser) -> Result<(), AppError> {
        let app = self.app.access()?;

        let host = app.storage.get_host(self.id).await?;

        if host.owner == Some(user.id()) || matches!(user.role().await?, Role::Admin) {
            {
                let mut app_images = app.app_image_cache.write().await;
                app_images.retain(|(_, host_id, _), _| *host_id != self.id);
            }

            drop(app);
            self.delete_no_auth().await
        } else {
            Err(AppError::Forbidden)
        }
    }
    pub async fn delete_no_auth(self) -> Result<(), AppError> {
        let app = self.app.access()?;

        app.storage.remove_host(self.id).await?;

        Ok(())
    }
}
