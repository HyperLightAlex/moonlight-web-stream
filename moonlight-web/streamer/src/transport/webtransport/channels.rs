//! WebTransport input channels
//!
//! Handles bidirectional streams for input data (mouse, keyboard, touch, controllers).
//! Supports separate input session for hybrid mode (like WebRTC).

use std::sync::Arc;

use bytes::Bytes;
use common::api_bindings::TransportChannelId;
use log::{debug, error, info, warn};
use tokio::sync::Mutex;
use wtransport::SendStream;

use crate::{
    buffer::ByteBuffer,
    transport::{InboundPacket, OutboundPacket, TransportChannel},
};

/// Read from RecvStream using wtransport API
/// 
/// wtransport RecvStream API verified:
/// - read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, StreamReadError>
/// - read_exact(&mut self, buf: &mut [u8]) -> Result<(), StreamReadExactError>
/// - Also implements AsyncRead trait
async fn read_from_recv_stream(
    reader: &mut wtransport::RecvStream,
    buf: &mut [u8],
) -> Result<Option<usize>, anyhow::Error> {
    match reader.read(buf).await {
        Ok(Some(bytes_read)) => Ok(Some(bytes_read)),
        Ok(None) => Ok(None), // Stream finished/closed gracefully
        Err(e) => Err(anyhow::anyhow!("Stream read error: {e:?}")),
    }
}

/// Manages bidirectional streams for input channels
pub struct WebTransportChannels {
    // Map of channel ID to bidirectional stream writer
    channel_writers: Mutex<std::collections::HashMap<u8, Arc<Mutex<Option<SendStream>>>>>,
    // Map of channel ID to stream reader task handles
    channel_readers: Mutex<std::collections::HashMap<u8, tokio::task::JoinHandle<()>>>,
    // Receiver for inbound packets from all channels
    packet_receiver: tokio::sync::mpsc::Receiver<InboundPacket>,
    packet_sender: tokio::sync::mpsc::Sender<InboundPacket>,
}

impl WebTransportChannels {
    pub fn new() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(100);
        Self {
            channel_writers: Mutex::new(std::collections::HashMap::new()),
            channel_readers: Mutex::new(std::collections::HashMap::new()),
            packet_receiver: receiver,
            packet_sender: sender,
        }
    }

    /// Set up bidirectional streams for input channels on a session
    /// This should be called when an input session is established
    pub async fn setup_channels_on_session(
        &self,
        session: Arc<wtransport::Connection>,
    ) -> Result<(), anyhow::Error> {
        info!("[WebTransport]: Setting up input channels on session");
        
        // Create bidirectional streams for each input channel type
        // We'll create them as the client requests them, or create them proactively
        
        // For now, we'll wait for the client to create streams
        // and handle them in handle_incoming_stream
        
        Ok(())
    }

    /// Handle an incoming bidirectional stream from the client
    pub async fn handle_incoming_stream(
        &self,
        stream: wtransport::RecvStream,
        send_stream: SendStream,
        channel_id: u8,
    ) {
        info!("[WebTransport]: Handling incoming stream for channel {}", channel_id);
        
        // Store the send stream for outgoing packets
        {
            let mut writers = self.channel_writers.lock().await;
            writers.insert(channel_id, Arc::new(Mutex::new(Some(send_stream))));
        }
        
        // Spawn a task to read from the stream
        let sender = self.packet_sender.clone();
        let channel = TransportChannel(channel_id);
        
        let reader_handle = tokio::spawn(async move {
            // wtransport RecvStream API verified:
            // - read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, StreamReadError>
            
            let mut reader = stream;
            let mut buffer = Vec::new();
            let mut temp_buf = [0u8; 8192];
            
            loop {
                // Using verified wtransport API: reader.read()
                match read_from_recv_stream(&mut reader, &mut temp_buf).await {
                    Ok(Some(bytes_read)) => {
                        buffer.extend_from_slice(&temp_buf[..bytes_read]);
                        
                        // Try to deserialize packets from the buffer
                        while let Some((packet, consumed)) = Self::try_deserialize_packet(&buffer, channel) {
                            if sender.send(packet).await.is_err() {
                                debug!("[WebTransport]: Packet receiver closed, stopping stream reader");
                                break;
                            }
                            buffer.drain(..consumed);
                        }
                    }
                    Ok(None) => {
                        debug!("[WebTransport]: Stream closed for channel {}", channel_id);
                        break;
                    }
                    Err(err) => {
                        warn!("[WebTransport]: Error reading from stream channel {}: {err:?}", channel_id);
                        break;
                    }
                }
            }
        });
        
        // Store the reader handle
        let mut readers = self.channel_readers.lock().await;
        readers.insert(channel_id, reader_handle);
    }

    /// Try to deserialize a packet from the buffer
    fn try_deserialize_packet(
        buffer: &[u8],
        channel: TransportChannel,
    ) -> Option<(InboundPacket, usize)> {
        if buffer.is_empty() {
            return None;
        }
        
        // Try to deserialize using the existing InboundPacket logic
        // For now, we'll use a simple approach: read the full buffer as one packet
        // In practice, we might need framing
        
        match InboundPacket::deserialize(channel, buffer) {
            Some(packet) => Some((packet, buffer.len())),
            None => None,
        }
    }

    /// Send a packet on the appropriate channel
    pub async fn send_packet(&self, packet: OutboundPacket) -> Result<(), anyhow::Error> {
        let mut raw_buffer = Vec::new();
        let Some((channel, range)) = packet.serialize(&mut raw_buffer) else {
            return Err(anyhow::anyhow!("Failed to serialize packet"));
        };
        
        let channel_id = channel.0;
        let data = Bytes::from(raw_buffer[range].to_vec());
        
        let writers = self.channel_writers.lock().await;
        if let Some(writer_arc) = writers.get(&channel_id) {
            let mut writer = writer_arc.lock().await;
            if let Some(ref mut send_stream) = *writer {
                send_stream.write(&data).await
                    .map_err(|e| anyhow::anyhow!("Failed to write to stream: {e:?}"))?;
                Ok(())
            } else {
                Err(anyhow::anyhow!("Stream writer not available for channel {}", channel_id))
            }
        } else {
            warn!("[WebTransport]: No stream writer for channel {}", channel_id);
            Err(anyhow::anyhow!("Channel {} not set up", channel_id))
        }
    }

    /// Receive the next packet (non-blocking)
    pub fn try_receive_packet(&mut self) -> Option<InboundPacket> {
        self.packet_receiver.try_recv().ok()
    }

    /// Receive the next packet (blocking)
    pub async fn receive_packet(&mut self) -> Option<InboundPacket> {
        self.packet_receiver.recv().await
    }
}
