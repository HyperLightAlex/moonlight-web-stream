//! WebTransport video transport
//!
//! Handles sending video frames as unreliable datagrams over WebTransport.
//! Uses similar codec logic to WebRTC but sends packetized data as datagrams.

use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use bytes::BytesMut;
use log::{info, trace, warn};
use moonlight_common::stream::{
    bindings::{DecodeResult, FrameType, SupportedVideoFormats, VideoDecodeUnit, VideoFormat},
    video::VideoSetup,
};
use tokio::runtime::Handle;
use webrtc::rtp::{
    codecs::{
        h264::H264Payloader,
        h265::{HevcPayloader, RTP_OUTBOUND_MTU},
    },
    packetizer::Payloader,
};

use crate::transport::webrtc::video::{
    h264::reader::H264Reader,
    h265::reader::H265Reader,
};

enum VideoCodec {
    H264 {
        payloader: H264Payloader,
    },
    H265 {
        payloader: HevcPayloader,
    },
}

pub struct WebTransportVideo {
    supported_video_formats: SupportedVideoFormats,
    // wtransport 0.6: Connection.send_datagram() is synchronous, no separate writer needed
    connection: Arc<tokio::sync::Mutex<Option<Arc<wtransport::Connection>>>>,
    needs_idr: Arc<AtomicBool>,
    clock_rate: u32,
    codec: Option<VideoCodec>,
    /// Track the start time for local timestamp generation.
    stream_start_time: Option<Instant>,
    frame_queue_size: usize,
}

impl WebTransportVideo {
    pub fn new(
        _runtime: Handle,
        supported_video_formats: SupportedVideoFormats,
        frame_queue_size: usize,
    ) -> Self {
        Self {
            clock_rate: 0,
            needs_idr: Arc::new(AtomicBool::new(false)),
            connection: Arc::new(tokio::sync::Mutex::new(None)),
            codec: None,
            supported_video_formats,
            stream_start_time: None,
            frame_queue_size,
        }
    }

    pub fn set_connection(&mut self, conn: Arc<wtransport::Connection>) {
        let mut guard = self.connection.blocking_lock();
        *guard = Some(conn);
        info!("[WebTransport]: Video connection set");
    }

    pub async fn setup(&mut self, setup: VideoSetup) -> bool {
        info!("[WebTransport]: Setting up video codec: {:?}", setup.format);
        
        self.clock_rate = 90000; // Standard video clock rate
        
        // Initialize codec based on format
        self.codec = match setup.format {
            VideoFormat::H264 => Some(VideoCodec::H264 {
                payloader: H264Payloader::default(),
            }),
            VideoFormat::H265 => Some(VideoCodec::H265 {
                payloader: HevcPayloader::default(),
            }),
            // AV1 not supported on all platforms
            _ => {
                warn!("[WebTransport]: Unsupported or unavailable video format: {:?}", setup.format);
                return false;
            }
        };
        
        self.stream_start_time = Some(Instant::now());
        info!("[WebTransport]: Video codec setup complete");
        true
    }

    pub async fn send_decode_unit(&mut self, unit: &VideoDecodeUnit<'_>) -> DecodeResult {
        // Generate timestamp from local clock (same as WebRTC)
        let timestamp = if let Some(start_time) = self.stream_start_time {
            let elapsed = start_time.elapsed();
            (elapsed.as_secs_f64() * self.clock_rate as f64) as u32
        } else {
            (unit.presentation_time.as_secs_f64() * self.clock_rate as f64) as u32
        };

        let mut full_frame = Vec::new();
        for buffer in unit.buffers {
            full_frame.extend_from_slice(buffer.data);
        }

        let important = matches!(unit.frame_type, FrameType::Idr);
        
        // Collect samples into a local Vec first
        let mut samples: Vec<BytesMut> = Vec::new();
        
        // Collect samples based on codec type (immutable borrow)
        let codec_type = match &self.codec {
            Some(VideoCodec::H264 { .. }) => Some("h264"),
            Some(VideoCodec::H265 { .. }) => Some("h265"),
            None => None,
        };

        match codec_type {
            Some("h264") => {
                let mut nal_reader = H264Reader::new(Cursor::new(full_frame.clone()), full_frame.len());

                while let Ok(Some(nal)) = nal_reader.next_nal() {
                    trace!(
                        "[WebTransport] H264, Start Code: {:?}, NAL: {:?}",
                        nal.start_code,
                        nal.header,
                    );

                    let data = trim_bytes_to_range(
                        nal.full,
                        nal.header_range.start..nal.payload_range.end,
                    );

                    samples.push(data);
                }
            }
            Some("h265") => {
                let mut nal_reader = H265Reader::new(Cursor::new(full_frame.clone()), full_frame.len());

                while let Ok(Some(nal)) = nal_reader.next_nal() {
                    trace!(
                        "[WebTransport] H265, Start Code: {:?}, NAL: {:?}",
                        nal.start_code,
                        nal.header,
                    );

                    let data = trim_bytes_to_range(
                        nal.full,
                        nal.header_range.start..nal.payload_range.end,
                    );

                    samples.push(data);
                }
            }
            None | Some(_) => {
                warn!("[WebTransport]: Failed to send decode unit because of missing codec!");
                return DecodeResult::Ok;
            }
        }

        // Now send the samples - get mutable access to payloader (separate scope)
        let send_result = match &mut self.codec {
            Some(VideoCodec::H264 { payloader }) => {
                Self::send_samples(
                    &self.connection,
                    &self.needs_idr,
                    samples,
                    payloader,
                    timestamp,
                ).await
            }
            Some(VideoCodec::H265 { payloader }) => {
                Self::send_samples(
                    &self.connection,
                    &self.needs_idr,
                    samples,
                    payloader,
                    timestamp,
                ).await
            }
            None => Ok(()),
        };

        if send_result.is_err() {
            self.needs_idr.store(true, Ordering::Release);
        }

        if self
            .needs_idr
            .compare_exchange_weak(true, false, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            return DecodeResult::NeedIdr;
        }

        DecodeResult::Ok
    }

    /// Send samples as datagrams via the WebTransport connection (static method)
    async fn send_samples<P: Payloader>(
        connection: &Arc<tokio::sync::Mutex<Option<Arc<wtransport::Connection>>>>,
        needs_idr: &Arc<AtomicBool>,
        samples: Vec<BytesMut>,
        payloader: &mut P,
        timestamp: u32,
    ) -> Result<(), ()> {
        let conn_guard = connection.lock().await;
        let conn = match conn_guard.as_ref() {
            Some(c) => c,
            None => {
                warn!("[WebTransport]: No connection available for video datagrams");
                needs_idr.store(true, Ordering::Release);
                return Err(());
            }
        };

        let mut packets_sent = 0u32;
        let mut packets_failed = 0u32;
        let mut peekable = samples.into_iter().peekable();

        while let Some(sample) = peekable.next() {
            // Packetize the sample (similar to RTP packetization but without RTP headers)
            let max_payload_size = RTP_OUTBOUND_MTU - 12; // Reserve some space
            
            let payloads = match payloader.payload(max_payload_size, &sample.freeze()) {
                Ok(payloads) => payloads,
                Err(err) => {
                    warn!("[WebTransport]: Failed to packetize: {err:?}");
                    continue;
                }
            };

            // Send each packetized payload as a datagram
            for (i, payload) in payloads.iter().enumerate() {
                // Create a simple packet format: [timestamp: u32][sequence: u16][is_last: u8][payload]
                let is_last = peekable.peek().is_none() && i == payloads.len() - 1;
                let mut datagram = BytesMut::with_capacity(4 + 2 + 1 + payload.len());
                datagram.extend_from_slice(&timestamp.to_be_bytes());
                datagram.extend_from_slice(&(packets_sent as u16).to_be_bytes());
                datagram.extend_from_slice(&[if is_last { 1 } else { 0 }]);
                datagram.extend_from_slice(payload);

                // wtransport 0.6: send_datagram is synchronous
                match conn.send_datagram(datagram.freeze()) {
                    Ok(_) => {
                        packets_sent += 1;
                    }
                    Err(err) => {
                        packets_failed += 1;
                        warn!("[WebTransport]: Failed to send video datagram: {err:?}");
                        // If we're dropping too many packets, request IDR
                        if packets_failed > packets_sent / 2 {
                            needs_idr.store(true, Ordering::Release);
                            return Err(());
                        }
                    }
                }
            }
        }

        if packets_failed > 0 {
            warn!(
                "[WebTransport]: Failed to send {}/{} video packets",
                packets_failed,
                packets_sent + packets_failed
            );
            if packets_failed > packets_sent {
                needs_idr.store(true, Ordering::Release);
                return Err(());
            }
        }

        Ok(())
    }
}

/// Trim bytes to a specific range (helper function from WebRTC implementation)
fn trim_bytes_to_range(mut buf: BytesMut, range: std::ops::Range<usize>) -> BytesMut {
    if range.start > 0 {
        let _ = buf.split_to(range.start);
    }

    if range.end - range.start < buf.len() {
        let _ = buf.split_off(range.end - range.start);
    }

    buf
}
