use log::warn;
use moonlight_common::stream::bindings::{Colorspace, SupportedVideoFormats};
use serde::{Deserialize, Serialize};

pub mod api_bindings;
pub mod api_bindings_consts;
pub mod config;
pub mod ipc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSettings {
    pub bitrate: u32,
    pub packet_size: u32,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    pub video_frame_queue_size: u32,
    pub audio_sample_queue_size: u32,
    pub play_audio_local: bool,
    pub video_supported_formats: SupportedVideoFormats,
    pub video_colorspace: Colorspace,
    pub video_color_range_full: bool,
    /// When true, the client uses a separate WebRTC connection for input.
    /// The server should NOT create input data channels on the primary connection.
    #[serde(default)]
    pub hybrid_mode: bool,
}

pub fn serialize_json<T>(message: &T) -> Option<String>
where
    T: Serialize,
{
    let Ok(json) = serde_json::to_string(&message) else {
        warn!("[Stream]: failed to serialize to json");
        return None;
    };

    Some(json)
}
