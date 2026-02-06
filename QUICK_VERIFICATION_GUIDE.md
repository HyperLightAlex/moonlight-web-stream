# Quick WebTransport API Verification Guide

## Method 1: Compile and Check Errors (Fastest)

The fastest way to verify API calls is to try compiling:

```powershell
cd moonlight-web/streamer
cargo build
```

**What to look for:**
- If it compiles: The API calls are likely correct (or at least syntactically valid)
- If errors occur: Check the error messages - they'll tell you exactly what's wrong

**Common errors you might see:**
- `no method named 'X' found` → Method doesn't exist or has different name
- `expected type X, found type Y` → Return type is different than expected
- `trait bound not satisfied` → Type doesn't implement expected trait

## Method 2: Check Documentation (Most Reliable)

Generate and view the wtransport documentation:

```powershell
cd moonlight-web/streamer
cargo doc --open --package wtransport
```

This opens the API docs in your browser. Navigate to:
- `wtransport::server::Session` - Check methods like `datagrams()`, `accept_bi()`, `open_uni()`
- `wtransport::server::SendStream` - Check `write()` method
- `wtransport::server::RecvStream` - Check reading methods
- `wtransport::server::SendDatagram` - Check `send()` method
- `wtransport::Endpoint` - Check `incoming()` method

## Method 3: Check Source Code (Most Detailed)

If docs are unclear, check the actual source:

```powershell
# Find where cargo stores the source
$env:CARGO_HOME
# Usually: C:\Users\YourName\.cargo\registry\src\index.crates.io-*\wtransport-*

# Or use cargo vendor to download source locally
cd moonlight-web/streamer
cargo vendor
# Source will be in: vendor/wtransport/
```

## Method 4: Runtime Testing (Most Practical)

1. **Start your server** with WebTransport enabled
2. **Connect from browser** - Use Chrome/Edge (they support WebTransport)
3. **Check browser console** for connection errors
4. **Check server logs** for any API-related errors

## Specific API Calls to Verify

### 1. Session Acceptance (`mod.rs` line ~190)
```rust
// Current: endpoint.incoming().await
// Verify: Does this return an iterator/stream of ConnectionRequest?
```

### 2. Datagram Sending (`video.rs` line ~79)
```rust
// Current: datagrams.send_stream() and send_datagram.send()
// Verify: Are these the correct method names?
```

### 3. Audio Stream Creation (`audio.rs` line ~32)
```rust
// Current: TODO - needs verification
// Verify: Is it session.open_uni() or session.create_uni()?
```

### 4. Stream Reading (`channels.rs` line ~25)
```rust
// Current: read_from_recv_stream() is a placeholder
// Verify: Does RecvStream implement AsyncRead? Or use read_chunk()?
```

## Quick Checklist

Run this to see what needs verification:

```powershell
# Find all TODO comments related to API
Get-ChildItem -Recurse -Include *.rs | Select-String -Pattern "TODO.*API|TODO.*wtransport|TODO.*verify" -CaseSensitive:$false
```

## Recommended Approach

1. **First**: Try `cargo build` - if it compiles, you're 80% there
2. **Second**: Check docs with `cargo doc --open --package wtransport`
3. **Third**: Test with a real browser connection
4. **Fourth**: Fix any runtime errors that appear

## Getting Help

If you're stuck:
1. Check the wtransport crate on crates.io: https://crates.io/crates/wtransport
2. Look for examples in the crate repository
3. Check the wtransport GitHub issues/discussions
4. The error messages from `cargo build` are usually very helpful
