# WebRTC Single-Negotiation Refactor Plan

## Purpose

This document captures the current understanding of the Android WebView `144.xx+`
streaming regression and the recommended production refactor plan for the primary
WebRTC peer.

The goal is to eliminate the late media renegotiation on the primary peer and move
to a single-negotiation path that works with hybrid input mode.

This document separates confirmed observations from stronger inferences. That
matters because the current evidence is enough to drive a refactor, but not enough
to claim a specific Chromium root cause with certainty.

## What We Know

### Confirmed Facts

1. The regression is specific to newer Android WebView builds.
- The issue reproduces on WebView `145.xx`.
- Earlier WebView builds are known to work.
- `144.xx` is the earliest confirmed bad line, but a later `143.xx` regression cannot
  be ruled out from public information alone.

2. The primary peer initially negotiates successfully.
- In the current legacy flow, the browser creates the initial offer in
  `moonlight-web/web-server/web/stream/transport/webrtc.ts`.
- The server answers, ICE reaches `connected`, and the primary peer becomes usable.
- Hybrid input is a separate peer and can remain connected even when the primary
  media peer later fails.

3. The failed pre-negotiation experiment did not break the initial peer setup.
- In the experimental branch, the browser still created the initial offer and the
  server still answered.
- The primary peer could still reach `connected`, and the rest of the stream startup
  flow still advanced.
- What changed was the media shape after the first negotiation:
  - the browser predeclared an audio `recvonly` transceiver
  - the server reserved an audio sender and later used `replace_track()`
  - video still relied on the legacy add-track plus renegotiation path
- In practice, that branch produced a separate regression where tracks never arrived.

4. In the legacy flow, media is added after the initial connection.
- The streamer only starts the Moonlight host stream after the primary peer reaches
  `Connected` in `moonlight-web/streamer/src/transport/webrtc/mod.rs`.
- Audio/video tracks are attached later in
  `moonlight-web/streamer/src/transport/webrtc/audio.rs` and
  `moonlight-web/streamer/src/transport/webrtc/video.rs`.
- When tracks are attached with `add_track()`, the streamer calls `send_offer()` to
  renegotiate.

5. The current failure happens after the late renegotiation succeeds enough to start media.
- In the latest successful-media test on March 11, 2026:
  - the primary peer connected at about `4:17:57 PM`
  - the browser received a second remote SDP `offer` at about `4:18:02 PM`
  - audio/video tracks arrived immediately after
  - ICE went `disconnected` at about `4:18:07 PM`
  - the primary peer went `failed` at about `4:18:17 PM`

6. The earlier "no audio/video rendered" branch was a separate regression.
- That branch changed the server/browser negotiation shape and produced a
  media-attach failure where the primary connection stayed up but tracks never arrived.
- That branch did not disprove the renegotiation hypothesis.
- It only showed that a broken media path can leave the primary peer alive longer.

7. Android-side debug instrumentation is a confounder, but not the primary root cause.
- The debug monitors mutate WebRTC runtime behavior in Backbone.
- They should stay disabled while validating server-side fixes.
- However, disabling them alone did not fix the earlier no-media branch.

8. `ConnectionComplete` currently precedes real media attachment.
- In the current flow, `ConnectionComplete` is emitted from the streamer before the
  browser has necessarily received audio/video tracks.
- The Backbone/WebView client currently treats that as "stream ready", even though
  first-track or first-frame is a later event.

9. Public Chromium release notes do not identify a definitive regression entry.
- Public release notes near the `143/144` boundary do not explicitly describe this
  failure mode.
- The only nearby public signal that overlaps this architecture is around subsequent
  WebRTC offer/answer behavior.
- That is useful context, but it is not proof.

### Strongest Inferences

1. The remaining regression is most likely triggered by the post-connect media renegotiation.
- The clearest correlation is:
  - initial connection succeeds
  - second SDP exchange happens
  - media starts
  - ICE disconnects shortly after

2. Rendering is probably not the cause.
- Media actually renders briefly in the failing `145.xx` test before the primary peer dies.
- That points to WebRTC state/consent behavior after renegotiation, not to the video
  element or autoplay path itself.

3. The earlier mixed pre-negotiation attempt failed because it still depended on an
   unstable media setup shape.
- It changed the SDP flow, but it did not produce a clean, codec-aware, single-
  negotiation media path.
- The result was a separate no-media regression rather than a real production fix.
- The strongest explanation is not "pre-negotiation is impossible".
- The stronger explanation is "partial pre-negotiation with the old sequencing still
  left the peer in an inconsistent intermediate shape".

4. Public Chromium release notes do not give a definitive smoking gun.
- There is public signal around changes to subsequent WebRTC offer/answer behavior
  near the `143/144` boundary.
- That overlaps the current architecture, but it is not proof by itself.

5. ICE keepalive timing is still a secondary suspect, but weaker than renegotiation.
- The server currently uses a `500ms` keepalive interval.
- That may aggravate newer WebView behavior, but the current timeline still points
  more strongly at the second offer/answer boundary than at keepalive timing alone.

## What The Failed Pre-Negotiation Attempt Actually Tells Us

The experimental branch that predeclared audio and reserved a sender is important,
but it should not be over-interpreted.

What it proves:

1. A changed initial SDP shape can still produce a healthy first connection.
2. A broken media-attach path can leave the primary peer alive without delivering tracks.
3. The no-media result does not mean rendering is the trigger.

What it does not prove:

1. It does not prove the issue is unrelated to renegotiation.
2. It does not prove full single-negotiation media is impossible.
3. It does not prove Chromium only fails when media bytes are actually flowing.

The most useful reading is:

1. The legacy flow proves that successful late renegotiation is followed by failure on
   modern WebView.
2. The mixed branch proves that changing media attachment shape can create a separate
   regression before reaching that point.
3. Therefore the production fix should be a clean single-negotiation architecture,
   not another partial hybrid of the old and new flows.

## Current Architecture Summary

### Primary Peer Today

1. Browser opens the stream page and creates a primary peer.
2. Browser sends the initial SDP offer.
3. Streamer answers.
4. ICE reaches `Connected`.
5. Only then does the streamer start the Moonlight host stream.
6. When audio/video become available, the streamer attaches tracks.
7. Track attachment triggers a second SDP exchange.
8. Browser receives the second offer, applies it, and media starts.
9. On modern WebView, the primary peer then dies shortly afterward.

### Hybrid Input Today

1. The native Android client opens a separate input-only peer using the hybrid
   `session_token`.
2. That peer can remain connected even when the primary media peer fails.
3. This is expected and should not be used as evidence that the primary media
   connection is healthy.

## Production Recommendation

Move the primary WebRTC flow to a true single-negotiation path.

The recommended production shape is a server-offer, media-first primary peer.

This is the preferred production direction over "client offers first, but later uses
predeclared media sections" because:

1. the server can wait until actual media setup is known
2. the first SDP can reflect the real primary peer shape
3. the browser no longer needs to guess future media structure
4. the late media renegotiation path can be removed entirely

The target runtime sequence is:

1. Browser opens the stream page and WebSocket.
2. Fuji/web server launches or resumes the game immediately.
3. Streamer starts Moonlight immediately and waits until actual media setup is known.
4. Streamer creates the primary peer with:
   - primary data channels
   - real audio track
   - real video track
5. Streamer sends the first SDP offer.
6. Browser answers.
7. No later media renegotiation occurs on the primary peer.
8. `ConnectionComplete` is sent only after the primary peer is fully negotiated.

This is the cleanest production direction because the first SDP already reflects the
real media shape instead of guessing and adding media later.

## Concrete Implementation Plan

The phases below are intentionally ordered so each step unlocks the next one without
leaving the system in a half-migrated state.

### Phase 1: Signaling Contract Changes

Update shared signaling in `moonlight-web/common/src/api_bindings.rs`.

Recommended changes:

1. Keep `StreamServerMessage::Setup` for:
- ICE servers
- hybrid `session_token`

2. Add a primary negotiation mode for the browser.
- The browser should know that the primary peer will wait for a server-created offer.
- Recommended shape:
  - add a field on `Setup`, such as `primary_negotiation_role`
  - initial supported values: `client_offer`, `server_offer`
  - use `server_offer` only for the new primary path

3. Optionally add a distinct message for readiness if needed.
- Example purpose:
  - browser learns that the streamer has enough information to negotiate
  - browser does not create the primary SDP offer on its own

4. Keep hybrid input signaling unchanged in this phase.
- The input-only peer is already separate.
- Avoid mixing the primary-peer refactor with hybrid input signaling changes.

### Phase 2: Browser Transport Refactor

Refactor `moonlight-web/web-server/web/stream/transport/webrtc.ts`.

Required behavior:

1. `initPeer()` becomes passive.
- Create the `RTCPeerConnection`
- register event listeners
- do not call `onNegotiationNeeded()` immediately

2. Add `ondatachannel` handling for primary channels.
- The browser should accept server-created primary data channels
- this removes dependence on the browser being the initial offerer

3. Keep track handling unchanged.
- `ontrack` remains the source of real media readiness

4. When a remote offer arrives:
- set remote description
- create answer
- set local description
- send answer back to the server

5. Keep a temporary compatibility path.
- While the refactor is landing, allow `client_offer` mode to preserve the legacy flow.
- This gives a cleaner migration and simpler testing while the server side is being
  reshaped.

### Phase 3: Primary Data Channel Ownership

Move primary data channel creation into
`moonlight-web/streamer/src/transport/webrtc/mod.rs`.

Recommended server-created channels:

- `general`
- `stats`
- mouse channels
- keyboard
- touch
- controller channels

Reason:

- the browser can still send on those channels once they open
- the server-controlled offer can include the full primary peer shape from the start

Implementation note:

1. The browser-side channel map in `webrtc.ts` currently assumes client-created data
   channels for the primary peer.
2. Refactor the channel initialization so:
   - media channels remain track-based
   - data channels are populated from `ondatachannel`
   - channel IDs and names stay consistent with the existing transport layer

### Phase 4: Stream Startup Sequencing Change

Refactor primary stream startup in:

- `moonlight-web/streamer/src/transport/webrtc/mod.rs`
- `moonlight-web/streamer/src/main.rs`
- `moonlight-web/web-server/src/api/stream.rs`

This is the core production change.

Recommended change:

1. Stop using `RTCPeerConnectionState::Connected` as the trigger for `StartStream`.
2. Split "peer object created" from "stream started".
3. Start Moonlight as soon as:
   - the `/host/stream` request is accepted
   - Fuji launch/resume has succeeded
   - the streamer process is ready to receive stream setup callbacks
4. Do not emit `ConnectionComplete` from this early-start path.
5. Use this early startup window to learn:
   - actual `VideoSetup`
   - actual audio configuration
This is the key sequencing change needed to avoid the second SDP exchange.

### Phase 5: Create Tracks Before First Offer

Refactor:

- `moonlight-web/streamer/src/transport/webrtc/audio.rs`
- `moonlight-web/streamer/src/transport/webrtc/video.rs`
- `moonlight-web/streamer/src/transport/webrtc/sender.rs`
- `moonlight-web/streamer/src/transport/webrtc/mod.rs`

Required behavior:

1. Create audio/video tracks as soon as actual media setup is known.

2. Attach them to the primary peer before the first offer is generated.

3. Generate the first primary offer only after:
- primary data channels exist
- audio track exists or is explicitly known to be absent
- video track exists or is explicitly known to be absent

4. Remove the primary-path dependence on late `send_offer()`.

5. Keep `send_offer()` only where it is still required by hybrid input or other
   non-primary paths.

Target end state:

- the primary offer/answer is the only media negotiation
- primary audio/video setup does not trigger a later offer

Implementation note:

1. Do not reuse the earlier partial pre-negotiation approach as-is.
2. The new path should create the full primary shape in one place, with one signaling
   owner, after actual media setup is known.

### Phase 6: Tighten ConnectionComplete Semantics

Refactor:

- `moonlight-web/streamer/src/main.rs`
- `moonlight-web/web-server/web/stream/index.ts`

Recommended behavior:

1. `ConnectionComplete` should no longer mean "server orchestration finished but media is
   still waiting on later renegotiation".

2. After the refactor, `ConnectionComplete` should only be sent once:
- the primary answer has been applied
- the primary peer shape is final
- media tracks are part of the negotiated session
- the browser is no longer expected to receive a second primary offer later

This makes the browser/app state model more accurate.

### Phase 7: Startup Rollback and Timeouts

Because the game/host stream will now start earlier, add explicit rollback behavior.

Refactor:

- `moonlight-web/streamer/src/main.rs`
- `moonlight-web/web-server/src/api/stream.rs`
- Fuji orchestration callbacks as needed

Required protections:

1. If the browser never answers, stop the stream.
2. If ICE never reaches `Connected` within the startup timeout, stop the stream.
3. If Fuji launched a game for this session and startup fails, roll it back.

Suggested startup timeout:

- `10-15s` from initial signaling start

### Phase 8: Client State Cleanup

Backbone should eventually distinguish:

- transport negotiated
- first media track arrived
- stream ready for user

Relevant files:

- `backbone/app/src/main/java/com/playbackbone/android/pcstreaming/ui/stream/webview/HybridPCStreamViewModel.kt`
- `backbone/app/src/main/java/com/playbackbone/android/pcstreaming/ui/stream/webview/HybridStreamJSInterface.kt`

This is not the root fix for the WebView regression, but it will improve correctness
and debugging after the server refactor.

## Validation Criteria

The refactor should be considered correct only if all of the following are true:

1. On the browser side, the primary peer uses only one media negotiation.
- no second remote SDP `offer`
- no second local `answer` for the primary media path

2. Audio/video tracks arrive without a later renegotiation.

3. The primary peer does not transition to `disconnected` a few seconds after media starts
   on modern WebView.

4. Hybrid input still works unchanged.

5. Failure to complete startup cleanly rolls back the launched stream/game state.

## Open Questions

1. Does modern WebView fail specifically because of the second offer/answer, or because of
   some later consent/ICE behavior that only appears once media is flowing?
- Current evidence strongly points to the late renegotiation boundary.
- It is still possible that renegotiation is the trigger rather than the direct root cause.

2. Will server-created primary data channels integrate cleanly with the existing browser
   transport/channel mapping?
- This should be straightforward, but it is still an implementation risk.

3. Is it necessary to wait for both audio and video setup before the first offer?
- Most likely video is the critical dependency.
- Audio may be optional if it lags slightly, but the cleanest design is to negotiate both.

## Other Findings To Keep In Mind

1. Hybrid input persisting is expected.
- It uses a separate peer outside the WebView primary media path.
- It should not be used as evidence that the primary peer is healthy.

2. The current Backbone readiness model is optimistic.
- `onStreamConnected` currently maps to `ConnectionComplete`, not first media track/frame.
- This is not the primary bug, but it can obscure failure analysis.

3. The WebRTC debug monitors in Backbone should remain disabled while validating this work.
- They are not the main cause of the server-side regression.
- They still mutate runtime behavior and make bad evidence harder to interpret.

## Practical Guidance

1. Keep Android debug WebRTC monitors disabled while validating this refactor.
2. Do not treat the earlier no-media branch as evidence against the renegotiation theory.
3. The key target is not "make rendering succeed".
4. The key target is "make the first SDP contain the final primary media shape".
5. The clean production goal is not "pre-negotiate something".
6. The clean production goal is "negotiate exactly once, after actual media setup is known".

## Implementation Checklist

Use this checklist as the execution order during the refactor. Each milestone should
be completed and sanity-checked before moving to the next one.

### Milestone 0: Lock the Baseline

1. Keep the current legacy renegotiation path available as the fallback path.
2. Keep Backbone WebRTC debug monitors disabled during testing.
3. Capture one fresh baseline run on current WebView with:
- initial offer/answer success
- second offer arrival
- track arrival
- later ICE disconnect/failure

Files to watch:

- `moonlight-web/web-server/web/stream/transport/webrtc.ts`
- `moonlight-web/streamer/src/transport/webrtc/mod.rs`
- `moonlight-web/streamer/src/transport/webrtc/audio.rs`
- `moonlight-web/streamer/src/transport/webrtc/video.rs`

Exit condition:

- baseline behavior is still reproducible before the refactor begins

### Milestone 1: Add Signaling Role Support

1. Extend `StreamServerMessage::Setup` in `moonlight-web/common/src/api_bindings.rs`
   with a primary negotiation role field.
2. Regenerate or rebuild the shared TS bindings if needed.
3. Update the browser stream client to recognize:
- `client_offer`
- `server_offer`
4. Keep `client_offer` as the default until later milestones are ready.

Files to change:

- `moonlight-web/common/src/api_bindings.rs`
- generated `moonlight-web/web-server/web/api_bindings.ts` if applicable
- `moonlight-web/web-server/web/stream/index.ts`

Exit condition:

- old behavior still works unchanged when role is `client_offer`

### Milestone 2: Make The Browser Peer Passive In Server-Offer Mode

1. Refactor `WebRTCTransport.initPeer()` so server-offer mode:
- creates the peer
- registers handlers
- does not immediately call `onNegotiationNeeded()`
2. Keep existing `handleRemoteDescription()` logic for incoming offers and answers.
3. Add server-offer compatible primary `ondatachannel` handling.
4. Preserve the existing client-created channel path for `client_offer` mode.

Files to change:

- `moonlight-web/web-server/web/stream/transport/webrtc.ts`
- `moonlight-web/web-server/web/stream/index.ts`
- `moonlight-web/web-server/web/stream/transport/index.ts` if channel abstraction needs updates

Exit condition:

- browser can idle cleanly in server-offer mode without sending an initial offer

### Milestone 3: Move Primary Data Channel Creation To The Streamer

1. Create the primary data channels from the streamer side.
2. Ensure browser-side transport can bind channels received via `ondatachannel`.
3. Keep hybrid input peer behavior unchanged.
4. Do not change media sequencing yet.

Files to change:

- `moonlight-web/streamer/src/transport/webrtc/mod.rs`
- `moonlight-web/web-server/web/stream/transport/webrtc.ts`

Exit condition:

- in server-offer mode, primary non-media channels open and work without the browser
  creating them first

### Milestone 4: Decouple StartStream From Primary Connected

1. Remove the current dependency on primary `RTCPeerConnectionState::Connected` as the
   trigger for `TransportEvent::StartStream`.
2. Start Moonlight after:
- `/host/stream` init succeeds
- Fuji launch/resume succeeds
- streamer is ready to process stream setup callbacks
3. Keep rollback behavior simple at first:
- if early startup fails, terminate the streamer cleanly
4. Do not emit `ConnectionComplete` as part of this step.

Files to change:

- `moonlight-web/streamer/src/transport/webrtc/mod.rs`
- `moonlight-web/streamer/src/main.rs`
- `moonlight-web/web-server/src/api/stream.rs`

Exit condition:

- actual `VideoSetup` and audio config become available before the first primary SDP offer

### Milestone 5: Create The Final Primary Peer Shape Before First Offer

1. Create primary audio/video tracks only after actual media setup is known.
2. Attach those tracks before the first offer is generated.
3. In server-offer mode:
- create primary data channels
- create real media tracks
- then create the first offer
4. Remove primary-path calls to `send_offer()` from audio/video setup when server-offer
   mode is active.
5. Keep legacy renegotiation path only under `client_offer` fallback mode.

Files to change:

- `moonlight-web/streamer/src/transport/webrtc/audio.rs`
- `moonlight-web/streamer/src/transport/webrtc/video.rs`
- `moonlight-web/streamer/src/transport/webrtc/sender.rs`
- `moonlight-web/streamer/src/transport/webrtc/mod.rs`

Exit condition:

- server-offer mode performs exactly one primary media negotiation

### Milestone 6: Tighten Ready/Complete Semantics

1. Change `ConnectionComplete` so it is sent only after:
- the final primary offer/answer is complete
- the browser should not expect another primary offer
2. Keep browser-side setup robust if media initialization fails after that point.
3. Do not let `ConnectionComplete` continue to mean "stream orchestration is done but
   media renegotiation is still pending".

Files to change:

- `moonlight-web/streamer/src/main.rs`
- `moonlight-web/web-server/web/stream/index.ts`

Exit condition:

- app/browser state reflects the final primary peer shape, not an intermediate state

### Milestone 7: Add Startup Failure Rollback

1. Add startup timeout tracking for the new earlier-start sequence.
2. Stop the stream if:
- browser never answers
- ICE never reaches connected in the startup window
- final primary negotiation never completes
3. Roll back Fuji-launched session state if startup fails.
4. Make failure reporting explicit so Fuji/web server do not remain in a false
   "streaming" state.

Files to change:

- `moonlight-web/streamer/src/main.rs`
- `moonlight-web/web-server/src/api/stream.rs`
- Fuji integration code as needed

Exit condition:

- failed early-start sessions do not leak running stream state

### Milestone 8: Remove Legacy Primary Renegotiation Path

1. After server-offer mode is proven stable, remove the legacy primary-path dependence on:
- browser-created initial primary offer
- late primary audio/video `send_offer()`
2. Keep hybrid input renegotiation logic only if still required for the separate input peer.
3. Reduce code paths so the primary peer has one production negotiation model.

Files to change:

- `moonlight-web/web-server/web/stream/transport/webrtc.ts`
- `moonlight-web/streamer/src/transport/webrtc/mod.rs`
- `moonlight-web/streamer/src/transport/webrtc/audio.rs`
- `moonlight-web/streamer/src/transport/webrtc/video.rs`

Exit condition:

- one primary-peer architecture remains in production code

### Milestone 9: Clean Up Client State Modeling

1. Update Backbone state semantics so "stream ready" eventually means:
- first media track
- or first media frame/bytes
2. Keep this as a follow-up, not the main blocker for the server refactor.

Files to change:

- `backbone/app/src/main/java/com/playbackbone/android/pcstreaming/ui/stream/webview/HybridPCStreamViewModel.kt`
- `backbone/app/src/main/java/com/playbackbone/android/pcstreaming/ui/stream/webview/HybridStreamJSInterface.kt`

Exit condition:

- app state more accurately matches user-visible stream readiness

### Validation Gates Between Milestones

1. After Milestone 2:
- browser in server-offer mode does not emit an initial primary offer

2. After Milestone 3:
- primary data channels work in server-offer mode

3. After Milestone 5:
- only one primary offer/answer occurs
- audio/video tracks arrive without a second primary offer

4. After Milestone 7:
- startup failures cleanly tear down host/session state

5. After Milestone 8:
- modern WebView no longer shows the old "second offer, tracks arrive, then disconnect"
  sequence on the primary peer
