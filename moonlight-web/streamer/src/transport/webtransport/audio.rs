//! WebTransport audio transport
//!
//! Handles sending audio samples on a reliable unidirectional stream over WebTransport.

use std::sync::Arc;

use bytes::Bytes;
use log::{debug, error, info, warn};
use moonlight_common::stream::bindings::{AudioConfig, OpusMultistreamConfig};
use tokio::runtime::Handle;
use tokio::sync::Mutex;

pub struct WebTransportAudio {
    // wtransport 0.6 uses wtransport::SendStream (not server::SendStream)
    pub stream_writer: Arc<Mutex<Option<wtransport::SendStream>>>,
    config: Option<OpusMultistreamConfig>,
    audio_config: Option<AudioConfig>,
    channel_queue_size: usize,
    sample_buffer: Arc<Mutex<Vec<Bytes>>>,
}

impl WebTransportAudio {
    pub fn new(_runtime: Handle, channel_queue_size: usize) -> Self {
        Self {
            stream_writer: Arc::new(Mutex::new(None)),
            config: None,
            audio_config: None,
            channel_queue_size,
            sample_buffer: Arc::new(Mutex::new(Vec::with_capacity(channel_queue_size))),
        }
    }

    pub fn set_stream_writer(&mut self, writer: wtransport::SendStream) {
        let mut guard = self.stream_writer.blocking_lock();
        *guard = Some(writer);
        info!("[WebTransport]: Audio stream writer set");
    }

    pub async fn setup(
        &mut self,
        audio_config: AudioConfig,
        stream_config: OpusMultistreamConfig,
    ) -> i32 {
        info!("[WebTransport]: Setting up audio: config={:?}, sample_rate={}", 
              audio_config, stream_config.sample_rate);
        
        const SUPPORTED_SAMPLE_RATES: &[u32] = &[80000, 12000, 16000, 24000, 48000];
        if !SUPPORTED_SAMPLE_RATES.contains(&stream_config.sample_rate) {
            warn!(
                "[WebTransport] Audio could have problems because of the sample rate, Selected: {}, Expected one of: {SUPPORTED_SAMPLE_RATES:?}",
                stream_config.sample_rate
            );
        }
        
        if audio_config != self.audio_config.unwrap_or(audio_config) {
            warn!(
                "[WebTransport] A different audio configuration than requested was selected, Expected: {:?}, Found: {audio_config:?}",
                self.audio_config
            );
        }

        // Check if stream writer is available
        let writer_guard = self.stream_writer.lock().await;
        if writer_guard.is_none() {
            error!("[WebTransport]: Audio stream writer not available");
            return -1;
        }
        drop(writer_guard);

        self.config = Some(stream_config);
        self.audio_config = Some(audio_config);

        info!("[WebTransport]: Audio setup complete");
        0
    }

    pub async fn send_audio_sample(&mut self, data: &[u8]) {
        let mut writer_guard = self.stream_writer.lock().await;
        
        if let Some(ref mut writer) = *writer_guard {
            // wtransport SendStream API verified:
            // - write(&mut self, buf: &[u8]) -> Result<usize, StreamWriteError>
            // - write_all(&mut self, buf: &[u8]) -> Result<(), StreamWriteError>
            match writer.write_all(data).await {
                Ok(_) => {
                    // Successfully sent
                }
                Err(err) => {
                    warn!("[WebTransport]: Failed to send audio sample: {err:?}");
                    // Audio stream might be closed, but we'll continue trying
                }
            }
        } else {
            // Buffer the sample if writer not ready yet
            let mut buffer = self.sample_buffer.lock().await;
            if buffer.len() < self.channel_queue_size {
                buffer.push(Bytes::from(data.to_vec()));
            } else {
                warn!("[WebTransport]: Audio sample buffer full, dropping sample");
            }
        }
    }

    /// Flush any buffered samples when stream writer becomes available
    pub async fn flush_buffer(&self) {
        let mut writer_guard = self.stream_writer.lock().await;
        if let Some(ref mut writer) = *writer_guard {
            let mut buffer = self.sample_buffer.lock().await;
            while let Some(sample) = buffer.pop() {
                // wtransport SendStream.write_all() verified
                if writer.write_all(&sample).await.is_err() {
                    warn!("[WebTransport]: Failed to flush buffered audio sample");
                    break;
                }
            }
        }
    }
}
