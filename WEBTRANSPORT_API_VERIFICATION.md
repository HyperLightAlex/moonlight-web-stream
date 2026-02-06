# WebTransport API Verification Guide

This document outlines how to verify the wtransport crate API calls used in the implementation.

## Prerequisites

1. **Build the project** to check for compilation errors:
   ```bash
   cd moonlight-web/streamer
   cargo build
   ```

2. **Generate documentation** for the wtransport crate:
   ```bash
   cargo doc --open --package wtransport
   ```
   This will open the wtransport API documentation in your browser.

## API Calls to Verify

### 1. Server Setup (`mod.rs`)

**Location**: `moonlight-web/streamer/src/transport/webtransport/mod.rs`

**Calls to verify**:
- [ ] `Endpoint::server(ServerConfig)` - Verify constructor signature
- [ ] `endpoint.incoming()` - Verify this returns an async iterator/stream
- [ ] `connection_request.accept()` - Verify this accepts the connection and returns a Session

**Current implementation**:
```rust
let endpoint = Endpoint::server(server_config)?;
while let Some(connection_request) = endpoint.incoming().await {
    let session = connection_request.accept().await?;
}
```

**How to verify**:
1. Check `cargo doc --open --package wtransport`
2. Look for `wtransport::Endpoint` documentation
3. Verify `incoming()` method signature
4. Verify `ConnectionRequest::accept()` method

### 2. Video Datagrams (`video.rs`)

**Location**: `moonlight-web/streamer/src/transport/webtransport/video.rs`

**Calls to verify**:
- [ ] `session.datagrams()` - Verify this returns a datagram handle
- [ ] `datagrams.send_stream()` - Verify this returns `SendDatagram`
- [ ] `send_datagram.send(data)` - Verify the send method signature

**Current implementation**:
```rust
let datagrams = session.datagrams();
let send_stream = datagrams.send_stream();
send_stream.send(data).await?;
```

**How to verify**:
1. Check `wtransport::server::Session` documentation
2. Look for `datagrams()` method
3. Verify `SendDatagram` type and its methods

### 3. Audio Streams (`audio.rs`)

**Location**: `moonlight-web/streamer/src/transport/webtransport/audio.rs`

**Calls to verify**:
- [ ] `session.open_uni()` or `session.create_uni()` - Verify method to create unidirectional stream
- [ ] `SendStream::write(data)` - Verify write method signature
- [ ] `SendStream::write_all(data)` - Check if this exists as an alternative

**Current implementation** (needs verification):
```rust
// TODO: Verify wtransport API for creating unidirectional stream
// Likely: session.open_uni().await or session.create_uni().await
let send_stream = session.open_uni().await?;
send_stream.write(&bytes).await?;
```

**How to verify**:
1. Check `wtransport::server::Session` documentation
2. Look for methods to create unidirectional streams
3. Verify `SendStream` type and its write methods

### 4. Input Channels (`channels.rs`)

**Location**: `moonlight-web/streamer/src/transport/webtransport/channels.rs`

**Calls to verify**:
- [ ] `session.accept_bi()` - Verify method to accept bidirectional streams
- [ ] `session.accept_uni()` - Verify method to accept unidirectional streams
- [ ] `RecvStream::read_chunk(size)` - Verify reading method
- [ ] `RecvStream` implements `AsyncRead` trait - Check if we can use standard async read

**Current implementation** (needs verification):
```rust
// TODO: Verify wtransport API for accepting bidirectional streams
match session.accept_bi().await {
    Ok((send_stream, recv_stream)) => {
        // Handle stream
    }
}

// TODO: Verify reading from RecvStream
match recv_stream.read_chunk(65536, true).await {
    Ok(Some(chunk)) => {
        // Process chunk
    }
}
```

**How to verify**:
1. Check `wtransport::server::Session` documentation
2. Look for `accept_bi()` and `accept_uni()` methods
3. Verify `RecvStream` type and its reading methods
4. Check if `RecvStream` implements `tokio::io::AsyncRead`

## Verification Steps

### Step 1: Check Compilation

```bash
cd moonlight-web/streamer
cargo build 2>&1 | tee build_errors.txt
```

Look for errors related to:
- Unknown methods on `wtransport` types
- Type mismatches
- Missing trait implementations

### Step 2: Generate and Review Documentation

```bash
# Generate docs for wtransport only
cargo doc --no-deps --package wtransport --open

# Or generate all docs
cargo doc --open
```

Navigate to:
- `wtransport::server::Session`
- `wtransport::server::SendStream`
- `wtransport::server::RecvStream`
- `wtransport::server::SendDatagram`
- `wtransport::Endpoint`
- `wtransport::server::ConnectionRequest`

### Step 3: Check Examples

Look for examples in the wtransport crate:
```bash
# Find example files
find ~/.cargo/registry -name "*.rs" -path "*/wtransport-*/examples/*" 2>/dev/null

# Or check the crate source
cargo vendor  # If needed
```

### Step 4: Test Individual Components

Create a minimal test file to verify each API call:

```rust
// test_webtransport_api.rs
use wtransport::{Endpoint, ServerConfig};

#[tokio::test]
async fn test_endpoint_creation() {
    // Test endpoint creation
}

#[tokio::test]
async fn test_session_acceptance() {
    // Test session acceptance
}

#[tokio::test]
async fn test_datagram_sending() {
    // Test datagram sending
}

#[tokio::test]
async fn test_stream_creation() {
    // Test stream creation
}
```

### Step 5: Runtime Testing

1. **Start the server** with WebTransport enabled
2. **Connect from browser** using the TypeScript client
3. **Check logs** for any runtime errors
4. **Monitor network** in browser DevTools (should show HTTP/3)

## Common Issues to Look For

1. **Method name differences**: The API might use different names (e.g., `create_uni` vs `open_uni`)
2. **Return types**: Methods might return `Result` or `Option` differently than expected
3. **Async patterns**: Some methods might be sync, others async
4. **Trait implementations**: `RecvStream` might implement `AsyncRead`, allowing standard reading patterns

## Quick Verification Checklist

- [ ] Project compiles without errors
- [ ] `Endpoint::server()` works correctly
- [ ] `endpoint.incoming()` returns connection requests
- [ ] `connection_request.accept()` creates a session
- [ ] `session.datagrams()` returns datagram handle
- [ ] `SendDatagram::send()` works for video
- [ ] Unidirectional stream creation works for audio
- [ ] Bidirectional stream acceptance works for input
- [ ] Stream reading methods work correctly
- [ ] All TODO comments in code are resolved

## Alternative: Check wtransport Source

If documentation is unclear, check the actual source:

```bash
# Find wtransport source location
cargo tree | grep wtransport

# Or check in Cargo.lock
grep -A 5 "wtransport" Cargo.lock

# Source is typically in:
# ~/.cargo/registry/src/*/wtransport-*/
```

Then read the source files directly to understand the API.

## Next Steps After Verification

Once you've verified the API:

1. **Update placeholders** in the code with correct API calls
2. **Remove TODO comments** that are resolved
3. **Add error handling** for API-specific error types
4. **Test end-to-end** with a real browser connection
5. **Update documentation** with any findings
